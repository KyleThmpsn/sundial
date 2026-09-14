//! Layout checks for the program canvas at a small and a large window, in the style of
//! `app/ui_tests/runtime_layout.rs`. Nothing here opens a window or touches packages.
use super::*;
use sundial::package_authoring::{
    sandbox_perk::{
        action::{ActionSummary, GroupSummary, SummaryLine},
        nodes::Support,
        program::{Action, Asset, Position, Program, Trigger},
    },
    weapon_runtime::{WeaponRuntimeResource, WeaponRuntimeRoot, WeaponRuntimeRootKind},
};

const SIZES: [egui::Vec2; 2] = [egui::vec2(640.0, 480.0), egui::vec2(1320.0, 900.0)];

#[test]
fn stat_bonus_table_preserves_signed_deltas() {
    let mut workbench = Workbench::default();
    let mut recipe = PerkRecipe::new();
    recipe.stats = vec![
        WeaponStatOverride {
            definition_index: 1,
            value: -25,
        },
        WeaponStatOverride {
            definition_index: 2,
            value: 150,
        },
    ];
    let before = recipe.clone();
    let (output, screen) = panel(440.0, |ui| workbench.draw_stats(ui, None, &mut recipe));
    assert_fits_horizontally(&output, screen);
    assert_eq!(recipe, before);
}

#[test]
fn action_picker_reaches_the_last_native_kind_near_viewport_edges() {
    for size in SIZES {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let mut program = Program::default();
        let keys = Default::default();
        let mut draw = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        crate::app::style::workbench_style(ui);
                        ui.scope_builder(
                            egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                                egui::pos2(size.x - 210.0, size.y - 80.0),
                                screen.max - egui::vec2(10.0, 10.0),
                            )),
                            |ui| program::draw_actions_footer(ui, &mut program, &keys),
                        );
                    });
                },
            )
        };
        let click = |pos, pressed| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]
        };
        let mut output = draw(vec![]);
        let button = placements(&output, "Add Action…")[0].1;
        assert!(
            button.left() < size.x - 190.0,
            "Add Action stays left aligned"
        );
        draw(click(button.center(), true));
        draw(click(button.center(), false));
        draw(vec![egui::Event::Text("54".into())]);
        for _ in 0..8 {
            output = draw(vec![]);
        }
        let name = format!(
            "54: {}",
            sundial::package_authoring::sandbox_perk::nodes::EFFECTS[54].name
        );
        assert_visible(&output, &name, screen);
        let target = placements(&output, &name)[0].1.center();
        draw(click(target, true));
        draw(click(target, false));
        assert!(matches!(program.actions.last(), Some(Action::Native { node }) if node.kind == 54));
    }
}

#[test]
fn ingredient_operation_summaries_wrap_inside_the_details_pane() {
    use sundial::package_authoring::sandbox_perk::dependencies::{
        Behavior, DetailLine, DetailSection,
    };
    let behavior = Behavior {
        headline: String::new(), support: Support::Readable, editable: true,
        condition_kinds: Vec::new(), effect_kinds: vec![9], notes: Vec::new(),
        details: vec![DetailSection {
            group: "Main Program".into(), heading: "Then".into(),
            lines: vec![DetailLine {
                text: "Scales a selected component value by a value program, with an optional limit.".into(),
                kind: "Component Value Adjustment".into(), fields: Vec::new(), depth: 0, asset: None,
            }; 3],
        }],
    };
    for size in SIZES {
        let (output, screen) = panel(size.x * 0.48, |ui| {
            reading::overview(ui, &behavior, &BTreeMap::new());
        });
        assert_fits_horizontally(&output, screen);
    }
}

fn line(text: &str, kind: &str, support: Support, asset: Option<u32>) -> SummaryLine {
    SummaryLine {
        native: None,
        text: text.into(),
        detail: vec!["Asset: 0x815282E1".into()],
        kind_name: kind.into(),
        support,
        depth: 0,
        asset,
    }
}

