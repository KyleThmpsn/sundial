//! Program picker interactions at small and large viewport sizes.
use super::*;
use sundial::package_authoring::runtime::{BindingHash, SchemaHandle};
use sundial::package_authoring::{
    runtime::{WeaponRuntimeResource, WeaponRuntimeRoot, WeaponRuntimeRootKind},
    sandbox_perk::program::{Action, Program},
};

const SIZES: [egui::Vec2; 2] = [egui::vec2(640.0, 480.0), egui::vec2(1320.0, 900.0)];

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
