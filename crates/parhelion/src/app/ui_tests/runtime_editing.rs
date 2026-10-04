use super::*;

#[test]
fn replacing_recipe_closes_private_window_and_returns_to_weapon_page() {
    let mut app = PackageAuthoringApp {
        workbench_page: WorkbenchPage::Appearance,
        perk_request: Some(custom_perks::workbench::Request::EditChoice {
            socket: 4,
            choice: 0,
        }),
        ..Default::default()
    };
    app.clear_dependent_picker_queries();
    assert!(app.perk_request.is_none());
    assert_eq!(app.workbench_page, WorkbenchPage::Weapon);
}

#[test]
fn experimental_gameplay_controls_are_hidden_without_dropping_saved_overrides() {
    let mut app = PackageAuthoringApp::default();
    app.recipe.overrides.base_sandbox_perks = Some(vec![7]);
    let value = field(
        WeaponRuntimeValueKind::Float32,
        WeaponRuntimeValue::Float32Bits(1.0_f32.to_bits()),
    );
    app.recipe.overrides.runtime_values = vec![WeaponRuntimeValueOverride {
        locator: value.locator,
        value: value.value,
    }];
    let before = app.recipe.clone();
    for width in [480.0, 900.0, 1320.0] {
        app.show_experimental_options = false;
        let (output, overflow) = render(width, |ui| app.draw_gameplay_workspace(ui, None));
        let labels = text(&output);
        assert!(labels.contains("Firing & Runtime Baseline"));
        assert!(labels.contains("Parts"));
        assert!(!labels.contains("Technical"));
        assert!(overflow < 1.0, "width {width}: {overflow}");
        assert_eq!(app.recipe, before);
    }
    let hidden = technical_recipe_features(&app.recipe);
    assert!(
        hidden
            .iter()
            .any(|feature| feature == "Advanced: edited runtime values")
    );
    assert!(
        hidden
            .iter()
            .any(|feature| feature == "Advanced: base weapon perks")
    );
    app.runtime_bindings_open = true;
    app.set_show_experimental_options(false);
    assert!(!app.runtime_bindings_open);
    app.set_show_experimental_options(true);
    let (output, _) = render(900.0, |ui| app.draw_gameplay_workspace(ui, None));
    assert!(text(&output).contains("Parts"));
    assert!(text(&output).contains("Technical"));
    assert_eq!(app.recipe, before);
}

#[test]
fn ambiguous_private_fields_are_hidden_unless_a_saved_edit_needs_repair() {
    let field = field(
        WeaponRuntimeValueKind::Float32,
        WeaponRuntimeValue::Float32Bits(1.0_f32.to_bits()),
    );
    assert!(private_perk_runtime_field_is_visible(
        &field,
        "",
        &[],
        false,
        1
    ));
    assert!(!private_perk_runtime_field_is_visible(
        &field,
        "",
        &[],
        true,
        2
    ));
    let saved = vec![WeaponRuntimeValueOverride {
        locator: field.locator.clone(),
        value: field.value.clone(),
    }];
    assert!(private_perk_runtime_field_is_visible(
        &field, "", &saved, false, 2
    ));
}

#[test]
fn encoder_incompatible_fields_cannot_be_presented_as_editable() {
    let mut field = field(
        WeaponRuntimeValueKind::Float32,
        WeaponRuntimeValue::Float32Bits(0),
    );
    assert!(runtime_field_is_editable(&field));
    field.locator.byte_size = 8;
    assert!(!runtime_field_is_editable(&field));
    field.locator.byte_size = 4;
    field.kind = WeaponRuntimeValueKind::UnsignedInteger { bits: 24 };
    field.value = WeaponRuntimeValue::Unsigned(1);
    assert!(!runtime_field_is_editable(&field));
}

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
