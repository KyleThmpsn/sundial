//! Editable backend for an authored program. The canvas calls one block at a time.
use super::assets::{AssetScope, draw_attach_technical_fields};
use super::controls::{bytes, float_field, hex_key, numbers, seconds};
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

/// The trigger block: the trigger itself and, for kill triggers, the activation chance. A
/// native trigger adds the condition kind and its fields.
pub(super) fn draw_trigger_block(ui: &mut egui::Ui, program: &mut Program) {
    let mut properties = super::properties::Panel::new(ui, "trigger");
    let mut selected = program.trigger;
    let retained = program.actions.is_empty() || program.actions.iter().any(Action::retained);
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("program-trigger")
            .selected_text(trigger_label(program.trigger, retained))
            .show_ui(ui, |ui| {
                crate::app::style::workbench_style(ui);
                for trigger in Trigger::ALL {
                    ui.selectable_value(&mut selected, trigger, trigger_label(trigger, retained))
                        .on_hover_text(trigger.description());
                }
            })
            .response
            .on_hover_text(program.trigger.description());
        if selected != program.trigger {
            change_trigger(program, selected);
        }
        if program.trigger == Trigger::Native {
            let node = program
                .native_trigger
                .get_or_insert_with(|| NativeNode::condition(6).expect("a plain condition kind"));
            draw_native_kind(ui, "native-trigger-kind", node, NativeFamily::Condition);
            properties.button(ui);
        } else {
            program.native_trigger = None;
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
        properties.show(ui, |ui| {
            ui.strong("Condition");
            native::draw(ui, "native-trigger", node, NativeFamily::Condition);
        });
    }
}

fn trigger_label(trigger: Trigger, retained: bool) -> &'static str {
    match (trigger, retained) {
        (Trigger::Drawn, false) => "On Draw",
        (Trigger::Equipped, false) => "On Equip",
        (Trigger::Always, false) => "On Perk Activation",
        _ => trigger.label(),
    }
}

mod native;
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
    if trigger.is_event() {
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

    fn blank(self, kind: u8) -> Option<NativeNode> {
        match self {
            Self::Condition => NativeNode::condition(kind),
            Self::Effect => NativeNode::effect(kind),
        }
    }

    fn name(self, kind: u8) -> String {
        match self {
            Self::Condition => nodes::condition_name(kind),
            Self::Effect => nodes::effect_name(kind),
        }
    }
}

/// The kind picker of a native node. Changing the kind starts a fresh node of that kind.
fn draw_native_kind(ui: &mut egui::Ui, id: &str, node: &mut NativeNode, family: NativeFamily) {
    let mut kind = node.kind;
    egui::ComboBox::from_id_salt(id)
        .width(240.0)
        .selected_text(format!("{kind:02}: {}", family.name(kind)))
        .show_ui(ui, |ui| {
            crate::app::style::workbench_style(ui);
            let entries: &[nodes::NodeKind] = match family {
                NativeFamily::Condition => &nodes::CONDITIONS,
                NativeFamily::Effect => &nodes::EFFECTS,
            };
            for entry in entries {
                ui.add_enabled_ui(entry.support == nodes::Support::Authorable, |ui| {
                    ui.selectable_value(
                        &mut kind,
                        entry.kind,
                        format!("{:02}: {}", entry.kind, entry.name),
                    )
                    .on_hover_text(entry.summary)
                    .on_disabled_hover_text(entry.support.detail());
                });
            }
        })
        .response
        .on_hover_text(family.catalog(node.kind).map_or("", |entry| entry.summary));
    if kind != node.kind
        && let Some(fresh) = family.blank(kind)
    {
        *node = fresh;
    }
}

