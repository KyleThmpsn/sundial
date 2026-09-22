use super::*;
fn array(data: &mut [u8], descriptor: usize, header: usize, count: u64, class: u32) {
    put(data, descriptor, &count.to_le_bytes()).unwrap();
    put(
        data,
        descriptor + 8,
        &((header as i64) - (descriptor as i64 + 8)).to_le_bytes(),
    )
    .unwrap();
    put(data, header, &count.to_le_bytes()).unwrap();
    put(data, header + 8, &class.to_le_bytes()).unwrap();
}
fn fixture() -> (tempfile::TempDir, Payload, Value) {
    let dir = tempfile::tempdir().unwrap();
    for name in ["source/raw", "native/raw", "out"] {
        fs::create_dir_all(dir.path().join(name)).unwrap();
    }
    let mut modern = vec![0u8; 0x1A0];
    array(&mut modern, 16, 0xA0, 1, 0x80806EC5);
    array(&mut modern, 0xB0 + 32, 0x130, 2, 0x80806ECB);
    for stage in 1..24 {
        put(&mut modern, 0xB0 + 48 + stage * 2, &1u16.to_le_bytes()).unwrap();
    }
    put(&mut modern, 0xB0 + 48 + 48, &2u16.to_le_bytes()).unwrap();
    for (offset, tag) in [(0x140, 1u32), (0x164, 2)] {
        put(&mut modern, offset, &tag.to_le_bytes()).unwrap();
        put(&mut modern, offset + 4, &u16::MAX.to_le_bytes()).unwrap();
        put(&mut modern, offset + 6, &5u16.to_le_bytes()).unwrap();
        put(&mut modern, offset + 12, &3u32.to_le_bytes()).unwrap();
        put(&mut modern, offset + 24, &5u32.to_le_bytes()).unwrap();
    }
    let mut native = vec![0u8; 0x170];
    array(&mut native, 16, 0xA0, 1, 0x80807378);
    array(&mut native, 0xB0 + 24, 0x140, 1, 0x8080737E);
    put(&mut native, 0xB0, &10u32.to_le_bytes()).unwrap();
    put(&mut native, 0xB4, &11u32.to_le_bytes()).unwrap();
    for stage in 1..24 {
        put(&mut native, 0xB0 + 40 + stage * 2, &1u16.to_le_bytes()).unwrap();
    }
    put(&mut native, 0xB0 + 88, &137i16.to_le_bytes()).unwrap();
    put(&mut native, 0x150, &3u32.to_le_bytes()).unwrap();
    put(&mut native, 0x150 + 24, &5u16.to_le_bytes()).unwrap();
    fs::write(dir.path().join("native/raw/00000004.bin"), native).unwrap();
    for (tag, stride) in [(10, 8u16), (11, 20u16)] {
        let mut h = vec![0u8; 12];
        put(&mut h, 4, &stride.to_le_bytes()).unwrap();
        fs::write(dir.path().join(format!("native/raw/{tag:08X}.bin")), h).unwrap();
    }
    for (root, tag, bind) in [("source", 1, 1u32), ("source", 2, 6), ("native", 3, 1)] {
        let mut m = vec![0u8; 1024];
        put(&mut m, 8, &bind.to_le_bytes()).unwrap();
        fs::write(dir.path().join(format!("{root}/raw/{tag:08X}.bin")), m).unwrap();
    }
    (
        dir,
        Payload(modern),
        json!({"models":[{"model":"00000004"}]}),
    )
}
#[test]
fn carrier_rejects_matching_strides_with_an_incompatible_stage_layout() {
    let (dir, model, mut template) = fixture();
    let native = dir.path().join("native");
    let mut incompatible = fs::read(native.join("raw/00000004.bin")).unwrap();
    put(&mut incompatible, 0xB0 + 88, &141i16.to_le_bytes()).unwrap();
    fs::write(native.join("raw/00000005.bin"), incompatible).unwrap();
    template["models"] = json!([{"model":"00000005"},{"model":"00000004"}]);
    let result = choose_carrier(
        &dir.path().join("source"),
        &native,
        &model,
        0xB0,
        &template,
        (true, 20),
    )
    .unwrap();
    assert_eq!(result.0, 4);
    template["models"] = json!([{"model":"00000005"}]);
    assert!(
        choose_carrier(
            &dir.path().join("source"),
            &native,
            &model,
            0xB0,
            &template,
            (true, 20),
        )
        .is_err()
    );
}

#[test]
fn carrier_skips_effect_attachments_with_matching_vertex_strides() {
    let (dir, model, mut template) = fixture();
    let mut attachment = fs::read(dir.path().join("native/raw/00000004.bin")).unwrap();
    for stage in 1..=7 {
        put(&mut attachment, 0xB0 + 40 + stage * 2, &0u16.to_le_bytes()).unwrap();
    }
    fs::write(dir.path().join("native/raw/00000005.bin"), attachment).unwrap();
    template["models"] = json!([{"model":"00000005"},{"model":"00000004"}]);
    let report = map(
        &dir.path().join("source"),
        &dir.path().join("native"),
        &dir.path().join("out"),
        &model,
        0xB0,
        &template,
    )
    .unwrap();
    assert_eq!(report["native_carrier"], "00000004");
}

