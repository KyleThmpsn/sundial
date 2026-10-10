//! Real-package checks that a stock ornament can lend its model and inventory icon.
//!
//! Ornaments are plugs, so they never appear in the donor catalog. Parhelion applies one through
//! the two appearance sources it already has: the translation-art override for the model and the
//! icon donor for the icon. These tests pin that an ornament item resolves in the icon-donor role.
use super::*;

/// Sunshot, its Red Dwarf ornament, and the appearance rows that ornament carries.
const SUNSHOT: u32 = 0xAD47_46D5;
const RED_DWARF: u32 = 0x6131_29F0;
const RED_DWARF_ARRANGEMENT: u16 = 900;
const RED_DWARF_LOCKED_DYES: [(i8, u16); 3] = [(4, 3702), (5, 3703), (6, 3704)];

fn red_dwarf_dye_rows() -> [Vec<WeaponDyeReferenceOverride>; 3] {
    [
        Vec::new(),
        Vec::new(),
        RED_DWARF_LOCKED_DYES
            .into_iter()
            .map(
                |(channel_index, dye_reference_index)| WeaponDyeReferenceOverride {
                    channel_index,
                    dye_reference_index,
                },
            )
            .collect(),
    ]
}

fn ornament_appearance_spec(namespace: &str) -> WeaponCloneSpec {
    WeaponCloneSpec {
        kind: crate::ItemKind::Weapon,
        namespace: namespace.to_owned(),
        donor_item_hash: SUNSHOT,
        expected_donor_name: Some("Sunshot".to_owned()),
        presentation_donor: None,
        icon_donor: Some(WeaponIconDonorReference {
            item_hash: RED_DWARF,
            expected_name: Some("Red Dwarf".to_owned()),
        }),
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        identity: WeaponCloneIdentity::from_namespace(namespace)
            .expect("test namespace should allocate"),
        text: WeaponCloneText {
            name: "Ornament Appearance".to_owned(),
            flavor: "Wears a stock ornament's model and icon.".to_owned(),
            source: "Source: integration test".to_owned(),
            ..WeaponCloneText::default()
        },
        overrides: WeaponCloneOverrides {
            art_arrangements: Some(vec![WeaponArtArrangementOverride {
                character_class: -1,
                arrangement: RED_DWARF_ARRANGEMENT,
            }]),
            render_dye_rows: Some(red_dwarf_dye_rows()),
            ..WeaponCloneOverrides::default()
        },
    }
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_ornament_resolves_as_an_icon_donor_with_its_own_container() {
    let packages = crate::test_support::stock_packages();
    let sources = crate::item::sources::load_project_sources(Path::new(&packages))
        .expect("clean stock sources should load");

    let weapon = crate::item::donors::resolve_icon_donor(
        &sources,
        &WeaponIconDonorReference {
            item_hash: SUNSHOT,
            expected_name: Some("Sunshot".to_owned()),
        },
    )
    .expect("the base weapon should resolve as its own icon donor");
    let ornament = crate::item::donors::resolve_icon_donor(
        &sources,
        &WeaponIconDonorReference {
            item_hash: RED_DWARF,
            expected_name: Some("Red Dwarf".to_owned()),
        },
    )
    .expect("a stock ornament should resolve as an icon donor");
    assert_ne!(
        ornament.icon_container, weapon.icon_container,
        "the ornament must contribute its own icon container"
    );
    assert_ne!(ornament.item_index, weapon.item_index);

    let error = crate::item::donors::resolve_icon_donor(
        &sources,
        &WeaponIconDonorReference {
            item_hash: RED_DWARF,
            expected_name: Some("Heretic Robe".to_owned()),
        },
    )
    .err()
    .expect("a mismatched ornament name should be rejected")
    .to_string();
    assert!(
        error.contains("Icon donor") && error.contains("Red Dwarf"),
        "{error}"
    );
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_ornament_model_and_icon_survive_resolution_and_build() {
    let packages = crate::test_support::stock_packages();
    let sources = crate::item::sources::load_project_sources(&packages)
        .expect("clean stock sources should load");
    let spec = ornament_appearance_spec("parhelion.ornament-appearance.integration");

    let resolved = resolve::resolve_project_weapons(&sources, std::slice::from_ref(&spec))
        .expect("an ornament icon donor should resolve");
    let ornament = crate::item::donors::resolve_icon_donor(
        &sources,
        spec.icon_donor
            .as_ref()
            .expect("the spec selects an ornament"),
    )
    .expect("the ornament should resolve");
    // The icon and its container come from the ornament, and the model and dyes below come from
    // it too. The dense presentation row does not: that row is the client's UI cache entry for the
    // item, carrying its presentation type and classification, so templating it from the ornament
    // made the authored weapon present as an ornament and draw as an empty tile in the grid.
    assert_eq!(resolved[0].donor_icon_container, ornament.icon_container);
    assert_eq!(resolved[0].donor_icon_index, ornament.icon_index);
    assert_eq!(
        resolved[0].icon_template_item_index, resolved[0].donor_item_index,
        "the presentation row templates on the weapon this is, not on the icon donor"
    );
    assert_ne!(
        resolved[0].icon_template_item_index, ornament.item_index,
        "the ornament must not supply the item's presentation row"
    );
    assert_ne!(
        resolved[0].icon_template_container, ornament.icon_container,
        "the presentation row is checked against its own item's container, not the ornament's"
    );

    let bundle = build_weapon_project(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![spec],
        },
    )
    .expect("a weapon wearing a stock ornament should build");

    let view = staged_view(&packages, ".parhelion-ornament-test-", &bundle);
    let manager = open_manager(&view.path().join("packages")).expect("staged view should open");
    let definition = read_tag(
        &manager,
        bundle.plan.weapons[0].definition_tag,
        "authored ornament weapon",
    )
    .expect("the authored definition should load from the staged package");
    assert_eq!(
        weapon_art_arrangements(&definition).unwrap(),
        vec![WeaponArtArrangementOverride {
            character_class: -1,
            arrangement: RED_DWARF_ARRANGEMENT,
        }],
        "the staged weapon must keep the ornament's model"
    );
    assert_eq!(
        weapon_render_dye_rows(&definition).unwrap(),
        [vec![], red_dwarf_dye_rows()[2].clone(), vec![]],
        "the staged weapon must keep the ornament's colors as shaderable defaults"
    );
    let base = read_tag(
        &manager,
        bundle.plan.weapons[0].template_definition_tag,
        "stock Sunshot",
    )
    .expect("the stock base should load unchanged");
    assert_ne!(
        weapon_art_arrangements(&base).unwrap(),
        weapon_art_arrangements(&definition).unwrap(),
        "the ornament must differ from the base weapon's model"
    );
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn crafted_exotics_and_borrowed_exotic_appearances_accept_later_shaders() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("shader-unlock");
    assert!(!output.exists(), "Use a fresh artifact directory");
    let mut exotic = ornament_appearance_spec("parhelion.shader-ready-exotic.integration");
    exotic.icon_donor = None;
    exotic.overrides = WeaponCloneOverrides::default();
    let mut borrowed = exotic.clone();
    borrowed.namespace = "parhelion.shader-ready-appearance.integration".to_owned();
    borrowed.identity = WeaponCloneIdentity::from_namespace(&borrowed.namespace).unwrap();
    borrowed.donor_item_hash = 0x2979_48F4;
    borrowed.expected_donor_name = Some("Trust".to_owned());
    borrowed.presentation_donor = Some(WeaponPresentationDonorReference {
        item_hash: SUNSHOT,
        expected_name: Some("Sunshot".to_owned()),
    });
    let ornament = ornament_appearance_spec("parhelion.shader-ready-ornament.integration");
    let exotic_hash = exotic.identity.item_hash;
    let borrowed_hash = borrowed.identity.item_hash;
    let ornament_hash = ornament.identity.item_hash;
    let bundle = build_weapon_project(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![exotic, borrowed, ornament],
        },
    )
    .unwrap();
    let view = staged_view(&packages, ".parhelion-shader-unlock-", &bundle);
    let manager = open_manager(&view.path().join("packages")).unwrap();
    let mut evidence = Vec::new();
    let sunshot = read_tag(
        &manager,
        bundle
            .plan
            .weapons
            .iter()
            .find(|plan| plan.item_hash == exotic_hash)
            .unwrap()
            .template_definition_tag,
        "stock Sunshot",
    )
    .unwrap();
    let source = weapon_render_dye_rows(&sunshot).unwrap();
    assert!(
        !source[2].is_empty(),
        "The native fixture must exercise locked colors"
    );
    for plan in &bundle.plan.weapons {
        let definition = read_tag(&manager, plan.definition_tag, "shader-ready weapon").unwrap();
        let dyes = weapon_render_dye_rows(&definition).unwrap();
        assert!(
            dyes[2].is_empty(),
            "No selected shader should be required to unlock dyes"
        );
        assert!(!dyes[1].is_empty(), "The base colors must survive");
        let expected = if plan.item_hash == ornament_hash {
            red_dwarf_dye_rows()
        } else {
            source.clone()
        };
        for row in &expected[2] {
            assert!(dyes[1].contains(row), "An Exotic base color was lost");
        }
        if plan.item_hash == borrowed_hash {
            assert_eq!(
                weapon_rarity(&definition).unwrap(),
                AuthoredWeaponRarity::Legendary
            );
            assert_eq!(
                weapon_art_arrangements(&definition).unwrap(),
                weapon_art_arrangements(&sunshot).unwrap()
            );
        }
        evidence.push(serde_json::json!({"item_hash":plan.item_hash,"dyes":format!("{dyes:?}")}));
    }
    fs::create_dir_all(&output).unwrap();
    bundle.write_new(&output).unwrap();
    fs::write(
        output.join("shader-unlock.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "items":evidence,"gameplay_verified":false
        }))
        .unwrap(),
    )
    .unwrap();
}
