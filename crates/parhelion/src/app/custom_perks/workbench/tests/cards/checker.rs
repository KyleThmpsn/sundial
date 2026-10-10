//! Persisted recipes, the shared Apply gate and navigation to nested native controls.
//! Failure analysis is recorded before implementation in the checker repair plan.
use super::*;
use sundial::package_authoring::sandbox_perk::{
    action,
    program::{NativeGroup, NativeNode, native_draft},
};

fn with_program(program: Program) -> (egui::Context, Workbench) {
    let (ctx, mut workbench) = setup();
    workbench.documents[0].recipe.effects.truncate(1);
    workbench.documents[0].recipe.effects[0].program = Some(program);
    (ctx, workbench)
}

/// Exercise the same cached diagnostics gate used by every attachment footer, without
/// requiring a catalog or a personal installation to supply a weapon socket.
fn click_apply(workbench: &mut Workbench, capture_name: &str) -> Option<PerkRecipe> {
    let ctx = egui::Context::default();
    let mut applied = None;
    let mut draw = |events| {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 500.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(recipe) = workbench.draw_mod_attachment(ui) {
                        applied = Some(recipe);
                    }
                });
            },
        )
    };
    let mut output = draw(vec![]);
    for _ in 0..3 {
        output = draw(vec![]);
    }
    let button = label(&output, "Apply to Mod").unwrap().center();
    for events in crate::test_support::driver::tap(button) {
        output = draw(events);
    }
    capture::write(&ctx, &output, capture_name);
    applied
}

#[test]
fn legacy_optional_choices_reopen_apply_and_preserve_explicit_values() {
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().join("perks")).unwrap();
    let mut drop = NativeNode::effect(13).unwrap();
    drop.bytes[0x10..0x14].fill(0);
    let draft = Program {
        trigger: Trigger::Drawn,
        actions: vec![
            Action::attach(Asset {
                graph: 0x8123_4567,
                ..Asset::default()
            }),
            Action::Native { node: drop },
        ],
        ..Program::default()
    };
    let mut native = native_draft(&draft).unwrap();
    let attach = native
        .graph
        .blocks
        .iter_mut()
        .find(|b| b.class == 0x8080_3E45)
        .unwrap();
    for at in [0x18, 0x1C, 0x30] {
        attach.bytes[at..at + 4].fill(0);
    }
    attach.bytes[0x20..0x24].copy_from_slice(&3.5_f32.to_le_bytes());
    let raw = native.graph.emit().unwrap();
    // Package reads retain original bytes. Only authored recipe loading repairs legacy None.
    let package = action::decode(&raw).unwrap();
    let original = package.effects().find(|effect| effect.kind == 1).unwrap();
    assert_eq!(&original.native[0x30..0x34], &[0; 4]);
    let (_, mut workbench) = with_program(draft.with_native(native).unwrap());
    let entry = library.save(&workbench.documents[0].recipe, None).unwrap();
    let reopened = Library::read(&entry.path).unwrap();
    workbench.documents[0] = Document::new(reopened.recipe, Some(reopened.baseline));
    let applied = click_apply(&mut workbench, "perk-checker-optional-apply")
        .expect("legacy optional values must allow Apply");
    let program = applied.effects[0].program.as_ref().unwrap();
    let bytes = native_draft(program).unwrap().graph.emit().unwrap();
    let decoded = action::decode(&bytes).unwrap();
    let attach = decoded.effects().find(|effect| effect.kind == 1).unwrap();
    for at in [0x18, 0x1C, 0x30] {
        assert_eq!(&attach.native[at..at + 4], &0x811C_9DC5u32.to_le_bytes());
    }
    assert_eq!(&attach.native[0x20..0x24], &3.5_f32.to_le_bytes());
    assert_eq!(attach.referenced_tag, Some(0x8123_4567));
    let drop = decoded.effects().find(|effect| effect.kind == 13).unwrap();
    assert_eq!(&drop.native[0x10..0x14], &u32::MAX.to_le_bytes());
    let mut explicit = applied.clone();
    let graph = &mut explicit.effects[0]
        .program
        .as_mut()
        .unwrap()
        .native
        .as_mut()
        .unwrap()
        .graph;
    let attach = graph
        .blocks
        .iter_mut()
        .find(|b| b.class == 0x8080_3E45)
        .unwrap();
    for (at, key) in [
        (0x18, 0x1234_5678u32),
        (0x1C, 0x9ABC_DEF0),
        (0x30, 0x1122_3344),
    ] {
        attach.bytes[at..at + 4].copy_from_slice(&key.to_le_bytes());
    }
    let entry = library.save(&explicit, Some(&entry.baseline)).unwrap();
    assert_eq!(Library::read(&entry.path).unwrap().recipe, explicit);
    crate::test_support::artifact(
        "perk-checker-optional.json",
        &serde_json::json!({
            "legacy_action": raw, "reopened": applied, "emitted_action": bytes,
            "explicit": explicit, "limit": "Draft emission and UI Apply, no package residency or gameplay run"
        }),
    );
}

