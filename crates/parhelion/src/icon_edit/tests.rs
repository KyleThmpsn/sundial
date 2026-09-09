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
fn fixed_hue_rotation_moves_primary_colors_by_exact_sectors() {
    let mut pixels = [255, 0, 0, 7, 0, 255, 0, 9, 0, 0, 255, 11];
    WeaponIconEdit {
        hue_shift_degrees: 120,
        ..WeaponIconEdit::default()
    }
    .apply_to_rgba8(&mut pixels)
    .unwrap();
    assert_eq!(pixels, [0, 255, 0, 7, 0, 0, 255, 9, 255, 0, 0, 11]);
}

#[test]
fn brightness_then_invert_has_stable_fixed_pixels() {
    let mut pixels = [10, 100, 200, 17];
    WeaponIconEdit {
        brightness: 50,
        invert: true,
        ..WeaponIconEdit::default()
    }
    .apply_to_rgba8(&mut pixels)
    .unwrap();
    assert_eq!(pixels, [122, 77, 27, 17]);

    let mut black = [10, 100, 200, 33];
    WeaponIconEdit {
        brightness: -100,
        ..WeaponIconEdit::default()
    }
    .apply_to_rgba8(&mut black)
    .unwrap();
    assert_eq!(black, [0, 0, 0, 33]);
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
fn opacity_scales_alpha_without_changing_rgb() {
    let mut pixels = [12, 34, 56, 200];
    WeaponIconEdit {
        opacity_percent: 25,
        ..WeaponIconEdit::default()
    }
    .apply_to_rgba8(&mut pixels)
    .unwrap();
    assert_eq!(pixels, [12, 34, 56, 50]);
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
