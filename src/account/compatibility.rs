//! Check the reviewed account against the complete incoming native definition table.
use super::AuthoredAccountCleanup;
use crate::persistence::json_account::{equipment, inventory};
use crate::persistence::native_account::snapshot::{Cell, Snapshot, Table};
use std::collections::BTreeSet;

impl AuthoredAccountCleanup {
    /// The item and plug definitions the proposed state references that the incoming table
    /// lacks, including stock rows and references already missing before this install, such as
    /// items left from an older build. A replacement removes them the way it removes the items a
    /// build leaves out. It does not read a second, potentially newer account.
    pub fn missing_incoming_definitions(
        &self,
        definitions: &BTreeSet<u32>,
    ) -> Result<BTreeSet<u32>, String> {
        Ok(self
            .missing_references(definitions)?
            .into_iter()
            .map(|(hash, _)| hash)
            .collect())
    }

    /// Refuses a proposed state that still references a definition the incoming table lacks.
    pub fn validate_incoming_definitions(&self, definitions: &BTreeSet<u32>) -> Result<(), String> {
        let references = self.missing_references(definitions)?;
        if references.is_empty() {
            return Ok(());
        }
        Err(format!(
            "The active account references item definitions unavailable in this build:\n{}\nRestore the packages that define these items or remove the affected items and socket selections before installing.",
            references
                .iter()
                .map(|(hash, location)| format!("{location}: 0x{hash:08X}"))
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }

    fn missing_references(
        &self,
        definitions: &BTreeSet<u32>,
    ) -> Result<Vec<(u32, String)>, String> {
        if definitions.is_empty() {
            return Err(
                "Cannot check account compatibility with an empty incoming item table".into(),
            );
        }
        let mut missing = Missing {
            definitions,
            references: Vec::new(),
        };
        if super::is_database(&self.settings_path) || super::is_dawn_database(&self.settings_path) {
            let snapshot: Snapshot =
                serde_json::from_slice(&self.cleaned_bytes).map_err(|e| e.to_string())?;
            missing.sqlite(&snapshot)?;
        } else {
            let document = crate::strict_json::from_reader(self.cleaned_bytes.as_slice())
                .map_err(|e| e.to_string())?;
            missing.json(&document)?;
        }
        Ok(missing.references)
    }
}

struct Missing<'a> {
    definitions: &'a BTreeSet<u32>,
    references: Vec<(u32, String)>,
}

impl Missing<'_> {
    fn check(&mut self, hash: u32, location: &str) {
        if !self.definitions.contains(&hash) {
            self.references.push((hash, location.to_owned()));
        }
    }

    fn json(&mut self, document: &serde_json::Value) -> Result<(), String> {
        let characters = document
            .pointer("/state/characters")
            .and_then(|v| v.as_array());
        for character in 0..characters.map_or(0, Vec::len) {
            let owner = format!("Character {}", character + 1);
            for row in inventory::character_inventory(document, character)
                .map_err(|e| e.to_string())?
                .unwrap_or_default()
            {
                let location = format!(
                    "{owner} inventory[{}] instance 0x{:016X}",
                    row.location.item_index, row.instance_soid
                );
                self.check(row.definition_hash, &location);
                if let inventory::ItemPlugs::Authored(plugs) = row.plugs {
                    for (lane, hash) in plugs.into_iter().enumerate() {
                        if let Some(hash) = hash {
                            self.check(hash, &format!("{location} socket[{lane}]"));
                        }
                    }
                }
            }
            for row in equipment::equipped_item_snapshots(document, character)? {
                let location = format!("{owner} equipment/{}", row.slot);
                let hash = row
                    .definition_hash
                    .and_then(|hash| u32::try_from(hash).ok())
                    .ok_or_else(|| format!("Invalid item definition in {location}"))?;
                self.check(hash, &location);
                if let equipment::EquippedItemPlugs::Authored(plugs) = row.plugs {
                    for (lane, value) in plugs.into_iter().enumerate() {
                        if let equipment::EquippedPlugValue::Hash(hash) = value {
                            self.check(
                                u32::try_from(hash).map_err(|e| e.to_string())?,
                                &format!("{location} socket[{lane}]"),
                            );
                        }
                    }
                }
            }
        }
        for row in inventory::profile_items(document)
            .map_err(|e| e.to_string())?
            .unwrap_or_default()
        {
            self.check(
                row.definition_hash,
                &format!("Profile items[{}]", row.location.index),
            );
        }
        for row in inventory::dismantle_rewards(document)
            .map_err(|e| e.to_string())?
            .unwrap_or_default()
        {
            self.check(
                row.definition_hash,
                &format!("Dismantle rewards[{}]", row.location.index),
            );
        }
        Ok(())
    }

    fn sqlite(&mut self, snapshot: &Snapshot) -> Result<(), String> {
        for name in [
            "items",
            "character_items",
            "profile_items",
            "character_stacks",
            "dismantle_rewards",
            "pending_rewards",
            "sockets",
            "item_sockets",
        ] {
            let Some(table) = snapshot.tables.get(name) else {
                continue;
            };
            let socket = matches!(name, "sockets" | "item_sockets");
            let column_name = if socket {
                "plug_hash"
            } else {
                "definition_hash"
            };
            let column = table
                .columns
                .iter()
                .position(|column| column == column_name)
                .ok_or_else(|| format!("Missing {name}.{column_name} in account snapshot"))?;
            for row in &table.rows {
                let hash = match row.get(column) {
                    Some(Cell::Null) if socket => continue,
                    Some(Cell::Integer(hash)) => u32::try_from(*hash)
                        .map_err(|e| format!("Invalid {name}.{column_name}: {e}"))?,
                    _ => return Err(format!("Invalid {name}.{column_name} in account snapshot")),
                };
                let mut location = format!("{name} {}", row_location(table, row));
                if socket {
                    let parent = if name == "sockets" {
                        "items"
                    } else {
                        "character_items"
                    };
                    if let (Some(instance), Some(items)) = (
                        cell(table, row, "instance_soid"),
                        snapshot.tables.get(parent),
                    ) && let Some(item) = items
                        .rows
                        .iter()
                        .find(|item| cell(items, item, "instance_soid") == Some(instance))
                    {
                        location = format!("{parent} {} {location}", row_location(items, item));
                    }
                }
                self.check(hash, &location);
            }
        }
        Ok(())
    }
}

fn cell<'a>(table: &Table, row: &'a [Cell], name: &str) -> Option<&'a Cell> {
    row.get(table.columns.iter().position(|column| column == name)?)
}

fn row_location(table: &Table, row: &[Cell]) -> String {
    [
        "character_slot",
        "character_soid",
        "location",
        "position",
        "instance_soid",
        "lane",
        "debt_id",
    ]
    .into_iter()
    .filter_map(|name| match cell(table, row, name) {
        Some(Cell::Integer(value)) => Some(format!("{name}={value}")),
        Some(Cell::Text(value)) => Some(format!("{name}={value}")),
        _ => None,
    })
    .collect::<Vec<_>>()
    .join(" ")
}
