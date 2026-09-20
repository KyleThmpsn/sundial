//! Editable backend for an authored program. The canvas calls one block at a time.
use super::assets::{AssetScope, draw_attach_technical_fields};
use super::controls::{
    COLUMN_WIDTH, NARROW_COLUMN, bytes, column, float_field, hex_key, numbers, seconds, sized,
};
use super::*;
use sundial::package_authoring::sandbox_perk::{
    action::{
        FactValue,
        layout::{self, FieldFormat, Layout},
    },
    nodes,
    program::{
        Action, AmmunitionStore, AmmunitionTarget, Asset, EMPTY_KEY, KeyCatalog, KeyEvidence,
        NativeNode, Position, Program, Trigger, properties::KeyIndex,
    },
};

#[derive(Default)]
pub(super) struct Keys {
    index: Option<Arc<KeyIndex>>,
    sources: Option<Arc<sundial::investment::IngredientCatalog>>,
    pub catalog: KeyCatalog,
}

impl Keys {
    pub fn sync(
        &mut self,
        index: Option<&Arc<KeyIndex>>,
        sources: Option<&Arc<sundial::investment::IngredientCatalog>>,
    ) {
        fn same<T>(a: Option<&Arc<T>>, b: Option<&Arc<T>>) -> bool {
            match (a, b) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
        }
        if same(self.index.as_ref(), index) && same(self.sources.as_ref(), sources) {
            return;
        }
        self.catalog = index.map_or_else(KeyCatalog::default, |index| {
            KeyCatalog::from_index(index, |perk| {
                sources.map_or_else(Vec::new, |sources| {
                    sources
                        .references
                        .get(perk)
                        .iter()
                        .filter(|source| !source.name.starts_with("Item 0x"))
                        .map(|source| source.name.clone())
                        .collect()
                })
            })
        });
        self.index = index.cloned();
        self.sources = sources.cloned();
    }
}

pub(super) fn new_effect(metadata_index: u16) -> WeaponSandboxPerkRuntimeRecipe {
    let mut effect = PerkRecipe::effect(metadata_index);
    effect.program = Some(Program::default());
    effect
}

/// What the user asked for on one action block this frame.
#[derive(Default)]
pub(super) struct ActionEvent {
    /// Open the asset property editor for this action.
    pub edit: bool,
    /// Remove this action from the program.
    pub remove: bool,
    /// Swap this action with the one at the given index.
    pub swap_with: Option<usize>,
}

const DURATION_HINT: &str = "How long the activation stays active. Attached effects end with it. Spawned effects control their own lifetime.";
const COOLDOWN_HINT: &str = "The delay before this effect can activate again.";
const REPEAT_HINT: &str = "The interval at which an always-active effect runs its actions again.";
const EXTEND_HINT: &str = "Seconds added to each running timer on another matching kill.";
const CAP_HINT: &str = "The most time a running timer can hold after the extension.";
pub(super) type ConditionPicker<'a> = dyn FnMut(&mut egui::Ui) -> Option<NativeNode> + 'a;

/// The same behavior catalog supplies built-in and recovered native triggers.
pub(super) fn draw_trigger_block(
    ui: &mut egui::Ui,
    program: &mut Program,
    mut pick: impl FnMut(&mut egui::Ui, &str, bool) -> Option<super::behaviors::Selection>,
) {
    let retained = program.actions.is_empty() || program.actions.iter().any(Action::retained);
    ui.horizontal_wrapped(|ui| {
        let label = program
            .native_trigger
            .as_ref()
            .filter(|_| program.trigger == Trigger::Native)
            .map_or_else(
                || trigger_label(program.trigger, retained).to_owned(),
                native_condition_text,
            );
        if let Some(selection) = pick(ui, &label, retained) {
            match selection {
                super::behaviors::Selection::Trigger(trigger) => change_trigger(program, trigger),
                super::behaviors::Selection::Condition(node) => {
                    program.native_trigger = Some(node);
                    change_trigger(program, Trigger::Native);
                }
                super::behaviors::Selection::Action(_) => {}
            }
        }
        if program.trigger.is_event() {
            ui.label("Chance")
                .on_hover_text("The chance that a matching kill starts the effect.");
            let mut percent = f32::from(program.chance_permyriad) / 100.0;
            if ui
                .add_sized(
                    [controls::CONTROL_WIDTH, ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut percent)
                        .range(0.0..=100.0)
                        .suffix("%"),
                )
                .changed()
            {
                program.chance_permyriad = (percent * 100.0).round() as u16;
            }
        }
    });
    if let (Trigger::Native, Some(node)) = (program.trigger, &mut program.native_trigger) {
        native::draw(ui, "native-trigger", node, NativeFamily::Condition);
    }
    draw_alternatives(
        ui,
        "alternative-trigger",
        "Also Starts When",
        &mut program.alternative_triggers,
    );
    if let Some(policy) = &program.policy {
        ui.small(format!(
            "Execution Policy {}, carried from the stock perk. Its behavior has no controls yet.",
            policy.selector
        ));
    }
    if !program.additional_groups.is_empty() {
        ui.small(format!(
            "{} further program(s) carried from the stock perk without controls.",
            program.additional_groups.len()
        ));
    }
    if let Some(hint) = program.authoring_hint() {
        ui.weak(hint);
    }
}

/// Further conditions a stock action accepts beside the primary one. Each has the same
/// controls as a native node. Removing one is reported by the conversion review.
fn draw_alternatives(ui: &mut egui::Ui, id: &str, heading: &str, nodes: &mut Vec<NativeNode>) {
    if nodes.is_empty() {
        return;
    }
    ui.strong(heading);
    let mut removed = None;
    for (index, node) in nodes.iter_mut().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.label(native_condition_text(node));
            if ui.small_button("Remove").clicked() {
                removed = Some(index);
            }
        });
        native::draw(ui, &format!("{id}-{index}"), node, NativeFamily::Condition);
    }
    if let Some(index) = removed {
        nodes.remove(index);
    }
}

pub(super) fn trigger_label(trigger: Trigger, retained: bool) -> &'static str {
    match (trigger, retained) {
        (Trigger::Drawn, false) => "On Draw",
        (Trigger::Equipped, false) => "On Equip",
        (Trigger::Always, false) => "On Perk Activation",
        _ => trigger.label(),
    }
}

pub(super) mod native;
pub(super) use native::draw_complete;
#[cfg(test)]
mod tests;

/// Keep settings that still apply and remove hidden state from the previous trigger.
fn change_trigger(program: &mut Program, trigger: Trigger) {
    program.trigger = trigger;
    if trigger != Trigger::Native {
        program.native_trigger = None;
    } else {
        program
            .native_trigger
            .get_or_insert_with(|| NativeNode::condition(6).unwrap());
    }
    if trigger != Trigger::Always {
        program.removal_key = None;
    }
    if !matches!(trigger, Trigger::Always | Trigger::Native) {
        program.native_removal = None;
    }
    if program.has_kill_trigger() {
        program.duration_ms = program.duration_ms.max(1);
    } else {
        for action in &mut program.actions {
            match action {
                Action::Spawn { position, .. } => *position = Position::Owner,
                Action::Native { node } if node.kind == 5 && node.bytes.get(2) == Some(&1) => {
                    node.bytes[2] = 0;
                }
                _ => {}
            }
        }
    }
    if !trigger.supports_cooldown() {
        program.cooldown_ms = 0;
    }
}

fn select_ending_key(program: &mut Program, key: Option<u32>) {
    program.removal_key = key;
    if key.is_some() {
        program.native_removal = None;
    }
}

pub(super) fn read_native(ui: &mut egui::Ui, condition: bool, node: &NativeNode) {
    ui.add_enabled_ui(false, |ui| {
        native::draw(
            ui,
            "native-reader",
            &mut node.clone(),
            if condition {
                NativeFamily::Condition
            } else {
                NativeFamily::Effect
            },
        )
    });
}

/// Which node table a native node comes from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NativeFamily {
    Condition,
    Effect,
}

impl NativeFamily {
    fn layouts(self) -> &'static [Layout] {
        match self {
            Self::Condition => layout::CONDITION_LAYOUTS,
            Self::Effect => layout::EFFECT_LAYOUTS,
        }
    }

    fn catalog(self, kind: u8) -> Option<&'static nodes::NodeKind> {
        match self {
            Self::Condition => nodes::condition(kind),
            Self::Effect => nodes::effect(kind),
        }
    }

    fn name(self, kind: u8) -> String {
        match self {
            Self::Condition => nodes::condition_name(kind),
            Self::Effect => nodes::effect_name(kind),
        }
    }
}

