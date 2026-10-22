use std::{collections::HashMap, sync::OnceLock};

use serde::Deserialize;

use crate::hash::parse_hash_hex;

const DEFINITION_DATABASE_JSON: &str = include_str!("../../assets/unnamed-plugs.json");
const DEFINITION_DATABASE_SCHEMA: u32 = 2;

#[derive(Debug, Deserialize)]
struct DefinitionDatabase {
    schema: u32,
    manifest_version: String,
    plugs: Vec<SerializedPlugDefinition>,
}

#[derive(Debug, Deserialize)]
struct SerializedPlugDefinition {
    #[serde(rename = "hash")]
    hash_hex: String,
    name: String,
    type_name: String,
}

#[derive(Debug, PartialEq, Eq)]
struct PlugDefinition<'a> {
    hash: u64,
    name: &'a str,
    type_name: &'a str,
}

pub(crate) fn apply_to_catalog(
    names: &mut HashMap<u64, String>,
    type_names: &mut HashMap<u64, String>,
) {
    for definition in definitions() {
        match names.get_mut(&definition.hash) {
            Some(name) if is_placeholder_name(name) => definition.name.clone_into(name),
            Some(_) => {}
            None => {
                names.insert(definition.hash, definition.name.to_owned());
            }
        }

        match type_names.get_mut(&definition.hash) {
            Some(type_name) if is_missing_label(type_name) => {
                definition.type_name.clone_into(type_name);
            }
            Some(_) => {}
            None => {
                type_names.insert(definition.hash, definition.type_name.to_owned());
            }
        }
    }
}

/// Whether a plug's name came from this database rather than the game, because it has none.
pub(crate) fn contains(hash: u64) -> bool {
    definitions().any(|definition| definition.hash == hash)
}

fn definitions() -> impl Iterator<Item = PlugDefinition<'static>> {
    definition_database().plugs.iter().filter_map(|definition| {
        Some(PlugDefinition {
            hash: parse_hash_hex(&definition.hash_hex)?,
            name: definition.name.trim(),
            type_name: definition.type_name.trim(),
        })
    })
}

fn definition_database() -> &'static DefinitionDatabase {
    static DATABASE: OnceLock<DefinitionDatabase> = OnceLock::new();
    DATABASE.get_or_init(|| {
        let database: DefinitionDatabase = serde_json::from_str(DEFINITION_DATABASE_JSON)
            .expect("embedded unnamed plug definition database must be valid JSON");
        assert_eq!(
            database.schema, DEFINITION_DATABASE_SCHEMA,
            "embedded unnamed plug definition database schema is unsupported"
        );
        assert!(
            !database.manifest_version.trim().is_empty(),
            "embedded unnamed plug definition database must identify its source manifest"
        );
        database
    })
}

fn is_placeholder_name(name: &str) -> bool {
    let name = name.trim();
    is_missing_label(name)
        || name.eq_ignore_ascii_case("Upgrade Armor")
        || name.eq_ignore_ascii_case("Change Energy Type")
        || name.eq_ignore_ascii_case("Masterwork")
}

fn is_missing_label(label: &str) -> bool {
    let label = label.trim();
    label.is_empty() || label.to_ascii_lowercase().starts_with("unknown")
}
