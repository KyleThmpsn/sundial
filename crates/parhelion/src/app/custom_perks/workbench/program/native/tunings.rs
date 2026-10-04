//! Ability tunings in the Ability Property key picker: the program's own script-parameter
//! rows, offered beside the stock keys of the node's slot, and the modal that defines one.
//! The picker draws deep inside the node editor, which holds the graph but not the program,
//! so the program card publishes its tunings to the frame before the graph draws and takes
//! the picker's definitions back after.
use std::sync::Arc;

use sundial::package_authoring::{
    ability_bank::{
        AbilityTarget, StockParameter, bank_name, parameter_abilities, parameter_label,
        parameter_meaning, parameter_name, slot_banks, slot_name, slot_parameters,
    },
    sandbox_perk::program::AbilityTuning,
};

use crate::app::custom_perks::workbench::controls::{float_field_with, sized};

const PUBLISHED: &str = "ability-tunings";
const CHANGES: &str = "ability-tuning-changes";
const LABEL_WIDTH: f32 = 104.0;
const CONTROL_WIDTH: f32 = 230.0;

/// A tuning a picker defined this frame, replacing the one under `replaced` when it edited
/// an existing tuning.
#[derive(Clone)]
pub(in crate::app::custom_perks::workbench) struct Change {
    pub replaced: Option<u32>,
    pub tuning: AbilityTuning,
}

/// Makes the program's tunings readable by every key picker drawn until `withdraw`.
pub(in crate::app::custom_perks::workbench) fn publish(
    ctx: &egui::Context,
    tunings: &[AbilityTuning],
) {
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new(PUBLISHED),
            Arc::<[AbilityTuning]>::from(tunings.to_vec()),
        );
    });
}

/// The definitions the pickers made since `publish`, which ends the program's frame.
pub(in crate::app::custom_perks::workbench) fn withdraw(ctx: &egui::Context) -> Vec<Change> {
    ctx.data_mut(|data| {
        data.remove::<Arc<[AbilityTuning]>>(egui::Id::new(PUBLISHED));
        data.remove_temp::<Vec<Change>>(egui::Id::new(CHANGES))
            .unwrap_or_default()
    })
}

fn published(ui: &egui::Ui) -> Arc<[AbilityTuning]> {
    ui.ctx()
        .data(|data| data.get_temp::<Arc<[AbilityTuning]>>(egui::Id::new(PUBLISHED)))
        .unwrap_or_else(|| Arc::from(Vec::new()))
}

fn record(ui: &egui::Ui, change: Change) {
    ui.ctx().data_mut(|data| {
        let id = egui::Id::new(CHANGES);
        let mut changes = data.get_temp::<Vec<Change>>(id).unwrap_or_default();
        changes.push(change);
        data.insert_temp(id, changes);
    });
}

/// How a key reads when it is one of the program's tunings.
pub(super) fn reading(ui: &egui::Ui, key: u32) -> Option<String> {
    published(ui)
        .iter()
        .find(|tuning| tuning.key == key)
        .map(label)
}

/// What a tuning key stands for, under the picker.
pub(super) fn evidence(ui: &egui::Ui, key: u32) -> Option<String> {
    published(ui)
        .iter()
        .find(|tuning| tuning.key == key)
        .map(hover)
}

/// The tuning as its key reads: the parameter and the change it makes.
pub(in crate::app::custom_perks::workbench) fn label(tuning: &AbilityTuning) -> String {
    let parameter =
        parameter_label(tuning.parameter).map_or_else(|| unnamed(tuning.parameter), str::to_owned);
    let value = tuning.value();
    if tuning.add {
        format!("{parameter} {value:+}")
    } else {
        format!("{parameter} = {value}")
    }
}

fn unnamed(parameter: u32) -> String {
    format!("Parameter 0x{parameter:08X}")
}

fn hover(tuning: &AbilityTuning) -> String {
    let slot = slot_name(tuning.slot).unwrap_or("this ability");
    let parameter = parameter_name(tuning.parameter).map_or_else(
        || format!("parameter 0x{:08X}", tuning.parameter),
        |named| named.name.to_owned(),
    );
    let change = if tuning.add {
        format!("Adds {} to {parameter}", tuning.value())
    } else {
        format!("Sets {parameter} to {}", tuning.value())
    };
    format!(
        "{change} in every {slot} bank that lists it. This perk's own property, 0x{:08X}.",
        tuning.key
    )
}