/// One field control by format. The stored value only changes on valid input.
fn draw_native_field(ui: &mut egui::Ui, field: &layout::Field, bytes: &mut [u8]) {
    let Some(value) = field.read(bytes) else {
        ui.weak("unreadable");
        return;
    };
    let width = [controls::CONTROL_WIDTH, ui.spacing().interact_size.y];
    // The label is drawn beside the control by the caller, so the control is named here.
    let name = |ui: &egui::Ui, response: &egui::Response| {
        pickers::name_response(ui, response, field.label);
    };
    let changed = match (field.format, value) {
        (FieldFormat::Byte, FactValue::Selector(mut byte)) => {
            let response = ui.add_sized(width, egui::DragValue::new(&mut byte).range(0..=255));
            name(ui, &response);
            response.changed().then_some(FactValue::Selector(byte))
        }
        (FieldFormat::Flag, FactValue::Flag(mut flag)) => {
            let response = ui.checkbox(&mut flag, "");
            name(ui, &response);
            response.changed().then_some(FactValue::Flag(flag))
        }
        (FieldFormat::Mask8, FactValue::Mask(mask)) => {
            let mut byte = mask as u8;
            let response = ui.add_sized(
                width,
                egui::DragValue::new(&mut byte)
                    .range(0..=255)
                    .hexadecimal(2, false, true),
            );
            name(ui, &response);
            response.changed().then(|| FactValue::Mask(byte.into()))
        }
        (FieldFormat::Mask32, FactValue::Mask(mask)) => {
            let mut word = mask as u32;
            let response = hex_key(ui, field.offset, &mut word);
            name(ui, &response);
            (u64::from(word) != mask).then(|| FactValue::Mask(word.into()))
        }
        (FieldFormat::Key, FactValue::Key(mut key)) => {
            let before = key;
            let response = hex_key(ui, field.offset, &mut key);
            name(ui, &response);
            // A key that the label registry names reads as that name, the same way an
            // unmapped key field already does. The registry is engine data, so this adds
            // no interpretation of its own.
            if let Some(name) = sundial::package_authoring::sandbox_perk::action::label_name(key) {
                ui.weak(name)
                    .on_hover_text("Name from the engine's label registry.");
            }
            (key != before).then_some(FactValue::Key(key))
        }
        (FieldFormat::Float, FactValue::Number(number)) => {
            let mut bits = number.to_bits();
            let response = float_field(ui, &mut bits);
            name(ui, &response);
            (bits != number.to_bits()).then(|| FactValue::Number(f32::from_bits(bits)))
        }
        (FieldFormat::Seconds, FactValue::Seconds(value)) => {
            let mut seconds = value;
            let response = ui.add(
                egui::DragValue::new(&mut seconds)
                    .range(0.0..=3600.0)
                    .clamp_existing_to_range(false)
                    .suffix(" s"),
            );
            name(ui, &response);
            response.changed().then_some(FactValue::Seconds(seconds))
        }
        (FieldFormat::Range, FactValue::Range(low, high)) => {
            let (mut low_bits, mut high_bits) = (low.to_bits(), high.to_bits());
            ui.horizontal(|ui| {
                let low = float_field(ui, &mut low_bits);
                pickers::name_response(ui, &low, &format!("{} Low", field.label));
                ui.label("to");
                let high = float_field(ui, &mut high_bits);
                pickers::name_response(ui, &high, &format!("{} High", field.label));
            });
            (low_bits != low.to_bits() || high_bits != high.to_bits())
                .then(|| FactValue::Range(f32::from_bits(low_bits), f32::from_bits(high_bits)))
        }
        _ => None,
    };
    if let Some(value) = changed {
        field.write(bytes, &value);
    }
}

/// The reading of a native condition for a sentence: its kind and its fields.
pub(super) fn native_condition_text(node: &NativeNode) -> String {
    sundial::package_authoring::sandbox_perk::action::decode_condition_node(&node.bytes)
        .map(|condition| condition.description())
        .unwrap_or_else(|_| native_text(node, NativeFamily::Condition))
}

fn native_text(node: &NativeNode, family: NativeFamily) -> String {
    let facts = family
        .layouts()
        .iter()
        .find(|layout| layout.kind == node.kind)
        .map(|layout| layout.facts(&node.bytes))
        .unwrap_or_default();
    if facts.is_empty() {
        family.name(node.kind)
    } else {
        format!(
            "{} ({})",
            family.name(node.kind),
            facts
                .iter()
                .map(sundial::package_authoring::sandbox_perk::action::Fact::render)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl Workbench {
    /// The removal block: a duration for kill triggers, an optional ending event key for an
    /// always-active program, otherwise the fixed pairing the trigger implies.
    pub(super) fn draw_removal_block(&mut self, ui: &mut egui::Ui, program: &mut Program) {
        if program.trigger.is_timed() {
            canvas::row(ui, "Duration", DURATION_HINT, |ui| {
                ui.horizontal_wrapped(|ui| {
                    seconds(
                        ui,
                        "",
                        DURATION_HINT,
                        &mut program.duration_ms,
                        u32::from(program.trigger.is_event()),
                    );
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        seconds(ui, "Cooldown", COOLDOWN_HINT, &mut program.cooldown_ms, 0);
                    });
                });
            });
            if program.trigger == Trigger::Native || program.native_removal.is_some() {
                self.draw_native_ending(ui, program);
            }
        } else if program.trigger == Trigger::Always {
            if program.actions.iter().any(Action::retained) {
                ui.label(if program.removal_key.is_some() {
                    "Ends on a technical event key."
                } else {
                    "Retained effects stay until the perk leaves the weapon."
                });
            }
            self.draw_native_ending(ui, program);
            egui::CollapsingHeader::new("Advanced")
                .id_salt("ending-event-key")
                .default_open(program.removal_key.is_some())
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let mut ends_on_key = program.removal_key.is_some();
                        let reading = if ends_on_key {
                            "On an Event Key"
                        } else {
                            "When the Perk Is Removed"
                        };
                        // A combo takes the width of its selected text, and this row wraps,
                        // so it is held to the shared value column with its full reading on
                        // hover rather than running to the edge of the pane.
                        column(ui, |ui| {
                            egui::ComboBox::from_id_salt("always-removal")
                                .width(COLUMN_WIDTH)
                                .truncate()
                                .selected_text(reading)
                                .show_ui(ui, |ui| {
                                    crate::app::style::workbench_style(ui);
                                    ui.selectable_value(
                                        &mut ends_on_key,
                                        false,
                                        "When the Perk Is Removed",
                                    );
                                    ui.selectable_value(&mut ends_on_key, true, "On an Event Key");
                                })
                                .response
                                .on_hover_text(format!("Effect Removal: {reading}"));
                            pickers::name_combo(ui, "always-removal", "Effect Removal");
                        });
                        match (ends_on_key, program.removal_key) {
                            (false, Some(_)) => select_ending_key(program, None),
                            (true, None) => {
                                select_ending_key(
                                    program,
                                    Some(
                                        self.keys
                                            .catalog
                                            .removal_keys()
                                            .first()
                                            .map_or(EMPTY_KEY, KeyEvidence::hash),
                                    ),
                                );
                            }
                            _ => {}
                        }
                        if let Some(key) = &mut program.removal_key {
                            draw_key_picker(
                                ui,
                                "removal-key",
                                key,
                                self.keys.catalog.removal_keys(),
                                REMOVAL_KEY_NOTE,
                                &mut self.removal_query,
                            );
                        }
                    });
                    if program.removal_key.is_some() {
                        ui.small(
                            "The events behind the keys are unnamed. Counts are installed uses.",
                        );
                        self.draw_key_status(ui);
                    }
                });
        } else if program.actions.iter().any(Action::retained) || program.native_removal.is_some() {
            self.draw_native_ending(ui, program);
        }
        draw_alternatives(
            ui,
            "alternative-removal",
            "Also Ends When",
            &mut program.alternative_removals,
        );
    }

    fn draw_native_ending(&mut self, ui: &mut egui::Ui, program: &mut Program) {
        let label = removal_text(program, Some(&self.keys.catalog))
            .unwrap_or_else(|| "Default for This Trigger".into());
        canvas::row(
            ui,
            "End Condition",
            "Ends this effect when the selected condition passes.",
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(label);
                    if let Some(node) = self.behaviors.draw_condition(
                        ui,
                        &self.discovery,
                        &self.perk_names,
                        &self.asset_labels,
                    ) {
                        program.removal_key = None;
                        program.native_removal = Some(node);
                    }
                    if (program.native_removal.is_some() || program.removal_key.is_some())
                        && ui.small_button("Reset").clicked()
                    {
                        program.native_removal = None;
                        program.removal_key = None;
                    }
                });
                if let Some(node) = &mut program.native_removal {
                    native::draw(ui, "native-removal", node, NativeFamily::Condition);
                }
            },
        );
    }
}

const REMOVAL_KEY_NOTE: &str = "installed ending conditions";

/// A searchable list of installed keys. Unmapped keys remain editable during discovery.
fn draw_key_picker<'a>(
    ui: &mut egui::Ui,
    id: &str,
    key: &mut u32,
    table: &'a [KeyEvidence],
    what: &str,
    query: &mut String,
) -> Option<&'a KeyEvidence> {
    let raw = hex_key(ui, (id, "raw"), key);
    pickers::name_response(ui, &raw, &format!("{what} Key"));
    let current = table.iter().find(|entry| entry.hash() == *key);
    // A key whose purpose the installed data establishes reads as that purpose instead of a
    // list of the perks that happen to use it.
    let label = match current {
        Some(evidence) => match evidence.purpose() {
            Some(purpose) => format!(
                "0x{key:08X} · {}",
                purpose.split(['.', ',']).next().unwrap_or(purpose)
            ),
            None => format!("0x{key:08X} · {}", evidence.seen_in_as(what)),
        },
        None => format!("0x{key:08X}"),
    };
    if let Some(purpose) = current.and_then(KeyEvidence::purpose) {
        ui.label("ⓘ").on_hover_text(purpose);
    }
    let picked = pickers::popup(ui, id, &label, query, |ui, query, reset, height| {
        let choices = table
            .iter()
            .filter(|entry| {
                pickers::matches(query, &format!("{} {}", entry.key, entry.perks.join(" ")))
            })
            .collect::<Vec<_>>();
        pickers::results(
            ui,
            (id, "results"),
            choices.len(),
            height,
            reset,
            sundial::investment::authoring_choice_row_height(ui),
            |ui, index| {
                let entry = choices[index];
                sundial::investment::draw_asset_choice_row(
                    ui,
                    &format!("0x{} · {} {what}", entry.key, entry.nodes),
                    &entry
                        .purpose()
                        .map_or_else(|| entry.seen_in_as(what), str::to_owned),
                    entry.hash() == *key,
                )
                .clicked()
                .then_some(entry)
            },
        )
    });
    if let Some(entry) = picked {
        *key = entry.hash();
    }
    picked
}

