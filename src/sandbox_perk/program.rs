//! Authored action programs, independent of a stock perk's action graph.
//!
//! The compiler owns routing masks, condition ordinals and retained-state counts.
//! Native entities remain reusable building blocks, with edits kept on private clones.
use crate::sandbox_perk::action::native::NodeKind as NativeNodeKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub use crate::ability::AbilityTarget;
use crate::runtime::WeaponRuntimeValueOverride;
mod selectors;
pub use selectors::{
    AbilityState, AbilityVersion, AttachmentTarget, DamageMode, PropertyOperation, ValueSource,
};

pub(crate) mod compiler;
pub use compiler::draft as native_draft;
pub use compiler::{Compiled, compile};
pub mod decompile;
pub mod properties;
pub use properties::{KeyCatalog, KeyEvidence};

/// The empty FNV-1 hash. A native key holds this value when nothing is named.
pub const EMPTY_KEY: u32 = 0x811C_9DC5;

/// The input selector value most stock Component Value Adjustment nodes store.
const fn no_input() -> ValueSource {
    ValueSource::None
}

fn is_no_input(input: &ValueSource) -> bool {
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
            Self::Always => "Always",
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
            Self::Always => "Starts when the perk is applied and lasts until it is removed.",
            Self::Equipped => "Starts when this weapon is equipped and ends when it is unequipped.",
            Self::Drawn => "Starts when this weapon is drawn and ends when it is holstered.",
            Self::WeaponKill => "Starts on a kill with this weapon.",
            Self::PrecisionKill => "Starts on a precision kill with this weapon.",
            Self::MeleeKill => "Starts on any melee kill, whichever weapon is in hand.",
            Self::GrenadeKill => "Starts on any grenade kill, whichever weapon is in hand.",
            Self::AnyKill => "Starts on any credited kill.",
            Self::Native => {
                "Starts when its condition passes. Its fields are kept as the game stores them."
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
        Self::fresh(NativeNodeKind::Effect(kind))
    }

    /// An editable configuration of an observed native condition kind.
    #[must_use]
    pub fn condition(kind: u8) -> Option<Self> {
        Self::fresh(NativeNodeKind::Condition(kind))
    }

    fn fresh(selection: NativeNodeKind) -> Option<Self> {
        use crate::sandbox_perk::action::{layout, native};
        let condition = selection.is_condition();
        let kind = selection.byte();
        let mut bytes = if condition {
            layout::blank_condition(kind)
                .or_else(|| native::template(NativeNodeKind::Condition(kind)))?
        } else {
            layout::blank_effect(kind).or_else(|| native::template(NativeNodeKind::Effect(kind)))?
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
            return graph.validate_node(if condition {
                NativeNodeKind::Condition(self.kind)
            } else {
                NativeNodeKind::Effect(self.kind)
            });
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
            Self::Owner => "You",
            Self::Event => "Triggering Event",
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
    /// What the graph's Status Icon shows on the HUD, in place of its stock status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hud_status: Option<HudStatus>,
}

/// A HUD status of the project's own. The HUD finds a status's name and icon by its name hash,
/// so the build gives this one a hash of its own, a string under it and a row in the HUD status
/// table naming an icon.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HudStatus {
    pub name: String,
    /// The stock HUD status whose icon this one shows, by its name hash. `None` keeps the icon
    /// of the status the graph shows.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "hex_key_option"
    )]
    pub icon: Option<u32>,
    /// An icon of the author's own, a base64 PNG, shown in place of any stock status's icon. The
    /// build fits it to each texture the stock icon has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

/// One checked byte replacement in a resource of a privately cloned native effect graph.
/// The expected bytes prevent a different package version from receiving the patch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeAssetResourcePatch {
    pub binding_hash: u32,
    pub resource_index: u16,
    pub offset: u32,
    #[serde(with = "hex_bytes")]
    pub expected: Vec<u8>,
    #[serde(with = "hex_bytes")]
    pub bytes: Vec<u8>,
    /// An imported particle node of the weapon whose plug carries the program. The build writes
    /// that node's tag in place of `bytes`, which then only hold its four-byte width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_particle: Option<String>,
}