#[test]
fn a_searched_rendering_template_carries_a_model_the_donor_cannot() {
    // Edge Transit's later part needs material contracts no grenade launcher
    // donor has. A carrier found elsewhere in the packages supplies the draw
    // records without changing the gameplay or animation donor.
    let (dir, model, mut template) = fixture();
    let source = dir.path().join("source");
    let native = dir.path().join("native");
    fs::write(source.join("raw/00000020.bin"), &model.0).unwrap();
    let report = json!({"models":[{"model":"00000020"}]});
    let carrier = fs::read(native.join("raw/00000004.bin")).unwrap();
    fs::write(native.join("raw/00000006.bin"), carrier).unwrap();

    template["models"] = json!([]);
    assert_eq!(
        uncarried(&source, &native, &report, &template, 20).unwrap(),
        [(0x20, 0xB0)]
    );
    check_carriers(&source, &native, &report, &template, 20).unwrap_err();

    template["carrier_models"] = json!([{"model":"00000006","owner":"0000000A"}]);
    assert!(
        uncarried(&source, &native, &report, &template, 20)
            .unwrap()
            .is_empty()
    );
    check_carriers(&source, &native, &report, &template, 20).unwrap();
    assert_eq!(
        primary_carrier_owner(&source, &native, &report, &template, 20).unwrap(),
        10
    );
}

#[test]
fn preflight_checks_later_model_parts_without_emitting_assets() {
    let (dir, model, template) = fixture();
    let source = dir.path().join("source");
    let native = dir.path().join("native");
    fs::write(source.join("raw/00000020.bin"), &model.0).unwrap();
    let report = json!({"models":[{"model":"00000020"}]});
    check_carriers(&source, &native, &report, &template, 20).unwrap();
    let mut unsupported = model.clone();
    put(&mut unsupported.0, 0x140 + 24, &13u32.to_le_bytes()).unwrap();
    fs::write(source.join("raw/00000021.bin"), &unsupported.0).unwrap();
    let report = json!({"models":[{"model":"00000020"},{"model":"00000021"}]});
    let error = check_carriers(&source, &native, &report, &template, 20).unwrap_err();
    assert!(format!("{error:#}").contains("source model 00000021"));
    assert!(
        fs::read_dir(dir.path().join("out"))
            .unwrap()
            .next()
            .is_none()
    );
    assert!(
        map_plated_stride(
            &source,
            &native,
            &dir.path().join("out"),
            &unsupported,
            0xB0,
            &template,
            20
        )
        .is_err()
    );
}

