use super::*;

/// The library and its drafts file through a restart: what a reader left open comes back with
/// its edits, what they saved is on disk, a draft the workbench cannot read is set aside
/// without taking the others with it, and export, import and delete work on the files.
/// With `SUNDIAL_TEST_ARTIFACTS` set, a report of each step is written there.
#[test]
fn drafts_and_saved_perks_survive_a_restart_and_a_bad_draft_is_set_aside() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("perks");
    let mut steps = Vec::new();
    let bravo_id = first_session(&root, &mut steps);
    restart_reads_back(&root, &bravo_id, &mut steps);
    let mut third = a_bad_draft_is_set_aside(&root, &mut steps);
    export_and_import(&mut third, temporary.path(), &bravo_id, &mut steps);
    delete_by_id(&mut third, &root, &bravo_id, &mut steps);
    two_windows_keep_both_drafts(&root, &mut steps);
    write_library_report(&steps);
}

/// A workbench opened on the library at `root`, as a launch opens the default one.
fn open_workbench(root: &Path) -> Workbench {
    let mut workbench = Workbench::default();
    workbench.initialize_from(Library::open(root.to_owned()));
    workbench
}

fn named(name: &str) -> PerkRecipe {
    let mut recipe = PerkRecipe::new();
    recipe.name = name.into();
    recipe
}

#[test]
fn imported_filter_keeps_saved_perks_and_open_edits_through_refresh() {
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().join("perks")).unwrap();
    let custom = library.save(&named("Personal Perk"), None).unwrap();
    library.import_embedded([named("Imported Perk")]).unwrap();
    let imported = library
        .scan()
        .unwrap()
        .entries
        .into_iter()
        .find(|entry| entry.recipe.name == "Imported Perk")
        .unwrap();
    let mut workbench = open_workbench(library.root());
    workbench.add_document(Document::new(
        imported.recipe.clone(),
        Some(imported.baseline.clone()),
    ));
    workbench.documents[workbench.selected].recipe.description = "Unsaved edit".into();
    let ctx = egui::Context::default();
    let draw = |workbench: &mut Workbench, events| {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| workbench.draw_library(ui, None, None));
            },
        )
    };
    let mut output = draw(&mut workbench, vec![]);
    for _ in 0..3 {
        output = draw(&mut workbench, vec![]);
    }
    assert_eq!(
        label(&output, "Imported Perk · Draft").is_some(),
        cfg!(feature = "d2-model-importer")
    );
    let mut visible = cfg!(feature = "d2-model-importer");
    let mut observations = Vec::new();
    for _ in 0..3 {
        let control = label(&output, "Show Imported Perks").unwrap().center();
        for events in crate::test_support::driver::tap(control) {
            draw(&mut workbench, events);
        }
        visible = !visible;
        workbench.refresh_library();
        output = draw(&mut workbench, vec![]);
        assert_eq!(label(&output, "Imported Perk · Draft").is_some(), visible);
        assert!(label(&output, "Personal Perk").is_some());
        assert_eq!(
            workbench.documents[workbench.selected].recipe.description,
            "Unsaved edit"
        );
        capture::write(
            &ctx,
            &output,
            if visible {
                "imported-perks-visible"
            } else {
                "imported-perks-hidden"
            },
        );
        observations.push(visible);
    }
    assert_eq!(std::fs::read(&custom.path).unwrap(), custom.baseline);
    assert_eq!(std::fs::read(&imported.path).unwrap(), imported.baseline);
    // A closed imported entry obeys the same filter after reopening the library.
    workbench.documents.clear();
    workbench.refresh_library();
    output = draw(&mut workbench, vec![]);
    assert_eq!(label(&output, "Imported Perk").is_some(), visible);
    crate::test_support::artifact(
        "imported-perk-filter.json",
        &serde_json::json!({
            "feature_enabled": cfg!(feature = "d2-model-importer"), "observations": observations,
            "imported": imported.recipe.id, "custom": custom.recipe.id,
            "saved_files_unchanged": true, "open_edits_retained": true
        }),
    );
}

fn names(workbench: &Workbench) -> Vec<String> {
    workbench
        .documents
        .iter()
        .map(|document| document.recipe.name.clone())
        .collect()
}

fn document_with_id<'a>(workbench: &'a Workbench, id: &str) -> Option<&'a Document> {
    workbench
        .documents
        .iter()
        .find(|document| document.recipe.id == id)
}

fn has_entry(workbench: &Workbench, id: &str) -> bool {
    workbench.entries.iter().any(|entry| entry.recipe.id == id)
}

/// Session one: two drafts, one of them saved to Custom Perks. Returns the saved perk's id.
fn first_session(root: &Path, steps: &mut Vec<String>) -> String {
    let mut first = open_workbench(root);
    assert!(first.drafts_writable, "{:?}", first.error);
    first.add_document(Document::new(named("Alpha"), None));
    first.documents[first.selected].recipe.description = "kept as a draft".into();
    first.persist_drafts();
    first.add_document(Document::new(named("Bravo"), None));
    first.save(false);
    assert_eq!(first.error, None);
    assert!(first.documents[first.selected].unchanged());
    let bravo_id = first.documents[first.selected].recipe.id.clone();
    steps.push(format!(
        "Session one opened {:?}, saved Bravo as {bravo_id}",
        names(&first)
    ));
    bravo_id
}

