use super::technical_build_report;
use crate::artifact::ArtifactMetadata;
use crate::recipe::{AdditionalBehaviorRecipe, WeaponRecipe};
use crate::workflow::{
    BuildReport, CustomPlugBuildReport, PrivatePerkBuildReport, WeaponBuildReport,
};
use std::path::PathBuf;

fn build() -> BuildReport {
    BuildReport {
        weapons: vec![WeaponBuildReport {
            name: "Good Company".into(),
            namespace: "parhelion.good-company".into(),
            item_hash: 0x1111_2222,
            item_definition_hash: 0x8080_0001,
            item_string_hash: 0x8080_0002,
            icon_definition_hash: 0x8080_0003,
            item_index: 15_726,
            collectible_hash: 0x3333_4444,
            collectible_index: 5_378,
            unlock_hash: 0x5555_6666,
            unlock_definition_index: 21_810,
            unlock_bank: 1,
            unlock_slot: 12_301,
            custom_plugs: vec![CustomPlugBuildReport {
                socket_index: 3,
                choice_index: 1,
                name: Some("Private Outlaw".into()),
                item_hash: 0x7777_8888,
                item_index: 15_727,
                definition_hash: 0x8080_0004,
                string_hash: 0x8080_0005,
                icon_definition_hash: Some(0x8080_0006),
                name_hash: Some(0x9999_0001),
                description_hash: None,
                perks: vec![PrivatePerkBuildReport {
                    source_perk_index: 512,
                    perk_hash: 0xAAAA_BBBB,
                    runtime_key: 0xA43A_8C2E,
                }],
            }],
        }],
        run_directory: PathBuf::from("/staging/run-1"),
        manifest_path: PathBuf::from("/staging/run-1/manifest.json"),
        artifacts: vec![ArtifactMetadata {
            file_name: "w64_investment_globals_01a3_0.pkg".into(),
            byte_length: 4_242,
            sha256: "abc123".into(),
        }],
        selection_fingerprint: "fingerprint-1".into(),
        staged_recipe_paths: vec![PathBuf::from("/staging/run-1/recipes/good-company.json")],
    }
}

fn recipe() -> WeaponRecipe {
    let mut recipe =
        WeaponRecipe::from_json_str(include_str!("../../../recipes/redacted.parhelion.json"))
            .unwrap();
    recipe.overrides.additional_behaviors = vec![AdditionalBehaviorRecipe {
        behavior: "graviton-lance-graph".to_owned(),
    }];
    recipe
}

/// The window exists so a build can be checked against what was intended, so every identity the
/// build assigned has to appear. A missing one is invisible rather than wrong, which is the
/// failure this guards.
#[test]
fn the_report_carries_every_identity_the_build_assigned() {
    let report = technical_build_report(Some(&build()), &recipe(), None, &|_| true);
    for expected in [
        "fingerprint-1",
        "/staging/run-1",
        "manifest.json",
        "Good Company",
        "parhelion.good-company",
        "0x11112222",
        "0x80800001",
        "0x80800002",
        "0x80800003",
        "15726",
        "0x33334444",
        "5378",
        "0x55556666",
        "21810",
        "1 / 12301",
        "w64_investment_globals_01a3_0.pkg",
        "4242",
        "abc123",
        "good-company.json",
    ] {
        assert!(report.contains(expected), "missing {expected}:\n{report}");
    }
}

/// A private perk's identities are what a bug report needs, and they are three levels down.
#[test]
fn the_report_reaches_private_plugs_and_their_perks() {
    let report = technical_build_report(Some(&build()), &recipe(), None, &|_| true);
    for expected in [
        "socket 3 choice 1",
        "Private Outlaw",
        "0x77778888",
        "15727",
        "0x80800004",
        "0x80800005",
        "0x80800006",
        "0x99990001",
        "perk source 512",
        "0xAAAABBBB",
        "0xA43A8C2E",
    ] {
        assert!(report.contains(expected), "missing {expected}:\n{report}");
    }
    // The description hash was absent, so it is not invented as a zero.
    assert!(!report.contains("description     0x00000000"));
}