/// Private edits to the existing graph referenced by one native effect action.
/// The action is compiled against the live source graph, then its new private copy is rebound.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeAssetPatch {
    pub action_index: usize,
    pub source_graph: u32,
    pub patches: Vec<NativeAssetResourcePatch>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub appends: Vec<NativeAssetResourceAppend>,
    /// Component owners the private copy leaves out, with the events they send. An owner that
    /// another owner still sends events into is refused.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_owners: Vec<u32>,
}

/// Serialized definition data appended to a private owner. Runtime allocation
/// is unchanged. Checked patches separately retarget its existing descriptors.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeAssetResourceAppend {
    pub binding_hash: u32,
    pub resource_index: u16,
    pub expected_owner_size: u32,
    #[serde(with = "hex_bytes")]
    pub bytes: Vec<u8>,
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
/// one of three weapon slots or one of three ammunition types. The slot amounts follow
/// the same Kinetic/Energy/Power bank as the magazine predicates. E8F780 selects the ammo
/// category amounts, independently corroborated by Scavenger and Armaments records.
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
            Self::Slot1 => "Kinetic Slot",
            Self::Slot2 => "Energy Slot",
            Self::Slot3 => "Power Slot",
            Self::Category1 => "Primary Ammo",
            Self::Category2 => "Special Ammo",
            Self::Category3 => "Heavy Ammo",
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
        /// Native target at +0x02. Zero selects the perk's hosting object, one its owning
        /// player. Two and three resolve event objects. Other values remain byte-exact.
        #[serde(
            default = "default_attach_mode",
            skip_serializing_if = "is_default_attach_mode"
        )]
        mode: AttachmentTarget,
        /// Cleanup policy key at +0x18 and removal parameter at +0x1C. A nonempty first
        /// key suppresses automatic retirement. The second names the value written on removal.
        #[serde(
            default = "empty_keys",
            skip_serializing_if = "is_empty_keys",
            with = "hex_keys"
        )]
        keys: [u32; 2],
        /// Four lanes written to the named removal parameter. Bits remain exact. Their
        /// units and meaning belong to the attached entity, not to this action kind.
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
    /// selector names the ability (see `action::component_target`). State, ability version
    /// and value-program input have independent contracts in `action::native::fields`.
    /// The compiler preserves their native bytes, including unknown values.
    AdjustComponent {
        /// The ability selector byte at `+0x02`. Stock nodes store 0, 1, 2 and 7.
        target: AbilityTarget,
        /// Ability state at `+0x03`: 0 any, 1 inactive, 2 active. Other values skip the action.
        #[serde(default, skip_serializing_if = "is_zero_byte")]
        flag: AbilityState,
        /// Ability version at `+0x04`: zero current, nonzero base/original ability.
        #[serde(default, skip_serializing_if = "is_zero_byte")]
        option: AbilityVersion,
        /// The scale at `+0x08`, kept as a bit pattern so recipe equality stays exact.
        #[serde(with = "float_bit")]
        scale_bits: u32,
        /// The limit at `+0x0C`, kept as a bit pattern. Negative disables it, zero is a limit.
        #[serde(default, skip_serializing_if = "is_zero", with = "float_bit")]
        limit_bits: u32,
        /// The constant the value program pushes, kept as a bit pattern.
        #[serde(with = "float_bit")]
        value_bits: u32,
        /// The input selector byte at `+0x48`. Stock nodes store 0xFF in 106 of 179 cases.
        #[serde(default = "no_input", skip_serializing_if = "is_no_input")]
        input: ValueSource,
    },
    /// Changes a named property inside one ability's bank, the way stock exotics grant an
    /// extra grenade charge or improve a jump.
    ///
    /// The ability follows the target selector (see `action::ability_slot`). The key names
    /// an ability property, and the operation adds or removes a reference to that property.
    /// The engine reverses the operation when the effect ends.
    AbilityProperty {
        /// The ability selector byte at `+0x02`. Stock nodes store 0, 1, 2, 3, 4 and 7.
        target: AbilityTarget,
        /// The property key at `+0x04`, a 32-bit hash.
        #[serde(with = "hex_key")]
        key: u32,
        /// Operation at `+0x08`: zero applies a property reference, nonzero removes one.
        #[serde(default, skip_serializing_if = "is_zero_byte")]
        option: PropertyOperation,
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
        mode: DamageMode,
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
fn is_zero_byte<T: Copy + Into<u8>>(value: &T) -> bool {
    (*value).into() == 0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// A single key written as a `0x` hexadecimal string.
/// A single float carried as its bits, so a program stays `Eq`, written as the number.
mod f32_bits {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(bits: &u32, serializer: S) -> Result<S::Ok, S::Error> {
        f32::from_bits(*bits).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
        f32::deserialize(deserializer).map(f32::to_bits)
    }
}

/// A property a program defines for itself. The build gives every bank of `slot` that lists
/// `parameter` a row under `key` that sets the parameter to `value`, added to the running
/// value or written over it, so an Ability Property action applying `key` on that slot works
/// on every Subclass. The key is a hash of the tuning (`ability::bank::tuning_key`), so equal
/// tunings on any perk share one row. It is 32 bits, so different tunings can share a key, and
/// the program and the build refuse that rather than let one take the other's row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbilityTuning {
    #[serde(with = "hex_key")]
    pub key: u32,
    pub slot: AbilityTarget,
    #[serde(with = "hex_key")]
    pub parameter: u32,
    #[serde(rename = "value", with = "f32_bits")]
    pub value_bits: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub add: bool,
}

/// A private adjustment to a native base ability input. The property lifetime controls
/// its weight, so removing or holstering the item restores the unmodified ability.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbilityInput {
    #[serde(with = "hex_key")]
    pub key: u32,
    pub slot: AbilityTarget,
    pub input: u8,
    #[serde(rename = "value", with = "f32_bits")]
    pub value_bits: u32,
    pub multiply: bool,
}

