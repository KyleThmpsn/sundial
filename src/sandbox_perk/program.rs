//! Authored action programs, independent of a stock perk's action graph.
//!
//! The compiler owns routing masks, condition ordinals and retained-state counts.
//! Native entities remain reusable building blocks, with edits kept on private clones.
use serde::{Deserialize, Serialize};

use crate::weapon_runtime::WeaponRuntimeValueOverride;

pub(crate) mod compiler;
pub use compiler::{Compiled, compile};
pub mod decompile;
pub mod properties;
pub use properties::{KeyCatalog, KeyEvidence};

/// The empty FNV-1 hash. A native key holds this value when nothing is named.
pub const EMPTY_KEY: u32 = 0x811C_9DC5;

/// The input selector value most stock Component Value Adjustment nodes store.
const fn no_input() -> u8 {
    0xFF
}

fn is_no_input(input: &u8) -> bool {
    *input == no_input()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// Active from the moment the perk is applied. An optional interval repeats it.
    Always,
    Equipped,
    #[default]
    Drawn,
    WeaponKill,
    PrecisionKill,
    MeleeKill,
    GrenadeKill,
    AnyKill,
    /// A native condition node carried verbatim in `Program::native_trigger`. The event it
    /// waits for is whatever the node's kind listens to.
    Native,
}

impl Trigger {
    pub const ALL: [Self; 9] = [
        Self::Always,
        Self::Equipped,
        Self::Drawn,
        Self::WeaponKill,
        Self::PrecisionKill,
        Self::MeleeKill,
        Self::GrenadeKill,
        Self::AnyKill,
        Self::Native,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Always => "Always Active",
            Self::Equipped => "While Equipped",
            Self::Drawn => "While Drawn",
            Self::WeaponKill => "On Weapon Kill",
            Self::PrecisionKill => "On Precision Weapon Kill",
            Self::MeleeKill => "On Melee Kill",
            Self::GrenadeKill => "On Grenade Kill",
            Self::AnyKill => "On Any Credited Kill",
            Self::Native => "On Native Condition",
        }
    }

    /// Whether the trigger is a kill event, which carries a chance, a label filter and the
    /// event position.
    #[must_use]
    pub const fn is_event(self) -> bool {
        !matches!(
            self,
            Self::Always | Self::Equipped | Self::Drawn | Self::Native
        )
    }

    /// Whether the trigger fires on an event and so ends on a timer: a kill or a native
    /// condition.
    #[must_use]
    pub const fn is_timed(self) -> bool {
        self.is_event() || matches!(self, Self::Native)
    }

    /// Whether the rearm list can carry a timer. Kill triggers use it as a cooldown and an
    /// always-active program uses it as a repeat interval.
    #[must_use]
    pub const fn supports_cooldown(self) -> bool {
        matches!(self, Self::Always) || self.is_timed()
    }

    /// Sentence-case description of the trigger for a reader.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Always => {
                "Actions start as soon as the perk is applied and stay until it is removed."
            }
            Self::Equipped => {
                "Actions start when equipped. Retained entities and pattern overrides are removed when unequipped."
            }
            Self::Drawn => {
                "Actions start when drawn. Retained entities and pattern overrides are removed when holstered."
            }
            Self::WeaponKill => "Actions start on a kill with this weapon.",
            Self::PrecisionKill => "Actions start on a precision kill with this weapon.",
            Self::MeleeKill => "Actions start on a melee kill.",
            Self::GrenadeKill => "Actions start on a grenade kill.",
            Self::AnyKill => "Actions start on any credited kill.",
            Self::Native => {
                "Actions start when the native condition passes. Its fields are carried as the client stores them."
            }
        }
    }
}

/// An authored native node and its complete relative-pointer allocation graph.
/// Existing scalar recipes retain their original representation. Complex records also
/// carry nested arrays, strings, filters, conditions and value programs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeNode {
    pub kind: u8,
    #[serde(with = "hex_bytes")]
    pub bytes: Vec<u8>,
}

impl NativeNode {
    /// An editable configuration of an observed native effect kind.
    #[must_use]
    pub fn effect(kind: u8) -> Option<Self> {
        Self::fresh(false, kind)
    }

    /// An editable configuration of an observed native condition kind.
    #[must_use]
    pub fn condition(kind: u8) -> Option<Self> {
        Self::fresh(true, kind)
    }

    fn fresh(condition: bool, kind: u8) -> Option<Self> {
        use crate::sandbox_perk::action::{layout, native};
        let mut bytes = if condition {
            layout::blank_condition(kind).or_else(|| native::template(true, kind))?
        } else {
            layout::blank_effect(kind).or_else(|| native::template(false, kind))?
        };
        // A kind with a plain title starts as the configuration that title describes.
        for (offset, value) in layout::stock_defaults(condition, kind) {
            if let Some(byte) = bytes.get_mut(*offset) {
                *byte = *value;
            }
        }
        Some(Self { kind, bytes })
    }

