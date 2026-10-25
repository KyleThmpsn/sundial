//! Fresh source conversion must retain the dependency through portable reload and staging.
use super::*;
use parhelion_import::{
    d1_roi::{NativeOptions, Source},
    d2_mot::service,
};

fn configured(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("Configure {name}"))
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, SUNDIAL_TEST_ARTIFACTS, PARHELION_D1_ROI_PACKAGES, PARHELION_D1_XG and PARHELION_IMPORT_MODERN_PACKAGES"]
fn fresh_d1_and_d2_imports_carry_bundled_projectiles_into_staged_weapons() {
    let native = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("imported-wolfpack");
    fs::create_dir(&output).unwrap();
    let source = Source::open(&configured("PARHELION_D1_ROI_PACKAGES"), |_, _| {}).unwrap();
    let d1 = source
        .prepare_weapon(
            0xD471_D331,
            &native,
            0x0823_36CE,
            &output.join("d1"),
            &NativeOptions {
                xg_library: Some(configured("PARHELION_D1_XG")),
                ..Default::default()
            },
            &mut |message| eprintln!("{message}"),
        )
        .unwrap();
    let weapon = service::Weapon {
        hash: 0x514B_4081,
        name: "Gjallarhorn".into(),
        weapon_type: "Rocket Launcher".into(),
        ..Default::default()
    };
    let donors = serde_json::json!({"weapons":[{"hash":0x0823_36CEu32,
        "name":"Shining Sphere","weapon_type":"Rocket Launcher","present_in_native":true}]});
    let d2 = service::prepare_with_progress(
        &weapon,
        &configured("PARHELION_IMPORT_MODERN_PACKAGES"),
        &native,
        &donors,
        &output.join("d2"),
        &mut |message| eprintln!("{message}"),
    )
    .unwrap();
    let mut recipes = Vec::new();
    for (name, path) in [("d1", d1), ("d2", d2)] {
        let recipe = crate::WeaponRecipe::load_json(path).unwrap();
        let encoded = recipe.to_json_pretty().unwrap();
        let portable = output.join(format!("{name}.parhelion.json"));
        fs::write(&portable, encoded).unwrap();
        let recipe = crate::WeaponRecipe::load_json(portable).unwrap();
        assert!(
            recipe.overrides.fired_graph.is_none(),
            "conversion needs no external projectile graph"
        );
        assert!(
            recipe
                .overrides
                .socket_plug_variants
                .iter()
                .any(|variant| variant.projectile
                    == Some(crate::weapon::projectile::Preset::WolfpackRounds))
        );
        recipes.push(recipe);
    }
    stage(&native, &output, recipes);
}
