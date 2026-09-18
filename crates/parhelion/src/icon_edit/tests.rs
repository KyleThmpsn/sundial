use super::*;

#[test]
fn icon_json_contains_only_changed_transformations() {
    assert_eq!(
        serde_json::to_value(WeaponIconEdit::default()).unwrap(),
        serde_json::json!({})
    );
    let edit = WeaponIconEdit {
        hue_shift_degrees: 45,
        flip_horizontal: true,
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(&edit).unwrap(),
        serde_json::json!({"hue_shift_degrees":45,"flip_horizontal":true})
    );
    assert_eq!(
        serde_json::from_value::<WeaponIconEdit>(serde_json::to_value(&edit).unwrap()).unwrap(),
        edit
    );
    let transparent = WeaponIconEdit {
        opacity_percent: 0,
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(transparent).unwrap(),
        serde_json::json!({"opacity_percent":0})
    );
    let old: WeaponIconEdit = serde_json::from_value(serde_json::json!({"rotation_quarter_turns":0,"flip_horizontal":false,"flip_vertical":false,"hue_shift_degrees":0,"saturation":0,"brightness":0,"contrast":0,"red_balance":0,"green_balance":0,"blue_balance":0,"invert":false,"opacity_percent":100})).unwrap();
    assert_eq!(old, WeaponIconEdit::default());
}

#[test]
fn validation_rejects_out_of_range_controls() {
    assert!(
        WeaponIconEdit {
            hue_shift_degrees: 181,
            ..WeaponIconEdit::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        WeaponIconEdit {
            brightness: -101,
            ..WeaponIconEdit::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        WeaponIconEdit {
            rotation_quarter_turns: 4,
            ..WeaponIconEdit::default()
        }
        .validate()
        .is_err()
    );
}

#[test]
fn color_transforms_preserve_exact_channels_and_operation_order() {
    for (edit, source, expected) in [
        (
            WeaponIconEdit {
                hue_shift_degrees: 120,
                ..Default::default()
            },
            vec![255, 0, 0, 7, 0, 255, 0, 9, 0, 0, 255, 11],
            vec![0, 255, 0, 7, 0, 0, 255, 9, 255, 0, 0, 11],
        ),
        (
            WeaponIconEdit {
                brightness: 50,
                invert: true,
                ..Default::default()
            },
            vec![10, 100, 200, 17],
            vec![122, 77, 27, 17],
        ),
        (
            WeaponIconEdit {
                brightness: -100,
                ..Default::default()
            },
            vec![10, 100, 200, 33],
            vec![0, 0, 0, 33],
        ),
        (
            WeaponIconEdit {
                opacity_percent: 25,
                ..Default::default()
            },
            vec![12, 34, 56, 200],
            vec![12, 34, 56, 50],
        ),
    ] {
        let mut pixels = source;
        edit.apply_to_rgba8(&mut pixels).unwrap();
        assert_eq!(pixels, expected, "{edit:?}");
    }
}

#[test]
fn color_operations_preserve_alpha_at_full_opacity() {
    let mut pixels = [12, 34, 56, 0, 78, 90, 123, 127, 210, 4, 99, 255];
    let alpha = [pixels[3], pixels[7], pixels[11]];
    WeaponIconEdit {
        hue_shift_degrees: -73,
        saturation: 38,
        brightness: 27,
        contrast: 12,
        red_balance: -9,
        green_balance: 7,
        blue_balance: 3,
        invert: true,
        ..WeaponIconEdit::default()
    }
    .apply_to_rgba8(&mut pixels)
    .unwrap();
    assert_eq!([pixels[3], pixels[7], pixels[11]], alpha);
}

#[test]
fn quarter_turn_and_flips_reposition_complete_rgba_pixels() {
    let mut pixels = [1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255];
    WeaponIconEdit {
        rotation_quarter_turns: 1,
        ..WeaponIconEdit::default()
    }
    .apply_to_rgba8_sized(&mut pixels, 2, 2)
    .unwrap();
    assert_eq!(
        pixels,
        [3, 0, 0, 255, 1, 0, 0, 255, 4, 0, 0, 255, 2, 0, 0, 255]
    );

    WeaponIconEdit {
        flip_horizontal: true,
        flip_vertical: true,
        ..WeaponIconEdit::default()
    }
    .apply_to_rgba8_sized(&mut pixels, 2, 2)
    .unwrap();
    assert_eq!(
        pixels,
        [2, 0, 0, 255, 4, 0, 0, 255, 1, 0, 0, 255, 3, 0, 0, 255]
    );
}

#[test]
fn malformed_rgba_payload_is_rejected_without_mutation() {
    let mut pixels = [1, 2, 3];
    let before = pixels;
    assert!(
        WeaponIconEdit {
            invert: true,
            ..WeaponIconEdit::default()
        }
        .apply_to_rgba8(&mut pixels)
        .is_err()
    );
    assert_eq!(pixels, before);
}

#[test]
fn clearing_a_color_removes_it_and_its_halo_but_not_separate_art() {
    let plate = [0xF2, 0xE3, 0x70];
    // Row 0: the plate and its darkened edge, which touch. Row 1: a similar colour that does not.
    let mut pixels = vec![
        plate[0], plate[1], plate[2], 255, //
        0x79, 0x71, 0x38, 255, //
        0x20, 0x40, 0x80, 255, //
        0x79, 0x71, 0x38, 255,
    ];
    WeaponIconEdit {
        cleared_color: Some(plate),
        ..Default::default()
    }
    .apply_to_rgba8_sized(&mut pixels, 2, 2)
    .expect("clearing a color should apply");

    assert_eq!(&pixels[0..4], &[0, 0, 0, 0], "the plate is cleared");
    assert_eq!(
        &pixels[4..8],
        &[0, 0, 0, 0],
        "its darkened edge is cleared with it"
    );
    assert_eq!(
        &pixels[8..12],
        &[0x20, 0x40, 0x80, 255],
        "unrelated artwork stays"
    );
    assert_eq!(
        &pixels[12..16],
        &[0, 0, 0, 0],
        "art on the same ramp is cleared only where it touches the region"
    );
}

#[test]
fn levels_stretch_the_input_range_and_hue_rules_take_every_shade() {
    // Levels: 64 becomes black, 192 becomes white, and the midpoint lands halfway.
    let mut pixels = vec![
        64, 128, 192, 255, //
        0, 0, 0, 0, //
        0, 0, 0, 0, //
        0, 0, 0, 0,
    ];
    WeaponIconEdit {
        black_point: 64,
        white_point: 192,
        ..Default::default()
    }
    .apply_to_rgba8_sized(&mut pixels, 2, 2)
    .expect("levels should apply");
    assert_eq!(&pixels[0..4], &[0, 128, 255, 255]);

    // A hue rule claims a dark shade of its source that a color range never reaches.
    let gold = [0xF2, 0xE3, 0x70];
    let dark_gold = [0x79, 0x71, 0x38];
    let by_range = IconColorReplacement {
        source: gold,
        replacement: [0x20, 0x40, 0x80],
        range_percent: 20,
        hue_range_degrees: None,
    };
    let by_hue = IconColorReplacement {
        hue_range_degrees: Some(20),
        ..by_range.clone()
    };
    assert_eq!(
        by_range.weight(dark_gold),
        0,
        "range cannot reach the shade"
    );
    assert!(by_hue.weight(dark_gold) > 0, "hue takes the whole surface");
    assert_eq!(
        by_hue.weight([0x80, 0x80, 0x80]),
        0,
        "a grey has no hue to match"
    );
}