    fn check(&self, family: &str, size: Option<usize>, mapped: bool) -> Result<(), String> {
        if family == "Condition" || !mapped {
            let condition = family == "Condition";
            let entry = if condition {
                crate::sandbox_perk::nodes::condition(self.kind)
            } else {
                crate::sandbox_perk::nodes::effect(self.kind)
            }
            .filter(|entry| entry.observed())
            .ok_or_else(|| {
                format!(
                    "{family} kind {} has no recovered native layout.",
                    self.kind
                )
            })?;
            let graph =
                crate::sandbox_perk::action::native::Graph::read(&self.bytes, 0, entry.class)?;
            return graph.validate_node(condition, self.kind);
        }
        if Some(self.bytes.len()) != size {
            return Err(format!(
                "{family} kind {} needs {} bytes, not {}.",
                self.kind,
                size.unwrap_or(0),
                self.bytes.len()
            ));
        }
        use crate::sandbox_perk::action::layout;
        if self.bytes[0] != self.kind || self.bytes[1] > 1 {
            return Err(format!(
                "Effect kind {} has an invalid kind or retained-state header.",
                self.kind
            ));
        }
        let layout = layout::effect_layout(self.kind);
        if let Some(layout) = layout {
            for field in layout.fields {
                use crate::sandbox_perk::action::FactValue;
                let valid = match field.read(&self.bytes) {
                    Some(FactValue::Seconds(value)) => {
                        value.is_finite() && (0.0..=3600.0).contains(&value)
                    }
                    Some(FactValue::Number(value)) => value.is_finite(),
                    Some(FactValue::Range(low, high)) => low.is_finite() && high.is_finite(),
                    Some(FactValue::Flag(_)) => self.bytes[field.offset] <= 1,
                    Some(_) => true,
                    None => false,
                };
                if !valid {
                    return Err(format!(
                        "{family} kind {} has an invalid {} value.",
                        self.kind, field.label
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Node bytes written as one `0x` hexadecimal string, so a recipe reads like the native data.
mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let mut text = String::with_capacity(2 + bytes.len() * 2);
        text.push_str("0x");
        for byte in bytes {
            text.push_str(&format!("{byte:02X}"));
        }
        text.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        let digits = text
            .strip_prefix("0x")
            .or_else(|| text.strip_prefix("0X"))
            .ok_or_else(|| D::Error::custom("node bytes must start with 0x"))?;
        if digits.len() % 2 != 0 {
            return Err(D::Error::custom(
                "node bytes need an even number of hex digits",
            ));
        }
        if !digits.is_ascii() {
            return Err(D::Error::custom("node bytes are not hexadecimal"));
        }
        (0..digits.len())
            .step_by(2)
            .map(|at| {
                u8::from_str_radix(&digits[at..at + 2], 16)
                    .map_err(|_| D::Error::custom("node bytes are not hexadecimal"))
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    #[default]
    Owner,
    Event,
}

impl Position {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Owner => "Owner Position",
            Self::Event => "Event Position",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub graph: u32,
    /// Original native spelling, used for display and the compiled debug reference.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<WeaponRuntimeValueOverride>,
}

/// Which ammunition pool an ammunition action fills. The names come from how the stock
/// perks use the byte: Triple Tap returns rounds through path 1, and the ammo pickup perks
/// add to the ammo types through path 0.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmmunitionStore {
    #[default]
    Magazine,
    Reserves,
}

impl AmmunitionStore {
    pub const ALL: [Self; 2] = [Self::Magazine, Self::Reserves];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Magazine => "Magazine",
            Self::Reserves => "Reserves",
        }
    }

    /// The native selector byte.
    #[must_use]
    pub const fn byte(self) -> u8 {
        match self {
            Self::Reserves => 0,
            Self::Magazine => 1,
        }
    }

    #[must_use]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Reserves),
            1 => Some(Self::Magazine),
            _ => None,
        }
    }
}

/// Which of the seven amounts of an ammunition node carries the value: the owning weapon,
/// one of three weapon slots or one of three ammunition types. The client traces the slot
/// and type positions but does not name them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmmunitionTarget {
    #[default]
    OwningWeapon,
    Slot1,
    Slot2,
    Slot3,
    Category1,
    Category2,
    Category3,
}

impl AmmunitionTarget {
    pub const ALL: [Self; 7] = [
        Self::OwningWeapon,
        Self::Slot1,
        Self::Slot2,
        Self::Slot3,
        Self::Category1,
        Self::Category2,
        Self::Category3,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::OwningWeapon => "This Weapon",
            Self::Slot1 => "Weapon Slot 1",
            Self::Slot2 => "Weapon Slot 2",
            Self::Slot3 => "Weapon Slot 3",
            Self::Category1 => "Ammo Type 1",
            Self::Category2 => "Ammo Type 2",
            Self::Category3 => "Ammo Type 3",
        }
    }

    /// The amount's position among the seven the node stores.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::OwningWeapon => 0,
            Self::Slot1 => 1,
            Self::Slot2 => 2,
            Self::Slot3 => 3,
            Self::Category1 => 4,
            Self::Category2 => 5,
            Self::Category3 => 6,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Spawn {
        asset: Asset,
        #[serde(default)]
        position: Position,
    },
    Attach {
        asset: Asset,
        /// The attachment mode byte at `+0x02` of the Create Entity node. Stock actions store
        /// 0 through 3. Its role is not mapped, so the compiler writes it verbatim. The default
        /// is 1, which is what the compiler wrote before the field existed.
        #[serde(
            default = "default_attach_mode",
            skip_serializing_if = "is_default_attach_mode"
        )]
        mode: u8,
        /// The two keys at `+0x18` and `+0x1C`. Most stock nodes leave both at the empty hash,
        /// which is also what the compiler wrote before the field existed.
        #[serde(
            default = "empty_keys",
            skip_serializing_if = "is_empty_keys",
            with = "hex_keys"
        )]
        keys: [u32; 2],
        /// The four floats at `+0x20` through `+0x2C`, kept as bit patterns so recipe equality
        /// stays exact. Stock nodes store zero or `1.0`. The default is zero, which is what the
        /// compiler wrote before the field existed.
        #[serde(default, skip_serializing_if = "is_zero_bits", with = "float_bits")]
        float_bits: [u32; 4],
    },
    Pattern {
        asset: Asset,
    },
    /// Extends the timers that are already running when the trigger fires again while the
    /// effect is active. The compiler re-emits the trigger as the nested condition, which is
    /// the shape stock kill perks such as Outlaw use.
    ExtendTimers {
        /// Seconds added to each running timer, in milliseconds.
        extend_ms: u32,
        /// The most a timer can hold after the extension, in milliseconds.
        cap_ms: u32,
    },
    /// Sets a named property to a constant value while the effect is active.
    ///
    /// Every field mirrors a byte of the native Named Property node. The key and selector
    /// meanings are not mapped, so the workbench shows them as technical controls and the
    /// compiler writes them verbatim with the constant value program stock nodes use.
    Property {
        /// The property key at `+0x08`, a 32-bit hash.
        #[serde(with = "hex_key")]
        key: u32,
        /// The target selector byte at `+0x02`. Stock nodes store 0 through 3.
        #[serde(default)]
        target: u8,
        /// The operation byte at `+0x49`. Stock nodes store 0, 1 and 3.
        #[serde(default)]
        operation_byte: u8,
        /// The removal policy byte at `+0x4A`. Stock nodes store 0, 1 and 2.
        #[serde(default)]
        removal: u8,
        /// The constant value the program pushes, kept as a bit pattern.
        #[serde(with = "float_bit")]
        value_bits: u32,
        /// The removal value at `+0x4C`, kept as a bit pattern. Stock nodes store 0 or 1.
        #[serde(default, skip_serializing_if = "is_zero", with = "float_bit")]
        restore_bits: u32,
        /// The ability slot mask at `+0x04`. Stock nodes leave it zero in 190 of 203 cases.
        #[serde(default, skip_serializing_if = "is_zero", with = "hex_key")]
        ability_mask: u32,
        /// The input selector byte at `+0x48`. Stock nodes store 0 in all but four cases.
        #[serde(default, skip_serializing_if = "is_zero_byte")]
        input: u8,
        /// The byte at `+0x03`. Stock nodes store 1 in all but one case.
        #[serde(default = "one", skip_serializing_if = "is_one")]
        flag: u8,
    },
    /// Scales an ability's energy component, the way stock perks grant grenade, melee, super
    /// or class ability energy.
    ///
    /// Every field mirrors a byte of the native Component Value Adjustment node. The target
    /// selector names the ability (see `action::component_target`). The flag, option and
    /// input bytes are a separate axis whose role is not mapped, so the workbench shows them
    /// as technical controls and the compiler writes them verbatim.
    AdjustComponent {
        /// The ability selector byte at `+0x02`. Stock nodes store 0, 1, 2 and 7.
        target: u8,
        /// The flag byte at `+0x03`.
        #[serde(default, skip_serializing_if = "is_zero_byte")]
        flag: u8,
        /// The option byte at `+0x04`.
        #[serde(default, skip_serializing_if = "is_zero_byte")]
        option: u8,
        /// The scale at `+0x08`, kept as a bit pattern so recipe equality stays exact.
        #[serde(with = "float_bit")]
        scale_bits: u32,
        /// The limit at `+0x0C`, kept as a bit pattern. Zero means no limit is applied.
        #[serde(default, skip_serializing_if = "is_zero", with = "float_bit")]
        limit_bits: u32,
        /// The constant the value program pushes, kept as a bit pattern.
        #[serde(with = "float_bit")]
        value_bits: u32,
        /// The input selector byte at `+0x48`. Stock nodes store 0xFF in 106 of 179 cases.
        #[serde(default = "no_input", skip_serializing_if = "is_no_input")]
        input: u8,
    },
    /// Changes a named property inside one ability's bank, the way stock exotics grant an
    /// extra grenade charge or improve a jump.
    ///
    /// The ability follows the target selector (see `action::ability_slot`). The property key
    /// and the option byte that selects which property of that ability changes are carried
    /// verbatim, since their meanings are not resolved.
    AbilityProperty {
        /// The ability selector byte at `+0x02`. Stock nodes store 0, 1, 2, 3, 4 and 7.
        target: u8,
        /// The property key at `+0x04`, a 32-bit hash.
        #[serde(with = "hex_key")]
        key: u32,
        /// The property index at `+0x08`. Stock nodes store 26 distinct values.
        #[serde(default, skip_serializing_if = "is_zero_byte")]
        option: u8,
    },
    /// Sets the transmat effect a weapon plays. The key names the effect and its consumer is
    /// not resolved, so the key is carried verbatim.
    TransmatContext {
        /// The effect key at `+0x04`, a 32-bit hash.
        #[serde(with = "hex_key")]
        key: u32,
    },
    /// Replaces a key on the host while the effect is active, the way stock perks swap a
    /// weapon firing mode.
    OverrideHostKey {
        /// The target selector byte at `+0x02`.
        #[serde(default, skip_serializing_if = "is_zero_byte")]
        target: u8,
        /// The interface selector byte at `+0x03`.
        #[serde(default, skip_serializing_if = "is_zero_byte")]
        interface: u8,
        /// The key written in place of the host's own, at `+0x04`.
        #[serde(with = "hex_key")]
        key: u32,
        /// Whether the replacement applies to the player rather than the weapon, at `+0x08`.
        #[serde(default, skip_serializing_if = "is_false")]
        apply_to_player: bool,
    },
    /// Changes the weapon's damage type, which is what The Fundamentals and the element mods
    /// do. The mode is the element: 0 Kinetic, 1 Solar, 2 Arc, 3 Void.
    SetDamageType {
        /// The damage type byte at `+0x02`.
        mode: u8,
        /// Whether the change survives the effect ending, at `+0x03`.
        #[serde(default, skip_serializing_if = "is_false")]
        keep_after_removal: bool,
    },
    /// Counts a weapon reference while the effect is active. The operation byte selects what
    /// the count does, and its values are not resolved.
    WeaponReferenceCount {
        /// The operation byte at `+0x02`. Named `selector` because the enum is serde-tagged
        /// on `operation`.
        #[serde(default, skip_serializing_if = "is_zero_byte")]
        selector: u8,
    },
    /// Writes the program's accumulator, the value an Accumulator condition counts toward
    /// its threshold.
    UpdateAccumulator {
        /// The mode byte at `+0x02`, which selects the supplied value. Every stock node
        /// stores 1.
        #[serde(default = "one", skip_serializing_if = "is_one")]
        mode: u8,
        /// The value at `+0x04`, kept as a bit pattern.
        #[serde(with = "float_bit")]
        value_bits: u32,
    },
    /// Adds a whole number of rounds when the effect starts. The native Fixed Ammunition
    /// Adjustment node, kind 14, the one Triple Tap returns its round through.
    AddRounds {
        /// Signed rounds. Stock nodes store 1 through 15, and one stores -1.
        rounds: i32,
        #[serde(default)]
        target: AmmunitionTarget,
        #[serde(default)]
        store: AmmunitionStore,
        /// The overflow flag at `+0x69`, which lets the magazine exceed its capacity.
        #[serde(default, skip_serializing_if = "is_false")]
        overflow: bool,
        /// The flag at `+0x6A`. The ammo pickup perks set it on their ammo type amounts.
        #[serde(default, skip_serializing_if = "is_false")]
        unit_scaled: bool,
        /// The flag at `+0x6B`. Four stock nodes set it, all inside predicate-driven perks.
        #[serde(default, skip_serializing_if = "is_false")]
        action_scaled: bool,
    },
    /// Adds a fraction of a capacity when the effect starts. The native Proportional
    /// Ammunition Adjustment node, kind 15, the one kill-to-reload perks use.
    AddFraction {
        /// The fraction of the capacity, kept as a bit pattern. 0.5 is half a magazine.
        #[serde(with = "float_bit")]
        fraction_bits: u32,
        #[serde(default)]
        target: AmmunitionTarget,
        #[serde(default)]
        store: AmmunitionStore,
        /// Which capacity the fraction scales. Stock nodes pair it with the store.
        #[serde(default)]
        capacity: AmmunitionStore,
        /// The overflow flag at `+0x69`, which lets the magazine exceed its capacity.
        #[serde(default, skip_serializing_if = "is_false")]
        overflow: bool,
        /// The flag at `+0x6B`. Three stock nodes set it, all inside predicate-driven perks.
        #[serde(default, skip_serializing_if = "is_false")]
        action_scaled: bool,
    },
    /// A plain scalar effect node carried verbatim. Every kind in `action::layout` fits:
    /// the node holds fixed-width fields only, so the compiler writes it byte for byte and
    /// the workbench edits the mapped fields in place.
    Native {
        #[serde(flatten)]
        node: NativeNode,
    },
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

