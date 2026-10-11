//! Layout checks for the program canvas at a small and a large window. Nothing here opens a
//! window or touches packages.
use super::*;
use sundial::package_authoring::runtime::{BindingHash, SchemaHandle};
use sundial::package_authoring::{
    runtime::{WeaponRuntimeResource, WeaponRuntimeRoot, WeaponRuntimeRootKind},
    sandbox_perk::program::{Action, Asset, Position, Program, Trigger},
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
fn action_picker_reaches_a_technical_native_kind_near_viewport_edges() {
    for size in SIZES {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let mut program = Program::default();
        let mut workbench = Workbench::default();
        let keys = Default::default();
        let mut draw = |events| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        crate::app::style::perk_workbench_style(ui);
                        ui.scope_builder(
                            egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                                egui::pos2(size.x - 210.0, size.y - 80.0),
                                screen.max - egui::vec2(10.0, 10.0),
                            )),
                            |ui| {
                                if let Some(action) = workbench.behaviors.draw_action(
                                    ui,
                                    &workbench.discovery,
                                    &workbench.perk_names,
                                    &workbench.asset_labels,
                                    &program,
                                    &keys,
                                ) {
                                    program.actions.push(action);
                                }
                            },
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
        for _ in 0..8 {
            output = draw(vec![]);
        }
        let all = placements(&output, "Show All")[0].1.center();
        draw(click(all, true));
        draw(click(all, false));
        for _ in 0..8 {
            output = draw(vec![]);
        }
        // A bare native kind has no plain-language name, so reaching it needs Advanced.
        let detail = placements(&output, "Standard")[0].1.center();
        draw(click(detail, true));
        draw(click(detail, false));
        for _ in 0..8 {
            output = draw(vec![]);
        }
        let advanced = placements(&output, "Advanced")
            .into_iter()
            .map(|(_, rect)| rect)
            .next_back()
            .expect("the open list offers Advanced");
        draw(click(advanced.center(), true));
        draw(click(advanced.center(), false));
        for _ in 0..8 {
            output = draw(vec![]);
        }
        let search = placements(&output, "Search Behaviors")[0].1.center();
        draw(click(search, true));
        draw(click(search, false));
        draw(vec![egui::Event::Text("51".into())]);
        for _ in 0..8 {
            output = draw(vec![]);
        }
        // Kind 51's enum values are unresolved, so it has no plain-language name: it is
        // reachable only under Advanced and renders as the engine operation it was traced
        // to. Naming a kind moves it out of this list, which is the point of the check.
        let name = sundial::package_authoring::sandbox_perk::nodes::EFFECTS
            .iter()
            .find(|entry| entry.kind == 51)
            .unwrap()
            .name
            .to_owned();
        assert_visible(&output, &name, screen);
        let target = placements(&output, &name)[0].1.center();
        draw(click(target, true));
        draw(click(target, false));
        output = draw(vec![]);
        let use_action = placements(&output, "Add Action")[0].1.center();
        draw(click(use_action, true));
        draw(click(use_action, false));
        assert!(matches!(program.actions.last(), Some(Action::Native { node }) if node.kind == 51));
    }
}

/// A moving-projectile resource shaped the way `entity::projectile::parameters::discover` expects,
/// carrying only the speed lanes.
pub(super) fn movement_resource() -> WeaponRuntimeResource {
    let root = |kind: WeaponRuntimeRootKind, schema: u32, size: u32, offset: u32| {
        let field = WeaponRuntimeField {
            locator: WeaponRuntimeFieldLocator {
                graph_tag: None,
                binding_hash: BindingHash::new(1),
                resource_index: 0,
                root: kind,
                root_schema: schema.into(),
                path: vec![],
                type_handle: SchemaHandle::new(2),
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
    loaded.projectile_slots = vec![(0x8152_82E1, 0x8152_82E1)];
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
                damage_type: None,
                rows: Vec::new(),
                hud_status: None,
                script: None,
            }),
            Action::Spawn {
                asset: Asset {
                    graph: 0x80BC_2F21,
                    path: "content/sandbox/effects/burst/burst.entity.tft".into(),
                    values: Vec::new(),
                    damage_type: None,
                    rows: Vec::new(),
                    hud_status: None,
                    script: None,
                },
                position: Position::Event,
            },
            Action::ExtendTimers {
                extend_ms: 5_000,
                cap_ms: 10_000,
            },
            Action::property(0x5EE2_66FC),
        ],
        native_asset_patches: Vec::new(),
        imported_assets: Vec::new(),
        removal_key: None,
        native_trigger: None,
        native_removal: None,
        native: None,
        auxiliary: Vec::new(),
        policy: None,
        alternative_triggers: Vec::new(),
        alternative_removals: Vec::new(),
        native_rearm: None,
        ability_tunings: Vec::new(),
        ability_inputs: Vec::new(),
        alternative_rearms: Vec::new(),
        additional_groups: Vec::new(),
    });
    recipe.effects.push(effect);
    recipe
}

/// Draws one panel in a tall viewport so every row lays out.
fn panel(width: f32, mut draw: impl FnMut(&mut egui::Ui)) -> (egui::FullOutput, egui::Rect) {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 1600.0));
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    crate::app::style::perk_workbench_style(ui);
                    egui::ScrollArea::vertical().show(ui, |ui| draw(ui));
                });
            },
        );
    }
    (output, screen)
}

/// Every rendered text shape whose text matches, as (clip rect, text rect).
pub(super) fn placements(output: &egui::FullOutput, name: &str) -> Vec<(egui::Rect, egui::Rect)> {
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
fn stock_behavior_editor_fits_without_mutating_the_draft() {
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

#[test]
fn the_behavior_stock_filter_keeps_its_choice_after_the_frame_that_set_it() {
    let size = egui::vec2(1320.0, 900.0);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    let program = Program::default();
    let mut workbench = Workbench::default();
    let keys = Default::default();
    let mut draw = |events| {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    crate::app::style::perk_workbench_style(ui);
                    workbench.behaviors.draw_action(
                        ui,
                        &workbench.discovery,
                        &workbench.perk_names,
                        &workbench.asset_labels,
                        &program,
                        &keys,
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
    let settle = |draw: &mut dyn FnMut(Vec<egui::Event>) -> egui::FullOutput| {
        let mut output = draw(Vec::new());
        for _ in 0..8 {
            output = draw(Vec::new());
        }
        output
    };
    let mut output = settle(&mut draw);
    let open = placements(&output, "Add Action…")[0].1.center();
    draw(click(open, true));
    draw(click(open, false));
    output = settle(&mut draw);
    let combo = placements(&output, "Any Stock Use")[0].1.center();
    draw(click(combo, true));
    draw(click(combo, false));
    output = settle(&mut draw);
    // The open list shows every source; "Guided" is the one under the current value.
    let guided = placements(&output, "Unused by Stock Perks")
        .into_iter()
        .map(|(_, rect)| rect)
        .next_back()
        .expect("the open list offers Unused by Stock Perks");
    draw(click(guided.center(), true));
    draw(click(guided.center(), false));
    output = settle(&mut draw);
    // The choice must survive the frame that set it. Reading and writing the state through
    // different `Ui` ids silently discards it, which a rendering check does not catch.
    assert!(
        !placements(&output, "Unused by Stock Perks").is_empty(),
        "the source filter fell back to its default after the frame that set it"
    );
    assert!(
        placements(&output, "Any Stock Use").is_empty(),
        "the source filter still shows All Sources after Guided was chosen"
    );
}