/// The rearm block: a cooldown for kill triggers or a repeat interval for an always-active
/// program.
pub(super) fn draw_rearm_block(ui: &mut egui::Ui, program: &mut Program) {
    if program.trigger == Trigger::Always && program.native_rearm.is_none() {
        canvas::row(ui, "Repeat Interval", REPEAT_HINT, |ui| {
            seconds(
                ui,
                "",
                "Zero runs the actions once.",
                &mut program.cooldown_ms,
                0,
            );
        });
    }
    let mut reset = false;
    if let Some(node) = &mut program.native_rearm {
        canvas::row(
            ui,
            "Ready Again When",
            "Rearms this effect when the carried condition passes, in place of a cooldown.",
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(native_condition_text(node));
                    reset = ui.small_button("Reset").clicked();
                });
                native::draw(ui, "native-rearm", node, NativeFamily::Condition);
            },
        );
    }
    if reset {
        program.native_rearm = None;
    }
    draw_alternatives(
        ui,
        "alternative-rearm",
        "Also Ready Again When",
        &mut program.alternative_rearms,
    );
}

pub(super) fn rearm_label(program: &Program) -> &'static str {
    if program.trigger == Trigger::Always {
        "Repeat Interval"
    } else {
        "Cooldown"
    }
}

/// Whether the program has a rearm block to show at all.
pub(super) fn has_rearm(program: &Program) -> bool {
    program.trigger.supports_cooldown()
        || program.native_rearm.is_some()
        || !program.alternative_rearms.is_empty()
}

/// The locked reading of the trigger block.
pub(super) fn trigger_text(program: &Program) -> String {
    let primary = if let (Trigger::Native, Some(node)) = (program.trigger, &program.native_trigger)
    {
        native_condition_text(node)
    } else if program.trigger.is_event() && program.chance_permyriad != 10_000 {
        format!(
            "{} ({}% chance)",
            program.trigger.label(),
            f32::from(program.chance_permyriad) / 100.0
        )
    } else {
        program.trigger.label().to_owned()
    };
    std::iter::once(primary)
        .chain(
            program
                .alternative_triggers
                .iter()
                .map(native_condition_text),
        )
        .collect::<Vec<_>>()
        .join(" or ")
}

/// The locked reading of the removal block, or `None` when the program has no removal list.
pub(super) fn removal_text(program: &Program, keys: Option<&KeyCatalog>) -> Option<String> {
    let primary = if let Some(node) = &program.native_removal {
        Some(format!("When {}", native_condition_text(node)))
    } else {
        match program.trigger {
            Trigger::Native => (program.duration_ms != 0)
                .then(|| format!("After {} s", program.duration_ms as f32 / 1000.0)),
            Trigger::Always => program.removal_key.map(|key| {
                let seen = keys
                    .and_then(|keys| keys.removal_key(key))
                    .map_or_else(String::new, |evidence| {
                        format!(" ({})", evidence.seen_in_as(REMOVAL_KEY_NOTE))
                    });
                format!("On event key 0x{key:08X}{seen}")
            }),
            Trigger::Equipped => Some("The weapon is detached".into()),
            Trigger::Drawn => Some("The weapon is holstered".into()),
            _ => Some(format!("After {} s", program.duration_ms as f32 / 1000.0)),
        }
    };
    let parts = primary
        .into_iter()
        .chain(
            program
                .alternative_removals
                .iter()
                .map(|node| format!("When {}", native_condition_text(node))),
        )
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join(" or "))
}

/// The locked reading of the rearm block, or `None` when the program has no rearm timer.
pub(super) fn rearm_text(program: &Program) -> Option<String> {
    let primary = if let Some(node) = &program.native_rearm {
        Some(format!("When {}", native_condition_text(node)))
    } else if program.trigger.supports_cooldown() && program.cooldown_ms != 0 {
        let seconds = program.cooldown_ms as f32 / 1000.0;
        Some(if program.trigger == Trigger::Always {
            format!("Every {seconds} s")
        } else {
            format!("After {seconds} s")
        })
    } else {
        None
    };
    let parts = primary
        .into_iter()
        .chain(
            program
                .alternative_rearms
                .iter()
                .map(|node| format!("When {}", native_condition_text(node))),
        )
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join(" or "))
}

/// The locked reading of one action block.
pub(super) fn action_text(action: &Action, keys: Option<&KeyCatalog>) -> String {
    let asset_name = |asset: &Asset| {
        if asset.graph == 0 {
            "no asset chosen".to_owned()
        } else if asset.path.is_empty() {
            format!("Asset 0x{:08X}", asset.graph)
        } else {
            sundial::package_authoring::tft::asset_label(&asset.path)
        }
    };
    match action {
        Action::Spawn {
            asset, position, ..
        } => format!(
            "{}: {} at the {}",
            action.label(),
            asset_name(asset),
            position.label()
        ),
        Action::Attach { asset, .. } | Action::Pattern { asset } => {
            format!("{}: {}", action.label(), asset_name(asset))
        }
        Action::ExtendTimers { extend_ms, cap_ms } => format!(
            "{}: {} s more, up to {} s",
            action.label(),
            *extend_ms as f32 / 1000.0,
            *cap_ms as f32 / 1000.0
        ),
        Action::Property {
            key, value_bits, ..
        } => {
            let seen = keys
                .and_then(|keys| keys.property_key(*key))
                .map_or_else(String::new, |evidence| format!(" ({})", evidence.seen_in()));
            format!(
                "{}: 0x{key:08X} = {}{seen}",
                action.label(),
                f32::from_bits(*value_bits)
            )
        }
        Action::AdjustComponent {
            scale_bits,
            value_bits,
            ..
        } => format!(
            "{}: scale {} by {}",
            action.label(),
            f32::from_bits(*scale_bits),
            f32::from_bits(*value_bits)
        ),
        Action::UpdateAccumulator { value_bits, .. } => {
            format!("{}: {}", action.label(), f32::from_bits(*value_bits))
        }
        Action::AbilityProperty { key, option, .. } => {
            let operation = if *option == 0 { "apply" } else { "remove" };
            let property =
                sundial::package_authoring::sandbox_perk::action::native::fields::keys::name(*key)
                    .map_or_else(|| format!("0x{key:08X}"), str::to_owned);
            format!("{}: {operation} {property}", action.label())
        }
        Action::TransmatContext { key } | Action::OverrideHostKey { key, .. } => {
            format!("{}: 0x{key:08X}", action.label())
        }
        Action::SetDamageType { .. } => action.label().to_owned(),
        Action::WeaponReferenceCount { selector } => {
            format!("{}: operation {selector}", action.label())
        }
        Action::AddRounds {
            rounds,
            target,
            store,
            overflow,
            ..
        } => format!(
            "{}: {rounds} {} to the {} of {}{}",
            action.label(),
            if rounds.abs() == 1 { "round" } else { "rounds" },
            store.label().to_lowercase(),
            target.label(),
            if *overflow { ", past capacity" } else { "" }
        ),
        Action::AddFraction {
            fraction_bits,
            target,
            store,
            capacity,
            overflow,
            ..
        } => format!(
            "{}: {}% of the {} capacity to the {} of {}{}",
            action.label(),
            f32::from_bits(*fraction_bits) * 100.0,
            capacity.label().to_lowercase(),
            store.label().to_lowercase(),
            target.label(),
            if *overflow { ", past capacity" } else { "" }
        ),
        Action::Native { node } if node.kind == 5 && node.bytes.len() >= 32 => {
            let count = u32::from_le_bytes(node.bytes[4..8].try_into().expect("orb count"));
            let entity = if count == 1 {
                "Orb of Light"
            } else {
                "Orbs of Light"
            };
            let place = match node.bytes[2] {
                0 => "at your position",
                1 => "at the triggering event",
                _ => "using its native position setting",
            };
            format!("Generate {count} {entity} {place}")
        }
        Action::Native { node } => native_text(node, NativeFamily::Effect),
    }
}

