use super::*;

/// Sunshot and the ornaments its own sockets offer.
const SUNSHOT: u32 = 0xAD47_46D5;

#[test]
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES; package-backed headless layout check"]
#[allow(clippy::cognitive_complexity)]
fn real_appearance_section_offers_and_applies_the_donor_ornaments() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap());
    let mut app = PackageAuthoringApp::default();
    let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
    app.donor_summaries = catalog.weapon_donors();
    app.catalog = Some(catalog);
    app.packages = packages;
    app.recipe =
        WeaponRecipe::new_weapon_for_donor("parhelion.ornament-layout", SUNSHOT, "Sunshot")
            .unwrap();

    let ornaments = app.catalog.as_ref().unwrap().weapon_ornaments(SUNSHOT);
    let ornament = ornaments
        .iter()
        .find(|ornament| ornament.name == "Red Dwarf")
        .expect("Sunshot should offer its stock ornaments");
    assert!(ornament.changes_model(&app.current_donor().unwrap().art_arrangements));

    let saved_recipe = serde_json::to_value(&app.recipe).unwrap();
    let mut candidate = app.recipe.clone();
    donor_view::ornaments::apply(&mut candidate, ornament);
    let catalog = app.catalog.as_ref().unwrap();
    let candidate_appearance =
        catalog.preview_appearance(&donor_view::preview::loadout(catalog, &candidate).unwrap());
    assert_eq!(serde_json::to_value(&app.recipe).unwrap(), saved_recipe);
    assert!(
        ornament
            .art_arrangements
            .iter()
            .any(|row| row.arrangement == candidate_appearance.arrangement)
    );

    let (output, overflow) = render(900.0, |ui| app.draw_appearance_workspace(ui));
    let rendered = text(&output);
    assert!(rendered.contains("Ornament"), "{rendered}");
    assert!(rendered.contains("Use Ornament"), "{rendered}");
    assert_eq!(overflow, 0.0, "the appearance workspace must fit its width");

    donor_view::ornaments::apply(&mut app.recipe, ornament);
    let catalog = app.catalog.as_ref().unwrap();
    assert_eq!(
        catalog.preview_appearance(&donor_view::preview::loadout(catalog, &app.recipe).unwrap()),
        candidate_appearance
    );
    let (output, overflow) = render(900.0, |ui| app.draw_appearance_workspace(ui));
    let rendered = text(&output);
    assert!(rendered.contains("Red Dwarf"), "{rendered}");
    assert!(rendered.contains("Default Appearance"), "{rendered}");
    assert_eq!(overflow, 0.0);
    assert_eq!(
        app.recipe
            .icon_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok()),
        Some(ornament.hash)
    );
    app.recipe
        .validate()
        .expect("an ornament recipe stays valid");

    // A different appearance restores its own model and icon, so the ornament cannot follow it.
    let saved_recipe = serde_json::to_value(&app.recipe).unwrap();
    let mut candidate = app.recipe.clone();
    candidate.set_presentation_donor(Some(WeaponDonorReference {
        item_hash: 0x514E_69D9_u32.into(),
        expected_name: Some("The Last Word".to_owned()),
    }));
    let catalog = app.catalog.as_ref().unwrap();
    let donor_preview =
        catalog.preview_appearance(&donor_view::preview::loadout(catalog, &candidate).unwrap());
    assert_eq!(serde_json::to_value(&app.recipe).unwrap(), saved_recipe);
    app.recipe
        .set_presentation_donor(Some(WeaponDonorReference {
            item_hash: 0x514E_69D9_u32.into(),
            expected_name: Some("The Last Word".to_owned()),
        }));
    let catalog = app.catalog.as_ref().unwrap();
    assert_eq!(
        catalog.preview_appearance(&donor_view::preview::loadout(catalog, &app.recipe).unwrap()),
        donor_preview
    );
    assert_eq!(app.recipe.overrides.art_arrangements, None);
    assert_eq!(app.recipe.icon_donor, None);
    let (output, _) = render(900.0, |ui| app.draw_appearance_workspace(ui));
    let rendered = text(&output);
    assert!(rendered.contains("Use Ornament"), "{rendered}");
    assert!(!rendered.contains("Red Dwarf"), "{rendered}");
}
