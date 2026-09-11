use super::*;
use crate::presentation::{Artwork, Badge};

pub(super) fn artwork(seed: u8) -> Artwork {
    let image = image::RgbaImage::from_fn(96, 96, |x, y| {
        image::Rgba([
            seed,
            140,
            220,
            if (x + y + u32::from(seed)) % 23 < 11 {
                255
            } else {
                0
            },
        ])
    });
    let mut bytes = std::io::Cursor::new(vec![]);
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    Artwork::from_png(&bytes.into_inner()).unwrap()
}

#[test]
fn personalization_roundtrips_embedded_artwork_and_multiline_lore() {
    let mut recipe = crate::WeaponRecipe::every_end();
    let legacy = serde_json::to_string(&recipe).unwrap();
    assert!(!recipe.overrides.exclude_from_sunrise_badge);
    assert!(!legacy.contains("exclude_from_sunrise_badge"));
    recipe.overrides.exclude_from_sunrise_badge = true;
    assert!(!legacy.contains("corner_icon"));
    assert!(!legacy.contains("\"lore\""));
    recipe.overrides.badge = Some(Badge {
        name: "The Wanderers".into(),
        description: "A personal set.".into(),
        icon: Some(artwork(40)),
    });
    recipe.overrides.corner_icon = Some(artwork(90));
    recipe.overrides.lore = Some("One story.\n\nAnother chapter: 星 ✨".into());
    let decoded: crate::WeaponRecipe =
        serde_json::from_str(&serde_json::to_string(&recipe).unwrap()).unwrap();
    assert_eq!(decoded, recipe);
    let spec = decoded.to_spec().unwrap();
    assert!(spec.overrides.exclude_from_sunrise_badge);
    assert_eq!(spec.overrides.badge, recipe.overrides.badge);
    assert_eq!(spec.overrides.corner_icon, recipe.overrides.corner_icon);
    assert_eq!(spec.overrides.lore, recipe.overrides.lore);
    let values = project_authored_localized_values(&[spec.clone()], &[], 0)
        .unwrap()
        .into_iter()
        .map(|(h, v)| (h, v.to_owned()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        values[&crate::presentation::text_hash(&spec.namespace, "lore")],
        recipe.overrides.lore.unwrap()
    );
    assert!(Artwork::from_png(b"not a PNG").is_err());
}

#[test]
fn personal_badges_share_text_but_reject_conflicting_settings_and_over_capacity() {
    let mut first =
        crate::WeaponRecipe::new_named_weapon_for_donor("First Story", 0xA25B8F8F, "Arc Logic")
            .unwrap()
            .to_spec()
            .unwrap();
    first.overrides.badge = Some(Badge {
        name: "Shared Set".into(),
        ..Default::default()
    });
    let mut second =
        crate::WeaponRecipe::new_named_weapon_for_donor("Second Story", 0xA25B8F8F, "Arc Logic")
            .unwrap()
            .to_spec()
            .unwrap();
    second.overrides.badge = first.overrides.badge.clone();
    let pair = [first.clone(), second.clone()];
    let values = project_authored_localized_values(&pair, &[], 0).unwrap();
    assert_eq!(
        values
            .iter()
            .filter(|(_, text)| *text == "Shared Set")
            .count(),
        1
    );
    second.overrides.badge.as_mut().unwrap().description = "Conflicting description".into();
    assert!(
        canonical_project_weapons(&WeaponProjectSpec {
            weapons: vec![first, second]
        })
        .is_err()
    );
    let weapons = (0..25)
        .map(|i| {
            let mut spec = crate::WeaponRecipe::new_named_weapon_for_donor(
                format!("Set Weapon {i}"),
                0xA25B8F8F,
                "Arc Logic",
            )
            .unwrap()
            .to_spec()
            .unwrap();
            spec.overrides.badge = Some(Badge {
                name: format!("Set {i}"),
                ..Default::default()
            });
            spec
        })
        .collect::<Vec<_>>();
    assert!(
        canonical_project_weapons(&WeaponProjectSpec {
            weapons: weapons[..24].to_vec()
        })
        .is_ok()
    );
    assert!(
        canonical_project_weapons(&WeaponProjectSpec { weapons })
            .unwrap_err()
            .to_string()
            .contains("24")
    );
}

#[test]
fn custom_lore_rejects_empty_oversized_and_control_text() {
    let mut recipe = crate::WeaponRecipe::every_end();
    for text in ["   ".to_owned(), "x\0y".to_owned(), "é".repeat(8193)] {
        recipe.overrides.lore = Some(text);
        assert!(recipe.validate().is_err());
    }
    recipe.overrides.lore = Some("é".repeat(8192));
    recipe.validate().unwrap();
}
