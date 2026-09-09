use super::*;

fn field(kind: WeaponRuntimeValueKind, value: WeaponRuntimeValue) -> WeaponRuntimeField {
    WeaponRuntimeField {
        locator: WeaponRuntimeFieldLocator {
            binding_hash: 0xB176_70ED,
            resource_index: 0,
            root: sundial::package_authoring::weapon_runtime::WeaponRuntimeRootKind::ComponentDefinition,
            root_schema: 0x8080_388F,
            path: Vec::new(),
            type_handle: 0x8080_2F16,
            value_offset: 0x48,
            byte_size: kind.byte_size(),
        },
        owner_offset: 0x100,
        name: "Runtime Field".into(),
        path_label: "Component / Runtime Field".into(),
        kind,
        value,
        source: WeaponRuntimeFieldSource::GeneratedSchema,
        generated_kind: None,
    }
}

fn frame(
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    mut draw: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                workbench_style(ui);
                draw(ui);
            });
        },
    )
}

fn text(output: &egui::FullOutput) -> String {
    fn append(shape: &egui::Shape, result: &mut String) {
        match shape {
            egui::Shape::Text(value) => {
                result.push_str(&value.galley.job.text);
                result.push('\n');
            }
            egui::Shape::Vec(values) => {
                for value in values {
                    append(value, result);
                }
            }
            _ => {}
        }
    }
    let mut result = String::new();
    for shape in &output.shapes {
        append(&shape.shape, &mut result);
    }
    result
}

#[test]
fn passive_render_preserves_all_ieee_float_bits_and_saved_overrides() {
    let patterns = [
        0,
        0x8000_0000,
        1,
        0x007F_FFFF,
        0x3F80_0001,
        0x7F7F_FFFF,
        0x7F80_0000,
        0xFF80_0000,
        0x7FC0_1234,
        0xFFC0_5678,
        0x7F80_0001,
        0xFF80_0001,
    ];
    for bits in patterns {
        for value in [
            WeaponRuntimeValue::Float32Bits(bits),
            WeaponRuntimeValue::Vector4Float32Bits([bits, 0x3F80_0000, bits, 0x8000_0000]),
        ] {
            let kind = match value {
                WeaponRuntimeValue::Float32Bits(_) => WeaponRuntimeValueKind::Float32,
                _ => WeaponRuntimeValueKind::Vector4Float32,
            };
            let field = field(kind, value);
            for customized in [false, true] {
                let ctx = egui::Context::default();
                let mut drafts = BTreeMap::new();
                let mut overrides = if customized {
                    vec![WeaponRuntimeValueOverride {
                        locator: field.locator.clone(),
                        value: field.value.clone(),
                    }]
                } else {
                    Vec::new()
                };
                let before = overrides.clone();
                for _ in 0..3 {
                    frame(&ctx, Vec::new(), |ui| {
                        draw_runtime_value_override_field(ui, &field, &mut overrides, &mut drafts);
                    });
                    assert_eq!(overrides, before, "passive render changed {bits:08X}");
                    assert_eq!(drafts[&(field.locator.clone(), 0)], format!("0x{bits:08X}"));
                }
            }
        }
    }
}

#[test]
fn finite_decimal_display_round_trips_without_rounding_small_values_to_zero() {
    let ctx = egui::Context::default();
    for bits in [1, 0x007F_FFFF, 0x3F80_0001, 0x7F7F_FFFF, 0x8000_0000] {
        let expected = format!("{:?}", f32::from_bits(bits));
        let mut output = egui::FullOutput::default();
        for _ in 0..2 {
            output = frame(&ctx, Vec::new(), |ui| {
                assert_eq!(draw_runtime_float_decimal(ui, bits), None);
            });
        }
        assert!(
            text(&output).contains(&expected),
            "{bits:08X}: {}",
            text(&output)
        );
        assert_eq!(expected.parse::<f32>().unwrap().to_bits(), bits);
    }
}

#[test]
fn integer_64_bit_display_is_exact_at_precision_boundaries() {
    for (kind, current) in [
        (
            WeaponRuntimeValueKind::SignedInteger { bits: 64 },
            WeaponRuntimeValue::Signed(i64::MIN),
        ),
        (
            WeaponRuntimeValueKind::SignedInteger { bits: 64 },
            WeaponRuntimeValue::Signed(i64::MAX),
        ),
        (
            WeaponRuntimeValueKind::SignedInteger { bits: 64 },
            WeaponRuntimeValue::Signed(-9_007_199_254_740_993),
        ),
        (
            WeaponRuntimeValueKind::UnsignedInteger { bits: 64 },
            WeaponRuntimeValue::Unsigned(9_007_199_254_740_993),
        ),
        (
            WeaponRuntimeValueKind::Enum { bits: 64 },
            WeaponRuntimeValue::Unsigned(u64::MAX),
        ),
        (
            WeaponRuntimeValueKind::BitFlags { bits: 64 },
            WeaponRuntimeValue::Unsigned(u64::MAX - 1),
        ),
    ] {
        let expected = match current {
            WeaponRuntimeValue::Signed(value) => value.to_string(),
            WeaponRuntimeValue::Unsigned(value) => value.to_string(),
            _ => unreachable!(),
        };
        let field = field(kind, current);
        let ctx = egui::Context::default();
        let mut drafts = BTreeMap::new();
        let mut saved = Vec::new();
        let mut output = egui::FullOutput::default();
        for _ in 0..2 {
            output = frame(&ctx, Vec::new(), |ui| {
                draw_runtime_value_override_field(ui, &field, &mut saved, &mut drafts);
            });
        }
        assert!(text(&output).contains(&expected), "{}", text(&output));
        assert!(saved.is_empty());
    }
}