/// One field control by format. The stored value only changes on valid input.
fn draw_native_field(ui: &mut egui::Ui, field: &layout::Field, bytes: &mut [u8]) {
    let Some(value) = field.read(bytes) else {
        ui.weak("unreadable");
        return;
    };
    let width = [controls::CONTROL_WIDTH, ui.spacing().interact_size.y];
    let changed = match (field.format, value) {
        (FieldFormat::Byte, FactValue::Selector(mut byte)) => ui
            .add_sized(width, egui::DragValue::new(&mut byte).range(0..=255))
            .changed()
            .then_some(FactValue::Selector(byte)),
        (FieldFormat::Flag, FactValue::Flag(mut flag)) => ui
            .checkbox(&mut flag, "")
            .changed()
            .then_some(FactValue::Flag(flag)),
        (FieldFormat::Mask8, FactValue::Mask(mask)) => {
            let mut byte = mask as u8;
            ui.add_sized(
                width,
                egui::DragValue::new(&mut byte)
                    .range(0..=255)
                    .hexadecimal(2, false, true),
            )
            .changed()
            .then(|| FactValue::Mask(byte.into()))
        }
        (FieldFormat::Mask32, FactValue::Mask(mask)) => {
            let mut word = mask as u32;
            hex_key(ui, field.offset, &mut word);
            (u64::from(word) != mask).then(|| FactValue::Mask(word.into()))
        }
        (FieldFormat::Key, FactValue::Key(mut key)) => {
            let before = key;
            hex_key(ui, field.offset, &mut key);
            (key != before).then_some(FactValue::Key(key))
        }
        (FieldFormat::Float, FactValue::Number(number)) => {
            let mut bits = number.to_bits();
            float_field(ui, &mut bits);
            (bits != number.to_bits()).then(|| FactValue::Number(f32::from_bits(bits)))
        }
        (FieldFormat::Seconds, FactValue::Seconds(value)) => {
            let mut seconds = value;
            ui.add(
                egui::DragValue::new(&mut seconds)
                    .range(0.0..=3600.0)
                    .suffix(" s"),
            )
            .changed()
            .then_some(FactValue::Seconds(seconds))
        }
        (FieldFormat::Range, FactValue::Range(low, high)) => {
            let (mut low_bits, mut high_bits) = (low.to_bits(), high.to_bits());
            ui.horizontal(|ui| {
                float_field(ui, &mut low_bits);
                ui.label("to");
                float_field(ui, &mut high_bits);
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
    native_text(node, NativeFamily::Condition)
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
            if program.trigger == Trigger::Native {
                draw_native_ending(ui, program);
            }
        } else if program.trigger == Trigger::Always {
            if program.actions.iter().any(Action::retained) {
                ui.label(if program.removal_key.is_some() {
                    "Ends on a technical event key."
                } else {
                    "Retained effects stay until the perk leaves the weapon."
                });
            }
            egui::CollapsingHeader::new("Technical Ending Condition")
                .default_open(program.removal_key.is_some())
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let mut ends_on_key = program.removal_key.is_some();
                        egui::ComboBox::from_id_salt("always-removal")
                            .selected_text(if ends_on_key {
                                "On an Event Key"
                            } else {
                                "When the Perk Is Removed"
                            })
                            .show_ui(ui, |ui| {
                                crate::app::style::workbench_style(ui);
                                ui.selectable_value(
                                    &mut ends_on_key,
                                    false,
                                    "When the Perk Is Removed",
                                );
                                ui.selectable_value(&mut ends_on_key, true, "On an Event Key");
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
                    if program.removal_key.is_none() {
                        draw_native_ending(ui, program);
                    }
                });
        } else if program.actions.iter().any(Action::retained)
            && let Some(text) = removal_text(program, Some(&self.keys.catalog))
        {
            canvas::row(ui, "End Condition", DURATION_HINT, |ui| {
                ui.label(text);
            });
        }
    }
}

/// An optional native ending condition, for always-active and native-triggered programs.
fn draw_native_ending(ui: &mut egui::Ui, program: &mut Program) {
    let mut enabled = program.native_removal.is_some();
    ui.checkbox(&mut enabled, "Native Ending Condition")
        .on_hover_text(
            "End the effect when a native condition node passes, carried as the client stores it.",
        );
    if enabled && program.native_removal.is_none() {
        program.removal_key = None;
        program.native_removal = NativeNode::condition(29);
    } else if !enabled {
        program.native_removal = None;
    }
    if let Some(node) = &mut program.native_removal {
        let mut properties = super::properties::Panel::new(ui, "ending-condition");
        ui.horizontal_wrapped(|ui| {
            draw_native_kind(ui, "native-removal-kind", node, NativeFamily::Condition);
            properties.button(ui);
        });
        properties.show(ui, |ui| {
            ui.strong("Condition");
            native::draw(ui, "native-removal", node, NativeFamily::Condition);
        });
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
    hex_key(ui, (id, "raw"), key);
    let current = table.iter().find(|entry| entry.hash() == *key);
    let label = match current {
        Some(evidence) => format!("0x{key:08X} · {}", evidence.seen_in_as(what)),
        None => format!("0x{key:08X}"),
    };
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
                    &entry.seen_in_as(what),
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
    if program.trigger == Trigger::Always {
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
}

/// The locked reading of the trigger block.
pub(super) fn trigger_text(program: &Program) -> String {
    if let (Trigger::Native, Some(node)) = (program.trigger, &program.native_trigger) {
        return native_condition_text(node);
    }
    if program.trigger.is_event() && program.chance_permyriad != 10_000 {
        format!(
            "{} ({}% chance)",
            program.trigger.label(),
            f32::from(program.chance_permyriad) / 100.0
        )
    } else {
        program.trigger.label().to_owned()
    }
}

/// The locked reading of the removal block, or `None` when the program has no removal list.
pub(super) fn removal_text(program: &Program, keys: Option<&KeyCatalog>) -> Option<String> {
    if let Some(node) = &program.native_removal {
        return Some(format!("When {}", native_condition_text(node)));
    }
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
}

/// The locked reading of the rearm block, or `None` when the program has no rearm timer.
pub(super) fn rearm_text(program: &Program) -> Option<String> {
    if !program.trigger.supports_cooldown() || program.cooldown_ms == 0 {
        return None;
    }
    let seconds = program.cooldown_ms as f32 / 1000.0;
    Some(if program.trigger == Trigger::Always {
        format!("Every {seconds} s")
    } else {
        format!("After {seconds} s")
    })
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
            let place = match node.bytes[2] {
                0 => "at your position",
                1 => "at the triggering event",
                _ => "using its native position setting",
            };
            format!(
                "Generate {count} {} {place}",
                if count == 1 {
                    "Orb of Light"
                } else {
                    "Orbs of Light"
                }
            )
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
        Action::AddRounds { .. } => {
            "Adds whole rounds when the effect starts, the way Triple Tap returns a round."
        }
        Action::AddFraction { .. } => {
            "Adds a share of a capacity when the effect starts, the way kill-to-reload perks refill half a magazine."
        }
    }
}

/// Perk validation is shown once beside Apply to Weapon.
pub(super) fn draw_actions_footer(ui: &mut egui::Ui, program: &mut Program, keys: &KeyCatalog) {
    ui.horizontal(|ui| draw_add_action(ui, program, keys));
}

fn common_actions(
    program: &Program,
    keys: &KeyCatalog,
) -> [(&'static str, &'static str, Action); 8] {
    [
        (
            "Spawn an Object or Effect",
            "Spawn a pickup, relic, world object, projectile or effect at the player or event location.",
            Action::Spawn {
                asset: Asset::default(),
                position: if program.trigger.is_event() {
                    Position::Event
                } else {
                    Position::Owner
                },
            },
        ),
        (
            "Generate Orb of Light",
            "Use the native masterwork operation to create a collectible orb at the kill location or owner.",
            Action::generate_orb(if program.trigger.is_event() {
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
            "Add Rounds",
            "Add whole rounds to the magazine or reserves, as Triple Tap returns a round.",
            Action::add_rounds(1),
        ),
        (
            "Add Ammunition Fraction",
            "Add a share of the magazine or reserve capacity, as kill-to-reload perks refill half a magazine.",
            Action::add_fraction(0.5),
        ),
        (
            "Named Property",
            "Adjust a named weapon or ability property.",
            keys.property_keys()
                .first()
                .map_or_else(|| Action::property(EMPTY_KEY), Action::property_from),
        ),
    ]
}

fn draw_add_action(ui: &mut egui::Ui, program: &mut Program, keys: &KeyCatalog) {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let width = nodes::EFFECTS
        .iter()
        .map(|entry| {
            ui.painter()
                .layout_no_wrap(
                    format!("{:02}: {}", entry.kind, entry.name),
                    font.clone(),
                    ui.visuals().text_color(),
                )
                .size()
                .x
        })
        .fold(280.0, f32::max)
        + ui.spacing().indent
        + 32.0;
    ui.add_enabled_ui(program.actions.len() < 16, |ui| {
        if let Some(action) = pickers::popup_with_width(
            ui,
            "add-action",
            "Add Action\u{2026}",
            &mut String::new(),
            width,
            |ui, query, reset, height| {
                let mut selected = None;
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt("action-choices")
                    .max_height(height);
                if reset {
                    scroll = scroll.vertical_scroll_offset(0.0);
                }
                scroll.show(ui, |ui| {
                    for (label, description, action) in common_actions(program, keys) {
                        if !pickers::matches(query, &format!("{label} {description}")) {
                            continue;
                        }
                        let enabled = match action {
                            Action::Pattern { .. } => !program
                                .actions
                                .iter()
                                .any(|current| matches!(current, Action::Pattern { .. })),
                            Action::ExtendTimers { .. } => program.trigger.is_event(),
                            _ => true,
                        };
                        if ui
                            .add_enabled(enabled, egui::Button::new(label).frame(false))
                            .on_hover_text(description)
                            .on_disabled_hover_text(description)
                            .clicked()
                        {
                            selected = Some(action);
                        }
                    }
                    egui::CollapsingHeader::new("Technical Actions")
                        .id_salt("technical-actions")
                        .open((!query.is_empty()).then_some(true))
                        .show(ui, |ui| {
                            for entry in &nodes::EFFECTS {
                                let label = format!("{:02}: {}", entry.kind, entry.name);
                                if !pickers::matches(query, &format!("{label} {}", entry.summary)) {
                                    continue;
                                }
                                let action = Action::native(entry.kind);
                                if ui
                                    .add_enabled(
                                        action.is_some(),
                                        egui::Button::new(label).frame(false),
                                    )
                                    .on_hover_text(entry.summary)
                                    .on_disabled_hover_text(entry.support.detail())
                                    .clicked()
                                {
                                    selected = action;
                                }
                            }
                        });
                });
                selected
            },
        ) {
            program.actions.push(action);
        }
    });
}

impl Workbench {
    /// One editable action block. The caller applies the returned event after the loop.
    pub(super) fn draw_action_block(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        trigger: Trigger,
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
                properties.button(ui);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                    egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                    |ui| {
                        let title = match action {
                            Action::Spawn { .. } => "Spawn an Object or Effect",
                            Action::Attach { .. } => "Attach an Effect",
                            Action::Pattern { .. } => "Change Fired Projectile",
                            _ => action.label(),
                        };
                        ui.strong(format!("{}. {}", index + 1, title))
                            .on_hover_text(action_description(action));
                        if let Some(asset) = action.asset_mut() {
                            self.draw_asset_picker(ui, catalog, asset, scope);
                        }
                    },
                );
            });
        });
        properties.show(ui, |ui| {
            if !matches!(action, Action::Pattern { .. }) {
                ui.strong("Action");
            }
            match action {
                Action::Spawn { position, .. } => draw_spawn_position(ui, trigger, position),
                Action::Attach {
                    mode,
                    keys,
                    float_bits,
                    ..
                } => draw_attach_technical_fields(ui, mode, keys, float_bits),
                Action::Pattern { .. } => {}
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
                Action::AddRounds {
                    rounds,
                    target,
                    store,
                    overflow,
                    unit_scaled,
                    action_scaled,
                } => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Rounds")
                            .on_hover_text("Whole rounds. Negative removes rounds.");
                        ui.add_sized(
                            [controls::CONTROL_WIDTH, ui.spacing().interact_size.y],
                            egui::DragValue::new(rounds).range(-999..=999),
                        );
                        draw_ammunition_target(ui, target, store);
                    });
                    draw_ammunition_technical_fields(
                        ui,
                        overflow,
                        Some(unit_scaled),
                        None,
                        action_scaled,
                    );
                }
                Action::AddFraction {
                    fraction_bits,
                    target,
                    store,
                    capacity,
                    overflow,
                    action_scaled,
                } => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Share")
                            .on_hover_text("A percentage of the chosen capacity. 50% of the magazine capacity is half a magazine.");
                        let mut percent = f32::from_bits(*fraction_bits) * 100.0;
                        if ui
                            .add_sized(
                                [controls::CONTROL_WIDTH, ui.spacing().interact_size.y],
                                egui::DragValue::new(&mut percent)
                                    .range(-10_000.0..=10_000.0)
                                    .max_decimals(2)
                                    .suffix("%"),
                            )
                            .changed()
                            && percent.is_finite()
                        {
                            *fraction_bits = (percent / 100.0).to_bits();
                        }
                        draw_ammunition_target(ui, target, store);
                    });
                    draw_ammunition_technical_fields(ui, overflow, None, Some(capacity), action_scaled);
                }
                Action::Native { node } => {
                    native::draw(ui, "native-effect", node, NativeFamily::Effect);
                }
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

    /// The Named Property controls: the key picker, the constant value and the bytes the
    /// installed nodes carry beside them.
    fn draw_property_action(&mut self, ui: &mut egui::Ui, action: &mut Action) {
        let Action::Property {
            key,
            target,
            operation_byte,
            removal,
            value_bits,
            restore_bits,
            ability_mask,
            input,
            flag,
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
        ui.horizontal_wrapped(|ui| {
            ui.label("Value")
                .on_hover_text("The constant the value program pushes.");
            float_field(ui, value_bits);
        });
        draw_property_technical_fields(
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

/// The position selector of a spawn action. The event position needs a kill trigger.
fn draw_spawn_position(ui: &mut egui::Ui, trigger: Trigger, position: &mut Position) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Spawn Location");
        egui::ComboBox::from_id_salt("spawn-position")
            .selected_text(position_label(trigger, *position))
            .show_ui(ui, |ui| {
                crate::app::style::workbench_style(ui);
                ui.selectable_value(
                    position,
                    Position::Owner,
                    position_label(trigger, Position::Owner),
                );
                ui.add_enabled_ui(trigger.is_event(), |ui| {
                    ui.selectable_value(
                        position,
                        Position::Event,
                        position_label(trigger, Position::Event),
                    );
                });
            });
    });
}

