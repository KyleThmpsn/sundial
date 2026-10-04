use super::*;

#[test]
fn source_items_may_have_no_flavor_text() {
    let mut text = project_weapon("parhelion.empty-flavor", 1).text;
    text.flavor.clear();
    validate_weapon_clone_text(&text).unwrap();
    text.flavor = "\0".to_owned();
    assert!(validate_weapon_clone_text(&text).is_err());
    text.flavor = " ".to_owned();
    assert!(validate_weapon_clone_text(&text).is_err());
    text.flavor.clear();
    text.name.clear();
    assert!(validate_weapon_clone_text(&text).is_err());
}

#[test]
fn project_rejects_empty_but_accepts_batches_above_thirty_two_weapons() {
    assert!(canonical_project_weapons(&WeaponProjectSpec { weapons: vec![] }).is_err());
    let weapons = (0..64)
        .map(|index| project_weapon(&format!("parhelion.weapon-{index}"), index + 1))
        .collect();
    let canonical = canonical_project_weapons(&WeaponProjectSpec { weapons }).unwrap();
    assert_eq!(canonical.len(), 64);
    let mut collision = canonical.clone();
    collision[63].identity.item_hash = collision[0].identity.item_hash;
    assert!(canonical_project_weapons(&WeaponProjectSpec { weapons: collision }).is_err());
}

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
fn exact_donor_inheritance_validates_without_loading_a_catalog() {
    let spec = project_weapon("parhelion.generic-donor", 0x6212_9AF7);
    spec.validate().unwrap();
    let temp = tempfile::tempdir().unwrap();
    validate_weapon_clone_specs_against_catalog(&temp.path().join("missing-install"), [&spec])
        .expect("an exact donor clone should skip loading the installed catalog");
}

#[test]
fn generic_clone_rejects_ambiguous_overrides_and_donor_collision() {
    let mut spec = project_weapon("parhelion.invalid-generic", 0x6212_9AF7);
    spec.overrides.investment_stats = vec![(15, 50), (15, 60)];
    assert!(spec.validate().is_err());

    spec.overrides.investment_stats.clear();
    spec.donor_item_hash = spec.identity.item_hash;
    assert!(spec.validate().is_err());
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