const fn one() -> u8 {
    1
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_one(value: &u8) -> bool {
    *value == 1
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero_byte(value: &u8) -> bool {
    *value == 0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// A single key written as a `0x` hexadecimal string.
mod hex_key {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    pub fn serialize<S: Serializer>(key: &u32, serializer: S) -> Result<S::Ok, S::Error> {
        format!("0x{key:08X}").serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
        let text = String::deserialize(deserializer)?;
        let digits = text
            .strip_prefix("0x")
            .or_else(|| text.strip_prefix("0X"))
            .ok_or_else(|| D::Error::custom(format!("key {text} must start with 0x")))?;
        u32::from_str_radix(digits, 16)
            .map_err(|_| D::Error::custom(format!("key {text} is not a 32-bit hash")))
    }
}

/// A single float written as a number and stored as its exact bit pattern.
mod float_bit {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(bits: &u32, serializer: S) -> Result<S::Ok, S::Error> {
        f32::from_bits(*bits).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
        f32::deserialize(deserializer).map(f32::to_bits)
    }
}

const fn default_attach_mode() -> u8 {
    1
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_attach_mode(mode: &u8) -> bool {
    *mode == default_attach_mode()
}

const fn empty_keys() -> [u32; 2] {
    [EMPTY_KEY; 2]
}

fn is_empty_keys(keys: &[u32; 2]) -> bool {
    *keys == empty_keys()
}

fn is_zero_bits(bits: &[u32; 4]) -> bool {
    *bits == [0; 4]
}

/// Keys are written as `0x` hexadecimal strings so a recipe reads like the native data.
mod hex_keys {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    pub fn serialize<S: Serializer>(keys: &[u32; 2], serializer: S) -> Result<S::Ok, S::Error> {
        keys.map(|key| format!("0x{key:08X}")).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u32; 2], D::Error> {
        let text = <[String; 2]>::deserialize(deserializer)?;
        let mut keys = [0; 2];
        for (key, text) in keys.iter_mut().zip(&text) {
            let digits = text
                .strip_prefix("0x")
                .or_else(|| text.strip_prefix("0X"))
                .ok_or_else(|| D::Error::custom(format!("key {text} must start with 0x")))?;
            *key = u32::from_str_radix(digits, 16)
                .map_err(|_| D::Error::custom(format!("key {text} is not a 32-bit hash")))?;
        }
        Ok(keys)
    }
}

/// Floats are written as numbers and stored as their exact bit patterns.
mod float_bits {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(bits: &[u32; 4], serializer: S) -> Result<S::Ok, S::Error> {
        bits.map(f32::from_bits).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u32; 4], D::Error> {
        <[f32; 4]>::deserialize(deserializer).map(|floats| floats.map(f32::to_bits))
    }
}

impl Action {
    /// An attach action with the native fields at the values the compiler always wrote.
    #[must_use]
    pub const fn attach(asset: Asset) -> Self {
        Self::Attach {
            asset,
            mode: default_attach_mode(),
            keys: empty_keys(),
            float_bits: [0; 4],
        }
    }

    /// A named property action shaped like the stock nodes that set `evidence`'s key: the
    /// bytes and the value they agree on, and the common defaults where they differ.
    #[must_use]
    pub fn property_from(evidence: &KeyEvidence) -> Self {
        let mut action = Self::property(evidence.hash());
        if let Self::Property {
            target,
            operation_byte,
            removal,
            value_bits,
            ..
        } = &mut action
        {
            if let Some(byte) = KeyEvidence::single(&evidence.targets) {
                *target = byte;
            }
            if let Some(byte) = KeyEvidence::single(&evidence.operations) {
                *operation_byte = byte;
            }
            if let Some(byte) = KeyEvidence::single(&evidence.removals) {
                *removal = byte;
            }
            if let [value] = evidence.values.as_slice() {
                *value_bits = value.to_bits();
            }
        }
        action
    }

    /// A named property action with the bytes stock nodes use most: target 0, operation 0,
    /// removal policy 1 and a constant value of 1.0.
    #[must_use]
    pub const fn property(key: u32) -> Self {
        Self::Property {
            key,
            target: 0,
            operation_byte: 0,
            removal: 1,
            value_bits: 0x3F80_0000,
            restore_bits: 0,
            ability_mask: 0,
            input: 0,
            flag: 1,
        }
    }

    /// An add-rounds action shaped like Triple Tap's: one round into this weapon's magazine.
    /// An adjustment of the given ability's energy, with the scale and value stock energy
    /// perks use most.
    #[must_use]
    pub const fn adjust_component(target: u8) -> Self {
        Self::AdjustComponent {
            target,
            flag: 0,
            option: 0,
            scale_bits: 0x3F80_0000,
            limit_bits: 0,
            value_bits: 0x3F80_0000,
            input: no_input(),
        }
    }

    #[must_use]
    pub const fn transmat_context(key: u32) -> Self {
        Self::TransmatContext { key }
    }

    #[must_use]
    pub const fn override_host_key(key: u32) -> Self {
        Self::OverrideHostKey {
            target: 0,
            interface: 0,
            key,
            apply_to_player: false,
        }
    }

    #[must_use]
    pub const fn set_damage_type(mode: u8) -> Self {
        Self::SetDamageType {
            mode,
            keep_after_removal: false,
        }
    }

    #[must_use]
    pub const fn weapon_reference_count(selector: u8) -> Self {
        Self::WeaponReferenceCount { selector }
    }

    /// A property change on the given ability, starting from the grenade bank.
    #[must_use]
    pub const fn ability_property(target: u8) -> Self {
        Self::AbilityProperty {
            target,
            key: EMPTY_KEY,
            option: 0,
        }
    }

    #[must_use]
    pub const fn update_accumulator(value: f32) -> Self {
        Self::UpdateAccumulator {
            mode: 1,
            value_bits: value.to_bits(),
        }
    }

    #[must_use]
    pub const fn add_rounds(rounds: i32) -> Self {
        Self::AddRounds {
            rounds,
            target: AmmunitionTarget::OwningWeapon,
            store: AmmunitionStore::Magazine,
            overflow: false,
            unit_scaled: false,
            action_scaled: false,
        }
    }

    /// An add-fraction action shaped like the kill-to-reload perks: a share of the magazine
    /// capacity into this weapon's magazine.
    #[must_use]
    pub const fn add_fraction(fraction: f32) -> Self {
        Self::AddFraction {
            fraction_bits: fraction.to_bits(),
            target: AmmunitionTarget::OwningWeapon,
            store: AmmunitionStore::Magazine,
            capacity: AmmunitionStore::Magazine,
            overflow: false,
            action_scaled: false,
        }
    }

    /// A verbatim native effect node of `kind`, when the kind is a plain scalar node.
    #[must_use]
    pub fn native(kind: u8) -> Option<Self> {
        Some(Self::Native {
            node: NativeNode::effect(kind)?,
        })
    }

    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Spawn { .. } => "Spawn Entity",
            Self::Attach { .. } => "Attach Entity",
            Self::Pattern { .. } => "Override Weapon Pattern",
            Self::ExtendTimers { .. } => "Extend Timers",
            Self::Property { .. } => "Named Property",
            Self::AdjustComponent { target, .. } => {
                match crate::sandbox_perk::action::component_target(*target, 0, 0) {
                    Some("Grenade Energy") => "Adjust Grenade Energy",
                    Some("Super Energy") => "Adjust Super Energy",
                    Some("Melee Energy") => "Adjust Melee Energy",
                    Some("Class Ability Energy") => "Adjust Class Ability Energy",
                    _ => "Adjust Component Value",
                }
            }
            Self::AbilityProperty { target, .. } => {
                match crate::sandbox_perk::action::ability_slot(*target) {
                    Some("Grenade") => "Change a Grenade Property",
                    Some("Super") => "Change a Super Property",
                    Some("Melee") => "Change a Melee Property",
                    Some("Jump") => "Change a Jump Property",
                    Some("Class Ability") => "Change a Class Ability Property",
                    _ => "Change an Ability Property",
                }
            }
            Self::TransmatContext { .. } => "Set Transmat Effect",
            Self::OverrideHostKey { .. } => "Override a Host Key",
            Self::SetDamageType { mode, .. } => match mode {
                0 => "Change Damage Type to Kinetic",
                1 => "Change Damage Type to Solar",
                2 => "Change Damage Type to Arc",
                3 => "Change Damage Type to Void",
                _ => "Change Damage Type",
            },
            Self::WeaponReferenceCount { .. } => "Count a Weapon Reference",
            Self::UpdateAccumulator { .. } => "Update Accumulator",
            Self::AddRounds { .. } => "Add Rounds",
            Self::AddFraction { .. } => "Add Ammunition Fraction",
            Self::Native { node } => crate::sandbox_perk::nodes::effect(node.kind)
                .map_or("Native Effect", |kind| kind.name),
        }
    }

    /// The entity graph or pattern this action references, when it references one.
    #[must_use]
    pub const fn asset(&self) -> Option<&Asset> {
        match self {
            Self::Spawn { asset, .. } | Self::Attach { asset, .. } | Self::Pattern { asset } => {
                Some(asset)
            }
            Self::ExtendTimers { .. }
            | Self::Property { .. }
            | Self::AdjustComponent { .. }
            | Self::UpdateAccumulator { .. }
            | Self::AbilityProperty { .. }
            | Self::TransmatContext { .. }
            | Self::OverrideHostKey { .. }
            | Self::SetDamageType { .. }
            | Self::WeaponReferenceCount { .. }
            | Self::AddRounds { .. }
            | Self::AddFraction { .. }
            | Self::Native { .. } => None,
        }
    }

    pub const fn asset_mut(&mut self) -> Option<&mut Asset> {
        match self {
            Self::Spawn { asset, .. } | Self::Attach { asset, .. } | Self::Pattern { asset } => {
                Some(asset)
            }
            Self::ExtendTimers { .. }
            | Self::Property { .. }
            | Self::AdjustComponent { .. }
            | Self::UpdateAccumulator { .. }
            | Self::AbilityProperty { .. }
            | Self::TransmatContext { .. }
            | Self::OverrideHostKey { .. }
            | Self::SetDamageType { .. }
            | Self::WeaponReferenceCount { .. }
            | Self::AddRounds { .. }
            | Self::AddFraction { .. }
            | Self::Native { .. } => None,
        }
    }

    /// Whether the effect keeps retained state the action must release on removal. An
    /// ammunition add happens once and leaves nothing to undo. A native node says so in its
    /// second byte, which is carried from the stock node it came from.
    #[must_use]
    pub fn retained(&self) -> bool {
        match self {
            // Every stock Component Value Adjustment, Accumulator Update and Transmat Context
            // node clears the retained byte: 179, 147 and 104 of them respectively, with no
            // exception in the captured survey.
            Self::Spawn { .. }
            | Self::ExtendTimers { .. }
            | Self::AddRounds { .. }
            | Self::AddFraction { .. }
            | Self::AdjustComponent { .. }
            | Self::UpdateAccumulator { .. }
            | Self::TransmatContext { .. } => false,
            // Every stock Ability Property node sets the retained byte, as Named Property does.
            // The three kinds below keep it set too, since each holds state until the effect ends.
            Self::AbilityProperty { .. }
            | Self::OverrideHostKey { .. }
            | Self::SetDamageType { .. }
            | Self::WeaponReferenceCount { .. } => true,
            Self::Native { node } => node.bytes.get(1).is_some_and(|byte| *byte != 0),
            _ => true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    pub name: String,
    pub trigger: Trigger,
    pub duration_ms: u32,
    pub cooldown_ms: u32,
    /// Probability in hundredths of a percent. Integers keep recipe equality stable.
    pub chance_permyriad: u16,
    pub actions: Vec<Action>,
    /// For an always-active program, the Event Key Match key that ends it. `None` keeps the
    /// actions until the perk is removed. Stock always-active perks that end early all use
    /// this one condition kind, with a key whose event is not named.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "hex_key_option"
    )]
    pub removal_key: Option<u32>,
    /// The activation node of a `Trigger::Native` program, carried verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_trigger: Option<NativeNode>,
    /// A verbatim ending condition, replacing the trigger's default ending condition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_removal: Option<NativeNode>,
    /// Complete native form for programs with arbitrary groups, policies and conditions.
    /// When present, this owns the behavior and the convenience controls must be defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<NativeProgram>,
    /// Root records outside the program, preserved verbatim from a stock action.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auxiliary: Vec<NativeRecord>,
    /// The execution policy preserved from a stock action. `None` is the default policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<Policy>,
    /// Further activation conditions beside the trigger, carried verbatim. The engine starts
    /// the effect when any condition in the list passes, so these widen the trigger.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternative_triggers: Vec<NativeNode>,
    /// Further ending conditions beside the primary one, carried verbatim. Any one ends the
    /// effect.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternative_removals: Vec<NativeNode>,
    /// A verbatim rearm condition, replacing the cooldown timer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_rearm: Option<NativeNode>,
    /// Further rearm conditions beside the primary one, carried verbatim.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternative_rearms: Vec<NativeNode>,
    /// Further programs a stock action runs beside this one, carried verbatim.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_groups: Vec<NativeGroup>,
}

/// One further program of a stock action: its four condition lists and its effects, each
/// node carried verbatim in native list order. Only the primary program has typed controls.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeGroup {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activation: Vec<NativeNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<NativeNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removal: Vec<NativeNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rearm: Vec<NativeNode>,
}

impl NativeGroup {
    /// Every condition of the group, then every effect, for checks and reservations.
    pub fn conditions(&self) -> impl Iterator<Item = &NativeNode> {
        self.activation
            .iter()
            .chain(&self.removal)
            .chain(&self.rearm)
    }

    /// Every node of the group, conditions flagged `true` and effects `false`.
    pub fn nodes(&self) -> impl Iterator<Item = (bool, &NativeNode)> {
        self.conditions()
            .map(|node| (true, node))
            .chain(self.effects.iter().map(|node| (false, node)))
    }

    /// Every node of the group for editing, conditions flagged `true` and effects `false`.
    pub fn nodes_mut(&mut self) -> impl Iterator<Item = (bool, &mut NativeNode)> {
        self.activation
            .iter_mut()
            .chain(&mut self.removal)
            .chain(&mut self.rearm)
            .map(|node| (true, node))
            .chain(self.effects.iter_mut().map(|node| (false, node)))
    }
}

mod native;
pub use native::NativeProgram;

/// One closed native record outside the program, carried by class and the bytes of its
/// allocation graph.
///
/// Stock records hold keys, resource tags, scalars and at most a string, with no link into
/// the program, so preserving them is a copy. Their meaning is not resolved and they have no
/// controls.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRecord {
    #[serde(with = "hex_key")]
    pub class: u32,
    #[serde(with = "hex_bytes")]
    pub bytes: Vec<u8>,
}

