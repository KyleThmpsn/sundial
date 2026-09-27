//! Editable backend for an authored program. The canvas calls one block at a time.
use super::assets::AssetScope;
use super::controls::{float_field, hex_key};
use super::*;
use sundial::package_authoring::sandbox_perk::{
    action::{
        FactValue,
        layout::{self, FieldFormat, Layout},
    },
    nodes,
    program::{
        Action, AmmunitionTarget, Asset, EMPTY_KEY, KeyCatalog, NativeNode, Position, Program,
        Trigger, properties::KeyIndex,
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

/// A new effect with a name, so it starts valid. Its card shows its place in the list, so the
/// name does not repeat it, and a moved effect never reads "3. Effect 2".
pub(super) fn named_effect(metadata_index: u16) -> WeaponSandboxPerkRuntimeRecipe {
    let mut effect = new_effect(metadata_index);
    if let Some(program) = &mut effect.program {
        program.name = "New Effect".into();
    }
    effect
}

/// What the user asked for on one action block this frame.
#[derive(Default)]
pub(super) struct ActionEvent {
    /// Remove this action from the program.
    pub remove: bool,
    /// Swap this action with the one at the given index.
    pub swap_with: Option<usize>,
}

pub(super) enum NativeRequest<'a> {
    Condition(&'a str),
    /// The trigger picker, with whether the group's actions hold state.
    Trigger(&'a str, bool),
    Action(&'a Program),
    Asset(&'a mut Asset, AssetScope),
}

pub(super) type NativePicker<'a> =
    dyn FnMut(&mut egui::Ui, NativeRequest<'_>) -> Option<super::behaviors::Selection> + 'a;

pub(super) fn pick_condition(
    pick: &mut NativePicker<'_>,
    ui: &mut egui::Ui,
    label: &str,
) -> Option<NativeNode> {
    match pick(ui, NativeRequest::Condition(label))? {
        super::behaviors::Selection::Condition(node) => Some(node),
        _ => None,
    }
}

/// The name a standard trigger carries in its picker, on its card and in its reading. Without
/// retained actions a weapon trigger is only its event, so it takes that condition's name.
pub(super) fn trigger_label(trigger: Trigger, retained: bool) -> &'static str {
    match (trigger, retained) {
        (Trigger::Drawn, false) => nodes::condition_title(16),
        (Trigger::Equipped, false) => nodes::condition_title(14),
        _ => trigger.label(),
    }
}

/// Whether the trigger reads as a held state rather than an event: its actions keep state
/// until the effect ends, or it has none yet.
fn retains(program: &Program) -> bool {
    program.actions.is_empty() || program.actions.iter().any(Action::retained)
}

pub(super) mod native;
pub(super) use native::draw_complete;
#[cfg(test)]
mod tests;

pub(super) fn read_native(ui: &mut egui::Ui, condition: bool, node: &NativeNode) {
    ui.add_enabled_ui(false, |ui| {
        let mut node = node.clone();
        if condition {
            native::negation(ui, &mut node);
        }
        native::draw(
            ui,
            "native-reader",
            &mut node,
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

/// The name a condition carries wherever it appears: its picker row, the card's button, the
/// complete editor and the readings. It changes with the node's configuration, so a kill
/// filter reads as the kill trigger it matches.
pub(super) fn condition_title(node: &NativeNode) -> String {
    sundial::package_authoring::sandbox_perk::action::decode_condition_node(&node.bytes)
        .map_or_else(
            |_| nodes::condition_title(node.kind).to_owned(),
            |condition| decoded_condition_title(&condition),
        )
}

/// `condition_title` for a condition already decoded in place.
pub(super) fn decoded_condition_title(
    condition: &sundial::package_authoring::sandbox_perk::action::DecodedCondition,
) -> String {
    sundial::investment::discovery::conditions::name(condition)
        .unwrap_or_else(|| nodes::condition_title(condition.kind).to_owned())
}

/// The locked reading of a native effect: its name and its mapped fields.
fn native_action_text(node: &NativeNode) -> String {
    let name = native_action_label(node.kind, &node.bytes);
    let facts = NativeFamily::Effect
        .layouts()
        .iter()
        .find(|layout| layout.kind == node.kind)
        .map(|layout| layout.facts(&node.bytes))
        .unwrap_or_default();
    if facts.is_empty() {
        name
    } else {
        format!(
            "{name} ({})",
            facts
                .iter()
                .map(sundial::package_authoring::sandbox_perk::action::Fact::render)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

const REMOVAL_KEY_NOTE: &str = "installed ending conditions";

/// The reactivation row carries one name in every reading. Repeat Interval and Cooldown are
/// the names of the timer tiles inside Timing, not of the row.
pub(super) fn rearm_label(_program: &Program) -> &'static str {
    "Reactivation"
}

/// Where a spawn places its object, as the card's readings say it.
fn position_phrase(position: Position) -> &'static str {
    match position {
        Position::Owner => "at your position",
        Position::Event => "at the triggering event",
    }
}

/// The locked reading of the trigger block.
pub(super) fn trigger_text(program: &Program) -> String {
    let name = trigger_label(program.trigger, retains(program));
    let primary = if let (Trigger::Native, Some(node)) = (program.trigger, &program.native_trigger)
    {
        condition_title(node)
    } else if program.trigger.is_event() && program.chance_permyriad != 10_000 {
        format!(
            "{name} ({}% chance)",
            f32::from(program.chance_permyriad) / 100.0
        )
    } else {
        name.to_owned()
    };
    std::iter::once(primary)
        .chain(program.alternative_triggers.iter().map(condition_title))
        .collect::<Vec<_>>()
        .join(" or ")
}

/// The locked reading of the removal block, or `None` when the program has no removal list.
pub(super) fn removal_text(program: &Program, keys: Option<&KeyCatalog>) -> Option<String> {
    let parts = primary_removal_text(program, keys)
        .into_iter()
        .chain(program.alternative_removals.iter().map(condition_title))
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join(" or "))
}

fn primary_removal_text(program: &Program, keys: Option<&KeyCatalog>) -> Option<String> {
    if let Some(node) = &program.native_removal {
        Some(condition_title(node))
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
            // The ending events the compiler adds for a weapon trigger, named as the picker
            // offers them.
            Trigger::Equipped => Some(nodes::condition_title(15).to_owned()),
            Trigger::Drawn => Some(nodes::condition_title(17).to_owned()),
            _ => Some(format!("After {} s", program.duration_ms as f32 / 1000.0)),
        }
    }
}

/// The locked reading of the rearm block, or `None` when the program has no rearm timer.
pub(super) fn rearm_text(program: &Program) -> Option<String> {
    let primary = if let Some(node) = &program.native_rearm {
        Some(condition_title(node))
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
        .chain(program.alternative_rearms.iter().map(condition_title))
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join(" or "))
}

/// What a folded card reads: the first behavior group's trigger, with its alternatives, the
/// action names in the order they run, and how many more behavior groups the program holds.
pub(super) fn brief(program: &Program) -> Option<(String, Vec<String>, usize)> {
    let Some(native) = &program.native else {
        return Some((
            trigger_text(program),
            program.actions.iter().map(action_title).collect(),
            0,
        ));
    };
    let decoded = native
        .graph
        .emit()
        .and_then(|bytes| sundial::package_authoring::sandbox_perk::action::decode(&bytes))
        .ok()?;
    let group = decoded.groups.first()?;
    // An empty trigger list reads as the Always condition it stands for, the name the card's
    // own trigger button carries.
    let trigger = if group.activation.is_empty() {
        nodes::condition_title(0).to_owned()
    } else {
        group
            .activation
            .iter()
            .map(brief_condition)
            .collect::<Vec<_>>()
            .join(" or ")
    };
    // Storage order is the reverse of the order the actions run and are listed in.
    let actions = group
        .effects
        .iter()
        .rev()
        .map(|effect| native_action_label(effect.kind, &effect.native))
        .collect();
    // The first behavior leads the reading, and the rest are counted, so a card with three
    // behaviors never reads as its first one alone.
    Some((trigger, actions, decoded.groups.len() - 1))
}

/// A trigger as a folded card reads it. A requirement set reads as the things it requires
/// and a counter as how many of what it counts, since their own titles name every such
/// trigger alike and tell no two cards apart.
fn brief_condition(
    condition: &sundial::package_authoring::sandbox_perk::action::DecodedCondition,
) -> String {
    match condition.kind {
        31 if !condition.subgroups.is_empty() => condition
            .subgroups
            .iter()
            .map(|subgroup| {
                let alternatives = subgroup
                    .conditions
                    .iter()
                    .map(brief_condition)
                    .collect::<Vec<_>>();
                if alternatives.len() > 1 {
                    format!("({})", alternatives.join(" or "))
                } else {
                    alternatives.join(" or ")
                }
            })
            .collect::<Vec<_>>()
            .join(" and "),
        26 if !condition.children.is_empty() => {
            let needed = condition
                .native
                .get(0x20..0x24)
                .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
                .map(f32::from_le_bytes)
                .filter(|needed| *needed >= 1.0);
            let counted = condition
                .children
                .iter()
                .map(brief_condition)
                .collect::<Vec<_>>()
                .join(" or ");
            match needed {
                Some(needed) => format!("{} × {counted}", needed as i64),
                None => counted,
            }
        }
        _ => decoded_condition_title(condition),
    }
}

/// The locked reading of one action block: its name, then what it is set to.
pub(super) fn action_text(action: &Action, keys: Option<&KeyCatalog>) -> String {
    use sundial::package_authoring::sandbox_perk::action::{ability_slot, component_target};
    let asset_name = |asset: &Asset| {
        if asset.graph == 0 {
            "no asset chosen".to_owned()
        } else if asset.path.is_empty() {
            format!("Asset 0x{:08X}", asset.graph)
        } else {
            sundial::package_authoring::tft::asset_label(&asset.path)
        }
    };
    let title = action_title(action);
    match action {
        Action::Spawn {
            asset, position, ..
        } => format!(
            "{title}: {} {}",
            asset_name(asset),
            position_phrase(*position)
        ),
        Action::Attach { asset, .. } | Action::Pattern { asset } => {
            format!("{title}: {}", asset_name(asset))
        }
        Action::ExtendTimers { extend_ms, cap_ms } => format!(
            "{title}: {} s more, up to {} s",
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
                "{title}: 0x{key:08X} = {}{seen}",
                f32::from_bits(*value_bits)
            )
        }
        Action::AdjustComponent {
            target,
            scale_bits,
            value_bits,
            ..
        } => {
            let energy = component_target(*target, 0, 0)
                .map_or_else(|| format!("selector {target}"), str::to_owned);
            format!(
                "{title}: {energy}, scale {} by {}",
                f32::from_bits(*scale_bits),
                f32::from_bits(*value_bits)
            )
        }
        Action::UpdateAccumulator { value_bits, .. } => {
            format!("{title}: {}", f32::from_bits(*value_bits))
        }
        Action::AbilityProperty {
            target,
            key,
            option,
        } => {
            let ability =
                ability_slot(*target).map_or_else(|| format!("selector {target}"), str::to_owned);
            let operation = if *option == 0 { "apply" } else { "remove" };
            let property =
                sundial::package_authoring::sandbox_perk::action::native::fields::keys::name(*key)
                    .map_or_else(|| format!("0x{key:08X}"), str::to_owned);
            format!("{title}: {ability}, {operation} {property}")
        }
        Action::TransmatContext { key } | Action::OverrideHostKey { key, .. } => {
            let named =
                sundial::package_authoring::sandbox_perk::action::native::fields::keys::name(*key)
                    .map_or_else(|| format!("0x{key:08X}"), str::to_owned);
            format!("{title}: {named}")
        }
        Action::SetDamageType { mode, .. } => match mode {
            0 => format!("{title}: Kinetic"),
            1 => format!("{title}: Solar"),
            2 => format!("{title}: Arc"),
            3 => format!("{title}: Void"),
            _ => title,
        },
        Action::WeaponReferenceCount { selector } => {
            format!("{title}: operation {selector}")
        }
        Action::AddRounds {
            rounds,
            target,
            store,
            overflow,
            ..
        } => format!(
            "{title}: {rounds} {} to the {} of {}{}",
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
            "{title}: {}% of the {} capacity to the {} of {}{}",
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
        Action::Native { node } => native_action_text(node),
    }
}

// The plain names live beside the node catalog, so the picker, the cards, the readings and
// the build's own messages all read from one table. Anything unnamed there keeps the engine's
// traced name and is held behind the picker's Advanced setting.
pub(super) use sundial::package_authoring::sandbox_perk::nodes::{
    plain_condition_title, plain_effect_title as plain_action_title,
};

/// What a player would be told this effect kind does. Present for exactly the kinds
/// `plain_action_title` names, which the tests enforce.
pub(super) fn plain_action_summary(kind: u8) -> Option<&'static str> {
    Some(match kind {
        1 => "Keep an object or effect attached while this effect lasts.",
        2 => "Keep an effect attached and drive one of its values from a formula.",
        3 => "Spawn a pickup, relic, world object, projectile or effect at the player or event.",
        4 => "Apply a referenced effect to a target the action selects.",
        5 => "Create a collectible Orb of Light, the way Masterwork weapons do.",
        6 => "Change the weapon's damage type, as The Fundamentals does.",
        7 => "Change a named stat inside an ability.",
        8 => "Scale grenade, melee, Super or class ability energy, with an optional limit.",
        10 => "Adjust a named weapon or ability property.",
        // Kind 11: the Primary, Special and Heavy Ammo Finder mods each write one of the
        // three triples, so the triples are per ammo type, and the Finder mods set the
        // second value of theirs. All four described stock perks read "increases the drop
        // chance of ... ammo on kill".
        11 => "Add to the ammo drop chances by ammo type, as the Ammo Finder mods do.",
        // Kind 13: the three weights are the ammo types. Snapload Finisher weights only the
        // first and generates Primary ammo, Special Finisher only the second, Heavy Finisher
        // only the third, and every other described stock perk agrees.
        13 => {
            "Pick Primary, Special or Heavy ammo by weight and drop it, as Special Finisher does."
        }
        14 => "Add whole rounds to the magazine or reserves, as Triple Tap returns a round.",
        15 => "Add a share of the magazine or reserve capacity, as kill-to-reload perks do.",
        16 => "Move ammo from reserves into the magazine, adding none.",
        // Kind 18: Long March and Radar Booster, the only stock uses, both write the third
        // host float (80 and 56) and leave the other two at -1, which the callback leaves
        // unchanged. Those other two settings are not identified.
        18 => "Replace the radar detection range while the effect is active, as Long March does.",
        // Kind 20: the radar component counter feeds the HUD radar detail mode, which the
        // client clamps to 0 through 2. Upgraded Sensor Pack turns it on while crouching.
        // It is separate from the radar range and from radar visibility while aiming.
        20 => {
            "Add a step of radar detail while active, as Upgraded Sensor Pack does while crouching."
        }
        // Kind 25: what reads the key is not established.
        25 => "Replace a named key on the weapon's firing pattern, and clear it afterwards.",
        26 => "Use a selected projectile pattern while the effect is active.",
        // Kind 28: the ten full auto perks set Hold to Fire, Charge Shot and Ahamkara's Eye
        // Hold to Charge.
        28 => {
            "Make holding the trigger fire or charge, as Full Auto Trigger System and Charge Shot do."
        }
        // Kind 29: what the three values drive is not established.
        29 => "Replace three of the weapon's own values, and restore them afterwards.",
        // Kind 30: other weapon state changes as the count crosses zero.
        30 => "Hold one weapon count while the effect is active.",
        // Kind 33: every stock use scales the damage its wearer takes. Chaotic Exchanger
        // ("resist incoming damage") and Resistant Tether sit below 1, Riven's Curse
        // ("also take more damage from all sources") above it, so below 1 takes less and
        // above 1 takes more. The distance at +0C is negative in every stock node, which
        // turns the distance limit off.
        33 => "Scale the damage the wearer takes: down as Chaotic Exchanger, up as Riven's Curse.",
        32 => "Another matching kill adds time to the effect's timers, as Outlaw does.",
        // Kind 35: every stock use is a firing mode. Full Auto Trigger System, Rapid-Fire
        // Frame and Thunderer write the same key, and Fan Fire clears it.
        35 => {
            "Set the weapon's firing mode while the effect is active, as Full Auto Trigger System does."
        }
        // Kind 37: the stock filters name weapon families and abilities, and the champion
        // mods use it to add labels such as stagger and overload.
        37 => "Label the triggering event by damage source, so other perks can read it.",
        // Kind 40: its rows and bonus scale the damage of the hits its filters pass. The stock
        // uses raise damage dealt, as Anti-Taken Fletching does against Taken, and damage taken
        // belongs to kind 33.
        40 => "Change the damage dealt by hits that pass its filters, as a bonus or a multiplier.",
        // Kind 41: the Sword Guard perks (Burst, Enduring, Heavy and Swordmaster's Guard)
        // all raise the same named count, the key the guard conditions read back by name.
        41 => "Raise a named count while active and lower it after, as the Sword Guard perks do.",
        42 => "Write the effect's counter, the value Count Needed is measured against.",
        // Kind 43: the Ace of Spades and Trinity Ghoul catalysts publish one, and Blessing
        // of the Sky another. What receives a given signal is not established.
        43 => "Publish a named signal that On a Game Signal can listen for.",
        // Kind 47: all 112 stock perks that carry it belong to Transmat Effect items, one key
        // each, which is what the key identifies. The client code that reads the key has
        // not been traced.
        47 => "Set which transmat effect this perk carries.",
        // Kind 48: the referenced resource is a behavior script whose path names it, such
        // as apply_tiered_charge_of_light, the one 17 Charged with Light mods run. Some
        // scripts need specific game state or an owning object.
        48 => "Run a game behavior script, such as Charged with Light. Test it in game.",
        // Kind 49: Vengeance ("Highlights the enemy who dares to damage you") stores the
        // enemy under a name, and its When a Remembered Target Is Far Away condition reads
        // the same name back. The player keeps at most four names.
        49 => "Store a target under a name conditions can read back, as Vengeance does.",
        // Kind 52: the player table holds at most four names, and what reads them is not
        // established.
        52 => "Add to a named player value while the effect is active, and subtract it afterwards.",
        53 => "Add to, replace or multiply the named values that match.",
        // Kind 54: stock perks use it for the champion effects: stagger, pierce and overload.
        54 => "Label the triggering event by target, for stagger, pierce and overload.",
        _ => return None,
    })
}

/// What a player would be told this condition kind does. Present for exactly the kinds
/// `plain_condition_title` names.
pub(super) fn plain_condition_summary(kind: u8) -> Option<&'static str> {
    Some(match kind {
        0 => "Always passes. The effect still obeys its chance and its trigger.",
        1 => "Waits a fixed number of seconds, as a duration or a cooldown.",
        2 => "Passes on a kill. It can require this weapon and a label such as a precision hit.",
        // Kind 4: every described stock perk on it reads as damage dealt, from Impact
        // Induction ("causing damage with a melee attack") and The Perfect Fifth ("precision
        // hits") to Disruption Break ("breaking an enemy's shield with this weapon"). Its
        // label filter names what dealt the damage: precision, grenade, sword and so on.
        4 => "Passes when damage is dealt, as Impact Induction reads a melee hit.",
        // Kind 6: every described stock perk on it is a Scavenger or Lead from Gold, and all
        // read "when you pick up ammo". The ammunition type mask is named from the same
        // perks, and picks Primary, Special or Heavy.
        6 => "Passes when ammo is picked up, as every Scavenger perk does.",
        // Kind 8: every described stock perk on it reads as an ability cast, from Bomber
        // ("when using your class ability") to Radiant Light ("casting your Super"). The
        // ability mask is named from the same perks and picks the grenade, the Super or the
        // class ability.
        8 => "Passes when an ability is used, as Bomber reads the class ability.",
        // Kind 9: the event carries the ability slot as a bit, numbered as kind 8's mask.
        // Resolute and Volatile Conduction read Super casts on bit 1, Aeon Energy a dodge
        // on bit 7.
        9 => "Passes when the selected ability activates, as Resolute reads a Super cast.",
        // Kind 10 compares the event's resource with its own. Fusion Harness and Bring the
        // Heat name the Fusion Grenade's, New Tricks the Skip Grenade's and Scissor Fingers
        // the knives'.
        10 => "Passes on one ability's events, as Bring the Heat names the Fusion Grenade.",
        // Kind 11: the same key match placed as an ending. Bring the Heat starts on the Fusion
        // Grenade through kind 10 and ends on it through kind 11, with the same key.
        11 => {
            "Ends the effect on one ability's events, as Bring the Heat ends on the Fusion Grenade."
        }
        // Kind 13: every stock use ends a hold-the-trigger perk.
        13 => "Passes when the trigger is released, as Spinning Up and Dynamic Sway Reduction end.",
        // Kind 5: Dreaded Visage ("when you're damaged"), Arc Conductor ("taking Arc
        // damage"), Vengeance ("those that harm you") and the Taken, Fallen and Hive Barrier
        // mods ("receiving Taken damage") all read as damage taken. Its labels name what
        // dealt the damage.
        5 => "Passes when the player takes damage, as Dreaded Visage reads.",
        // Kind 12: the event and context keys are named from the perks that listen to them,
        // 19 of them on the Orb of Light pickup alone, which is where the offered events
        // come from.
        12 => "Passes when a game event fires, such as picking up an Orb of Light.",
        // Kind 19: all 18 stock perks with the reload flag set read as reloading, from Kill
        // Clip and Impetus starting to Under Pressure and High-Impact Reserves ending. On
        // Reload is the setting all 18 use; the second weapon event is one no stock
        // description names.
        19 => "Passes when this weapon is reloaded, as Kill Clip starts and Under Pressure ends.",
        // Kind 22: its event setting picks the start or the end of crouching.
        22 => "Passes when crouching starts or ends, as Field Prep uses it.",
        // Kind 23: Rangefinder starts on aiming and Hip-Fire Grip on leaving it, which is
        // what the event setting picks between.
        23 => "Passes when aiming down sights starts or stops, as Rangefinder does.",
        // Kinds 24 and 25 each have one stock user, whose text names the movement.
        // Kind 18: a swap to a weapon, filtered by its slot and labels.
        18 => {
            "Passes when a weapon is swapped to, as Mecha Holster reads a readied Submachine Gun and Sprint Grip ends."
        }
        24 => "Passes when sliding starts, as Reflective Vents uses it.",
        25 => "Passes when sprinting starts or stops, as Striking Light uses it.",
        14 => "Passes when this weapon is equipped to the character.",
        15 => "Passes when this weapon is no longer equipped.",
        16 => "Passes when this weapon is drawn.",
        17 => "Passes when this weapon is put away.",
        // Kind 26: each Contributing Condition adds to, replaces or multiplies the counter
        // when it passes, and a new one adds 1. Set the Effect's Counter writes it directly.
        // A stacking perk is one whose Count Needed is more than 1.
        26 => "Passes when the effect's counter reaches Count Needed, its threshold.",
        // Kind 27: every described stock perk reads as a shot fired, from Tap the Trigger
        // and Under Pressure starting on one to Box Breathing and The Perfect Fifth ending.
        // Its mode can restrict it to a missed shot, as Mulligan and Reversal of Fortune do.
        27 => "Passes when a shot is fired, as Tap the Trigger starts and Box Breathing resets.",
        // Kind 29: the signals on offer are the ones stock perks start on, such as standing
        // near a Vex Relay.
        29 => "Passes when a game signal fires, such as collecting a Warmind Cell.",
        // Kind 30: an always-active effect ends on its own signal, and Relay Defender and
        // Resistant Tether end on the signals they started on.
        30 => "Ends the effect when a game signal fires, as Relay Defender does.",
        // Kind 38 reads back what Remember a Target by Name stored: Vengeance marks the
        // enemy that damaged it and then checks that the mark is still far away.
        38 => "Passes while a remembered target is farther away than the set distance.",
        // Kind 42: Bulwark Finisher reads the final blow and Reactive Pulse the finisher
        // starting and ending, which is what its event setting picks between.
        42 => "Passes on a finisher, as Bulwark Finisher reads the final blow.",
        // Kind 31 is structural and fully traced: every subgroup must pass, and the
        // conditions inside one subgroup are alternatives. Each subgroup reads as one
        // requirement. Backup Plan and Archer's Gambit use it to require two things at once,
        // a hip fire state and a precision hit.
        31 => {
            "Every requirement below must be met, each by any one condition under it, as Archer's Gambit needs both a hip fire state and a precision hit."
        }
        // Kind 35 runs the general predicate's state check and then its nested condition.
        35 => "Checks a state, such as aiming down sights, and then the condition under it.",
        _ => return None,
    })
}

/// Whether a decoded reading is the engine's own fallback rather than a real description.
///
/// `describe_effect` ends by handing back the node's catalogue summary, or its name when it
/// has none. When that is what came back, the reading says nothing the plain table cannot say
/// better, and the plain table is what a player reads.
fn fell_back(summary: Option<&'static str>, name: String, decoded: &str) -> bool {
    summary.is_some_and(|text| text == decoded) || name == decoded
}

/// An effect's reading for a card, in plain words where the engine had none of its own.
pub(super) fn native_action_reading(kind: u8, decoded: &str) -> String {
    let catalogued = nodes::effect(kind).map(|node| node.summary);
    if fell_back(catalogued, nodes::effect_name(kind), decoded)
        && let Some(plain) = plain_action_summary(kind)
    {
        return plain.to_owned();
    }
    decoded.to_owned()
}

/// The name a native effect carries. An ability energy change names the ability it targets,
/// in the verb its kind's name uses.
pub(super) fn native_action_label(kind: u8, bytes: &[u8]) -> String {
    if kind == 8
        && let Some(values) = bytes.get(2..5)
        && let Some(role) = sundial::package_authoring::sandbox_perk::action::component_target(
            values[0], values[1], values[2],
        )
    {
        return format!("Change {role}");
    }
    nodes::effect_title(kind).to_owned()
}

/// What a placed action calls itself on its card, in its reading and in the picker, so the
/// name that places an action and the name it then carries are one string.
pub(super) fn action_title(action: &Action) -> String {
    match action {
        Action::Native { node } => native_action_label(node.kind, &node.bytes),
        // Every typed action is one of the engine's effect kinds and carries that kind's name.
        other => other.label().to_owned(),
    }
}

/// Effect kinds offered as guided actions in their own right, beyond the ones the workbench
/// composes as typed `Action` variants. Each has a plain title and sentence, and the tests
/// assert that every one of them actually produces an editable node.
pub(super) const PROMOTED_NATIVE_ACTIONS: [u8; 16] =
    [2, 4, 11, 13, 16, 18, 20, 33, 37, 40, 41, 43, 48, 49, 52, 53];

/// Condition kinds offered in the picker without Show All, like the promoted actions. A
/// stock accumulator configuration never carries a recognized name, and its bare kind was
/// hidden behind the same switch, so the effect's counter could not be found at all.
///
/// Requiring two things at once is the same case. Kind 31 is how a perk asks for both a state
/// and an event, as Archer's Gambit and Backup Plan do, and its stock configurations are
/// structural rather than named, so nothing offered it until Show All was on. An author
/// looking for an "and" had no reason to think it lived behind a switch labelled as unnamed
/// configurations and empty kinds.
///
/// Sliding, sprinting and one ability's events (kinds 24, 25 and 10) are written up like
/// crouching and aiming, and their stock configurations carry no recognized name either, so
/// they are promoted the same way. Trigger release (13), weapon swap (18) and the ending form
/// of one ability's events (11) were identified from the stock perks that end or start on them
/// and joined for the same reason.
pub(super) const PROMOTED_NATIVE_CONDITIONS: [u8; 19] = [
    4, 5, 9, 10, 11, 13, 18, 19, 22, 23, 24, 25, 26, 27, 29, 30, 31, 38, 42,
];

/// One guided condition per engine variable the stock perks compare: a general predicate
/// composed from the stock template with that variable's own comparison. The variable
/// names come from the client's compiled source strings, so each row is a comparison the
/// game itself makes, and the editor lets the value, the operation and the variable change.
/// Each is named as its card will name it, comparison included.
pub(super) fn compiled_comparisons() -> Vec<(String, String, NativeNode)> {
    use sundial::package_authoring::sandbox_perk::action::native::predicate;
    predicate::VARIABLES
        .iter()
        .filter_map(|variable| {
            let bytes =
                predicate::compose(variable.name, variable.operation, variable.threshold).ok()?;
            let node = NativeNode { kind: 20, bytes };
            Some((
                condition_title(&node),
                format!(
                    "Passes while {} {} {}. {} The value, the comparison and the tracked variable can be changed.",
                    variable.plain, variable.operation, variable.threshold, variable.evidence
                ),
                node,
            ))
        })
        .collect()
}

/// The guided actions, in the order the picker suggests them when the stock perks give no
/// count to go by. Each is offered under the name its card carries.
pub(super) fn common_actions(
    program: &Program,
    keys: &KeyCatalog,
) -> Vec<(&'static str, &'static str, Action)> {
    let mut actions = vec![
        (
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
            plain_action_summary(5).unwrap_or_default(),
            Action::generate_orb(if program.has_kill_trigger() {
                Position::Event
            } else {
                Position::Owner
            }),
        ),
        (
            "Keep an object or effect attached while this effect lasts.",
            Action::attach(Asset::default()),
        ),
        (
            "Use a selected projectile pattern while the effect is active.",
            Action::Pattern {
                asset: Asset::default(),
            },
        ),
        (
            "Another matching kill while the effect is active adds time to its running timers, as Outlaw does. Needs a kill trigger.",
            Action::ExtendTimers {
                extend_ms: program.duration_ms.max(1_000),
                cap_ms: program.duration_ms.max(1_000),
            },
        ),
        (
            "Add whole rounds to the magazine or reserves, as Triple Tap returns a round.",
            Action::add_rounds(1),
        ),
        (
            "Add a share of the magazine or reserve capacity, as kill-to-reload perks refill half a magazine.",
            Action::add_fraction(0.5),
        ),
        (
            "Adjust a named weapon or ability property.",
            keys.property_keys()
                .first()
                .map_or_else(|| Action::property(EMPTY_KEY), Action::property_from),
        ),
        (
            plain_action_summary(8).unwrap_or_default(),
            Action::adjust_component(0),
        ),
        (
            "Write the effect's counter, the value that When the Effect's Counter Is Reached counts toward Count Needed.",
            Action::update_accumulator(1.0),
        ),
        (
            "Change a named property of one ability, as And Another Thing grants an extra grenade charge and Jump Jets improves the jump.",
            Action::ability_property(0),
        ),
        (
            "Change the weapon's damage type to Kinetic, Solar, Arc or Void, as The Fundamentals does.",
            Action::set_damage_type(1),
        ),
        (
            "Play a chosen transmat effect. The effect is named by a key.",
            Action::transmat_context(EMPTY_KEY),
        ),
        (
            "Replace a key on the weapon or the player while the effect is active, as the firing mode perks do.",
            Action::override_host_key(EMPTY_KEY),
        ),
        (
            "Hold a weapon reference count while the effect is active.",
            Action::weapon_reference_count(0),
        ),
    ];
    // A native kind is offered as a guided action once its traced behavior supports a plain
    // sentence and its fields already carry names. Each of these keeps its complete native
    // record; the guided entry supplies the sentence and a starting configuration.
    for kind in PROMOTED_NATIVE_ACTIONS {
        if let (Some(node), Some(summary)) = (NativeNode::effect(kind), plain_action_summary(kind))
        {
            actions.push((summary, Action::Native { node }));
        }
    }
    actions
        .into_iter()
        .map(|(summary, action)| (action.label(), summary, action))
        .collect()
}
