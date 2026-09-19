mod accessibility;
mod canvas;
mod entry;
mod guidance;
mod identity;
mod navigation;
use super::attachment::{Change, Target};
use super::*;
use crate::app::custom_perks::editor::tests::{editor, fixture};

impl Workbench {
    pub(in crate::app) fn set_test_editor(&mut self, editor: PerkEditor) {
        self.initialized = true;
        self.open = true;
        let mut recipe = PerkRecipe::new();
        recipe.effects.push(PerkRecipe::effect(1178));
        self.documents = vec![Document::new(recipe, None)];
        self.editing_effect = Some(1178);
        self.editor = Some(editor);
    }
}

fn frame(
    ctx: &egui::Context,
    workbench: &mut Workbench,
    experimental: bool,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let weapon = WeaponRecipe::every_end();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            assert!(
                workbench
                    .show(ctx, Path::new(""), None, &[], experimental, (&weapon, None))
                    .is_none()
            );
        },
    )
}

fn label(output: &egui::FullOutput, name: &str) -> Option<egui::Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.job.text == name => {
            Some(text.galley.rect.translate(text.pos.to_vec2()))
        }
        _ => None,
    })
}

#[test]
fn inline_parameters_keep_back_and_validation_available_during_loading() {
    let mut pending = editor(fixture());
    let (sender, receiver) = mpsc::channel();
    pending.receiver = Some(receiver);
    pending.graph = None;
    let mut workbench = Workbench::default();
    workbench.set_test_editor(pending);
    let ctx = egui::Context::default();
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = frame(
            &ctx,
            &mut workbench,
            false,
            egui::vec2(1000.0, 720.0),
            vec![],
        );
    }
    assert!(label(&output, "Wait for the perk data").is_none());
    let position = label(&output, "Back").expect("Back while loading").center();
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut workbench,
            false,
            egui::vec2(1000.0, 720.0),
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    assert!(workbench.editor.is_none());
    assert_eq!(workbench.retired_editors.len(), 1);
    assert!(
        workbench.documents[0].recipe.effects[0]
            .runtime_values
            .is_empty()
    );
    drop(sender);
}

#[test]
fn unified_workbench_preserves_drafts_and_parameter_controls_in_both_modes() {
    for size in [
        egui::vec2(640.0, 480.0),
        egui::vec2(1000.0, 720.0),
        egui::vec2(1320.0, 900.0),
    ] {
        for experimental in [false, true] {
            let mut workbench = Workbench::default();
            workbench.set_test_editor(editor(fixture()));
            let before = workbench.documents[0].recipe.clone();
            let ctx = egui::Context::default();
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                output = frame(&ctx, &mut workbench, experimental, size, vec![]);
            }
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for text in ["Apply and Back", "Back", "…"] {
                let rect = label(&output, text).unwrap_or_else(|| panic!("Missing {text}"));
                assert!(
                    screen.contains_rect(rect),
                    "{text} outside {size:?}: {rect:?}"
                );
            }
            assert_eq!(workbench.documents[0].recipe, before);
        }
    }
}

#[test]
fn discard_restores_saved_or_initial_perk_and_clears_pending_edits() {
    for saved in [false, true] {
        let mut workbench = Workbench::default();
        workbench.set_test_editor(editor(fixture()));
        let original = workbench.documents[0].recipe.clone();
        if saved {
            workbench.documents[0].baseline = Some(serde_json::to_vec(&original).unwrap());
        }
        workbench.documents[0].recipe.name = "Changed".into();
        workbench.documents[0]
            .recipe
            .stats
            .push(WeaponStatOverride {
                definition_index: 1,
                value: 50,
            });
        workbench.capture_effect_draft();
        workbench.discard_changes();
        assert_eq!(workbench.documents[0].recipe, original);
        assert!(workbench.documents[0].pending_effect.is_none());
        assert!(workbench.editor.is_none());
    }
}

#[test]
fn the_engine_catalog_opens_as_its_own_window_with_a_kind_selected() {
    let mut workbench = Workbench {
        open: true,
        initialized: true,
        documents: vec![Document::new(PerkRecipe::new(), None)],
        ..Default::default()
    };
    workbench.open_engine_catalog();
    workbench.engine.kinds.selected = Some((
        sundial::investment::native_content::kinds::Family::Effects,
        1,
    ));
    let ctx = egui::Context::default();
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = frame(
            &ctx,
            &mut workbench,
            false,
            egui::vec2(1320.0, 900.0),
            vec![],
        );
    }
    for text in [
        "Engine Catalog",
        "Installed Effect Entries",
        "Create Entity",
    ] {
        assert!(label(&output, text).is_some(), "Missing {text}");
    }
    // Unavailable installed counts must not be replaced by the bundled survey.
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape,
        egui::Shape::Text(text) if text.galley.job.text.contains("unavailable until native content")
    )));
}

