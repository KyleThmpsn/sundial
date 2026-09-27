//! A general predicate with one compiled comparison, edited as the comparison the game makes.
use super::*;

pub(super) fn comparison_editor(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
) -> Result<bool, String> {
    use sundial::package_authoring::sandbox_perk::action::native::predicate;
    let blocks = {
        let mut pending = vec![index];
        let mut seen = std::collections::BTreeSet::new();
        let mut found = Vec::new();
        while let Some(at) = pending.pop() {
            if !seen.insert(at) {
                continue;
            }
            let Some(block) = graph.blocks.get(at) else {
                continue;
            };
            if predicate::read(graph, at).is_some() {
                found.push(at);
            }
            pending.extend(block.links.values().copied());
        }
        found
    };
    let [block] = blocks.as_slice() else {
        return Ok(false);
    };
    let block = *block;
    let current = predicate::read(graph, block).ok_or("The comparison no longer reads.")?;
    let source = graph.blocks[block].links[&0];
    let raw = String::from_utf8_lossy(&graph.blocks[source].bytes)
        .trim_end_matches('\0')
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned();
    let mut variable = raw.clone();
    let mut operation = current.operation;
    let mut threshold = f32::from_bits(current.threshold);
    let known = predicate::variable(&raw);
    // A variable the list does not name is typed under it, as the engine spells it, once
    // Other Value… is chosen or while the condition already compares one.
    let typed = super::Typed::new(ui, "predicate-variable", known.is_some());
    let mut chose_listed = false;
    crate::app::style::tiles(ui, |ui, width| {
        crate::app::style::tile(
            ui,
            width,
            "compared-value",
            "Compared Value",
            "The engine variable this condition compares. Each choice is one the game's own perks compare.",
            false,
            |ui| {
                let selected = known.map_or_else(|| raw.clone(), |v| v.plain.to_owned());
                // A combo takes the width of its selected text and `width` only sets a floor, so
                // the longest variable name would run to the edge of the pane. The allocation
                // bounds it and the evidence for the choice stays on hover.
                let hover = known.map_or_else(
                    || format!("{selected}\nNo stock perk compares this variable."),
                    |v| format!("{selected}\n{}", v.evidence),
                );
                sized(ui, ui.available_width(), |ui| {
                    egui::ComboBox::from_id_salt("predicate-variable")
                        .width(ui.available_width())
                        .truncate()
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for candidate in predicate::VARIABLES {
                                chose_listed |= ui
                                    .selectable_value(
                                        &mut variable,
                                        candidate.name.to_owned(),
                                        candidate.plain,
                                    )
                                    .on_hover_text(candidate.evidence)
                                    .clicked();
                            }
                            typed.row(ui);
                        })
                        .response
                        .on_hover_text(hover);
                    pickers::name_combo(ui, "predicate-variable", "Compared Value");
                });
            },
        );
        if typed.shown {
            crate::app::style::tile(
                ui,
                width,
                "variable-name",
                "Variable Name",
                "",
                false,
                |ui| {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut variable)
                            .desired_width(ui.available_width())
                            .hint_text("engine_variable_name"),
                    );
                    pickers::name_response(ui, &response, "Variable Name");
                },
            );
        }
        crate::app::style::tile(
            ui,
            width,
            "comparison",
            "Comparison",
            "How the value is compared with the threshold.",
            false,
            |ui| {
                let hover = format!("Comparison: {operation}");
                sized(ui, ui.available_width(), |ui| {
                    egui::ComboBox::from_id_salt("predicate-operation")
                        .width(ui.available_width())
                        .truncate()
                        .selected_text(operation)
                        .show_ui(ui, |ui| {
                            for (name, _) in predicate::OPERATIONS {
                                ui.selectable_value(&mut operation, name, name);
                            }
                        })
                        .response
                        .on_hover_text(hover);
                    pickers::name_combo(ui, "predicate-operation", "Comparison");
                });
            },
        );
        crate::app::style::tile(
            ui,
            width,
            "threshold",
            "Threshold",
            "The value the compared value is measured against.",
            false,
            |ui| {
                let threshold_drag = ui.add_sized(
                    [ui.available_width(), ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut threshold).speed(0.1),
                );
                pickers::name_response(ui, &threshold_drag, "Threshold");
            },
        );
    });
    if chose_listed {
        typed.listed(ui);
    }
    // A typed name is written once it is a whole token, so a name half typed is never compiled.
    let variable = variable.trim().to_owned();
    let whole = !variable.is_empty() && !variable.contains(char::is_whitespace);
    if (variable != raw && whole)
        || operation != current.operation
        || threshold.to_bits() != current.threshold
    {
        predicate::rewrite(graph, block, &variable, operation, threshold)?;
    }
    Ok(true)
}
