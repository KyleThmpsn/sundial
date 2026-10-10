//! Attached presentation through the real editor, persisted reloads and rendered artifacts.
//! Failure boundaries: an undiscoverable child, variants edited inconsistently, an icon landing
//! on the parent, inherited color made explicit, stale async selections, edits lost on selection
//! changes and restoring the attached ability erasing the parent's presentation.
use super::*;

fn control(ctx: &egui::Context, app: &mut PackageAuthoringApp, name: &str) {
    let output = settle(ctx, app);
    let at = accessible(&output, name)
        .map(|rect| rect.center())
        .unwrap_or_else(|| find(&output, name, |text, _| text == name));
    click(ctx, app, at);
}

fn ready(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> egui::FullOutput {
    let start = Instant::now();
    loop {
        let output = settle(ctx, app);
        if accessible(&output, "Attached Ability HUD Color Source").is_some() {
            return output;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "attached HUD controls did not load"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
#[ignore = "requires SUNDIAL_INSTALL and SUNDIAL_TEST_ARTIFACTS for attached HUD controls"]
fn attached_ability_hud_controls_save_reload_and_restore_independently() {
    let install = crate::test_support::install();
    let artifacts = crate::test_support::artifact_dir("attached-hud-ui");
    fs::create_dir_all(&artifacts).unwrap();
    let mut app = PackageAuthoringApp {
        packages: install.join("packages"),
        perk_workbench: Workbench::offline(),
        ..Default::default()
    };
    app.install_catalog(crate::test_support::catalog(&install).unwrap());
    let base = app
        .subclasses
        .iter()
        .find(|s| s.name == "Sunbreaker")
        .expect("Sunbreaker corpus")
        .clone();
    let donor = app
        .subclasses
        .iter()
        .find(|s| s.name == "Arcstrider")
        .expect("Arcstrider corpus")
        .clone();
    let ctx = context();
    new_from_menu(&ctx, &mut app, ItemKind::Subclass);
    app.recipe.set_donor(base.hash, base.name.clone());
    app.recipe
        .rename_authored_item("Attached Ability HUD Check")
        .unwrap();
    let place = Place::Ability(layout::SUPER);
    app.subclass_page.selection = SubclassSelection::Entry(place);
    let original = app.recipe.clone();
    let output = ready(&ctx, &mut app);
    capture::write(&ctx, &output, "attached-hud-original");
    control(&ctx, &mut app, "Attached Ability HUD Color Source");
    control(&ctx, &mut app, "Custom Color");
    control(&ctx, &mut app, "Attached Ability HUD Color Hex Code");
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
            },
            egui::Event::Text("#24D6AD".into()),
        ],
    );
    control(&ctx, &mut app, "Change Attached Ability Icon…");
    control(&ctx, &mut app, "Search Icons");
    let donor_entry = layout::GRENADES[0];
    let name = donor.entry_names.get(&donor_entry).unwrap();
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text(format!("{name} {}", donor.name))],
    );
    control(&ctx, &mut app, &format!("{name}\n{}", donor.name));
    let file = artifacts.join("attached.parhelion.json");
    app.recipe.save_json(&file).unwrap();
    let saved = WeaponRecipe::load_json(&file).unwrap();
    let edits = saved
        .overrides
        .subclass_abilities
        .as_ref()
        .unwrap()
        .edits(base.hash, place);
    assert_eq!(edits.color, None);
    assert_eq!(edits.icon, None);
    assert_eq!(
        edits.attached_abilities.len(),
        2,
        "both native Hammer variants are authored"
    );
    for graph in [0x80BAA866, 0x80BAA9D5] {
        let attached = edits
            .attached_abilities
            .iter()
            .find(|edit| edit.graph == graph)
            .unwrap();
        assert_eq!(attached.color, Some([36, 214, 173]));
        assert_eq!(
            attached.icon,
            Some(EntryIcon::Ability {
                subclass: donor.hash,
                entry: donor_entry
            })
        );
    }
    app.recipe = saved.clone();
    app.subclass_page.selection = SubclassSelection::Entry(Place::Ability(layout::GRENADES[0]));
    settle(&ctx, &mut app);
    app.subclass_page.selection = SubclassSelection::Entry(place);
    let output = ready(&ctx, &mut app);
    capture::write(&ctx, &output, "attached-hud-reopened");
    control(&ctx, &mut app, "Change Attached Ability Icon…");
    control(&ctx, &mut app, "Icon Source");
    control(&ctx, &mut app, "All Icons");
    control(&ctx, &mut app, "Select Existing Perk");
    control(&ctx, &mut app, "Search Perks");
    frame(&ctx, &mut app, vec![egui::Event::Text("Outlaw".into())]);
    control(&ctx, &mut app, "Outlaw");
    control(&ctx, &mut app, "Use Icon");
    // The deferred texture belongs to the attached ability even after selecting another node.
    app.subclass_page.selection = SubclassSelection::Entry(Place::Ability(layout::GRENADES[0]));
    let start = Instant::now();
    loop {
        settle(&ctx, &mut app);
        let edits = app
            .recipe
            .overrides
            .subclass_abilities
            .as_ref()
            .unwrap()
            .edits(base.hash, place);
        if edits
            .attached_abilities
            .iter()
            .all(|edit| matches!(edit.icon, Some(EntryIcon::Artwork { .. })))
        {
            assert_eq!(edits.attached_abilities.len(), 2);
            assert!(
                edits
                    .attached_abilities
                    .iter()
                    .all(|edit| edit.color == Some([36, 214, 173]))
            );
            assert!(edits.icon.is_none());
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "attached artwork did not finish loading"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    app.recipe
        .save_json(artifacts.join("artwork.parhelion.json"))
        .unwrap();
    let artwork = WeaponRecipe::load_json(artifacts.join("artwork.parhelion.json")).unwrap();
    assert!(
        artwork
            .overrides
            .subclass_abilities
            .as_ref()
            .unwrap()
            .edits(base.hash, Place::Ability(layout::GRENADES[0]))
            .is_empty()
    );
    app.recipe = artwork.clone();
    app.subclass_page.selection = SubclassSelection::Entry(place);
    let output = ready(&ctx, &mut app);
    capture::write(&ctx, &output, "attached-hud-artwork");
    control(&ctx, &mut app, "Restore Attached Ability");
    app.recipe.save_json(&file).unwrap();
    let restored = WeaponRecipe::load_json(&file).unwrap();
    assert!(
        restored.same_saved_content(&original),
        "attached reset restores the unedited recipe"
    );
    let output = ready(&ctx, &mut app);
    capture::write(&ctx, &output, "attached-hud-restored");
    fs::write(artifacts.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "source_build":"86657.20.08.23.1800.d2_rc___release", "base":base.hash,
        "recipe":saved, "artwork":artwork, "restored":restored, "input":install,
        "limits":"Rendered editor and persisted recipe. Package emission is verified separately."
    })).unwrap()).unwrap();
}
