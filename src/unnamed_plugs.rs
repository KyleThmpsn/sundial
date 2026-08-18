use std::{collections::HashMap, sync::OnceLock};

use serde::Deserialize;

use crate::hash::parse_hash;

const DEFINITION_DATABASE_JSON: &str = include_str!("../assets/unnamed-plugs.json");
const DEFINITION_DATABASE_SCHEMA: u32 = 2;

#[derive(Debug, Deserialize)]
struct DefinitionDatabase {
    schema: u32,
    manifest_version: String,
    plugs: Vec<SerializedPlugDefinition>,
}

#[derive(Debug, Deserialize)]
struct SerializedPlugDefinition {
    hash: String,
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

pub(crate) fn manifest_version() -> &'static str {
    &definition_database().manifest_version
}

fn definitions() -> impl Iterator<Item = PlugDefinition<'static>> {
    definition_database().plugs.iter().filter_map(|definition| {
        Some(PlugDefinition {
            hash: parse_hash(&definition.hash)?,
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

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;

    #[test]
    fn embedded_plug_definitions_are_complete_and_unique() {
        let database = definition_database();
        let definitions = definitions().collect::<Vec<_>>();
        let hashes = definitions
            .iter()
            .map(|definition| definition.hash)
            .collect::<HashSet<_>>();

        assert_eq!(database.schema, DEFINITION_DATABASE_SCHEMA);
        assert_eq!(database.manifest_version, "86657.20.08.23.1800-9");
        assert_eq!(definitions.len(), 316);
        assert_eq!(hashes.len(), definitions.len());
        assert!(
            definitions
                .windows(2)
                .all(|pair| pair[0].hash < pair[1].hash)
        );
        assert!(
            definitions.iter().all(|definition| {
                !definition.name.is_empty() && !definition.type_name.is_empty()
            })
        );
        assert_eq!(
            definitions
                .iter()
                .filter(|definition| definition.type_name == "Armor Energy")
                .count(),
            115
        );
        assert_eq!(
            definitions
                .iter()
                .filter(|definition| definition.type_name == "Top Stat Allocation")
                .count(),
            98
        );
        assert_eq!(
            definitions
                .iter()
                .filter(|definition| definition.type_name == "Bottom Stat Allocation")
                .count(),
            103
        );
        assert!(
            definitions
                .iter()
                .all(|definition| match definition.type_name {
                    "Top Stat Allocation" => {
                        definition.name.contains("Mobility")
                            && definition.name.contains("Resilience")
                            && definition.name.contains("Recovery")
                    }
                    "Bottom Stat Allocation" => {
                        definition.name.contains("Discipline")
                            && definition.name.contains("Intellect")
                            && definition.name.contains("Strength")
                    }
                    "Armor Energy" => definition.name.contains("Energy"),
                    _ => false,
                })
        );
    }

    #[test]
    fn embedded_data_is_only_a_fallback_for_specific_local_names() {
        let mut names = HashMap::from([
            (0x0104_5049, "Locally resolved name".to_owned()),
            (0x0F8A_2F00, "Upgrade Armor".to_owned()),
            (0x0001_1D37, "Unknown plug".to_owned()),
        ]);
        let mut type_names = HashMap::new();

        apply_to_catalog(&mut names, &mut type_names);

        assert_eq!(names[&0x0104_5049], "Locally resolved name");
        assert_eq!(names[&0x0F8A_2F00], "Void Energy 3");
        assert_eq!(
            names[&0x0001_1D37],
            "1 Discipline / 9 Intellect / 5 Strength"
        );
        assert_eq!(type_names[&0x0F8A_2F00], "Armor Energy");
        assert_eq!(type_names[&0x0001_1D37], "Bottom Stat Allocation");
    }
}
