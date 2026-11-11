//! The native creation menu's six identity rows and typed option lists.
use super::Appearance;
use crate::{package_payload::*, package_runtime::reader::PackageManager};
use std::path::Path;
use tiger_pkg::TagHash;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Field {
    Face,
    Hair,
    Feature,
    Marking,
    Skin,
    Lips,
    Eye,
    HairColor,
    AccentColor,
    MarkColor,
}

impl Field {
    const ALL: [Self; 10] = [
        Self::Face,
        Self::Hair,
        Self::Feature,
        Self::Marking,
        Self::Skin,
        Self::Lips,
        Self::Eye,
        Self::HairColor,
        Self::AccentColor,
        Self::MarkColor,
    ];
    pub(super) fn layout(self) -> (usize, usize, bool) {
        match self {
            Self::Face => (0, 1, false),
            Self::Hair => (2, 1, true),
            Self::Feature => (3, 1, true),
            Self::Marking => (4, 1, true),
            Self::Skin => (6, 1, false),
            Self::Lips => (8, 1, false),
            Self::Eye => (10, 1, false),
            Self::HairColor => (12, 3, false),
            Self::AccentColor => (18, 3, false),
            Self::MarkColor => (24, 1, false),
        }
    }
    pub fn is_color(self) -> bool {
        matches!(
            self,
            Self::Skin | Self::Lips | Self::Eye | Self::HairColor | Self::MarkColor
        )
    }
}

pub(crate) struct Choice {
    pub value: [u16; 3],
    pub icon_container: Option<u32>,
    pub swatch: Option<[u8; 3]>,
}
pub(crate) struct Group {
    pub field: Field,
    pub label: String,
    pub options: Vec<Choice>,
    pub visible: bool,
}
pub(crate) struct Identity {
    pub race: u8,
    pub gender: u8,
    pub groups: Vec<Group>,
}
pub(crate) struct Catalog {
    pub identities: Vec<Identity>,
}

impl Catalog {
    pub fn load(packages: &Path) -> Result<Self, String> {
        let manager = crate::investment::discovery::open_packages(packages)?;
        Self::read(&manager)
    }

    pub fn read(manager: &PackageManager) -> Result<Self, String> {
        use crate::investment::{
            localization::{LocalizedStringCache, resolve_string},
            schema::*,
        };
        let globals = manager.read_tag(crate::package_runtime::resolve_live_named_tag(
            manager,
            "investment_globals",
            None,
        )?)?;
        let tag = investment_globals_table_tag(&globals, 11)?;
        let bytes = checked(manager, tag, 0x8080_5854)?;
        let (count, identity_rows) = rows(&bytes, 8, 0x8080_5858, 0x1B0, 6)?;
        if count != 6 {
            return Err(
                "The installed character customization table must contain all six identities"
                    .into(),
            );
        }
        let index = checked(
            manager,
            investment_globals_table_tag(&globals, GLOBALS_LOCALIZED_STRING_INDEX_TABLE_SLOT)?,
            0x8080_5F98,
        )?;
        let (n, start) = rows(&index, 8, 0x8080_5F9E, 8, 65536)?;
        let tags = (0..n)
            .map(|i| u32_at(&index, start + i * 8 + 4).map(TagHash))
            .collect::<Result<Vec<_>, _>>()?;
        let mut cache = LocalizedStringCache::default();
        let icons = crate::catalog::scan_item_icon_containers(manager, &globals)?;
        let mut identities = Vec::new();
        for i in 0..count {
            let row = identity_rows + i * 0x1B0;
            let race = bytes[row + 4];
            let gender = bytes[row + 5];
            if race > 2
                || gender > 1
                || identities
                    .iter()
                    .any(|v: &Identity| v.race == race && v.gender == gender)
            {
                return Err(
                    "The customization table contains an invalid or repeated identity".into(),
                );
            }
            let name = resolve_string(manager, &tags, &mut cache, &bytes, row + 12)
                .ok_or("Missing character identity name")?;
            let groups = read_groups(manager, &tags, &mut cache, &bytes, row, &name, &icons)?;
            identities.push(Identity {
                race,
                gender,
                groups,
            });
        }
        Ok(Self { identities })
    }

