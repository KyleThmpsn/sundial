//! Opt-in package verification for modern state predicates translated to native conditions.

use std::{env, fs, path::PathBuf};

use parhelion_import::d2_mot::{gameplay::perks, payload::Payload, reader::Reader};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn configured(key: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(env::var_os(key)
        .map(PathBuf::from)
        .ok_or_else(|| format!("Set {key}"))?)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES and PARHELION_IMPORT_PERK_OUTPUT"]
fn modern_perk_bridge_discovers_complete_source_dependencies()
-> Result<(), Box<dyn std::error::Error>> {
    let output = configured("PARHELION_IMPORT_PERK_OUTPUT")?.join("production-bridge");
    let mut reader = Reader::new(
        &configured("PARHELION_IMPORT_MODERN_PACKAGES")?,
        &output.join("source"),
        true,
    )?;
    let mut evidence = Vec::new();
    for (plug, expected_actions) in [
        (2_077_819_806, vec![0x80C3_0E09]),
        (1_618_208_178, vec![0x80C3_06B3, 0x80C3_0E09]),
    ] {
        let sources = perks::translate::extract_all(&mut reader, plug)?;
        assert_eq!(
            sources
                .iter()
                .map(|source| source.action_tag)
                .collect::<Vec<_>>(),
            expected_actions
        );
        for source in sources {
            assert_eq!(source.plug_hash, plug);
            assert_eq!(source.routing.cooldown_ms, 3000);
            assert_eq!(source.routing.effects.len(), 3);
            assert_eq!(source.routing.removals.len(), 5);
            assert_eq!(source.angular.scales.near_bits, 0.5f32.to_bits());
            assert_eq!(source.angular.scales.far_bits, 0.12f32.to_bits());
            let controller = perks::controller::read(&source.controller)?;
            let draw = controller
                .states
                .iter()
                .find(|state| {
                    state.transitions.iter().any(|transition| {
                        transition
                            .conditions
                            .iter()
                            .any(|condition| condition.kind == 16)
                    })
                })
                .ok_or("missing draw state")?;
            let mut wrong_destination = source.controller.0.clone();
            let pointer = draw.transitions[0].offset + 8;
            let delta = i64::try_from(draw.offset)? - i64::try_from(pointer)?;
            wrong_destination[pointer..pointer + 8].copy_from_slice(&delta.to_le_bytes());
            assert!(perks::translate::routing(&Payload(wrong_destination)).is_err());
            let mut unknown_effect = source.controller.0.clone();
            let class = source.routing.effects[0] - 4;
            unknown_effect[class..class + 4].copy_from_slice(&0x8080_FFFFu32.to_le_bytes());
            assert!(perks::translate::routing(&Payload(unknown_effect)).is_err());
            evidence.push(json!({
                "plug_hash": plug,
                "controller": format!("{:08X}", source.action_tag),
                "controller_sha256": digest(&source.controller.0),
                "effects": source.routing.effects,
                "removals": source.routing.removals,
                "lunge_owner": format!("{:08X}", source.lunge.owner_tag),
                "lunge_modifier_offset": source.lunge.modifier_offset,
                "angular_owner": format!("{:08X}", source.angular.owner_tag),
                "cooldown_ms": source.routing.cooldown_ms,
                "auxiliary": source.routing.auxiliary
            }));
        }
    }
    assert!(perks::translate::extract(&mut reader, 1_618_208_178).is_err());
    assert!(perks::translate::extract(&mut reader, 1_160_238_071).is_err());
    fs::create_dir_all(&output)?;
    fs::write(
        output.join("verified-source-bridge.json"),
        serde_json::to_vec_pretty(&json!({
            "sources": evidence,
            "wrong_destination_rejected": true,
            "unknown_effect_rejected": true,
            "unsupported_perk_rejected": true,
            "gameplay_verified": false
        }))?,
    )?;
    reader.finish()?;
    Ok(())
}

fn translated_state<'a>(trace: &'a Value, tag: &str) -> Result<&'a Value, String> {
    let action = trace["perks"]
        .as_array()
        .and_then(|perks| perks.iter().find(|perk| perk["tag"] == tag))
        .ok_or_else(|| format!("Missing source controller {tag}"))?;
    let active = action["states"]
        .as_array()
        .and_then(|states| states.iter().find(|state| state["key"] == 0x43F1_9C55_u64))
        .ok_or("Missing Eager active state")?;
    active["condition_groups"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| group["conditions"].as_array().into_iter().flatten())
        .find(|condition| condition["class"] == "80803061")
        .ok_or("Missing Eager source state-value condition".into())
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, PARHELION_IMPORT_NATIVE_PACKAGES and PARHELION_IMPORT_PERK_OUTPUT"]
fn modern_draw_condition_prefix_matches_two_native_pairs() -> Result<(), Box<dyn std::error::Error>>
{
    let output = configured("PARHELION_IMPORT_PERK_OUTPUT")?.join("draw-condition");
    let mut source = Reader::new(
        &configured("PARHELION_IMPORT_MODERN_PACKAGES")?,
        &output.join("source"),
        true,
    )?;
    let mut native = Reader::new(
        &configured("PARHELION_IMPORT_NATIVE_PACKAGES")?,
        &output.join("native"),
        false,
    )?;
    let snapshot = native.tag(0x80BB_CA85, Some(0x8080_40B5))?;
    let backup = native.tag(0x80BB_C8BB, Some(0x8080_40B5))?;
    let snapshot_prefix = &snapshot.0[0x100..0x118];
    let backup_prefix = &backup.0[0x190..0x1A8];
    assert_eq!(snapshot_prefix, backup_prefix);
    let expected = [
        0, 0, 0x80, 0x3F, 0xFF, 16, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    assert_eq!(snapshot_prefix, expected);
    let ordinary = source.tag(0x80C3_0E09, Some(0x8080_B835))?;
    let enhanced = source.tag(0x80C3_06B3, Some(0x8080_B835))?;
    let ordinary_prefix = perks::lower::draw_condition_prefix(&ordinary, 0x300)?;
    let enhanced_prefix = perks::lower::draw_condition_prefix(&enhanced, 0x300)?;
    assert_eq!(ordinary_prefix, expected);
    assert_eq!(enhanced_prefix, expected);
    let mut malformed = ordinary.0.clone();
    malformed[0x309] = 7;
    assert!(perks::lower::draw_condition_prefix(&Payload(malformed), 0x300).is_err());
    fs::create_dir_all(&output)?;
    fs::write(output.join("native-draw-prefix.bin"), ordinary_prefix)?;
    fs::write(
        output.join("verified-draw-condition.json"),
        serde_json::to_vec_pretty(&json!({
            "source_class": "808030AE",
            "native_class": "80803DF5",
            "source_actions": ["80C30E09", "80C306B3"],
            "native_controls": ["80BBCA85", "80BBC8BB"],
            "prefix_sha256": digest(&ordinary_prefix),
            "paired_prefixes_equal": true,
            "gameplay_verified": false
        }))?,
    )?;
    source.finish()?;
    native.finish()?;
    Ok(())
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, PARHELION_IMPORT_NATIVE_PACKAGES and PARHELION_IMPORT_PERK_OUTPUT"]
fn modern_state_value_condition_lowers_to_shipped_native_predicate()
-> Result<(), Box<dyn std::error::Error>> {
    let output = configured("PARHELION_IMPORT_PERK_OUTPUT")?.join("state-value");
    let mut source = Reader::new(
        &configured("PARHELION_IMPORT_MODERN_PACKAGES")?,
        &output.join("source"),
        true,
    )?;
    let mut native = Reader::new(
        &configured("PARHELION_IMPORT_NATIVE_PACKAGES")?,
        &output.join("native"),
        false,
    )?;

    let whirlwind = source.tag(0x80CECE9C, Some(0x8080B835))?;
    let shipped = native.tag(0x80BBC83D, Some(0x808040B5))?;
    let lowered = perks::lower::state_value_condition(&whirlwind, 0xDF8)?;
    assert_eq!(lowered.class, 0x80803DCE);
    assert_eq!(lowered.kind, 20);
    assert_eq!(lowered.bytes, shipped.0[0x9B0..0xAB0]);

    let eager = source.tag(0x80C30E09, Some(0x8080B835))?;
    let eager_node = perks::lower::state_value_condition(&eager, 0x758)?;
    assert_eq!(&eager_node.bytes[0xD4..0xD8], &0x6A1C_27EEu32.to_le_bytes());
    assert_eq!(
        &eager_node.bytes[0xD8..0xE0],
        &[0, 0, 0, 0, 0, 0, 0x80, 0x3F]
    );
    let different = eager_node
        .bytes
        .iter()
        .zip(lowered.bytes.iter())
        .enumerate()
        .filter_map(|(at, (left, right))| (left != right).then_some(at))
        .collect::<Vec<_>>();
    assert_eq!(different, [0xD4, 0xD5, 0xD6, 0xD7, 0xDA, 0xDB]);

    let mut malformed = eager.0.clone();
    malformed[0x758 + 0x110..0x758 + 0x114].copy_from_slice(&0x8080920Eu32.to_le_bytes());
    assert!(perks::lower::state_value_condition(&Payload(malformed), 0x758).is_err());

    let base_trace = perks::trace(&mut source, 2_077_819_806)?;
    let enhanced_trace = perks::trace(&mut source, 1_618_208_178)?;
    for (trace, tag) in [(&base_trace, "80C30E09"), (&enhanced_trace, "80C306B3")] {
        let condition = translated_state(trace, tag)?;
        assert_eq!(condition["native_kind"], 20);
        assert_eq!(condition["native_class"], "80803DCE");
        assert_eq!(condition["native_node"]["kind"], 20);
    }

    fs::create_dir_all(&output)?;
    fs::write(output.join("eager-native-node.bin"), &eager_node.bytes)?;
    fs::write(output.join("whirlwind-native-node.bin"), &lowered.bytes)?;
    fs::write(
        output.join("verified-state-value.json"),
        serde_json::to_vec_pretty(&json!({
            "source_class": "80803061",
            "native_class": "80803DCE",
            "shipped_pair": ["80CECE9C", "80BBC83D"],
            "shipped_native_bytes_equal": true,
            "eager_key": "6A1C27EE",
            "eager_minimum": 0.0,
            "eager_maximum": 1.0,
            "base_source_trace": base_trace,
            "enhanced_source_trace": enhanced_trace,
            "eager_node_sha256": digest(&eager_node.bytes),
            "whirlwind_node_sha256": digest(&lowered.bytes),
            "gameplay_verified": false,
            "successful_sword_event_covered": false
        }))?,
    )?;
    source.finish()?;
    native.finish()?;
    println!(
        "Verified native state-value artifacts at {}",
        output.display()
    );
    Ok(())
}
