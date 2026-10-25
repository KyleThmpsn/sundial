//! Custom material values a shader recipe gives its dyes.
//!
//! A shader carries one dye per channel for each gear type, and each dye paints two surfaces from
//! 27 material vectors (Bungie's 2019 gear dye layout, which Sundial's dye reader reads too) and
//! binds a detail texture and a detail normal texture both surfaces share. A surface edit sets
//! any of a surface's values, and a texture edit a dye's textures and their tiling. Either applies
//! to that channel's dye on every gear type, so the shader looks the same on armor, weapons,
//! ships, Sparrows and Ghosts, unless it names one gear type. An edit for one gear type wins over
//! the every-gear edit, setting by setting.

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// The rows of the game's iridescence lookup. Rows past the authored ones hold a placeholder.
pub const IRIDESCENCE_ROWS: i16 = 128;
/// A surface with no iridescence.
pub const NO_IRIDESCENCE: i16 = -1;

/// Preserve base colors while allowing equipped shaders to replace locked channels.
pub(crate) fn unlock_base_colors(rows: &mut [Vec<crate::WeaponDyeReferenceOverride>; 3]) {
    let locked = std::mem::take(&mut rows[2]);
    let channels = locked
        .iter()
        .map(|row| row.channel_index)
        .collect::<std::collections::BTreeSet<_>>();
    // A source custom row must not hide the previously locked base color once
    // that color becomes a default. Keep unrelated custom channels intact.
    rows[0].retain(|row| !channels.contains(&row.channel_index));
    rows[1].retain(|row| !channels.contains(&row.channel_index));
    rows[1].extend(locked);
}

/// One of the three channels every gear type paints with its own dye.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DyeChannel {
    Armor,
    Cloth,
    Suit,
}

impl DyeChannel {
    pub const ALL: [Self; 3] = [Self::Armor, Self::Cloth, Self::Suit];

    /// The channel's position among the three, as dyes count their surfaces.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Armor => 0,
            Self::Cloth => 1,
            Self::Suit => 2,
        }
    }

    /// The channel's offset within each gear type's first dye key.
    #[must_use]
    pub const fn offset(self) -> i8 {
        match self {
            Self::Armor => 0,
            Self::Cloth => 1,
            Self::Suit => 2,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Armor => "Armor",
            Self::Cloth => "Cloth",
            Self::Suit => "Suit",
        }
    }

    /// The channel's name on one gear type, or on every gear type at once. Bungie names only
    /// armor's channels (their hashes are its words "armor", "cloth" and "suit") and numbers every
    /// other gear type's, so those go by number.
    #[must_use]
    pub const fn name_on(self, gear: Option<GearType>) -> &'static str {
        match (gear, self) {
            (None | Some(GearType::Armor), _) => self.label(),
            (_, Self::Armor) => "Channel 1",
            (_, Self::Cloth) => "Channel 2",
            (_, Self::Suit) => "Channel 3",
        }
    }
}

/// One of the two surfaces a dye paints.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DyeSurface {
    Primary,
    Secondary,
}

impl DyeSurface {
    pub const ALL: [Self; 2] = [Self::Primary, Self::Secondary];

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Primary => 0,
            Self::Secondary => 1,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Secondary => "Secondary",
        }
    }
}

/// A kind of gear a shader paints with three dyes of its own.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GearType {
    Armor,
    Weapon,
    Ship,
    Sparrow,
    GhostShell,
}

impl GearType {
    pub const ALL: [Self; 5] = [
        Self::Armor,
        Self::Weapon,
        Self::Ship,
        Self::Sparrow,
        Self::GhostShell,
    ];

    /// The gear type's first dye key. Its channels follow in order.
    #[must_use]
    pub const fn first_key(self) -> i8 {
        match self {
            Self::Armor => 0,
            Self::Weapon => 4,
            Self::Ship => 7,
            Self::Sparrow => 10,
            Self::GhostShell => 13,
        }
    }

