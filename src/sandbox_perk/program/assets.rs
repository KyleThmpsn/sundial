//! Assets a program may reference: HUD statuses, native asset patches and ammunition targets.
use super::*;

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
    /// The damage type every damage profile the graph and the graphs below it name deals, on
    /// private copies of the profiles. `None` keeps each profile's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub damage_type: Option<DamageMode>,
    /// Modifier rows added to the graph's own: one more record each in its Property Modifiers
    /// array, written after the graph's records in a private copy.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<ModifierRow>,
}

/// One modifier row added to an attachment: the component interface and the input within it
/// the row changes, the ability slot when the interface is Abilities, how the amount applies
/// and the amount.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModifierRow {
    pub component: u8,
    #[serde(default = "no_ability", skip_serializing_if = "is_no_ability")]
    pub ability: i16,
    pub input: i16,
    /// 0 adds the amount, 1 multiplies by it.
    pub operation: u8,
    #[serde(with = "super::float_bit")]
    pub amount_bits: u32,
}

const fn no_ability() -> i16 {
    -1
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_no_ability(ability: &i16) -> bool {
    *ability == no_ability()
}

impl ModifierRow {
    /// A row that leaves its input alone: nothing added to the Barrel's first Rounds per Burst
    /// lane.
    #[must_use]
    pub const fn neutral() -> Self {
        Self {
            component: 2,
            ability: -1,
            input: 22,
            operation: 0,
            amount_bits: 0,
        }
    }

    #[must_use]
    pub fn amount(&self) -> f32 {
        f32::from_bits(self.amount_bits)
    }

    pub fn set_amount(&mut self, amount: f32) {
        self.amount_bits = amount.to_bits();
    }
}

/// A translated attachment group owned by one action. The package allocator resolves
/// the declared root symbol after native compilation has checked the action's envelope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedAsset {
    pub action_index: usize,
    pub directory: std::path::PathBuf,
    pub sha256: String,
    pub symbol: String,
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
        with = "super::hex_key_option"
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
    #[serde(with = "super::hex_bytes")]
    pub expected: Vec<u8>,
    #[serde(with = "super::hex_bytes")]
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
    #[serde(with = "super::hex_bytes")]
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
