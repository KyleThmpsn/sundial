use super::*;

#[test]
fn exporting_the_open_recipe_updates_its_save_baseline() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let recipe = WeaponRecipe::new_weapon("parhelion.export-regression").unwrap();
    let path = library.save_new(&recipe).unwrap();
    let mut app = PackageAuthoringApp {
        recipe_library: Some(library),
        recipe: recipe.clone(),
        recipe_baseline: recipe,
        recipe_path: Some(path.clone()),
        ..Default::default()
    };
    app.recipe.name = "Exported draft".into();
    app.recipe_dirty = true;
    app.export_recipe_to(&path).unwrap();
    assert_eq!(app.recipe_baseline, app.recipe);
    assert!(!app.recipe_dirty);
    app.recipe.name = "Next edit".into();
    app.save_library_recipe();
    assert_eq!(WeaponRecipe::load_json(&path).unwrap().name, "Next edit");
    let other = app
        .recipe_library
        .as_ref()
        .unwrap()
        .root()
        .join("duplicate.json");
    assert!(app.export_recipe_to(&other).is_err());
    assert!(!other.exists());
    let external = directory.path().join("export.json");
    app.export_recipe_to(&external).unwrap();
    assert_eq!(WeaponRecipe::load_json(&external).unwrap(), app.recipe);
}

fn investment_stat(definition_index: u16, value: i32) -> WeaponInvestmentStat {
    WeaponInvestmentStat {
        definition_index,
        definition_hash: None,
        name: format!("Stat {definition_index}"),
        value,
        minimum_value: None,
        maximum_value: None,
        display_as_numeric: false,
        is_linear: false,
        display_interpolation: Vec::new(),
    }
}

#[test]
fn default_app_is_idle_and_waits_for_the_host_package_path() {
    let app = PackageAuthoringApp::default();

    assert!(app.build_receiver.is_none());
    assert!(app.build_progress.is_none());
    assert!(app.latest_build.is_none());
    assert!(app.install_receiver.is_none());
    assert!(app.latest_install.is_none());
    assert_eq!(app.build_dialog_step, BuildDialogStep::Build);
    assert!(app.packages.as_os_str().is_empty());
    assert_ne!(app.staging, app.backup_root);
    assert!(app.ignore_installed);
}

#[test]
fn stat_display_donor_adds_missing_rows_without_overwriting_recipe_values() {
    let mut overrides = WeaponRecipeOverrides {
        investment_stats: vec![WeaponStatOverride {
            definition_index: 20,
            value: 91,
        }],
        removed_investment_stats: vec![14, 30],
        ..WeaponRecipeOverrides::default()
    };
    let gameplay = [investment_stat(14, 600), investment_stat(20, 45)];
    let profile = [
        investment_stat(14, 720),
        investment_stat(20, 60),
        investment_stat(30, 80),
    ];

    merge_stat_profile_investment_rows(&mut overrides, &gameplay, &profile);

    assert!(overrides.removed_investment_stats.is_empty());
    assert_eq!(
        overrides.investment_stats,
        vec![
            WeaponStatOverride {
                definition_index: 20,
                value: 91,
            },
            WeaponStatOverride {
                definition_index: 30,
                value: 80,
            },
        ]
    );
}

#[test]
fn standard_workbench_controls_are_not_reported_as_technical_overrides() {
    let mut recipe = PackageAuthoringApp::default().recipe;
    recipe.overrides.investment_stats.push(WeaponStatOverride {
        definition_index: 14,
        value: 50,
    });
    recipe.overrides.socket_columns = vec![Some(WeaponSocketColumnRecipe {
        choices: vec![HexHash::new(0xAAAA_AAAA)],
        ..WeaponSocketColumnRecipe::default()
    })];

    assert!(technical_recipe_features(&recipe).is_empty());
}