    /// The dye key of one of the gear type's channels.
    #[must_use]
    pub const fn key(self, channel: DyeChannel) -> i8 {
        self.first_key() + channel.offset()
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Armor => "Armor",
            Self::Weapon => "Weapons",
            Self::Ship => "Ships",
            Self::Sparrow => "Sparrows",
            Self::GhostShell => "Ghost Shells",
        }
    }
}

/// The gear type and channel a dye key paints, if it is one of the gear-type keys.
#[must_use]
pub fn slot_of_key(key: i8) -> Option<(GearType, DyeChannel)> {
    GearType::ALL.into_iter().find_map(|gear| {
        DyeChannel::ALL
            .into_iter()
            .find(|channel| gear.key(*channel) == key)
            .map(|channel| (gear, channel))
    })
}

/// Where a dye's values sit among its 27 material vectors, for the primary and the secondary
/// surface.
const EMISSIVE: [usize; 2] = [3, 4];
const ALBEDO: [usize; 2] = [9, 13];
/// Detail color, detail normal and detail smoothness strengths, then metalness.
const PARAMS: [usize; 2] = [10, 14];
/// The iridescence row, in the first lane.
const ADVANCED: [usize; 2] = [11, 15];
/// The smoothness remap: offset, scale, least and range.
const SMOOTHNESS: [usize; 2] = [12, 16];
const WORN_ALBEDO: [usize; 2] = [17, 21];
/// How the gear's wear mask becomes wear: offset, scale, least and range.
const WEAR: [usize; 2] = [18, 22];
const WORN_SMOOTHNESS: [usize; 2] = [19, 23];
const WORN_PARAMS: [usize; 2] = [20, 24];
/// The detail texture's and the detail normal texture's scale (x, y) and offset (z, w), which
/// both surfaces share.
const DETAIL_TILING: usize = 0;
const NORMAL_TILING: usize = 1;

/// Writes values into a dye's 27 vectors: a vector, its lane and the value. Writes past the
/// vectors are ignored.
pub fn write_vectors(vectors: &mut [[f32; 4]; 27], writes: &[(usize, usize, f32)]) {
    for &(vector, lane, value) in writes {
        if let Some(slot) = vectors
            .get_mut(vector)
            .and_then(|vector| vector.get_mut(lane))
        {
            *slot = value;
        }
    }
}

/// A finite material value, written in recipes as a plain number. Recipes compare exactly, so
/// negative zero reads as zero.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DyeValue(f32);

impl Eq for DyeValue {}

impl std::hash::Hash for DyeValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

impl DyeValue {
    /// The value, when it is finite.
    #[must_use]
    pub fn new(value: f32) -> Option<Self> {
        value.is_finite().then_some(Self(value + 0.0))
    }

    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for DyeValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f32::deserialize(deserializer)?;
        Self::new(value).ok_or_else(|| D::Error::custom("Dye values must be finite"))
    }
}

/// One surface's custom settings. A setting left unset keeps the dye's own.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DyeEdit {
    /// The one gear type the edit paints, or every gear type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gear: Option<GearType>,
    pub channel: DyeChannel,
    pub surface: DyeSurface,
    /// An sRGB color, written `#RRGGBB`.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "hex_color")]
    pub color: Option<[u8; 3]>,
    /// A row of the game's iridescence lookup, or -1 for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iridescence: Option<i16>,
    /// From paint (0) to bare metal (1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metalness: Option<DyeValue>,
    /// The least and most smoothness the surface takes, from 0 to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smoothness: Option<[DyeValue; 2]>,
    /// How strongly the dye's detail texture colors the surface, from 0 to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<DyeValue>,
    /// How strongly the dye's detail normal texture bumps the surface, from 0 to 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bumps: Option<DyeValue>,
    /// How strongly the detail texture's alpha changes the surface's smoothness, from 0 to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_smoothness: Option<DyeValue>,
    /// The sRGB color the surface glows with where its gear lets it, written `#RRGGBB`.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "hex_color")]
    pub glow: Option<[u8; 3]>,
    /// What wear exposes: its sRGB color, metalness and smoothness range.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "hex_color")]
    pub worn_color: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worn_metalness: Option<DyeValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worn_smoothness: Option<[DyeValue; 2]>,
    /// How the gear's wear mask becomes wear: an offset and a scale. A lower offset and a gentler
    /// scale wear more of the surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wear: Option<[DyeValue; 2]>,
}

