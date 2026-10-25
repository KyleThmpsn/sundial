//! Node icon browsing through the real editor, followed by saved recipe readback.
use super::*;

fn control(ctx: &egui::Context, app: &mut PackageAuthoringApp, name: &str) {
    let output = settle(ctx, app);
    let at = accessible(&output, name)
        .map(|rect| rect.center())
        .unwrap_or_else(|| find(&output, name, |text, _| text == name));
    click(ctx, app, at);
}

fn search(ctx: &egui::Context, app: &mut PackageAuthoringApp, text: &str) {
    control(ctx, app, "Search Icons");
    frame(ctx, app, vec![egui::Event::Text(text.to_owned())]);
}

fn saved(app: &PackageAuthoringApp, artifacts: &Path, name: &str) -> WeaponRecipe {
    let path = artifacts.join(format!("{name}.parhelion.json"));
    app.recipe.save_json(&path).unwrap();
    WeaponRecipe::load_json(&path).unwrap()
}

fn icon(recipe: &WeaponRecipe, base: u32, place: Place) -> Option<EntryIcon> {
    recipe
        .overrides
        .subclass_abilities
        .clone()
        .unwrap_or_default()
        .edits(base, place)
        .icon
}

#[test]
#[ignore = "requires SUNDIAL_INSTALL and SUNDIAL_TEST_ARTIFACTS for native icons and UI artifacts"]
fn node_icon_picker_searches_broadens_and_preserves_saved_choices() {
    let install = crate::test_support::install();
    let artifacts = crate::test_support::artifact_dir("node-icons");
    fs::create_dir_all(&artifacts).unwrap();
    let mut app = PackageAuthoringApp {
        packages: install.join("packages"),
        perk_workbench: Workbench::offline(),
        ..Default::default()
    };
    app.install_catalog(crate::test_support::catalog(&install).unwrap());
    let base = app.subclasses.first().expect("native subclasses").clone();
    let donor = app
        .subclasses
        .iter()
        .find(|subclass| subclass.class_type != base.class_type && !subclass.entry_icons.is_empty())
        .expect("an icon from another class")
        .clone();
    let entry = layout::CLASS_ABILITIES[0];
    assert!(donor.entry_icons.contains_key(&entry));
    let name = donor.entry_names.get(&entry).expect("named ability");
    let place = Place::Ability(layout::CLASS_ABILITIES[0]);
    let other = Place::Node(AttunementPath::Top, 2);
    let ctx = context();
    new_from_menu(&ctx, &mut app, ItemKind::Subclass);
    app.recipe
        .rename_authored_item("Node Icon Picker Check")
        .unwrap();
    app.recipe.set_donor(base.hash, base.name.clone());
    app.subclass_page.selection = SubclassSelection::Entry(place);
    let original = app.recipe.clone();
    settle_icons(&ctx, &mut app);
    control(&ctx, &mut app, "Change Icon…");
    let output = settle_icons(&ctx, &mut app);
    find(&output, "Ability Icons", |text, _| text == "Ability Icons");
    capture::write(&ctx, &output, "node-icons-abilities");
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    assert!(
        app.recipe.same_saved_content(&original),
        "cancel must preserve the recipe"
    );
    control(&ctx, &mut app, "Change Icon…");
    search(&ctx, &mut app, &format!("{name} {}", donor.name));
    control(&ctx, &mut app, &format!("{name}\n{}", donor.name));
    let ability = saved(&app, &artifacts, "ability");
    let expected = EntryIcon::Ability {
        subclass: donor.hash,
        entry,
    };
    assert_eq!(icon(&ability, base.hash, place), Some(expected));
    assert_eq!(icon(&ability, base.hash, other), None);
    app.recipe = ability.clone();
    app.subclass_page.selection = SubclassSelection::Entry(other);
    settle(&ctx, &mut app);
    app.subclass_page.selection = SubclassSelection::Entry(place);
    let output = settle_icons(&ctx, &mut app);
    capture::write(&ctx, &output, "node-icons-reopened");

    control(&ctx, &mut app, "Change Icon…");
    control(&ctx, &mut app, "Icon Source");
    control(&ctx, &mut app, "All Icons");
    let output = settle(&ctx, &mut app);
    find(&output, "All Icons", |text, _| text == "All Icons");
    capture::write(&ctx, &output, "node-icons-all");
    control(&ctx, &mut app, "Select Existing Perk");
    // Outlaw supplies native transparent perk artwork through the asynchronous package read.
    control(&ctx, &mut app, "Search Perks");
    frame(&ctx, &mut app, vec![egui::Event::Text("Outlaw".into())]);
    control(&ctx, &mut app, "Outlaw");
    control(&ctx, &mut app, "Use Icon");
    // Switching nodes during loading must still apply to the node that opened the picker.
    app.subclass_page.selection = SubclassSelection::Entry(other);
    let start = Instant::now();
    while !matches!(
        icon(&app.recipe, base.hash, place),
        Some(EntryIcon::Artwork { .. })
    ) {
        settle(&ctx, &mut app);
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "perk icon did not reach its node"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    let artwork = saved(&app, &artifacts, "perk");
    assert_eq!(icon(&artwork, base.hash, other), None);
    app.recipe = artwork.clone();
    app.subclass_page.selection = SubclassSelection::Entry(place);
    let output = settle_icons(&ctx, &mut app);
    capture::write(&ctx, &output, "node-icons-perk");
    control(&ctx, &mut app, "Restore the original value");
    let reset = saved(&app, &artifacts, "reset");
    assert!(
        reset.same_saved_content(&original),
        "reset must restore the original recipe"
    );
    fs::write(artifacts.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "base": base.hash,
        "donor": donor.hash,
        "entry": entry,
        "ability_icon": icon(&ability, base.hash, place),
        "perk_icon": icon(&artwork, base.hash, place),
        "reset_icon": icon(&reset, base.hash, place),
        "other_node_icon": icon(&artwork, base.hash, other),
        "limits": "Rendered editor and persisted recipes. Native emission and gameplay unchanged."
    })).unwrap()).unwrap();
}