#[test]
fn save_as_new_preserves_the_original_library_file_and_pending_document() {
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().to_owned()).unwrap();
    let mut original = PerkRecipe::new();
    original.effects.push(PerkRecipe::effect(405));
    let entry = library.save(&original, None).unwrap();
    let mut workbench = Workbench {
        initialized: true,
        library: Some(library.clone()),
        documents: vec![Document::new(
            original.clone(),
            Some(entry.baseline.clone()),
        )],
        ..Default::default()
    };
    workbench.documents[0].recipe.description = "Modified copy".into();
    let draft = workbench.documents[0].recipe.clone();
    workbench.save(true);
    assert!(workbench.error.is_none(), "{:?}", workbench.error);
    assert_eq!(std::fs::read(&entry.path).unwrap(), entry.baseline);
    assert_eq!(workbench.documents[0].recipe, draft);
    assert_ne!(workbench.documents[1].recipe.id, original.id);
    assert_eq!(workbench.documents[1].recipe.description, draft.description);
    assert_eq!(library.scan().unwrap().entries.len(), 2);
}

/// A workbench over a temporary library holding one saved perk named Alpha.
fn workbench_with_saved_perk() -> (tempfile::TempDir, Library, PerkRecipe, Entry, Workbench) {
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().to_owned()).unwrap();
    let mut original = PerkRecipe::new();
    original.name = "Alpha".into();
    original.effects.push(PerkRecipe::effect(405));
    let entry = library.save(&original, None).unwrap();
    let workbench = Workbench {
        initialized: true,
        library: Some(library.clone()),
        documents: vec![Document::new(
            original.clone(),
            Some(entry.baseline.clone()),
        )],
        ..Default::default()
    };
    (temporary, library, original, entry, workbench)
}

#[test]
fn duplicating_a_perk_opens_a_fresh_draft_and_leaves_the_file_alone() {
    let (_temporary, _library, original, entry, mut workbench) = workbench_with_saved_perk();
    workbench.duplicate(library::PerkSource::Document(0));
    assert_eq!(workbench.documents.len(), 2);
    assert_eq!(workbench.selected, 1);
    let copy = &workbench.documents[1];
    assert_eq!(copy.recipe.name, "Alpha Copy");
    assert_ne!(copy.recipe.id, original.id);
    assert_eq!(copy.recipe.effects, original.effects);
    assert!(copy.baseline.is_none());
    assert_eq!(std::fs::read(&entry.path).unwrap(), entry.baseline);
}

#[test]
fn deleting_a_perk_removes_its_file_and_keeps_one_document_selected() {
    let (_temporary, library, original, entry, mut workbench) = workbench_with_saved_perk();
    workbench.duplicate(library::PerkSource::Document(0));
    // Deleting the saved perk removes its file and its document, keeping the copy selected.
    workbench.delete(library::PerkSource::Document(0));
    assert!(workbench.error.is_none(), "{:?}", workbench.error);
    assert!(!entry.path.exists());
    assert_eq!(workbench.documents.len(), 1);
    assert_eq!(workbench.documents[0].recipe.name, "Alpha Copy");
    assert_eq!(workbench.selected, 0);
    // Deleting the last document leaves a fresh, unsaved perk open.
    workbench.delete(library::PerkSource::Document(0));
    assert!(workbench.error.is_none(), "{:?}", workbench.error);
    assert_eq!(workbench.documents.len(), 1);
    assert!(workbench.documents[0].baseline.is_none());
    assert_ne!(workbench.documents[0].recipe.name, "Alpha Copy");
    // A saved perk that is not open can be deleted from the list as well.
    let entry = library.save(&original, None).unwrap();
    workbench.refresh_library();
    assert_eq!(workbench.entries.len(), 1);
    workbench.delete(library::PerkSource::Entry(0));
    assert!(workbench.error.is_none(), "{:?}", workbench.error);
    assert!(!entry.path.exists());
    assert!(workbench.entries.is_empty());
}

#[test]
fn deleting_a_perk_changed_outside_the_workbench_preserves_the_file() {
    let (_temporary, _library, _original, entry, mut workbench) = workbench_with_saved_perk();
    std::fs::write(&entry.path, b"{}").unwrap();
    workbench.delete(library::PerkSource::Document(0));
    assert!(workbench.error.is_some());
    assert!(entry.path.exists());
    assert_eq!(workbench.documents.len(), 1);
    assert_eq!(workbench.documents[0].recipe.name, "Alpha");
}