impl AbilityInput {
    #[must_use]
    pub fn new(slot: AbilityTarget, input: u8, value: f32, multiply: bool) -> Self {
        let value_bits = value.to_bits();
        let key = crate::hash::fnv1_name_hash(&format!(
            "parhelion.ability.input.{slot}.{input}.{value_bits:08x}.{}",
            u8::from(multiply)
        ));
        Self {
            key,
            slot,
            input,
            value_bits,
            multiply,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let value = f32::from_bits(self.value_bits);
        if self.input >= 7
            || !value.is_finite()
            || (self.multiply && value < 0.0)
            || crate::ability::bank::slot_banks(self.slot).is_empty()
            || self.key != Self::new(self.slot, self.input, value, self.multiply).key
        {
            return Err("Invalid private base ability input adjustment".into());
        }
        Ok(())
    }
}

impl AbilityTuning {
    /// A tuning with the key its definition hashes to.
    #[must_use]
    pub fn new(slot: AbilityTarget, parameter: u32, value: f32, add: bool) -> Self {
        Self {
            key: crate::ability::bank::tuning_key(slot, parameter, value.to_bits(), add),
            slot,
            parameter,
            value_bits: value.to_bits(),
            add,
        }
    }

    /// The value the row writes or adds.
    #[must_use]
    pub fn value(&self) -> f32 {
        f32::from_bits(self.value_bits)
    }

    /// Whether the key is the one this tuning's definition hashes to.
    #[must_use]
    pub fn keyed_by_definition(&self) -> bool {
        self.key
            == crate::ability::bank::tuning_key(
                self.slot,
                self.parameter,
                self.value_bits,
                self.add,
            )
    }
}

/// Effect kind 7, Ability Property, whose record holds the slot at +2 and the key at +4.
pub const ABILITY_PROPERTY_KIND: u8 = 7;
const ABILITY_PROPERTY_CLASS: u32 = 0x8080_3E1D;

/// The slot and key of every Ability Property action in a compiled action payload. An empty
/// payload, a declaration with no action of its own, applies nothing.
pub fn ability_properties_in(payload: &[u8]) -> Result<BTreeSet<(AbilityTarget, u32)>, String> {
    if payload.is_empty() {
        return Ok(BTreeSet::new());
    }
    let decoded = crate::sandbox_perk::action::decode(payload)?;
    Ok(decoded
        .effects()
        .filter(|effect| effect.kind == ABILITY_PROPERTY_KIND)
        .filter_map(|effect| ability_property_of(&effect.native))
        .collect())
}

fn ability_property_of(record: &[u8]) -> Option<(AbilityTarget, u32)> {
    let slot = *record.get(2)?;
    let key = u32::from_le_bytes(record.get(4..8)?.try_into().ok()?);
    Some((AbilityTarget::from_byte(slot), key))
}

/// The tunings a program defines for its Ability Property actions.
impl Program {
    /// Why this program cannot become an editable native program without losing part of it.
    /// The native form keeps component edits per referenced graph, so a guided program's
    /// private resource edits to the graphs its actions reference have no place in it.
    #[must_use]
    pub fn native_adoption_issue(&self) -> Option<&'static str> {
        (self.native.is_none() && !self.native_asset_patches.is_empty()).then_some(
            "This effect edits private copies of the objects it attaches, which editing it here would drop. It stays as it is.",
        )
    }