impl DyeEdit {
    /// An edit that sets nothing yet.
    #[must_use]
    pub const fn new(gear: Option<GearType>, channel: DyeChannel, surface: DyeSurface) -> Self {
        Self {
            gear,
            channel,
            surface,
            color: None,
            iridescence: None,
            metalness: None,
            smoothness: None,
            detail: None,
            bumps: None,
            detail_smoothness: None,
            glow: None,
            worn_color: None,
            worn_metalness: None,
            worn_smoothness: None,
            wear: None,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::new(self.gear, self.channel, self.surface)
    }

    /// This edit's settings over `under`'s, setting by setting.
    #[must_use]
    pub fn over(self, under: Self) -> Self {
        Self {
            gear: self.gear,
            channel: self.channel,
            surface: self.surface,
            color: self.color.or(under.color),
            iridescence: self.iridescence.or(under.iridescence),
            metalness: self.metalness.or(under.metalness),
            smoothness: self.smoothness.or(under.smoothness),
            detail: self.detail.or(under.detail),
            bumps: self.bumps.or(under.bumps),
            detail_smoothness: self.detail_smoothness.or(under.detail_smoothness),
            glow: self.glow.or(under.glow),
            worn_color: self.worn_color.or(under.worn_color),
            worn_metalness: self.worn_metalness.or(under.worn_metalness),
            worn_smoothness: self.worn_smoothness.or(under.worn_smoothness),
            wear: self.wear.or(under.wear),
        }
    }

    /// The values the edit writes into its dye's 27 vectors: a vector, its lane and the value.
    /// Builds, previews and icons all write these.
    #[must_use]
    pub fn writes(&self) -> Vec<(usize, usize, f32)> {
        let surface = self.surface.index();
        let mut writes = Vec::new();
        let mut color = |vector: usize, color: Option<[u8; 3]>| {
            for (lane, value) in color.into_iter().flatten().enumerate() {
                writes.push((vector, lane, srgb_to_linear(value)));
            }
        };
        color(ALBEDO[surface], self.color);
        color(EMISSIVE[surface], self.glow);
        color(WORN_ALBEDO[surface], self.worn_color);
        let values = [
            (ADVANCED[surface], 0, self.iridescence.map(f32::from)),
            (PARAMS[surface], 0, self.detail.map(DyeValue::get)),
            (PARAMS[surface], 1, self.bumps.map(DyeValue::get)),
            (
                PARAMS[surface],
                2,
                self.detail_smoothness.map(DyeValue::get),
            ),
            (PARAMS[surface], 3, self.metalness.map(DyeValue::get)),
            (
                WORN_PARAMS[surface],
                3,
                self.worn_metalness.map(DyeValue::get),
            ),
            (WEAR[surface], 0, self.wear.map(|[offset, _]| offset.get())),
            (WEAR[surface], 1, self.wear.map(|[_, scale]| scale.get())),
        ];
        writes.extend(
            values
                .into_iter()
                .filter_map(|(vector, lane, value)| Some((vector, lane, value?))),
        );
        // A range is its least value and how far the most lies past it.
        for (vector, range) in [
            (SMOOTHNESS[surface], self.smoothness),
            (WORN_SMOOTHNESS[surface], self.worn_smoothness),
        ] {
            if let Some([least, most]) = range {
                writes.extend([
                    (vector, 2, least.get()),
                    (vector, 3, most.get() - least.get()),
                ]);
            }
        }
        writes
    }

    /// The surface the edit paints, as its error messages name it.
    fn describe(&self) -> String {
        let surface = format!(
            "{} {} dye",
            self.channel.name_on(self.gear).to_lowercase(),
            self.surface.label().to_lowercase()
        );
        match self.gear {
            Some(gear) => format!("{surface} for {}", gear.label().to_lowercase()),
            None => surface,
        }
    }
}

/// What `edits` set for one surface of one gear type: that gear type's own edit over the edit for
/// every gear type, setting by setting. The result names no gear type, and is none when neither
/// edit sets anything.
#[must_use]
pub fn surface_edit(
    edits: &[DyeEdit],
    gear: GearType,
    channel: DyeChannel,
    surface: DyeSurface,
) -> Option<DyeEdit> {
    let find = |target: Option<GearType>| {
        edits
            .iter()
            .find(|edit| (edit.gear, edit.channel, edit.surface) == (target, channel, surface))
    };
    let empty = DyeEdit::new(None, channel, surface);
    let edit = find(Some(gear))
        .copied()
        .unwrap_or(empty)
        .over(find(None).copied().unwrap_or(empty));
    let edit = DyeEdit { gear: None, ..edit };
    (!edit.is_empty()).then_some(edit)
}

/// One dye's textures: the detail texture and detail normal texture its surfaces share, by tag,
/// and how often each repeats. A setting left unset keeps the dye's own.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DyeTextureEdit {
    /// The one gear type the edit paints, or every gear type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gear: Option<GearType>,
    pub channel: DyeChannel,
    /// A detail texture: color in red, green and blue and smoothness in alpha.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "hex_tag")]
    pub detail: Option<u32>,
    /// A detail normal texture: the normal in red and green and occlusion in blue.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "hex_tag")]
    pub normal: Option<u32>,
    /// How often each texture repeats across the gear (x, y) and how far it is shifted (x, y).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_tiling: Option<[DyeValue; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_tiling: Option<[DyeValue; 4]>,
}

