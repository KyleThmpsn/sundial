use super::*;
use crate::catalog::{CollectionConditionDef, CollectionConditionTokenDef};
use serde_json::json;

fn collectible(index: u16, slot: u32, inverse: bool) -> CollectibleDef {
    let mut tokens = vec![CollectionConditionTokenDef {
        kind: FLAG_INSTRUCTION,
        operand: slot,
    }];
    if inverse {
        tokens.push(CollectionConditionTokenDef {
            kind: 2,
            operand: 0,
        });
    }
    CollectibleDef {
        index,
        hash: u64::from(index) + 100,
        item_definition_index: index,
        item_hash: u64::from(index) + 200,
        material_requirement_set_index: None,
        material_requirement_set_hash: 0,
        material_requirements: Vec::new(),
        name: format!("Item {index}"),
        type_name: "Weapon".into(),
        paths: Vec::new(),
        conditions: vec![CollectionConditionDef { field: 4, tokens }],
    }
}

fn catalog() -> Catalog {
    Catalog::for_test(Vec::new(), Default::default()).with_test_progression(
        (0..3)
            .map(|slot| UnlockDefinition {
                code: 1,
                compact_slot: Some(slot),
                ..Default::default()
            })
            .collect(),
        Vec::new(),
        Vec::new(),
    )
}

#[test]
fn bulk_acquisition_round_trips_both_progression_views_and_preserves_unknown_fields() {
    let catalog = catalog();
    for native in [false, true] {
        let mut document = json!({"state":{"unlocks":{"future":[1,2,3]}},"future":{"keep":true}});
        if native {
            document["_native_progression"] = json!({"unlocks":[],"family":[]});
        }
        let definitions = [collectible(0, 0, false), collectible(1, 1, false)];
        let result = apply_rows(&mut document, &catalog, &definitions, true).unwrap();
        assert_eq!(result.changed, 2);
        let snapshot = collection_state_snapshot(&document).unwrap();
        for definition in &definitions {
            assert_eq!(
                collectible_acquired_state(definition, &snapshot, &catalog),
                Some(true)
            );
        }
        assert_eq!(
            apply_rows(&mut document, &catalog, &definitions, true)
                .unwrap()
                .unchanged,
            2
        );
        assert_eq!(
            apply_rows(&mut document, &catalog, &definitions, false)
                .unwrap()
                .changed,
            2
        );
        assert_eq!(document["future"], json!({"keep":true}));
        assert_eq!(document["state"]["unlocks"]["future"], json!([1, 2, 3]));
    }
}

#[test]
fn shared_flags_are_counted_once_per_collectible_and_conflicts_roll_back() {
    let catalog = catalog();
    let mut document = json!({});
    let same_flag = [collectible(0, 0, false), collectible(1, 0, false)];
    assert_eq!(
        apply_rows(&mut document, &catalog, &same_flag, true)
            .unwrap()
            .changed,
        2
    );
    let before = document.clone();
    let conflicting = [collectible(0, 0, false), collectible(1, 0, true)];
    assert!(apply_rows(&mut document, &catalog, &conflicting, true).is_err());
    assert_eq!(document, before);
}

#[test]
fn review_names_unselected_items_changed_by_shared_flags() {
    let catalog =
        catalog().with_test_collectibles(vec![collectible(0, 0, false), collectible(1, 0, false)]);
    let document = json!({});
    let mut job = Job::new(&document, vec![collectible(0, 0, false)], true).unwrap();
    while !job.step(&catalog, 8).unwrap() {}
    let review = job.review.as_ref().unwrap();
    assert_eq!(review.related.len(), 1);
    assert_eq!(review.related[0].name, "Item 1");
    assert_eq!(review.related[0].before, "Not Acquired");
    assert_eq!(review.related[0].after, "Acquired");
    assert_eq!(document, json!({}));
}

#[test]
fn unsupported_collectibles_are_reported_without_writing_their_state() {
    let catalog = catalog();
    let mut document = json!({});
    let definitions = [collectible(0, 0, false), collectible(1, 90, false)];
    assert_eq!(
        apply_rows(&mut document, &catalog, &definitions, true).unwrap(),
        Outcome {
            changed: 1,
            unchanged: 0,
            skipped: 1
        }
    );
}

#[test]
fn bulk_jobs_are_cancellable_and_reject_a_changed_source() {
    let catalog = catalog();
    let mut document = json!({"future":1});
    let before = document.clone();
    let mut cancelled = Job::new(
        &document,
        vec![collectible(0, 0, false), collectible(1, 1, false)],
        true,
    )
    .unwrap();
    assert!(!cancelled.step(&catalog, 1).unwrap());
    assert_eq!(document, before);
    drop(cancelled);
    let mut job = Job::new(&document, vec![collectible(0, 0, false)], true).unwrap();
    assert!(job.step(&catalog, 1).unwrap());
    document["future"] = json!(2);
    let external = document.clone();
    assert!(
        job.finish(&mut document, &catalog)
            .unwrap_err()
            .contains("account changed")
    );
    assert_eq!(document, external);
}

