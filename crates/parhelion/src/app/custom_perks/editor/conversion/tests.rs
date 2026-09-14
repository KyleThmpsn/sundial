use super::*;
use sundial::package_authoring::sandbox_perk::program::{Action, Asset};

#[test]
fn conversion_carries_component_edits_and_refuses_unrepresentable_references() {
    let mut loaded = super::super::tests::fixture();
    let tag = loaded.graphs[0].0;
    loaded.graphs[0].1.scope_fields();
    let field = loaded.graphs[0].1.fields().next().unwrap();
    let edit = WeaponRuntimeValueOverride {
        locator: field.locator.clone(),
        value: field.value.clone(),
    };
    let mut program = Program {
        actions: vec![Action::Pattern {
            asset: Asset {
                graph: tag,
                ..Asset::default()
            },
        }],
        ..Program::default()
    };
    transfer_values(&loaded, std::slice::from_ref(&edit), &mut program).unwrap();
    assert_eq!(
        program.actions[0].asset().unwrap().values,
        vec![edit.clone()]
    );
    program.actions = vec![Action::add_rounds(1)];
    assert!(
        transfer_values(&loaded, &[edit], &mut program)
            .unwrap_err()
            .contains("cannot carry")
    );
}

#[test]
fn conversion_preview_is_invalidated_by_each_kind_of_edit() {
    let mut editor = super::super::tests::editor(super::super::tests::fixture());
    let initial = editor.conversion_input();
    editor.activation = Some(PerkActivation::GrenadeKill);
    assert_ne!(editor.conversion_input(), initial);
    editor.activation = None;
    editor.projectile_draft.push(ProjectileSelection {
        source_graph: 1,
        donor_graph: 2,
    });
    assert_ne!(editor.conversion_input(), initial);
    editor.projectile_draft.clear();
    let field = editor.graph.as_ref().unwrap().graphs[0]
        .1
        .fields()
        .next()
        .unwrap();
    editor.draft.push(WeaponRuntimeValueOverride {
        locator: field.locator.clone(),
        value: field.value.clone(),
    });
    assert_ne!(editor.conversion_input(), initial);
    editor.draft.clear();
    editor.action_draft = actions::discover(&editor.graph.as_ref().unwrap().action_payload);
    editor.action_draft[0].value_bits = 2.0_f32.to_bits();
    assert_ne!(editor.conversion_input(), initial);
}

#[test]
fn conversion_uses_edited_action_values_and_rejects_stale_values() {
    let loaded = super::super::tests::fixture();
    let mut editor = super::super::tests::editor(loaded.clone());
    editor.action_draft = actions::discover(&loaded.action_payload);
    editor.action_draft[0].value_bits = 2.0_f32.to_bits();
    let changed = effective_action(
        loaded.action_tag,
        &loaded.action_payload,
        &editor.conversion_input(),
        &[],
    )
    .unwrap();
    assert_eq!(
        actions::source_bits(&changed, &editor.action_draft[0]).unwrap(),
        2.0_f32.to_bits()
    );
    assert_eq!(
        actions::source_bits(&loaded.action_payload, &editor.action_draft[0]).unwrap(),
        0.5_f32.to_bits()
    );
    editor.action_draft[0].expected_bits = 1.0_f32.to_bits();
    assert!(
        effective_action(
            loaded.action_tag,
            &loaded.action_payload,
            &editor.conversion_input(),
            &[]
        )
        .is_err()
    );
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
fn native_conversion_preserves_all_conditions_activation_and_component_values() {
    let path = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let manager = open_shadowkeep_package_manager(&path).unwrap();
    let key = PerkEditorKey {
        socket_index: 0,
        choice_index: 0,
        source_plug_hash: 1,
        source_perk_index: 421,
    };
    let loaded = load_private_perk_runtime_graph(&path, key, &[]).unwrap();
    for activation in PerkActivation::ALL {
        let mut editor = super::super::tests::editor(loaded.clone());
        editor.key = key;
        editor.activation = Some(activation);
        let input = editor.conversion_input();
        let preview = prepare(&manager, &loaded, &input).unwrap();
        assert!(preview.fidelity.unwrap().is_empty());
        let native = preview.program.native.as_ref().unwrap();
        let decoded =
            sundial::package_authoring::sandbox_perk::action::decode(&native.graph.emit().unwrap())
                .unwrap();
        assert_eq!(decoded.groups[0].removal.len(), 2);
        assert_eq!(editor.conversion_input(), input);
    }
    let key = PerkEditorKey {
        source_perk_index: 1178,
        ..key
    };
    let loaded = load_private_perk_runtime_graph(&path, key, &[]).unwrap();
    let mut editor = super::super::tests::editor(loaded.clone());
    editor.key = key;
    let (_, parameter) = movement::mapped(&loaded).into_iter().next().unwrap();
    parameter.set(&mut editor.draft, 4.25).unwrap();
    let preview = prepare(&manager, &loaded, &editor.conversion_input()).unwrap();
    let values = preview
        .program
        .assets()
        .flat_map(|asset| &asset.values)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(parameter.value(&values).unwrap(), 4.25);
    assert!(preview.fidelity.unwrap().is_empty());
}
