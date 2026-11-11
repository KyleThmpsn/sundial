//! Native option reference closure and composed preview, with configured package input.
use super::*;

fn identity_name(identity: &catalog::Identity) -> String {
    format!(
        "{} {}",
        ["Human", "Awoken", "Exo"][usize::from(identity.race)],
        ["Male", "Female"][usize::from(identity.gender)]
    )
}

#[test]
fn native_character_appearance_workflow_composes_all_identities() {
    let Some(packages) = std::env::var_os("SUNDIAL_APPEARANCE_PACKAGES") else {
        return;
    };
    let packages = std::path::PathBuf::from(packages);
    let catalog =
        Catalog::load(&packages).expect("Configured customization packages must be readable");
    assert_eq!(catalog.identities.len(), 6);
    let mut observations = Vec::new();
    let armor = native_armor(&packages);
    for identity in &catalog.identities {
        let mut appearance = catalog
            .default_appearance(identity.race, identity.gender)
            .unwrap();
        capture_character(&packages, identity, "default", &appearance, &[], true);
        for group in &identity.groups {
            if let Some(option) = group.options.len().checked_sub(1) {
                catalog
                    .choose(&mut appearance, group.field, option)
                    .unwrap();
            }
        }
        capture_character(&packages, identity, "edited", &appearance, &[], true);
        let request = crate::model_preview::character::Character {
            appearance: appearance.clone(),
            armor: armor.clone(),
            portrait: false,
        };
        let model = crate::model_preview::character::load_reported(
            &packages,
            &request,
            &crate::model_preview::Load::default(),
        )
        .unwrap();
        assert!(
            !model.triangles.is_empty(),
            "{} must have geometry",
            identity_name(identity)
        );
        if let Some(output) = crate::test_support::artifacts(&format!(
            "character-{}-{}.png",
            identity.race, identity.gender
        )) {
            let image = crate::model_preview::render::styled_image(
                &model,
                crate::model_preview::render::Camera {
                    yaw: -std::f32::consts::FRAC_PI_2,
                    pitch: 0.06,
                    ..Default::default()
                },
                Default::default(),
                [400, 600],
                0.0,
                Default::default(),
            );
            let pixels = image
                .pixels
                .iter()
                .flat_map(eframe::egui::Color32::to_array)
                .collect::<Vec<_>>();
            std::fs::write(
                output,
                crate::model_preview::export::png(&pixels, 400, 600).unwrap(),
            )
            .unwrap();
        }
        assert!(model.vertices.iter().flatten().all(|v| v.is_finite()));
        assert!(
            model
                .triangles
                .iter()
                .flatten()
                .all(|&v| (v as usize) < model.vertices.len())
        );
        observations.push(
            serde_json::json!({"identity":identity_name(identity),"header":appearance.header(),
            "vertices":model.vertices.len(),"triangles":model.triangles.len(),"tags":model.tags,
            "notices":model.notices}),
        );
    }
    if let Some(output) = crate::test_support::artifacts("character-appearance-native.json") {
        std::fs::write(output, serde_json::to_vec_pretty(&observations).unwrap()).unwrap();
    }
}

// These captures are review artifacts, not render comparisons. Each recipe records its native
// inputs so the visible skin, hair coverage, markings and eyes can be inspected independently.
fn capture_character(
    packages: &std::path::Path,
    identity: &catalog::Identity,
    recipe: &str,
    appearance: &Appearance,
    armor: &[crate::model_preview::appearance::Appearance],
    portrait: bool,
) {
    let Some(output) = crate::test_support::artifacts(&format!(
        "character-{}-{}-{recipe}.png",
        identity.race, identity.gender
    )) else {
        return;
    };
    let request = crate::model_preview::character::Character {
        appearance: appearance.clone(),
        armor: armor.to_vec(),
        portrait,
    };
    let model = crate::model_preview::character::load_reported(
        packages,
        &request,
        &crate::model_preview::Load::default(),
    )
    .unwrap();
    for (angle, yaw) in [
        ("", -std::f32::consts::FRAC_PI_2),
        ("-angle", -std::f32::consts::FRAC_PI_6),
    ] {
        let image = crate::model_preview::render::styled_image(
            &model,
            crate::model_preview::render::Camera {
                yaw,
                pitch: 0.06,
                ..Default::default()
            },
            Default::default(),
            [600, 700],
            0.0,
            Default::default(),
        );
        let pixels = image
            .pixels
            .iter()
            .flat_map(eframe::egui::Color32::to_array)
            .collect::<Vec<_>>();
        let image_output = output.with_file_name(format!(
            "{}{angle}.png",
            output.file_stem().unwrap().to_string_lossy()
        ));
        std::fs::write(
            &image_output,
            crate::model_preview::export::png(&pixels, 600, 700).unwrap(),
        )
        .unwrap();
        std::fs::write(
            image_output.with_extension("json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "identity":identity_name(identity), "recipe":recipe, "header":appearance.header(),
                "portrait":portrait, "yaw":yaw, "models":model.tags, "notices":model.notices
            }))
            .unwrap(),
        )
        .unwrap();
    }
}

/// Follow the stock account fixture's equipped armor into the native item definitions. These
/// inputs are independent of the appearance page's catalog and plug-selection projection.
pub(crate) fn native_armor(
    packages: &std::path::Path,
) -> Vec<crate::model_preview::appearance::Appearance> {
    use crate::package_payload::*;
    let manager = crate::investment::discovery::open_packages(packages).unwrap();
    let globals = manager
        .read_tag(
            crate::package_runtime::resolve_live_named_tag(&manager, "investment_globals", None)
                .unwrap(),
        )
        .unwrap();
    let root = manager.read_tag(u32_at(&globals, 16).unwrap()).unwrap();
    let table = manager
        .read_tag(u32_at(&root, 8 + 48 * 16).unwrap())
        .unwrap();
    let (count, _, rows, _) = native_array_at(&table, 8).unwrap();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sunrise-v8-d0fe8886-defaults.json"
    ))
    .unwrap();
    ["gauntlets", "chest", "legs", "class_item"]
        .into_iter()
        .map(|slot| {
            let text = fixture["state"]["characters"][0]["equipment"][slot]["definition_hash"]
                .as_str()
                .unwrap();
            let hash = crate::hash::parse_hash_hex(text).unwrap() as u32;
            let row = (0..count)
                .map(|i| rows + i * 24)
                .find(|&r| u32_at(&table, r).unwrap() == hash)
                .expect("Stock fixture armor must resolve");
            let item = manager.read_tag(u32_at(&table, row + 16).unwrap()).unwrap();
            let block = relative_offset(0x88, 0, i64_at(&item, 0x88).unwrap()).unwrap();
            let (count, _, rows, _) = native_array_at(&item, block).unwrap();
            let art = (0..count)
                .map(|i| rows + i * 4)
                .find(|&r| matches!(item[r] as i8, -1 | 1))
                .unwrap();
            crate::model_preview::appearance::Appearance {
                arrangement: u16_at(&item, art + 2).unwrap(),
                dyes: crate::model_preview::local::stock_default_dyes(&item),
                dye_textures: Vec::new(),
            }
        })
        .collect()
}
