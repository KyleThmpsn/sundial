//! All Requirements on the card: the requirements side by side, each with its hold.
use super::*;

/// The narrowest a requirement column gets before the requirements stack. Its header alone
/// (Requirement, its menu, Stays Met For and the seconds) takes about 300, and a counter's
/// contribution indents a condition inside it. At 260 two columns in a narrow window and three
/// with a counter in a wide one pushed the workbench past its width.
const REQUIREMENT_WIDTH: f32 = 340.0;

/// Every requirement must pass, so they stand side by side joined by And while the pane
/// affords each a readable column, and stack when it does not.
pub(super) fn requirements(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    path: &[usize],
    condition: &DecodedCondition,
    pick: &mut NativePicker<'_>,
    pending: &mut Option<(List, Edit)>,
) -> Result<(), String> {
    let subgroups = List {
        owner: path.to_vec(),
        field: 0x10,
        class: action::SUBGROUP_ROW_CLASS,
    };
    let count = condition.subgroups.len();
    let gap = ui.spacing().item_spacing.x;
    // Room for the And badge and the spacing either side of it.
    let joint = 32.0 + 2.0 * gap;
    let width =
        (ui.available_width() - joint * count.saturating_sub(1) as f32) / count.max(1) as f32;
    let beside = count > 1 && width >= REQUIREMENT_WIDTH;
    let mut requirement = |ui: &mut egui::Ui, row: usize| -> Result<(), String> {
        canvas::block(ui, ("requirement", row), |ui| {
            ui.horizontal_wrapped(|ui| {
                let color = crate::app::style::secondary(ui.visuals());
                ui.label(egui::RichText::new(format!("Requirement {}", row + 1)).color(color));
                // A hold of zero is every stock requirement's, so it appears only once set or
                // asked for from the menu.
                let reveal = ui.make_persistent_id(("requirement-hold", &subgroups.owner, row));
                let mut shown = ui.data(|data| data.get_temp::<bool>(reveal).unwrap_or(false));
                crate::app::style::more_menu(ui, "Requirement", |ui| {
                    let held = condition.subgroups[row].hold != 0.0;
                    if !shown && !held && ui.button("Set Stays Met For").clicked() {
                        shown = true;
                        ui.data_mut(|data| data.insert_temp(reveal, true));
                        ui.close_menu();
                    }
                    // The last requirement goes with the condition that holds it, from that
                    // condition's own menu, so All Requirements never stands empty.
                    if ui
                        .add_enabled(count > 1, egui::Button::new("Remove Requirement"))
                        .on_disabled_hover_text("Use Remove Condition.")
                        .clicked()
                    {
                        *pending = Some((subgroups.clone(), Edit::Remove(row)));
                        ui.close_menu();
                    }
                });
                ui.add_space(12.0);
                requirement_hold(ui, graph, &subgroups, row, shown)
            })
            .inner?;
            let mut owner = path.to_vec();
            owner.push(0x10);
            condition_list(
                ui,
                graph,
                Role::Requirement,
                &condition.subgroups[row].conditions,
                pick,
                List {
                    owner,
                    field: row * 0x20 + 0x10,
                    class: action::CONDITION_ROW_CLASS,
                },
                None,
                pending,
            )
        })
        .inner
    };
    if beside {
        ui.horizontal_top(|ui| {
            for row in 0..count {
                if row > 0 {
                    ui.vertical(|ui| {
                        ui.add_space(6.0);
                        crate::app::style::connector(ui, "And");
                    });
                }
                ui.allocate_ui_with_layout(
                    egui::vec2(width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(width);
                        requirement(ui, row)
                    },
                )
                .inner?;
            }
            Ok::<_, String>(())
        })
        .inner?;
    } else {
        for row in 0..count {
            if row > 0 {
                crate::app::style::connector(ui, "And");
            }
            requirement(ui, row)?;
        }
    }
    let add = ui
        .scope(|ui| {
            crate::app::style::quiet(ui);
            ui.add_enabled(count < LIST_LIMIT, egui::Button::new("Add Requirement"))
                .on_disabled_hover_text("A list holds at most 256 entries.")
                .clicked()
        })
        .inner;
    if add {
        *pending = Some((subgroups, Edit::AddGroup));
    }
    Ok(())
}

/// The most entries a native list holds.
const LIST_LIMIT: usize = 256;

/// A requirement's hold, small and inline: the time its conditions must keep passing.
fn requirement_hold(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    list: &List,
    row: usize,
    shown: bool,
) -> Result<(), String> {
    let mut path = list.owner.clone();
    path.push(list.field);
    structure::scoped(graph, &path, |graph, index| {
        let Some(field) = fields::describe(list.class)?
            .into_iter()
            .find(|field| field.label == "Hold Duration")
        else {
            return Ok(());
        };
        let held = field
            .bytes(&graph.blocks[index], row)
            .is_some_and(|bytes| bytes.iter().any(|byte| *byte != 0));
        if !held && !shown {
            return Ok(());
        }
        let label = super::plain_field_label(list.class, &field.label);
        let hint = fields::contract(list.class, &field).description;
        ui.label(
            egui::RichText::new(label)
                .small()
                .color(crate::app::style::secondary(ui.visuals())),
        )
        .on_hover_text(hint);
        sized(ui, 64.0, |ui| {
            ui.spacing_mut().interact_size.x = 64.0;
            super::scalar(ui, &field, &mut graph.blocks[index], row)
        })
    })
}
