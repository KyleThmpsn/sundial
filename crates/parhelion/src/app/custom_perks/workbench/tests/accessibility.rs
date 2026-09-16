//! Screen-reader checks driven by the accessibility tree egui publishes through AccessKit.
//! A screenshot cannot establish these: a control can look right and still announce
//! nothing. Nothing here opens a window or touches packages.
use super::*;
use egui::accesskit;
use sundial::package_authoring::sandbox_perk::program::NativeNode;

/// Roles a screen reader stops on and announces. A control with one of these roles and no
/// label reads as an unnamed button, which is the usual accessibility defect.
fn is_interactive(role: accesskit::Role) -> bool {
    use accesskit::Role;
    matches!(
        role,
        Role::Button
            | Role::CheckBox
            | Role::ComboBox
            | Role::RadioButton
            | Role::Slider
            | Role::SpinButton
            | Role::TextInput
            | Role::MultilineTextInput
    )
}

/// Every interactive node in the published tree, with whatever a screen reader would
/// announce for it: its own label, or the text of the nodes that label it.
fn announced(update: &accesskit::TreeUpdate) -> Vec<(accesskit::Role, Option<String>)> {
    let nodes = update
        .nodes
        .iter()
        .map(|(id, node)| (*id, node))
        .collect::<BTreeMap<_, _>>();
    update
        .nodes
        .iter()
        .filter(|(_, node)| is_interactive(node.role()))
        .map(|(_, node)| {
            let spoken = node.label().map(str::to_owned).or_else(|| {
                let borrowed = node
                    .labelled_by()
                    .iter()
                    .filter_map(|id| nodes.get(id)?.label())
                    .collect::<Vec<_>>()
                    .join(" ");
                (!borrowed.trim().is_empty()).then_some(borrowed)
            });
            (node.role(), spoken.filter(|text| !text.trim().is_empty()))
        })
        .collect()
}

fn accessibility_tree(output: &egui::FullOutput) -> accesskit::TreeUpdate {
    output
        .platform_output
        .accesskit_update
        .clone()
        .expect("AccessKit is enabled, so every frame publishes a tree")
}

#[test]
fn every_workbench_control_announces_itself_to_a_screen_reader() {
    for size in [egui::vec2(640.0, 480.0), egui::vec2(1320.0, 900.0)] {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut workbench = Workbench {
            open: true,
            initialized: true,
            ..Default::default()
        };
        let mut output = frame(&ctx, &mut workbench, true, size, Vec::new());
        for _ in 0..4 {
            output = frame(&ctx, &mut workbench, true, size, Vec::new());
        }
        let controls = announced(&accessibility_tree(&output));
        assert!(
            controls.len() >= 4,
            "expected the workbench to publish its controls, found {}",
            controls.len()
        );
        let silent = controls
            .iter()
            .filter(|(_, spoken)| spoken.is_none())
            .map(|(role, _)| format!("{role:?}"))
            .collect::<Vec<_>>();
        assert!(
            silent.is_empty(),
            "controls with no accessible name at {size:?}: {silent:?}"
        );
    }
}

/// The names a screen reader announces for one native node's editor, drawn the way the
/// trigger block or the action reader draws it.
fn editor_names(condition: bool, node: NativeNode) -> Vec<String> {
    use sundial::package_authoring::sandbox_perk::program::{Program, Trigger};
    let size = egui::vec2(1320.0, 900.0);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    let mut program = Program {
        trigger: Trigger::Native,
        native_trigger: Some(node.clone()),
        ..Default::default()
    };
    let mut output = None;
    for _ in 0..6 {
        output = Some(ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    crate::app::style::workbench_style(ui);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if condition {
                            program::draw_trigger_block(ui, &mut program, |ui, label, _| {
                                let _ = ui.button(label);
                                None
                            });
                        } else {
                            program::read_native(ui, false, &node);
                        }
                    });
                });
            },
        ));
    }
    let controls = announced(&accessibility_tree(&output.expect("a frame ran")));
    let silent = controls
        .iter()
        .filter(|(_, spoken)| spoken.is_none())
        .map(|(role, _)| format!("{role:?}"))
        .collect::<Vec<_>>();
    assert!(
        silent.is_empty(),
        "kind {} has unnamed controls: {silent:?}. Named: {:?}",
        node.kind,
        controls
            .iter()
            .filter_map(|(_, spoken)| spoken.clone())
            .collect::<Vec<_>>()
    );
    controls
        .into_iter()
        .filter_map(|(_, spoken)| spoken)
        .collect()
}

