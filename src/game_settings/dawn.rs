//! Extensions belonging to a positively identified Dawn runtime, never inferred from JSON.
mod page;
pub(crate) use page::draw;

use crate::hash::parse_unsigned_value;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const OMEGA: &str = "/experiments/omega";
const CLIENT: &str = "/client";
const OMEGA_FLAGS: [(&str, &str); 9] = [
    ("coo_executor", "Omega Lua Executor"),
    ("directive_ui", "Directive UI"),
    (
        "ikora_carrier_model_suppression",
        "Ikora Carrier Model Suppression",
    ),
    ("ikora_vfx_rebind", "Ikora VFX Rebind"),
    ("scene_authority", "Scene Authority"),
    ("gate_authority", "Gate Authority"),
    ("portal_mutation", "Portal Mutation"),
    ("synthetic_stage_machine", "Synthetic Stage Machine"),
    ("unsafe_diagnostics", "Unsafe Diagnostics"),
];
const CLIENT_FLAGS: [(&str, &str); 2] = [
    ("roster_force_authored", "Force Authored Roster"),
    ("seed_authored_sensors", "Seed Authored Sensors"),
];

/// Limits a Dawn runtime compiles in, mirrored from its account and inventory state headers.
const CHARACTER_CAPACITY: usize = 3;
const CHARACTER_ITEM_CAPACITY: usize = 135;
const PLUG_CAPACITY: usize = 12;
/// The engine no-definition hash cannot identify an authored item or plug.
const NO_DEFINITION_HASH: u32 = 0x811C_9DC5;
/// The 16 named equipment slots, in the order Dawn's EquipmentSlot enum declares them.
const EQUIPMENT_SLOTS: [&str; 16] = [
    "kinetic",
    "energy",
    "heavy",
    "helmet",
    "gauntlets",
    "chest",
    "legs",
    "class_item",
    "ghost",
    "vehicle",
    "ship",
    "subclass",
    "clan_banner",
    "emblem",
    "emote",
    "finisher",
];
/// Character fields Dawn range checks when it reads them back, with each inclusive maximum.
const CHARACTER_RANGES: [(&str, u8); 9] = [
    ("race", 2),
    ("gender", 1),
    ("class", 2),
    ("level", 255),
    ("movement_ability", 255),
    ("grenade_ability", 255),
    ("super_ability", 255),
    ("melee_ability", 255),
    ("class_ability", 255),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Runtime {
    pub dll_path: PathBuf,
    pub script_path: PathBuf,
    pub script_problem: Option<String>,
}

impl Runtime {
    // Call only after positive DLL detection. File checks are cached between refreshes.
    pub(crate) fn inspect(dll_path: &Path) -> Self {
        let script_path = dll_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("Sunrise/scripts/omega.lua");
        let script_problem = match fs::read(&script_path) {
            Ok(bytes) if bytes.iter().any(|b| !b.is_ascii_whitespace()) => None,
            Ok(_) => Some("The Omega Lua script is empty.".into()),
            Err(error) => Some(format!(
                "The Omega Lua script is missing or unreadable: {error}"
            )),
        };
        Self {
            dll_path: dll_path.to_owned(),
            script_path,
            script_problem,
        }
    }

    pub(crate) fn validate(&self, json: &Value) -> Result<(), String> {
        let mut issues = settings_issues(json);
        if executor_enabled(json)
            && let Some(problem) = &self.script_problem
        {
            issues.push(format!(
                "{problem} Restore {} or turn off Omega Lua Executor before saving.",
                self.script_path.display()
            ));
        }
        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues.join(" "))
        }
    }
}

pub(crate) fn executor_enabled(json: &Value) -> bool {
    json.pointer("/experiments/omega/coo_executor")
        .and_then(Value::as_bool)
        == Some(true)
}

pub(crate) fn settings_issues(json: &Value) -> Vec<String> {
    let mut issues = Vec::new();
    if super::schema_version(json) != Some(6) {
        issues.push("This detected Dawn runtime expects settings schema v6.".into());
    }
    if json
        .pointer("/state/activity/default_destination/previous_activity_index")
        .and_then(Value::as_i64)
        .is_none()
    {
        issues.push(
            "Dawn settings are missing state.activity.default_destination.previous_activity_index."
                .into(),
        );
    }
    if !json.pointer("/state/account").is_some_and(Value::is_object)
        || json
            .pointer("/state/characters")
            .and_then(Value::as_array)
            .is_none_or(std::vec::Vec::is_empty)
    {
        issues.push("Dawn needs account and character data in settings.json.".into());
    }
    for group in ["/experiments", OMEGA, CLIENT] {
        if json.pointer(group).is_some_and(|v| !v.is_object()) {
            issues.push(format!("{} must be an object.", dotted(group)));
        }
    }
    for (group, fields) in [
        (OMEGA, OMEGA_FLAGS.as_slice()),
        (CLIENT, CLIENT_FLAGS.as_slice()),
    ] {
        for (key, _) in fields {
            let path = format!("{group}/{key}");
            if json.pointer(&path).is_some_and(|v| !v.is_boolean()) {
                issues.push(format!("{} must be true or false.", dotted(&path)));
            }
        }
    }
    issues.append(&mut account_issues(json));
    issues
}