/// The rows the key list leads with for the node's slot: the program's tunings on it, then
/// Add Property… and, while the key is a tuning, Edit Property…, above the stock keys.
/// Returns whether a row was chosen, which closes the list.
pub(super) fn rows(
    ui: &mut egui::Ui,
    editor: &Editor,
    slot: AbilityTarget,
    value: &mut u32,
) -> bool {
    let tunings = published(ui);
    let mut chosen = false;
    let own = tunings
        .iter()
        .filter(|tuning| tuning.slot == slot)
        .collect::<Vec<_>>();
    for tuning in &own {
        chosen |= ui
            .selectable_value(value, tuning.key, label(tuning))
            .on_hover_text(hover(tuning))
            .clicked();
    }
    if slot_parameters(slot).is_empty() {
        if !own.is_empty() {
            ui.separator();
        }
        return chosen;
    }
    if ui
        .selectable_label(false, "Add Property…")
        .on_hover_text("A value of this ability this perk sets or adds to.")
        .clicked()
    {
        editor.open(ui, Draft::new(slot, None));
        chosen = true;
    }
    if let Some(current) = own.iter().find(|tuning| tuning.key == *value)
        && ui.selectable_label(false, "Edit Property…").clicked()
    {
        editor.open(ui, Draft::of(current));
        chosen = true;
    }
    ui.separator();
    chosen
}

/// The modal that defines a tuning for one key picker. Its draft lives with the picker's id,
/// so the modal opens for the control that asked for it and nowhere else.
pub(super) struct Editor {
    id: egui::Id,
}

#[derive(Clone)]
struct Draft {
    slot: AbilityTarget,
    replaced: Option<u32>,
    parameter: u32,
    add: bool,
    value_bits: u32,
}

impl Draft {
    /// A new tuning of the parameter the list leads with, at the change the stock rows make.
    fn new(slot: AbilityTarget, replaced: Option<u32>) -> Self {
        let first = ordered(slot).into_iter().next();
        let mut draft = Self {
            slot,
            replaced,
            parameter: 0,
            add: false,
            value_bits: 0,
        };
        if let Some(parameter) = first {
            draft.choose(parameter);
        }
        draft
    }

    fn of(tuning: &AbilityTuning) -> Self {
        Self {
            slot: tuning.slot,
            replaced: Some(tuning.key),
            parameter: tuning.parameter,
            add: tuning.add,
            value_bits: tuning.value_bits,
        }
    }

    /// Takes a parameter with the change its stock rows make, a starting point a reader
    /// adjusts rather than a blank.
    fn choose(&mut self, parameter: &StockParameter) {
        self.parameter = parameter.hash;
        self.add = parameter.add;
        self.value_bits = parameter.applied.to_bits();
    }

    fn tuning(&self) -> AbilityTuning {
        AbilityTuning::new(
            self.slot,
            self.parameter,
            f32::from_bits(self.value_bits),
            self.add,
        )
    }
}

impl Editor {
    pub(super) fn new(ui: &egui::Ui, salt: &str) -> Self {
        Self {
            id: ui.make_persistent_id((salt, "tuning")),
        }
    }

    fn open(&self, ui: &egui::Ui, draft: Draft) {
        ui.data_mut(|data| data.insert_temp(self.id, draft));
    }

    /// Opens the modal on the program's tuning under `key`, from the card beside its reading.
    pub(super) fn edit(&self, ui: &egui::Ui, key: u32) {
        if let Some(tuning) = published(ui).iter().find(|tuning| tuning.key == key) {
            self.open(ui, Draft::of(tuning));
        }
    }