fn stock_summary() -> ActionSummary {
    ActionSummary {
        headline: "The weapon is drawn, then applies 2 effects.".into(),
        groups: vec![GroupSummary {
            label: "Main Program".into(),
            activation: vec![line(
                "The weapon is drawn",
                "Draw Event",
                Support::Authorable,
                None,
            )],
            effects: vec![
                line(
                    "Attach demo for as long as the effect lasts",
                    "Create Entity",
                    Support::Authorable,
                    Some(0x8152_82E1),
                ),
                line(
                    "Extend the running timers by 5 s, up to 5 s",
                    "Extend Timers",
                    Support::Readable,
                    None,
                ),
            ],
            removal: vec![line(
                "The weapon is holstered",
                "Holster Event",
                Support::Authorable,
                None,
            )],
            rearm: Vec::new(),
        }],
        notes: vec!["This describes the compiled action.".into()],
        support: Support::Readable,
    }
}

/// A moving-projectile resource shaped the way `projectile::parameters::discover` expects,
/// carrying only the speed lanes.
fn movement_resource() -> WeaponRuntimeResource {
    let root = |kind: WeaponRuntimeRootKind, schema: u32, size: u32, offset: u32| {
        let field = WeaponRuntimeField {
            locator: WeaponRuntimeFieldLocator {
                graph_tag: None,
                binding_hash: 1,
                resource_index: 0,
                root: kind,
                root_schema: schema,
                path: vec![],
                type_handle: 2,
                value_offset: offset,
                byte_size: 8,
            },
            owner_offset: offset,
            name: "Projectile".into(),
            path_label: "Projectile".into(),
            kind: WeaponRuntimeValueKind::FixedBytes { size: 8 },
            value: WeaponRuntimeValue::Bytes([1.0f32.to_le_bytes(), [0; 4]].concat()),
            source: WeaponRuntimeFieldSource::OpaqueNativeType,
            generated_kind: None,
            name_inferred: false,
        };
        WeaponRuntimeRoot {
            kind,
            schema,
            owner_offset: 0,
            byte_size: size,
            generated_schema: false,
            structure: Default::default(),
            fields: vec![field],
        }
    };
    WeaponRuntimeResource {
        binding_hash: 1,
        binding_label: "Projectile".into(),
        resource_index: 0,
        resource_count: 1,
        owner_tag: 0x8152_82E7,
        concrete_class: 0x8080_3B73,
        alias_bindings: vec![],
        instance: root(
            WeaponRuntimeRootKind::ComponentInstance,
            0x8080_3B73,
            0x1E0,
            0x144,
        ),
        definition: Some(root(
            WeaponRuntimeRootKind::ComponentDefinition,
            0x8080_388F,
            0x5D0,
            0x88,
        )),
    }
}

fn stock_editor() -> PerkEditor {
    let mut loaded = fixture();
    loaded.summary = Some(stock_summary());
    loaded.projectile_slots = vec![(0x8152_82E1, 0x8152_82E1)];
    loaded.program = Some(Err("Test reason".into()));
    loaded.graphs[0].1.resources.push(movement_resource());
    editor(loaded)
}

fn program_recipe() -> PerkRecipe {
    let mut recipe = PerkRecipe::new();
    let mut effect = PerkRecipe::effect(421);
    effect.program = Some(Program {
        name: "Trail".into(),
        trigger: Trigger::PrecisionKill,
        duration_ms: 5_000,
        cooldown_ms: 2_500,
        chance_permyriad: 10_000,
        actions: vec![
            Action::attach(Asset {
                graph: 0x80BC_5810,
                path: "content/sandbox/effects/trail/trail.entity.tft".into(),
                values: Vec::new(),
            }),
            Action::Spawn {
                asset: Asset {
                    graph: 0x80BC_2F21,
                    path: "content/sandbox/effects/burst/burst.entity.tft".into(),
                    values: Vec::new(),
                },
                position: Position::Event,
            },
            Action::ExtendTimers {
                extend_ms: 5_000,
                cap_ms: 10_000,
            },
            Action::property(0x5EE2_66FC),
        ],
        removal_key: None,
        native_trigger: None,
        native_removal: None,
        native: None,
    });
    recipe.effects.push(effect);
    recipe
}

/// Draws one panel in a tall viewport so every row lays out, as `runtime_layout` does.
fn panel(width: f32, mut draw: impl FnMut(&mut egui::Ui)) -> (egui::FullOutput, egui::Rect) {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 1600.0));
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    crate::app::style::workbench_style(ui);
                    egui::ScrollArea::vertical().show(ui, |ui| draw(ui));
                });
            },
        );
    }
    (output, screen)
}

