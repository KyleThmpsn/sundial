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
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_ornament_resolves_as_an_icon_donor_with_its_own_container() {
    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
        .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages");
    let sources = crate::weapon::sources::load_project_sources(Path::new(&packages))
        .expect("clean stock sources should load");

    let weapon = crate::weapon::donors::resolve_icon_donor(
        &sources,
        &WeaponIconDonorReference {
            item_hash: SUNSHOT,
            expected_name: Some("Sunshot".to_owned()),
        },
    )
    .expect("the base weapon should resolve as its own icon donor");
    let ornament = crate::weapon::donors::resolve_icon_donor(
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

    let error = crate::weapon::donors::resolve_icon_donor(
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
        error.contains("Icon donor item resolves to \"Red Dwarf\""),
        "{error}"
    );
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_ornament_model_and_icon_survive_resolution_and_build() {
    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
        .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages");
    let packages = PathBuf::from(packages);
    let sources = crate::weapon::sources::load_project_sources(&packages)
        .expect("clean stock sources should load");
    let spec = ornament_appearance_spec("parhelion.ornament-appearance.integration");

    let resolved = resolve::resolve_project_weapons(&sources, &[spec.clone()])
        .expect("an ornament icon donor should resolve");
    let ornament = crate::weapon::donors::resolve_icon_donor(
        &sources,
        spec.icon_donor
            .as_ref()
            .expect("the spec selects an ornament"),
    )
    .expect("the ornament should resolve");
    assert_eq!(resolved[0].donor_icon_container, ornament.icon_container);
    assert_eq!(resolved[0].donor_icon_index, ornament.icon_index);
    assert_eq!(resolved[0].icon_template_item_index, ornament.item_index);

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
        red_dwarf_dye_rows(),
        "the staged weapon must keep the ornament's own colors"
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
