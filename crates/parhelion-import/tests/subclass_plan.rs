//! Persisted research capture -> checked selection -> replayable, non-installable port plan.

use anyhow::{Result, ensure};
use parhelion_import::{cancellation, d2_mot::subclass};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

fn input() -> Value {
    json!({
        "request": {
            "item_hash": 100,
            "element": "strand",
            "selections": [
                {"socket_index": 4, "plug_hash": 200, "role": "melee"},
                {"socket_index": 8, "plug_hash": 300, "role": "aspect"}
            ]
        },
        "capture": {
            "format": 1,
            "provenance": {
                "decoder": "Destiny2TheEdgeOfFate",
                "package_stamp": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "executable_build": null
            },
            "item_hash": 100, "item_tag": "80801000",
            "sockets": [
                {"index": 4, "socket_type_index": 91, "plug_sets": [400], "choices": [
                    {"hash": 200, "tag": "80801001", "category": 500, "default": true},
                    {"hash": 201, "tag": "80801002", "category": 500, "default": false}
                ]},
                {"index": 8, "socket_type_index": 92, "plug_sets": [401], "choices": [
                    {"hash": 300, "tag": "80801003", "category": 501, "default": true}
                ]}
            ],
            "traces": [
                {"format": 1, "plug_hash": 200, "item_tag": "80801001", "installable": false,
                 "perks": [{"perk_hash": 600, "runtime_key": 700, "source_index": 12,
                    "tag": "80801004", "class": "8080B835", "decoded_controller": true,
                    "requires_native_lowering": true, "states": [{"key": 800, "offset": 128, "effects": [], "condition_groups": []}],
                    "auxiliary": [], "future_source_field": {"opaque": 44}}]},
                {"format": 1, "plug_hash": 300, "item_tag": "80801003", "installable": false,
                 "perks": [
                    {"perk_hash": 601, "runtime_key": 701, "source_index": 13,
                     "tag": "80801005", "class": "8080FFFF", "decoded_controller": false,
                     "requires_native_lowering": true, "states": []},
                    {"perk_hash": 602, "runtime_key": 702, "source_index": 14,
                     "states": [], "unresolved": "No standalone runtime assignment"}
                 ]}
            ]
        }
    })
}

fn save(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[test]
fn selected_source_evidence_survives_planning_and_replay() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    let source = root.join("input.json");
    save(&source, &input())?;
    let before = fs::read(&source)?;
    let output = root.join("plan.json");
    subclass::plan_file(&source, &output)?;
    let report_bytes = fs::read(&output)?;
    let report: Value = serde_json::from_slice(&report_bytes)?;

    // Read the artifact as JSON, independently of the planner's model types.
    ensure!(report["installable"] == false && report["gameplay_verified"] == false);
    ensure!(report["target_build"] == "86657.20.08.23.1800.d2_rc___release");
    ensure!(report["input"] == input());
    ensure!(report["input_sha256"] == digest(&serde_json::to_vec(&report["input"])?));
    ensure!(report["plan"]["damage"]["requested"] == "strand");
    ensure!(report["plan"]["damage"]["native"] == "kinetic");
    ensure!(report["plan"]["damage"]["presentation"] == "strand");
    ensure!(report["plan"]["damage"]["policy"] == "strand_as_kinetic");
    let abilities = report["plan"]["abilities"].as_array().unwrap();
    ensure!(abilities.len() == 2);
    ensure!(abilities[0]["selection"]["socket_index"] == 4);
    ensure!(abilities[0]["source_tag"] == "80801001");
    ensure!(abilities[0]["controllers"][0]["runtime_key"] == 700);
    ensure!(abilities[0]["controllers"][0]["status"] == "captured");
    ensure!(abilities[1]["controllers"][0]["status"] == "unknown_class");
    ensure!(abilities[1]["controllers"][1]["status"] == "unresolved");
    ensure!(
        report["plan"]["pending"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "source_damage_providers")
    );

    let replay = root.join("replay-input.json");
    save(&replay, &report["input"])?;
    subclass::plan_file(&replay, &root.join("replayed.json"))?;
    ensure!(fs::read(root.join("replayed.json"))? == report_bytes);
    ensure!(fs::read(&source)? == before);

    // Existing output is immutable, even when the next plan would be different.
    let mut changed = input();
    changed["request"]["element"] = json!("void");
    save(&source, &changed)?;
    ensure!(subclass::plan_file(&source, &output).is_err());
    ensure!(fs::read(&output)? == report_bytes);
    ensure!(subclass::plan_file(&source, &source).is_err());
    ensure!(serde_json::from_slice::<Value>(&fs::read(&source)?)? == changed);

    let artifact = json!({
        "fixture_kind": "synthetic persisted source trace, not mounted game packages",
        "source_build": null,
        "target_build": report["target_build"],
        "revision": std::env::var("PARHELION_TEST_REVISION").ok(),
        "input": report["input"], "report": report,
        "input_sha256": digest(&before), "report_sha256": digest(&report_bytes),
        "independent_readback": "JSON identities, membership, controller resolution, damage and readiness",
        "limits": ["No modern ability graph lowering", "No native packages emitted", "No gameplay trial"]
    });
    if let Some(folder) = std::env::var_os("PARHELION_SUBCLASS_TEST_OUTPUT") {
        let folder = std::path::PathBuf::from(folder);
        fs::create_dir(&folder)?;
        save(&folder.join("verified-plan.json"), &artifact)?;
        fs::write(folder.join("input.json"), before)?;
        fs::write(folder.join("plan.json"), report_bytes)?;
    } else {
        println!("{}", serde_json::to_string(&artifact)?);
    }
    Ok(())
}