/// What one action does, for the hover on its title.
fn action_description(action: &Action) -> &'static str {
    match action {
        Action::Native { node } => nodes::effect(node.kind).map_or(
            "A native effect node carried as the client stores it.",
            |entry| entry.summary,
        ),
        Action::Spawn { .. } => {
            "Creates the entity once per activation. The entity controls its own lifetime."
        }
        Action::Attach { .. } => "Attaches the entity to the weapon until the effect ends.",
        Action::Pattern { .. } => "Uses this projectile pattern while the effect is active.",
        Action::ExtendTimers { .. } => {
            "Another matching kill while the effect is active adds time to its running timers, up to the cap."
        }
        Action::Property { .. } => {
            "Sets a native property to a constant while the effect is active. The keys are not mapped."
        }
        Action::AdjustComponent { .. } => {
            "Scales the selected ability's energy by the value. The ability follows the target selector, established from eleven stock perks."
        }
        Action::UpdateAccumulator { .. } => {
            "Writes the accumulator that this program's Accumulator condition counts toward its threshold."
        }
        Action::AbilityProperty { .. } => {
            "Applies or removes a named property on the base ability. The change is reversed when the effect ends."
        }
        Action::TransmatContext { .. } => {
            "Plays the transmat effect the key names. What consumes the key is not resolved, so it is carried as the game stores it."
        }
        Action::OverrideHostKey { .. } => {
            "Replaces a key on the weapon or player while the effect is active. The selector bytes are carried as the game stores them."
        }
        Action::SetDamageType { .. } => {
            "Changes the weapon's element. The mode byte is the damage type, established from the element perks that each name their own."
        }
        Action::WeaponReferenceCount { .. } => {
            "Holds a weapon reference count while the effect is active. What the operation byte selects is not resolved."
        }
        Action::AddRounds { .. } => {
            "Adds whole rounds when the effect starts, the way Triple Tap returns a round."
        }
        Action::AddFraction { .. } => {
            "Adds a share of a capacity when the effect starts, the way kill-to-reload perks refill half a magazine."
        }
    }
}

/// A name in the words a player uses, for the effect kinds whose traced behavior says
/// plainly what happens in game.
///
/// A kind is listed here only when its recorded evidence in `nodes.rs` leaves the behavior
/// resolved. Kinds whose evidence ends in "remains unresolved" stay off this list even when
/// their fields are editable, because the workbench would be putting words to something it
/// has not established. Everything not listed keeps the engine's own traced name and is
/// held behind the picker's Advanced setting.
pub(super) fn plain_action_title(kind: u8) -> Option<&'static str> {
    Some(match kind {
        1 => "Attach an Effect",
        2 => "Attach an Effect with a Dynamic Value",
        3 => "Spawn an Object or Effect",
        4 => "Apply an Effect to a Chosen Target",
        5 => "Generate Orbs of Light",
        6 => "Change Damage Type",
        7 => "Change an Ability Stat",
        8 => "Change Ability Energy",
        10 => "Change a Weapon or Ability Stat",
        11 => "Change Ammo Drop Chance",
        13 => "Drop Ammo by Weighted Chance",
        14 => "Adjust Ammunition",
        15 => "Adjust Ammunition by Capacity",
        16 => "Reload from Reserves",
        18 => "Set Radar Detection Range",
        26 => "Change Fired Projectile",
        32 => "Extend Timers",
        35 => "Set a Weapon Firing Mode",
        37 => "Label the Event when the Damage Source Matches",
        40 => "Change the Event's Values",
        42 => "Set the Effect's Counter",
        47 => "Set Transmat Effect",
        48 => "Run a Game Script",
        53 => "Adjust Several Named Values",
        54 => "Label the Event when the Target Matches",
        _ => return None,
    })
}

/// What a player would be told this effect kind does. Present for exactly the kinds
/// `plain_action_title` names, which the tests enforce.
pub(super) fn plain_action_summary(kind: u8) -> Option<&'static str> {
    Some(match kind {
        1 => "Keep an entity attached for the duration of the effect.",
        2 => "Keep an effect attached and drive one of its values from a formula.",
        3 => {
            "Spawn a pickup, relic, world object, projectile or effect at the player or event location."
        }
        4 => "Apply a referenced effect to a target the action selects.",
        5 => {
            "Create a collectible Orb of Light at the kill location or at the player, the way Masterwork weapons do."
        }
        6 => {
            "Change the weapon's damage type to Kinetic, Solar, Arc or Void, as The Fundamentals does."
        }
        7 => "Change a named stat inside an ability.",
        8 => "Scale grenade, melee or class ability energy, with an optional limit.",
        10 => "Adjust a named weapon or ability property.",
        // Kind 11: the Primary, Special and Heavy Ammo Finder mods each write one of the
        // three triples, so the triples are per ammo type. All four described stock perks
        // read "increases the drop chance of ... ammo on kill".
        11 => {
            "Add to the ammo drop chances by ammo type, as the Primary, Special and Heavy Ammo Finder mods do. Each type has three values, and the Finder mods set the second."
        }
        // Kind 13: the three weights are the ammo types. Snapload Finisher weights only the
        // first and generates Primary ammo, Special Finisher only the second, Heavy Finisher
        // only the third, and every other described stock perk agrees.
        13 => {
            "Pick Primary, Special or Heavy ammo by weight and drop it, as Special Finisher weights only Special and Heavy Finisher only Heavy."
        }
        14 => "Add whole rounds to the magazine or reserves, as Triple Tap returns a round.",
        15 => {
            "Add a share of the magazine or reserve capacity, as kill-to-reload perks refill half a magazine."
        }
        16 => "Move ammunition out of reserves and into the magazine, without adding any new ammo.",
        // Kind 18: Long March and Radar Booster, the only stock uses, both write the third
        // host float (80 and 56) and leave the other two at -1, which the callback leaves
        // unchanged.
        18 => {
            "Replace the radar detection range while the effect is active, as Long March sets 80 and Radar Booster 56. A value of -1 leaves a setting unchanged, and the other two settings are not identified."
        }
        26 => "Use a selected projectile pattern while the effect is active.",
        32 => {
            "Another matching kill while the effect is active adds time to its running timers, as Outlaw does."
        }
        // Kind 35: every stock use is a firing mode. Full Auto Trigger System, Rapid-Fire
        // Frame and Thunderer write the same key, and Fan Fire clears it.
        35 => {
            "Set the weapon's firing mode while the effect is active. Every stock use is a firing mode: Full Auto Trigger System, Rapid-Fire Frame and Thunderer all set full auto."
        }
        37 => {
            "Add labels to the event that started this effect when its damage source filter passes, so other perks can read them. The stock filters name weapon families and abilities, and the champion mods use it to add labels such as stagger and overload."
        }
        40 => "Change the values the triggering event carries, after its filters pass.",
        42 => "Write the counter value that accumulator conditions read.",
        // Kind 47: all 112 stock perks that carry it belong to Transmat Effect items, one key
        // each, which is what the key identifies. The client code that reads the key has
        // not been traced, and the sentence says so.
        47 => {
            "Set which transmat effect this perk carries. Every one of the 112 stock perks with this action belongs to a Transmat Effect item. The game code that reads it has not been traced."
        }
        // Kind 48: the referenced resource is a behavior script whose path names it, such
        // as apply_tiered_charge_of_light, the one 17 Charged with Light mods run.
        48 => {
            "Run an object-behavior script from the installed game packages, such as applying a stack of Charged with Light. Some scripts need specific game state or an owning object. Test the combination in game."
        }
        53 => "Add to, replace or multiply the named values that match.",
        54 => {
            "Add labels to the event that started this effect when its target filter passes, so other perks can read them. Stock perks use it for the champion effects: stagger, pierce and overload."
        }
        _ => return None,
    })
}

/// A name in the words a player uses for a condition kind, on the same terms as
/// `plain_action_title`: listed only where the traced evidence leaves the behavior resolved.
pub(super) fn plain_condition_title(kind: u8) -> Option<&'static str> {
    Some(match kind {
        0 => "Always",
        1 => "After a Delay",
        2 => "On a Kill",
        4 => "On Dealing Damage",
        5 => "On Taking Damage",
        6 => "On Picking Up Ammo",
        8 => "On Using an Ability",
        9 => "On Activating an Ability",
        12 => "On a Game Event",
        14 => "When the Weapon Is Equipped",
        15 => "When the Weapon Is Unequipped",
        16 => "When the Weapon Is Drawn",
        17 => "When the Weapon Is Holstered",
        19 => "On Reloading",
        22 => "On Crouching",
        23 => "On Aiming Down Sights",
        26 => "After Enough Stacks",
        27 => "On Firing This Weapon",
        29 => "On a Game Signal",
        30 => "Ends on a Game Signal",
        31 => "When All Requirements Are Met",
        42 => "On a Finisher",
        _ => return None,
    })
}