impl DyeTextureEdit {
    /// An edit that sets nothing yet.
    #[must_use]
    pub const fn new(gear: Option<GearType>, channel: DyeChannel) -> Self {
        Self {
            gear,
            channel,
            detail: None,
            normal: None,
            detail_tiling: None,
            normal_tiling: None,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::new(self.gear, self.channel)
    }

    /// This edit's settings over `under`'s, setting by setting.
    #[must_use]
    pub fn over(self, under: Self) -> Self {
        Self {
            gear: self.gear,
            channel: self.channel,
            detail: self.detail.or(under.detail),
            normal: self.normal.or(under.normal),
            detail_tiling: self.detail_tiling.or(under.detail_tiling),
            normal_tiling: self.normal_tiling.or(under.normal_tiling),
        }
    }

    /// The tiling the edit writes into its dye's vectors: a vector, its lane and the value.
    #[must_use]
    pub fn writes(&self) -> Vec<(usize, usize, f32)> {
        [
            (DETAIL_TILING, self.detail_tiling),
            (NORMAL_TILING, self.normal_tiling),
        ]
        .into_iter()
        .filter_map(|(vector, tiling)| Some((vector, tiling?)))
        .flat_map(|(vector, tiling)| {
            tiling
                .into_iter()
                .enumerate()
                .map(move |(lane, value)| (vector, lane, value.get()))
        })
        .collect()
    }

    fn describe(&self) -> String {
        let dye = format!(
            "{} dye textures",
            self.channel.name_on(self.gear).to_lowercase()
        );
        match self.gear {
            Some(gear) => format!("{dye} for {}", gear.label().to_lowercase()),
            None => dye,
        }
    }
}

/// What `edits` set for one dye of one gear type: that gear type's own edit over the edit for
/// every gear type. The result names no gear type, and is none when neither sets anything.
#[must_use]
pub fn texture_edit(
    edits: &[DyeTextureEdit],
    gear: GearType,
    channel: DyeChannel,
) -> Option<DyeTextureEdit> {
    let find = |target: Option<GearType>| {
        edits
            .iter()
            .find(|edit| (edit.gear, edit.channel) == (target, channel))
            .copied()
    };
    let empty = DyeTextureEdit::new(None, channel);
    let edit = find(Some(gear))
        .unwrap_or(empty)
        .over(find(None).unwrap_or(empty));
    let edit = DyeTextureEdit { gear: None, ..edit };
    (!edit.is_empty()).then_some(edit)
}

/// Checks that each dye's textures are edited once for every gear type and once for each gear
/// type, and that every texture names a tag.
pub fn validate_texture_edits(edits: &[DyeTextureEdit]) -> Result<(), String> {
    for (index, edit) in edits.iter().enumerate() {
        if edit.is_empty() {
            return Err(format!("The {} edit sets nothing", edit.describe()));
        }
        if edits[..index]
            .iter()
            .any(|other| (other.gear, other.channel) == (edit.gear, edit.channel))
        {
            return Err(format!("The {} are edited twice", edit.describe()));
        }
        if [edit.detail, edit.normal]
            .into_iter()
            .flatten()
            .any(|tag| matches!(tag, 0 | u32::MAX))
        {
            return Err(format!("The {} name no texture", edit.describe()));
        }
    }
    Ok(())
}

/// Checks that each surface is edited once for every gear type and once for each gear type, and
/// every iridescence is a lookup row or none.
pub fn validate_edits(edits: &[DyeEdit]) -> Result<(), String> {
    for (index, edit) in edits.iter().enumerate() {
        if edit.is_empty() {
            return Err(format!("The {} edit sets nothing", edit.describe()));
        }
        if edits[..index].iter().any(|other| {
            (other.gear, other.channel, other.surface) == (edit.gear, edit.channel, edit.surface)
        }) {
            return Err(format!("The {} is edited twice", edit.describe()));
        }
        if let Some(iridescence) = edit.iridescence
            && !(NO_IRIDESCENCE..IRIDESCENCE_ROWS).contains(&iridescence)
        {
            return Err(format!(
                "Iridescence {iridescence} is not a row of the game's lookup"
            ));
        }
        let fractions = [
            ("metalness", edit.metalness),
            ("detail strength", edit.detail),
            ("detail smoothness", edit.detail_smoothness),
            ("worn metalness", edit.worn_metalness),
        ]
        .into_iter()
        .chain(
            [
                ("smoothness", edit.smoothness),
                ("worn smoothness", edit.worn_smoothness),
            ]
            .into_iter()
            .flat_map(|(name, range)| {
                range
                    .into_iter()
                    .flatten()
                    .map(move |value| (name, Some(value)))
            }),
        );
        for (name, value) in fractions {
            if let Some(value) = value
                && !(0.0..=1.0).contains(&value.get())
            {
                return Err(format!(
                    "The {} {name} {} is not from 0 to 1",
                    edit.describe(),
                    value.get()
                ));
            }
        }
        if let Some(bumps) = edit.bumps
            && !(0.0..=4.0).contains(&bumps.get())
        {
            return Err(format!(
                "The {} bump strength {} is not from 0 to 4",
                edit.describe(),
                bumps.get()
            ));
        }
    }
    Ok(())
}

/// Converts an sRGB byte to the linear value a dye stores.
#[must_use]
pub fn srgb_to_linear(value: u8) -> f32 {
    let value = f32::from(value) / 255.0;
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// Converts a dye's linear value to an sRGB byte.
#[must_use]
pub fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Recipes write texture tags as `0x` and eight hex digits.
mod hex_tag {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(value: &Option<u32>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(tag) => serializer.serialize_str(&format!("0x{tag:08X}")),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<u32>, D::Error> {
        let Some(text) = Option::<String>::deserialize(deserializer)? else {
            return Ok(None);
        };
        text.strip_prefix("0x")
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .map(Some)
            .ok_or_else(|| D::Error::custom(format!("{text:?} is not a 0x texture tag")))
    }
}

/// Recipes write colors as `#RRGGBB`.
mod hex_color {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(
        value: &Option<[u8; 3]>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some([red, green, blue]) => {
                serializer.serialize_str(&format!("#{red:02X}{green:02X}{blue:02X}"))
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<[u8; 3]>, D::Error> {
        let Some(text) = Option::<String>::deserialize(deserializer)? else {
            return Ok(None);
        };
        let digits = text
            .strip_prefix('#')
            .filter(|digits| digits.len() == 6)
            .ok_or_else(|| D::Error::custom(format!("{text:?} is not a #RRGGBB color")))?;
        let channel =
            |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).map_err(D::Error::custom);
        Ok(Some([channel(0)?, channel(2)?, channel(4)?]))
    }
}