#[test]
fn empty_masks_and_counters_warn_without_blocking_other_behaviors() {
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().join("perks")).unwrap();
    let mut receipts = Vec::new();
    for (kind, effect_mask) in [
        (6, false),
        (7, false),
        (8, false),
        (9, false),
        (28, false),
        (22, true),
        (26, false),
    ] {
        let mut node = if effect_mask {
            NativeNode::effect(kind)
        } else {
            NativeNode::condition(kind)
        }
        .unwrap();
        if kind == 26 && !effect_mask {
            let mut graph = action::native::Graph::read(&node.bytes, 0, 0x8080_3E30).unwrap();
            graph.blocks[0].bytes[8..16].fill(0);
            graph.blocks[0].links.remove(&0x10);
            node.bytes = graph.emit().unwrap();
        } else {
            // Native no-selection lanes, independent of the editor's field classifier.
            if effect_mask {
                node.bytes[2] = 0;
            } else {
                node.bytes[8..12].fill(0);
            }
        }
        let extra = NativeGroup {
            activation: vec![if effect_mask {
                NativeNode::condition(0).unwrap()
            } else {
                node.clone()
            }],
            effects: vec![if effect_mask {
                node
            } else {
                NativeNode::effect(6).unwrap()
            }],
            removal: vec![NativeNode::condition(0).unwrap()],
            ..NativeGroup::default()
        };
        let (_, mut workbench) = with_program(Program {
            trigger: Trigger::Drawn,
            actions: vec![Action::add_rounds(3)],
            additional_groups: vec![extra],
            ..Program::default()
        });
        let entry = library.save(&workbench.documents[0].recipe, None).unwrap();
        workbench.documents[0] = Document::new(Library::read(&entry.path).unwrap().recipe, None);
        let diagnostics = workbench.selected_diagnostics();
        assert!(diagnostics.iter().any(|issue| !issue.blocking));
        let applied = click_apply(
            &mut workbench,
            &format!("perk-checker-warning-{kind}-{effect_mask}"),
        )
        .expect("an empty selection or counter must allow Apply");
        let bytes = native_draft(applied.effects[0].program.as_ref().unwrap())
            .unwrap()
            .graph
            .emit()
            .unwrap();
        let decoded = action::decode(&bytes).unwrap();
        assert_eq!(decoded.groups.len(), 2);
        let inactive = &decoded.groups[1];
        if effect_mask {
            assert_eq!(inactive.effects[0].native[2], 0);
        } else if kind == 26 {
            assert!(inactive.activation[0].children.is_empty());
        } else {
            assert_eq!(inactive.activation[0].native[8], 0);
        }
        let ammo = decoded.groups[0]
            .effects
            .iter()
            .find(|effect| effect.kind == 14)
            .expect("the primary behavior must retain its ammunition action");
        assert_eq!(&ammo.native[0x6C..0x70], &3i32.to_le_bytes());
        receipts.push(serde_json::json!({"kind": kind, "effect_mask": effect_mask, "recipe": applied, "action": bytes}));
    }
    crate::test_support::artifact("perk-checker-warnings.json", &receipts);
}

#[test]
fn show_problem_reveals_nested_fields_and_native_structure_without_editing() {
    let size = egui::vec2(1320.0, 1000.0);
    let mut program = Program {
        trigger: Trigger::Drawn,
        actions: vec![Action::add_rounds(1)],
        ..Program::default()
    };
    let mut node = NativeNode::condition(20).unwrap();
    node.bytes[0xD4..0xD8].fill(0);
    program::native::insert_catalog_node(
        &mut program,
        0,
        super::super::super::catalog_insert::Placement::Requirement,
        node,
    )
    .unwrap();
    let mut apply = NativeNode::effect(4).unwrap();
    apply.bytes[0x10..0x14].fill(0);
    let raw_resource = Program {
        trigger: Trigger::Drawn,
        actions: vec![Action::Native { node: apply }],
        ..Program::default()
    };
    let mut receipts = Vec::new();
    for (program, field, structure, capture_name) in [
        (program, "State", false, "perk-checker-nested-field"),
        (
            raw_resource,
            "Applied Resource",
            true,
            "perk-checker-native-field",
        ),
    ] {
        let (ctx, mut workbench) = with_program(program);
        let before = workbench.documents[0].recipe.clone();
        collapsed(&ctx, &workbench);
        let output = render(&ctx, &mut workbench, size);
        assert!(label(&output, field).is_none(), "the target starts hidden");
        let button = label(&output, "Show Problem").unwrap().center();
        pointer(&ctx, &mut workbench, size, button, true);
        pointer(&ctx, &mut workbench, size, button, false);
        // The reveal opens the card, then the nested node, then scrolls to the field, and each
        // opening animates over several frames.
        let mut output = render(&ctx, &mut workbench, size);
        for _ in 0..3 {
            output = render(&ctx, &mut workbench, size);
        }
        let target = label(&output, field).expect("Show Problem must reveal the field control");
        assert!(target.center().y > 0.0 && target.center().y < size.y);
        assert_eq!(Card::new(&before.id, 1178, 0, 1).structure(&ctx), structure);
        assert_eq!(workbench.documents[0].recipe, before);
        assert!(workbench.reveal_problem.is_none());
        capture::write(&ctx, &output, capture_name);
        receipts.push(serde_json::json!({"field": field, "recipe": before}));
    }
    crate::test_support::artifact("perk-checker-navigation.json", &receipts);
}

#[test]
fn missing_required_assets_and_broken_graphs_still_prevent_apply() {
    for graph in [0, u32::MAX] {
        let (_, mut workbench) = with_program(Program {
            actions: vec![Action::Spawn {
                asset: Asset {
                    graph,
                    ..Asset::default()
                },
                position: Position::Owner,
            }],
            ..Program::default()
        });
        assert!(
            click_apply(&mut workbench, &format!("perk-checker-refused-{graph:08X}")).is_none()
        );
    }
    let mut native = NativeProgram::empty();
    native.graph.blocks[0].links.insert(0x40, usize::MAX);
    let (_, mut workbench) = with_program(Program {
        native: Some(native),
        ..Program::default()
    });
    assert!(click_apply(&mut workbench, "perk-checker-refused-graph").is_none());
    crate::test_support::artifact("perk-checker-refused.json", &workbench.documents[0].recipe);
}
