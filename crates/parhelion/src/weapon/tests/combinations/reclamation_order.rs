use super::*;

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES and PARHELION_COMBINATION_ROOT"]
fn native_reclamation_order_uses_special_ammo() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let output = PathBuf::from(std::env::var_os("PARHELION_COMBINATION_ROOT").unwrap());
    fs::create_dir_all(&output).unwrap();
    let recipe = WeaponRecipe::from_json_str(include_str!(
        "../../../../recipes/reclamation-order.parhelion.json"
    ))
    .unwrap();
    assert_eq!(recipe.overrides.ammo_type, Some(RecipeAmmoType::Special));
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let stock = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    // The donor catalog validates the install layout as well as its package directory.
    fs::hard_link(
        packages.parent().unwrap().join("destiny2.exe"),
        stock.path().parent().unwrap().join("destiny2.exe"),
    )
    .unwrap();
    let (_, variants) = run_batch(
        stock.path(),
        &output,
        "reclamation-special",
        &[recipe],
        verify_weapon,
    );
    assert!(variants > 0, "Native ammo properties must be inspected");
    fs::remove_file(stock.path().parent().unwrap().join("destiny2.exe")).unwrap();
    stock.close().unwrap();
}
