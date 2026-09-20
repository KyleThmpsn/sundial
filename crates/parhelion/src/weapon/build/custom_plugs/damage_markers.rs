//! Preserve native fixed-element identities when their behavior is unchanged.
use super::*;

pub(super) fn keeps_stock_identity(perk: &WeaponSandboxPerkRuntimeOverride) -> bool {
    sundial::package_authoring::native_weapon::fixed_damage_marker(perk.source_perk_index).is_some()
        && perk.program.is_none()
        && perk.projectiles.is_empty()
        && perk.activation.is_none()
        && perk.runtime_values.is_empty()
        && perk.action_float_values.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(index: u16) -> WeaponSandboxPerkRuntimeOverride {
        WeaponSandboxPerkRuntimeOverride {
            source_perk_index: index,
            program: None,
            projectiles: Vec::new(),
            activation: None,
            runtime_values: Vec::new(),
            action_float_values: Vec::new(),
        }
    }

    #[test]
    fn only_unmodified_fixed_element_markers_keep_stock_identity() {
        for index in [83, 84, 85, 449, 450, 451] {
            assert!(keeps_stock_identity(&effect(index)));
        }
        for index in [203, 405, 421, 461, 462, 463, 464, 1178] {
            assert!(!keeps_stock_identity(&effect(index)));
        }
        let mut edited = effect(84);
        edited
            .action_float_values
            .push(WeaponSandboxPerkActionFloatOverride {
                node_type_handle: 0,
                node_occurrence: 0,
                value_pointer_offset: 0,
                value_type_handle: 0,
                expected_bits: 0,
                value_bits: 1.0_f32.to_bits(),
            });
        assert!(!keeps_stock_identity(&edited));
    }
}