#[test]
fn runtime_value_scope_defaults_to_resolved_weapon_fields() {
    assert!(runtime_field_is_in_editor_scope(
        WeaponRuntimeFieldSource::GeneratedSchema,
        true,
        false,
        false,
        false,
    ));
    assert!(runtime_field_is_in_editor_scope(
        WeaponRuntimeFieldSource::NativeMember,
        true,
        false,
        false,
        false,
    ));
    assert!(!runtime_field_is_in_editor_scope(
        WeaponRuntimeFieldSource::OpaqueNativeType,
        true,
        false,
        false,
        true,
    ));
}

#[test]
fn runtime_value_scope_preserves_saved_values_and_gates_show_all() {
    assert!(runtime_field_is_in_editor_scope(
        WeaponRuntimeFieldSource::OpaqueNativeType,
        false,
        true,
        false,
        false,
    ));
    assert!(!runtime_field_is_in_editor_scope(
        WeaponRuntimeFieldSource::OpaqueNativeType,
        true,
        false,
        true,
        false,
    ));
    assert!(runtime_field_is_in_editor_scope(
        WeaponRuntimeFieldSource::OpaqueNativeType,
        true,
        false,
        true,
        true,
    ));
    assert!(!runtime_field_is_in_editor_scope(
        WeaponRuntimeFieldSource::OpaqueNativeType,
        false,
        false,
        true,
        true,
    ));
}

#[test]
fn hidden_technical_recipe_data_is_detected_without_being_mutated() {
    let mut app = PackageAuthoringApp::default();
    app.recipe.overrides.rarity = Some(RecipeRarity::Exotic);
    app.recipe.overrides.weapon_pattern_index = Some(7);
    app.recipe.overrides.socket_columns = vec![Some(WeaponSocketColumnRecipe {
        choices: vec![HexHash::new(0xAAAA_AAAA)],
        socket_type: Some(3),
        ..WeaponSocketColumnRecipe::default()
    })];
    let before = app.recipe.clone();

    assert_eq!(
        technical_recipe_features(&app.recipe),
        vec!["native socket fields"]
    );
    app.set_show_experimental_options(true);
    app.set_show_experimental_options(false);

    assert_eq!(app.recipe.clone(), before);
    assert_eq!(
        app.take_preferences_changed(),
        Some(PackageAuthoringPreferences {
            show_parhelion_experimental_options: false,
        })
    );
}

#[test]
fn socket_choices_keep_readable_widths_and_wrap_after_three_columns() {
    assert_eq!(socket_choice_columns(700.0, 1), 1);
    assert_eq!(socket_choice_columns(700.0, 3), 3);
    assert_eq!(socket_choice_columns(700.0, 8), 3);
    assert_eq!(socket_choice_columns(330.0, 8), 1);
    assert_eq!(socket_choice_columns(f32::NAN, 8), 1);
}