/// Reports account data a Dawn runtime imports but then refuses to resolve.
///
/// Dawn imports settings.json once and never reads it again, so a shape it accepts at import but
/// rejects while preparing a loadout leaves the client waiting through investment sign-in until it
/// times out. These checks mirror the limits Dawn compiles in.
pub(crate) fn account_issues(json: &Value) -> Vec<String> {
    let mut issues = Vec::new();
    let Some(characters) = json.pointer("/state/characters").and_then(Value::as_array) else {
        return issues;
    };
    if characters.len() > CHARACTER_CAPACITY {
        issues.push(format!(
            "Dawn supports {CHARACTER_CAPACITY} characters and this account has {}.",
            characters.len()
        ));
    }
    let mut instances: BTreeMap<u64, usize> = BTreeMap::new();
    for (index, character) in characters.iter().enumerate() {
        let label = format!("Character {}", index + 1);
        for (field, limit) in CHARACTER_RANGES {
            if let Some(value) = character.get(field).and_then(Value::as_i64)
                && (value < 0 || value > i64::from(limit))
            {
                issues.push(format!(
                    "{label} has {field} {value}, outside 0 to {limit}."
                ));
            }
        }
        if let Some(inventory) = character.get("inventory").and_then(Value::as_array) {
            if inventory.len() > CHARACTER_ITEM_CAPACITY {
                issues.push(format!(
                    "{label} holds {} unequipped items and Dawn stores {CHARACTER_ITEM_CAPACITY}.",
                    inventory.len()
                ));
            }
            for item in inventory {
                item_issues(
                    item,
                    &label,
                    "an inventory item",
                    &mut instances,
                    &mut issues,
                );
            }
        }
        let Some(equipment) = character.get("equipment").and_then(Value::as_object) else {
            continue;
        };
        for (slot, item) in equipment {
            if !EQUIPMENT_SLOTS.contains(&slot.as_str()) {
                issues.push(format!("{label} has unknown equipment slot {slot}."));
            }
            item_issues(item, &label, slot, &mut instances, &mut issues);
        }
    }
    for (soid, count) in instances {
        if count > 1 {
            issues.push(format!(
                "Instance {soid} appears {count} times. Dawn requires one owner per instance."
            ));
        }
    }
    issues
}

fn item_issues(
    item: &Value,
    label: &str,
    slot: &str,
    instances: &mut BTreeMap<u64, usize>,
    issues: &mut Vec<String>,
) {
    if !item.is_object() {
        return;
    }
    // Hashes reach this validator the way settings.json stores them, which for everything
    // Sundial writes is a `0x` string rather than a number. Reading them as numbers alone made
    // both checks below silently pass everything, and made every authored plug fail.
    if let Some(soid) = item.get("instance_soid").and_then(parse_unsigned_value) {
        *instances.entry(soid).or_default() += 1;
    }
    if item.get("definition_hash").and_then(parse_unsigned_value)
        == Some(u64::from(NO_DEFINITION_HASH))
    {
        issues.push(format!("{label} {slot} has no usable definition hash."));
    }
    match item.get("plugs") {
        None | Some(Value::Null) => {}
        // An empty lane list is how Dawn's own defaults express a socket-less item such as a
        // subclass or an emblem, so it is not a problem on its own.
        Some(Value::Array(plugs)) if plugs.is_empty() => {}
        Some(Value::Array(plugs)) => {
            if plugs.len() > PLUG_CAPACITY {
                issues.push(format!(
                    "{label} {slot} lists {} socket lanes and Dawn accepts {PLUG_CAPACITY}.",
                    plugs.len()
                ));
            }
            for plug in plugs {
                if !plug.is_null()
                    && parse_unsigned_value(plug).is_none_or(|hash| {
                        hash > u64::from(u32::MAX) || hash == u64::from(NO_DEFINITION_HASH)
                    })
                {
                    issues.push(format!("{label} {slot} has an unusable plug hash."));
                }
            }
        }
        Some(_) => issues.push(format!("{label} {slot} plugs must be null or a list.")),
    }
}

fn dotted(path: &str) -> String {
    path.trim_start_matches('/').replace('/', ".")
}

fn editable_group(json: &Value, group: &str) -> bool {
    let mut path = String::new();
    for part in group.trim_start_matches('/').split('/') {
        path.push('/');
        path.push_str(part);
        if json.pointer(&path).is_some_and(|v| !v.is_object()) {
            return false;
        }
    }
    json.is_object()
}

fn set_flag(json: &mut Value, group: &str, key: &str, enabled: bool) -> bool {
    if !editable_group(json, group)
        || json
            .pointer(&format!("{group}/{key}"))
            .and_then(Value::as_bool)
            == Some(enabled)
    {
        return false;
    }
    let mut target = json;
    for part in group.trim_start_matches('/').split('/') {
        target = target
            .as_object_mut()
            .expect("checked object")
            .entry(part)
            .or_insert_with(|| serde_json::json!({}));
    }
    target
        .as_object_mut()
        .expect("checked group")
        .insert(key.into(), Value::Bool(enabled));
    true
}

#[cfg(test)]
mod tests;