pub(super) fn position_label(trigger: Trigger, position: Position) -> &'static str {
    match position {
        Position::Owner => "At Your Position",
        Position::Event if trigger.is_event() => "At the Defeated Enemy",
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
    ui.label("To").on_hover_text(
        "This weapon, a weapon slot or an ammo type. The client traces the slot and type positions without naming them.",
    );
    egui::ComboBox::from_id_salt("ammunition-target")
        .selected_text(target.label())
        .show_ui(ui, |ui| {
            crate::app::style::workbench_style(ui);
            for choice in AmmunitionTarget::ALL {
                ui.selectable_value(target, choice, choice.label());
            }
        });
    egui::ComboBox::from_id_salt("ammunition-store")
        .selected_text(store.label())
        .show_ui(ui, |ui| {
            crate::app::style::workbench_style(ui);
            for choice in AmmunitionStore::ALL {
                ui.selectable_value(store, choice, choice.label());
            }
        })
        .response
        .on_hover_text(
            "Read from stock use: Triple Tap returns rounds to the magazine through this byte, and the ammo pickup perks add to reserves.",
        );
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
    egui::CollapsingHeader::new(egui::RichText::new("Technical Fields").small())
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
                ui.horizontal(|ui| {
                    ui.label("Capacity")
                        .on_hover_text("Byte +0x6A. Which capacity the share scales. Stock nodes match it to the destination.");
                    egui::ComboBox::from_id_salt("ammunition-capacity")
                        .selected_text(format!("{} Capacity", capacity.label()))
                        .show_ui(ui, |ui| {
                            crate::app::style::workbench_style(ui);
                            for choice in AmmunitionStore::ALL {
                                ui.selectable_value(
                                    capacity,
                                    choice,
                                    format!("{} Capacity", choice.label()),
                                );
                            }
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
    egui::CollapsingHeader::new(egui::RichText::new("Technical Fields").small())
        .id_salt("property-technical-fields")
        .show(ui, |ui| {
            ui.weak("Native bytes with unmapped roles.");
            egui::Grid::new("property-technical-grid")
                .num_columns(2)
                .spacing([12.0, 4.0])
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
                        ui.label(label).on_hover_text(hint);
                        ui.add(egui::DragValue::new(value).range(0..=255));
                        ui.end_row();
                    }
                    ui.label("Removal Value")
                        .on_hover_text("Float at +0x4C. Stock nodes store 0 or 1.");
                    float_field(ui, bytes.restore_bits);
                    ui.end_row();
                    ui.label("Ability Slot Mask").on_hover_text(
                        "Mask at +0x04. Stock nodes leave it zero in 190 of 203 cases.",
                    );
                    hex_key(ui, "ability-mask", bytes.ability_mask);
                    ui.end_row();
                });
        });
}
