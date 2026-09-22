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
fn action_asset_and_location_share_a_column_without_overflow_or_mutation() {
    for width in [340.0, 440.0, 640.0, 900.0] {
        let mut workbench = Workbench::default();
        let mut action = Action::Spawn {
            asset: Asset::default(),
            position: Position::Owner,
        };
        let before = action.clone();
        let (output, screen) = panel(width, |ui| {
            workbench.draw_action_block(ui, None, false, &mut action, 0, 1);
        });
        assert_fits_horizontally(&output, screen);
        assert_no_text_overlap(&output);
        let asset = placements(&output, "Choose Object or Effect…")[0].1;
        let location = placements(&output, "At Your Position")[0].1;
        let asset_label = placements(&output, "Object or Effect *")[0].1;
        let location_label = placements(&output, "Spawn Location")[0].1;
        assert!((asset_label.right() - location_label.right()).abs() < 1.0);
        assert!(asset.left() > asset_label.right() || asset.top() >= asset_label.bottom());
        assert!(
            location.left() > location_label.right() || location.top() >= location_label.bottom()
        );
        assert!(asset.bottom() < location.top());
        assert_visible(&output, "Object or Effect *", screen);
        assert_eq!(action, before);
    }
}

/// Text shapes may not overlap each other. A wrapped label inside a one-line row is the
/// usual way this breaks, and it reads as garbled text on screen.
fn assert_no_text_overlap(output: &egui::FullOutput) {
    let rects = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if !text.galley.job.text.trim().is_empty() => Some((
                text.galley.job.text.clone(),
                text.galley.rect.translate(text.pos.to_vec2()),
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    for (i, (a, first)) in rects.iter().enumerate() {
        for (b, second) in &rects[i + 1..] {
            let overlap = first.intersect(*second);
            assert!(
                overlap.width() <= 1.0 || overlap.height() <= 1.0,
                "{a:?} at {first:?} overlaps {b:?} at {second:?}"
            );
        }
    }
}

#[test]
fn property_rows_keep_long_labels_on_one_line_in_a_narrow_pane() {
    for width in [240.0, 320.0, 640.0] {
        let (output, screen) = panel(width, |ui| {
            let mut values = [3_u8, 7, 250];
            for (label, value) in [
                "Target Role and Input Selector",
                "Second Selected Slot Program Words",
                "Requires Owning Weapon While Sprinting",
            ]
            .into_iter()
            .zip(&mut values)
            {
                super::super::properties::field(ui, label, "A hint.", |ui| {
                    ui.add(egui::DragValue::new(value).range(0..=255));
                });
            }
        });
        assert_fits_horizontally(&output, screen);
        assert_no_text_overlap(&output);
    }
}

#[test]
fn the_description_summary_command_sits_under_the_text_box() {
    let mut workbench = Workbench {
        page: Page::Basics,
        ..Default::default()
    };
    let mut recipe = PerkRecipe::new();
    let (output, screen) = panel(640.0, |ui| workbench.draw_basics(ui, None, &mut recipe));
    assert_fits_horizontally(&output, screen);
    let hint = placements(
        &output,
        "Describe what this perk does. Leave blank for no description.",
    );
    let command = placements(&output, "Use Effect Summary");
    let (hint, command) = (hint[0].1, command[0].1);
    assert!(
        command.top() > hint.bottom() && command.top() - hint.bottom() < 80.0,
        "the command drifted to {command:?} below the text box at {hint:?}"
    );
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
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
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
        draw(vec![egui::Event::Text("52".into())]);
        for _ in 0..8 {
            output = draw(vec![]);
        }
        // Kind 52 has no plain-language name, so it is reachable only under Advanced and
        // still renders as the engine operation it was traced to.
        let name = format!(
            "52: {}",
            sundial::package_authoring::sandbox_perk::nodes::EFFECTS[52].name
        );
        assert_visible(&output, &name, screen);
        let target = placements(&output, &name)[0].1.center();
        draw(click(target, true));
        draw(click(target, false));
        output = draw(vec![]);
        let use_action = placements(&output, "Add Action")[0].1.center();
        draw(click(use_action, true));
        draw(click(use_action, false));
        assert!(matches!(program.actions.last(), Some(Action::Native { node }) if node.kind == 52));
    }
}

#[test]
fn ingredient_operation_summaries_wrap_inside_the_details_pane() {
    use sundial::package_authoring::sandbox_perk::dependencies::{
        Behavior, DetailLine, DetailSection,
    };
    let behavior = Behavior {
        headline: String::new(), support: Support::Readable, editable: true, program: None,
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
pub(super) fn movement_resource() -> WeaponRuntimeResource {
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
        auxiliary: Vec::new(),
        policy: None,
        alternative_triggers: Vec::new(),
        alternative_removals: Vec::new(),
        native_rearm: None,
        alternative_rearms: Vec::new(),
        additional_groups: Vec::new(),
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

#[test]
fn the_behavior_picker_toolbar_keeps_every_control_reachable_in_a_narrow_window() {
    for (size, trigger) in SIZES
        .into_iter()
        .flat_map(|size| [false, true].map(|trigger| (size, trigger)))
    {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        ctx.set_fonts(fonts);
        if let Some(install) = std::env::var_os("PARHELION_UI_FONT_INSTALL") {
            sundial::investment::configure_authoring_fonts(&ctx, std::path::Path::new(&install))
                .expect("capture fonts must be available");
        }
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let program = Program::default();
        let mut workbench = Workbench::default();
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
                        crate::app::style::perk_workbench_style(ui);
                        if trigger {
                            workbench.behaviors.draw_trigger(
                                ui,
                                &workbench.discovery,
                                &workbench.perk_names,
                                &workbench.asset_labels,
                                "Choose a Trigger",
                                false,
                            );
                        } else {
                            workbench.behaviors.draw_action(
                                ui,
                                &workbench.discovery,
                                &workbench.perk_names,
                                &workbench.asset_labels,
                                &program,
                                &keys,
                            );
                        }
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
        let opener = if trigger {
            format!("Choose a Trigger {}", egui_phosphor::regular::CARET_DOWN)
        } else {
            "Add Action…".to_owned()
        };
        let button = placements(&output, &opener)[0].1.center();
        draw(click(button, true));
        draw(click(button, false));
        for _ in 0..8 {
            output = draw(vec![]);
        }
        // The search box, both new pickers and Show All stay inside the window and clear
        // of each other, wrapping to a second line when the window is too narrow.
        let controls = [
            "Search Behaviors",
            "Any Category",
            "Standard",
            "Any Stock Use",
            "Sort: Suggested",
            "Show All",
        ];
        for control in controls {
            assert_visible(&output, control, screen);
        }
        let rects = controls.map(|control| (control, placements(&output, control)[0].1));
        // A reserved count label must not pull the list back across the toolbar divider.
        for shape in &output.shapes {
            let egui::Shape::LineSegment { points, .. } = &shape.shape else {
                continue;
            };
            if (points[0].y - points[1].y).abs() > 0.1
                || (points[0].x - points[1].x).abs() < size.x * 0.5
            {
                continue;
            }
            for clipped in &output.shapes {
                let egui::Shape::Text(text) = &clipped.shape else {
                    continue;
                };
                let rect = text
                    .galley
                    .rect
                    .translate(text.pos.to_vec2())
                    .intersect(clipped.clip_rect);
                assert!(
                    rect.width() <= 0.0
                        || rect.height() <= 0.0
                        || points[0].y <= rect.top()
                        || points[0].y >= rect.bottom(),
                    "divider crosses {} at {rect:?}",
                    text.galley.job.text
                );
            }
        }
        let kind = if trigger { "trigger" } else { "action" };
        super::capture::write(&ctx, &output, &format!("{kind}-browser-{}", size.x));
        for (index, (name, first)) in rects.iter().enumerate() {
            for (other, second) in &rects[index + 1..] {
                let overlap = first.intersect(*second);
                assert!(
                    overlap.width() <= 1.0 || overlap.height() <= 1.0,
                    "{name} at {first:?} overlaps {other} at {second:?}"
                );
            }
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
        ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
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

#[test]
fn a_label_filter_keeps_its_column_however_long_its_selection_reads() {
    use super::super::controls::{COLUMN_WIDTH, column};
    // A kill filter reads as every label the author chose, joined. Eight of these stacked
    // in one column used to take the whole pane and wrap onto several lines each.
    let selection = "Precision, Grenade, Sword, Super, Melee, Shotgun, Sniper, Fusion, Bow";
    for width in [640.0_f32, 1320.0] {
        let (output, screen) = panel(width, |ui| {
            column(ui, |ui| {
                egui::ComboBox::from_id_salt("filter-column-width")
                    .width(COLUMN_WIDTH)
                    .truncate()
                    .selected_text(selection)
                    .show_ui(ui, |_| {});
            });
        });
        assert_fits_horizontally(&output, screen);
        let (_, rect) = placements(&output, selection)
            .into_iter()
            .next()
            .expect("the filter's selected text");
        assert!(
            rect.width() <= COLUMN_WIDTH,
            "a filter ran to {} at a pane width of {width}, past its {COLUMN_WIDTH} column",
            rect.width()
        );
    }
}

#[test]
fn an_ammunition_row_keeps_its_amount_destination_and_store_inside_a_narrow_pane() {
    use sundial::package_authoring::sandbox_perk::program::{AmmunitionStore, AmmunitionTarget};
    // A label column, an amount, a destination and a store on one line are wider than a
    // narrow pane, so the value has to wrap rather than run off the edge.
    let mut recipe = PerkRecipe::new();
    let mut effect = PerkRecipe::effect(421);
    let mut program = program_recipe().effects[0]
        .program
        .clone()
        .expect("the fixture program");
    program.actions = vec![
        Action::AddRounds {
            rounds: 3,
            target: AmmunitionTarget::ALL[0],
            store: AmmunitionStore::ALL[0],
            overflow: false,
            unit_scaled: false,
            action_scaled: false,
        },
        Action::AddFraction {
            fraction_bits: 0.5_f32.to_bits(),
            target: AmmunitionTarget::ALL[0],
            store: AmmunitionStore::ALL[0],
            capacity: AmmunitionStore::ALL[0],
            overflow: false,
            action_scaled: false,
        },
    ];
    effect.program = Some(program);
    recipe.effects.push(effect);
    let before = recipe.clone();
    let mut workbench = Workbench::default();
    for width in [440.0_f32, 520.0, 640.0, 900.0, 1320.0] {
        let (output, screen) = panel(width, |ui| {
            workbench.draw_effects(ui, Path::new(""), None, &[], &mut recipe, true);
        });
        assert_fits_horizontally(&output, screen);
        // The row has to reach the screen for the assertion above to mean anything.
        for name in ["Rounds", "Share", "To"] {
            assert!(
                !placements(&output, name).is_empty(),
                "{name} was not drawn at a pane width of {width}"
            );
        }
    }
    assert_eq!(recipe, before);
}
