use super::*;

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES, PARHELION_COMBINATION_ROOT and PARHELION_STRESS_RECIPES with 128 recipes"]
fn native_large_mixed_batch_is_permutation_identical() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let output = PathBuf::from(std::env::var_os("PARHELION_COMBINATION_ROOT").unwrap());
    fs::create_dir_all(&output).unwrap();
    let source = PathBuf::from(std::env::var_os("PARHELION_STRESS_RECIPES").unwrap());
    let recipes: Vec<WeaponRecipe> = serde_json::from_slice(&fs::read(source).unwrap()).unwrap();
    assert_eq!(recipes.len(), 128);
    assert_eq!(
        recipes
            .iter()
            .map(|recipe| &recipe.namespace)
            .collect::<BTreeSet<_>>()
            .len(),
        128
    );
    let stock = open_manager(&packages).unwrap();
    let source_globals = Tables::read(&stock).globals;
    let mut identities = private::PrivateIdentities::default();
    let (build, ammo) = run_batch(
        &packages,
        &output,
        "mixed-128",
        &recipes,
        |manager, tables, recipe| {
            let ammo = verify_fields(manager, tables, recipe);
            private::verify_private(
                manager,
                tables,
                &stock,
                &source_globals,
                recipe,
                &mut identities,
            );
            ammo
        },
    );
    assert!(identities.plugs.len() >= 12);
    let reverse = build_weapon_project(
        &packages,
        &WeaponProjectSpec {
            weapons: recipes
                .iter()
                .rev()
                .map(|recipe| recipe.to_spec().unwrap())
                .collect(),
        },
    )
    .unwrap();
    assert_eq!(reverse.artifacts.len(), build.artifacts.len());
    for artifact in &reverse.artifacts {
        assert_eq!(
            artifact.bytes(),
            fs::read(build.run_directory.join(&artifact.plan.output_file_name)).unwrap()
        );
    }
    eprintln!(
        "MIXED_STRESS_PASS weapons=128 private_plugs={} ammo_variants={ammo} permutation_identical=true stage={}",
        identities.plugs.len(),
        build.run_directory.display()
    );
}