/// A borrowed behavior is the part hardest to verify in game, so the report names both halves it
/// brings and the perks it pins.
#[test]
fn the_report_explains_what_a_borrowed_behavior_brings() {
    let report = technical_build_report(Some(&build()), &recipe(), None, &|_| true);
    let entry = crate::weapon_behavior::behavior("graviton-lance-graph").unwrap();
    assert!(report.contains("graviton-lance-graph"));
    assert!(report.contains("Graviton Lance"));
    assert!(report.contains(&format!("0x{:08X}", entry.graph_tag().unwrap())));
    assert!(report.contains(&format!("0x{:08X}", entry.intrinsic_plug.unwrap())));
    assert!(report.contains(&format!("0x{:08X}", entry.trait_plug.unwrap())));
    // It pairs with its weapon's record, which the graft applies without being asked.
    assert!(report.contains("carries record of"));
}

/// A recipe naming a behavior the catalogue does not hold must say so rather than drop the line.
#[test]
fn an_unknown_behavior_is_reported_rather_than_skipped() {
    let mut recipe = recipe();
    recipe.overrides.additional_behaviors = vec![AdditionalBehaviorRecipe {
        behavior: "not-a-behavior".to_owned(),
    }];
    let report = technical_build_report(Some(&build()), &recipe, None, &|_| true);
    assert!(report.contains("not-a-behavior"));
    assert!(report.contains("<not in the catalogue>"));
}

/// Opening the window before a build is the common case: the recipe already fixes every hash a
/// build will write, so the report says what the next build assigns rather than staying shut.
#[test]
fn without_a_staged_build_the_report_lists_the_identities_the_next_build_assigns() {
    let recipe = recipe();
    let report = technical_build_report(None, &recipe, None, &|_| true);
    assert!(report.starts_with(&format!("NEXT BUILD  {}", recipe.name)));
    assert!(report.contains("No build is staged"));
    assert!(report.contains(&format!("namespace                 {}", recipe.namespace)));
    let hashes = recipe.identity.parsed_hashes(&recipe.namespace).unwrap();
    for (name, value) in [
        ("item hash", hashes[0]),
        ("collectible hash", hashes[1]),
        ("unlock hash", hashes[2]),
        ("collection requirement", hashes[11]),
    ] {
        assert!(
            report.contains(&format!("{name:<26}0x{value:08X}")),
            "{name} missing from {report}"
        );
    }
    assert!(!report.contains("ARTIFACTS"), "no artifacts exist yet");
    assert!(report.contains("ITEM DEFINITION"));
}

/// "Every single thing" is the requirement. The curated sections can only name fields someone
/// remembered, so the document walk at the end must surface every key the recipe serializes,
/// at every depth, and the resolved sections must cover each donor-inheritable property.
#[test]
fn the_report_walks_every_key_of_the_recipe_document() {
    let recipe = recipe();
    let report = technical_build_report(None, &recipe, None, &|_| true);
    let document = serde_json::to_value(&recipe).unwrap();
    let mut keys = Vec::new();
    collect_keys(&document, &mut keys);
    assert!(
        keys.len() > 40,
        "the fixture recipe should be rich: {}",
        keys.len()
    );
    for key in keys {
        assert!(
            report.contains(&format!("{key}:")),
            "recipe key {key} is missing from the report"
        );
    }
    for section in [
        "DONORS",
        "ITEM DEFINITION",
        "INVESTMENT STATS",
        "BASE PERKS AND TRAITS",
        "TEXT",
        "APPEARANCE",
        "BORROWED BEHAVIOR",
        "SOCKETS",
        "PRIVATE PERK VARIANTS",
        "RUNTIME PATCHES",
        "RECIPE DOCUMENT",
    ] {
        assert!(report.contains(section), "{section} section missing");
    }
    for property in [
        "inventory slot",
        "ammo type",
        "damage type",
        "rarity",
        "power cap groups",
        "max stack size",
        "socket entry list",
        "plug category hash",
        "roll set index",
        "linked plug index",
        "weapon pattern index",
        "stat group index",
        "base sandbox perks",
        "trait indices",
        "art arrangements",
        "dye rows custom",
    ] {
        assert!(
            report.contains(&format!("{property:<26}")),
            "{property} is not resolved"
        );
    }
    assert!(report.contains("catalog not loaded"));
}

fn collect_keys(value: &serde_json::Value, keys: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                keys.push(key.clone());
                collect_keys(value, keys);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_keys(item, keys);
            }
        }
        _ => {}
    }
}
