//! Search words for behavior rows, and the text and choices their detail pane shows.
use super::*;

/// A row's detail, or nothing when it only repeats the title.
pub(super) fn distinct_detail<'a>(detail: &'a str, title: &str) -> &'a str {
    if detail == title { "" } else { detail }
}

pub(super) fn configuration<'a>(
    ui: &mut egui::Ui,
    group: &Group<'a>,
    catalog: Option<&native::Catalog>,
) -> &'a Row {
    if group.rows.len() == 1 {
        return group.rows[0];
    }
    let id = ui.make_persistent_id(("behavior-configuration", group.family));
    let mut selected = ui
        .data(|data| data.get_temp::<u64>(id))
        .filter(|key| group.rows.iter().any(|row| row.key() == *key))
        .unwrap_or_else(|| group.rows[0].key());
    let details = group
        .rows
        .iter()
        .map(|row| match (&row.choice, catalog) {
            (Choice::Condition(index), Some(catalog)) => {
                catalog.conditions[*index].condition.details.as_slice()
            }
            (Choice::Effect(index), Some(catalog)) => catalog.effects[*index].details.as_slice(),
            _ => &[],
        })
        .collect::<Vec<_>>();
    let labels = group
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            if matches!(row.choice, Choice::Trigger(_) | Choice::Action(_)) {
                return "Custom Configuration".to_owned();
            }
            let changes = details[index]
                .iter()
                .filter(|value| {
                    !details
                        .iter()
                        .filter(|other| !other.is_empty())
                        .all(|other| other.contains(value))
                })
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join(" · ");
            if changes.is_empty() && group.rows.iter().any(|other| other.detail != row.detail) {
                format!("{} · {}", index + 1, row.detail)
            } else if changes.is_empty() {
                format!("Configuration {}", index + 1)
            } else {
                format!("{} · {changes}", index + 1)
            }
        })
        .collect::<Vec<_>>();
    let current = group
        .rows
        .iter()
        .position(|row| row.key() == selected)
        .unwrap_or(0);
    // A configuration is named by the values that tell it apart, so the reading runs long.
    // A combo takes the width of its selected text and `width` only sets a floor, so the
    // allocation is what holds it to the row. The whole reading stays on hover.
    let width = ui.available_width().min(340.0);
    let chosen = labels[current].clone();
    controls::sized(ui, width, |ui| {
        egui::ComboBox::from_id_salt(id.with("selector"))
            .width(width)
            .truncate()
            .selected_text(&chosen)
            .show_ui(ui, |ui| {
                ui.set_max_width(440.0);
                for (row, label) in group.rows.iter().zip(&labels) {
                    ui.selectable_value(&mut selected, row.key(), label);
                }
            })
            .response
            .on_hover_text(format!(
                "{chosen}\n{} configurations available. Selecting one keeps its values and restrictions, which you can edit after adding it.", group.rows.len()
            ));
        pickers::name_combo(ui, id.with("selector"), "Configuration");
    });
    ui.data_mut(|data| data.insert_temp(id, selected));
    group
        .rows
        .iter()
        .find(|row| row.key() == selected)
        .copied()
        .unwrap_or(group.rows[0])
}