#[test]
fn removing_a_socket_choice_keeps_native_metadata_aligned() {
    let mut recipe = PackageAuthoringApp::default().recipe;
    recipe.overrides.socket_columns = vec![Some(WeaponSocketColumnRecipe {
        choices: [1, 2, 3].into_iter().map(HexHash::new).collect(),
        choice_weight_bits: vec![10, 20, 30],
        choice_conditions: [1, 2, 3]
            .into_iter()
            .map(|operand| {
                vec![WeaponNumericInstructionRecipe {
                    opcode: 10,
                    operand,
                }]
            })
            .collect(),
        ..WeaponSocketColumnRecipe::default()
    })];

    set_recipe_socket_column(&mut recipe, 1, 0, &[1, 2, 3], vec![1, 3], Some(1));

    let column = recipe.overrides.socket_columns[0].as_ref().unwrap();
    assert_eq!(column.choices, vec![HexHash::new(1), HexHash::new(3)]);
    assert_eq!(column.choice_weight_bits, vec![10, 30]);
    assert_eq!(
        column
            .choice_conditions
            .iter()
            .map(|program| program[0].operand)
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
}

#[test]
fn removing_a_socket_choice_shifts_only_later_picker_queries() {
    let mut queries = BTreeMap::from([
        (0, "default".to_owned()),
        (1, "removed".to_owned()),
        (2, "next".to_owned()),
        (5, "later".to_owned()),
    ]);

    shift_socket_choice_queries_after_removal(&mut queries, 1);

    assert_eq!(
        queries,
        BTreeMap::from([
            (0, "default".to_owned()),
            (1, "next".to_owned()),
            (4, "later".to_owned()),
        ])
    );
}

#[test]
fn disabled_socket_configuration_starts_invalid_and_preserves_other_rows() {
    let mut recipe = PackageAuthoringApp::default().recipe;
    recipe.overrides.socket_columns = vec![Some(WeaponSocketColumnRecipe {
        choices: vec![HexHash::new(0xAAAA_AAAA)],
        ..WeaponSocketColumnRecipe::default()
    })];

    materialize_socket_column(&mut recipe, 3, 2, &[], true);

    assert_eq!(
        recipe.overrides.socket_columns[0].as_ref().unwrap().choices,
        vec![HexHash::new(0xAAAA_AAAA)]
    );
    let pending = recipe.overrides.socket_columns[2].as_ref().unwrap();
    assert_eq!(pending.socket_type, Some(u16::MAX));
    assert_eq!(pending.choices, vec![HexHash::new(0)]);
}

#[test]
fn build_worker_events_update_progress_then_finish_with_an_error() {
    let mut app = PackageAuthoringApp::default();
    let (sender, receiver) = mpsc::channel();
    app.build_receiver = Some(receiver);
    sender
        .send(BuildWorkerEvent::Progress(TimedBuildProgress {
            phase: BuildPhase::ValidatingPackages,
            current_artifact: Some("test.pkg".to_owned()),
            completed: 2,
            total: 6,
            elapsed: Duration::from_millis(750),
        }))
        .unwrap();

    app.poll_build();

    let progress = app.build_progress.as_ref().unwrap();
    assert_eq!(progress.phase, BuildPhase::ValidatingPackages);
    assert_eq!(progress.current_artifact.as_deref(), Some("test.pkg"));
    assert_eq!((progress.completed, progress.total), (2, 6));
    assert!((progress.fraction() - (1.0 / 3.0)).abs() < f32::EPSILON);

    sender
        .send(BuildWorkerEvent::Finished {
            result: Err("synthetic failure".to_owned()),
            elapsed: Duration::from_secs(2),
        })
        .unwrap();
    app.poll_build();

    assert!(app.build_receiver.is_none());
    assert!(matches!(app.latest_build, Some(Err(ref error)) if error == "synthetic failure"));
    assert_eq!(
        app.build_progress.as_ref().unwrap().elapsed,
        Duration::from_secs(2)
    );
}

#[test]
fn successful_build_event_enters_completed_install_ready_state() {
    let mut app = PackageAuthoringApp::default();
    let (sender, receiver) = mpsc::channel();
    app.build_receiver = Some(receiver);
    sender
        .send(BuildWorkerEvent::Finished {
            result: Ok(BuildReport {
                weapons: Vec::new(),
                run_directory: PathBuf::from("staged-run"),
                manifest_path: PathBuf::from("staged-run/manifest.json"),
                artifacts: Vec::new(),
                selection_fingerprint: "fingerprint".to_owned(),
                staged_recipe_paths: Vec::new(),
            }),
            elapsed: Duration::from_secs(3),
        })
        .unwrap();

    app.poll_build();

    assert!(app.build_receiver.is_none());
    assert!(matches!(app.latest_build, Some(Ok(_))));
    let progress = app.build_progress.as_ref().unwrap();
    assert_eq!(progress.phase, BuildPhase::Complete);
    assert_eq!((progress.completed, progress.total), (1, 1));
    assert_eq!(progress.elapsed, Duration::from_secs(3));
}

#[test]
fn active_catalog_work_uses_the_loading_state() {
    let mut app = PackageAuthoringApp::default();
    assert!(!app.catalog_is_loading());

    let (_sender, receiver) = mpsc::channel();
    app.catalog_receiver = Some(receiver);

    assert!(app.catalog_is_loading());
    assert!(app.has_background_work());
}

#[test]
fn default_app_opens_a_clean_name_derived_new_recipe() {
    let app = PackageAuthoringApp::default();

    assert_eq!(app.recipe.namespace, "parhelion.new-recipe");
    assert_eq!(app.recipe.name, "New Recipe");
    assert_eq!(app.recipe.donor.item_hash.parse_u32(), Ok(0));
    assert!(app.recipe.donor.expected_name.is_none());
    assert!(app.recipe.identity_is_name_derived());
    assert!(app.recipe_path.is_none());
    assert!(!app.recipe_dirty);
    assert_eq!(
        app.recipe.collection_placement,
        crate::RecipeCollectionPlacement::SunriseBadge
    );
}

#[test]
fn dirty_recipe_requires_confirmation_before_replacement() {
    let mut app = PackageAuthoringApp {
        recipe_dirty: true,
        ..PackageAuthoringApp::default()
    };
    app.recipe.name = "Unsaved name".to_owned();

    assert!(!app.request_recipe_action(PendingRecipeAction::New));
    assert_eq!(app.recipe.name, "Unsaved name");
    assert_eq!(app.pending_recipe_action, Some(PendingRecipeAction::New));

    let action = app.pending_recipe_action.take().unwrap();
    assert!(app.execute_recipe_action(action));
    assert_eq!(app.recipe.name, "New Recipe");
    assert!(!app.recipe_dirty);
}

#[test]
fn close_approval_is_deferred_until_dirty_recipe_is_discarded() {
    let mut app = PackageAuthoringApp {
        recipe_dirty: true,
        ..PackageAuthoringApp::default()
    };

    assert!(!app.request_recipe_action(PendingRecipeAction::Close));
    assert!(!app.take_close_approved());
    assert_eq!(app.pending_recipe_action, Some(PendingRecipeAction::Close));

    let action = app.pending_recipe_action.take().unwrap();
    assert!(!app.execute_recipe_action(action));
    assert!(app.take_close_approved());
    assert!(!app.take_close_approved());
}

#[test]
fn inherited_socket_column_starts_with_the_scalar_default() {
    assert_eq!(
        inherited_socket_choices(
            Some(0xAAAA_AAAA),
            &[0xBBBB_BBBB, 0xAAAA_AAAA, 0xCCCC_CCCC],
            3,
        ),
        vec![0xAAAA_AAAA, 0xBBBB_BBBB, 0xCCCC_CCCC]
    );
}

#[test]
fn inherited_socket_column_deduplicates_and_honors_the_authored_limit() {
    assert_eq!(
        inherited_socket_choices(
            Some(0xAAAA_AAAA),
            &[0xAAAA_AAAA, 0xBBBB_BBBB, 0xCCCC_CCCC],
            2,
        ),
        vec![0xAAAA_AAAA, 0xBBBB_BBBB]
    );
    assert_eq!(
        inherited_socket_choices(None, &[0xBBBB_BBBB, 0xBBBB_BBBB], 3),
        vec![0xBBBB_BBBB]
    );
    assert!(inherited_socket_choices(Some(0xAAAA_AAAA), &[0xBBBB_BBBB], 0).is_empty());
}

#[test]
fn known_output_list_contains_optional_runtime_core_overlays_and_assets() {
    assert_eq!(CANONICAL_ARTIFACT_FILE_NAMES.len(), 10);
    assert_eq!(CANONICAL_ARTIFACT_FILE_NAMES[0], "w64_sandbox_01bb_7.pkg");
    assert_eq!(
        CANONICAL_ARTIFACT_FILE_NAMES[1],
        "w64_investment_0361_7.pkg"
    );
    assert_eq!(
        CANONICAL_ARTIFACT_FILE_NAMES[2],
        "w64_shared_manifest_0374_7.pkg"
    );
    assert!(
        CANONICAL_ARTIFACT_FILE_NAMES[4..9]
            .iter()
            .all(|name| name.ends_with("_4.pkg"))
    );
    assert_eq!(
        CANONICAL_ARTIFACT_FILE_NAMES[9],
        "w64_parhelion_assets_0aa0_0.pkg"
    );
    assert_eq!(CANONICAL_ARTIFACT_FILE_NAMES[3], "w64_ui_037e_6.pkg");
}
