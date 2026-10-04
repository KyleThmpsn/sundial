//! Opt-in modern package to native authored-program verification with shipped data as oracle.
#![cfg(feature = "d2-model-importer")]

use std::{env, fs, path::PathBuf};
use sundial::package_authoring::sandbox_perk::action::native::NodeKind as NativeNodeKind;

use parhelion_import::d2_mot::{gameplay::perks, payload::Payload, reader::Reader};
use serde_json::json;
use sha2::{Digest, Sha256};
use sundial::package_authoring::{
    PackageManager,
    entity::{weapon_component_binding_hashes, weapon_component_bindings},
    sandbox_perk::{
        action::{self, native::Graph},
        program::{
            self, Action, NativeAssetPatch, NativeAssetResourcePatch, NativeNode, Program, Trigger,
        },
    },
};

fn boxed(p: &Payload, at: usize) -> Result<&[u8], Box<dyn std::error::Error>> {
    let record = p.pointer(at + 8)?;
    p.0.get(record..record + 72)
        .ok_or_else(|| "Movement record extends beyond payload".into())
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, PARHELION_IMPORT_NATIVE_PACKAGES and PARHELION_IMPORT_PERK_OUTPUT"]
fn modern_movement_records_compile_as_native_private_perk_effects()
-> Result<(), Box<dyn std::error::Error>> {
    let configured = |key| {
        env::var_os(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("Set {key}"))
    };
    let output = configured("PARHELION_IMPORT_PERK_OUTPUT")?.join("native-movement");
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

    // These are independently shipped nodes of the same perk in the supported package eras.
    let modern_tome = source.tag(0x80C30D89, Some(0x8080B835))?;
    let native_tome = native.tag(0x80BB7412, Some(0x808040B5))?;
    let translated = perks::lower::host_record(&modern_tome, 856)?;
    let native_action = action::decode(&native_tome.0)?;
    let stock = native_action
        .effects()
        .find(|effect| effect.kind == 36)
        .ok_or("Stock movement effect")?;
    let stock_graph = Graph::read(&native_tome.0, stock.offset, 0x80803E19)?;
    let translated_graph = Graph::read(&translated.bytes, 0, translated.class)?;
    assert_eq!(
        translated_graph, stock_graph,
        "Modern record must translate into the independently shipped native allocation graph"
    );

    let eager = source.tag(0x80C30E09, Some(0x8080B835))?;
    let eager_translation = perks::lower::host_record(&eager, 0x578)?;
    let node = NativeNode {
        kind: eager_translation.kind,
        bytes: eager_translation.bytes.clone(),
    };
    let program = Program {
        name: "Imported Movement Record".into(),
        trigger: Trigger::Drawn,
        actions: vec![Action::Native { node }],
        ..Program::default()
    };
    let compiler = PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )?;
    let compiled = program::compile(&compiler, &program)?;
    let decoded = action::decode(&compiled.payload)?;
    let emitted = decoded
        .effects()
        .find(|effect| effect.kind == 36)
        .ok_or("Compiled movement effect")?;
    let native_payload = Payload(compiled.payload.clone());
    assert_eq!(
        boxed(&native_payload, emitted.offset)?,
        boxed(&eager, 0x578)?,
        "Source movement values and identity must survive the native compiler"
    );
    let graph = Graph::read(&compiled.payload, emitted.offset, 0x80803E19)?;
    graph.validate_node(NativeNodeKind::Effect(36))?;

    let mut truncated = eager.0.clone();
    truncated.truncate(eager.pointer(0x580)? + 71);
    assert!(perks::lower::host_record(&Payload(truncated), 0x578).is_err());
    let mut wrong_class = eager.0.clone();
    let record = eager.pointer(0x580)?;
    wrong_class[record - 4..record].copy_from_slice(&0x8080639Du32.to_le_bytes());
    assert!(perks::lower::host_record(&Payload(wrong_class), 0x578).is_err());

    fs::create_dir_all(&output)?;
    fs::write(output.join("native-action.bin"), &compiled.payload)?;
    fs::write(
        output.join("native-action.json"),
        serde_json::to_vec_pretty(&program)?,
    )?;
    source.finish()?;
    native.finish()?;
    fs::write(
        output.join("verified-movement.json"),
        serde_json::to_vec_pretty(&json!({
            "source_class": "808030F3", "native_class": "80803E19", "record_class": "8080692C",
            "stock_oracle": "Tome of Dawn", "source_record_preserved": true,
            "native_action_sha256": hex_digest(&compiled.payload),
            "process_access": false, "full_perk_installable": false,
            "remaining": ["Controller state routing", "Modifier consumers", "Private dependencies", "Gameplay verification"]
        }))?,
    )?;
    println!("Verified native movement artifact at {}", output.display());
    Ok(())
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, PARHELION_IMPORT_NATIVE_PACKAGES and PARHELION_IMPORT_PERK_OUTPUT"]
#[expect(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn modern_controller_routing_preserves_nested_and_multiple_transitions()
-> Result<(), Box<dyn std::error::Error>> {
    let configured = |key| {
        env::var_os(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("Set {key}"))
    };
    let output = configured("PARHELION_IMPORT_PERK_OUTPUT")?.join("controller-routing");
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
    let mut artifacts = Vec::new();
    for tag in [
        0x80C30E09, 0x80C306B3, 0x80CECFB8, 0x80C30682, 0x80C30BB2, 0x80A5AAF6,
    ] {
        let payload = source.tag(tag, Some(0x8080B835))?;
        let controller = perks::controller::read(&payload)?;
        if matches!(tag, 0x80C30E09 | 0x80C306B3) {
            assert_eq!(
                controller
                    .states
                    .iter()
                    .map(|s| s.event_mask)
                    .collect::<Vec<_>>(),
                [0x10000, 0x0002_0000_0024_0102, 2]
            );
            assert_eq!(
                controller
                    .states
                    .iter()
                    .map(|s| s.transitions[0].destination)
                    .collect::<Vec<_>>(),
                [0x178, 0x200, 0xF0]
            );
        }
        if tag == 0x80C30682 {
            // Valiant Charge's nested condition contributes kind 5 under kind 36.
            let condition = &controller.states[0].transitions[0].conditions[0];
            assert_eq!(condition.kind, 36);
            assert_eq!(condition.children[0].kind, 5);
            assert_eq!(controller.states[0].event_mask, (1 << 36) | (1 << 5));
        }
        if tag == 0x80A5AAF6 {
            // Five consecutive transition rows expose an incorrect single-row stride.
            assert_eq!(controller.states[0].transitions.len(), 5);
            assert_eq!(controller.states[0].transitions[1].offset, 0x240);
            assert_eq!(controller.states[0].transitions[1].destination, 0xE8);
            assert_eq!(controller.states[0].transitions[2].destination, 0x170);
            assert_eq!(controller.states[0].event_mask, (1 << 36) | (1 << 2));
        }
        if tag == 0x80CECFB8 {
            let stock = native.tag(0x80BBCB39, Some(0x808040B5))?;
            let stock_action = action::decode(&stock.0)?;
            assert_eq!(stock_action.groups[0].activation[0].kind, 20);
            assert_eq!(controller.states[0].transitions[0].conditions[0].kind, 21);
            // Timer and holster retain their dispatch kinds across package eras.
            assert_eq!(stock.u64(0x90)?, controller.states[1].event_mask);
        }
        artifacts.push(json!({"tag": format!("{tag:08X}"),
            "source_sha256": hex_digest(&payload.0), "controller": controller}));
    }
    source.finish()?;
    native.finish()?;
    fs::write(
        output.join("verified-routing.json"),
        serde_json::to_vec_pretty(&json!({
            "source_only": true, "full_perk_installable": false, "controllers": artifacts
        }))?,
    )?;
    Ok(())
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, PARHELION_IMPORT_NATIVE_PACKAGES and PARHELION_IMPORT_PERK_OUTPUT"]
fn modern_object_slot_conditions_compile_into_private_native_routing()
-> Result<(), Box<dyn std::error::Error>> {
    let configured = |key| {
        env::var_os(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("Set {key}"))
    };
    let output = configured("PARHELION_IMPORT_PERK_OUTPUT")?.join("native-object-slot");
    let mut source = Reader::new(
        &configured("PARHELION_IMPORT_MODERN_PACKAGES")?,
        &output.join("source"),
        true,
    )?;
    let native = Reader::new(
        &configured("PARHELION_IMPORT_NATIVE_PACKAGES")?,
        &output.join("native"),
        false,
    )?;
    let compiler = PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )?;
    let mut cases = Vec::new();
    for (tag, mask) in [(0x80C30E09, 4u8), (0x80C306B3, 4), (0x80C30BB2, 15)] {
        let payload = source.tag(tag, Some(0x8080B835))?;
        let controller = perks::controller::read(&payload)?;
        let condition = controller
            .states
            .iter()
            .flat_map(|state| &state.transitions)
            .flat_map(|transition| &transition.conditions)
            .find(|condition| condition.class == 0x8080BDCF)
            .ok_or("Source four-channel condition")?;
        assert_eq!(condition.kind, 49);
        assert_eq!(payload.u8(condition.offset + 8)?, mask);
        let lowered = perks::lower::object_slot_condition(&payload, condition.offset)?;
        assert_eq!((lowered.class, lowered.kind), (0x808029E1, 41));
        assert_eq!(lowered.bytes.len(), 40);
        assert_eq!(lowered.bytes[8], mask);
        assert_eq!(payload.u64(condition.offset + 16)?, 0x100);
        assert_eq!(payload.u64(condition.offset + 24)?, 0);
        assert_eq!(lowered.bytes[16..40], [0; 24]);

        let program = Program {
            name: format!("Translated Object Slot {tag:08X}"),
            trigger: Trigger::Drawn,
            duration_ms: 0,
            native_removal: Some(NativeNode {
                kind: lowered.kind,
                bytes: lowered.bytes,
            }),
            ..Program::default()
        };
        let compiled = program::compile(&compiler, &program)?;
        let decoded = action::decode(&compiled.payload)?;
        let removal = decoded.groups[0]
            .removal
            .first()
            .ok_or("Compiled native removal condition")?;
        assert_eq!((removal.class, removal.kind), (0x808029E1, 41));
        assert_eq!(removal.native[8], mask);
        assert_eq!(
            u64::from_le_bytes(compiled.payload[0x90..0x98].try_into()?),
            1u64 << 41
        );
        Graph::read(&compiled.payload, removal.offset, removal.class)?
            .validate_node(NativeNodeKind::Condition(41))?;
        cases.push(json!({
            "source_tag": format!("{tag:08X}"),
            "source_sha256": hex_digest(&payload.0),
            "source_kind": 49,
            "native_kind": 41,
            "channel_mask": mask,
            "native_action_sha256": hex_digest(&compiled.payload)
        }));
        fs::create_dir_all(&output)?;
        fs::write(
            output.join(format!("{tag:08X}-native-action.bin")),
            compiled.payload,
        )?;
    }
    let eager = source.tag(0x80C30E09, Some(0x8080B835))?;
    let mut invalid_mask = eager.0.clone();
    // The Eager condition is located through its controller rather than a fixed offset.
    let offset = perks::controller::read(&eager)?
        .states
        .iter()
        .flat_map(|state| &state.transitions)
        .flat_map(|transition| &transition.conditions)
        .find(|condition| condition.class == 0x8080BDCF)
        .ok_or("Eager object-slot condition")?
        .offset;
    invalid_mask[offset + 8] = 0x80;
    assert!(perks::lower::object_slot_condition(&Payload(invalid_mask), offset).is_err());
    let mut nonempty_filter = eager.0.clone();
    nonempty_filter[offset + 24..offset + 32].copy_from_slice(&1u64.to_le_bytes());
    assert!(perks::lower::object_slot_condition(&Payload(nonempty_filter), offset).is_err());
    source.finish()?;
    native.finish()?;
    fs::create_dir_all(&output)?;
    fs::write(
        output.join("verified-object-slot.json"),
        serde_json::to_vec_pretty(&json!({
            "cases": cases,
            "native_predicate_rva": "0107E270",
            "modern_predicate_rva": "018A1D00",
            "process_access": false,
            "full_perk_installable": false,
            "event_producer_equivalence_proven": false
        }))?,
    )?;
    Ok(())
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, PARHELION_IMPORT_NATIVE_PACKAGES and PARHELION_IMPORT_PERK_OUTPUT"]
fn modern_ability_condition_matches_shipped_native_pair_and_eager_routing()
-> Result<(), Box<dyn std::error::Error>> {
    let configured = |key| {
        env::var_os(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("Set {key}"))
    };
    let output = configured("PARHELION_IMPORT_PERK_OUTPUT")?.join("native-ability-condition");
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
    let source_pair = source.tag(0x80C2F8CE, Some(0x8080B835))?;
    // Vengeance's auxiliary list contains another source class. The independently
    // paired ability record is checked directly without accepting that class.
    let pair_condition = 0x748;
    let lowered_pair = perks::lower::ability_condition(&source_pair, pair_condition)?;
    let native_pair = native.tag(0x80BBC9B3, Some(0x808040B5))?;
    let stock = action::decode(&native_pair.0)?;
    let shipped = stock
        .groups
        .iter()
        .flat_map(|group| {
            group
                .activation
                .iter()
                .chain(&group.removal)
                .chain(&group.rearm)
        })
        .find(|condition| condition.kind == 8)
        .ok_or("Vengeance native ability condition")?;
    assert_eq!((lowered_pair.class, lowered_pair.kind), (0x80803E01, 8));
    assert_eq!(lowered_pair.bytes, shipped.native);

    let eager = source.tag(0x80C30E09, Some(0x8080B835))?;
    let eager_condition = perks::controller::read(&eager)?
        .states
        .iter()
        .flat_map(|state| &state.transitions)
        .flat_map(|transition| &transition.conditions)
        .find(|condition| condition.class == 0x808030BE)
        .ok_or("Eager source ability condition")?
        .offset;
    let translated = perks::lower::ability_condition(&eager, eager_condition)?;
    assert_eq!(translated.bytes[8], 2);
    let compiler = PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )?;
    let program = Program {
        name: "Translated Ability Condition".into(),
        trigger: Trigger::Drawn,
        duration_ms: 0,
        native_removal: Some(NativeNode {
            kind: translated.kind,
            bytes: translated.bytes,
        }),
        ..Program::default()
    };
    let compiled = program::compile(&compiler, &program)?;
    let decoded = action::decode(&compiled.payload)?;
    assert_eq!(decoded.groups[0].removal[0].kind, 8);
    assert_eq!(decoded.groups[0].removal[0].native[8], 2);
    assert_eq!(
        u64::from_le_bytes(compiled.payload[0x90..0x98].try_into()?),
        1 << 8
    );
    fs::create_dir_all(&output)?;
    fs::write(output.join("eager-native-action.bin"), &compiled.payload)?;
    fs::write(
        output.join("verified-ability-condition.json"),
        serde_json::to_vec_pretty(&json!({
            "source_pair": "80C2F8CE",
            "native_pair": "80BBC9B3",
            "source_pair_sha256": hex_digest(&source_pair.0),
            "native_pair_sha256": hex_digest(&native_pair.0),
            "eager_source_sha256": hex_digest(&eager.0),
            "eager_action_sha256": hex_digest(&compiled.payload),
            "full_perk_installable": false
        }))?,
    )?;
    source.finish()?;
    native.finish()?;
    Ok(())
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, PARHELION_IMPORT_NATIVE_PACKAGES and PARHELION_IMPORT_PERK_OUTPUT"]
fn modern_timer_condition_matches_shipped_native_pair_and_eager_duration()
-> Result<(), Box<dyn std::error::Error>> {
    let configured = |key| {
        env::var_os(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("Set {key}"))
    };
    let output = configured("PARHELION_IMPORT_PERK_OUTPUT")?.join("native-timer-condition");
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
    let en_garde = source.tag(0x80CECFB8, Some(0x8080B835))?;
    let source_timer = perks::controller::read(&en_garde)?
        .states
        .iter()
        .flat_map(|state| &state.transitions)
        .flat_map(|transition| &transition.conditions)
        .find(|condition| condition.class == 0x80803060)
        .ok_or("En Garde source timer")?
        .offset;
    let lowered_pair = perks::lower::timer_condition(&en_garde, source_timer)?;
    let stock = native.tag(0x80BBCB39, Some(0x808040B5))?;
    let decoded_stock = action::decode(&stock.0)?;
    let native_timer = decoded_stock
        .groups
        .iter()
        .flat_map(|group| {
            group
                .activation
                .iter()
                .chain(&group.removal)
                .chain(&group.rearm)
        })
        .find(|condition| condition.kind == 1 && condition.native[8..12] == 1f32.to_le_bytes())
        .ok_or("En Garde native timer")?;
    assert_eq!((lowered_pair.class, lowered_pair.kind), (0x80803DCD, 1));
    assert_eq!(lowered_pair.bytes, native_timer.native);

    let eager = source.tag(0x80C30E09, Some(0x8080B835))?;
    let eager_timer = perks::controller::read(&eager)?
        .states
        .iter()
        .flat_map(|state| &state.transitions)
        .flat_map(|transition| &transition.conditions)
        .find(|condition| {
            condition.class == 0x80803060
                && eager.u32(condition.offset + 8).ok() == Some(3f32.to_bits())
        })
        .ok_or("Eager three-second timer")?
        .offset;
    let lowered = perks::lower::timer_condition(&eager, eager_timer)?;
    assert_eq!(lowered.bytes[8..12], 3f32.to_le_bytes());
    let compiler = PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )?;
    let program = Program {
        name: "Translated Timer Condition".into(),
        trigger: Trigger::Drawn,
        duration_ms: 0,
        native_removal: Some(NativeNode {
            kind: lowered.kind,
            bytes: lowered.bytes,
        }),
        ..Program::default()
    };
    let compiled = program::compile(&compiler, &program)?;
    let decoded = action::decode(&compiled.payload)?;
    assert_eq!(
        decoded.groups[0].removal[0].native[8..12],
        3f32.to_le_bytes()
    );
    fs::create_dir_all(&output)?;
    fs::write(output.join("eager-native-action.bin"), &compiled.payload)?;
    fs::write(
        output.join("verified-timer-condition.json"),
        serde_json::to_vec_pretty(&json!({
            "source_pair": "80CECFB8",
            "native_pair": "80BBCB39",
            "source_pair_sha256": hex_digest(&en_garde.0),
            "native_pair_sha256": hex_digest(&stock.0),
            "eager_source_sha256": hex_digest(&eager.0),
            "eager_action_sha256": hex_digest(&compiled.payload),
            "full_perk_installable": false
        }))?,
    )?;
    source.finish()?;
    native.finish()?;
    Ok(())
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, PARHELION_IMPORT_NATIVE_PACKAGES and PARHELION_IMPORT_PERK_OUTPUT"]
fn modern_weapon_swap_condition_relocates_shared_label_dependency()
-> Result<(), Box<dyn std::error::Error>> {
    let configured = |key| {
        env::var_os(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("Set {key}"))
    };
    let output = configured("PARHELION_IMPORT_PERK_OUTPUT")?.join("native-weapon-swap");
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
    let sprint = source.tag(0x80CECE71, Some(0x8080B835))?;
    let source_condition = perks::controller::read(&sprint)?
        .states
        .iter()
        .flat_map(|state| &state.transitions)
        .flat_map(|transition| &transition.conditions)
        .find(|condition| condition.class == 0x80803086)
        .ok_or("Sprint Grip source weapon-swap condition")?
        .offset;
    let translated_pair = perks::lower::weapon_swap_condition(&sprint, source_condition)?;
    let stock = native.tag(0x80BC2A95, Some(0x808040B5))?;
    let decoded = action::decode(&stock.0)?;
    let native_condition = decoded
        .groups
        .iter()
        .flat_map(|group| {
            group
                .activation
                .iter()
                .chain(&group.removal)
                .chain(&group.rearm)
        })
        .find(|condition| condition.kind == 18)
        .ok_or("Sprint Grip native weapon-swap condition")?;
    assert_eq!(
        (translated_pair.class, translated_pair.kind),
        (0x80803DDD, 18)
    );
    assert_eq!(
        Graph::read(&translated_pair.bytes, 0, translated_pair.class)?,
        Graph::read(&stock.0, native_condition.offset, native_condition.class)?
    );

    let eager = source.tag(0x80C30E09, Some(0x8080B835))?;
    let eager_condition = perks::controller::read(&eager)?
        .states
        .iter()
        .flat_map(|state| &state.transitions)
        .flat_map(|transition| &transition.conditions)
        .find(|condition| condition.class == 0x80803086)
        .ok_or("Eager source weapon-swap condition")?
        .offset;
    let translated = perks::lower::weapon_swap_condition(&eager, eager_condition)?;
    assert_eq!(translated.bytes[8], 1);
    let compiler = PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )?;
    let program = Program {
        name: "Translated Weapon Swap Condition".into(),
        trigger: Trigger::Drawn,
        duration_ms: 0,
        native_removal: Some(NativeNode {
            kind: translated.kind,
            bytes: translated.bytes,
        }),
        ..Program::default()
    };
    let compiled = program::compile(&compiler, &program)?;
    let decoded = action::decode(&compiled.payload)?;
    assert_eq!(decoded.groups[0].removal[0].kind, 18);
    assert_eq!(decoded.groups[0].removal[0].native[8], 1);
    assert_eq!(
        u64::from_le_bytes(compiled.payload[0x90..0x98].try_into()?),
        1 << 18
    );
    fs::create_dir_all(&output)?;
    fs::write(output.join("eager-native-action.bin"), &compiled.payload)?;
    fs::write(
        output.join("verified-weapon-swap.json"),
        serde_json::to_vec_pretty(&json!({
            "source_pair": "80CECE71",
            "native_pair": "80BC2A95",
            "source_pair_sha256": hex_digest(&sprint.0),
            "native_pair_sha256": hex_digest(&stock.0),
            "eager_source_sha256": hex_digest(&eager.0),
            "eager_action_sha256": hex_digest(&compiled.payload),
            "full_perk_installable": false
        }))?,
    )?;
    source.finish()?;
    native.finish()?;
    Ok(())
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, PARHELION_IMPORT_NATIVE_PACKAGES and PARHELION_IMPORT_PERK_OUTPUT"]
fn eager_active_transitions_compile_as_one_private_or_group()
-> Result<(), Box<dyn std::error::Error>> {
    let configured = |key| {
        env::var_os(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("Set {key}"))
    };
    let output = configured("PARHELION_IMPORT_PERK_OUTPUT")?.join("native-eager-or-group");
    let mut source = Reader::new(
        &configured("PARHELION_IMPORT_MODERN_PACKAGES")?,
        &output.join("source"),
        true,
    )?;
    let native = Reader::new(
        &configured("PARHELION_IMPORT_NATIVE_PACKAGES")?,
        &output.join("native"),
        false,
    )?;
    let compiler = PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )?;
    let mut cases = Vec::new();
    for tag in [0x80C30E09, 0x80C306B3] {
        let payload = source.tag(tag, Some(0x8080B835))?;
        let controller = perks::controller::read(&payload)?;
        let active = controller
            .states
            .iter()
            .flat_map(|state| &state.transitions)
            .find(|transition| {
                transition
                    .conditions
                    .iter()
                    .any(|condition| condition.class == 0x8080BDCF)
            })
            .ok_or("Eager active transition")?;
        let initial = controller.states.first().ok_or("Eager initial state")?;
        assert_eq!(initial.transitions[0].conditions[0].kind, 16);
        let cooldown = controller
            .states
            .iter()
            .flat_map(|state| &state.transitions)
            .find(|transition| {
                transition.destination == initial.offset
                    && transition.conditions.len() == 1
                    && transition.conditions[0].kind == 1
            })
            .ok_or("Eager rearm transition")?;
        let rearm = perks::lower::timer_condition(&payload, cooldown.conditions[0].offset)?;
        let rearm_seconds = f32::from_le_bytes(rearm.bytes[8..12].try_into()?);
        assert_eq!(rearm_seconds, 3.0);
        assert_eq!(
            active
                .conditions
                .iter()
                .map(|condition| condition.kind)
                .collect::<Vec<_>>(),
            [18, 1, 49, 8, 21]
        );
        let removals = active
            .conditions
            .iter()
            .map(|condition| perks::lower::condition(&payload, condition.offset))
            .collect::<Result<Vec<_>, _>>()?;
        let movement_marker = 0x808030F3u32.to_le_bytes();
        let movement = payload
            .0
            .windows(4)
            .position(|bytes| bytes == movement_marker)
            .ok_or("Eager movement-effect class")?
            + 4;
        let movement = perks::lower::host_record(&payload, movement)?;
        let program = Program {
            name: format!("Translated Eager Transition {tag:08X}"),
            trigger: Trigger::Native,
            native_trigger: Some(NativeNode::condition(16).ok_or("Native draw condition")?),
            duration_ms: 0,
            cooldown_ms: (rearm_seconds * 1000.0) as u32,
            native_removal: Some(NativeNode {
                kind: removals[0].kind,
                bytes: removals[0].bytes.clone(),
            }),
            alternative_removals: removals[1..]
                .iter()
                .map(|node| NativeNode {
                    kind: node.kind,
                    bytes: node.bytes.clone(),
                })
                .collect(),
            actions: vec![Action::Native {
                node: NativeNode {
                    kind: movement.kind,
                    bytes: movement.bytes,
                },
            }],
            ..Program::default()
        };
        let compiled = program::compile(&compiler, &program)?;
        let decoded = action::decode(&compiled.payload)?;
        assert_eq!(
            decoded.groups[0]
                .removal
                .iter()
                .map(|node| node.kind)
                .collect::<Vec<_>>(),
            [18, 1, 41, 8, 20]
        );
        assert_eq!(decoded.groups[0].rearm[0].kind, 1);
        assert_eq!(
            decoded.effects().map(|node| node.kind).collect::<Vec<_>>(),
            [36]
        );
        let expected_mask = (1u64 << 18) | (1 << 1) | (1 << 41) | (1 << 8) | (1 << 20);
        assert_eq!(
            u64::from_le_bytes(compiled.payload[0x90..0x98].try_into()?),
            expected_mask
        );
        let name = format!("{tag:08X}-native-action.bin");
        fs::create_dir_all(&output)?;
        fs::write(output.join(name), &compiled.payload)?;
        cases.push(json!({
            "source_tag": format!("{tag:08X}"),
            "source_sha256": hex_digest(&payload.0),
            "native_action_sha256": hex_digest(&compiled.payload),
            "native_removal_kinds": [18, 1, 41, 8, 20],
            "removal_mask": format!("{expected_mask:016X}")
        }));
    }
    source.finish()?;
    native.finish()?;
    fs::write(
        output.join("verified-or-group.json"),
        serde_json::to_vec_pretty(&json!({
            "cases": cases,
            "full_perk_installable": false,
            "attachments_translated": false,
            "gameplay_verified": false
        }))?,
    )?;
    Ok(())
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, PARHELION_IMPORT_NATIVE_PACKAGES and PARHELION_IMPORT_PERK_OUTPUT"]
fn modern_modifier_settings_match_independently_shipped_native_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let configured = |key| {
        env::var_os(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("Set {key}"))
    };
    let output = configured("PARHELION_IMPORT_PERK_OUTPUT")?.join("native-modifiers");
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
    let modern_rapid = source.tag(0x80CEDE31, Some(0x80809B06))?;
    let native_rapid = native.tag(0x80FEFAA2, Some(0x80809C36))?;
    let source_rows = modern_rapid.array(modern_rapid.pointer(24)? + 88, 112, Some(0x80802D33))?;
    let native_rows = native_rapid.array(native_rapid.pointer(24)? + 88, 88, Some(0x80803B06))?;
    assert!(!source_rows.is_empty());
    assert_eq!(source_rows.len(), native_rows.len());
    for (&from, &to) in source_rows.iter().zip(&native_rows) {
        let translated = perks::lower::modifier_settings(&modern_rapid, from, &native_rapid, to)?;
        assert_eq!(
            translated,
            native_rapid.bytes::<88>(to)?,
            "The modern row must lower to the independently shipped native settings"
        );
    }

    let reach = source.tag(0x80C378E0, Some(0x80809B06))?;
    let reach_rows = reach.array(reach.pointer(24)? + 88, 112, Some(0x80802D33))?;
    assert!(!reach_rows.is_empty());
    let translated =
        perks::lower::modifier_settings(&reach, reach_rows[0], &native_rapid, native_rows[0])?;
    assert_eq!(
        u32::from_le_bytes(translated[40..44].try_into()?),
        0x3FE66666
    );
    assert_eq!(translated[44], 0);
    assert_eq!(u16::from_le_bytes(translated[74..76].try_into()?), 3);
    assert_eq!(translated[76], 11);
    let angles = source.tag(0x80C378E1, Some(0x80809B06))?;
    let angle_rows = angles.array(angles.pointer(24)? + 88, 112, Some(0x80802D33))?;
    assert!(!angle_rows.is_empty());
    for from in angle_rows {
        assert!(
            perks::lower::modifier_settings(&angles, from, &native_rapid, native_rows[0]).is_err(),
            "Unavailable native angular inputs must block translation"
        );
    }
    fs::create_dir_all(&output)?;
    fs::write(output.join("native-reach-settings.bin"), translated)?;
    source.finish()?;
    native.finish()?;
    fs::write(
        output.join("verified-modifiers.json"),
        serde_json::to_vec_pretty(&json!({
            "stock_oracle": "Rapid Hit", "source_settings_class": "80802D33",
            "native_settings_class": "80803B06", "native_reach_input": 3,
            "native_component": 11, "full_perk_installable": false,
            "native_envelope_used_only_for_settings_verification": true,
            "remaining": ["Private dependency allocation", "Angular consumers", "Controller state routing", "Gameplay verification"]
        }))?,
    )?;
    Ok(())
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, PARHELION_IMPORT_NATIVE_PACKAGES and PARHELION_IMPORT_PERK_OUTPUT"]
fn modern_dynamic_attachments_keep_their_equations_in_native_programs()
-> Result<(), Box<dyn std::error::Error>> {
    let configured = |key| {
        env::var_os(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("Set {key}"))
    };
    let output = configured("PARHELION_IMPORT_PERK_OUTPUT")?.join("native-values");
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
    let harmonic = source.tag(0x80CECE78, Some(0x8080B835))?;
    let stock = native.tag(0x80BBC7FF, Some(0x808040B5))?;
    let stock_action = action::decode(&stock.0)?;
    let stock_node = stock_action
        .effects()
        .find(|e| e.kind == 2)
        .ok_or("Stock dynamic attachment")?;
    let entity = u32::from_le_bytes(stock_node.native[16..20].try_into()?);
    let translated = perks::lower::dynamic_entity(&harmonic, 704, entity)?;
    let converted_graph = Graph::read(&translated.bytes, 0, translated.class)?;
    let shipped_graph = Graph::read(&stock.0, stock_node.offset, translated.class)?;
    assert_eq!(
        converted_graph, shipped_graph,
        "The shared source equation must produce the independently shipped native graph"
    );

    let eager = source.tag(0x80C30E09, Some(0x8080B835))?;
    let lowered = perks::lower::dynamic_entity(&eager, 0x3E0, entity)?;
    let graph = Graph::read(&lowered.bytes, 0, lowered.class)?;
    let equation = action::native::value::Program::read(&graph, 0, 32)?;
    for input in [0.0, 0.5, 1.0, 3.0] {
        let expected = 1.0 - input * f32::from_bits(0x3E4CCCCD);
        assert_eq!(equation.evaluate(input), Some(expected));
    }
    let program = Program {
        name: "Imported Dynamic Attachment".into(),
        trigger: Trigger::Drawn,
        actions: vec![Action::Native {
            node: NativeNode {
                kind: lowered.kind,
                bytes: lowered.bytes,
            },
        }],
        ..Program::default()
    };
    let compiler = PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )?;
    let compiled = program::compile(&compiler, &program)?;
    let decoded = action::decode(&compiled.payload)?;
    let emitted = decoded
        .effects()
        .find(|e| e.kind == 2)
        .ok_or("Compiled dynamic attachment")?;
    let emitted_graph = Graph::read(&compiled.payload, emitted.offset, lowered.class)?;
    emitted_graph.validate_node(NativeNodeKind::Effect(2))?;
    let emitted_equation = action::native::value::Program::read(&emitted_graph, 0, 32)?;
    assert_eq!(emitted_equation, equation);

    let mut extra = eager.0.clone();
    extra[0x3E0 + 88..0x3E0 + 96].copy_from_slice(&1u64.to_le_bytes());
    assert!(perks::lower::dynamic_entity(&Payload(extra), 0x3E0, entity).is_err());
    let mut unknown_input = eager.0.clone();
    unknown_input[0x3E0 + 104] = 7;
    assert!(perks::lower::dynamic_entity(&Payload(unknown_input), 0x3E0, entity).is_err());

    fs::create_dir_all(&output)?;
    fs::write(output.join("native-action.bin"), &compiled.payload)?;
    fs::write(
        output.join("native-action.json"),
        serde_json::to_vec_pretty(&program)?,
    )?;
    source.finish()?;
    native.finish()?;
    fs::write(
        output.join("verified-values.json"),
        serde_json::to_vec_pretty(&json!({
            "stock_oracle": "Harmonic Laser", "source_program": "1 - input * 0.2",
            "native_action_sha256": hex_digest(&compiled.payload), "source_constants_preserved": true,
            "full_perk_installable": false, "oracle_entity_used_only_for_expression_verification": true,
            "remaining": ["Native source attachment conversion", "Controller state routing", "Angular consumers", "Gameplay verification"]
        }))?,
    )?;
    Ok(())
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, PARHELION_IMPORT_NATIVE_PACKAGES and PARHELION_IMPORT_PERK_OUTPUT"]
fn modern_lunge_modifier_compiles_with_a_scoped_native_graph_patch()
-> Result<(), Box<dyn std::error::Error>> {
    let configured = |key| {
        env::var_os(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("Set {key}"))
    };
    let output = configured("PARHELION_IMPORT_PERK_OUTPUT")?.join("native-lunge-graph");
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
    let eager = source.tag(0x80C30E09, Some(0x8080B835))?;
    let modern_settings = source.tag(0x80C378E0, Some(0x80809B06))?;
    let native_settings = native.tag(0x8162C919, None)?;
    let native_entity = native.tag(0x8162C91A, None)?;
    let source_rows =
        modern_settings.array(modern_settings.pointer(24)? + 88, 112, Some(0x80802D33))?;
    let native_rows =
        native_settings.array(native_settings.pointer(24)? + 88, 88, Some(0x80803B06))?;
    assert!(!source_rows.is_empty());
    assert!(!native_rows.is_empty());
    let translated = perks::lower::modifier_settings(
        &modern_settings,
        source_rows[0],
        &native_settings,
        native_rows[0],
    )?;
    let binding = weapon_component_binding_hashes(&native_entity.0)?
        .into_iter()
        .map(|hash| weapon_component_bindings(&native_entity.0, hash))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .find(|binding| {
            binding.owner_tag == 0x8162C919
                && usize::try_from(binding.resource_offset)
                    .ok()
                    .is_some_and(|start| start <= native_rows[0])
        })
        .ok_or("Native lunge modifier owner binding")?;
    let relative = native_rows[0] - usize::try_from(binding.resource_offset)?;
    let expected = native_settings.bytes::<88>(native_rows[0])?;
    assert_ne!(translated, expected);
    let patch = NativeAssetResourcePatch {
        binding_hash: binding.binding_hash,
        resource_index: u16::try_from(binding.resource_index)?,
        offset: u32::try_from(relative)?,
        expected: expected.to_vec(),
        bytes: translated.to_vec(),
        imported_particle: None,
    };
    let lunge = perks::lower::dynamic_entity(&eager, 0x3E0, 0x8162C91A)?;
    let program = Program {
        name: "Translated Lunge Dependency".into(),
        trigger: Trigger::Drawn,
        actions: vec![Action::Native {
            node: NativeNode {
                kind: lunge.kind,
                bytes: lunge.bytes,
            },
        }],
        native_asset_patches: vec![NativeAssetPatch {
            action_index: 0,
            source_graph: 0x8162C91A,
            appends: Vec::new(),
            remove_owners: Vec::new(),
            patches: vec![patch],
        }],
        ..Program::default()
    };
    let compiler = PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )?;
    let compiled = program::compile(&compiler, &program)?;
    assert!(compiled.asset_offsets.is_empty());
    let offset = compiled.graph_offsets[0].ok_or("Native lunge graph offset")?;
    assert_eq!(
        u32::from_le_bytes(compiled.payload[offset..offset + 4].try_into()?),
        0x8162C91A
    );
    fs::create_dir_all(&output)?;
    fs::write(
        output.join("native-action-before-private-rebind.bin"),
        &compiled.payload,
    )?;
    fs::write(
        output.join("native-action.json"),
        serde_json::to_vec_pretty(&program)?,
    )?;
    source.finish()?;
    native.finish()?;
    fs::write(
        output.join("verified-lunge-graph.json"),
        serde_json::to_vec_pretty(&json!({
            "source_attachment_class": "80803130",
            "native_attachment_class": format!("{:08X}", lunge.class),
            "source_settings_class": "80802D33",
            "native_settings_class": "80803B06",
            "native_entity_template": "8162C91A",
            "native_owner_template": "8162C919",
            "native_graph_offset": offset,
            "native_action_sha256": hex_digest(&compiled.payload),
            "graph_rebind_pending_stage": true,
            "gameplay_verified": false
        }))?,
    )?;
    Ok(())
}
