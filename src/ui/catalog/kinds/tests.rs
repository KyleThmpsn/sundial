use super::*;
use crate::sandbox_perk::nodes::EFFECTS;

#[test]
fn example_search_combines_source_behavior_and_effect_number() {
    let row = StockUse {
        index: 421,
        name: "Outlaw".into(),
        description: "Precision kills improve reload speed.".into(),
    };
    assert!(matches_use(&row, "OUTLAW reload 421"));
    assert!(matches_use(&row, "  precision  speed "));
    assert!(!matches_use(&row, "Outlaw damage"));
}

#[test]
fn installed_filter_uses_decoded_entries_and_waits_for_available_content() {
    let index = dependencies::Index {
        patterns: vec![],
        caster: None,
        perks: vec![perk(1, &[1], &[0])],
    };
    let mut kinds = Kinds {
        installed_only: true,
        family: Some(Family::Effects),
        ..Default::default()
    };
    let rows = kinds.visible_kinds(Some(&index));
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].0, rows[0].1.kind, rows[0].2),
        (Family::Effects, 1, Some(1))
    );
    assert!(kinds.visible_kinds(None).len() > 1);
    kinds.query = "no such kind".into();
    assert!(kinds.visible_kinds(Some(&index)).is_empty());
}

#[test]
fn catalog_keeps_example_actions_in_view_and_clears_hidden_selection() {
    for width in [560.0, 980.0, 1280.0] {
        let index = dependencies::Index {
            patterns: vec![],
            caster: None,
            perks: (1..80).map(|index| perk(index, &[1], &[])).collect(),
        };
        let mut kinds = Kinds {
            selected: Some((Family::Effects, 1)),
            selected_use: Some(1),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let sources = PerkSources::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 640.0));
        let mut action_rect = None;
        for _ in 0..3 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let mut actions = |ui: &mut egui::Ui, selected, _| {
                            assert_eq!(selected, Some(1));
                            let response = ui.button("Copy as New Perk");
                            action_rect = Some((response.rect, ui.clip_rect()));
                        };
                        kinds.draw(ui, &sources, Source::Ready(&index), &mut Some(&mut actions));
                    });
                },
            );
        }
        let (rect, clip) = action_rect.expect("copy action");
        assert!(
            screen.contains_rect(rect) && clip.contains_rect(rect),
            "copy action clipped at {width}: {rect:?} in {clip:?}"
        );
        kinds.use_query = "no matching example".into();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    kinds.draw(ui, &sources, Source::Ready(&index), &mut None);
                });
            },
        );
        assert_eq!(kinds.selected_use, None);
    }
}

fn perk(index: usize, effects: &[u8], conditions: &[u8]) -> dependencies::Perk {
    dependencies::Perk {
        index,
        hash: 0,
        runtime_key: 0,
        action: Some(0x8080_0000),
        graphs: Vec::new(),
        error: None,
        behavior: Some(dependencies::Behavior {
            headline: String::new(),
            support: Support::Readable,
            editable: true,
            program: None,
            condition_kinds: conditions.to_vec(),
            effect_kinds: effects.to_vec(),
            details: Vec::new(),
            notes: Vec::new(),
        }),
    }
}

#[test]
fn users_come_from_the_decoded_behaviors_of_each_family() {
    let mut undecoded = perk(2, &[1], &[]);
    undecoded.behavior = None;
    let perks = vec![perk(0, &[1, 3], &[0]), perk(1, &[3], &[1]), undecoded];
    let indices = |family, kind| {
        users(&perks, family, kind)
            .into_iter()
            .map(|perk| perk.index)
            .collect::<Vec<_>>()
    };
    assert_eq!(indices(Family::Effects, 1), vec![0]);
    assert_eq!(indices(Family::Effects, 3), vec![0, 1]);
    assert_eq!(indices(Family::Conditions, 1), vec![1]);
    assert!(indices(Family::Effects, 9).is_empty());
}

#[test]
fn installed_users_exclude_failed_and_unassigned_entries_even_with_stale_digests() {
    let mut failed = perk(1, &[1], &[]);
    failed.error = Some("Incomplete action".into());
    let mut unassigned = perk(2, &[1], &[]);
    unassigned.action = None;
    let perks = [perk(0, &[1], &[]), failed, unassigned];
    let found = users(&perks, Family::Effects, 1);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].index, 0);
}

#[test]
fn effect_rows_describe_decoded_actions_and_display_all_source_names() {
    let mut effect = perk(453, &[43], &[26]);
    effect.behavior.as_mut().unwrap().headline = "Decoded action summary.".into();
    let sources = [(1, "Thorn Catalyst"), (2, "Masterwork Weapon")]
        .into_iter()
        .map(|(hash, name)| {
            (
                453,
                crate::investment::PerkSource {
                    hash,
                    name: name.into(),
                    type_name: String::new(),
                },
            )
        })
        .collect();
    let rows = usage_rows(&[&effect], &sources);
    assert_eq!(rows[0].description, "Decoded action summary.");
    assert!(rows[0].name.contains("Masterwork Weapon"));
    assert!(rows[0].name.contains("Thorn Catalyst"));
    let mut engine = Kinds::default();
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            engine.draw_uses(ui, usage_rows(&[&effect], &sources), &sources, &mut None)
        });
    });
    let labels = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(labels.contains(&"Referenced By"));
    assert!(labels.contains(&"Decoded Behavior"));
    let format = |needle: &str| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text.contains(needle) => {
                    Some(&text.galley.job.sections[0].format)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing {needle}"))
    };
    assert_ne!(
        format("Thorn Catalyst").color,
        format("Decoded action summary.").color,
        "source names and supporting descriptions must have distinct emphasis"
    );
}

#[test]
fn searches_match_numbers_names_and_summaries() {
    let create = EFFECTS
        .iter()
        .find(|node| node.name == "Create Entity")
        .expect("the create entity kind");
    assert!(matches_query(create, "create"));
    assert!(matches_query(create, &create.kind.to_string()));
    assert!(matches_query(create, "entity create"));
    assert!(!matches_query(create, "ammunition"));
}

#[test]
fn stock_uses_sort_by_each_column_in_both_directions() {
    let rows = || {
        vec![
            (
                421,
                "Outlaw".to_owned(),
                "Precision kills reload.".to_owned(),
            ),
            (
                338,
                "Rampage".to_owned(),
                "Kills increase damage.".to_owned(),
            ),
            (
                405,
                "dragonfly".to_owned(),
                "Precision kills explode.".to_owned(),
            ),
        ]
    };
    let indices = |sorted: Vec<(u16, String, String)>| {
        sorted.into_iter().map(|row| row.0).collect::<Vec<_>>()
    };
    assert_eq!(
        indices(sorted_uses(rows(), UseColumn::Effect, false)),
        [338, 405, 421]
    );
    assert_eq!(
        indices(sorted_uses(rows(), UseColumn::Effect, true)),
        [421, 405, 338]
    );
    // Names sort without regard to case.
    assert_eq!(
        indices(sorted_uses(rows(), UseColumn::Name, false)),
        [405, 421, 338]
    );
    assert_eq!(
        indices(sorted_uses(rows(), UseColumn::Description, true)),
        [421, 405, 338]
    );
}