#[test]
fn invalid_or_cancelled_selections_publish_nothing() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    let mut cases = Vec::new();
    let mut changed = input();
    changed["request"]["item_hash"] = json!(999);
    cases.push(changed);
    let mut changed = input();
    changed["request"]["selections"] = json!([]);
    cases.push(changed);
    let mut changed = input();
    changed["request"]["selections"][1]["socket_index"] = json!(4);
    cases.push(changed);
    let mut changed = input();
    changed["request"]["selections"][0]["plug_hash"] = json!(300);
    cases.push(changed);
    let mut changed = input();
    changed["capture"]["traces"][0]["item_tag"] = json!("80801099");
    cases.push(changed);
    let mut changed = input();
    changed["capture"]["traces"][0]["plug_hash"] = json!(201);
    cases.push(changed);
    let mut changed = input();
    changed["capture"]["format"] = json!(2);
    cases.push(changed);
    let mut changed = input();
    changed["capture"]["sockets"][1]["index"] = json!(4);
    cases.push(changed);
    let mut changed = input();
    changed["capture"]["traces"][0]["installable"] = json!(true);
    cases.push(changed);
    let mut changed = input();
    changed["capture"]["traces"][1]["perks"][0]["tag"] = json!("bad-tag");
    cases.push(changed);
    for (index, case) in cases.iter().enumerate() {
        let source = root.join(format!("input-{index}.json"));
        let output = root.join(format!("plan-{index}.json"));
        save(&source, case)?;
        ensure!(
            subclass::plan_file(&source, &output).is_err(),
            "Accepted invalid case {index}"
        );
        ensure!(!output.exists(), "Published invalid case {index}");
    }
    let source = root.join("cancelled-input.json");
    let output = root.join("cancelled-plan.json");
    save(&source, &input())?;
    let error = cancellation::run(Arc::new(AtomicBool::new(true)), || {
        subclass::plan_file(&source, &output)
    })
    .unwrap_err();
    ensure!(error.downcast_ref::<cancellation::Cancelled>().is_some());
    ensure!(!output.exists());
    Ok(())
}

#[test]
fn absent_runtime_and_unknown_element_remain_explicit() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let mut value = input();
    value["request"]["element"] = Value::Null;
    value["capture"]["traces"][0]["perks"] = json!([]);
    let source = temporary.path().join("input.json");
    let output = temporary.path().join("plan.json");
    save(&source, &value)?;
    subclass::plan_file(&source, &output)?;
    let report: Value = serde_json::from_slice(&fs::read(&output)?)?;
    ensure!(report["plan"]["damage"]["requested"] == Value::Null);
    ensure!(report["plan"]["damage"]["native"] == Value::Null);
    ensure!(report["plan"]["abilities"][0]["controllers"] == json!([]));
    ensure!(
        report["plan"]["abilities"][0]["pending"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "source_ability_graph")
    );
    ensure!(report["installable"] == false);
    value["request"]["element"] = json!("stasis");
    save(&source, &value)?;
    let stasis_output = temporary.path().join("stasis-plan.json");
    subclass::plan_file(&source, &stasis_output)?;
    let stasis: Value = serde_json::from_slice(&fs::read(stasis_output)?)?;
    ensure!(stasis["plan"]["damage"]["requested"] == "stasis");
    ensure!(stasis["plan"]["damage"]["native"] == Value::Null);
    ensure!(stasis["plan"]["damage"]["policy"] == "unresolved");
    Ok(())
}

