//! Overlay active account values for editing without rewriting inactive JSON containers.
use serde_json::{Value, json};

use super::{AccountDocument, WorkspaceDocument};

impl WorkspaceDocument {
    pub(in crate::app) fn runtime_view(&self) -> Value {
        let mut view = self.json.clone();
        if let AccountDocument::Sqlite(document) = &self.account {
            for parent in ["state", "server"] {
                if !view.get(parent).is_some_and(Value::is_object) {
                    view[parent] = json!({});
                }
            }
            view["server"]["entitlements"] = document.entitlements().clone();
            view["state"]["account"] = document.runtime()["account"].clone();
            view["state"]["characters"] = document.runtime()["characters"].clone();
        }
        view
    }

    pub(in crate::app) fn apply_runtime_view(&mut self, mut view: Value) -> Result<(), String> {
        if let AccountDocument::Sqlite(document) = &mut self.account {
            let account = view
                .pointer("/state/account")
                .ok_or("The runtime draft is missing active account data")?;
            let characters = view
                .pointer("/state/characters")
                .ok_or("The runtime draft is missing active character data")?;
            let native = json!({"account": account, "characters": characters});
            let entitlements = view
                .pointer("/server/entitlements")
                .cloned()
                .ok_or("The runtime draft is missing active ownership data")?;
            restore_members(&mut view, &self.json, "state", &["account", "characters"]);
            restore_members(&mut view, &self.json, "server", &["entitlements"]);
            document.set_runtime(native);
            document.set_entitlements(entitlements);
        }
        self.json = view;
        Ok(())
    }
}

fn restore_members(view: &mut Value, source: &Value, parent: &str, members: &[&str]) {
    let Some(object) = view.get_mut(parent).and_then(Value::as_object_mut) else {
        return;
    };
    for &key in members {
        match source.get(parent).and_then(|value| value.get(key)) {
            Some(value) => {
                object.insert(key.into(), value.clone());
            }
            None => {
                object.remove(key);
            }
        }
    }
    // Missing, null and opaque parent values are distinct source data. An overlay alone
    // must not normalize any of them to an empty object during an unrelated edit.
    if object.is_empty() && !source.get(parent).is_some_and(Value::is_object) {
        match source.get(parent) {
            Some(value) => view[parent] = value.clone(),
            None => {
                view.as_object_mut().unwrap().remove(parent);
            }
        }
    }
}
