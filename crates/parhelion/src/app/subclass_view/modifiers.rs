//! What an ability or node changes about the abilities of its subclass while it is selected, as
//! the Ability Changes chips: the keys its stock pool applies, each removable, then its own, then
//! Add Change, which puts one together in a form under them: the ability, what to change, and the
//! value.
use super::*;
use crate::subclass::{
    AbilityModifier, MOST_CHARGES, ModifierEffect, RECHARGE_RANGE, StockModifier, place_entry,
};
use sundial::investment::{AbilityParameter, AbilityRowSummary};
use sundial::package_authoring::ability_bank::{
    ParameterKind, bank_name, parameter_label, parameter_meaning, slot_name,
};
use sundial::package_authoring::sandbox_perk::action::native::fields::keys;

/// A script parameter by its recovered name, or by its hash, as the workbench's tunings name it.
pub(super) fn parameter_name(parameter: u32) -> String {
    parameter_label(parameter).map_or_else(|| format!("Parameter 0x{parameter:08X}"), str::to_owned)
}

/// A parameter's value as the parameter tiles show it, two to four decimals.
pub(super) fn parameter_value(value: f32) -> String {
    egui::emath::format_with_decimals_in_range(f64::from(value), 2..=4)
}

/// A parameter's stock value, after what it does when that is known.
fn stock_hover(parameter: &AbilityParameter) -> String {
    let stock = format!("Stock {}", parameter_value(parameter.reset));
    match parameter_meaning(parameter.name) {
        Some(meaning) => format!("{meaning}\n{stock}"),
        None => stock,
    }
}

/// A value as the workbench's tunings write it: set, or added. A switch set reads On or Off and a
/// multiplier set reads with ×, as the parameter tiles show them.
fn set_or_add(parameter: u32, value: f32, add: bool) -> String {
    if !add {
        let kind = super::tuning::shown_kind(parameter, value, value);
        if kind != ParameterKind::Number {
            return super::tuning::reading(kind, value);
        }
    }
    let shown = parameter_value(value);
    match (add, value.is_sign_negative()) {
        (false, _) => format!("= {shown}"),
        (true, true) => shown,
        (true, false) => format!("+{shown}"),
    }
}

fn charges_label(count: i64) -> String {
    if count == 1 {
        "+1 Charge".to_owned()
    } else {
        format!("{count:+} Charges")
    }
}

/// A key by the name the stock perks that apply it establish, else by the stock nodes whose
/// pools apply it, as High Jump names the key that picks it, else by its hash.
fn key_name(subclasses: &[SubclassSummary], key: u32) -> String {
    if let Some(name) = keys::name(key) {
        return name.to_owned();
    }
    let mut nodes = std::collections::BTreeSet::new();
    for subclass in subclasses {
        for (entry, modifiers) in &subclass.entry_modifiers {
            if modifiers.iter().any(|(each, _)| *each == key)
                && let Some(name) = subclass.entry_names.get(entry)
            {
                nodes.insert(name.as_str());
            }
        }
    }
    if nodes.is_empty() {
        format!("Key 0x{key:08X}")
    } else {
        nodes.into_iter().collect::<Vec<_>>().join(" / ")
    }
}

