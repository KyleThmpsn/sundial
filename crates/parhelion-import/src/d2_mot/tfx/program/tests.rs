use super::*;

fn bindings() -> Bindings {
    Bindings {
        objects: BTreeMap::from([("A7A7FE43".into(), 5)]),
        globals: BTreeMap::from([(124, 110)]),
        constant_count: 4,
        output_count: 20,
        sampler_count: 2,
        ..Default::default()
    }
}
fn decode(s: &str) -> Vec<u8> {
    hex::decode(s.replace(' ', "")).unwrap()
}

#[test]
fn missing_globals_stop_donor_retries_but_objects_can_try_another_donor() {
    let mut global = lower(&decode("5dff 5200"), &bindings()).unwrap();
    assert!(crate::d2_mot::is_source_limit(
        &global.require_runtime_inputs().unwrap_err()
    ));
    global.evidence[0]["required_by_shader"] = json!(false);
    global.require_runtime_inputs().unwrap();
    let object = lower(&decode("5c12345678 5200"), &bindings()).unwrap();
    assert!(!crate::d2_mot::is_source_limit(
        &object.require_runtime_inputs().unwrap_err()
    ));
    lower(&decode("5d7c 5200"), &bindings())
        .unwrap()
        .require_runtime_inputs()
        .unwrap();
}

#[test]
fn absent_renderer_globals_use_explicit_authored_constants_only() {
    let mut b = bindings();
    b.global_defaults.insert(250, 3);
    let lowered = lower(&decode("5dfa 5200"), &b).unwrap();
    lowered.require_runtime_inputs().unwrap();
    assert_eq!(lowered.code, [0x34, 3, 0x43, 0]);
    b.globals.insert(250, 12);
    assert_eq!(
        lower(&decode("5dfa 5200"), &b).unwrap().code,
        [0x4E, 12, 0x43, 0]
    );
    b.globals.remove(&250);
    b.global_defaults.insert(250, 4);
    assert!(lower(&decode("5dfa 5200"), &b).is_err());
}

#[test]
fn frame_exposure_uses_the_native_scalar_without_replacing_unknown_fields() {
    let lowered = lower(&decode("4a0107 5200"), &bindings()).unwrap();
    lowered.require_runtime_inputs().unwrap();
    assert_eq!(lowered.code, [0x3C, 1, 7, 0x43, 0]);
    let unknown = lower(&decode("4a01ff 5200"), &bindings()).unwrap();
    assert!(crate::d2_mot::is_source_limit(
        &unknown.require_runtime_inputs().unwrap_err()
    ));
}

#[test]
fn channel_identity_and_sampler_binding() {
    let r = lower(&decode("5ca7a7fe43 5d7c 03 5211 5b01 5822"), &bindings()).unwrap();
    assert_eq!(hex::encode(r.code), "4d054e6e0343114c014922");
    assert_eq!(r.evidence[0]["translated"], true);
    assert_eq!(r.samplers, BTreeMap::from([(2, 1)]));
}

#[test]
fn vertex_samplers_keep_their_stage_and_require_explicit_vertex_context() {
    let code = decode("5b01 5841");
    assert!(lower(&code, &bindings()).is_err());
    let mut b = bindings();
    b.sampler_stage = Some(2);
    let lowered = lower(&code, &b).unwrap();
    assert_eq!(lowered.code, [0x4C, 1, 0x49, 0x41]);
    assert_eq!(lowered.samplers, BTreeMap::from([(1, 1)]));
    assert!(lower(&decode("5b01 5821"), &b).is_err());
    assert!(lower(&decode("5b02 5841"), &b).is_err());
}
#[test]
fn texture_dimensions_require_resource_indices() {
    let code = decode("6006ff4200025200");
    let mut b = Bindings {
        constant_count: 1,
        output_count: 1,
        sampler_count: 8,
        ..Default::default()
    };
    assert!(
        lower(&code, &b)
            .unwrap_err()
            .to_string()
            .contains("resource-table mapping")
    );
    b.textures.insert(6, 8);
    assert!(lower(&code, &b).is_err());
    b.textures.insert(6, 7);
    assert_eq!(
        hex::encode(lower(&code, &b).unwrap().code),
        "5107ff3400024300"
    );
}