impl NativeRecord {
    /// The bytes must read as one closed allocation graph of the declared class, with no
    /// bytes left over.
    pub fn check(&self) -> Result<(), String> {
        let graph = crate::sandbox_perk::action::native::Graph::read(&self.bytes, 0, self.class)?;
        let needed = graph.emit()?.len();
        if needed != self.bytes.len() {
            return Err(format!(
                "Native record 0x{:08X} holds {} bytes where its allocation graph needs {needed}.",
                self.class,
                self.bytes.len()
            ));
        }
        Ok(())
    }
}

impl From<&crate::sandbox_perk::action::DecodedRecord> for NativeRecord {
    fn from(record: &crate::sandbox_perk::action::DecodedRecord) -> Self {
        Self {
            class: record.class,
            bytes: record.bytes.clone(),
        }
    }
}

/// The execution policy a stock action selects at its root, preserved verbatim.
///
/// Stock perks use selectors 1, 3 and 5, each with its own configuration record class. The
/// policies' behavior is not resolved, so a program carries the selection without controls.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub selector: u8,
    /// Root byte +0xB9. Set in two stock actions, role unresolved.
    #[serde(default)]
    pub modifier: u8,
    /// Root key at +0x80. The empty key in all but one stock action, role unresolved.
    #[serde(with = "hex_key")]
    pub key: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<NativeRecord>,
}