#[test]
fn bulk_acquisition_persists_in_the_selected_json_or_sqlite_account() {
    use crate::app::account_workspace::WorkspaceDocument;
    let catalog = catalog();
    let definitions = [collectible(0, 0, false), collectible(1, 1, false)];
    for native in [false, true] {
        let directory = crate::test_support::TestDirectory::new("bulk-account-routing");
        let path = directory.0.join("settings.json");
        let json = json!({"version":if native {18} else {8},"state":{"characters":[],"unlocks":{"future":true}}});
        std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        if native {
            crate::persistence::sqlite_account::tests::create_fixture(
                &directory.0.join("data/investment.sqlite3"),
                3,
            );
        }
        let mut workspace = WorkspaceDocument::load(json.clone(), &path, false);
        for acquired in [true, false] {
            let mut view = workspace.progression_view(0);
            assert_eq!(
                apply_rows(&mut view, &catalog, &definitions, acquired)
                    .unwrap()
                    .changed,
                2
            );
            workspace.apply_progression_view(0, view).unwrap();
            if native {
                assert_eq!(workspace.json(), &json);
                crate::persistence::sqlite_account::tests::save_fixture_document(
                    workspace.native_account_mut().unwrap(),
                    &directory.0.join(format!("backup-{acquired}.sqlite3")),
                );
            } else {
                std::fs::write(&path, serde_json::to_vec(workspace.json()).unwrap()).unwrap();
            }
            workspace = WorkspaceDocument::load(
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap(),
                &path,
                false,
            );
            let snapshot = collection_state_snapshot(&workspace.progression_view(0)).unwrap();
            for definition in &definitions {
                assert_eq!(
                    collectible_acquired_state(definition, &snapshot, &catalog),
                    Some(acquired)
                );
            }
        }
    }
}
#[test]
fn large_boolean_conditions_are_editable_and_unsupported_rows_wait_for_review() {
    let catalog = Catalog::for_test(Vec::new(), Default::default()).with_test_progression(
        (0..20)
            .map(|slot| UnlockDefinition {
                code: 1,
                compact_slot: Some(slot),
                ..Default::default()
            })
            .collect(),
        Vec::new(),
        Vec::new(),
    );
    let mut definition = collectible(0, 0, false);
    let mut tokens = vec![CollectionConditionTokenDef {
        kind: 1,
        operand: 0,
    }];
    for index in 1..20 {
        tokens.push(CollectionConditionTokenDef {
            kind: 1,
            operand: index,
        });
        tokens.push(CollectionConditionTokenDef {
            kind: 4,
            operand: 0,
        });
    }
    definition.conditions[0].tokens = tokens;
    let mut document = json!({});
    let before = document.clone();
    let mut job = Job::new(&document, vec![definition, collectible(1, 90, false)], true).unwrap();
    while !job.step(&catalog, 8).unwrap() {}
    assert_eq!(job.targets.len(), 1);
    assert_eq!(job.issues.len(), 1);
    assert!(job.issues[0].1.contains("#90 is unavailable"));
    assert_eq!(
        document, before,
        "Preparation must not commit a supported subset"
    );
    assert_eq!(job.finish(&mut document, &catalog).unwrap().changed, 1);
}

#[test]
fn the_review_action_row_stays_visible_above_a_long_impact_list() {
    // One selected item, forty more sharing its flag: the impact list is unbounded.
    let catalog = catalog().with_test_collectibles(
        (0..41u16)
            .map(|index| collectible(index, 0, false))
            .collect(),
    );
    let mut document = json!({});
    let mut job = Job::new(&document, vec![collectible(0, 0, false)], true).unwrap();
    while !job.step(&catalog, 64).unwrap() {}
    assert_eq!(job.review.as_ref().unwrap().related.len(), 40);
    let mut state = UiState {
        bulk_ready: Some(job),
        ..Default::default()
    };
    let ctx = egui::Context::default();
    let mut output = egui::FullOutput::default();
    for _ in 0..8 {
        output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 420.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    jobs(ui, &mut document, &catalog, &mut state);
                });
            },
        );
    }
    crate::app::tests::capture::write(&ctx, &output, "collection-review-long");
    let (pos, size, clip) = output
        .shapes
        .iter()
        .find_map(|shape| {
            let egui::Shape::Text(text) = &shape.shape else {
                return None;
            };
            text.galley
                .job
                .text
                .starts_with("Apply")
                .then(|| (text.pos, text.galley.size(), shape.clip_rect))
        })
        .expect("the apply action is missing from the modal");
    assert!(
        clip.contains(pos) && clip.contains(pos + size),
        "the action row is clipped away: {pos:?} {size:?} {clip:?}"
    );
    assert!(
        pos.y + size.y <= 420.0,
        "the action row is off screen: {pos:?}"
    );
}