/// Real installed examples help explain a behavior without inventing gameplay claims. A
/// recipe's examples are the perks that carry one of its actions exactly.
pub(super) fn stock_examples(
    group: &Group<'_>,
    catalog: &native::Catalog,
    names: &BTreeMap<u16, String>,
) -> Vec<String> {
    group
        .rows
        .iter()
        .flat_map(|row| match &row.choice {
            Choice::Condition(index) => catalog.conditions[*index]
                .sources
                .iter()
                .collect::<Vec<_>>(),
            Choice::Effect(index) => catalog.effects[*index].sources.iter().collect(),
            Choice::Recipe(nodes) => catalog
                .effects
                .iter()
                .filter(|effect| {
                    nodes
                        .iter()
                        .any(|node| node.kind == effect.kind && node.bytes == effect.bytes)
                })
                .flat_map(|effect| &effect.sources)
                .collect(),
            _ => Vec::new(),
        })
        .filter_map(|source| names.get(&source.perk))
        .filter(|name| !unnamed_ability(name))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The stock perks behind a row, by name, so a search for a perk finds what it does.
pub(super) fn perk_names(sources: &[native::Source], names: &BTreeMap<u16, String>) -> String {
    sources
        .iter()
        .filter_map(|source| names.get(&source.perk))
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Everyday words for what a behavior does, searched beside its own text, so a search for the
/// goal finds it: "headshot" finds precision kills and "stacks" the counter.
pub(super) fn goal_words(family: &Family, title: &str) -> String {
    let words = match family {
        Family::Condition(ConditionFamily::Kill { .. }) => KILL_WORDS,
        Family::Condition(_) => family.kind().map_or("", condition_words),
        Family::Effect(kind, _) => action_words(*kind),
    };
    if title.to_lowercase().contains("precision") {
        format!("{words} headshot headshots crit critical")
    } else {
        words.to_owned()
    }
}

const KILL_WORDS: &str = "kill kills killing defeat defeated final blow";

fn condition_words(kind: u8) -> &'static str {
    match kind {
        0 => "passive always permanent constant",
        1 => "timer delay wait seconds",
        2 => KILL_WORDS,
        4 => "hit hits shoot shooting shot damage dealt",
        5 => "hurt damaged hit shield broken",
        6 => "ammo brick pickup scavenger",
        8..=10 => "grenade melee super class ability dodge barricade rift cast",
        14 | 16 => "passive equip equipped draw drawn ready swap switch",
        15 | 17 => "unequip holster stow swap switch",
        19 => "reload reloading reloaded",
        20 | 35 => "while state status",
        22 => "crouch crouching",
        23 => "ads aim aiming scope zoom sights",
        24 => "slide sliding",
        25 => "sprint sprinting run running",
        26 => "stack stacks stacking count counter times in a row after",
        27 => "shoot shooting fire firing shot trigger",
        29 | 30 => "signal",
        31 => "and both while requirement requirements",
        42 => "finisher execute",
        _ => "",
    }
}

fn action_words(kind: u8) -> &'static str {
    match kind {
        1 | 2 => "buff debuff status aura explode explosion blast",
        3 => "spawn explode explosion blast",
        4 => "debuff target",
        5 => "orb orbs power light masterwork",
        6 => "element elemental solar arc void kinetic",
        7 | 8 => "grenade melee super class ability energy cooldown refund recharge",
        10 => "stat stats buff boost range stability handling reload speed aim assist zoom",
        11 | 13 => "ammo drop finder brick special heavy primary",
        14 | 15 => "ammo rounds magazine mag refill return reserves",
        16 => "reload refill magazine mag instant",
        18 | 20 => "radar minimap",
        25 | 26 => "projectile rocket bullet pattern",
        28 | 29 | 35 => "full auto trigger charge firing mode",
        30 => "tracking homing lock on rocket",
        32 => "extend duration timer longer",
        33 => "damage resistance reduction resist defense",
        37 | 54 => "label champion stagger overload pierce unstoppable barrier",
        40 => "damage bonus buff more damage multiplier",
        41 | 42 => "stack stacks counter count",
        43 => "signal",
        49 => "mark marked remember",
        _ => "",
    }
}

/// A perk known only by its ability list and entry reads "Ability 1 / 19" (see the investment
/// ingredients), which names nothing a reader could look up, so it is no example.
fn unnamed_ability(name: &str) -> bool {
    name.strip_prefix("Ability ")
        .and_then(|rest| rest.split_once(" / "))
        .is_some_and(|(list, entry)| list.parse::<u32>().is_ok() && entry.parse::<u32>().is_ok())
}

pub(super) fn asset_names(text: &str, labels: &BTreeMap<u32, String>) -> String {
    let mut result = text.to_owned();
    for word in text.split_whitespace() {
        if let Some(tag) = word
            .strip_prefix("0x")
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            && let Some(name) = labels.get(&tag)
        {
            result = result.replace(word, name);
        }
    }
    result
}

pub(super) fn technical(
    ui: &mut egui::Ui,
    sources: &[native::Source],
    names: &BTreeMap<u16, String>,
    details: &[String],
) {
    egui::CollapsingHeader::new("Technical Details").show(ui, |ui| {
        for detail in details {
            ui.label(detail);
        }
        ui.separator();
        for source in sources {
            ui.label(format!(
                "{} · {}",
                names
                    .get(&source.perk)
                    .cloned()
                    .unwrap_or_else(|| format!("Effect {}", source.perk)),
                source.role
            ));
        }
    });
}
