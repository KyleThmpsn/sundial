use super::*;
use crate::catalog::Catalog;

#[test]
fn dawn_progression_pages_use_the_live_database() {
    let directory = TestDirectory::new("dawn-progression-pages");
    let mut app = app(directory.0.clone());
    crate::persistence::dawn_account::tests::create_fixture(&directory.0.join("player-state.db"));
    app.document = crate::app::account_workspace::WorkspaceDocument::load(
        serde_json::json!({"version":6}),
        &directory.0.join("settings.json"),
        true,
    );
    app.manifest = progression_catalog();
    app.preferences.experimental_progression = true;
    app.select_view(ViewMode::Progression);
    let before = app.document.clone();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    for (section, name) in [
        (ProgressionSection::Collections, "collections"),
        (ProgressionSection::Triumphs, "triumphs"),
        (ProgressionSection::Seasonal, "seasonal"),
        (ProgressionSection::Unlocks, "unlocks"),
        (ProgressionSection::Investment, "investment"),
    ] {
        app.progression_section = section;
        app.progression_ui.reset_navigation();
        for _ in 0..3 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 900.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    app.draw_app_chrome(ctx, None);
                    app.draw_active_view(ctx);
                },
            );
            let labels: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                !labels
                    .iter()
                    .any(|label| label.contains("Invalid progression settings"))
            );
            assert!(!super::inventory_recovery::button(&output, "Seasonal").1);
            if section == ProgressionSection::Seasonal {
                assert!(
                    labels
                        .iter()
                        .any(|label| label.contains("Seasonal Unavailable"))
                );
                assert!(!labels.contains(&"Apply XP"));
            }
            crate::app::tests::capture::write(&ctx, &output, &format!("dawn-progression-{name}"));
        }
        assert_eq!(app.document, before);
        assert!(!app.dirty);
    }
}

#[test]
fn progression_browsing_remains_available_when_editing_is_disabled_or_reset() {
    let directory = TestDirectory::new("progression-access");
    let mut app = app(directory.0.clone());
    let before = app.document.clone();
    let ctx = egui::Context::default();
    app.select_view(ViewMode::Progression);
    assert!(app.view_mode == ViewMode::Progression);

    for editing in [false, true, false] {
        app.preferences.experimental_progression = editing;
        for section in [
            ProgressionSection::Collections,
            ProgressionSection::Triumphs,
            ProgressionSection::Unlocks,
            ProgressionSection::Investment,
        ] {
            app.progression_section = section;
            let output = ctx.run(Default::default(), |ctx| {
                app.draw_app_chrome(ctx, None);
                app.draw_active_view(ctx);
            });
            assert!(!output.shapes.is_empty());
            assert_eq!(app.progression_ui.read_only, !editing);
            assert_eq!(app.collections_ui.read_only, !editing);
            assert_eq!(app.document, before);
            assert!(!app.dirty);
        }
    }
    app.reset_preferences_to_defaults(&ctx);
    assert!(app.view_mode == ViewMode::Progression);
    assert!(!app.preferences.experimental_progression);
}

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
            tested_by: vec![ProgressionContextDef {
                hash: 200 + index as u64,
                kind: ProgressionContextKind::Record,
                name: (*name).into(),
                type_name: String::new(),
                description: String::new(),
                paths: vec![vec!["Destinations".into(), "Triumphs".into()]],
                condition_programs: Vec::new(),
                direct_references: vec!["Record completion flag".into()],
            }],
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
fn progression_tables_keep_named_content_visible_in_both_themes_and_sizes() {
    for width in [640.0, 1100.0] {
        for dark in [false, true] {
            for section in [
                ProgressionSection::Collections,
                ProgressionSection::Triumphs,
                ProgressionSection::Unlocks,
            ] {
                let directory = TestDirectory::new("progression-layout");
                let mut app = app(directory.0.clone());
                app.manifest = progression_catalog();
                app.progression_section = section;
                app.preferences.experimental_progression = true;
                app.document = WorkspaceDocument::json_only(
                    serde_json::json!({"version":8,"state":{"characters":[],"unlocks":{"account_flag_runs":[[0,1]],"objective_values":[[0,10],[1,3]]}}}),
                );
                let before = app.document.clone();
                let ctx = egui::Context::default();
                ctx.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                let mut output = egui::FullOutput::default();
                for _ in 0..3 {
                    output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 760.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default()
                                .show(ctx, |ui| app.draw_progression_page(ui));
                        },
                    );
                }
                crate::app::tests::capture::write(
                    &ctx,
                    &output,
                    &format!(
                        "progression-{}-{dark}-{width}",
                        match section {
                            ProgressionSection::Collections => "collections",
                            ProgressionSection::Triumphs => "triumphs",
                            _ => "unlocks",
                        }
                    ),
                );
                let texts = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) => Some(text),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let expected = match section {
                    ProgressionSection::Collections => "Better Devils",
                    ProgressionSection::Triumphs => "Destinations",
                    _ => "First Victory",
                };
                let text = texts
                    .iter()
                    .find(|text| text.galley.job.text == expected)
                    .unwrap_or_else(|| panic!("Missing {expected}"));
                assert!(text.pos.x >= 0.0 && text.pos.x < width && text.pos.y < 740.0);
                if section == ProgressionSection::Unlocks {
                    let details = texts
                        .iter()
                        .filter(|text| text.galley.job.text == "Details")
                        .collect::<Vec<_>>();
                    assert_eq!(details.len(), 5);
                    for cell in &details[1..] {
                        assert!(
                            (cell.pos.x - details[0].pos.x).abs() < 10.0,
                            "Details cells must align with their header"
                        );
                    }
                }
                assert_eq!(app.document, before);
                assert!(!app.dirty);
            }
        }
    }
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
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 760.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.draw_progression_page(ui));
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
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    for _ in 0..8 {
        frame(&mut app, Vec::new());
    }
    assert!(!app.dirty, "Related changes must be reviewed before commit");
    let output = frame(&mut app, Vec::new());
    crate::app::tests::capture::write(&ctx, &output, "collections-bulk-review");
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
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
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

