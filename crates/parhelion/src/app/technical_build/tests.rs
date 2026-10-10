use super::{json_summary, marker_section, runtime_registry_section, technical_build_report};
use crate::artifact::ArtifactMetadata;
use crate::recipe::{AdditionalBehaviorRecipe, WeaponRecipe};
use crate::workflow::{
    BuildReport, CustomPlugBuildReport, PrivatePerkBuildReport, WeaponBuildReport,
};
use std::path::PathBuf;
use sundial::package_authoring::gear_markers::{Marker, MarkerSet};
use sundial::package_authoring::runtime::{BindingHash, SchemaHandle};
use sundial::package_authoring::runtime::{
    WeaponRuntimeBinding, WeaponRuntimeField, WeaponRuntimeFieldLocator, WeaponRuntimeFieldSource,
    WeaponRuntimeGraph, WeaponRuntimeOwner, WeaponRuntimeResource, WeaponRuntimeRoot,
    WeaponRuntimeRootKind, WeaponRuntimeValue, WeaponRuntimeValueKind,
};

fn build() -> BuildReport {
    BuildReport {
        weapons: vec![WeaponBuildReport {
            name: "Good Company".into(),
            kind: crate::ItemKind::Weapon,
            namespace: "parhelion.good-company".into(),
            item_hash: 0x1111_2222,
            item_definition_hash: 0x8080_0001,
            item_string_hash: 0x8080_0002,
            icon_definition_hash: 0x8080_0003,
            item_index: 15_726,
            details: None,
            subclass: None,
            recipe_fingerprint: String::new(),
            collection: Some(crate::NewCollectionPlan {
                collectible_hash: 0x3333_4444,
                collectible_index: 5_378,
                unlock_hash: 0x5555_6666,
                unlock_definition_index: 21_810,
                unlock_bank: 1,
                unlock_slot: 12_301,
            }),
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
                offered_sets: Vec::new(),
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
    let report = technical_build_report(Some(&build()), &recipe(), None, "", "");
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
    let report = technical_build_report(Some(&build()), &recipe(), None, "", "");
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
    assert!(
        !report
            .lines()
            .any(|line| { line.contains("description") && line.contains("0x00000000") })
    );
}

/// A borrowed behavior is the part hardest to verify in game, so the report names both halves it
/// brings and the perks it pins.
#[test]
fn the_report_explains_what_a_borrowed_behavior_brings() {
    let report = technical_build_report(Some(&build()), &recipe(), None, "", "");
    assert!(report.contains("graviton-lance-graph"));
    assert!(report.contains("Graviton Lance"));
    // It pairs with its weapon's record, which the graft applies without being asked.
    assert!(report.contains("carries record of"));
}

/// A recipe naming a behavior the catalog does not hold must say so rather than drop the line.
#[test]
fn an_unknown_behavior_is_reported_rather_than_skipped() {
    let mut recipe = recipe();
    recipe.overrides.additional_behaviors = vec![AdditionalBehaviorRecipe {
        behavior: "not-a-behavior".to_owned(),
    }];
    let report = technical_build_report(Some(&build()), &recipe, None, "", "");
    assert!(report.contains("not-a-behavior"));
    assert!(report.contains("<not in the catalog>"));
}

/// Opening the window before a build is the common case: the recipe already fixes every hash a
/// build will write, so the report says what the next build assigns rather than staying shut.
#[test]
fn without_a_staged_build_the_report_lists_the_identities_the_next_build_assigns() {
    let recipe = recipe();
    let report = technical_build_report(None, &recipe, None, "", "");
    assert!(
        report
            .lines()
            .next()
            .is_some_and(|line| { line.contains("NEXT BUILD") && line.contains(&recipe.name) })
    );
    assert!(report.contains("No staged build"));
    assert!(
        report
            .lines()
            .any(|line| { line.contains("namespace") && line.contains(&recipe.namespace) })
    );
    let hashes = recipe.identity.parsed_hashes(&recipe.namespace).unwrap();
    for (name, value) in [
        ("item hash", hashes[0]),
        ("collectible hash", hashes[1]),
        ("unlock hash", hashes[2]),
        ("collection requirement", hashes[11]),
    ] {
        assert!(
            report
                .lines()
                .any(|line| { line.contains(name) && line.contains(&format!("0x{value:08X}")) }),
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
    let report = technical_build_report(None, &recipe, None, "", "");
    let document = serde_json::to_value(&recipe).unwrap();
    let mut keys = Vec::new();
    collect_keys(&document, &mut keys);
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
        "UNIQUE WEAPON BEHAVIOR",
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
        assert!(report.contains(property), "{property} is not resolved");
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

fn registry_graph() -> WeaponRuntimeGraph {
    let field = |name: &str, kind, value| WeaponRuntimeField {
        locator: WeaponRuntimeFieldLocator {
            graph_tag: None,
            binding_hash: BindingHash::new(0xD5A1_23FF),
            resource_index: 0,
            root: WeaponRuntimeRootKind::Instance,
            root_schema: SchemaHandle::new(0x8080_3889),
            path: Vec::new(),
            type_handle: SchemaHandle::new(0),
            value_offset: 8,
            byte_size: 4,
        },
        owner_offset: 0x44,
        name: name.to_owned(),
        path_label: format!("Instance.{name}"),
        kind,
        value,
        source: WeaponRuntimeFieldSource::NativeMember,
        generated_kind: None,
        name_inferred: false,
    };
    let root = WeaponRuntimeRoot {
        kind: WeaponRuntimeRootKind::Instance,
        schema: 0x8080_3889,
        owner_offset: 0x40,
        byte_size: 64,
        generated_schema: false,
        structure: Default::default(),
        fields: vec![
            field(
                "Rounds Per Minute",
                WeaponRuntimeValueKind::UnsignedInteger { bits: 32 },
                WeaponRuntimeValue::Unsigned(600),
            ),
            field(
                "Recoil Scale",
                WeaponRuntimeValueKind::Float32,
                WeaponRuntimeValue::Float32Bits(0.75_f32.to_bits()),
            ),
            {
                let mut traversed = field(
                    "Unnamed Field +0x150",
                    WeaponRuntimeValueKind::Vector4Float32,
                    WeaponRuntimeValue::Vector4Float32Bits([0; 4]),
                );
                traversed.path_label =
                    "Component Instance / Type 0x808092D8 +0x150 / Unnamed Field +0x150".to_owned();
                traversed
            },
        ],
    };
    WeaponRuntimeGraph {
        item_hash: 0x5ED8_0A4D,
        pattern_global_id_hash: 0x1315_B807,
        entity_tag: 0x80BB_825C,
        bindings: vec![WeaponRuntimeBinding {
            binding_hash: 0xD5A1_23FF,
            binding_label: "Trigger".into(),
            resource_index: 0,
            resource_count: 1,
            owner_tag: 0x8152_2686,
            concrete_class: 0x8080_388A,
            resource_offset: 0x45B0,
        }],
        resources: vec![WeaponRuntimeResource {
            binding_hash: 0xD5A1_23FF,
            binding_label: "Trigger".into(),
            resource_index: 0,
            resource_count: 1,
            owner_tag: 0x8152_2686,
            concrete_class: 0x8080_388A,
            alias_bindings: vec![(0x68F6_8780, 0)],
            instance: root.clone(),
            definition: None,
        }],
        owners: vec![WeaponRuntimeOwner {
            owner_tag: 0x8152_2686,
            anchor_binding_hash: 0xD5A1_23FF,
            anchor_resource_index: 0,
            roots: vec![root],
        }],
    }
}

/// The registry is the reason to open this window when a runtime edit misbehaves: it has to
/// name the binding, the owner it resolves to and the field's current value, or a locator in
/// the recipe cannot be matched to what it addresses.
#[test]
fn the_report_carries_the_effective_runtime_registry() {
    let graph = registry_graph();
    let report = runtime_registry_section(Some(Ok(&graph)), true);
    assert!(report.lines().any(|line| {
        line.contains("RUNTIME REGISTRY")
            && line.contains("0x80BB825C")
            && line.contains("0x5ED80A4D")
    }));
    assert!(report.contains("RUNTIME BINDINGS  (1)"));
    assert!(
        report
            .lines()
            .any(|line| { line.contains("0xD5A123FF") && line.contains("Trigger") })
    );
    assert!(report.contains("owner 0x81522686  class 0x8080388A"));
    // The alias tells a reader that regrafting this binding moves the other one too.
    assert!(report.contains("also 0x68F68780#0"));
    assert!(report.contains("OWNER 0x81522686  anchor 0xD5A123FF#0"));
    assert!(report.contains("resource fields"));
    assert!(report.contains("Rounds Per Minute"));
    assert!(report.contains("600"));
    // Float fields are decoded, not left as raw bits, so a value can be read at a glance.
    assert!(report.contains("0.75"));
    // A path that only repeats the field name is noise, so it is left off.
    assert!(!report.contains("Instance.Recoil Scale"));
    // A path that records a real traversal is what a recipe locator stores, so it is kept.
    assert!(report.contains("Component Instance / Type 0x808092D8 +0x150 / Unnamed Field +0x150"));
    // The section is what the window appends, so the report has to carry it verbatim.
    let embedded = technical_build_report(None, &recipe(), None, "", &report);
    assert!(embedded.contains("RUNTIME BINDINGS  (1)"));
    assert!(embedded.contains("RECIPE DOCUMENT"));
}

/// A marker's name is the whole point of the section: the runtime resolves markers by name, so
/// a reader has to see which named points the appearance actually carries and where they sit.
#[test]
fn the_report_names_the_markers_the_appearance_carries() {
    let markers = vec![MarkerSet {
        entity: 0x8087_1F2A,
        component: 0x8161_ECC9,
        markers: vec![
            Marker {
                name: 0xF2A0_71CC,
                position: [0.613_493, -0.001_370, 0.099_092],
                orientation: [0.0, 0.0, 0.0, 1.0],
            },
            // Same name and place as the marker above would be indistinguishable in the report
            // without the rotation, which is the only thing that differs on a real weapon.
            Marker {
                name: 0xF2A0_71CC,
                position: [0.613_493, -0.001_370, 0.099_092],
                orientation: [0.0, -0.642_8, 0.0, 0.766_0],
            },
            Marker {
                name: 0x0B7B_A45D,
                position: [0.0, 0.5, -0.25],
                orientation: [0.0, 0.0, 0.0, 1.0],
            },
        ],
    }];
    let section = marker_section(Some(Ok(&markers)), &[]);
    assert!(section.contains("MARKERS  3 on 1 objects, 2 named"));
    assert!(section.contains("object 0x80871F2A  set 0x8161ECC9  (3)"));
    // The turned copy is distinguishable from the aligned one, which is the whole point.
    assert!(section.contains("facing"));
    assert_eq!(section.matches("facing").count(), 1);
    assert!(section.contains("primary_fire"));
    assert!(section.contains("0.61349"));
    // The third marker's distinct position is retained even if its name becomes known.
    assert!(section.contains("-0.25"));
    // The section is what the window appends, so the report has to carry it verbatim.
    let report = technical_build_report(None, &recipe(), None, &section, "");
    assert!(report.contains("primary_fire"));
    assert!(report.contains("APPEARANCE"));
}

/// Markers are read from the packages, so an unread or failed read has to say so rather than
/// look like a weapon whose art carries no markers at all.
#[test]
fn an_unread_or_empty_marker_read_is_distinguishable_from_one_with_no_markers() {
    assert!(marker_section(None, &[]).contains("MARKERS  not read"));
    let failed = marker_section(Some(Err("Resource 0x80805DF5 is missing")), &[]);
    assert!(failed.contains("MARKERS  unavailable: Resource 0x80805DF5 is missing"));
    let none = marker_section(Some(Ok(&[])), &[]);
    assert!(none.contains("MARKERS  0 on 0 objects, 0 named"));
    assert!(none.contains("this appearance carries none"));
}

/// Without a finished scan the section says so instead of silently omitting itself, so a
/// reader never mistakes an unread registry for a weapon that has none.
#[test]
fn an_unread_or_failed_registry_is_reported_rather_than_omitted() {
    let pending = runtime_registry_section(None, true);
    assert!(pending.contains("RUNTIME REGISTRY  not read"));
    let failed = runtime_registry_section(Some(Err("runtime row 216 is outside the table")), true);
    assert!(failed.contains("RUNTIME REGISTRY  unavailable: runtime row 216 is outside the table"));
    assert!(!failed.contains("RUNTIME BINDINGS"));
}

/// An embedded image is summarized by length and digest, so the appearance section stays one
/// short line per icon instead of carrying the whole image.
#[test]
fn embedded_images_are_summarized_rather_than_written_out() {
    let image = "A".repeat(4_096);
    let line = json_summary(&serde_json::json!({ "png_base64": image, "scale": 1.5 }));
    assert!(line.contains("<4096 bytes, sha256 "), "{line}");
    assert!(line.contains("\"scale\":1.5"), "{line}");
    assert!(line.contains("6896d9ea3f73"), "{line}");
    assert!(!line.contains(&"A".repeat(256)), "{line}");
}