#[test]
fn effects_and_animation_banks_remain_linked_to_the_selected_source() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    let mut value = input();
    let components = json!([
        {"owner":"80801007", "instance_class":"808025F7", "definition_class":"808025F8",
         "instance_offset":64, "definition_offset":256, "decoded_modifiers":false}
    ]);
    value["capture"]["traces"][0]["perks"][0]["states"][0]["effects"] = json!([
        {"offset":256, "class":"8080B7C6", "source_kind":1, "header":"0100000000000000",
         "native_kind":null, "entity":{"tag":"80801006", "components":components}},
        {"offset":280, "class":"8080FFFE", "source_kind":93, "header":"5d00000000000000",
         "native_kind":null, "future_effect_payload":{"source_element":6}}
    ]);
    value["capture"]["traces"][0]["perks"][0]["states"][0]["condition_groups"] = json!([
        {"offset":304, "key":801, "destination_key":800, "conditions":[
            {"offset":320, "class":"80803060", "source_kind":1, "header":"0000000000010000",
             "seconds":0.5, "native_kind":1, "native_class":"80803DF4",
             "native_node":{"kind":1,"bytes":"0x0100"}}
        ]}
    ]);
    value["capture"]["animations"] = json!([
        {"entity_tag":"80801006", "components":components, "skeletons":[],
         "clips":{"runtime_ready":false,
             "banks":[{"owner":"80801007", "bank":"80801008", "clips":["80801009"]}],
             "clips":{"80801009":{"runtime_ready":false, "conversion_error":"unsupported source event record"}}}}
    ]);
    let source = root.join("input.json");
    let output = root.join("plan.json");
    save(&source, &value)?;
    subclass::plan_file(&source, &output)?;
    let report: Value = serde_json::from_slice(&fs::read(&output)?)?;
    let ability = &report["plan"]["abilities"][0];
    let nodes = ability["effects"]["nodes"].as_array().unwrap();
    ensure!(nodes.len() == 3);
    ensure!(nodes[1]["record"]["future_effect_payload"]["source_element"] == 6);
    ensure!(nodes[1]["lowering"] == "unexamined");
    ensure!(nodes[2]["domain"] == "condition" && nodes[2]["lowering"] == "recorded");
    ensure!(nodes[2]["state_key"] == 800 && nodes[2]["transition_key"] == 801);
    let animation = &ability["animation"];
    ensure!(animation["runtime_ready"] == false);
    ensure!(animation["attachments"][0]["clips"]["banks"][0]["clips"][0] == "80801009");
    ensure!(
        animation["attachments"][0]["clips"]["clips"]["80801009"]["conversion_error"]
            == "unsupported source event record"
    );
    ensure!(
        animation["pending"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "root_motion")
    );
    ensure!(
        animation["pending"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "event_timing")
    );
    ensure!(report["plan"]["damage"]["native"] == "kinetic");
    ensure!(report["input"] == value);

    let mut broken = value.clone();
    broken["capture"]["animations"][0]["clips"]["banks"][0]["clips"][0] = json!("80801010");
    save(&source, &broken)?;
    ensure!(subclass::plan_file(&source, &root.join("broken-clip.json")).is_err());
    ensure!(!root.join("broken-clip.json").exists());
    broken = value.clone();
    broken["capture"]["animations"][0]["entity_tag"] = json!("80801011");
    save(&source, &broken)?;
    ensure!(subclass::plan_file(&source, &root.join("foreign-animation.json")).is_err());
    ensure!(!root.join("foreign-animation.json").exists());

    if let Some(folder) = std::env::var_os("PARHELION_SUBCLASS_TEST_OUTPUT") {
        // This test may run before the other artifact-producing test.
        let folder = std::path::PathBuf::from(folder).with_extension("effects");
        fs::create_dir(&folder)?;
        save(&folder.join("input.json"), &value)?;
        save(&folder.join("plan.json"), &report)?;
    } else {
        println!("{}", serde_json::to_string(&report)?);
    }
    Ok(())
}

#[test]
fn mounted_capture_requires_readable_packages_and_external_output() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    let packages = root.join("source/packages");
    fs::create_dir_all(&packages)?;
    let request = serde_json::from_value(input()["request"].clone())?;
    ensure!(subclass::inspect(&packages, &request, &root.join("empty")).is_err());
    ensure!(!root.join("empty").exists());
    ensure!(subclass::inspect(&root.join("missing"), &request, &root.join("unreadable")).is_err());
    ensure!(!root.join("unreadable").exists());
    ensure!(subclass::inspect(&packages, &request, &packages.join("research")).is_err());
    ensure!(!packages.join("research").exists());
    Ok(())
}