#[test]
fn triumph_header_selection_commits_one_undoable_edit() {
    let directory = TestDirectory::new("bulk-buttons");
    let mut app = app(directory.0.clone());
    app.manifest = progression_catalog();
    app.preferences.experimental_progression = true;
    app.progression_section = ProgressionSection::Triumphs;
    let before = app.document.clone();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let frame = |app: &mut SundialApp, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 760.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.draw_progression_page(ui));
            },
        )
    };
    for label in [
        "Select All",
        "Clear Selection",
        "Select All",
        "Complete Selected",
    ] {
        frame(&mut app, Vec::new());
        let output = frame(&mut app, Vec::new());
        let pos = triumph_control_position(&output, label);
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        if label != "Complete Selected" {
            let output = frame(&mut app, Vec::new());
            let has_bulk_actions = output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Complete Selected")
            });
            assert_eq!(has_bulk_actions, label != "Clear Selection");
            assert_eq!(app.document, before, "Selection must not edit the account");
        }
    }
    for _ in 0..8 {
        frame(&mut app, Vec::new());
    }
    assert!(!app.dirty, "Related changes must be reviewed before commit");
    let output = frame(&mut app, Vec::new());
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.starts_with("Apply ") => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            _ => None,
        })
        .expect("Shared-state impact review must offer Apply");
    for pressed in [true, false] {
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(app.dirty);
    assert_eq!(app.undo_history.len(), 1);
    let view = app.document.progression_view(0);
    let snapshot = progression::collection_state_snapshot(&view).unwrap();
    for record in app.manifest.records().unwrap() {
        assert_eq!(
            snapshot.evaluated_flag(usize::from(record.completion_flag.unwrap()), &app.manifest),
            Some(true)
        );
    }
    let undo = app.undo_history.pop().unwrap();
    app.restore_history_document(undo, true);
    assert_eq!(app.document, before);
}

fn triumph_control_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    let text_label = if label == "Complete Selected" {
        label
    } else {
        "Triumph"
    };
    let text_pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == text_label => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing {text_label}"));
    if label == "Complete Selected" {
        text_pos
    } else {
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("accessible widgets")
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == egui::accesskit::Role::CheckBox)
            .filter_map(|(_, node)| node.bounds())
            .find(|bounds| {
                bounds.x1 <= f64::from(text_pos.x)
                    && bounds.y0 <= f64::from(text_pos.y)
                    && bounds.y1 >= f64::from(text_pos.y)
            })
            .map(|bounds| {
                egui::pos2(
                    ((bounds.x0 + bounds.x1) * 0.5) as f32,
                    ((bounds.y0 + bounds.y1) * 0.5) as f32,
                )
            })
            .expect("Selection checkbox to the left of Triumph")
    }
}