#[test]
fn carrier_mapping_rebuilds_native_records_and_relocations() {
    let (dir, model, template) = fixture();
    let report = map(
        &dir.path().join("source"),
        &dir.path().join("native"),
        &dir.path().join("out"),
        &model,
        0xB0,
        &template,
    )
    .unwrap();
    assert_eq!(report["native_parts"], 1);
    assert_eq!(report["removed_compute_parts"].as_array().unwrap().len(), 1);
    let native = Payload(fs::read(dir.path().join("out/model.unlinked.bin")).unwrap());
    let mesh = native.array(16, 136, Some(0x80807378)).unwrap()[0];
    let part = native.array(mesh + 24, 32, Some(0x8080737E)).unwrap()[0];
    assert_eq!(native.u32(part).unwrap(), u32::MAX);
    assert_eq!(native.u32(part + 12).unwrap(), 3);
    assert_eq!(native.u16(mesh + 88).unwrap(), 137);
    for fixup in report["relocations"].as_array().unwrap() {
        assert_eq!(
            native
                .u32(fixup["offset"].as_u64().unwrap() as usize)
                .unwrap(),
            u32::MAX
        );
    }
}
#[test]
fn plated_mapping_omits_unsupported_effect_carriers_but_keeps_geometry() {
    let (dir, mut modern, template) = fixture();
    // Move the second draw from compute to depth prepass. It has no compatible
    // native material and is explicitly omitted by the plated path.
    for stage in 13..=24 {
        put(&mut modern.0, 0xB0 + 48 + stage * 2, &2u16.to_le_bytes()).unwrap();
    }
    assert!(
        map(
            &dir.path().join("source"),
            &dir.path().join("native"),
            &dir.path().join("out"),
            &modern,
            0xB0,
            &template
        )
        .is_err()
    );
    let report = map_plated_stride(
        &dir.path().join("source"),
        &dir.path().join("native"),
        &dir.path().join("out"),
        &modern,
        0xB0,
        &template,
        20,
    )
    .unwrap();
    assert_eq!(report["native_parts"], 1);
    let depth = report["stages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["stage"] == 12)
        .unwrap();
    assert_eq!(depth["source_range"], json!([1, 2]));
    assert_eq!(depth["native_range"], json!([1, 1]));
    assert_eq!(depth["native_layout"], -1);
}

#[test]
fn compute_only_geometry_is_rejected() {
    let (dir, mut model, template) = fixture();
    put(&mut model.0, 0x164 + 12, &4u32.to_le_bytes()).unwrap();
    assert!(
        map(
            &dir.path().join("source"),
            &dir.path().join("native"),
            &dir.path().join("out"),
            &model,
            0xB0,
            &template
        )
        .unwrap_err()
        .to_string()
        .contains("compute-only")
    );
}
#[test]
fn missing_material_carrier_is_rejected() {
    let (dir, model, template) = fixture();
    let path = dir.path().join("native/raw/00000003.bin");
    let mut material = fs::read(&path).unwrap();
    put(&mut material, 8, &2u32.to_le_bytes()).unwrap();
    fs::write(path, material).unwrap();
    assert!(
        map(
            &dir.path().join("source"),
            &dir.path().join("native"),
            &dir.path().join("out"),
            &model,
            0xB0,
            &template
        )
        .unwrap_err()
        .to_string()
        .contains("matches all retained material contracts")
    );
}

#[test]
fn body_slot_model_hosts_the_import_and_is_tried_first() {
    // Table order lists an attachment before the body. Parent bytes hold the
    // entity tag at offset 16.
    let parent = |entity: u32| {
        let mut bytes = vec![0u8; 24];
        bytes[16..20].copy_from_slice(&entity.to_le_bytes());
        hex::encode(bytes)
    };
    let template = json!({
        "parents": [
            {"assignment": "0C2D1AAE", "parent_bytes": parent(0x80BBA91A), "placement": {"selector": 3, "position": 0}},
            {"assignment": "AF70C637", "parent_bytes": parent(0x80BBA5DE), "placement": {"selector": 0, "position": 0}},
            {"assignment": "F820A68C", "parent_bytes": parent(0x80BBA64E), "placement": {"selector": 21, "position": 0}}
        ],
        "models": [
            {"entity": "80BBA91A", "owner": "80BBA918", "model": "80EF06CD"},
            {"entity": "80BBA5DE", "owner": "80BBA5DD", "model": "80EF04CB"},
            {"entity": "80BBA64E", "owner": "80BBA64D", "model": "80EF0554"}
        ],
        "carrier_models": [{"model": "80BA7148", "rendering_template_only": true}]
    });
    let body = body_host(&template).unwrap();
    assert_eq!(body["owner"], "80BBA5DD");
    let order = carrier_candidates(&template)
        .iter()
        .map(|m| m["model"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(order, ["80EF04CB", "80EF06CD", "80EF0554", "80BA7148"]);

    // A row with no slots displays its single directly.
    let single = json!({
        "parents": [{"assignment": "CFBE7264", "parent_bytes": parent(0x80EC2729), "placement": {"single": 0}}],
        "models": [{"entity": "80EC2729", "owner": "80EC2727", "model": "80EC2722"}]
    });
    assert_eq!(body_host(&single).unwrap()["owner"], "80EC2727");

    // Older extractions without placement keep table order and no body host.
    let mut legacy = template.clone();
    for p in legacy["parents"].as_array_mut().unwrap() {
        p.as_object_mut().unwrap().remove("placement");
    }
    assert!(body_host(&legacy).is_none());
    assert_eq!(carrier_candidates(&legacy)[0]["model"], "80EF06CD");

    // A body slot whose parent has no runtime entity cannot host anything.
    let mut empty = template.clone();
    empty["parents"][1]["parent_bytes"] = json!(parent(u32::MAX));
    assert!(body_host(&empty).is_none());

    // A searched rendering template is not one of the donor's models, so the
    // header block comes from the host model. A donor model keeps its own.
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("raw")).unwrap();
    let mut host = vec![0u8; 0xA0];
    host[0x30..0x50].copy_from_slice(&[7u8; 32]);
    fs::write(dir.path().join("raw/80EF04CB.bin"), host).unwrap();
    let header = host_header(dir.path(), &template, 0x80BA7148)
        .unwrap()
        .unwrap();
    assert_eq!(header.bytes::<32>(0x30).unwrap(), [7u8; 32]);
    assert!(
        host_header(dir.path(), &template, 0x80EF06CD)
            .unwrap()
            .is_none()
    );

    // When the body owner's channels cannot adapt, table order returns and any
    // searched rendering template is rehosted on the table-order model.
    let mut fallback = template.clone();
    fallback["carrier_models"][0]["owner"] = json!("80BBA5DD");
    fallback["carrier_models"][0]["entity"] = json!("80BBA5DE");
    use_table_order(&mut fallback, "nonvector allocation").unwrap();
    assert!(body_host(&fallback).is_none());
    assert_eq!(host_model(&fallback).unwrap()["owner"], "80BBA918");
    assert_eq!(fallback["carrier_models"][0]["owner"], "80BBA918");
    assert_eq!(carrier_candidates(&fallback)[0]["model"], "80EF06CD");
}