/// An optional key written as a `0x` hexadecimal string.
mod hex_key_option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(key: &Option<u32>, serializer: S) -> Result<S::Ok, S::Error> {
        key.map(|key| format!("0x{key:08X}")).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<u32>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|text| {
                super::hex_key::deserialize(serde::de::value::StringDeserializer::<D::Error>::new(
                    text,
                ))
            })
            .transpose()
    }
}

impl Default for Program {
    fn default() -> Self {
        Self {
            name: "Custom Effect".into(),
            trigger: Trigger::Drawn,
            duration_ms: 1000,
            cooldown_ms: 0,
            chance_permyriad: 10_000,
            actions: Vec::new(),
            removal_key: None,
            native_trigger: None,
            native_removal: None,
            native: None,
            auxiliary: Vec::new(),
            policy: None,
            alternative_triggers: Vec::new(),
            alternative_removals: Vec::new(),
            native_rearm: None,
            alternative_rearms: Vec::new(),
            additional_groups: Vec::new(),
        }
    }
}

impl Program {
    /// Every native node the compiler carries verbatim, conditions flagged `true` and
    /// effects `false`: the trigger and ending, every alternative and rearm condition, the
    /// native actions, and the conditions and effects of every further group. Any pass that
    /// must reach every native node, such as compiling label masks against the current
    /// registry or proving referenced resources are live, walks this list so a new position
    /// cannot be left out.
    pub fn native_nodes(&self) -> impl Iterator<Item = (bool, &NativeNode)> {
        self.native_trigger
            .iter()
            .chain(&self.alternative_triggers)
            .chain(&self.native_removal)
            .chain(&self.alternative_removals)
            .chain(&self.native_rearm)
            .chain(&self.alternative_rearms)
            .map(|node| (true, node))
            .chain(self.actions.iter().filter_map(|action| match action {
                Action::Native { node } => Some((false, node)),
                _ => None,
            }))
            .chain(self.additional_groups.iter().flat_map(NativeGroup::nodes))
    }

