use super::*;

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
