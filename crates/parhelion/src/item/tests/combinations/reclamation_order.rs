use super::*;

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn native_reclamation_order_uses_special_ammo() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("combinations");
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
    // A view whose packages something still holds open is kept for a later cleanup, the
    // same outcome the build reports as a warning rather than a failure.
    if let Err(error) = stock.close() {
        assert!(
            error.contains("preserved for a later cleanup attempt"),
            "{error}"
        );
    }
}