    /// The same nodes as [`Self::native_nodes`], for passes that rewrite them.
    pub fn native_nodes_mut(&mut self) -> impl Iterator<Item = (bool, &mut NativeNode)> {
        self.native_trigger
            .iter_mut()
            .chain(&mut self.alternative_triggers)
            .chain(&mut self.native_removal)
            .chain(&mut self.alternative_removals)
            .chain(&mut self.native_rearm)
            .chain(&mut self.alternative_rearms)
            .map(|node| (true, node))
            .chain(self.actions.iter_mut().filter_map(|action| match action {
                Action::Native { node } => Some((false, node)),
                _ => None,
            }))
            .chain(
                self.additional_groups
                    .iter_mut()
                    .flat_map(NativeGroup::nodes_mut),
            )
    }

    /// Kill-event capabilities come from the native event kind, regardless of
    /// whether the condition was authored from defaults or copied from a perk.
    pub fn has_kill_trigger(&self) -> bool {
        self.trigger.is_event()
            || self.trigger == Trigger::Native
                && self
                    .native_trigger
                    .as_ref()
                    .is_some_and(|node| node.kind == 2)
    }

    /// Drafts can be empty. Build readiness is checked separately.
    pub fn validate_structure(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.contains('\0') {
            return Err("Enter a name for the custom effect.".into());
        }
        if let Some(native) = &self.native {
            let defaults = Self::default();
            if !self.actions.is_empty()
                || self.native_trigger.is_some()
                || self.native_removal.is_some()
                || self.removal_key.is_some()
                || !self.auxiliary.is_empty()
                || self.policy.is_some()
                || !self.alternative_triggers.is_empty()
                || !self.alternative_removals.is_empty()
                || self.native_rearm.is_some()
                || !self.alternative_rearms.is_empty()
                || !self.additional_groups.is_empty()
                || self.trigger != defaults.trigger
                || self.duration_ms != defaults.duration_ms
                || self.cooldown_ms != defaults.cooldown_ms
                || self.chance_permyriad != defaults.chance_permyriad
            {
                return Err(
                    "A complete native program cannot also contain simplified behavior settings."
                        .into(),
                );
            }
            return native.validate();
        }
        if self.actions.len() > 16 {
            return Err("A custom effect can contain up to 16 actions.".into());
        }
        for record in self.auxiliary.iter().chain(
            self.policy
                .iter()
                .filter_map(|policy| policy.configuration.as_ref()),
        ) {
            record.check()?;
        }
        match self.removal_key {
            Some(_) if self.trigger != Trigger::Always => {
                return Err("An ending event key applies to an always-active effect only.".into());
            }
            Some(key) if key == 0 || key == EMPTY_KEY => {
                return Err("Choose an ending event key or clear it.".into());
            }
            _ => {}
        }
        use crate::sandbox_perk::action::layout;
        match (&self.native_trigger, self.trigger) {
            (Some(node), Trigger::Native) => node.check(
                "Condition",
                layout::condition_size(node.kind),
                layout::condition_layout(node.kind).is_some(),
            )?,
            (Some(_), _) => {
                return Err("A native trigger node needs the On Native Condition trigger.".into());
            }
            (None, Trigger::Native) => {
                return Err("Choose a native condition for the trigger.".into());
            }
            (None, _) => {}
        }
        if let Some(node) = &self.native_removal {
            if self.removal_key.is_some() {
                return Err(
                    "Use an ending event key or a native ending condition, not both.".into(),
                );
            }
            node.check(
                "Condition",
                layout::condition_size(node.kind),
                layout::condition_layout(node.kind).is_some(),
            )?;
        }
        if self.native_rearm.is_some() && self.cooldown_ms != 0 {
            return Err("Use a cooldown or a native rearm condition, not both.".into());
        }
        for node in self
            .alternative_triggers
            .iter()
            .chain(&self.alternative_removals)
            .chain(&self.native_rearm)
            .chain(&self.alternative_rearms)
            .chain(
                self.additional_groups
                    .iter()
                    .flat_map(NativeGroup::conditions),
            )
        {
            node.check(
                "Condition",
                layout::condition_size(node.kind),
                layout::condition_layout(node.kind).is_some(),
            )?;
        }
        for node in self
            .additional_groups
            .iter()
            .flat_map(|group| &group.effects)
        {
            node.check(
                "Effect",
                layout::effect_size(node.kind),
                layout::effect_layout(node.kind).is_some(),
            )?;
        }
        if self.chance_permyriad > 10_000
            || self.duration_ms > 3_600_000
            || self.cooldown_ms > 3_600_000
        {
            return Err("Use a chance from 0 to 100 percent and times up to one hour.".into());
        }
        if self
            .actions
            .iter()
            .filter(|action| matches!(action, Action::Pattern { .. }))
            .count()
            > 1
        {
            return Err("An effect can select one weapon pattern at a time.".into());
        }
        for action in &self.actions {
            if let Some(asset) = action.asset()
                && (asset.path.contains('\0') || asset.path.len() > 1024)
            {
                return Err("The selected asset has an invalid native path.".into());
            }
            match action {
                Action::Attach { float_bits, .. }
                    if float_bits
                        .iter()
                        .any(|bits| !f32::from_bits(*bits).is_finite()) =>
                {
                    return Err("Attach Entity technical values must be finite numbers.".into());
                }
                Action::ExtendTimers { extend_ms, cap_ms } => {
                    if *extend_ms == 0 || *cap_ms < *extend_ms || *cap_ms > 3_600_000 {
                        return Err(
                            "Extend Timers needs an extension above zero and a cap at least as long, up to one hour."
                                .into(),
                        );
                    }
                }
                Action::AdjustComponent {
                    scale_bits,
                    limit_bits,
                    value_bits,
                    ..
                } => {
                    if [*scale_bits, *limit_bits, *value_bits]
                        .iter()
                        .any(|bits| !f32::from_bits(*bits).is_finite())
                    {
                        return Err("Component adjustments need finite numbers.".into());
                    }
                }
                Action::UpdateAccumulator { value_bits, .. } => {
                    if !f32::from_bits(*value_bits).is_finite() {
                        return Err("The accumulator value needs a finite number.".into());
                    }
                }
                Action::AbilityProperty { .. } => {}
                // These four write every byte they carry verbatim, so any stock value round
                // trips. Refusing an odd one here would stop a stock perk opening, which is
                // worse than letting the workbench guide the author toward a sensible one.
                Action::TransmatContext { .. }
                | Action::OverrideHostKey { .. }
                | Action::SetDamageType { .. }
                | Action::WeaponReferenceCount { .. } => {}
                Action::Property {
                    key,
                    value_bits,
                    restore_bits,
                    ..
                } => {
                    if *key == 0 || *key == EMPTY_KEY {
                        return Err("Named Property needs a property key.".into());
                    }
                    if !f32::from_bits(*value_bits).is_finite()
                        || !f32::from_bits(*restore_bits).is_finite()
                    {
                        return Err("Named Property values must be finite numbers.".into());
                    }
                }
                Action::AddRounds { rounds, .. } => {
                    if *rounds == 0 || rounds.unsigned_abs() > 999 {
                        return Err(
                            "Add Rounds needs a round count from -999 to 999, not zero.".into()
                        );
                    }
                }
                Action::AddFraction { fraction_bits, .. } => {
                    let fraction = f32::from_bits(*fraction_bits);
                    if !fraction.is_finite() || fraction == 0.0 || fraction.abs() > 100.0 {
                        return Err(
                            "Add Ammunition Fraction needs a fraction from -100 to 100 capacities, not zero."
                                .into(),
                        );
                    }
                }
                Action::Native { node } => {
                    node.check(
                        "Effect",
                        layout::effect_size(node.kind),
                        layout::effect_layout(node.kind).is_some(),
                    )?;
                    if node.bytes.first() != Some(&node.kind) {
                        return Err(format!(
                            "Effect kind {} node bytes start with a different kind.",
                            node.kind
                        ));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_structure()?;
        if let Some(native) = &self.native {
            crate::sandbox_perk::action::decode(&native.graph.emit()?)?;
            return Ok(());
        }
        for action in &self.actions {
            if let Some(asset) = action.asset()
                && (asset.graph == 0 || asset.graph == u32::MAX)
            {
                return Err("Choose an asset for every action.".into());
            }
            if matches!(action, Action::ExtendTimers { .. }) && !self.has_kill_trigger() {
                return Err("Extend Timers requires a kill trigger.".into());
            }
        }
        Ok(())
    }

    /// Advice before building. Stock perks use each of these shapes, so they compile, but a
    /// hand-authored program rarely intends them.
    pub fn authoring_hint(&self) -> Option<&'static str> {
        if self.native.is_some() {
            return None;
        }
        if self.actions.is_empty() && self.additional_groups.is_empty() {
            return Some("Add an action to the custom effect.");
        }
        let never_ends = self.duration_ms == 0 && self.native_removal.is_none();
        if self.has_kill_trigger() && never_ends {
            return Some(
                "An event effect without a duration or ending condition stays active until the perk is removed.",
            );
        }
        if self.trigger == Trigger::Native
            && never_ends
            && self.actions.iter().any(Action::retained)
        {
            return Some(
                "A native-triggered effect with retained actions and no ending stays active until the perk is removed.",
            );
        }
        if !self.has_kill_trigger()
            && self.actions.iter().any(|action| {
                matches!(
                    action,
                    Action::Spawn {
                        position: Position::Event,
                        ..
                    }
                )
            })
        {
            return Some(
                "Event Position spawns at the triggering event, which only kill triggers supply.",
            );
        }
        None
    }
}
