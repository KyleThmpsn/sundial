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
fn damage_filters_reopen_and_apply_without_an_asset_choice() {
    use action::native::{Block, Graph};
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().join("perks")).unwrap();
    // An unrestricted constant outgoing modifier. The two label registries are absent,
    // while both runtime predicates select the native unrestricted mode.
    let mut bytes = vec![0; 0x150];
    bytes[0] = 40;
    bytes[0xA0] = 255;
    bytes[0xB0..0xB2].fill(1);
    bytes[0x110] = 255;
    let graph = Graph {
        blocks: vec![
            Block {
                class: 0x8080_2F16,
                count: None,
                bytes,
                links: BTreeMap::from([(0x140, 1)]),
            },
            Block {
                class: 0x8080_2F1A,
                count: None,
                bytes: 0.05_f32.to_le_bytes().repeat(2),
                links: BTreeMap::new(),
            },
        ],
    };
    let original = graph.emit().unwrap();
    let (_, mut workbench) = with_program(Program {
        trigger: Trigger::Drawn,
        actions: vec![Action::Native {
            node: NativeNode {
                kind: 40,
                bytes: original.clone(),
            },
        }],
        ..Program::default()
    });
    let entry = library.save(&workbench.documents[0].recipe, None).unwrap();
    let reopened = Library::read(&entry.path).unwrap();
    workbench.documents[0] = Document::new(reopened.recipe, Some(reopened.baseline));
    let applied = click_apply(&mut workbench, "perk-checker-outgoing-damage")
        .expect("an unrestricted label filter must not require an asset");
    let native = native_draft(applied.effects[0].program.as_ref().unwrap()).unwrap();
    let emitted = native.graph.emit().unwrap();
    let decoded = action::decode(&emitted).unwrap();
    let damage = decoded.effects().find(|effect| effect.kind == 40).unwrap();
    for at in [0x90, 0x108] {
        assert_eq!(&damage.native[at..at + 4], &[0; 4]);
    }
    let value = 0x140 + i64::from_le_bytes(damage.native[0x140..0x148].try_into().unwrap());
    let value = usize::try_from(value).unwrap();
    assert_eq!(
        &damage.native[value..value + 8],
        &0.05_f32.to_le_bytes().repeat(2)
    );
    let mut required = applied.clone();
    required.effects[0]
        .program
        .as_mut()
        .unwrap()
        .actions
        .push(Action::attach(Asset::default()));
    workbench.documents[0] = Document::new(required, None);
    assert!(click_apply(&mut workbench, "perk-checker-required-object").is_none());
    crate::test_support::artifact(
        "perk-checker-outgoing-damage.json",
        &serde_json::json!({
            "saved": entry.baseline, "applied": applied, "action": emitted,
            "required_object_blocked": true, "gameplay_tested": false
        }),
    );
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