/// What a player would be told this condition kind does. Present for exactly the kinds
/// `plain_condition_title` names.
pub(super) fn plain_condition_summary(kind: u8) -> Option<&'static str> {
    Some(match kind {
        0 => "Always passes. The effect still obeys its chance and its trigger.",
        1 => {
            "Waits a fixed number of seconds. The same condition serves as a duration and as a cooldown."
        }
        2 => "Passes on a kill. It can require this weapon and a label such as a precision hit.",
        // Kind 4: every described stock perk on it reads as damage dealt, from Impact
        // Induction ("causing damage with a melee attack") and The Perfect Fifth ("precision
        // hits") to Disruption Break ("breaking an enemy's shield with this weapon"). Its
        // label filter names what dealt the damage.
        4 => {
            "Passes when damage is dealt, as Impact Induction reads a melee hit and The Perfect Fifth a precision hit. Its labels name what dealt it, such as precision, grenade or sword."
        }
        // Kind 6: every described stock perk on it is a Scavenger or Lead from Gold, and all
        // read "when you pick up ammo". The ammunition type mask is named from the same perks.
        6 => {
            "Passes when ammunition is picked up, as every Scavenger perk does. Its ammo type setting picks Primary, Special or Heavy."
        }
        // Kind 8: every described stock perk on it reads as an ability cast, from Bomber
        // ("when using your class ability") to Radiant Light ("casting your Super"). The
        // ability mask is named from the same perks.
        8 => {
            "Passes when an ability is used. Its ability setting picks the grenade, the Super or the class ability, as Bomber reads the class ability and Radiant Light the Super."
        }
        // Kind 9: the event carries the ability slot as a bit, numbered as kind 8's mask.
        // Resolute and Volatile Conduction read Super casts on bit 1, Aeon Energy a dodge
        // on bit 7.
        9 => {
            "Passes when the selected ability activates, as Resolute reads a Super cast and Aeon Energy a dodge. Its ability setting picks the Super or the class ability."
        }
        // Kind 5: Dreaded Visage ("when you're damaged"), Arc Conductor ("taking Arc
        // damage"), Vengeance ("those that harm you") and the Taken, Fallen and Hive Barrier
        // mods ("receiving Taken damage") all read as damage taken.
        5 => {
            "Passes when the player takes damage, as Dreaded Visage, Arc Conductor and the Taken, Fallen and Hive Barrier mods read. Its labels name what dealt the damage."
        }
        // Kind 12: the event and context keys are named from the perks that listen to them,
        // 19 of them on the Orb of Light pickup alone.
        12 => {
            "Passes when a game event fires, such as picking up an Orb of Light, the event Innervation and Recuperation listen to. The events on offer are the ones stock perks listen to."
        }
        // Kind 19: all 18 stock perks with the reload flag set read as reloading, from Kill
        // Clip and Impetus starting to Under Pressure and High-Impact Reserves ending.
        19 => {
            "Passes when this weapon is reloaded, as Kill Clip starts and Under Pressure ends. On Reload is the setting all 18 of those stock perks use. The second weapon event is one no stock description names."
        }
        22 => {
            "Passes when crouching starts or ends, as Field Prep, Firmly Planted and Sneak Bow use it. Its event setting picks which."
        }
        23 => {
            "Passes when aiming down sights starts or stops, as Rangefinder starts on aiming and Hip-Fire Grip on leaving it. Its event setting picks which."
        }
        14 => "Passes when this weapon is equipped to the character.",
        15 => "Passes when this weapon is no longer equipped.",
        16 => "Passes when this weapon is drawn.",
        17 => "Passes when this weapon is put away.",
        26 => {
            "Counts toward a threshold and passes once it is reached. Its rows add to, replace or multiply the stored count."
        }
        // Kind 27: every described stock perk reads as a shot fired, from Tap the Trigger
        // and Under Pressure starting on one to Box Breathing and The Perfect Fifth ending.
        27 => {
            "Passes when a shot is fired, as Tap the Trigger starts and Box Breathing resets. Its mode can restrict it to a missed shot, as Mulligan and Reversal of Fortune do."
        }
        29 => {
            "Passes when a game signal fires, such as collecting a Warmind Cell or standing near a Vex Relay. The signals on offer are the ones stock perks start on."
        }
        30 => {
            "Ends the effect when a game signal fires. An always-active effect ends on its own signal, and Relay Defender and Resistant Tether end on the signals they started on."
        }
        42 => {
            "Passes on a finisher, as Bulwark Finisher reads the final blow and Reactive Pulse the finisher starting and ending. Its event setting picks which."
        }
        // Kind 31 is structural and fully traced: every subgroup must pass, and the
        // conditions inside one subgroup are alternatives. Each subgroup reads as one
        // requirement. Backup Plan and Archer's Gambit use it to require two things at once.
        31 => {
            "Passes only when every requirement below is met. A requirement is met by any one of the conditions listed under it, as Archer's Gambit needs both a hip fire state and a precision hit."
        }
        _ => return None,
    })
}

/// The condition title a reader sees: the plain one where it exists, otherwise the engine's
/// own traced name.
/// Whether a decoded reading is the engine's own fallback rather than a real description.
///
/// `describe_condition` and `describe_effect` end by handing back the node's catalogue
/// summary, or its name when it has none. When that is what came back, the reading says
/// nothing the plain table cannot say better, and the plain table is what a player reads.
fn fell_back(summary: Option<&'static str>, name: String, decoded: &str) -> bool {
    summary.is_some_and(|text| text == decoded) || name == decoded
}

/// A condition's reading for a card, in plain words where the engine had none of its own.
pub(super) fn native_condition_reading(kind: u8, decoded: &str) -> String {
    let catalogued = nodes::condition(kind).map(|node| node.summary);
    if fell_back(catalogued, nodes::condition_name(kind), decoded) {
        if let Some(plain) = plain_condition_summary(kind) {
            return plain.to_owned();
        }
        if let Some(plain) = plain_condition_title(kind) {
            return plain.to_owned();
        }
    }
    decoded.to_owned()
}

/// An effect's reading for a card, on the same rule.
pub(super) fn native_action_reading(kind: u8, decoded: &str) -> String {
    let catalogued = nodes::effect(kind).map(|node| node.summary);
    if fell_back(catalogued, nodes::effect_name(kind), decoded)
        && let Some(plain) = plain_action_summary(kind)
    {
        return plain.to_owned();
    }
    decoded.to_owned()
}

pub(super) fn native_condition_title(kind: u8) -> String {
    plain_condition_title(kind).map_or_else(|| nodes::condition_name(kind), str::to_owned)
}

/// The engine effect kind a typed action compiles to, so the two name themselves alike.
pub(super) fn action_kind(action: &Action) -> u8 {
    match action {
        Action::Attach { .. } => 1,
        Action::Spawn { .. } => 3,
        Action::Pattern { .. } => 26,
        Action::ExtendTimers { .. } => 32,
        Action::Property { .. } => 10,
        Action::AdjustComponent { .. } => 8,
        Action::UpdateAccumulator { .. } => 42,
        Action::AbilityProperty { .. } => 7,
        Action::TransmatContext { .. } => 47,
        Action::OverrideHostKey { .. } => 35,
        Action::SetDamageType { .. } => 6,
        Action::WeaponReferenceCount { .. } => 30,
        Action::AddRounds { .. } => 14,
        Action::AddFraction { .. } => 15,
        Action::Native { node } => node.kind,
    }
}

pub(super) fn native_action_title(kind: u8) -> &'static str {
    plain_action_title(kind)
        .unwrap_or_else(|| nodes::effect(kind).map_or("Action", |node| node.name))
}

pub(super) fn native_action_label(kind: u8, bytes: &[u8]) -> String {
    if kind == 8
        && let Some(values) = bytes.get(2..5)
        && let Some(role) = sundial::package_authoring::sandbox_perk::action::component_target(
            values[0], values[1], values[2],
        )
    {
        return format!("Adjust {role}");
    }
    native_action_title(kind).to_owned()
}

/// What a placed action calls itself on its card. The picker's offers are checked against
/// this, so the name that places an action and the name it then carries are one string.
pub(super) fn action_title(action: &Action) -> String {
    match action {
        Action::Native { node } => native_action_label(node.kind, &node.bytes),
        // Every typed action is one of the engine's effect kinds, so it takes the plain name
        // that kind already has. Falling back to the engine label put "Named Property" and
        // "Update Accumulator" on a card while the picker offered the same thing in plain
        // words.
        other => native_action_title(action_kind(other)).to_owned(),
    }
}

/// Effect kinds offered as guided actions in their own right, beyond the ones the workbench
/// composes as typed `Action` variants. Each has a plain title and sentence, and the tests
/// assert that every one of them actually produces an editable node.
pub(super) const PROMOTED_NATIVE_ACTIONS: [u8; 10] = [2, 4, 11, 13, 16, 18, 37, 40, 48, 53];

/// One guided condition per engine variable the stock perks compare: a general predicate
/// composed from the stock template with that variable's own comparison. The variable
/// names come from the client's compiled source strings, so each row is a comparison the
/// game itself makes, and the editor lets the value, the operation and the variable change.
pub(super) fn compiled_comparisons() -> Vec<(String, String, NativeNode)> {
    use sundial::package_authoring::sandbox_perk::action::native::predicate;
    predicate::VARIABLES
        .iter()
        .filter_map(|variable| {
            let bytes =
                predicate::compose(variable.name, variable.operation, variable.threshold).ok()?;
            Some((
                variable.plain.to_owned(),
                format!(
                    "Passes while {} {} {}. {} The value, the comparison and the tracked variable can be changed.",
                    variable.plain, variable.operation, variable.threshold, variable.evidence
                ),
                NativeNode { kind: 20, bytes },
            ))
        })
        .collect()
}

