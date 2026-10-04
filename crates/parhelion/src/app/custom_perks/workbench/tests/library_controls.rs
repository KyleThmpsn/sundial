use super::*;

fn save_key() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::S,
        physical_key: Some(egui::Key::S),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    }
}

#[test]
fn save_shortcut_writes_the_current_perk_and_keeps_other_files_unchanged() {
    let (_temporary, library, original, entry, mut workbench) = workbench_with_saved_perk();
    let mut sibling = PerkRecipe::new();
    sibling.name = "Sibling".into();
    let sibling = library.save(&sibling, None).unwrap();
    workbench.open = true;
    workbench.documents[0].recipe.description = "Saved with Ctrl+S".into();
    let ctx = egui::Context::default();
    frame(
        &ctx,
        &mut workbench,
        false,
        egui::vec2(1000.0, 720.0),
        vec![save_key()],
    );
    let saved = Library::read(&entry.path).unwrap();
    assert_eq!(saved.recipe.id, original.id);
    assert_eq!(saved.recipe.description, "Saved with Ctrl+S");
    assert_eq!(
        workbench.documents[0].baseline.as_deref(),
        Some(saved.baseline.as_slice())
    );
    assert_eq!(std::fs::read(sibling.path).unwrap(), sibling.baseline);
}

#[test]
fn save_shortcut_preserves_unapplied_parameters_and_closed_workbenches() {
    let (_temporary, _library, _original, entry, mut workbench) = workbench_with_saved_perk();
    let ctx = egui::Context::default();
    workbench.documents[0].recipe.description = "Not saved yet".into();
    frame(
        &ctx,
        &mut workbench,
        false,
        egui::vec2(1000.0, 720.0),
        vec![save_key()],
    );
    assert_eq!(std::fs::read(&entry.path).unwrap(), entry.baseline);
    workbench.open = true;
    workbench.editor = Some(editor(fixture()));
    workbench.editing_effect = Some(405);
    frame(
        &ctx,
        &mut workbench,
        false,
        egui::vec2(1000.0, 720.0),
        vec![save_key()],
    );
    assert_eq!(std::fs::read(&entry.path).unwrap(), entry.baseline);
    assert!(
        workbench
            .error
            .as_deref()
            .unwrap()
            .contains("Apply or discard")
    );
    assert!(workbench.editor.is_some());
    assert!(workbench.documents[0].pending_effect.is_some());
}

/// The library and its drafts file through a restart: what a reader left open comes back with
/// its edits, what they saved is on disk, a draft the workbench cannot read is set aside
/// without taking the others with it, and export, import and delete work on the files.
/// With `PARHELION_LIBRARY_OUT` set, a report of each step is written there.
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
    assert!(root.join("workbench-drafts.rejected.json").exists());
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

/// The report of each step, where `PARHELION_LIBRARY_OUT` asks for it.
fn write_library_report(steps: &[String]) {
    let Some(out) = std::env::var_os("PARHELION_LIBRARY_OUT") else {
        return;
    };
    let out = PathBuf::from(out);
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

#[test]
fn truncated_program_summary_opens_only_one_tooltip() {
    use super::super::{
        canvas::{self, Backend, Canvas},
        cards::Card,
    };
    use sundial::package_authoring::sandbox_perk::program::{Action, Program};
    let mut program = Program {
        actions: (1..=8).map(Action::add_rounds).collect(),
        ..Program::default()
    };
    let summary = super::super::guidance::summary_with_assets(&program, None, None);
    let labels = std::collections::BTreeMap::new();
    let ctx = egui::Context::default();
    ctx.style_mut(|style| style.interaction.tooltip_delay = 0.0);
    // A locked card reads its program as one line, truncated in a narrow pane.
    let mut frame = |events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(420.0, 720.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    canvas::draw_effect(
                        ui,
                        Canvas {
                            name: "",
                            backend: Backend::Program {
                                program: &mut program,
                                stock: None,
                                description: None,
                                labels: &labels,
                                editing: None,
                            },
                            header: None,
                            footer: None,
                            trigger_command: None,
                        },
                        Card::new("locked", 1, 0, 1),
                    );
                });
            },
        )
    };
    let mut output = egui::FullOutput::default();
    for _ in 0..4 {
        output = frame(vec![]);
    }
    let position = label(&output, &summary).expect("program summary").center();
    for _ in 0..4 {
        output = frame(vec![egui::Event::PointerMoved(position)]);
    }
    let summaries = output.shapes.iter().filter(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == summary)).count();
    assert_eq!(summaries, 2, "one summary label and one tooltip");
}
