use super::*;

fn weapon_with_perk(perk: &PerkRecipe) -> crate::WeaponRecipe {
    let mut weapon = crate::WeaponRecipe::new_weapon("parhelion.perk-validation").unwrap();
    weapon.overrides.socket_columns = vec![Some(crate::WeaponSocketColumnRecipe {
        socket_type: Some(92),
        choices: vec![perk.template_plug.clone()],
        ..Default::default()
    })];
    weapon.overrides.socket_plug_variants = vec![perk.at_socket(0, 0)];
    weapon
}

#[test]
fn library_perks_with_optional_descriptions_remain_saveable_after_attachment() {
    let temp = tempfile::tempdir().unwrap();
    let library = library::Library::open(temp.path().to_owned()).unwrap();
    for description in ["", " \t\n", "  Authored description with spacing.  "] {
        let mut perk = PerkRecipe::new();
        perk.description = description.into();
        let entry = library.save(&perk, None).unwrap();
        let loaded = library::Library::read(&entry.path).unwrap();
        assert_eq!(loaded.recipe.description, description);
        let weapon = weapon_with_perk(&loaded.recipe);
        let expected = (!description.trim().is_empty()).then_some(description);
        assert_eq!(
            weapon.overrides.socket_plug_variants[0]
                .description
                .as_deref(),
            expected
        );
        let json = weapon.to_json_pretty().unwrap();
        assert_eq!(crate::WeaponRecipe::from_json_str(&json).unwrap(), weapon);
        weapon.to_spec().unwrap().validate().unwrap();
    }
}

#[test]
fn standalone_perks_reject_invalid_compiler_inputs_before_library_save() {
    let temp = tempfile::tempdir().unwrap();
    let library = library::Library::open(temp.path().to_owned()).unwrap();
    let mut cases = Vec::new();
    for name in ["", " \t", "Invalid\0name"] {
        let mut perk = PerkRecipe::new();
        perk.name = name.into();
        cases.push(perk);
    }
    for hash in [0, 0x811C_9DC5] {
        let mut perk = PerkRecipe::new();
        perk.template_plug = hash.into();
        cases.push(perk);
    }
    for hash in [0, u32::MAX, 0x811C_9DC5] {
        let mut perk = PerkRecipe::new();
        perk.classification = Some(hash.into());
        cases.push(perk);
    }
    let mut perk = PerkRecipe::new();
    perk.description = "Invalid\0text".into();
    cases.push(perk);
    let mut perk = PerkRecipe::new();
    perk.description = "x".repeat(usize::from(u16::MAX) + 1);
    cases.push(perk);
    let mut perk = PerkRecipe::new();
    perk.stats = vec![WeaponStatOverride {
        definition_index: 256,
        value: 1,
    }];
    cases.push(perk);
    let mut perk = PerkRecipe::new();
    perk.effects = vec![PerkRecipe::effect(405); 2];
    cases.push(perk);
    let mut perk = PerkRecipe::new();
    let mut effect = PerkRecipe::effect(405);
    effect.projectiles.push(
        sundial::package_authoring::sandbox_perk::projectile::Selection {
            source_graph: 0,
            donor_graph: 0x80BB_DAD4,
        },
    );
    perk.effects.push(effect);
    cases.push(perk);
    for perk in cases {
        assert!(
            perk.validate().is_err(),
            "Invalid standalone perk was accepted"
        );
        assert!(library.save(&perk, None).is_err());
        assert!(weapon_with_perk(&perk).validate().is_err());
        assert!(
            perk.validate_draft().is_ok(),
            "Incomplete work can still be restored as a draft"
        );
    }
    assert!(library.scan().unwrap().entries.is_empty());
}

#[test]
fn independent_perks_roundtrip_without_weapon_identity_and_attach_as_copies() {
    let mut perk = PerkRecipe::new();
    perk.name = "A New Effect".into();
    perk.effects.push(PerkRecipe::effect(1178));
    perk.effects[0].projectiles.push(
        sundial::package_authoring::sandbox_perk::projectile::Selection {
            source_graph: 0x815282E1,
            donor_graph: 0x80BBDAD4,
        },
    );
    let json = serde_json::to_string(&perk).unwrap();
    assert!(!json.contains("socket_index") && !json.contains("weapon"));
    assert_eq!(serde_json::from_str::<PerkRecipe>(&json).unwrap(), perk);
    let attached = perk.at_socket(4, 2);
    assert!(attached.replace_effects);
    assert_eq!((attached.socket_index, attached.choice_index), (4, 2));
    perk.effects.clear();
    assert_eq!(attached.sandbox_perks.len(), 1);
    assert!(PerkRecipe::new().effects.is_empty());
}

#[test]
fn library_preserves_concurrent_saves_and_rejects_unsafe_ids() {
    let temp = tempfile::tempdir().unwrap();
    let library = library::Library::open(temp.path().to_owned()).unwrap();
    let mut perk = PerkRecipe::new();
    let first = library.save(&perk, None).unwrap();
    perk.description = "A newer version".into();
    let second = library.save(&perk, Some(&first.baseline)).unwrap();
    assert!(library.save(&first.recipe, Some(&first.baseline)).is_err());
    assert_eq!(
        library::Library::read(&first.path).unwrap().baseline,
        second.baseline
    );
    assert_eq!(library.scan().unwrap().entries.len(), 1);
    perk.id = "../outside".into();
    assert!(library.save(&perk, None).is_err());
}