/// Every rendered text shape whose text matches, as (clip rect, text rect).
fn placements(output: &egui::FullOutput, name: &str) -> Vec<(egui::Rect, egui::Rect)> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.job.text == name => Some((
                clipped.clip_rect,
                text.galley.rect.translate(text.pos.to_vec2()),
            )),
            _ => None,
        })
        .collect()
}

/// No label may run past either edge of the viewport, whatever its width.
fn assert_fits_horizontally(output: &egui::FullOutput, screen: egui::Rect) {
    for clipped in &output.shapes {
        if let egui::Shape::Text(text) = &clipped.shape {
            let rect = text.galley.rect.translate(text.pos.to_vec2());
            assert!(
                rect.right() <= screen.right() + 0.5 && rect.left() >= screen.left() - 0.5,
                "{:?} runs outside {screen:?}: {rect:?}",
                text.galley.job.text
            );
        }
    }
}

fn assert_visible(output: &egui::FullOutput, name: &str, screen: egui::Rect) {
    let (clip, rect) = placements(output, name)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("Missing rendered label: {name}"));
    assert!(
        screen.contains_rect(rect) && clip.contains_rect(rect),
        "{name} is clipped at {:?}: {rect:?} in {clip:?}",
        screen.size()
    );
}

#[test]
fn every_complete_native_record_reader_fits_the_private_perk_window() {
    use sundial::package_authoring::sandbox_perk::{nodes, program::NativeNode};
    for (condition, entries) in [
        (true, nodes::CONDITIONS.as_slice()),
        (false, nodes::EFFECTS.as_slice()),
    ] {
        for entry in entries.iter().filter(|entry| entry.observed()) {
            let node = if condition {
                NativeNode::condition(entry.kind)
            } else {
                NativeNode::effect(entry.kind)
            }
            .unwrap();
            let (output, screen) = panel(640.0, |ui| {
                super::super::program::read_native(ui, condition, &node)
            });
            assert_fits_horizontally(&output, screen);
            assert!(!output.shapes.is_empty(), "{}", entry.name);
        }
    }
}

#[test]
fn stock_canvas_fits_without_mutating_the_draft() {
    for size in SIZES {
        // The whole window: the editor chrome stays reachable and nothing overflows.
        let mut workbench = Workbench::default();
        workbench.set_test_editor(stock_editor());
        let before = workbench.documents[0].recipe.clone();
        let ctx = egui::Context::default();
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = frame(&ctx, &mut workbench, true, size, vec![]);
        }
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        assert_fits_horizontally(&output, screen);
        assert_eq!(workbench.documents[0].recipe, before);

        // The parameter panel alone, tall enough to lay out every row at this width.
        let mut editor = stock_editor();
        let (output, screen) = panel(size.x, |ui| {
            let ctx = ui.ctx().clone();
            editor.draw_parameters(ui, &ctx, true);
        });
        assert_fits_horizontally(&output, screen);
        assert!(editor.draft.is_empty() && editor.projectile_draft.is_empty());
    }
}

#[test]
fn program_canvas_fits_without_mutating_locked_or_editable_recipes() {
    for size in SIZES {
        for experimental in [false, true] {
            // The whole window: the canvas header is reachable and nothing overflows.
            let mut workbench = Workbench {
                open: true,
                initialized: true,
                documents: vec![Document::new(program_recipe(), None)],
                ..Default::default()
            };
            let before = workbench.documents[0].recipe.clone();
            let ctx = egui::Context::default();
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                output = frame(&ctx, &mut workbench, experimental, size, vec![]);
            }
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            assert_fits_horizontally(&output, screen);
            assert_eq!(workbench.documents[0].recipe, before);

            // The effects page alone, tall enough to lay out every row at this width.
            let mut recipe = program_recipe();
            let before = recipe.clone();
            let (output, screen) = panel(size.x, |ui| {
                workbench.draw_effects(ui, Path::new(""), None, &[], &mut recipe, experimental);
            });
            assert_fits_horizontally(&output, screen);
            assert_eq!(recipe, before);
        }
    }
}
