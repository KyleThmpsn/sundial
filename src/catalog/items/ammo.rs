use serde::{Deserialize, Serialize};

use crate::{
    investment_schema::{
        ITEM_STRING_AMMO_CLASS, ITEM_STRING_AMMO_CLASS_OFFSET, ITEM_STRING_AMMO_TYPE_OFFSET,
    },
    package_payload::{u16_at, u32_at},
};

/// Client-facing ammunition classification stored in the per-item string definition.
///
/// This is independent from the Kinetic/Energy/Power inventory bucket and from the runtime
/// magazine component. Zero is used by a small number of stock items as an inherited/unspecified
/// value and therefore decodes as `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ItemWeaponAmmoType {
    Primary,
    Special,
    Heavy,
}

impl ItemWeaponAmmoType {
    pub(crate) const ALL: [Self; 3] = [Self::Primary, Self::Special, Self::Heavy];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Special => "Special",
            Self::Heavy => "Heavy",
        }
    }

    const fn from_package_value(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::Primary),
            2 => Some(Self::Special),
            3 => Some(Self::Heavy),
            _ => None,
        }
    }
}

pub(in crate::catalog) fn item_weapon_ammo_type(
    string_definition: &[u8],
) -> Option<ItemWeaponAmmoType> {
    (u32_at(string_definition, ITEM_STRING_AMMO_CLASS_OFFSET).ok()? == ITEM_STRING_AMMO_CLASS)
        .then(|| u16_at(string_definition, ITEM_STRING_AMMO_TYPE_OFFSET).ok())
        .flatten()
        .and_then(ItemWeaponAmmoType::from_package_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ammo_type_requires_the_typed_client_field() {
        let mut definition = vec![0_u8; ITEM_STRING_AMMO_TYPE_OFFSET + 2];
        definition[ITEM_STRING_AMMO_CLASS_OFFSET..ITEM_STRING_AMMO_CLASS_OFFSET + 4]
            .copy_from_slice(&ITEM_STRING_AMMO_CLASS.to_le_bytes());

        for (value, expected) in [
            (1_u16, Some(ItemWeaponAmmoType::Primary)),
            (2_u16, Some(ItemWeaponAmmoType::Special)),
            (3_u16, Some(ItemWeaponAmmoType::Heavy)),
            (0_u16, None),
            (4_u16, None),
        ] {
            definition[ITEM_STRING_AMMO_TYPE_OFFSET..ITEM_STRING_AMMO_TYPE_OFFSET + 2]
                .copy_from_slice(&value.to_le_bytes());
            assert_eq!(item_weapon_ammo_type(&definition), expected);
        }

        definition[ITEM_STRING_AMMO_CLASS_OFFSET] ^= 1;
        assert_eq!(item_weapon_ammo_type(&definition), None);
        assert_eq!(
            item_weapon_ammo_type(&definition[..ITEM_STRING_AMMO_TYPE_OFFSET]),
            None
        );
    }
}
