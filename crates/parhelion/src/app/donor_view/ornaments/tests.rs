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
