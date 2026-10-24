//! Screen-reader checks driven by the accessibility tree egui publishes through AccessKit.
//! A screenshot cannot establish these: a control can look right and still announce
//! nothing. Nothing here opens a window or touches packages.
use super::*;
use egui::accesskit;

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
            !controls.is_empty(),
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
