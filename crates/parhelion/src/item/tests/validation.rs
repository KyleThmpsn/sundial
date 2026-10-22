use super::*;

#[test]
fn project_rejects_duplicate_namespaces_and_identity_domains_independently() {
    let first = project_weapon("parhelion.first", 1);
    let mut second = project_weapon("parhelion.second", 2);
    second.namespace = first.namespace.clone();
    assert!(
        canonical_project_weapons(&WeaponProjectSpec {
            weapons: vec![first.clone(), second]
        })
        .is_err()
    );

    for collision in 0..3 {
        let mut second = project_weapon("parhelion.second", 2);
        match collision {
            0 => second.identity.item_hash = first.identity.item_hash,
            1 => second.identity.collectible_hash = first.identity.collectible_hash,
            2 => second.identity.unlock_hash = first.identity.unlock_hash,
            _ => unreachable!(),
        }
        assert!(
            canonical_project_weapons(&WeaponProjectSpec {
                weapons: vec![first.clone(), second]
            })
            .is_err()
        );
    }
}

#[test]
fn native_power_cap_index_zero_is_authorable() {
    let mut overrides = WeaponCloneOverrides {
        power_cap_group: Some(0),
        ..Default::default()
    };
    validate_native_scalar_overrides(&overrides).unwrap();
    overrides.power_cap_group = None;
    overrides.power_cap_groups = Some(vec![0, 15]);
    validate_native_scalar_overrides(&overrides).unwrap();
    overrides.power_cap_groups = Some(Vec::new());
    assert!(validate_native_scalar_overrides(&overrides).is_err());
}