/// What a key does in an ability's bank: charges it adds, parameters it sets, or the key.
fn key_effect(subclasses: &[SubclassSummary], row: Option<&AbilityRowSummary>, key: u32) -> String {
    let Some(found) = row.and_then(|row| row.keys.iter().find(|each| each.key == key)) else {
        return key_name(subclasses, key);
    };
    if let Some(charges) = found.charges {
        return charges_label(charges);
    }
    if found.parameters.is_empty() {
        return key_name(subclasses, key);
    }
    found
        .parameters
        .iter()
        .map(|parameter| {
            format!(
                "{} {}",
                parameter_name(parameter.name),
                set_or_add(parameter.name, parameter.applied, parameter.add)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn effect_label(
    subclasses: &[SubclassSummary],
    row: Option<&AbilityRowSummary>,
    effect: ModifierEffect,
) -> String {
    match effect {
        ModifierEffect::Key { key } => key_effect(subclasses, row, key),
        ModifierEffect::Charges { count } => charges_label(i64::from(count)),
        ModifierEffect::Parameter {
            parameter,
            value_bits,
            add,
        } => format!(
            "{} {}",
            parameter_name(parameter),
            set_or_add(parameter, f32::from_bits(value_bits), add)
        ),
        ModifierEffect::Recharge { multiplier_bits } => {
            recharge_label(f32::from_bits(multiplier_bits))
        }
    }
}

/// A recharge multiplier as chips read it.
pub(super) fn recharge_label(multiplier: f32) -> String {
    format!("Recharge ×{}", parameter_value(multiplier))
}

/// A recharge multiplier's field: ×1 is stock, higher recharges faster.
pub(super) fn recharge_field(ui: &mut egui::Ui, multiplier: &mut f32) -> egui::Response {
    ui.add(
        egui::DragValue::new(multiplier)
            .speed(0.01)
            .range(RECHARGE_RANGE)
            .clamp_existing_to_range(false)
            .max_decimals(2)
            .prefix("×"),
    )
    .on_hover_text("Higher recharges faster")
}

/// An ability of this subclass a modifier can change: the entry holding it, how the Ability
/// list names it, and its stock row.
#[derive(Clone)]
pub(super) struct Target {
    pub(super) entry: u8,
    pub(super) label: String,
    pub(super) row: u8,
}

/// What Add Change changes about its ability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Charges,
    Recharge,
    Parameter,
    Key,
}

impl Kind {
    const fn label(self) -> &'static str {
        match self {
            Self::Charges => "Extra Charges",
            Self::Recharge => "Recharge",
            Self::Parameter => "Parameter",
            Self::Key => "Stock Key",
        }
    }

    /// Whether the ability's bank can take it.
    fn fits(self, row: &AbilityRowSummary) -> bool {
        match self {
            Self::Charges => row.charges,
            Self::Recharge => row.recharge,
            Self::Parameter => !row.parameters.is_empty(),
            Self::Key => !row.keys.is_empty(),
        }
    }
}

/// The modifier Add Change is putting together.
#[derive(Clone, Copy, Debug)]
pub(super) struct Draft {
    target: u8,
    kind: Kind,
    count: u8,
    multiplier: f32,
    parameter: u32,
    value_bits: u32,
    add: bool,
    key: u32,
}

impl Draft {
    fn new(target: u8) -> Self {
        Self {
            target,
            kind: Kind::Charges,
            count: 1,
            multiplier: 1.5,
            parameter: 0,
            value_bits: 0,
            add: false,
            key: 0,
        }
    }

    /// Keeps the draft to what `row`'s bank offers: a kind it takes, and one of its parameters
    /// and keys.
    fn fit(&mut self, row: &AbilityRowSummary) {
        if !self.kind.fits(row) {
            self.kind = [Kind::Charges, Kind::Recharge, Kind::Parameter, Kind::Key]
                .into_iter()
                .find(|kind| kind.fits(row))
                .unwrap_or(Kind::Key);
        }
        if let Some(first) = ordered(row).first()
            && !row
                .parameters
                .iter()
                .any(|parameter| parameter.name == self.parameter)
        {
            self.parameter = first.name;
            self.value_bits = first.reset.to_bits();
        }
        if let Some(first) = row.keys.first()
            && !row.keys.iter().any(|key| key.key == self.key)
        {
            self.key = first.key;
        }
    }

    fn effect(self) -> ModifierEffect {
        match self.kind {
            Kind::Charges => ModifierEffect::Charges { count: self.count },
            Kind::Recharge => ModifierEffect::Recharge {
                multiplier_bits: self.multiplier.to_bits(),
            },
            Kind::Parameter => ModifierEffect::Parameter {
                parameter: self.parameter,
                value_bits: self.value_bits,
                add: self.add,
            },
            Kind::Key => ModifierEffect::Key { key: self.key },
        }
    }
}

/// A bank's parameters as lists show them: named ones first, then by hash.
pub(super) fn ordered(row: &AbilityRowSummary) -> Vec<&AbilityParameter> {
    let mut ordered = row.parameters.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|parameter| {
        (
            parameter_label(parameter.name).is_none(),
            parameter_name(parameter.name),
        )
    });
    ordered
}

impl PackageAuthoringApp {
    /// The abilities of this subclass a modifier can change, in the order the list shows them.
    pub(super) fn ability_targets(
        &self,
        abilities: &SubclassAbilities,
        base: &SubclassSummary,
    ) -> Vec<Target> {
        Place::editable()
            .filter_map(|place| {
                let (source, entry) = source_of(abilities, base.hash, place);
                let row = *find_subclass(&self.subclasses, source)?
                    .entry_rows
                    .get(&entry)?;
                Some(Target {
                    entry: place_entry(place),
                    label: format!(
                        "{} · {}",
                        place.label(),
                        self.entry_title(abilities, base.hash, place)
                    ),
                    row,
                })
            })
            .collect()
    }

    /// An ability row's name: an ability of this subclass that holds it, else a named stock one,
    /// else its bank's or slot's name. Rows such as sprint are held by entries with no name of
    /// their own, which read as an unknown ability.
    fn row_name(&self, targets: &[Target], row: u8) -> String {
        if let Some(target) = targets.iter().find(|target| target.row == row) {
            return target
                .label
                .split_once(" · ")
                .map_or(target.label.clone(), |(_, name)| name.to_owned());
        }
        let stock = self.subclasses.iter().find_map(|subclass| {
            subclass
                .entry_rows
                .iter()
                .filter(|(_, each)| **each == row)
                .find_map(|(entry, _)| subclass.entry_names.get(entry).cloned())
        });
        let summary = self.ability_row(row);
        stock
            .or_else(|| summary.and_then(|summary| bank_name(summary.bank?)))
            .or_else(|| {
                summary
                    .and_then(|summary| slot_name(summary.slot?))
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| format!("Ability Row {row}"))
    }

    fn ability_row(&self, row: u8) -> Option<&AbilityRowSummary> {
        self.catalog.as_ref()?.ability_row(row)
    }

    /// The entry's modifiers as chips, then Add Change. Returns its edits once they change.
    pub(super) fn draw_entry_modifiers(
        &self,
        ui: &mut egui::Ui,
        (base, abilities): (&SubclassSummary, &SubclassAbilities),
        (summary, entry, place): (Option<&SubclassSummary>, u8, Place),
        edits: &EntryEdits,
        page: &mut PageState,
    ) -> Option<EntryEdits> {
        let targets = self.ability_targets(abilities, base);
        let own = place_entry(place);
        // Extra keys and additive parameter changes can target this ability as well as another.
        let others = targets.clone();
        let stock = summary
            .and_then(|summary| summary.entry_modifiers.get(&entry))
            .cloned()
            .unwrap_or_default();
        let mut changed = None;
        ui.horizontal_wrapped(|ui| {
            for (key, row) in stock {
                let removed = StockModifier { key, row };
                if edits.removed_modifiers.contains(&removed) {
                    continue;
                }
                let label = format!(
                    "{} · {}",
                    self.row_name(&targets, row),
                    key_effect(&self.subclasses, self.ability_row(row), key)
                );
                let (chip, remove, on_remove) =
                    perks::perk_chip(ui, None, &label, egui::Sense::hover());
                if !on_remove {
                    chip.on_hover_text(format!("Stock · Key 0x{key:08X}"));
                }
                if remove {
                    let mut edited = edits.clone();
                    edited.toggle_stock_modifier(removed);
                    changed = Some(edited);
                }
            }
            for (index, modifier) in edits.modifiers.iter().enumerate() {
                let target = targets
                    .iter()
                    .find(|target| target.entry == modifier.target);
                let label = format!(
                    "{} · {}",
                    target.map_or_else(
                        || format!("Entry {}", modifier.target),
                        |target| self.row_name(std::slice::from_ref(target), target.row),
                    ),
                    effect_label(
                        &self.subclasses,
                        target.and_then(|target| self.ability_row(target.row)),
                        modifier.effect
                    )
                );
                let (_, remove, _) = perks::perk_chip(ui, None, &label, egui::Sense::hover());
                if remove {
                    let mut edited = edits.clone();
                    edited.modifiers.remove(index);
                    changed = Some(edited);
                }
            }
            // The button stays selected while its form is open below, and closes it again.
            let open = page.modifier_draft.is_some();
            if ui
                .add_enabled(
                    !others.is_empty(),
                    egui::Button::new("Add Change").selected(open),
                )
                .clicked()
            {
                if open {
                    page.modifier_draft = None;
                    return;
                }
                let first = others
                    .iter()
                    .find(|target| target.entry == own)
                    .or(others.first())
                    .map_or(own, |target| target.entry);
                page.modifier_draft = Some(Draft::new(first));
            }
        });
        if !edits.removed_modifiers.is_empty() {
            egui::CollapsingHeader::new("Removed Stock Changes").show(ui, |ui| {
                for removed in &edits.removed_modifiers {
                    let label = format!(
                        "Restore {} · {}",
                        self.row_name(&targets, removed.row),
                        key_effect(&self.subclasses, self.ability_row(removed.row), removed.key)
                    );
                    if ui.button(label).clicked() {
                        let mut edited = changed.take().unwrap_or_else(|| edits.clone());
                        edited.toggle_stock_modifier(*removed);
                        changed = Some(edited);
                    }
                }
            });
        }
        if let Some(modifier) = self.draw_modifier_form(ui, &others, page) {
            let mut edited = changed.unwrap_or_else(|| edits.clone());
            if !edited.modifiers.contains(&modifier) {
                edited.modifiers.push(modifier);
            }
            changed = Some(edited);
        }
        changed
    }

    /// The Add Change form under the chips while a draft is open, so the page stays in view.
    /// Returns the modifier once it is added.
    fn draw_modifier_form(
        &self,
        ui: &mut egui::Ui,
        targets: &[Target],
        page: &mut PageState,
    ) -> Option<AbilityModifier> {
        let mut draft = page.modifier_draft?;
        let mut added = None;
        let mut close = false;
        ui.add_space(4.0);
        style::block(ui.style()).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let width = (ui.available_width() - LABEL_WIDTH - 8.0).clamp(160.0, 300.0);
            let row = targets
                .iter()
                .find(|target| target.entry == draft.target)
                .and_then(|target| self.ability_row(target.row));
            if let Some(row) = row {
                draft.fit(row);
            }
            egui::Grid::new("subclass-add-modifier-rows")
                .num_columns(2)
                .min_col_width(LABEL_WIDTH)
                .spacing(egui::vec2(8.0, 6.0))
                .show(ui, |ui| {
                    ui.label("Ability");
                    let current = targets
                        .iter()
                        .find(|target| target.entry == draft.target)
                        .map_or("Choose an Ability", |target| target.label.as_str());
                    egui::ComboBox::from_id_salt("subclass-modifier-ability")
                        .width(width)
                        .truncate()
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for target in targets {
                                ui.selectable_value(&mut draft.target, target.entry, &target.label);
                            }
                        });
                    crate::app::pickers::name_combo(ui, "subclass-modifier-ability", "Ability");
                    ui.end_row();
                    ui.label("Change");
                    egui::ComboBox::from_id_salt("subclass-modifier-kind")
                        .width(width)
                        .selected_text(draft.kind.label())
                        .show_ui(ui, |ui| {
                            for kind in [Kind::Charges, Kind::Recharge, Kind::Parameter, Kind::Key]
                            {
                                let fits = row.is_some_and(|row| kind.fits(row));
                                ui.add_enabled_ui(fits, |ui| {
                                    ui.selectable_value(&mut draft.kind, kind, kind.label());
                                });
                            }
                        });
                    crate::app::pickers::name_combo(ui, "subclass-modifier-kind", "Change");
                    ui.end_row();
                    match draft.kind {
                        Kind::Charges => {
                            ui.label("Charges");
                            egui::ComboBox::from_id_salt("subclass-modifier-count")
                                .width(width)
                                .selected_text(charges_label(i64::from(draft.count)))
                                .show_ui(ui, |ui| {
                                    for count in 1..=MOST_CHARGES {
                                        ui.selectable_value(
                                            &mut draft.count,
                                            count,
                                            charges_label(i64::from(count)),
                                        );
                                    }
                                });
                            crate::app::pickers::name_combo(
                                ui,
                                "subclass-modifier-count",
                                "Charges",
                            );
                            ui.end_row();
                        }
                        Kind::Recharge => {
                            ui.label("Recharge");
                            let field = recharge_field(ui, &mut draft.multiplier);
                            style::named_control(field, "Recharge");
                            ui.end_row();
                        }
                        Kind::Parameter => {
                            ui.label("Parameter");
                            egui::ComboBox::from_id_salt("subclass-modifier-parameter")
                                .width(width)
                                .truncate()
                                .selected_text(parameter_name(draft.parameter))
                                .show_ui(ui, |ui| {
                                    for parameter in row.map(ordered).unwrap_or_default() {
                                        if ui
                                            .selectable_label(
                                                draft.parameter == parameter.name,
                                                parameter_name(parameter.name),
                                            )
                                            .on_hover_text(stock_hover(parameter))
                                            .clicked()
                                        {
                                            draft.parameter = parameter.name;
                                            draft.value_bits = parameter.reset.to_bits();
                                        }
                                    }
                                });
                            crate::app::pickers::name_combo(
                                ui,
                                "subclass-modifier-parameter",
                                "Parameter",
                            );
                            ui.end_row();
                            ui.label("Change");
                            egui::ComboBox::from_id_salt("subclass-modifier-change")
                                .width(width)
                                .selected_text(if draft.add { "Add" } else { "Set" })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut draft.add, false, "Set");
                                    ui.selectable_value(&mut draft.add, true, "Add");
                                });
                            crate::app::pickers::name_combo(
                                ui,
                                "subclass-modifier-change",
                                "Change",
                            );
                            ui.end_row();
                            ui.label("Value");
                            ui.horizontal(|ui| {
                                let mut value = f32::from_bits(draft.value_bits);
                                // An added amount is a plain number. A set one is shown as
                                // its kind, as the parameter tiles show it.
                                let kind = if draft.add {
                                    ParameterKind::Number
                                } else {
                                    super::tuning::shown_kind(draft.parameter, value, value)
                                };
                                let field = super::tuning::value_field(ui, kind, &mut value);
                                style::named_control(field, "Value");
                                if value.is_finite() {
                                    draft.value_bits = value.to_bits();
                                }
                                if let Some(stock) = row.and_then(|row| {
                                    row.parameters
                                        .iter()
                                        .find(|parameter| parameter.name == draft.parameter)
                                }) {
                                    ui.weak(format!(
                                        "Stock {}",
                                        super::tuning::reading(
                                            super::tuning::shown_kind(
                                                stock.name,
                                                stock.reset,
                                                stock.reset,
                                            ),
                                            stock.reset,
                                        )
                                    ));
                                }
                            });
                            ui.end_row();
                        }
                        Kind::Key => {
                            ui.label("Key");
                            let reading = key_effect(&self.subclasses, row, draft.key);
                            egui::ComboBox::from_id_salt("subclass-modifier-key")
                                .width(width)
                                .truncate()
                                .selected_text(reading)
                                .show_ui(ui, |ui| {
                                    for key in
                                        row.map(|row| row.keys.as_slice()).unwrap_or_default()
                                    {
                                        ui.selectable_value(
                                            &mut draft.key,
                                            key.key,
                                            key_effect(&self.subclasses, row, key.key),
                                        )
                                        .on_hover_text(format!("Key 0x{:08X}", key.key));
                                    }
                                });
                            crate::app::pickers::name_combo(ui, "subclass-modifier-key", "Key");
                            ui.end_row();
                        }
                    }
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let ready = row.is_some_and(|row| draft.kind.fits(row));
                if ui.add_enabled(ready, egui::Button::new("Add")).clicked() {
                    added = Some(AbilityModifier {
                        target: draft.target,
                        effect: draft.effect(),
                    });
                    close = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        page.modifier_draft = (!close).then_some(draft);
        added
    }
}
