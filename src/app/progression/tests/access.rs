use crate::app::tests::driver::*;
use crate::app::*;
use crate::catalog::Catalog;

fn progression_catalog() -> Catalog {
    use crate::catalog::{
        CollectibleDef, CollectionConditionDef, CollectionConditionTokenDef, ObjectiveDef,
        ObjectiveOwnerDef, ObjectiveOwnerKind, ProgressionContextDef, ProgressionContextKind,
        UnlockDefinition,
    };
    let names = ["First Victory", "Adventurer"];
    let flags = names
        .iter()
        .enumerate()
        .map(|(index, name)| UnlockDefinition {
            hash: 100 + index as u64,
            code: 1,
            compact_slot: Some(index as u16),
            tested_by: vec![
                ProgressionContextDef {
                    hash: 200 + index as u64,
                    kind: ProgressionContextKind::Record,
                    name: (*name).into(),
                    type_name: String::new(),
                    description: String::new(),
                    paths: vec![vec!["Destinations".into(), "Triumphs".into()]],
                    condition_programs: Vec::new(),
                    direct_references: vec!["Record completion flag".into()],
                }
                .into(),
            ],
            ..Default::default()
        })
        .collect();
    let values = names
        .iter()
        .enumerate()
        .map(|(index, _)| UnlockDefinition {
            hash: 300 + index as u64,
            code: 1,
            compact_slot: Some(index as u16),
            ..Default::default()
        })
        .collect();
    let objectives = names
        .iter()
        .enumerate()
        .map(|(index, name)| ObjectiveDef {
            hash: 300 + index as u64,
            progress_description: "Complete activities".into(),
            completion_value: 10,
            related_unlock_value_definition_index: Some(index as u16),
            owners: vec![ObjectiveOwnerDef {
                hash: 200 + index as u64,
                kind: ObjectiveOwnerKind::Record,
                name: (*name).into(),
                type_name: "Record".into(),
                description: String::new(),
                traits: Vec::new(),
                paths: vec![vec!["Destinations".into(), "Triumphs".into()]],
            }],
            ..Default::default()
        })
        .collect();
    let collectibles = ["Better Devils", "Ace of Spades"]
        .iter()
        .enumerate()
        .map(|(index, name)| CollectibleDef {
            hash: 400 + index as u64,
            index: index as u16,
            item_hash: 500 + index as u64,
            item_definition_index: index as u16,
            name: (*name).into(),
            type_name: "Hand Cannon".into(),
            paths: Vec::new(),
            parent_nodes: Vec::new(),
            material_requirements: Vec::new(),
            material_requirement_set_index: None,
            material_requirement_set_hash: 0,
            conditions: vec![CollectionConditionDef {
                field: 4,
                tokens: vec![CollectionConditionTokenDef {
                    kind: 1,
                    operand: index as u32,
                }],
            }],
        })
        .collect();
    Catalog::for_test(Vec::new(), Default::default())
        .with_test_progression(flags, values, Vec::new())
        .with_test_objectives(objectives)
        .with_test_collectibles(collectibles)
        .with_test_records(
            names
                .iter()
                .enumerate()
                .map(|(index, name)| crate::catalog::RecordDefinition {
                    index,
                    hash: 200 + index as u64,
                    name: (*name).into(),
                    paths: vec![vec!["Destinations".into(), "Triumphs".into()]],
                    objectives: vec![index],
                    completion_flag: Some(index as u16),
                    ..Default::default()
                })
                .collect(),
        )
}

#[test]
fn collection_bulk_buttons_commit_one_undoable_edit() {
    let directory = TestDirectory::new("bulk-buttons");
    let mut app = app(directory.0.clone());
    app.manifest = progression_catalog();
    app.preferences.experimental_progression = true;
    let before = app.document.clone();
    let ctx = egui::Context::default();
    let frame = |app: &mut SundialApp, events| {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 760.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.draw_progression_page(ui));
            },
        )
    };
    for label in ["Select Filtered", "Acquire Selected"] {
        frame(&mut app, Vec::new());
        let output = frame(&mut app, Vec::new());
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.pos + text.galley.rect.center().to_vec2())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Missing {label}"));
        for pressed in [true, false] {
            frame(&mut app, crate::test_support::primary_press(pos, pressed));
        }
    }
    for _ in 0..8 {
        frame(&mut app, Vec::new());
    }
    assert!(!app.dirty, "Related changes must be reviewed before commit");
    let output = frame(&mut app, Vec::new());
    crate::test_support::capture::write(&ctx, &output, "collections-bulk-review");
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.starts_with("Apply ") => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            let labels = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            panic!("Shared-state impact review must offer Apply: {labels:?}");
        });
    for pressed in [true, false] {
        frame(&mut app, crate::test_support::primary_press(pos, pressed));
    }
    assert!(app.dirty);
    assert_eq!(app.undo_history.len(), 1);
    let view = app.document.progression_view(0);
    let snapshot = progression::collection_state_snapshot(&view).unwrap();
    for collectible in app.manifest.collectibles() {
        assert_eq!(
            collections_page::collectible_acquired_state(collectible, &snapshot, &app.manifest),
            Some(true)
        );
    }
    let undo = app.undo_history.pop().unwrap();
    app.restore_history_document(undo, true);
    assert_eq!(app.document, before);
    let restored_view = before.progression_view(0);
    let restored = progression::collection_state_snapshot(&restored_view).unwrap();
    let acquired = app
        .manifest
        .collectibles()
        .iter()
        .filter(|collectible| {
            collections_page::collectible_acquired_state(collectible, &restored, &app.manifest)
                == Some(true)
        })
        .count();
    let expected = format!(
        "{acquired} / {} acquired",
        app.manifest.collectibles().len()
    );
    let output = frame(&mut app, Vec::new());
    assert!(
        output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == expected)
        }),
        "The collection count should refresh after undo"
    );
}