#[test]
fn promoted_editors_announce_their_fields_in_plain_words() {
    use sundial::package_authoring::sandbox_perk::action::native::predicate;
    let cases: Vec<(bool, NativeNode, &[&str])> = vec![
        (true, NativeNode::condition(23).unwrap(), &["Event"]),
        (true, NativeNode::condition(22).unwrap(), &["Event"]),
        (true, NativeNode::condition(8).unwrap(), &["Ability"]),
        (true, NativeNode::condition(6).unwrap(), &["Ammo Type"]),
        (true, NativeNode::condition(27).unwrap(), &["Shots"]),
        (
            true,
            NativeNode::condition(12).unwrap(),
            &["Event", "Context"],
        ),
        (true, NativeNode::condition(29).unwrap(), &["Signal"]),
        (
            true,
            NativeNode {
                kind: 20,
                bytes: predicate::compose("is_arc", "=", 1.0).unwrap(),
            },
            &["Compared Value", "Comparison"],
        ),
        (false, NativeNode::effect(6).unwrap(), &["Damage Type"]),
        (false, NativeNode::effect(35).unwrap(), &["Firing Mode"]),
        (false, NativeNode::effect(48).unwrap(), &["Game Script"]),
        (false, NativeNode::effect(10).unwrap(), &["Property"]),
    ];
    for (condition, node, expected) in cases {
        let kind = node.kind;
        let names = editor_names(condition, node);
        for name in expected {
            assert!(
                names.iter().any(|spoken| spoken.contains(name)),
                "kind {kind} ({}) does not announce {name:?}. Announced: {names:?}",
                if condition { "condition" } else { "effect" }
            );
        }
        // No control announces the engine's own words for a named selector.
        for jargon in [
            "Native Value",
            "Slot Mask",
            "Event Byte",
            "Flag Mask",
            "Key",
        ] {
            assert!(
                !names.iter().any(|spoken| spoken == jargon),
                "kind {kind} announces the bare word {jargon:?}. Announced: {names:?}"
            );
        }
    }
}

#[test]
fn the_behavior_picker_controls_announce_themselves_by_name() {
    let size = egui::vec2(1320.0, 900.0);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    let program = sundial::package_authoring::sandbox_perk::program::Program::default();
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
                    crate::app::style::workbench_style(ui);
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
    let mut output = draw(Vec::new());
    let open = super::canvas::placements(&output, "Add Action…")[0]
        .1
        .center();
    draw(click(open, true));
    draw(click(open, false));
    for _ in 0..8 {
        output = draw(Vec::new());
    }
    let controls = announced(&accessibility_tree(&output));
    let silent = controls
        .iter()
        .filter(|(_, spoken)| spoken.is_none())
        .map(|(role, _)| format!("{role:?}"))
        .collect::<Vec<_>>();
    assert!(
        silent.is_empty(),
        "picker controls with no accessible name: {silent:?}"
    );
    // The filter and sort controls added for browsing must be reachable by name, not only
    // by their position on screen.
    let spoken = controls
        .iter()
        .filter_map(|(_, spoken)| spoken.clone())
        .collect::<Vec<_>>();
    // A combo announces what it selects, not only its current value, so the controls are
    // reachable by name rather than by position on screen.
    for expected in [
        "Category",
        "Stock Use",
        "Sort Order",
        "Show All",
        "Search Behaviors",
    ] {
        assert!(
            spoken.iter().any(|text| text.contains(expected)),
            "{expected} is not announced. Announced: {spoken:?}"
        );
    }
}