#[test]
fn innate_effects_and_non_perk_ability_grants_survive_replay() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    let mut value = input();
    let mut definition = [0u8; 28];
    definition[..4].copy_from_slice(&900u32.to_le_bytes());
    definition[4..8].copy_from_slice(&901u32.to_le_bytes());
    value["capture"]["ability_catalog"] = json!({
        "definition_tag":"80802000", "identity_tag":"80802001",
        "identities":[900], "rows":[hex::encode(definition)]
    });
    let mut grant = [0u8; 40];
    grant[4..8].copy_from_slice(&902u32.to_le_bytes());
    grant[8] = 3;
    grant[9] = 1;
    grant[16] = 1;
    let entity = json!({"tag":"80802002", "components":[]});
    value["capture"]["traces"][0]["perks"] = json!([]);
    value["capture"]["traces"][0]["ability_grants"] = json!([{
        "source_offset":512, "source_record":hex::encode(grant),
        "assignment":{"key":901,"target":"80802002"}, "entity":entity
    }]);
    value["capture"]["traces"][1]["ability_grants"] = json!([]);
    let mut innate = input()["capture"]["traces"][0].clone();
    innate["plug_hash"] = json!(100);
    innate["item_tag"] = json!("80801000");
    innate["ability_grants"] = json!([]);
    innate["perks"][0]["states"][0]["effects"] = json!([
        {"offset":556,"class":"80803126","source_kind":6,"header":"06010600",
         "source_damage_type":6,"native_kind":null},
        {"offset":600,"class":"80803128","source_kind":3,"header":"0300000000000000",
         "entity":{"tag":"80802003","components":[]},"native_kind":null}
    ]);
    value["capture"]["innate_trace"] = innate;
    let source = root.join("input.json");
    let output = root.join("plan.json");
    save(&source, &value)?;
    subclass::plan_file(&source, &output)?;
    let bytes = fs::read(&output)?;
    let report: Value = serde_json::from_slice(&bytes)?;
    let active = &report["plan"]["abilities"][0];
    ensure!(active["controllers"] == json!([]));
    let granted = &active["grants"][0];
    ensure!(granted["source_index"] == 0 && granted["source_identity"] == 900);
    ensure!(granted["property_key"] == 902 && granted["assignment_key"] == 901);
    ensure!(granted["slot"] == 3 && granted["flag_byte"] == 1);
    ensure!(granted["evidence"]["source_record"] == hex::encode(grant));
    ensure!(active["animation"]["uninspected_attachments"] == json!(["80802002"]));
    let innate = &report["plan"]["innate"];
    ensure!(innate["effects"]["nodes"][0]["record"]["source_damage_type"] == 6);
    ensure!(innate["animation"]["uninspected_attachments"] == json!(["80802003"]));
    ensure!(report["plan"]["damage"]["native"] == "kinetic");
    ensure!(report["plan"]["damage"]["source_providers_resolved"] == false);
    ensure!(report["input"] == value && report["installable"] == false);
    save(&source, &report["input"])?;
    subclass::plan_file(&source, &root.join("replayed.json"))?;
    ensure!(fs::read(root.join("replayed.json"))? == bytes);

    let mut invalid = Vec::new();
    let mut changed = value.clone();
    changed["capture"]["innate_trace"]["plug_hash"] = json!(200);
    invalid.push(changed);
    let mut changed = value.clone();
    changed["capture"]["ability_catalog"]["identities"][0] = json!(999);
    invalid.push(changed);
    let mut changed = value.clone();
    grant[..4].copy_from_slice(&1u32.to_le_bytes());
    changed["capture"]["traces"][0]["ability_grants"][0]["source_record"] =
        json!(hex::encode(grant));
    invalid.push(changed);
    let mut changed = value.clone();
    changed["capture"]["traces"][0]["ability_grants"][0]["assignment"]["key"] = json!(999);
    invalid.push(changed);
    let mut changed = value.clone();
    changed["capture"]["traces"][0]["ability_grants"][0]["entity"]["tag"] = json!("80802004");
    invalid.push(changed);
    let mut changed = value.clone();
    changed["capture"]["traces"][0]["ability_grants"][0]["source_record"] = json!("00");
    invalid.push(changed);
    for (index, changed) in invalid.iter().enumerate() {
        save(&source, changed)?;
        let destination = root.join(format!("invalid-grant-{index}.json"));
        ensure!(subclass::plan_file(&source, &destination).is_err());
        ensure!(!destination.exists());
    }
    if let Some(folder) = std::env::var_os("PARHELION_SUBCLASS_TEST_OUTPUT") {
        let folder = std::path::PathBuf::from(folder).with_extension("grants");
        fs::create_dir(&folder)?;
        save(&folder.join("input.json"), &value)?;
        fs::write(folder.join("plan.json"), bytes)?;
    } else {
        println!("{}", serde_json::to_string(&report)?);
    }
    Ok(())
}
