use super::*;
use serde_json::json;

#[test]
fn item_context_values_are_not_offered_ineffective_account_overrides() {
    let catalog = Catalog::for_test(vec![], Default::default());
    let snapshot = collection_state_snapshot(&json!({})).unwrap();
    for (value, bank) in [(false, 5), (true, 4)] {
        let reason = edit_blocked(
            value,
            0,
            &UnlockDefinition {
                code: bank,
                ..Default::default()
            },
            &snapshot,
            &catalog,
        )
        .unwrap();
        assert_eq!(reason.0, "Item Context");
        assert!(reason.1.contains("cannot change"));
    }
    assert!(
        edit_blocked(
            true,
            0,
            &UnlockDefinition {
                code: 3,
                ..Default::default()
            },
            &snapshot,
            &catalog
        )
        .is_none()
    );
}

#[test]
fn content_rows_preserve_unnamed_entries_and_use_record_roles() {
    let context = crate::catalog::ProgressionContextDef {
        hash: 55,
        kind: crate::catalog::ProgressionContextKind::Record,
        name: "First Victory".into(),
        type_name: String::new(),
        description: String::new(),
        paths: vec![vec!["Crucible".into(), "Triumphs".into()]],
        condition_programs: Vec::new(),
        direct_references: vec!["Record completion flag".into()],
    };
    let catalog = Catalog::for_test(Vec::new(), Default::default()).with_test_progression(
        vec![
            UnlockDefinition {
                code: 1,
                compact_slot: Some(4),
                tested_by: vec![context],
                ..Default::default()
            },
            UnlockDefinition {
                hash: 0x1234abcd,
                code: 6,
                compact_slot: Some(2),
                ..Default::default()
            },
        ],
        Vec::new(),
        Vec::new(),
    );
    let rows = entries(&catalog, false);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].name, "First Victory");
    assert_eq!(rows[0].purpose, "Completion");
    assert!(rows[0].named && rows[0].reference);
    assert!(rows[0].search.contains("crucible"));
    assert!(!rows[1].named);
    assert_eq!(rows[1].name, "Unnamed Unlock #1");
    assert_eq!(rows[1].scope, "Character");
    assert!(rows[1].search.contains("1234abcd"));
}

#[test]
fn search_finds_unsaved_definitions_in_both_content_tables() {
    let definitions = vec![
        UnlockDefinition {
            hash: 0x1234abcd,
            name: Some("Season Reward".into()),
            description: Some("Complete a Strike".into()),
            code: 1,
            compact_slot: Some(73),
            ..Default::default()
        },
        UnlockDefinition {
            hash: 0x5678ef01,
            description: Some("Unlabelled Quest Step".into()),
            ..Default::default()
        },
    ];
    let catalog = Catalog::for_test(Vec::new(), Default::default()).with_test_progression(
        definitions.clone(),
        definitions,
        Vec::new(),
    );
    let snapshot = collection_state_snapshot(&json!({})).unwrap();
    for value in [false, true] {
        let entries = entries(&catalog, value);
        for (query, expected) in [
            ("season reward", vec![0]),
            ("strike", vec![0]),
            ("1234abcd", vec![0]),
            ("73", vec![0]),
            ("quest step", vec![1]),
            ("absent", vec![]),
        ] {
            let filter = Filter {
                query: query.into(),
                names: Names::All,
                scope: None,
                sort: TableSort::ascending(0),
                value: Some(value),
                category: None,
            };
            assert_eq!(
                filtered_rows(&entries, &filter, &snapshot, &catalog),
                expected,
                "{query}, value={value}"
            );
        }
    }
}

#[test]
fn browsing_content_tables_preserves_documents_and_invalidates_saved_state() {
    let catalog = Catalog::for_test(Vec::new(), Default::default()).with_test_progression(
        vec![UnlockDefinition {
            hash: 1,
            name: Some("First Victory".into()),
            code: 1,
            compact_slot: Some(0),
            ..Default::default()
        }],
        Vec::new(),
        Vec::new(),
    );
    let mut document = json!({});
    let before = document.clone();
    let mut state = UiState {
        read_only: true,
        ..Default::default()
    };
    let ctx = egui::Context::default();
    for tab in [Tab::Entries, Tab::Ranks, Tab::Storage] {
        state.unlock_browser.tab = tab;
        state.reset_navigation();
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert!(!draw(
                    ui,
                    &mut document,
                    &UnlockPolicy::default(),
                    &catalog,
                    &mut state
                ));
            });
        });
        assert!(!output.shapes.is_empty());
        assert_eq!(document, before);
    }
    state.invalidate_cache();
    assert!(state.unlock_browser.snapshot.is_none());
}

#[test]
fn large_unlock_tables_reuse_filtered_rows_and_only_draw_the_viewport() {
    let catalog = Catalog::for_test(Vec::new(), Default::default()).with_test_progression(
        (0..23_000)
            .map(|index| UnlockDefinition {
                hash: index as u64,
                code: 1,
                compact_slot: Some(index as u16),
                name: Some(format!("Unlock {index:05}")),
                ..Default::default()
            })
            .collect(),
        Vec::new(),
        Vec::new(),
    );
    let mut document = json!({});
    let mut state = UiState {
        read_only: true,
        ..Default::default()
    };
    let ctx = egui::Context::default();
    let mut render = |state: &mut UiState| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 760.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw(ui, &mut document, &UnlockPolicy::default(), &catalog, state);
                });
            },
        )
    };
    for _ in 0..128 {
        render(&mut state);
        if state.unlock_browser.filtered.is_some() {
            break;
        }
    }
    let allocation = state.unlock_browser.filtered.as_ref().unwrap().1.as_ptr();
    let started = std::time::Instant::now();
    for _ in 0..30 {
        let output = render(&mut state);
        assert!(
            output.shapes.len() < 1000,
            "Only visible rows should be painted"
        );
        assert_eq!(
            state.unlock_browser.filtered.as_ref().unwrap().1.as_ptr(),
            allocation
        );
    }
    eprintln!(
        "23,000 unlocks, cached frame average: {:?}",
        started.elapsed() / 30
    );
    state.query = "Unlock 22999".into();
    render(&mut state);
    assert_eq!(state.unlock_browser.filtered.as_ref().unwrap().1.len(), 1);
}
