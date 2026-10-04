use super::*;
use sundial::investment::{WeaponArtArrangement, WeaponDyeReference};

fn ornament(hash: u32, name: &str, arrangement: u16) -> WeaponOrnament {
    WeaponOrnament {
        hash,
        name: name.to_owned(),
        rarity: sundial::investment::WeaponRarity::Exotic,
        socket_index: 4,
        art_arrangements: vec![WeaponArtArrangement {
            character_class: -1,
            arrangement,
        }],
        icon_container_tag: Some(0x8132_C7BD),
        render_dye_rows: [
            Vec::new(),
            Vec::new(),
            vec![WeaponDyeReference {
                channel_index: 4,
                dye_reference_index: arrangement + 1,
            }],
        ],
    }
}

/// An ornament offered by one weapon, as the chooser hands it to the recipe.
fn wearable(hash: u32, name: &str, arrangement: u16, host: u32) -> WeaponOrnamentAppearance {
    WeaponOrnamentAppearance {
        ornament: ornament(hash, name, arrangement),
        host_item_hash: host,
        host_name: format!("Host {host:08X}"),
    }
}

const GAMEPLAY: u32 = 0x30F1_D32B;
const OTHER: u32 = 0xAA45_882A;

fn presentation(recipe: &WeaponRecipe) -> Option<u32> {
    recipe
        .presentation_donor
        .as_ref()
        .and_then(|donor| donor.item_hash.parse_u32().ok())
}

fn recipe() -> WeaponRecipe {
    WeaponRecipe::new_weapon("parhelion.ornament-tests").expect("test recipe should allocate")
}

#[test]
fn applying_an_ornament_takes_its_model_rows_and_icon() {
    let ornaments = [ornament(0xCC92_C7C1, "Heretic Robe", 3610)];
    let mut recipe = recipe();
    assert!(applied(&recipe, &ornaments).is_none());

    apply(&mut recipe, &ornaments[0]);

    assert_eq!(
        recipe.overrides.art_arrangements,
        Some(vec![WeaponArtArrangementRecipe {
            character_class: -1,
            arrangement: 3610,
        }])
    );
    assert_eq!(
        recipe
            .overrides
            .render_dye_rows
            .as_ref()
            .map(|rows| rows[2].clone()),
        Some(vec![WeaponDyeReferenceRecipe {
            channel_index: 4,
            dye_reference_index: 3611,
        }]),
        "the ornament's own locked colors come with its model"
    );
    let icon = recipe
        .icon_donor
        .as_ref()
        .expect("icon donor should be set");
    assert_eq!(icon.item_hash.parse_u32(), Ok(0xCC92_C7C1));
    assert_eq!(icon.expected_name.as_deref(), Some("Heretic Robe"));
    assert_eq!(
        recipe.overrides.icon_edit.cleared_color,
        Some([0xF2, 0xE3, 0x70]),
        "the exotic ornament plate is cleared from the authored icon"
    );
    assert_eq!(
        applied(&recipe, &ornaments).map(|ornament| ornament.hash),
        Some(0xCC92_C7C1)
    );
}

#[test]
fn choosing_another_ornament_replaces_the_previous_one() {
    let ornaments = [
        ornament(0xCC92_C7C1, "Heretic Robe", 3610),
        ornament(0xCC3D_C725, "Wishes of Sorrow", 3111),
    ];
    let mut recipe = recipe();

    apply(&mut recipe, &ornaments[0]);
    apply(&mut recipe, &ornaments[1]);

    assert_eq!(
        applied(&recipe, &ornaments).map(|ornament| ornament.hash),
        Some(0xCC3D_C725)
    );
}

#[test]
fn restoring_clears_only_what_the_ornament_contributed() {
    let ornaments = [ornament(0xCC92_C7C1, "Heretic Robe", 3610)];
    let mut recipe = recipe();

    apply(&mut recipe, &ornaments[0]);
    restore(&mut recipe, &ornaments[0]);

    assert_eq!(recipe.overrides.art_arrangements, None);
    assert_eq!(recipe.overrides.render_dye_rows, None);
    assert_eq!(recipe.icon_donor, None);
    assert_eq!(recipe.overrides.icon_edit.cleared_color, None);

    apply(&mut recipe, &ornaments[0]);
    recipe.icon_donor = Some(WeaponDonorReference {
        item_hash: 0xAD47_46D5_u32.into(),
        expected_name: Some("Sunshot".to_owned()),
    });
    restore(&mut recipe, &ornaments[0]);

    assert_eq!(recipe.overrides.art_arrangements, None);
    assert_eq!(
        recipe
            .icon_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok()),
        Some(0xAD47_46D5),
        "an icon chosen after the ornament stays selected"
    );

    apply(&mut recipe, &ornaments[0]);
    recipe.overrides.icon_edit.hue_shift_degrees = 40;
    let edited = [Vec::new(), Vec::new(), Vec::new()];
    recipe.overrides.render_dye_rows = Some(edited.clone());
    restore(&mut recipe, &ornaments[0]);

    assert_eq!(
        recipe.overrides.render_dye_rows,
        Some(edited),
        "colors edited after the ornament stay in the recipe"
    );
    assert_eq!(
        recipe.overrides.icon_edit.hue_shift_degrees, 40,
        "icon edits made after the ornament stay in the recipe"
    );
}

#[test]
fn hand_edited_art_rows_are_not_reported_as_an_ornament() {
    let ornaments = [ornament(0xCC92_C7C1, "Heretic Robe", 3610)];
    let mut recipe = recipe();
    apply(&mut recipe, &ornaments[0]);
    recipe.overrides.art_arrangements = Some(vec![WeaponArtArrangementRecipe {
        character_class: -1,
        arrangement: 42,
    }]);

    assert!(applied(&recipe, &ornaments).is_none());
}