pub(super) fn common_actions(
    program: &Program,
    keys: &KeyCatalog,
) -> Vec<(&'static str, &'static str, Action)> {
    let mut actions = vec![
        (
            "Spawn an Object or Effect",
            "Spawn a pickup, relic, world object, projectile or effect at the player or event location.",
            Action::Spawn {
                asset: Asset::default(),
                position: if program.has_kill_trigger() {
                    Position::Event
                } else {
                    Position::Owner
                },
            },
        ),
        (
            "Generate Orbs of Light",
            "Use the native masterwork operation to create a collectible orb at the kill location or owner.",
            Action::generate_orb(if program.has_kill_trigger() {
                Position::Event
            } else {
                Position::Owner
            }),
        ),
        (
            "Attach an Effect",
            "Keep an entity attached for the duration of the effect.",
            Action::attach(Asset::default()),
        ),
        (
            "Change Fired Projectile",
            "Use a selected projectile pattern while the effect is active.",
            Action::Pattern {
                asset: Asset::default(),
            },
        ),
        (
            "Extend Timers",
            "Another matching kill while the effect is active adds time to its running timers, as Outlaw does. Needs a kill trigger.",
            Action::ExtendTimers {
                extend_ms: program.duration_ms.max(1_000),
                cap_ms: program.duration_ms.max(1_000),
            },
        ),
        (
            native_action_title(14),
            "Add whole rounds to the magazine or reserves, as Triple Tap returns a round.",
            Action::add_rounds(1),
        ),
        (
            native_action_title(15),
            "Add a share of the magazine or reserve capacity, as kill-to-reload perks refill half a magazine.",
            Action::add_fraction(0.5),
        ),
        (
            native_action_title(10),
            "Adjust a named weapon or ability property.",
            keys.property_keys()
                .first()
                .map_or_else(|| Action::property(EMPTY_KEY), Action::property_from),
        ),
        (
            native_action_title(8),
            "Grant or drain grenade, melee, super or class ability energy, as Ashes to Assets and Bomber do.",
            Action::adjust_component(0),
        ),
        (
            native_action_title(42),
            "Write the value the program's Accumulator condition counts toward its threshold.",
            Action::update_accumulator(1.0),
        ),
        (
            native_action_title(7),
            "Change a named property of one ability, as And Another Thing grants an extra grenade charge and Jump Jets improves the jump.",
            Action::ability_property(0),
        ),
        (
            "Change Damage Type",
            "Change the weapon's element, the way The Fundamentals and the element mods do.",
            Action::set_damage_type(1),
        ),
        (
            "Set Transmat Effect",
            "Play a chosen transmat effect. The effect is named by a key.",
            Action::transmat_context(EMPTY_KEY),
        ),
        (
            native_action_title(35),
            "Replace a key on the weapon or the player while the effect is active, as the firing mode perks do.",
            Action::override_host_key(EMPTY_KEY),
        ),
        (
            native_action_title(30),
            "Hold a weapon reference count while the effect is active.",
            Action::weapon_reference_count(0),
        ),
    ];
    // A native kind is offered as a guided action once its traced behavior supports a plain
    // sentence and its fields already carry names. Each of these keeps its complete native
    // record; the guided entry supplies the name, the sentence and a starting configuration.
    for kind in PROMOTED_NATIVE_ACTIONS {
        if let (Some(node), Some(title), Some(summary)) = (
            NativeNode::effect(kind),
            plain_action_title(kind),
            plain_action_summary(kind),
        ) {
            actions.push((title, summary, Action::Native { node }));
        }
    }
    actions
}