    /// Draws the modal while a draft is open. A confirmed draft becomes the picker's key and
    /// is recorded for the program.
    pub(super) fn show(&self, ui: &mut egui::Ui, value: &mut u32) {
        let Some(mut draft) = ui.data(|data| data.get_temp::<Draft>(self.id)) else {
            return;
        };
        let editing = draft.replaced.is_some();
        let mut confirm = false;
        let mut cancel = false;
        let response = egui::Modal::new(self.id.with("modal")).show(ui.ctx(), |ui| {
            crate::app::style::perk_workbench_style(ui);
            ui.set_width(LABEL_WIDTH + CONTROL_WIDTH + 24.0);
            ui.heading(if editing {
                "Edit Property"
            } else {
                "New Property"
            });
            ui.add_space(4.0);
            egui::Grid::new(self.id.with("rows"))
                .num_columns(2)
                .min_col_width(LABEL_WIDTH)
                .spacing(egui::vec2(8.0, 6.0))
                .show(ui, |ui| {
                    ui.label("Ability");
                    ui.weak(slot_name(draft.slot).unwrap_or("Unplaced"))
                        .on_hover_text(slot_abilities(draft.slot));
                    ui.end_row();
                    ui.label("Changes");
                    parameter_control(ui, &mut draft);
                    ui.end_row();
                    ui.label("How It Changes");
                    change_control(ui, &mut draft.add);
                    ui.end_row();
                    ui.label("Value");
                    ui.horizontal(|ui| {
                        let field = float_field_with(ui, &mut draft.value_bits, "");
                        crate::app::pickers::name_response(ui, &field, "Value");
                        if let Some(stock) = stock(draft.slot, draft.parameter) {
                            ui.weak(format!("Stock {}", stock.reset))
                                .on_hover_text("The value the bank resets the parameter to.");
                        }
                    });
                    ui.end_row();
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                confirm = ui
                    .add(crate::app::style::primary(
                        ui,
                        if editing {
                            "Save Property"
                        } else {
                            "Add Property"
                        },
                    ))
                    .clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        cancel |= response.should_close();
        if confirm {
            let tuning = draft.tuning();
            *value = tuning.key;
            record(
                ui,
                Change {
                    replaced: draft.replaced,
                    tuning,
                },
            );
            ui.data_mut(|data| data.remove::<Draft>(self.id));
        } else if cancel {
            ui.data_mut(|data| data.remove::<Draft>(self.id));
        } else {
            ui.data_mut(|data| data.insert_temp(self.id, draft));
        }
    }
}

/// The slot's parameters as the list shows them: named ones first, then by how many banks
/// list them, so the most widely read parameter leads.
fn ordered(slot: AbilityTarget) -> Vec<&'static StockParameter> {
    let mut ordered = slot_parameters(slot).iter().collect::<Vec<_>>();
    ordered.sort_by_key(|parameter| {
        (
            parameter_label(parameter.hash).is_none(),
            usize::MAX - parameter.banks,
        )
    });
    ordered
}

fn stock(slot: AbilityTarget, parameter: u32) -> Option<&'static StockParameter> {
    slot_parameters(slot)
        .iter()
        .find(|candidate| candidate.hash == parameter)
}

/// The slot's parameters, named ones first, then the rest by how many banks list them. A
/// listed parameter is one the bank's script reads, so nothing else is offered.
fn parameter_control(ui: &mut egui::Ui, draft: &mut Draft) {
    let slot = draft.slot;
    let total = slot_banks(slot).len();
    let ordered = ordered(slot);
    let reading =
        parameter_label(draft.parameter).map_or_else(|| unnamed(draft.parameter), str::to_owned);
    sized(ui, CONTROL_WIDTH, |ui| {
        egui::ComboBox::from_id_salt("tuning-parameter")
            .width(ui.available_width())
            .truncate()
            .selected_text(reading)
            .show_ui(ui, |ui| {
                for parameter in ordered {
                    let name = parameter_label(parameter.hash)
                        .map_or_else(|| unnamed(parameter.hash), str::to_owned);
                    let meaning = parameter_meaning(parameter.hash)
                        .unwrap_or("No name was recovered for this parameter.");
                    let abilities = parameter_abilities(slot, parameter.hash);
                    let listed = if abilities.is_empty() {
                        format!(
                            "{meaning}\nListed by {} of {total} banks. Stock rows reset it to {}.",
                            parameter.banks, parameter.reset
                        )
                    } else {
                        format!(
                            "{meaning}\nListed by {}. Stock rows reset it to {}.",
                            listed_names(&abilities),
                            parameter.reset
                        )
                    };
                    if ui
                        .selectable_label(draft.parameter == parameter.hash, name)
                        .on_hover_text(listed)
                        .clicked()
                    {
                        draft.choose(parameter);
                    }
                }
            });
        crate::app::pickers::name_combo(ui, "tuning-parameter", "Changes");
    });
}

fn change_control(ui: &mut egui::Ui, add: &mut bool) {
    sized(ui, CONTROL_WIDTH, |ui| {
        egui::ComboBox::from_id_salt("tuning-change")
            .width(ui.available_width())
            .selected_text(if *add { "Add" } else { "Set" })
            .show_ui(ui, |ui| {
                ui.selectable_value(add, true, "Add").on_hover_text(
                    "Adds the value to the running one, as Increased Blast Radius adds 0.14.",
                );
                ui.selectable_value(add, false, "Set").on_hover_text(
                    "Writes the value over the running one, as Longer Solar Grenades writes 4.",
                );
            });
        crate::app::pickers::name_combo(ui, "tuning-change", "How It Changes");
    });
}

/// The first few names, then how many more.
fn listed_names(names: &[String]) -> String {
    const SHOWN: usize = 6;
    let shown = names
        .iter()
        .take(SHOWN)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > SHOWN {
        format!("{shown} (+{})", names.len() - SHOWN)
    } else {
        shown
    }
}

/// The abilities a slot's property reaches, by the game's names for their banks.
fn slot_abilities(slot: AbilityTarget) -> String {
    let names = slot_banks(slot)
        .iter()
        .filter_map(|bank| bank_name(*bank))
        .map(|name| name.trim_end_matches(" Bank").to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    if names.is_empty() {
        "Every bank of this ability slot.".to_owned()
    } else {
        let names = names.into_iter().collect::<Vec<_>>();
        format!("Every bank of this slot: {}.", listed_names(&names))
    }
}
