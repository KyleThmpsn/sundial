//! Extensions belonging to a positively identified Dawn runtime, never inferred from JSON.
mod page;
pub(crate) use page::draw;

use serde_json::Value;
use std::{
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
            .is_none_or(|a| a.is_empty())
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
    issues
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