#[test]
fn integer_64_bit_typed_edits_do_not_pass_through_f64() {
    for (kind, expected, entry) in [
        (
            WeaponRuntimeValueKind::SignedInteger { bits: 64 },
            WeaponRuntimeValue::Signed(i64::MIN),
            "-9223372036854775808",
        ),
        (
            WeaponRuntimeValueKind::UnsignedInteger { bits: 64 },
            WeaponRuntimeValue::Unsigned(9_007_199_254_740_993),
            "9007199254740993",
        ),
        (
            WeaponRuntimeValueKind::BitFlags { bits: 64 },
            WeaponRuntimeValue::Unsigned(u64::MAX),
            "18446744073709551615",
        ),
    ] {
        let current = match expected {
            WeaponRuntimeValue::Signed(_) => WeaponRuntimeValue::Signed(0),
            _ => WeaponRuntimeValue::Unsigned(0),
        };
        let field = field(kind, current);
        let ctx = egui::Context::default();
        let mut drafts = BTreeMap::new();
        let mut next = None;
        let mut draw = |ui: &mut egui::Ui| {
            ui.horizontal_wrapped(|ui| {
                next = draw_runtime_value_editor(
                    ui,
                    &field.locator,
                    &field.kind,
                    &field.value,
                    &mut drafts,
                );
            });
        };
        for _ in 0..2 {
            frame(&ctx, Vec::new(), &mut draw);
        }
        for pressed in [true, false] {
            let pos = egui::pos2(25.0, 18.0);
            frame(
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                &mut draw,
            );
        }
        frame(
            &ctx,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
                egui::Event::Text(entry.into()),
            ],
            &mut draw,
        );
        assert_eq!(next, Some(expected));
    }
}

#[test]
fn external_changes_refresh_cached_values_but_invalid_drafts_survive_passive_frames() {
    for (kind, original, replacement, expected) in [
        (
            WeaponRuntimeValueKind::HexIdentifier { bits: 64 },
            WeaponRuntimeValue::Unsigned(1),
            WeaponRuntimeValue::Unsigned(2),
            "0x0000000000000002",
        ),
        (
            WeaponRuntimeValueKind::Float32,
            WeaponRuntimeValue::Float32Bits(0),
            WeaponRuntimeValue::Float32Bits(0x3F80_0000),
            "0x3F800000",
        ),
        (
            WeaponRuntimeValueKind::Vector4Float32,
            WeaponRuntimeValue::Vector4Float32Bits([0; 4]),
            WeaponRuntimeValue::Vector4Float32Bits([0x3F80_0000; 4]),
            "0x3F800000",
        ),
        (
            WeaponRuntimeValueKind::FixedBytes { size: 2 },
            WeaponRuntimeValue::Bytes(vec![0, 0]),
            WeaponRuntimeValue::Bytes(vec![0x12, 0x34]),
            "12 34",
        ),
        (
            WeaponRuntimeValueKind::UnsignedInteger { bits: 64 },
            WeaponRuntimeValue::Unsigned(0),
            WeaponRuntimeValue::Unsigned(u64::MAX),
            "18446744073709551615",
        ),
    ] {
        let field = field(kind, original);
        let ctx = egui::Context::default();
        let mut drafts = BTreeMap::new();
        frame(&ctx, Vec::new(), |ui| {
            assert_eq!(
                draw_runtime_value_editor(
                    ui,
                    &field.locator,
                    &field.kind,
                    &field.value,
                    &mut drafts
                ),
                None
            );
        });
        drafts.insert((field.locator.clone(), 0), "invalid draft".into());
        frame(&ctx, Vec::new(), |ui| {
            assert_eq!(
                draw_runtime_value_editor(
                    ui,
                    &field.locator,
                    &field.kind,
                    &field.value,
                    &mut drafts
                ),
                None
            );
        });
        assert_eq!(drafts[&(field.locator.clone(), 0)], "invalid draft");
        frame(&ctx, Vec::new(), |ui| {
            assert_eq!(
                draw_runtime_value_editor(
                    ui,
                    &field.locator,
                    &field.kind,
                    &replacement,
                    &mut drafts
                ),
                None
            );
        });
        assert_eq!(drafts[&(field.locator, 0)], expected);
    }
}

#[test]
fn incompatible_saved_values_show_the_donor_without_silently_replacing_the_saved_value() {
    let field = field(
        WeaponRuntimeValueKind::Float32,
        WeaponRuntimeValue::Float32Bits(0x3F80_0000),
    );
    let ctx = egui::Context::default();
    let mut drafts = BTreeMap::new();
    let mut overrides = vec![WeaponRuntimeValueOverride {
        locator: field.locator.clone(),
        value: WeaponRuntimeValue::Unsigned(u64::MAX),
    }];
    let before = overrides.clone();
    let output = frame(&ctx, Vec::new(), |ui| {
        draw_runtime_value_override_field(ui, &field, &mut overrides, &mut drafts);
    });
    assert_eq!(overrides, before);
    assert_eq!(drafts[&(field.locator, 0)], "0x3F800000");
    assert!(text(&output).contains("invalid for this field's type, size, or range"));
}