    /// This program's name and tunings around a native draft of it, for the editors that
    /// adopt the draft as the program. Refused while the program holds anything the native
    /// form cannot keep, so an edit never silently drops part of an effect.
    pub fn with_native(&self, native: NativeProgram) -> Result<Self, String> {
        if let Some(issue) = self.native_adoption_issue() {
            return Err(issue.into());
        }
        Ok(Self {
            name: self.name.clone(),
            native: Some(native),
            ability_tunings: self.ability_tunings.clone(),
            ability_inputs: self.ability_inputs.clone(),
            ..Self::default()
        })
    }

    /// The slot and key of every Ability Property action, read from the native graph when
    /// the program has one and from the typed actions otherwise.
    pub fn ability_properties(&self) -> Result<BTreeSet<(AbilityTarget, u32)>, String> {
        if let Some(native) = &self.native {
            return ability_properties_in(&native.graph.emit()?);
        }
        Ok(self
            .actions
            .iter()
            .filter_map(|action| match action {
                Action::AbilityProperty { target, key, .. } => Some((*target, *key)),
                Action::Native { node } if node.kind == ABILITY_PROPERTY_KIND => {
                    ability_property_of(&node.bytes)
                }
                _ => None,
            })
            .collect())
    }

    /// Defines a tuning. With `replaced`, the tuning under that key gives way to it, and
    /// every Ability Property action applying the old key applies the new one.
    pub fn define_ability_tuning(
        &mut self,
        replaced: Option<u32>,
        tuning: AbilityTuning,
    ) -> Result<(), String> {
        if !tuning.keyed_by_definition() {
            return Err(format!(
                "Ability tuning key {:08X} does not match its definition.",
                tuning.key
            ));
        }
        if let Some(old) = replaced
            && old != tuning.key
        {
            self.ability_tunings.retain(|existing| existing.key != old);
            self.rekey_ability_properties(old, tuning.key)?;
        }
        match self
            .ability_tunings
            .iter_mut()
            .find(|existing| existing.key == tuning.key)
        {
            Some(existing) if *existing == tuning => {}
            // The key is a 32-bit hash, so a different tuning can land on it. Replacing that one
            // would change the value every action applying the key gets.
            Some(_) => {
                return Err(format!(
                    "Another tuning of this effect already uses key {:08X}. Change this value slightly.",
                    tuning.key
                ));
            }
            None => self.ability_tunings.push(tuning),
        }
        Ok(())
    }

    fn rekey_ability_properties(&mut self, from: u32, to: u32) -> Result<(), String> {
        let from_bytes = from.to_le_bytes();
        for action in &mut self.actions {
            match action {
                Action::AbilityProperty { key, .. } if *key == from => *key = to,
                Action::Native { node }
                    if node.kind == ABILITY_PROPERTY_KIND
                        && node.bytes.get(4..8) == Some(&from_bytes[..]) =>
                {
                    node.bytes[4..8].copy_from_slice(&to.to_le_bytes());
                }
                _ => {}
            }
        }
        if let Some(native) = &mut self.native {
            let stride =
                crate::sandbox_perk::action::native::schema::record(ABILITY_PROPERTY_CLASS)?.size;
            for block in native
                .graph
                .blocks
                .iter_mut()
                .filter(|block| block.class == ABILITY_PROPERTY_CLASS)
            {
                for row in block.bytes.chunks_exact_mut(stride) {
                    if row.get(4..8) == Some(&from_bytes[..]) {
                        row[4..8].copy_from_slice(&to.to_le_bytes());
                    }
                }
            }
        }
        Ok(())
    }

    /// Drops the tunings no Ability Property action applies any more. A program that cannot
    /// be read keeps them all.
    pub fn prune_ability_tunings(&mut self) {
        if self.ability_tunings.is_empty() {
            return;
        }
        if let Ok(applied) = self.ability_properties() {
            self.ability_tunings
                .retain(|tuning| applied.iter().any(|(_, key)| *key == tuning.key));
        }
    }
}

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