/// Session two: the same library reads Alpha's edit and Bravo's file back.
fn restart_reads_back(root: &Path, bravo_id: &str, steps: &mut Vec<String>) {
    let second = open_workbench(root);
    assert_eq!(second.error, None);
    let alpha = second
        .documents
        .iter()
        .find(|document| document.recipe.name == "Alpha")
        .expect("Alpha draft");
    assert_eq!(alpha.recipe.description, "kept as a draft");
    let bravo = document_with_id(&second, bravo_id).expect("Bravo document");
    assert!(bravo.unchanged());
    assert!(has_entry(&second, bravo_id));
    steps.push(format!("Session two opened {:?}", names(&second)));
}

/// A drafts file with one entry the workbench cannot read: the others open, the bad one is
/// kept aside, and autosave carries on.
fn a_bad_draft_is_set_aside(root: &Path, steps: &mut Vec<String>) -> Workbench {
    let drafts = root.join("workbench-drafts.json");
    let mut entries: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(&drafts).unwrap()).unwrap();
    entries.push(serde_json::json!({ "recipe": { "schema": 999 } }));
    std::fs::write(&drafts, serde_json::to_vec_pretty(&entries).unwrap()).unwrap();
    let mut third = open_workbench(root);
    let error = third.error.clone().unwrap_or_default();
    assert!(error.starts_with("1 draft could not be opened"), "{error}");
    let kept = std::fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .any(|name| name.starts_with("workbench-drafts.rejected.") && name.ends_with(".json"));
    assert!(kept, "the rejected draft has no file of its own");
    assert!(names(&third).contains(&"Alpha".to_owned()));
    assert!(third.drafts_writable);
    third.persist_drafts();
    assert_eq!(third.drafts_error, None);
    let reread: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(&drafts).unwrap()).unwrap();
    assert_eq!(reread.len(), entries.len() - 1);
    steps.push(format!("Session three set one draft aside: {error}"));
    third
}

/// Export writes the open perk outside the library, and import opens it under a new id.
fn export_and_import(
    workbench: &mut Workbench,
    outside: &Path,
    bravo_id: &str,
    steps: &mut Vec<String>,
) {
    let bravo_index = workbench
        .documents
        .iter()
        .position(|document| document.recipe.id == bravo_id)
        .unwrap();
    workbench.select_document(bravo_index);
    let exported = outside.join("Bravo.perk.json");
    workbench.export_to(&exported);
    assert_eq!(workbench.error, None);
    workbench.import_from(&exported);
    let imported = &workbench.documents[workbench.selected];
    assert_eq!(imported.recipe.name, "Bravo");
    assert_ne!(imported.recipe.id, bravo_id);
    steps.push(format!(
        "Exported {} and imported it as {}",
        exported.display(),
        imported.recipe.id
    ));
}

/// Delete finds the saved perk by id and removes its file.
fn delete_by_id(workbench: &mut Workbench, root: &Path, bravo_id: &str, steps: &mut Vec<String>) {
    let source = workbench.source_of(bravo_id).expect("Bravo is open");
    workbench.delete(source);
    assert_eq!(workbench.error, None);
    assert!(!root.join(format!("{bravo_id}.perk.json")).exists());
    assert!(!has_entry(workbench, bravo_id));
    assert!(document_with_id(workbench, bravo_id).is_none());
    steps.push(format!("Deleted {bravo_id}, {:?} remain", names(workbench)));
}

/// Two windows open the library before either autosaves. However often the second one autosaves
/// after losing the drafts file to the first, the first window's drafts stay, and the next start
/// opens the drafts of both.
fn two_windows_keep_both_drafts(root: &Path, steps: &mut Vec<String>) {
    let mut first = open_workbench(root);
    let mut second = open_workbench(root);
    first.add_document(Document::new(named("Charlie"), None));
    assert_eq!(first.drafts_error, None);
    second.add_document(Document::new(named("Delta"), None));
    assert!(
        second.drafts_error.is_some(),
        "the second window must see the conflict"
    );
    for text in ["second window", "second window, edited again"] {
        second.documents[second.selected].recipe.description = text.into();
        second.persist_drafts();
    }
    let on_disk =
        String::from_utf8(std::fs::read(root.join("workbench-drafts.json")).unwrap()).unwrap();
    assert!(
        on_disk.contains("Charlie"),
        "the first window's draft was replaced"
    );
    let next = open_workbench(root);
    let opened = names(&next);
    assert!(opened.contains(&"Charlie".to_owned()), "{opened:?}");
    let delta = next
        .documents
        .iter()
        .find(|document| document.recipe.name == "Delta")
        .expect("the second window's draft");
    assert_eq!(delta.recipe.description, "second window, edited again");
    steps.push(format!(
        "Two windows kept both drafts: {}. The next start opened {opened:?}",
        second.drafts_error.unwrap_or_default()
    ));
}

/// The report of each step, where `SUNDIAL_TEST_ARTIFACTS` asks for it.
fn write_library_report(steps: &[String]) {
    let Some(out) = crate::test_support::artifacts("library-controls") else {
        return;
    };
    std::fs::create_dir_all(&out).unwrap();
    let report = format!(
        "# Custom perk library round trip\n\n{}\n",
        steps
            .iter()
            .map(|step| format!("- {step}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    std::fs::write(out.join("report.md"), report).unwrap();
}