impl Workbench {
    /// One editable action block. The caller applies the returned event after the loop.
    pub(super) fn draw_action_block(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        kill_trigger: bool,
        action: &mut Action,
        index: usize,
        count: usize,
    ) -> ActionEvent {
        let mut event = ActionEvent::default();
        let mut properties = super::properties::Panel::new(ui, "action");
        let scope = match action {
            Action::Pattern { .. } => AssetScope::Projectiles,
            Action::Spawn { .. } => AssetScope::Spawnable,
            _ => AssetScope::Any,
        };
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                crate::app::style::more_menu(ui, |ui| {
                    crate::app::style::workbench_style(ui);
                    if ui
                        .add_enabled(index > 0, egui::Button::new("Move Up"))
                        .clicked()
                    {
                        event.swap_with = Some(index - 1);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(index + 1 < count, egui::Button::new("Move Down"))
                        .clicked()
                    {
                        event.swap_with = Some(index + 1);
                        ui.close_menu();
                    }
                    if ui.button("Remove Action").clicked() {
                        event.remove = true;
                        ui.close_menu();
                    }
                });
                // The panel holds the technical bytes and the referenced object. An action
                // with neither, such as Extend Timers, has nothing to put behind the button.
                if action.asset().is_some() || has_technical_fields(action) {
                    properties.button(ui);
                }
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                    egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                    |ui| {
                        let title = action_title(action);
                        ui.strong(format!("{}. {}", index + 1, title))
                            .on_hover_text(action_description(action));
                    },
                );
            });
        });
        if let Some(asset) = action.asset_mut() {
            let label = match scope {
                AssetScope::Projectiles => "Projectile",
                AssetScope::Spawnable => "Object or Effect",
                AssetScope::Any => "Attachment",
            };
            let missing = asset.graph == 0;
            properties::row_with(
                ui,
                label,
                "The asset this action uses.",
                properties::Emphasis::Plain,
                |ui| {
                    if missing {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!("{label} *"))
                                    .color(ui.visuals().warn_fg_color),
                            )
                            .truncate(),
                        )
                        .on_hover_text(
                            "Required. Choose an asset for this action before applying the perk.",
                        );
                    }
                    missing
                },
                |ui| {
                    let width = ui.available_width().min(controls::COLUMN_WIDTH);
                    controls::sized(ui, width, |ui| {
                        ui.with_layout(
                            egui::Layout::left_to_right(egui::Align::Center)
                                .with_main_justify(true),
                            |ui| {
                                self.draw_asset_picker(ui, catalog, asset, scope);
                            },
                        );
                    })
                },
            );
        }
        // What the action is about stays in view, the way the native editors lead with
        // their named fields. Only the technical bytes wait behind Properties.
        match action {
            Action::Spawn { position, .. } => draw_spawn_position(ui, kill_trigger, position),
            Action::Native { node } => {
                native::draw(ui, "native-effect", node, NativeFamily::Effect);
            }
            Action::ExtendTimers { extend_ms, cap_ms } => {
                ui.horizontal_wrapped(|ui| {
                    seconds(ui, "Extend By", EXTEND_HINT, extend_ms, 1);
                    seconds(ui, "Up To", CAP_HINT, cap_ms, 1);
                });
                if *cap_ms < *extend_ms {
                    *cap_ms = *extend_ms;
                }
            }
            Action::Property { .. } => self.draw_property_action(ui, action),
            Action::AdjustComponent {
                target,
                flag,
                option,
                scale_bits,
                value_bits,
                ..
            } => {
                properties::field(ui, "Ability", "Which ability's energy is adjusted.", |ui| {
                    draw_component_target(ui, target);
                });
                native::byte_field(ui, 0x80803E4D, 3, flag);
                native::byte_field(ui, 0x80803E4D, 4, option);
                properties::field(
                    ui,
                    "Scale",
                    "Multiplies the value before it is applied. Stock energy perks use 1.",
                    |ui| {
                        let control = float_field(ui, scale_bits);
                        pickers::name_response(ui, &control, "Scale");
                    },
                );
                properties::field(
                    ui,
                    "Value",
                    "The constant the value program pushes.",
                    |ui| {
                        let control = float_field(ui, value_bits);
                        pickers::name_response(ui, &control, "Value");
                    },
                );
            }
            Action::UpdateAccumulator { value_bits, .. } => {
                properties::field(
                    ui,
                    "Value",
                    "The value written to the program's accumulator.",
                    |ui| {
                        let control = float_field(ui, value_bits);
                        pickers::name_response(ui, &control, "Value");
                    },
                );
            }
            Action::SetDamageType { mode, .. } => {
                properties::field(ui, "Element", "The weapon's damage type.", |ui| {
                    let reading = match mode {
                        0 => "Kinetic",
                        1 => "Solar",
                        2 => "Arc",
                        _ => "Void",
                    };
                    // Four fixed words, so a narrow column holds the whole vocabulary.
                    sized(ui, NARROW_COLUMN, |ui| {
                        egui::ComboBox::from_id_salt("damage-type")
                            .width(ui.available_width())
                            .truncate()
                            .selected_text(reading)
                            .show_ui(ui, |ui| {
                                crate::app::style::workbench_style(ui);
                                for (value, name) in
                                    [(0, "Kinetic"), (1, "Solar"), (2, "Arc"), (3, "Void")]
                                {
                                    ui.selectable_value(mode, value, name);
                                }
                            })
                            .response
                            .on_hover_text(format!("Element: {reading}"));
                        pickers::name_combo(ui, "damage-type", "Element");
                    });
                });
            }
            Action::TransmatContext { key } => {
                properties::field(ui, "Effect Key", "The transmat effect at +0x04.", |ui| {
                    let control = hex_key(ui, "transmat-key", key);
                    pickers::name_response(ui, &control, "Effect Key");
                });
            }
            Action::OverrideHostKey { key, .. } => {
                properties::field(
                    ui,
                    "Replacement Key",
                    "The key written in place of the host's own, at +0x04.",
                    |ui| {
                        let control = hex_key(ui, "override-host-key", key);
                        pickers::name_response(ui, &control, "Replacement Key");
                    },
                );
            }
            Action::WeaponReferenceCount { selector } => {
                properties::field(
                    ui,
                    "Operation",
                    "Byte +0x02. Its values are not resolved.",
                    |ui| {
                        let control = ui.add(egui::DragValue::new(selector).range(0..=255));
                        pickers::name_response(ui, &control, "Operation");
                    },
                );
            }
            Action::AbilityProperty {
                target,
                key,
                option,
            } => {
                properties::field(ui, "Ability", "Which ability's property changes.", |ui| {
                    draw_ability_slot(ui, target);
                });
                native::byte_field(ui, 0x80803E1D, 8, option);
                native::ability_property(ui, *target, key);
            }
            Action::AddRounds {
                rounds,
                target,
                store,
                ..
            } => {
                properties::field(
                    ui,
                    "Rounds",
                    "Whole rounds. Negative removes rounds.",
                    |ui| {
                        // An amount, a destination and a store do not fit one line beside
                        // the label column in a narrow pane, so this value wraps.
                        ui.horizontal_wrapped(|ui| {
                            let control = ui.add_sized(
                                [controls::CONTROL_WIDTH, ui.spacing().interact_size.y],
                                egui::DragValue::new(rounds).range(-999..=999),
                            );
                            pickers::name_response(ui, &control, "Rounds");
                            draw_ammunition_target(ui, target, store);
                        });
                    },
                );
            }
            Action::AddFraction {
                fraction_bits,
                target,
                store,
                ..
            } => {
                properties::field(
                    ui,
                    "Share",
                    "A percentage of the chosen capacity. 50% of the magazine capacity is half a magazine.",
                    |ui| {
                        // Same three controls as Rounds, so the value wraps the same way.
                        ui.horizontal_wrapped(|ui| {
                            let mut percent = f32::from_bits(*fraction_bits) * 100.0;
                            let control = ui.add_sized(
                                [controls::CONTROL_WIDTH, ui.spacing().interact_size.y],
                                egui::DragValue::new(&mut percent)
                                    .range(-10_000.0..=10_000.0)
                                    .max_decimals(2)
                                    .suffix("%"),
                            );
                            pickers::name_response(ui, &control, "Share");
                            if control.changed() && percent.is_finite() {
                                *fraction_bits = (percent / 100.0).to_bits();
                            }
                            draw_ammunition_target(ui, target, store);
                        });
                    },
                );
            }
            Action::Attach { .. } | Action::Pattern { .. } => {}
        }
        properties.show(ui, |ui| {
            if has_technical_fields(action) {
                ui.strong("Action");
            }
            match action {
                Action::Attach {
                    mode,
                    keys,
                    float_bits,
                    ..
                } => draw_attach_technical_fields(ui, mode, keys, float_bits),
                Action::AdjustComponent {
                    limit_bits, input, ..
                } => draw_component_technical_fields(ui, limit_bits, input),
                Action::SetDamageType {
                    keep_after_removal, ..
                } => {
                    properties::field(
                        ui,
                        "Keep After Removal",
                        "Byte +0x03. The element stays changed once the effect ends.",
                        |ui| {
                            let control = ui.checkbox(keep_after_removal, "");
                            pickers::name_response(ui, &control, "Keep After Removal");
                        },
                    );
                }
                Action::OverrideHostKey {
                    target,
                    interface,
                    apply_to_player,
                    ..
                } => {
                    for (label, value, hint) in [
                        ("Target Selector", target, "Byte +0x02."),
                        ("Interface Selector", interface, "Byte +0x03."),
                    ] {
                        properties::field(ui, label, hint, |ui| {
                            let control = ui.add(egui::DragValue::new(value).range(0..=255));
                            pickers::name_response(ui, &control, label);
                        });
                    }
                    properties::field(
                        ui,
                        "Apply to Player",
                        "Byte +0x08. The replacement applies to the player rather than the weapon.",
                        |ui| {
                            let control = ui.checkbox(apply_to_player, "");
                            pickers::name_response(ui, &control, "Apply to Player");
                        },
                    );
                }
                Action::UpdateAccumulator { mode, .. } => {
                    properties::field(
                        ui,
                        "Mode",
                        "Byte +0x02, which selects the supplied value. Every stock node stores 1.",
                        |ui| {
                            let control = ui.add(egui::DragValue::new(mode).range(0..=255));
                            pickers::name_response(ui, &control, "Mode");
                        },
                    );
                }
                Action::Property {
                    target,
                    operation_byte,
                    removal,
                    restore_bits,
                    ability_mask,
                    input,
                    flag,
                    ..
                } => draw_property_technical_fields(
                    ui,
                    PropertyBytes {
                        target,
                        operation: operation_byte,
                        removal,
                        restore_bits,
                        ability_mask,
                        input,
                        flag,
                    },
                ),
                Action::AddRounds {
                    overflow,
                    unit_scaled,
                    action_scaled,
                    ..
                } => {
                    draw_ammunition_technical_fields(
                        ui,
                        overflow,
                        Some(unit_scaled),
                        None,
                        action_scaled,
                    );
                }
                Action::AddFraction {
                    capacity,
                    overflow,
                    action_scaled,
                    ..
                } => {
                    draw_ammunition_technical_fields(
                        ui,
                        overflow,
                        None,
                        Some(capacity),
                        action_scaled,
                    );
                }
                Action::Spawn { .. }
                | Action::AbilityProperty { .. }
                | Action::Pattern { .. }
                | Action::ExtendTimers { .. }
                | Action::TransmatContext { .. }
                | Action::WeaponReferenceCount { .. }
                | Action::Native { .. } => {}
            }
            if let Some(asset) = action.asset() {
                ui.separator();
                ui.strong("Referenced Object");
                event.edit = super::properties::edit_object(ui, asset);
            }
        });
        if let Some(asset) = action.asset() {
            self.properties.draw(ui, asset);
        }
        event
    }

    /// The Named Property controls a reader needs in view: the key picker and the constant
    /// value. The bytes the installed nodes carry beside them wait under Properties.
    fn draw_property_action(&mut self, ui: &mut egui::Ui, action: &mut Action) {
        let Action::Property {
            key,
            target,
            operation_byte,
            removal,
            value_bits,
            ..
        } = action
        else {
            return;
        };
        // Picking a key adopts the bytes and value the installed actions agree on.
        if let Some(evidence) = self.draw_property_key(ui, key)
            && let Action::Property {
                target: t,
                operation_byte: o,
                removal: r,
                value_bits: v,
                ..
            } = Action::property_from(&evidence)
        {
            *target = t;
            *operation_byte = o;
            *removal = r;
            *value_bits = v;
        }
        properties::field(
            ui,
            "Value",
            "The constant the value program pushes.",
            |ui| {
                let value = float_field(ui, value_bits);
                pickers::name_response(ui, &value, "Value");
            },
        );
    }

    /// A searchable list of installed keys, each shown with the perks using it.
    /// Returns the evidence for a key the user just picked.
    fn draw_property_key(&mut self, ui: &mut egui::Ui, key: &mut u32) -> Option<KeyEvidence> {
        let picked = ui
            .horizontal_wrapped(|ui| {
                ui.label("Key").on_hover_text(
                    "The property key at +0x08. Pick an installed key or enter one.",
                );
                draw_key_picker(
                    ui,
                    "property-key",
                    key,
                    self.keys.catalog.property_keys(),
                    "installed nodes",
                    &mut self.property_query,
                )
                .cloned()
            })
            .inner;
        if let Some(evidence) = self.keys.catalog.property_key(*key) {
            ui.small(format!(
                "Observed values {}. Target {}, operation {}, removal {}.",
                numbers(&evidence.values),
                bytes(&evidence.targets),
                bytes(&evidence.operations),
                bytes(&evidence.removals)
            ));
        } else if self.discovery.keys.is_some() {
            ui.small("No installed use found for this key.");
        }
        self.draw_key_status(ui);
        picked
    }

    fn draw_key_status(&self, ui: &mut egui::Ui) {
        if let Some(error) = &self.discovery.key_error {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Could not read installed keys.",
            )
            .on_hover_text(error);
        } else if let Some(keys) = &self.discovery.keys {
            if !keys.issues.is_empty() {
                ui.weak(format!("{} actions could not be read.", keys.issues.len()))
                    .on_hover_text(keys.issues.join("\n"));
            }
        } else {
            ui.weak("Reading Installed Keys…");
        }
    }
}

/// Whether an action carries bytes that only the Properties panel edits: the attach
/// mode, keys and floats, the ammunition flags, or the unmapped Named Property bytes.
fn has_technical_fields(action: &Action) -> bool {
    matches!(
        action,
        Action::Attach { .. }
            | Action::AddRounds { .. }
            | Action::AddFraction { .. }
            | Action::Property { .. }
            | Action::AdjustComponent { .. }
            | Action::UpdateAccumulator { .. }
            | Action::SetDamageType { .. }
            | Action::OverrideHostKey { .. }
    )
}

/// The ability whose property changes, from the slots stock perks witness. Other selector
/// values stay a number, since no stock perk names them.
fn draw_ability_slot(ui: &mut egui::Ui, target: &mut u8) {
    use sundial::package_authoring::sandbox_perk::action::ability_slot;
    let current = ability_slot(*target).map_or_else(|| format!("Selector {target}"), str::to_owned);
    let hover = format!("Ability: {current}");
    // The ability names are a short vocabulary, and an unmapped selector keeps its own
    // spinner beside the control, so this one stays in a narrow column.
    sized(ui, NARROW_COLUMN, |ui| {
        egui::ComboBox::from_id_salt("ability-slot")
            .width(ui.available_width())
            .truncate()
            .selected_text(current)
            .show_ui(ui, |ui| {
                crate::app::style::workbench_style(ui);
                for selector in [0_u8, 1, 2, 3, 4, 7] {
                    if let Some(role) = ability_slot(selector) {
                        ui.selectable_value(target, selector, role);
                    }
                }
            })
            .response
            .on_hover_text(hover);
        pickers::name_combo(ui, "ability-slot", "Ability");
    });
    if ability_slot(*target).is_none() {
        ui.add(egui::DragValue::new(target).range(0..=255))
            .on_hover_text("Native selector. Other values have no identified ability role.");
    }
}