#[test]
fn an_ornament_without_model_rows_lends_only_its_icon() {
    let mut icon_only = ornament(0x1234_5678, "Icon Only", 0);
    icon_only.art_arrangements.clear();
    let ornaments = [icon_only];
    let mut recipe = recipe();

    apply(&mut recipe, &ornaments[0]);

    assert_eq!(recipe.overrides.art_arrangements, None);
    assert_eq!(
        applied(&recipe, &ornaments).map(|ornament| ornament.hash),
        Some(0x1234_5678)
    );
}

#[test]
fn an_ornament_from_another_weapon_brings_that_weapon_as_the_appearance() {
    let offered = [wearable(0xCC92_C7C1, "Still Hungry", 3119, OTHER)];
    let mut recipe = recipe();
    assert!(applied_appearance(&recipe, &offered, GAMEPLAY).is_none());

    apply_appearance(&mut recipe, &offered[0], GAMEPLAY);

    assert_eq!(
        presentation(&recipe),
        Some(OTHER),
        "the ornament carries no rig, so its own weapon has to lend one"
    );
    assert_eq!(
        recipe.presentation_donor.as_ref().unwrap().expected_name,
        Some(format!("Host {OTHER:08X}"))
    );
    assert_eq!(
        recipe.overrides.art_arrangements,
        Some(vec![WeaponArtArrangementRecipe {
            character_class: -1,
            arrangement: 3119,
        }]),
        "naming the appearance clears the overrides, so the rows must be written after it"
    );
    assert_eq!(
        recipe
            .icon_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok()),
        Some(0xCC92_C7C1)
    );
    assert_eq!(
        applied_appearance(&recipe, &offered, GAMEPLAY).map(|worn| worn.ornament.hash),
        Some(0xCC92_C7C1)
    );
}

#[test]
fn an_ornament_of_the_authored_weapon_leaves_the_appearance_inherited() {
    // Its rig is already the gameplay donor's, and naming the base as its own appearance would
    // be refused as an appearance equal to the base.
    let offered = [wearable(0xCC92_C7C1, "Still Hungry", 3119, GAMEPLAY)];
    let mut recipe = recipe();

    apply_appearance(&mut recipe, &offered[0], GAMEPLAY);

    assert_eq!(presentation(&recipe), None);
    assert!(recipe.overrides.art_arrangements.is_some());
}

#[test]
fn an_icon_only_ornament_does_not_move_the_appearance() {
    let mut offered = wearable(0x1234_5678, "Icon Only", 0, OTHER);
    offered.ornament.art_arrangements.clear();
    let mut recipe = recipe();

    apply_appearance(&mut recipe, &offered, GAMEPLAY);

    assert_eq!(presentation(&recipe), None, "an icon needs no rig");
    assert_eq!(recipe.overrides.art_arrangements, None);
}

#[test]
fn removing_an_icon_only_ornament_keeps_an_independent_appearance_donor() {
    let mut offered = wearable(0x1234_5678, "Icon Only", 0, OTHER);
    offered.ornament.art_arrangements.clear();
    let mut recipe = recipe();
    recipe.set_presentation_donor(Some(WeaponDonorReference {
        item_hash: OTHER.into(),
        expected_name: None,
    }));

    apply_appearance(&mut recipe, &offered, GAMEPLAY);
    restore_appearance(&mut recipe, &offered, GAMEPLAY);

    assert_eq!(presentation(&recipe), Some(OTHER));
}

#[test]
fn dropping_a_borrowed_ornament_releases_the_weapon_that_lent_it() {
    let offered = [wearable(0xCC92_C7C1, "Still Hungry", 3119, OTHER)];
    let mut recipe = recipe();
    apply_appearance(&mut recipe, &offered[0], GAMEPLAY);

    restore_appearance(&mut recipe, &offered[0], GAMEPLAY);

    assert_eq!(
        presentation(&recipe),
        None,
        "without the ornament nobody chose that weapon's plain model"
    );
    assert_eq!(recipe.overrides.art_arrangements, None);
    assert_eq!(recipe.icon_donor, None);
}

#[test]
fn restoring_an_ornament_of_the_current_base_clears_its_art_rows() {
    let offered = [wearable(0xCC92_C7C1, "Still Hungry", 3119, OTHER)];
    let mut recipe = recipe();
    apply_appearance(&mut recipe, &offered[0], OTHER);

    restore_appearance(&mut recipe, &offered[0], OTHER);

    assert_eq!(recipe.overrides.art_arrangements, None);
}

#[test]
fn the_worn_ornament_reported_is_the_one_whose_weapon_lends_the_rig() {
    // Two ornaments select the same model. The label has to name the one in force.
    let offered = [
        wearable(0xCC92_C7C1, "Still Hungry", 3119, OTHER),
        wearable(0xE5E4_3E1F, "Same Model", 3119, GAMEPLAY),
    ];
    let mut recipe = recipe();
    apply_appearance(&mut recipe, &offered[0], GAMEPLAY);

    assert_eq!(
        applied_appearance(&recipe, &offered, GAMEPLAY).map(|worn| worn.ornament.hash),
        Some(0xCC92_C7C1)
    );

    apply_appearance(&mut recipe, &offered[1], GAMEPLAY);

    assert_eq!(
        applied_appearance(&recipe, &offered, GAMEPLAY).map(|worn| worn.ornament.hash),
        Some(0xE5E4_3E1F)
    );
}