#[test]
fn a_deleted_bundled_perk_is_not_added_back() {
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().to_owned()).unwrap();
    library.materialize_bundled().unwrap();
    let scan = library.scan().unwrap();
    assert!(!scan.entries.is_empty());
    for entry in &scan.entries {
        library.delete(&entry.recipe, &entry.baseline).unwrap();
    }
    library.materialize_bundled().unwrap();
    assert!(library.scan().unwrap().entries.is_empty());
    // Deleting an already deleted perk is not an error.
    library
        .delete(&scan.entries[0].recipe, &scan.entries[0].baseline)
        .unwrap();
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES for native stock and extra socket choices"]
fn native_workbench_roundtrip_preserves_all_effects_extra_choices_and_socket_metadata() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
    let donor = catalog.weapon_donor(0x23DB_942F).unwrap();
    let mut weapon = WeaponRecipe::new_weapon_for_donor(
        "parhelion.workbench-parity",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    let socket = donor.sockets.len();
    weapon.overrides.socket_columns = vec![None; socket];
    weapon
        .overrides
        .socket_columns
        .push(Some(crate::WeaponSocketColumnRecipe {
            socket_type: Some(92),
            choices: vec![0x45A0_BDD7.into(), 0xDD5C_B37A.into()],
            choice_weight_bits: vec![2.0_f32.to_bits(), 3.0_f32.to_bits()],
            reusable_plug_set_index: Some(3),
            randomized_plug_set_index: Some(4),
            randomized_selection_program: vec![crate::WeaponNumericInstructionRecipe {
                opcode: 11,
                operand: 2,
            }],
            ..Default::default()
        }));
    let expanded = socket_editor::socket_editor_donor(&donor, &weapon).into_owned();
    let target = Target::capture(&weapon, &expanded, socket, 1).unwrap();
    let mut workbench = Workbench {
        initialized: true,
        ..Default::default()
    };
    workbench.open_target(target.clone(), &catalog);
    let recipe = &mut workbench.documents[0].recipe;
    assert_eq!(
        recipe
            .effects
            .iter()
            .map(|effect| effect.source_perk_index)
            .collect::<Vec<_>>(),
        catalog.item_sandbox_perk_indices(0xDD5C_B37A)
    );
    recipe.name = "Private Frame".into();
    recipe.description = "Preserve every sibling effect".into();
    recipe.classification = Some(0xC684_24BC.into());
    recipe.effects.push(PerkRecipe::effect(405));
    recipe.stats.push(WeaponStatOverride {
        definition_index: 255,
        value: 10,
    });
    let authored = recipe.clone();
    let before = weapon.clone();
    let change = Change {
        target,
        perk: Some(authored.clone()),
    };
    change.apply(&mut weapon, &donor).unwrap();
    assert_eq!(
        weapon.overrides.socket_columns,
        before.overrides.socket_columns
    );
    let variant = &weapon.overrides.socket_plug_variants[0];
    assert_eq!(variant.choice_index, 1);
    assert_eq!(variant.socket_index as usize, socket);
    let restored = templates::from_variant(variant, &catalog);
    assert_eq!(restored.effects, authored.effects);
    assert_eq!(restored.stats, authored.stats);
    assert_eq!(restored.classification, authored.classification);
    assert_eq!(
        WeaponRecipe::from_json_str(&weapon.to_json_pretty().unwrap()).unwrap(),
        weapon
    );
    let applied = weapon.clone();
    assert!(change.apply(&mut weapon, &donor).is_err());
    assert_eq!(weapon, applied);
    verify_extra_choices(weapon, donor, authored, socket);
}

fn verify_extra_choices(
    mut weapon: WeaponRecipe,
    donor: WeaponDonor,
    authored: PerkRecipe,
    socket: usize,
) {
    let applied = weapon.clone();
    let expanded = socket_editor::socket_editor_donor(&donor, &weapon).into_owned();
    let append = Target::capture(&weapon, &expanded, socket, 2).unwrap();
    let mut extra = authored.clone();
    extra.name = "Another Private Frame".into();
    Change {
        target: append,
        perk: Some(extra),
    }
    .apply(&mut weapon, &donor)
    .unwrap();
    assert_eq!(
        weapon.overrides.socket_plug_variants[0],
        applied.overrides.socket_plug_variants[0]
    );
    assert_eq!(
        recipe_socket_choices(&weapon, socket, &[]).unwrap().len(),
        3
    );
    let target = Target::capture(&weapon, &expanded, socket, 1).unwrap();
    Change { target, perk: None }
        .apply(&mut weapon, &donor)
        .unwrap();
    assert_eq!(weapon.overrides.socket_plug_variants.len(), 1);
    assert_eq!(weapon.overrides.socket_plug_variants[0].choice_index, 2);
    assert_eq!(
        recipe_socket_choices(&weapon, socket, &[]).unwrap()[1],
        0xDD5C_B37A
    );
}