#[test]
fn framebuffer_requires_both_verified_interface_and_binding_slot() {
    let code = decode("4d2d01562a");
    let mut b = bindings();
    assert!(lower(&code, &b).is_err());
    b.external_textures.insert([0x4D, 0x2D, 1], [0x3F, 0x2C, 1]);
    assert!(lower(&code, &b).is_err());
    b.texture_slots.insert(0x2A, 0x25);
    assert_eq!(lower(&code, &b).unwrap().code, decode("3f2c014725"));
    assert!(lower(&decode("4d2d02562a"), &b).is_err());
    assert!(lower(&decode("4d2d01562b"), &b).is_err());
}

#[test]
fn texture_tiling_requires_source_metadata_and_preserves_swizzle() {
    let mut b = bindings();
    b.constant_count = 3;
    b.output_count = 2;
    let code = decode("61011b52006201555201");
    assert!(lower(&code, &b).is_err());
    b.texture_metadata.insert([0x61, 1], 1);
    b.texture_metadata.insert([0x62, 1], 2);
    assert_eq!(
        lower(&code, &b).unwrap().code,
        decode("3401221b4300340222554301")
    );
    b.texture_metadata.insert([0x62, 1], 3);
    assert!(lower(&code, &b).is_err());
    assert!(relocate(&code, 0, &BTreeMap::new()).is_err());
}
#[test]
fn reused_temp_preserves_operand_order() {
    let r = lower(
        &decode("4200 4201 02 5500 5400 4202 03 5200 5400 5201"),
        &bindings(),
    )
    .unwrap();
    assert_eq!(hex::encode(r.code), "3400340102340203430034003401024301");
    assert_eq!(r.evidence.len(), 2);
}

#[test]
fn output_reads_capture_the_written_version_across_later_writes() {
    let code = decode("4200 5200 5100 5500 4201 5200 5400 5201 5100 5202");
    let result = lower(&code, &bindings()).unwrap();
    assert_eq!(
        result.code,
        decode("3400 4300 3401 4300 3400 4301 3401 4302")
    );
    assert!(result.evidence.iter().all(|e| e["translated"] == true));
}

