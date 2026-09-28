//! The counter's two decisions on its card: how many, and what happens after it fires.
use super::*;

/// What a counter does once it fires, read from and written to its Resets At field. The
/// stock perks use two settings: -1 keeps the count, and a reset equal to Count Needed
/// starts over, which is how the ones that fire every few kills are built.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AfterFiring {
    Keep,
    StartOver,
    Custom,
}

fn after_firing(needed: f32, resets_at: f32) -> AfterFiring {
    if resets_at < 0.0 {
        AfterFiring::Keep
    } else if resets_at == needed {
        AfterFiring::StartOver
    } else {
        AfterFiring::Custom
    }
}

/// The counter's two decisions in plain words: how many, and what happens after it fires.
pub(super) fn counter_editor(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
) -> Result<(), String> {
    let fields = fields::describe(0x80803E30)?;
    let field_at = |offset: usize| {
        fields
            .iter()
            .find(|field| field.offset == offset)
            .ok_or("The counter's fields are missing.")
    };
    let needed_field = field_at(0x20)?;
    let resets_field = field_at(0x24)?;
    let read = |graph: &Graph, field: &fields::Field| {
        field
            .bytes(&graph.blocks[index], 0)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .map_or(0.0, f32::from_le_bytes)
    };
    let needed_before = read(graph, needed_field);
    let resets_at = read(graph, resets_field);
    let mode_before = after_firing(needed_before, resets_at);
    let mut needed = needed_before;
    let mut mode = mode_before;
    // A reset count of the reader's own is typed under the list once Other Value… is chosen,
    // or while the node holds one no stock perk uses.
    let typed = super::Typed::new(
        ui,
        "counter-after-firing",
        mode_before != AfterFiring::Custom,
    );
    let mut resets = resets_at;
    let mut chose_listed = false;
    crate::app::style::tiles(ui, |ui, width| {
        crate::app::style::tile(
            ui,
            width,
            "count-needed",
            "Count Needed",
            "How high the counter must reach for this trigger to fire.",
            false,
            |ui| {
                let response = ui.add_sized(
                    [ui.available_width(), ui.spacing().interact_size.y],
                    // A stock count below one stays as it is until someone edits it,
                    // so drawing a copied counter never rewrites it.
                    egui::DragValue::new(&mut needed)
                        .range(1.0..=100_000.0)
                        .clamp_existing_to_range(false)
                        .speed(0.1)
                        .max_decimals(0),
                );
                pickers::name_response(ui, &response, "Count Needed");
            },
        );
        crate::app::style::tile(
            ui,
            width,
            "after-it-fires",
            "After It Fires",
            "Keep Counting leaves the count where it is, so the trigger stays satisfied. Start Over clears the count when it fires. A custom reset value is kept as it is.",
            false,
            |ui| {
                let label = match mode {
                    AfterFiring::Keep => "Keep Counting".to_owned(),
                    AfterFiring::StartOver => "Start Over".to_owned(),
                    AfterFiring::Custom => format!("Custom (Resets at {resets_at})"),
                };
                egui::ComboBox::from_id_salt("counter-after-firing")
                    .width(ui.available_width())
                    .truncate()
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        chose_listed |= ui
                            .selectable_value(&mut mode, AfterFiring::Keep, "Keep Counting")
                            .clicked();
                        chose_listed |= ui
                            .selectable_value(&mut mode, AfterFiring::StartOver, "Start Over")
                            .clicked();
                        if mode_before == AfterFiring::Custom {
                            ui.selectable_value(
                                &mut mode,
                                AfterFiring::Custom,
                                format!("Custom (Resets at {resets_at})"),
                            );
                        }
                        typed.row(ui);
                    });
                pickers::name_combo(ui, "counter-after-firing", "After It Fires");
            },
        );
        if typed.shown {
            crate::app::style::tile(ui, width, "resets-at", "Resets At", "", false, |ui| {
                let response = ui.add_sized(
                    [ui.available_width(), ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut resets)
                        .speed(0.1)
                        .max_decimals(0)
                        .clamp_existing_to_range(false),
                );
                pickers::name_response(ui, &response, "Resets At");
            });
        }
    });
    if chose_listed {
        typed.listed(ui);
    }
    if needed != needed_before {
        needed_field.write(&mut graph.blocks[index], 0, &needed.to_le_bytes())?;
    }
    if resets != resets_at {
        // A typed reset count is the reader's own, whatever it happens to equal.
        resets_field.write(&mut graph.blocks[index], 0, &resets.to_le_bytes())?;
        return Ok(());
    }
    let resets_to = match mode {
        AfterFiring::Keep => -1.0,
        AfterFiring::StartOver => needed,
        AfterFiring::Custom => resets_at,
    };
    if mode != mode_before || (mode == AfterFiring::StartOver && needed != needed_before) {
        resets_field.write(&mut graph.blocks[index], 0, &resets_to.to_le_bytes())?;
    }
    Ok(())
}