    pub fn identity(&self, race: u8, gender: u8) -> Result<&Identity, String> {
        self.identities
            .iter()
            .find(|v| v.race == race && v.gender == gender)
            .ok_or_else(|| "This character identity has no installed customization options".into())
    }
    pub fn default_appearance(&self, race: u8, gender: u8) -> Result<Appearance, String> {
        let mut header = [0; 36];
        header[28..32].copy_from_slice(&0x811C9DC5_u32.to_le_bytes());
        header[32] = 1;
        let mut value = Appearance::from_saved(race, gender, header)?;
        for group in &self.identity(race, gender)?.groups {
            value.set(
                group.field,
                group.options.first().map_or([u16::MAX; 3], |o| o.value),
            );
        }
        Ok(value)
    }
    pub fn reset(&self, value: &Appearance, race: u8, gender: u8) -> Result<Appearance, String> {
        let defaults = self.default_appearance(race, gender)?;
        let mut result = Appearance::from_saved(race, gender, value.header)?;
        for field in Field::ALL {
            result.set(field, defaults.value(field));
        }
        result.header[32] = 1;
        Ok(result)
    }
    pub fn choose(
        &self,
        value: &mut Appearance,
        field: Field,
        option: usize,
    ) -> Result<(), String> {
        let identity = self.identity(value.race, value.gender)?;
        let group = identity
            .groups
            .iter()
            .find(|g| g.field == field)
            .ok_or("Missing customization group")?;
        let choice = group
            .options
            .get(option)
            .ok_or("The selected appearance option is unavailable")?;
        value.set(field, choice.value);
        // Native hair/accent lists are parallel. The hidden eyebrow/accent follows hair color.
        if field == Field::HairColor
            && let Some(choice) = identity
                .groups
                .iter()
                .find(|g| g.field == Field::AccentColor)
                .and_then(|g| g.options.get(option))
        {
            value.set(Field::AccentColor, choice.value);
        }
        Ok(())
    }
    pub fn validate(&self, value: &Appearance) -> Result<(), String> {
        for group in &self.identity(value.race, value.gender)?.groups {
            let selected = value.value(group.field);
            if !group.options.is_empty() && !group.options.iter().any(|o| o.value == selected) {
                return Err(format!(
                    "The saved {} selection is not in the installed character options",
                    if group.visible {
                        group.label.as_str()
                    } else {
                        "appearance"
                    }
                ));
            }
        }
        Ok(())
    }
}

fn read_groups(
    manager: &PackageManager,
    tags: &[TagHash],
    cache: &mut crate::investment::localization::LocalizedStringCache,
    bytes: &[u8],
    row: usize,
    name: &str,
    icons: &[Option<u32>],
) -> Result<Vec<Group>, String> {
    use crate::investment::localization::resolve_string;
    let mut groups = Vec::new();
    for (j, field) in Field::ALL.into_iter().enumerate() {
        let at = row + 0x30 + j * 0x20;
        let label = resolve_string(manager, tags, cache, bytes, at);
        let (class, stride) = match field {
            Field::Face => (0x8080_5867, 24),
            Field::Hair | Field::Feature | Field::Marking => (0x8080_585B, 24),
            Field::HairColor | Field::AccentColor => (0x8080_5861, 28),
            _ => (0x8080_5864, 24),
        };
        let mut options = Vec::new();
        if u64_at(bytes, at + 16)? != 0 {
            let (n, start) = rows(bytes, at + 16, class, stride, 256)?;
            for k in 0..n {
                let op = start + k * stride;
                let mut value = [u16::MAX; 3];
                for (lane, v) in value.iter_mut().take(field.layout().1).enumerate() {
                    *v = u16_at(bytes, op + lane * 2)?;
                }
                if field.layout().2 && value[0] > 255 {
                    return Err("A byte customization option exceeds its native field".into());
                }
                options.push(Choice {
                    value,
                    icon_container: option_icon(bytes, op, field, icons)?,
                    swatch: None,
                });
            }
        }
        if options.is_empty() && field != Field::AccentColor {
            return Err(format!("{name} has no required customization options"));
        }
        groups.push(Group {
            field,
            visible: label.is_some(),
            label: label.unwrap_or_default(),
            options,
        });
    }
    fill_swatches(manager, &mut groups)?;
    Ok(groups)
}

fn option_icon(
    bytes: &[u8],
    option: usize,
    field: Field,
    icons: &[Option<u32>],
) -> Result<Option<u32>, String> {
    let offset = if field.layout().1 == 3 { 8 } else { 4 };
    let index = u16_at(bytes, option + offset)?;
    if index == u16::MAX {
        return Ok(None);
    }
    icons
        .get(usize::from(index))
        .copied()
        .ok_or_else(|| "A character option references a missing native icon row".into())
}

fn fill_swatches(manager: &PackageManager, groups: &mut [Group]) -> Result<(), String> {
    // A color's swatch uses the same installed dye as its model, never an invented palette.
    let requested = groups
        .iter()
        .filter(|g| g.field.is_color())
        .flat_map(|g| g.options.iter().map(|o| o.value[0]))
        .collect::<Vec<_>>();
    let colors = crate::dyes::load_with_manager(manager, &requested)?;
    for group in groups.iter_mut().filter(|g| g.field.is_color()) {
        for choice in &mut group.options {
            choice.swatch = colors
                .get(&choice.value[0])
                .and_then(|c| c.as_ref().ok())
                .map(|c| {
                    c.primary.map(|v| {
                        let v = v.clamp(0.0, 1.0);
                        ((if v <= 0.0031308 {
                            v * 12.92
                        } else {
                            1.055 * v.powf(1.0 / 2.4) - 0.055
                        }) * 255.0)
                            .round() as u8
                    })
                });
        }
    }
    Ok(())
}

pub(super) fn checked(manager: &PackageManager, tag: u32, class: u32) -> Result<Vec<u8>, String> {
    if manager.get_entry(tag).is_none_or(|e| e.reference != class) {
        return Err(format!(
            "Character resource {tag:08X} is not native class {class:08X}"
        ));
    }
    manager.read_tag(tag)
}
pub(super) fn rows(
    data: &[u8],
    at: usize,
    class: u32,
    stride: usize,
    max: usize,
) -> Result<(usize, usize), String> {
    let (n, _, start, actual) = native_array_at(data, at)?;
    if actual != class || n > max {
        return Err("Unsupported character customization array".into());
    }
    rows_fit(data, start, n, stride)?;
    Ok((n, start))
}
