use super::*;

#[test]
fn runtime_input_errors_persist_without_a_new_keystroke_or_recipe_mutation() {
    for (kind, value) in [
        (
            WeaponRuntimeValueKind::HexIdentifier { bits: 32 },
            WeaponRuntimeValue::Unsigned(1),
        ),
        (
            WeaponRuntimeValueKind::Float32,
            WeaponRuntimeValue::Float32Bits(1.0_f32.to_bits()),
        ),
        (
            WeaponRuntimeValueKind::Vector4Float32,
            WeaponRuntimeValue::Vector4Float32Bits([0; 4]),
        ),
        (
            WeaponRuntimeValueKind::FixedBytes { size: 16 },
            WeaponRuntimeValue::Bytes(vec![0; 16]),
        ),
    ] {
        let field = field(kind, value);
        let mut drafts = BTreeMap::from([((field.locator.clone(), 0), "not hex".into())]);
        let mut saved = Vec::new();
        let (output, _) = render(480.0, |ui| {
            draw_runtime_value_override_field(ui, &field, &mut saved, &mut drafts)
        });
        assert!(
            text(&output).to_ascii_lowercase().contains("not applied"),
            "{:?}",
            field.kind
        );
        assert!(
            saved.is_empty(),
            "invalid draft must not replace the last valid recipe value"
        );
    }
}