#[test]
fn unresolved_output_reads_keep_dependencies_and_validate_bounds() {
    let result = lower(&decode("5c12345678 5200 5100 5201"), &bindings()).unwrap();
    assert!(result.code.is_empty());
    assert_eq!(
        result.evidence[1]["unresolved"],
        json!(["object channel 12345678"])
    );
    assert!(lower(&decode("51ff"), &bindings()).is_err());
}
#[test]
fn missing_channels_propagate_through_temps_and_saturation() {
    let r = lower(&decode("5c12345678 5500 5400 2a 5200"), &bindings()).unwrap();
    assert!(r.code.is_empty());
    assert_eq!(
        r.evidence[0]["unresolved"],
        json!(["object channel 12345678"])
    );
}
#[test]
fn settled_approximation_requires_bounded_output() {
    let mut b = bindings();
    b.settled.insert("48E16077".into(), 3);
    assert_eq!(
        lower(&decode("5c48e16077 5200"), &b).unwrap().evidence[0]["translated"],
        false
    );
    assert_eq!(
        hex::encode(lower(&decode("5c48e16077 2a 5200"), &b).unwrap().code),
        "3403234300"
    );
    b.settled.insert("48E16077".into(), 4);
    assert!(lower(&decode("5c48e16077 2a 5200"), &b).is_err());
}
#[test]
fn invalid_programs_fail_before_emission() {
    for code in [
        "4200 4303 5200",
        "5b02 5821",
        "4200 5214",
        "01",
        "5c1234",
        "5100 5201",
        "5400",
        "5b00 5801",
        "61",
    ] {
        assert!(lower(&decode(code), &bindings()).is_err(), "{code}");
    }
}
#[test]
fn channel_inputs_follow_model_order_and_require_reciprocal_links() {
    let mut p = vec![0; 0x400];
    p[16..24].copy_from_slice(&0x70u64.to_le_bytes());
    p[0x7C..0x80].copy_from_slice(&0x808072B8u32.to_le_bytes());
    p[0x1A0..0x1A8].copy_from_slice(&2u64.to_le_bytes());
    p[0x1A8..0x1B0].copy_from_slice(&0x58u64.to_le_bytes());
    p[0x200..0x208].copy_from_slice(&2u64.to_le_bytes());
    p[0x208..0x210].copy_from_slice(&0x80809788u64.to_le_bytes());
    for (input, link, hash) in [(0x210, 0x300, 0x0F148B54u32), (0x270, 0x328, 0x49FCE899)] {
        p[input..input + 4].copy_from_slice(&0x80EC2727u32.to_le_bytes());
        p[input + 4..input + 8].copy_from_slice(&0x80809789u32.to_le_bytes());
        p[input + 8..input + 16].copy_from_slice(&(link as u64).to_le_bytes());
        p[link..link + 4].copy_from_slice(&0x80EC2727u32.to_le_bytes());
        p[link + 4..link + 8].copy_from_slice(&0x80809788u32.to_le_bytes());
        p[link + 8..link + 16].copy_from_slice(&(input as u64).to_le_bytes());
        p[link + 24..link + 32].copy_from_slice(&0x808097C1u64.to_le_bytes());
        p[link + 32..link + 36].copy_from_slice(&hash.to_le_bytes());
    }
    assert_eq!(
        object_channel_map(&p).unwrap(),
        BTreeMap::from([("0F148B54".into(), 0), ("49FCE899".into(), 1)])
    );
    assert!(object_channel_map(&p[..0x348]).is_err());
    p[0x330..0x338].copy_from_slice(&0x210u64.to_le_bytes());
    assert!(object_channel_map(&p).is_err());
}
#[test]
fn scope_relocation_preserves_frame_inputs_and_checks_tables() {
    let outputs = BTreeMap::from([(5, 35)]);
    assert_eq!(
        hex::encode(relocate(&decode("4a0100 4302 5205"), 12, &outputs).unwrap()),
        "4a0100430e5223"
    );
    assert!(relocate(&decode("5206"), 0, &outputs).is_err());
    assert!(relocate(&decode("4201"), 255, &outputs).is_err());
    assert!(relocate(&decode("5b00"), 0, &outputs).is_err());
}
#[test]
fn json_bridge_checks_index_widths_and_reports_unresolved_inputs() {
    let mut v = json!({"mode":"lower", "data":"5c123456785200", "constant_count":1, "output_count":1, "sampler_count":0});
    assert_eq!(request(&v).unwrap()["evidence"][0]["translated"], false);
    v["object_channels"] = json!({"12345678":256});
    assert!(request(&v).is_err());
}

#[test]
fn unused_stack_expressions_do_not_discard_outputs_or_hide_underflow() {
    let bindings = Bindings {
        constant_count: 2,
        output_count: 1,
        ..Default::default()
    };
    let result = lower(&[0x42, 0, 0x42, 1, 0x52, 0], &bindings).unwrap();
    assert_eq!(result.code, [0x34, 1, 0x43, 0]);
    assert!(lower(&[0x42, 0], &bindings).unwrap().code.is_empty());
    assert_eq!(result.evidence.len(), 1);
    assert_eq!(result.evidence[0]["translated"], true);
    assert!(lower(&[0x42, 0, 0x52, 0, 0x52, 0], &bindings).is_err());
}
