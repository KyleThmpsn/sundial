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

/// Sunrise manages an artifact mod's flag through Seasonal, so the flag itself is locked there.
/// Dawn has no Seasonal, so the same flag has to stay editable directly or it cannot be changed
/// at all.
#[test]
fn artifact_mod_flags_are_locked_for_sunrise_and_direct_for_dawn() {
    let catalog = Catalog::for_test(vec![], Default::default()).with_test_seasonal(
        crate::investment::seasonal::Definition {
            power_steps: vec![100],
            point_steps: vec![100],
            mods: vec![crate::investment::seasonal::ArtifactMod {
                sale_index: 0,
                category_index: 0,
                item_hash: 1,
                collectible_hash: 2,
                flag_definition: 0,
                character_slot: 9,
            }],
            reward_grants: Default::default(),
        },
    );
    let definition = UnlockDefinition {
        code: 3,
        ..Default::default()
    };
    let sunrise =
        collection_state_snapshot(&json!({"_native_progression": {"character_slot": 0}})).unwrap();
    assert_eq!(
        edit_blocked(false, 0, &definition, &sunrise, &catalog).map(|reason| reason.0),
        Some("Seasonal")
    );
    let dawn = collection_state_snapshot(
        &json!({"_native_progression": {"runtime": "dawn", "character_slot": 0}}),
    )
    .unwrap();
    assert!(edit_blocked(false, 0, &definition, &dawn, &catalog).is_none());
}

#[test]
fn large_unlock_tables_only_draw_the_viewport() {
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
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 760.0),
                )),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
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
    let started = std::time::Instant::now();
    for _ in 0..30 {
        let output = render(&mut state);
        assert!(output.shapes.iter().any(|shape| matches!(
            &shape.shape, egui::Shape::Text(text) if text.galley.job.text.starts_with("Unlock 000")
        )), "the viewport must paint actual unlock rows");
        assert!(
            output.shapes.len() < 1000,
            "Only visible rows should be painted"
        );
    }
    eprintln!(
        "23,000 unlocks, frame average: {:?}",
        started.elapsed() / 30
    );
    state.query = "Unlock 22999".into();
    render(&mut state);
    assert_eq!(state.unlock_browser.filtered.as_ref().unwrap().1.len(), 1);
}