/// The ability an adjustment targets, from the roles eleven stock perks establish. Other
/// selector values are kept as a number, since no stock perk names them.
fn draw_component_target(ui: &mut egui::Ui, target: &mut u8) {
    use sundial::package_authoring::sandbox_perk::action::component_target;
    let current =
        component_target(*target, 0, 0).map_or_else(|| format!("Selector {target}"), str::to_owned);
    let hover = format!("Ability: {current}");
    // Four named energies and a numeric selector are a short vocabulary.
    sized(ui, NARROW_COLUMN, |ui| {
        egui::ComboBox::from_id_salt("component-target")
            .width(ui.available_width())
            .truncate()
            .selected_text(current)
            .show_ui(ui, |ui| {
                crate::app::style::workbench_style(ui);
                for selector in [0_u8, 1, 2, 7] {
                    if let Some(role) = component_target(selector, 0, 0) {
                        ui.selectable_value(target, selector, role);
                    }
                }
            })
            .response
            .on_hover_text(hover);
        pickers::name_combo(ui, "component-target", "Ability");
    });
}

/// Value-program controls shared with the complete native editor.
fn draw_component_technical_fields(ui: &mut egui::Ui, limit_bits: &mut u32, input: &mut u8) {
    egui::CollapsingHeader::new("Advanced")
        .id_salt("component-technical-fields")
        .show(ui, |ui| {
            native::byte_field(ui, 0x80803E4D, 0x48, input);
            properties::field(
                ui,
                "Limit",
                "Negative values disable the limit. Zero is a real limit. The adjustment moves toward the limit without overshooting it.",
                |ui| {
                    let control = float_field(ui, limit_bits);
                    pickers::name_response(ui, &control, "Limit");
                },
            );
        });
}

/// The position selector of a spawn action. The event position needs a kill trigger.
fn draw_spawn_position(ui: &mut egui::Ui, kill_trigger: bool, position: &mut Position) {
    properties::field(
        ui,
        "Spawn Location",
        "Where the object or effect is created.",
        |ui| {
            let reading = position_label(kill_trigger, *position);
            // The longest reading is a short sentence, so the control keeps the shared value
            // column instead of taking the width of that sentence.
            column(ui, |ui| {
                egui::ComboBox::from_id_salt("spawn-position")
                    .width(COLUMN_WIDTH)
                    .truncate()
                    .selected_text(reading)
                    .show_ui(ui, |ui| {
                        crate::app::style::workbench_style(ui);
                        ui.selectable_value(
                            position,
                            Position::Owner,
                            position_label(kill_trigger, Position::Owner),
                        );
                        ui.add_enabled_ui(kill_trigger, |ui| {
                            ui.selectable_value(
                                position,
                                Position::Event,
                                position_label(kill_trigger, Position::Event),
                            );
                        });
                    })
                    .response
                    .on_hover_text(format!("Spawn Location: {reading}"));
                pickers::name_combo(ui, "spawn-position", "Spawn Location");
            });
        },
    );
}

pub(super) fn position_label(kill_trigger: bool, position: Position) -> &'static str {
    match position {
        Position::Owner => "At Your Position",
        Position::Event if kill_trigger => "At the Defeated Enemy",
        Position::Event => "At the Triggering Event",
    }
}

/// Where an ammunition action puts its amount: the weapon slot or ammo type, and the
/// magazine or the reserves.
fn draw_ammunition_target(
    ui: &mut egui::Ui,
    target: &mut AmmunitionTarget,
    store: &mut AmmunitionStore,
) {
    let destination = "This weapon, a weapon slot or an ammo type. The client traces the slot and type positions without naming them.";
    let held = "Read from stock use: Triple Tap returns rounds to the magazine through this byte, and the ammo pickup perks add to reserves.";
    ui.label("To").on_hover_text(destination);
    let chosen = target.label();
    // Both vocabularies are a handful of short names, and they share a row with the amount,
    // so each keeps a narrow column rather than growing to its selected text.
    sized(ui, NARROW_COLUMN, |ui| {
        egui::ComboBox::from_id_salt("ammunition-target")
            .width(ui.available_width())
            .truncate()
            .selected_text(chosen)
            .show_ui(ui, |ui| {
                crate::app::style::workbench_style(ui);
                for choice in AmmunitionTarget::ALL {
                    ui.selectable_value(target, choice, choice.label());
                }
            })
            .response
            .on_hover_text(format!("{chosen}\n{destination}"));
        pickers::name_combo(ui, "ammunition-target", "Ammunition Target");
    });
    let stored = store.label();
    sized(ui, NARROW_COLUMN, |ui| {
        egui::ComboBox::from_id_salt("ammunition-store")
            .width(ui.available_width())
            .truncate()
            .selected_text(stored)
            .show_ui(ui, |ui| {
                crate::app::style::workbench_style(ui);
                for choice in AmmunitionStore::ALL {
                    ui.selectable_value(store, choice, choice.label());
                }
            })
            .response
            .on_hover_text(format!("{stored}\n{held}"));
        pickers::name_combo(ui, "ammunition-store", "Ammunition Store");
    });
}

/// The flags of an ammunition node, kept under a disclosure since stock perks rarely set them.
fn draw_ammunition_technical_fields(
    ui: &mut egui::Ui,
    overflow: &mut bool,
    unit_scaled: Option<&mut bool>,
    capacity: Option<&mut AmmunitionStore>,
    action_scaled: &mut bool,
) {
    let open = *overflow || *action_scaled || unit_scaled.as_deref().is_some_and(|set| *set);
    egui::CollapsingHeader::new("Advanced")
        .id_salt("ammunition-technical-fields")
        .default_open(open)
        .show(ui, |ui| {
            ui.checkbox(overflow, "Allow Magazine Overflow")
                .on_hover_text("Byte +0x69. Lets the magazine hold more than its capacity, as Ambitious Assassin does.");
            if let Some(unit_scaled) = unit_scaled {
                ui.checkbox(unit_scaled, "Scale by Ammunition Unit")
                    .on_hover_text("Byte +0x6A. The ammo pickup perks set it on their ammo type amounts.");
            }
            if let Some(capacity) = capacity {
                let basis = "Byte +0x6A. Which capacity the share scales. Stock nodes match it to the destination.";
                properties::field(ui, "Capacity", basis, |ui| {
                    let reading = format!("{} Capacity", capacity.label());
                    let hover = format!("{reading}\n{basis}");
                    // Two readings, both two words, so a narrow column carries them.
                    sized(ui, NARROW_COLUMN, |ui| {
                        egui::ComboBox::from_id_salt("ammunition-capacity")
                            .width(ui.available_width())
                            .truncate()
                            .selected_text(reading)
                            .show_ui(ui, |ui| {
                                crate::app::style::workbench_style(ui);
                                for choice in AmmunitionStore::ALL {
                                    ui.selectable_value(
                                        capacity,
                                        choice,
                                        format!("{} Capacity", choice.label()),
                                    );
                                }
                            })
                            .response
                            .on_hover_text(hover);
                        pickers::name_combo(ui, "ammunition-capacity", "Capacity Basis");
                    });
                });
            }
            ui.checkbox(action_scaled, "Scale by Action Value")
                .on_hover_text("Byte +0x6B. A few predicate-driven stock perks set it. Its input is not mapped.");
        });
}

/// The remaining bytes of a Named Property node, borrowed together for editing.
struct PropertyBytes<'a> {
    target: &'a mut u8,
    operation: &'a mut u8,
    removal: &'a mut u8,
    restore_bits: &'a mut u32,
    ability_mask: &'a mut u32,
    input: &'a mut u8,
    flag: &'a mut u8,
}

/// The remaining bytes of a Named Property node. Their roles are not mapped.
fn draw_property_technical_fields(ui: &mut egui::Ui, bytes: PropertyBytes<'_>) {
    egui::CollapsingHeader::new("Advanced")
        .id_salt("property-technical-fields")
        .show(ui, |ui| {
            for (label, value, hint) in [
                (
                    "Target Selector",
                    bytes.target,
                    "Byte +0x02. Stock nodes store 0 through 3.",
                ),
                (
                    "Operation",
                    bytes.operation,
                    "Byte +0x49. Stock nodes store 0, 1 and 3.",
                ),
                (
                    "Removal Policy",
                    bytes.removal,
                    "Byte +0x4A. Stock nodes store 0, 1 and 2.",
                ),
                (
                    "Input Selector",
                    bytes.input,
                    "Byte +0x48. Stock nodes store 0 in all but four cases.",
                ),
                (
                    "Flag Byte",
                    bytes.flag,
                    "Byte +0x03. Stock nodes store 1 in all but one case.",
                ),
            ] {
                properties::field(ui, label, hint, |ui| {
                    ui.add(egui::DragValue::new(value).range(0..=255));
                });
            }
            properties::field(
                ui,
                "Removal Value",
                "Float at +0x4C. Stock nodes store 0 or 1.",
                |ui| {
                    let restore = float_field(ui, bytes.restore_bits);
                    pickers::name_response(ui, &restore, "Removal Value");
                },
            );
            properties::field(
                ui,
                "Ability Slot Mask",
                "Mask at +0x04. Stock nodes leave it zero in 190 of 203 cases.",
                |ui| {
                    let mask = hex_key(ui, "ability-mask", bytes.ability_mask);
                    pickers::name_response(ui, &mask, "Ability Slot Mask");
                },
            );
        });
}