const fn default_attach_mode() -> AttachmentTarget {
    AttachmentTarget::Player
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_attach_mode(mode: &AttachmentTarget) -> bool {
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

    /// An adjustment of the given ability's energy, with the scale and value stock energy
    /// perks use most and no limit on movement toward a target value.
    #[must_use]
    pub const fn adjust_component(target: AbilityTarget) -> Self {
        Self::AdjustComponent {
            target,
            flag: AbilityState::Any,
            option: AbilityVersion::Current,
            scale_bits: 0x3F80_0000,
            limit_bits: 0xBF80_0000,
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
    pub const fn set_damage_type(mode: DamageMode) -> Self {
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
    pub const fn ability_property(target: AbilityTarget) -> Self {
        Self::AbilityProperty {
            target,
            key: EMPTY_KEY,
            option: PropertyOperation::Apply,
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

    /// The engine effect kind this action compiles to.
    #[must_use]
    pub const fn kind(&self) -> u8 {
        match self {
            Self::Attach { .. } => 1,
            Self::Spawn { .. } => 3,
            Self::Pattern { .. } => 26,
            Self::ExtendTimers { .. } => 32,
            Self::Property { .. } => 10,
            Self::AdjustComponent { .. } => 8,
            Self::UpdateAccumulator { .. } => 42,
            Self::AbilityProperty { .. } => 7,
            Self::TransmatContext { .. } => 47,
            Self::OverrideHostKey { .. } => 35,
            Self::SetDamageType { .. } => 6,
            Self::WeaponReferenceCount { .. } => 30,
            Self::AddRounds { .. } => 14,
            Self::AddFraction { .. } => 15,
            Self::Native { node } => node.kind,
        }
    }

    /// The action's name, which is its effect kind's, so a typed action and the native node
    /// it compiles to read alike.
    #[must_use]
    pub fn label(&self) -> &'static str {
        crate::sandbox_perk::nodes::effect_title(self.kind())
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
    /// Checked resource edits for a graph referenced by a verbatim native effect.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub native_asset_patches: Vec<NativeAssetPatch>,
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
    /// Properties this program defines for its own Ability Property actions, which the build
    /// writes into the banks of each tuning's slot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ability_tunings: Vec<AbilityTuning>,
    /// Private adjustments to native ability inputs, activated by Ability Property actions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ability_inputs: Vec<AbilityInput>,
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

    /// Every node of the group, each carrying its condition or effect kind.
    pub fn nodes(&self) -> impl Iterator<Item = (NativeNodeKind, &NativeNode)> {
        self.conditions()
            .map(|node| (NativeNodeKind::Condition(node.kind), node))
            .chain(
                self.effects
                    .iter()
                    .map(|node| (NativeNodeKind::Effect(node.kind), node)),
            )
    }

    /// Every node of the group for editing, each carrying its condition or effect kind.
    pub fn nodes_mut(&mut self) -> impl Iterator<Item = (NativeNodeKind, &mut NativeNode)> {
        self.activation
            .iter_mut()
            .chain(&mut self.removal)
            .chain(&mut self.rearm)
            .map(|node| (NativeNodeKind::Condition(node.kind), node))
            .chain(
                self.effects
                    .iter_mut()
                    .map(|node| (NativeNodeKind::Effect(node.kind), node)),
            )
    }
}

mod native;
pub use native::{NativeIssue, NativeProgram};

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
            native_asset_patches: Vec::new(),
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
            ability_tunings: Vec::new(),
            ability_inputs: Vec::new(),
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
    pub fn native_nodes(&self) -> impl Iterator<Item = (NativeNodeKind, &NativeNode)> {
        self.native_trigger
            .iter()
            .chain(&self.alternative_triggers)
            .chain(&self.native_removal)
            .chain(&self.alternative_removals)
            .chain(&self.native_rearm)
            .chain(&self.alternative_rearms)
            .map(|node| (NativeNodeKind::Condition(node.kind), node))
            .chain(self.actions.iter().filter_map(|action| match action {
                Action::Native { node } => Some((NativeNodeKind::Effect(node.kind), node)),
                _ => None,
            }))
            .chain(self.additional_groups.iter().flat_map(NativeGroup::nodes))
    }

    /// The same nodes as [`Self::native_nodes`], for passes that rewrite them.
    pub fn native_nodes_mut(&mut self) -> impl Iterator<Item = (NativeNodeKind, &mut NativeNode)> {
        self.native_trigger
            .iter_mut()
            .chain(&mut self.alternative_triggers)
            .chain(&mut self.native_removal)
            .chain(&mut self.alternative_removals)
            .chain(&mut self.native_rearm)
            .chain(&mut self.alternative_rearms)
            .map(|node| (NativeNodeKind::Condition(node.kind), node))
            .chain(self.actions.iter_mut().filter_map(|action| match action {
                Action::Native { node } => Some((NativeNodeKind::Effect(node.kind), node)),
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

    /// Whether the trigger reports a place to spawn at: a kill or damage dealt, directly or
    /// nested in a counter, a "while" check or a requirement, as the workbench's card reads it.
    fn places_event(&self) -> bool {
        fn places(condition: &crate::sandbox_perk::action::DecodedCondition) -> bool {
            matches!(condition.kind, 2 | 4)
                || condition.children.iter().any(places)
                || condition
                    .subgroups
                    .iter()
                    .any(|subgroup| subgroup.conditions.iter().any(places))
        }
        self.trigger.is_event()
            || self.trigger == Trigger::Native
                && self.native_trigger.as_ref().is_some_and(|node| {
                    crate::sandbox_perk::action::decode_condition_node(&node.bytes)
                        .is_ok_and(|condition| places(&condition))
                })
    }

    fn validate_asset_patches(&self) -> Result<(), String> {
        let mut patched_actions = BTreeSet::new();
        for edit in &self.native_asset_patches {
            if !patched_actions.insert(edit.action_index) {
                return Err("A native asset action has more than one patch set.".into());
            }
            let action = self
                .actions
                .get(edit.action_index)
                .ok_or_else(|| "An asset patch must select an effect action.".to_owned())?;
            let matches_graph = if let Some(asset) = action.asset() {
                asset.graph == edit.source_graph && asset.values.is_empty()
            } else {
                matches!(action, Action::Native { node } if node.kind == 2
                    && node.bytes.get(16..20) == Some(&edit.source_graph.to_le_bytes()[..]))
            };
            if !matches_graph || matches!(edit.source_graph, 0 | u32::MAX) {
                return Err("A native asset patch must name its effect's live graph.".into());
            }
            if edit.patches.is_empty() || edit.patches.len() > 32 {
                return Err("A native asset patch needs one to 32 resource edits.".into());
            }
            for patch in &edit.patches {
                if patch.binding_hash == 0
                    || patch.expected.is_empty()
                    || patch.expected.len() != patch.bytes.len()
                    || patch.bytes.len() > 65_536
                    || patch.offset.checked_add(patch.bytes.len() as u32).is_none()
                {
                    return Err("A native asset resource edit has invalid bounds or bytes.".into());
                }
            }
            let mut appended = BTreeSet::new();
            for append in &edit.appends {
                if append.binding_hash == 0
                    || append.expected_owner_size < 32
                    || append.bytes.is_empty()
                    || append.bytes.len() > 262_144
                    || append
                        .expected_owner_size
                        .checked_add(append.bytes.len() as u32)
                        .is_none()
                    || !appended.insert((append.binding_hash, append.resource_index))
                {
                    return Err("An asset append has invalid bounds or duplicate resources.".into());
                }
            }
            if edit.appends.len() > 32 {
                return Err("An asset patch can append up to 32 resources.".into());
            }
            for (index, left) in edit.patches.iter().enumerate() {
                for right in &edit.patches[index + 1..] {
                    if left.binding_hash == right.binding_hash
                        && left.resource_index == right.resource_index
                        && left.offset < right.offset + right.bytes.len() as u32
                        && right.offset < left.offset + left.bytes.len() as u32
                    {
                        return Err("Native asset resource edits overlap.".into());
                    }
                }
            }
        }
        Ok(())
    }

    /// Drafts can be empty. Build readiness is checked separately.
    pub fn validate_structure(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.contains('\0') {
            return Err("Enter a name for the custom effect.".into());
        }
        if let Some(native) = &self.native {
            let defaults = Self::default();
            if !self.actions.is_empty()
                || !self.native_asset_patches.is_empty()
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
        self.validate_asset_patches()?;
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
                    return Err(format!(
                        "{} technical values must be finite numbers.",
                        action.label()
                    ));
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
                        return Err(format!("{} needs a property key.", action.label()));
                    }
                    if !f32::from_bits(*value_bits).is_finite()
                        || !f32::from_bits(*restore_bits).is_finite()
                    {
                        return Err(format!("{} values must be finite numbers.", action.label()));
                    }
                }
                Action::AddRounds { rounds, .. } => {
                    if *rounds == 0 || rounds.unsigned_abs() > 999 {
                        return Err(format!(
                            "{} needs a round count from -999 to 999, not zero.",
                            action.label()
                        ));
                    }
                }
                Action::AddFraction { fraction_bits, .. } => {
                    let fraction = f32::from_bits(*fraction_bits);
                    if !fraction.is_finite() || fraction == 0.0 || fraction.abs() > 100.0 {
                        return Err(format!(
                            "{} needs a fraction from -100 to 100 capacities, not zero.",
                            action.label()
                        ));
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
        if !self.ability_inputs.is_empty() {
            let applied = self.ability_properties()?;
            let mut keys = BTreeSet::new();
            for input in &self.ability_inputs {
                input.validate()?;
                if !keys.insert(input.key)
                    || !applied.contains(&(input.slot, input.key))
                    || self
                        .ability_tunings
                        .iter()
                        .any(|tuning| tuning.key == input.key)
                {
                    return Err(
                        "Private ability input is duplicated or lacks its activating action".into(),
                    );
                }
            }
        }
        if !self.ability_tunings.is_empty() {
            let applied = self.ability_properties()?;
            let slot_name = |slot: AbilityTarget| {
                crate::ability::bank::slot_name(slot)
                    .map_or_else(|| format!("slot {slot}"), str::to_owned)
            };
            let mut keys = BTreeSet::new();
            for tuning in &self.ability_tunings {
                if !keys.insert(tuning.key) {
                    return Err(format!(
                        "Two ability tunings of this effect share key {:08X}.",
                        tuning.key
                    ));
                }
                if crate::ability::bank::slot_banks(tuning.slot).is_empty() {
                    return Err(format!("Ability tuning slot {} has no banks.", tuning.slot));
                }
                if !tuning.keyed_by_definition() {
                    return Err(format!(
                        "Ability tuning key {:08X} does not match its definition.",
                        tuning.key
                    ));
                }
                if applied.contains(&(tuning.slot, tuning.key)) {
                    continue;
                }
                let elsewhere = applied.iter().find(|(_, key)| *key == tuning.key);
                return Err(match elsewhere {
                    Some((slot, _)) => format!(
                        "Ability tuning {:08X} is defined for {} but applied on {}.",
                        tuning.key,
                        slot_name(tuning.slot),
                        slot_name(*slot)
                    ),
                    None => format!(
                        "Ability tuning {:08X} is not applied by an Ability Property action on {}.",
                        tuning.key,
                        slot_name(tuning.slot)
                    ),
                });
            }
        }
        if let Some(native) = &self.native {
            if let Some(issue) = native.authoring_issue()? {
                return Err(format!(
                    "Behavior {}, Action {}, {}: {}",
                    issue.group + 1,
                    issue.action + 1,
                    issue.field,
                    issue.message
                ));
            }
            return Ok(());
        }
        for action in &self.actions {
            if let Action::Native { node } = action {
                let class = crate::sandbox_perk::nodes::effect(node.kind)
                    .ok_or("Unknown action kind.")?
                    .class;
                native::entity_reference(class, &node.bytes)?;
            }
            if let Some(asset) = action.asset()
                && (asset.graph == 0 || asset.graph == u32::MAX)
            {
                return Err("Choose an asset for every action.".into());
            }
            if matches!(action, Action::ExtendTimers { .. }) && !self.has_kill_trigger() {
                return Err("Extend Timers requires a kill trigger.".into());
            }
        }
        for group in &self.additional_groups {
            for node in &group.effects {
                let class = crate::sandbox_perk::nodes::effect(node.kind)
                    .ok_or("Unknown action kind.")?
                    .class;
                native::entity_reference(class, &node.bytes)?;
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
        if !self.places_event()
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
            return Some("Spawning at the triggering event needs a kill or damage trigger.");
        }
        None
    }
}
