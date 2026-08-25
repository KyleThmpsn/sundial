//! Installed item-version metadata used by power-aware editors.

use super::super::{
    Catalog,
    package::{array_at, u16_at},
};

const ITEM_VERSION_CLASS: u32 = 0x8080_5921;
const MAX_ITEM_VERSIONS: usize = 16;

impl Catalog {
    /// Highest power cap declared by any version of this installed definition.
    ///
    /// A shared definition can represent both an original and a later reissue.
    /// Sunrise settings do not store a version selector, so the highest authored
    /// cap is the useful safe limit for generated instances of that definition.
    pub(crate) fn item_power_cap(&self, hash: u64) -> Option<i64> {
        self.item_package_metadata
            .get(&hash)
            .and_then(|metadata| metadata.power_cap)
            .map(i64::from)
    }
}

pub(in crate::catalog) fn item_power_cap(item: &[u8]) -> Option<u16> {
    (0..item.len().saturating_sub(16))
        .step_by(8)
        .filter_map(|descriptor| array_at(item, descriptor).ok())
        .filter(|(count, _, class)| {
            *class == ITEM_VERSION_CLASS && (1..=MAX_ITEM_VERSIONS).contains(count)
        })
        .flat_map(|(count, rows, _)| {
            (0..count).filter_map(move |index| {
                let group = u16_at(item, rows.checked_add(index.checked_mul(2)?)?).ok()?;
                power_cap_for_version_group(group)
            })
        })
        .max()
}

const fn power_cap_for_version_group(group: u16) -> Option<u16> {
    match group {
        // Versions through Season of the Undying share the first sunset cap.
        7 | 8 => Some(1_060),
        9 => Some(1_260),
        10 => Some(1_310),
        11 => Some(1_360),
        // Exotics, unknown groups, and other unsunset definitions use the build-wide fallback.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version_data(groups: &[u16]) -> Vec<u8> {
        let mut data = vec![0_u8; 40 + groups.len() * 2];
        data[0..8].copy_from_slice(&(groups.len() as u64).to_le_bytes());
        data[8..16].copy_from_slice(&16_i64.to_le_bytes());
        data[24..32].copy_from_slice(&(groups.len() as u64).to_le_bytes());
        data[32..36].copy_from_slice(&ITEM_VERSION_CLASS.to_le_bytes());
        for (index, group) in groups.iter().enumerate() {
            let offset = 40 + index * 2;
            data[offset..offset + 2].copy_from_slice(&group.to_le_bytes());
        }
        data
    }

    #[test]
    fn maps_installed_version_groups_to_their_caps() {
        for (group, expected) in [
            (0, None),
            (7, Some(1_060)),
            (8, Some(1_060)),
            (9, Some(1_260)),
            (10, Some(1_310)),
            (11, Some(1_360)),
            (12, None),
        ] {
            assert_eq!(power_cap_for_version_group(group), expected);
        }
    }

    #[test]
    fn reissued_definition_uses_its_highest_authored_cap() {
        assert_eq!(item_power_cap(&version_data(&[7, 11])), Some(1_360));
    }

    #[test]
    fn uncapped_or_unrecognized_definitions_have_no_item_cap() {
        assert_eq!(item_power_cap(&version_data(&[0])), None);
        assert_eq!(item_power_cap(&version_data(&[12])), None);
        assert_eq!(item_power_cap(&[0; 64]), None);
    }
}
