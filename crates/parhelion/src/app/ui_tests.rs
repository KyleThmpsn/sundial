//! Headless UI contract/layout checks. These do not open a window or mutate installed content.
use super::*;

mod added_sockets;
mod authoring_safety;
mod runtime_layout;
mod socket_account_updates;

#[test]
fn replacement_confirmation_names_removals_without_changing_account_or_recipe() {
    let directory = tempfile::tempdir().unwrap();
    let packages = directory.path().join("packages");
    std::fs::create_dir(&packages).unwrap();
    let path = directory.path().join("settings.json");
    let original = br#"{"version":8,"state":{"account":{"primary_soid":"0x0000000000000001","profile_items":[{"definition_hash":100,"quantity":1}]},"characters":[]}}"#;
    std::fs::write(&path, original).unwrap();
    let review = crate::install::test_review(&packages, BTreeSet::from([100]));
    let mut app = PackageAuthoringApp {
        replacement_review: Some(Ok(review)),
        latest_build: Some(Ok(BuildReport {
            weapons: vec![],
            run_directory: PathBuf::from("staged"),
            manifest_path: PathBuf::from("staged/manifest.json"),
            artifacts: vec![],
            selection_fingerprint: "test".into(),
            staged_recipe_paths: vec![],
        })),
        ..Default::default()
    };
    let before = app.recipe.clone();
    let (output, _) = render(900.0, |ui| app.draw_install_confirmation(ui));
    let labels = text(&output);
    assert!(labels.contains("Account Changes"));
    assert!(labels.contains("Remove 1 saved item:"));
    assert!(labels.contains("Back Up, Remove & Install"));
    assert!(labels.contains("Cancel"));
    assert_eq!(std::fs::read(path).unwrap(), original);
    assert_eq!(app.recipe, before);
    assert!(app.install_receiver.is_none());
}

#[test]
fn successful_install_report_is_compact_and_ends_with_close() {
    let report = InstallReport {
        manifest_schema: 1,
        staged_run_directory: PathBuf::from("staged"),
        target_packages_directory: PathBuf::from(r"\\?\C:\Destiny2\packages"),
        backup_directory: PathBuf::from(r"\\?\C:\Backups\parhelion-backup-v2-abc-123-1-0"),
        artifacts: vec![],
        removed_obsolete_packages: vec![],
        recipe_backup_directory: None,
        pruned_backup_directories: vec![],
        backup_prune_warning: None,
        invalidated_sunrise_cache: None,
        invalidated_package_header_caches: vec![],
        profile_sync: Some(Err("Account is read-only".into())),
        cleaned_account: None,
    };
    let mut app = PackageAuthoringApp {
        latest_install: Some(Ok(report)),
        build_dialog_step: BuildDialogStep::Install,
        ..Default::default()
    };
    for width in [620.0, 960.0] {
        let (output, overflow) = render(width, |ui| app.draw_install_status(ui));
        let labels = text(&output);
        for label in [
            "Packages Installed",
            "Open Backup Folder",
            "Installation Details",
            "Close",
            "Account is read-only",
        ] {
            assert!(labels.contains(label), "Missing {label}");
        }
        assert!(!labels.contains("Review Installation"));
        assert!(!labels.contains("Back to Build"));
        assert!(!labels.contains("parhelion-backup-v2"));
        assert!(!labels.contains(r"\\?\"));
        assert!(
            overflow < 1.0,
            "Install report overflow {overflow} at {width}"
        );
        assert!(app.install_receiver.is_none());
    }
}

#[test]
fn build_report_counts_and_lists_optional_packages() {
    let artifacts = CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| crate::artifact::ArtifactMetadata {
            file_name: (*name).into(),
            byte_length: 10,
            sha256: "0".repeat(64),
        })
        .collect::<Vec<_>>();
    let report = BuildReport {
        weapons: vec![],
        run_directory: PathBuf::from("staged"),
        manifest_path: PathBuf::from("staged/manifest.json"),
        artifacts,
        selection_fingerprint: "test".into(),
        staged_recipe_paths: vec![],
    };
    let (output, _) = render(1200.0, |ui| draw_build_report(ui, &report));
    let labels = text(&output);
    assert!(labels.contains(&format!(
        "{} packages staged for installation",
        report.artifacts.len()
    )));
    assert!(labels.contains("Package Details"));
    assert!(!labels.contains(&report.artifacts[0].file_name));
    let (output, overflow) = render(620.0, |ui| reports::draw_build_details(ui, &report));
    let labels = text(&output);
    for artifact in &report.artifacts {
        assert!(labels.contains(&artifact.file_name));
        assert!(!labels.contains(&artifact.sha256));
    }
    assert!(overflow < 1.0);
}

#[test]
fn custom_perk_release_notice_preserves_recipes_with_experimental_options_on_or_off() {
    let mut app = PackageAuthoringApp {
        recipe: WeaponRecipe::every_end(),
        ..Default::default()
    };
    let before = app.recipe.clone();
    for experimental in [false, true] {
        app.show_experimental_options = experimental;
        app.private_perk_socket = Some(0);
        let (output, _) = render(640.0, |ui| app.draw_custom_perks_window(ui.ctx()));
        let labels = text(&output);
        assert!(labels.contains("Custom perk editing is planned for a future release."));
        assert!(labels.contains("Use Existing Custom Perk"));
        assert_eq!(app.recipe, before);
    }
}

#[test]
fn unnamed_sandbox_effects_do_not_claim_to_be_unused() {
    assert_eq!(
        sandbox_perk_choice_label(405, &[]),
        "Effect 405 · name unavailable"
    );
}

#[test]
fn dye_rows_fit_and_remain_unchanged_when_previewing() {
    let mut app = PackageAuthoringApp::default();
    app.recipe.overrides.render_dye_rows = Some([
        vec![
            WeaponDyeReferenceRecipe {
                channel_index: -1,
                dye_reference_index: 0,
            },
            WeaponDyeReferenceRecipe {
                channel_index: 4,
                dye_reference_index: 7656,
            },
        ],
        vec![],
        vec![],
    ]);
    let before = app.recipe.clone();
    for width in [480.0, 900.0, 1320.0] {
        let (output, overflow) = render(width, |ui| app.draw_translation_overrides(ui, None, None));
        assert!(overflow < 1.0, "Dye rows overflow at {width}: {overflow}");
        assert!(text(&output).contains("Disabled"));
        assert!(text(&output).contains("Base colors"));
        assert_eq!(app.recipe, before);
    }
}

#[test]
fn icon_rarity_follows_authored_tier_or_gameplay_donor_and_invalidates_cache() {
    use crate::AuthoredWeaponRarity as R;
    for (authored, inherited, expected) in [
        (None, Some(WeaponRarity::Common), Some(R::Common)),
        (None, Some(WeaponRarity::Uncommon), Some(R::Uncommon)),
        (None, Some(WeaponRarity::Rare), Some(R::Rare)),
        (None, Some(WeaponRarity::Legendary), Some(R::Legendary)),
        (None, Some(WeaponRarity::Exotic), Some(R::Exotic)),
        (
            Some(RecipeRarity::Legendary),
            Some(WeaponRarity::Exotic),
            Some(R::Legendary),
        ),
        (
            Some(RecipeRarity::Exotic),
            Some(WeaponRarity::Legendary),
            Some(R::Exotic),
        ),
        (Some(RecipeRarity::Rare), None, Some(R::Rare)),
        (None, Some(WeaponRarity::Unknown), None),
    ] {
        assert_eq!(effective_icon_rarity(authored, inherited), expected);
    }
    let legendary = AuthoredIconPreviewKey {
        item_hash: 1,
        container_tag: 2,
        rarity: R::Legendary,
        edit: Default::default(),
    };
    assert_ne!(
        legendary,
        AuthoredIconPreviewKey {
            rarity: R::Exotic,
            ..legendary.clone()
        }
    );
}

#[test]
fn constructing_the_editor_does_not_initialize_user_storage() {
    let app = PackageAuthoringApp::default();
    assert!(app.recipe_library.is_none());
    assert!(app.recipe_entries.is_empty());
    assert!(app.enabled_recipe_paths.is_empty());
    assert!(!app.recipe_dirty);
}

#[test]
fn preferences_pages_are_readable_and_keep_the_footer_visible() {
    for dark in [true, false] {
        for page in preferences_view::PreferencesPage::ALL {
            let mut app = PackageAuthoringApp {
                preferences_open: true,
                preferences_page: page,
                packages: PathBuf::from(format!(
                    "C:/very-long-installation/{}/packages",
                    "nested/".repeat(20)
                )),
                ..Default::default()
            };
            app.log
                .push(LogEntry::info("Test event belongs in the separate log"));
            let before = app.recipe.clone();
            for size in [egui::vec2(900.0, 640.0), egui::vec2(1320.0, 900.0)] {
                let ctx = egui::Context::default();
                ctx.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                let mut output = egui::FullOutput::default();
                for _ in 0..3 {
                    output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                            ..Default::default()
                        },
                        |ctx| app.draw_preferences_window(ctx),
                    );
                }
                let labels = text(&output);
                assert!(labels.contains("Parhelion Preferences"));
                assert!(labels.contains("Editor & Library") && labels.contains("Builds & Backups"));
                assert!(
                    !labels.contains("Test event belongs"),
                    "log must not be embedded in Preferences"
                );
                for label in ["Done", "Activity Log…", "Changes apply immediately."] {
                    let pos = text_origin(&output, label);
                    assert!(
                        pos.x < size.x - 10.0 && pos.y < size.y - 20.0,
                        "footer {label} at {pos:?}"
                    );
                }
                if page == preferences_view::PreferencesPage::EditorLibrary {
                    assert_body_label_readable(&output, "Shows detailed behavior");
                }
                assert_eq!(app.recipe, before);
                assert!(!app.preferences_changed);
                assert!(app.catalog_receiver.is_none());
            }
        }
    }
}

#[test]
fn activity_log_is_independent_read_only_and_copies_both_severities() {
    let mut app = PackageAuthoringApp {
        activity_log_open: true,
        ..Default::default()
    };
    app.log = ActivityLog::new(LogEntry::info("Build started"));
    app.log.push(LogEntry::error("Example build failure"));
    let before = app.recipe.clone();
    let (output, _) = render(900.0, |ui| {
        app.draw_preferences_window(ui.ctx());
        app.draw_activity_log_window(ui.ctx());
    });
    let labels = text(&output);
    assert!(labels.contains("Activity Log") && labels.contains("Copy Log"));
    assert!(!labels.contains("Parhelion Preferences"));
    assert!(labels.contains("[Error] Example build failure"));
    assert!(labels.contains("[Info] Build started"));
    let exported = app.activity_log_text();
    assert!(
        exported
            .lines()
            .next()
            .unwrap()
            .ends_with("[Error] Example build failure")
    );
    assert!(
        exported
            .lines()
            .nth(1)
            .unwrap()
            .ends_with("[Info] Build started")
    );
    assert!(labels.contains("Open Log Folder"));
    assert_eq!(app.recipe, before);
    assert_eq!(app.log.len(), 2);
}

#[test]
fn build_preferences_stay_locked_during_installation_review() {
    let mut app = PackageAuthoringApp {
        preferences_open: true,
        preferences_page: preferences_view::PreferencesPage::BuildsBackups,
        build_status_open: true,
        build_dialog_step: BuildDialogStep::ReviewInstall,
        ..Default::default()
    };
    let (output, _) = render(900.0, |ui| {
        ui.ctx().enable_accesskit();
        app.draw_preferences_window(ui.ctx());
    });
    assert!(text(&output).contains("locked during a package operation"));
    let tree = output.platform_output.accesskit_update.unwrap();
    for label in [
        "Keep last",
        "Include recipe snapshots in package backups",
        "Build from a temporary stock package view",
    ] {
        let node = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))
            .expect(label);
        assert!(node.1.is_disabled(), "{label} must be locked");
    }
    assert!(
        app.build_status_open,
        "reading preferences must not invalidate the staged review"
    );
}

#[test]
fn identity_groups_are_compact_aligned_and_read_only() {
    let mut app = PackageAuthoringApp::default();
    let before = app.recipe.clone();
    for width in [480.0, 800.0, 1320.0] {
        let (output, overflow) = render(width, |ui| app.draw_identity_workspace(ui));
        let labels = text(&output);
        assert!(labels.contains("Game Records") && labels.contains("Text References"));
        assert!(labels.contains("Build to allocate"));
        assert_eq!(labels.matches("Copy").count(), 13);
        assert!(overflow < 1.0, "Identity overflow at {width}: {overflow}");
        assert_eq!(app.recipe, before);
    }
}

#[test]
fn runtime_values_keep_a_useful_viewport_inside_the_split_page() {
    use sundial::package_authoring::weapon_runtime::{
        WeaponRuntimeOwner, WeaponRuntimeRoot, WeaponRuntimeRootKind,
    };
    let graph = WeaponRuntimeGraph {
        item_hash: 1,
        pattern_global_id_hash: 2,
        entity_tag: 3,
        bindings: Vec::new(),
        resources: Vec::new(),
        owners: (0..30)
            .map(|index| WeaponRuntimeOwner {
                anchor_binding_hash: index + 1,
                anchor_resource_index: 0,
                owner_tag: index + 1,
                roots: vec![WeaponRuntimeRoot {
                    kind: WeaponRuntimeRootKind::Instance,
                    schema: 1,
                    owner_offset: 0,
                    byte_size: 4,
                    generated_schema: true,
                    fields: vec![field(
                        WeaponRuntimeValueKind::Float32,
                        WeaponRuntimeValue::Float32Bits(1.0_f32.to_bits()),
                    )],
                }],
            })
            .collect(),
    };
    let mut app = PackageAuthoringApp::default();
    let (output, _) = render(1320.0, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(750.0, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    app.draw_runtime_values(ui, &graph);
                },
            );
        });
    });
    let ninth = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(value) if value.galley.job.text.contains("Binding 0x00000009") => {
                Some((clipped.clip_rect, value.pos))
            }
            _ => None,
        })
        .expect("ninth runtime row should be rendered");
    assert!(
        ninth.0.height() >= 300.0,
        "runtime viewport collapsed: {:?}",
        ninth.0
    );
    assert!(
        ninth.0.contains(ninth.1),
        "rows below the first two must be visible"
    );
}

#[test]
fn bundled_build_checkbox_toggles_all_defaults_without_touching_custom_selection() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut custom = WeaponRecipe::every_end();
    custom
        .rename_authored_item("Checkbox custom weapon")
        .unwrap();
    let custom_path = library.save_new(&custom).unwrap();
    let entries = library.scan().unwrap().entries;
    let mut selected = library.enabled_paths(&entries).unwrap();
    selected.insert(custom_path.clone());
    let before = selected.clone();
    let mut app = PackageAuthoringApp {
        recipe_library: Some(library.clone()),
        recipe_entries: entries,
        enabled_recipe_paths: selected,
        ..Default::default()
    };
    app.open_build_selection();
    app.build_selection_query = "no matching weapons".into();
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 760.0),
        )),
        ..Default::default()
    };
    for expected in [BTreeSet::from([custom_path]), before.clone()] {
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = ctx.run(input.clone(), |ctx| app.draw_library_windows(ctx));
        }
        let count = app
            .recipe_entries
            .iter()
            .filter(|entry| entry.bundled)
            .count();
        let included = app.build_selection_draft.as_ref().unwrap().len() - 1;
        let label = format!("Include default Parhelion weapons ({included}/{count})");
        let pos = text_origin(&output, &label) + egui::vec2(4.0, 4.0);
        for pressed in [true, false] {
            let mut click = input.clone();
            click.events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            let _ = ctx.run(click, |ctx| app.draw_library_windows(ctx));
        }
        assert_eq!(app.build_selection_draft.as_ref(), Some(&expected));
        assert_eq!(
            app.enabled_recipe_paths, before,
            "checkbox must remain transactional"
        );
    }
    app.apply_build_selection(BTreeSet::new()).unwrap();
    assert!(
        library
            .enabled_paths(&app.recipe_entries)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn recipe_toolbar_fits_without_exposing_secondary_actions_or_changing_the_draft() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = PackageAuthoringApp {
        recipe_library: Some(RecipeLibrary::open(directory.path().join("recipes")).unwrap()),
        ..Default::default()
    };
    app.recipe.name = "A deliberately long weapon name that must not stretch the toolbar".into();
    let before = app.recipe.clone();
    for width in [480.0, 900.0, 1320.0] {
        let (output, overflow) = render(width, |ui| {
            assert!(!app.draw_recipe_library(ui));
        });
        let labels = text(&output);
        assert!(labels.contains("Library…"));
        assert!(labels.contains("Recipe…"));
        for secondary in ["Duplicate", "Discard Changes", "Import…", "Export…"] {
            assert!(!labels.contains(secondary), "{secondary} escaped its menu");
        }
        assert!(
            overflow <= 1.0,
            "toolbar width {width}: overflow {overflow}"
        );
        assert_eq!(app.recipe, before);
    }
}

#[test]
fn base_perk_projection_warning_preserves_authored_rows_and_fits() {
    for width in [480.0, 1000.0] {
        for count in [4_u16, 5] {
            let mut app = PackageAuthoringApp::default();
            app.recipe.overrides.base_sandbox_perks = Some((0..count).collect());
            let before = app.recipe.clone();
            let (output, overflow) = render(width, |ui| app.draw_base_sandbox_perks(ui, None));
            assert_eq!(text(&output).contains("Sunrise compatibility"), count > 4);
            assert!(text(&output).contains("16 entries total"));
            assert!(overflow <= 1.0, "{width}px overflow: {overflow}");
            assert_eq!(app.recipe, before, "warnings must not trim or reset perks");
        }
    }
}

#[test]
fn build_pages_replace_each_other_and_selection_precedes_build() {
    let mut app = PackageAuthoringApp {
        build_status_open: true,
        latest_build: Some(Ok(BuildReport {
            weapons: Vec::new(),
            run_directory: PathBuf::from("staged-run"),
            manifest_path: PathBuf::from("staged-run/manifest.json"),
            artifacts: Vec::new(),
            selection_fingerprint: "fingerprint".into(),
            staged_recipe_paths: Vec::new(),
        })),
        ..Default::default()
    };
    let before = app.recipe.clone();
    let (output, _) = render(1320.0, |ui| app.draw_actions(ui));
    assert!(
        text_origin(&output, "0 weapons selected for build…").x
            < text_origin(&output, "Build & Stage").x
    );
    let action_y = text_origin(&output, "Build & Stage").y;
    for label in ["0 weapons selected for build…", "Build & Install Status…"] {
        assert!((text_origin(&output, label).y - action_y).abs() < 1.0);
    }
    for step in [
        BuildDialogStep::Build,
        BuildDialogStep::ReviewInstall,
        BuildDialogStep::Install,
    ] {
        app.build_dialog_step = step;
        let (output, _) = render(1320.0, |ui| app.draw_build_status_window(ui.ctx()));
        let labels = text(&output);
        assert_eq!(labels.matches("Build & Install").count(), 1);
        assert_eq!(
            labels.contains("Build Validated"),
            step == BuildDialogStep::Build
        );
        assert_eq!(
            labels.contains("Review Installation"),
            step != BuildDialogStep::Install
        );
        assert_eq!(
            labels.contains("This replaces your installed custom weapon set."),
            step == BuildDialogStep::ReviewInstall
        );
        assert_eq!(
            labels.contains("No installation result"),
            step == BuildDialogStep::Install
        );
        assert_eq!(app.recipe, before);
        assert!(app.install_receiver.is_none());
    }
}

#[test]
fn library_rows_show_authored_metadata_and_search_it_without_changing_selection() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut app = PackageAuthoringApp {
        recipe_entries: library.scan().unwrap().entries,
        library_open: true,
        ..Default::default()
    };
    let entry = &mut app.recipe_entries[0];
    entry.name = "Library test weapon".into();
    entry.type_name = Some("Micro-Missile Shotgun".into());
    entry.ammo_type = Some(RecipeAmmoType::Special);
    entry.damage_type = Some(crate::recipe::RecipeDamageType::Arc);
    entry.rarity = Some(RecipeRarity::Legendary);
    app.recipe_path = Some(entry.path.clone());
    let before = app.recipe.clone();
    app.library_query = "  test ARC   special micro-missile  ".into();
    let (output, _) = render(900.0, |ui| app.draw_library_windows(ui.ctx()));
    let labels = text(&output);
    assert!(labels.contains("Library test weapon"));
    assert!(labels.contains("Open"));
    assert!(labels.contains("Built-in"));
    assert!(labels.contains("Micro-Missile Shotgun"));
    assert!(labels.contains("Arc · Special"));
    assert!(
        (text_origin(&output, "Library test weapon").x
            - text_origin(&output, "Micro-Missile Shotgun · Legendary · Arc · Special").x)
            .abs()
            < 1.0
    );
    assert!(labels.contains("1 recipe ·"));
    assert_eq!(app.recipe, before);
    assert!(app.enabled_recipe_paths.is_empty());
}

#[test]
fn library_restore_confirmation_is_read_only_and_refresh_preserves_open_edits() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let entries = library.scan().unwrap().entries;
    let path = entries[0].path.clone();
    let original = std::fs::read(&path).unwrap();
    let mut app = PackageAuthoringApp {
        recipe_entries: entries,
        library_open: true,
        recipe_library: Some(library.clone()),
        ..Default::default()
    };
    let (output, _) = render(720.0, |ui| app.draw_library_windows(ui.ctx()));
    assert!(text(&output).contains("Refresh"));
    assert!(text(&output).contains("Restore Default Recipes…"));
    app.restore_defaults_preview = Some(library.prepare_restore_defaults().unwrap());
    let (output, _) = render(720.0, |ui| app.draw_library_windows(ui.ctx()));
    assert!(text(&output).contains("Restore Default Recipes?"));
    assert!(text(&output).contains("Cancel"));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    app.recipe.name = "Unsaved edit".into();
    app.recipe_dirty = true;
    let before = app.recipe.clone();
    app.refresh_recipe_library();
    assert_eq!(app.recipe, before);
    assert!(app.recipe_dirty);
}

#[test]
fn clicking_library_name_opens_the_recipe_instead_of_selecting_text() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let entries = library.scan().unwrap().entries;
    let path = entries[0].path.clone();
    let name = entries[0].name.clone();
    let mut app = PackageAuthoringApp {
        recipe_entries: entries,
        library_open: true,
        ..Default::default()
    };
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 1000.0),
        )),
        ..Default::default()
    };
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = ctx.run(input.clone(), |ctx| app.draw_library_windows(ctx));
    }
    let pos = text_origin(&output, &name) + egui::vec2(5.0, 5.0);
    for pressed in [true, false] {
        let mut click = input.clone();
        click.events = vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        let _ = ctx.run(click, |ctx| app.draw_library_windows(ctx));
    }
    assert_eq!(app.recipe_path.as_ref(), Some(&path));
    assert!(!app.library_open);
    assert!(app.enabled_recipe_paths.is_empty());
    assert!(!app.recipe_dirty);
}

#[test]
fn filtered_build_selection_changes_only_the_draft_and_cancel_discards_it() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let entries = library.scan().unwrap().entries;
    let shown = entries[0].path.clone();
    let hidden = entries[1].path.clone();
    let name = entries[0].name.clone();
    let mut app = PackageAuthoringApp {
        recipe_entries: entries,
        enabled_recipe_paths: BTreeSet::from([hidden.clone()]),
        ..Default::default()
    };
    app.open_build_selection();
    app.build_selection_query = name.to_lowercase();
    let before = app.recipe.clone();
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(902.0, 760.0),
        )),
        ..Default::default()
    };
    let click_label = |app: &mut PackageAuthoringApp, label: &str| {
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = ctx.run(input.clone(), |ctx| app.draw_library_windows(ctx));
        }
        let pos = text_origin(&output, label) + egui::vec2(5.0, 5.0);
        assert!(pos.y < 760.0, "{label} must remain reachable");
        for pressed in [true, false] {
            let mut click = input.clone();
            click.events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            let _ = ctx.run(click, |ctx| app.draw_library_windows(ctx));
        }
    };
    click_label(&mut app, "Select Shown");
    assert_eq!(
        app.build_selection_draft.as_ref().unwrap(),
        &BTreeSet::from([shown.clone(), hidden.clone()])
    );
    click_label(&mut app, "Clear Shown");
    assert_eq!(
        app.build_selection_draft.as_ref().unwrap(),
        &BTreeSet::from([hidden.clone()])
    );
    click_label(&mut app, &name);
    assert!(app.build_selection_draft.as_ref().unwrap().contains(&shown));
    assert_eq!(app.enabled_recipe_paths, BTreeSet::from([hidden]));
    click_label(&mut app, "Cancel");
    assert!(app.build_selection_draft.is_none());
    assert_eq!(app.recipe, before);
    app.open_build_selection();
    assert!(app.build_selection_query.is_empty());
    assert_eq!(
        app.build_selection_draft.as_ref(),
        Some(&app.enabled_recipe_paths)
    );
}

#[test]
fn incomplete_name_edits_are_retained_but_cannot_be_saved_or_built() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let path = library.scan().unwrap().entries[0].path.clone();
    let mut app = PackageAuthoringApp {
        recipe_library: Some(library),
        ..Default::default()
    };
    assert!(app.open_recipe_path(&path));
    app.enabled_recipe_paths.insert(path.clone());
    let original = app.recipe.clone();
    let bytes = std::fs::read(&path).unwrap();
    app.edit_weapon_name(String::new());
    assert_eq!(app.invalid_weapon_name.as_ref().unwrap().0, "");
    assert_eq!(
        app.recipe, original,
        "an intermediate edit must not corrupt identities"
    );
    assert!(app.recipe_dirty);
    let (output, overflow) = render(480.0, |ui| app.draw_weapon_name(ui));
    assert!(text(&output).contains("Finish editing the weapon name"));
    assert!(overflow <= 1.0);
    let library = app.recipe_library.clone().unwrap();
    let (output, _) = render(1320.0, |ui| {
        ui.ctx().enable_accesskit();
        app.draw_recipe_library_primary(ui, &library, &mut false);
    });
    let tree = output.platform_output.accesskit_update.as_ref().unwrap();
    let save = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Save Changes"))
        .expect("Save button is exposed");
    assert!(
        save.1.is_disabled(),
        "invalid names must visibly disable Save"
    );
    app.save_library_recipe();
    app.save_recipe_copy();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(!app.duplicate_recipe());
    assert!(app.batch_request().unwrap_err().contains("Finish editing"));
}

#[test]
fn incomplete_name_edits_are_guarded_and_can_be_discarded_or_completed() {
    let mut app = PackageAuthoringApp::default();
    let original = app.recipe.clone();
    app.edit_weapon_name(String::new());
    assert!(!app.request_recipe_action(PendingRecipeAction::New));
    assert!(app.pending_recipe_action.is_some());
    app.discard_recipe_changes();
    assert!(app.invalid_weapon_name.is_none());
    assert!(!app.recipe_dirty);
    assert!(app.batch_request().is_ok());
    app.edit_weapon_name(String::new());
    app.edit_weapon_name("A Different Tomorrow".into());
    assert!(app.invalid_weapon_name.is_none());
    assert_eq!(app.recipe.name, "A Different Tomorrow");
    assert!(app.recipe.identity_is_name_derived());
    assert_ne!(app.recipe.identity, original.identity);
}

#[test]
fn core_fields_and_build_selection_expose_accessible_names() {
    let mut app = PackageAuthoringApp::default();
    let (output, _) = render(900.0, |ui| {
        ui.ctx().enable_accesskit();
        app.draw_definition_panel(ui, None);
    });
    let tree = output.platform_output.accesskit_update.unwrap();
    for label in ["Weapon Name", "Flavor Text", "Rarity", "Power Cap"] {
        let ids: Vec<_> = tree
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some(label) || node.value() == Some(label))
            .map(|(id, _)| *id)
            .collect();
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.labelled_by().iter().any(|id| ids.contains(id))),
            "{label} must label its input"
        );
    }
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    app.recipe_entries = library.scan().unwrap().entries;
    app.open_build_selection();
    let (output, _) = render(900.0, |ui| {
        ui.ctx().enable_accesskit();
        app.draw_library_windows(ui.ctx());
    });
    let tree = output.platform_output.accesskit_update.unwrap();
    assert!(
        tree.nodes
            .iter()
            .any(|(_, node)| node.label() == Some("Search weapons for this build"))
    );
    let checkboxes: Vec<_> = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == egui::accesskit::Role::CheckBox)
        .collect();
    assert!(!checkboxes.is_empty());
    assert!(checkboxes.iter().all(|(_, node)| {
        node.label().is_some_and(|label| {
            label.starts_with("Include default Parhelion weapons")
                || (label.starts_with("Include ") && label.contains("in this build"))
        })
    }));
}

#[test]
fn build_selection_is_keyboard_operable_without_duplicate_row_tab_stops() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let entries = library.scan().unwrap().entries;
    let path = entries[0].path.clone();
    let name = entries[0].name.clone();
    let mut app = PackageAuthoringApp {
        recipe_entries: entries,
        ..Default::default()
    };
    app.open_build_selection();
    app.build_selection_query = name.clone();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 640.0),
        )),
        ..Default::default()
    };
    let frame = |app: &mut PackageAuthoringApp, key: Option<egui::Key>| {
        let mut input = input.clone();
        if let Some(key) = key {
            input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        ctx.run(input, |ctx| app.draw_library_windows(ctx))
    };
    let focused_label = |output: &egui::FullOutput| {
        let tree = output.platform_output.accesskit_update.as_ref().unwrap();
        tree.nodes
            .iter()
            .find(|(id, _)| *id == tree.focus)
            .and_then(|(_, node)| node.label())
            .unwrap_or_default()
            .to_owned()
    };
    for _ in 0..3 {
        frame(&mut app, None);
    }
    let mut reached = false;
    for _ in 0..8 {
        let output = frame(&mut app, Some(egui::Key::Tab));
        if focused_label(&output).starts_with(&format!("Include {name} in this build")) {
            reached = true;
            break;
        }
    }
    assert!(reached, "the inclusion checkbox must be keyboard reachable");
    frame(&mut app, Some(egui::Key::Space));
    assert_eq!(
        app.build_selection_draft.as_ref().unwrap(),
        &BTreeSet::from([path])
    );
    let output = frame(&mut app, Some(egui::Key::Tab));
    assert_eq!(
        focused_label(&output),
        "Apply Selection",
        "one tab stop per weapon"
    );
    frame(&mut app, Some(egui::Key::Escape));
    assert!(app.build_selection_draft.is_none());
    assert!(app.enabled_recipe_paths.is_empty());
}

#[test]
fn workbench_tabs_keep_recipe_and_build_selection_unchanged() {
    let mut app = PackageAuthoringApp::default();
    let before = app.recipe.clone();
    let selected = app.enabled_recipe_paths.clone();
    assert_eq!(app.workbench_page, WorkbenchPage::Weapon);
    assert_eq!(
        WorkbenchPage::ALL.map(WorkbenchPage::label),
        ["Weapon", "Appearance", "Advanced Gameplay", "Identity"]
    );
    for page in WorkbenchPage::ALL {
        app.workbench_page = page;
        let (_, overflow) = render(900.0, |ui| {
            app.draw_workbench_tabs(ui);
            app.draw_recipe_editor(ui);
        });
        assert!(overflow < 1.0, "{page:?} overflow: {overflow}");
        assert_eq!(app.recipe, before);
        assert_eq!(app.enabled_recipe_paths, selected);
    }
}

#[test]
fn replacing_recipe_closes_private_window_and_returns_to_weapon_page() {
    let mut app = PackageAuthoringApp {
        workbench_page: WorkbenchPage::Appearance,
        private_perk_socket: Some(4),
        ..Default::default()
    };
    app.clear_dependent_picker_queries();
    assert!(app.private_perk_socket.is_none());
    assert_eq!(app.workbench_page, WorkbenchPage::Weapon);
}

#[test]
fn recipe_changes_invalidate_results_even_while_already_dirty() {
    let mut app = PackageAuthoringApp {
        build_status_open: true,
        ..Default::default()
    };
    app.synchronize_recipe_dirty();
    assert!(
        app.build_status_open,
        "an unchanged frame preserves results"
    );

    app.recipe.flavor.push_str(" first edit");
    app.synchronize_recipe_dirty();
    assert!(app.recipe_dirty);
    assert!(!app.build_status_open);

    app.build_status_open = true;
    app.build_dialog_step = BuildDialogStep::ReviewInstall;
    app.recipe.flavor.push_str(" second edit");
    app.synchronize_recipe_dirty();
    assert!(app.recipe_dirty);
    assert!(!app.build_status_open);
    assert_eq!(app.build_dialog_step, BuildDialogStep::Build);

    app.build_status_open = true;
    app.synchronize_recipe_dirty();
    assert!(app.build_status_open, "observing the same edit is a no-op");
}

#[test]
fn restoring_original_content_clears_dirty_but_unsaved_copies_stay_protected() {
    let mut app = PackageAuthoringApp::default();
    app.observed_recipe = app.recipe.clone();
    app.recipe.flavor.push_str(" changed");
    app.synchronize_recipe_dirty();
    assert!(app.recipe_dirty);
    app.recipe = app.recipe_baseline.clone();
    app.synchronize_recipe_dirty();
    assert!(!app.recipe_dirty);
    assert!(app.duplicate_recipe());
    app.synchronize_recipe_dirty();
    assert!(app.recipe_dirty);
    app.discard_recipe_changes();
    assert!(
        app.recipe_dirty,
        "an unsaved copy must still require a save or discard on close"
    );
}

#[test]
fn reopening_build_selection_preserves_its_draft_and_error() {
    let mut app = PackageAuthoringApp::default();
    app.enabled_recipe_paths.insert("first.json".into());
    app.open_build_selection();
    app.build_selection_draft.as_mut().unwrap().clear();
    app.build_selection_error = Some("Write failed".into());
    app.open_build_selection();
    assert!(app.build_selection_draft.as_ref().unwrap().is_empty());
    assert_eq!(app.build_selection_error.as_deref(), Some("Write failed"));
    assert_eq!(app.enabled_recipe_paths.len(), 1);
    assert!(!app.current_recipe_is_in_build());
    app.recipe_path = Some("first.json".into());
    assert!(app.current_recipe_is_in_build());
}

#[test]
fn save_conflicts_keep_both_versions_and_remain_visible_outside_settings() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut app = PackageAuthoringApp::default();
    let path = library.root().join("every-end.parhelion.json");
    assert!(app.open_recipe_path(&path));
    let mut external = app.recipe.clone();
    external.flavor = "Changed outside the workbench".into();
    library.save_existing(&path, &external).unwrap();
    app.recipe.flavor = "My unsaved draft".into();
    app.synchronize_recipe_dirty();
    app.recipe_library = Some(library);
    app.save_library_recipe();
    assert_eq!(WeaponRecipe::load_json(&path).unwrap(), external);
    assert_eq!(app.recipe.flavor, "My unsaved draft");
    assert!(app.recipe_dirty);
    app.log.push(LogEntry::info("A later background event"));
    let (output, overflow) = render(480.0, |ui| app.draw_action_error(ui));
    assert!(text(&output).contains("changed on disk"));
    assert!(overflow <= 1.0, "save error overflow: {overflow}");
}

#[test]
fn build_checks_external_changes_without_replacing_the_open_draft() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut app = PackageAuthoringApp::default();
    let path = library.root().join("every-end.parhelion.json");
    assert!(app.open_recipe_path(&path));
    app.enabled_recipe_paths.insert(path.clone());
    assert!(app.batch_request().is_ok());
    let mut external = app.recipe.clone();
    external.flavor = "External change".into();
    library.save_existing(&path, &external).unwrap();
    assert!(app.batch_request().unwrap_err().contains("changed on disk"));
    assert_ne!(app.recipe.flavor, external.flavor);
}

#[test]
fn guarded_save_accepts_formatting_changes_but_refuses_missing_sources() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let path = library.root().join("every-end.parhelion.json");
    let baseline = WeaponRecipe::load_json(&path).unwrap();
    std::fs::write(&path, serde_json::to_vec(&baseline).unwrap()).unwrap();
    let mut edited = baseline.clone();
    edited.flavor = "My safe edit".into();
    library
        .save_existing_if_unchanged(&path, &baseline, &edited)
        .unwrap();
    assert_eq!(WeaponRecipe::load_json(&path).unwrap(), edited);
    std::fs::remove_file(&path).unwrap();
    assert!(
        library
            .save_existing_if_unchanged(&path, &edited, &baseline)
            .is_err()
    );
    assert!(
        !path.exists(),
        "a missing source must not be silently recreated"
    );
}

#[test]
fn build_scope_warnings_are_visible_and_fit_narrow_windows() {
    let mut app = PackageAuthoringApp::default();
    app.enabled_recipe_paths.insert("included.json".into());
    let (output, overflow) = render(480.0, |ui| app.draw_actions(ui));
    assert!(text(&output).contains("not saved or included"));
    assert!(overflow <= 1.0, "unsaved footer overflow: {overflow}");
    app.recipe_path = Some("excluded.json".into());
    let (output, overflow) = render(480.0, |ui| app.draw_actions(ui));
    assert!(text(&output).contains("not included in this build"));
    assert!(overflow <= 1.0, "excluded footer overflow: {overflow}");
}

#[test]
fn escape_discards_build_selection_without_changing_the_document() {
    let mut app = PackageAuthoringApp::default();
    let selected = BTreeSet::from([PathBuf::from("saved.parhelion.json")]);
    app.enabled_recipe_paths = selected.clone();
    app.build_selection_draft = Some(BTreeSet::new());
    let before = app.recipe.clone();
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| app.draw_library_windows(ctx));
    assert!(app.build_selection_draft.is_none());
    assert_eq!(app.enabled_recipe_paths, selected);
    assert_eq!(app.recipe.clone(), before);
}

#[test]
fn build_selection_is_explicit_and_failed_commits_do_not_change_it() {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let mut app = PackageAuthoringApp::default();
    app.recipe_entries = library.scan().unwrap().entries;
    app.enabled_recipe_paths = library.enabled_paths(&app.recipe_entries).unwrap();
    app.recipe_library = Some(library);
    let before = app.enabled_recipe_paths.clone();
    let recipe_before = app.recipe.clone();
    app.build_selection_draft = Some(BTreeSet::new());
    assert_eq!(
        app.enabled_recipe_paths, before,
        "draft changes must not commit"
    );
    app.build_selection_draft = None; // Cancel/close.
    assert_eq!(app.enabled_recipe_paths, before);
    assert!(
        app.apply_build_selection([directory.path().join("outside.json")].into())
            .is_err()
    );
    assert_eq!(app.enabled_recipe_paths, before);
    app.apply_build_selection(BTreeSet::new()).unwrap();
    assert!(app.enabled_recipe_paths.is_empty());
    assert!(
        app.recipe_library
            .as_ref()
            .unwrap()
            .enabled_paths(&app.recipe_entries)
            .unwrap()
            .is_empty()
    );
    assert_eq!(app.recipe.clone(), recipe_before);
}

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
        name: "Runtime test field".into(),
        path_label: "Component / Runtime test field".into(),
        kind, value,
        source: WeaponRuntimeFieldSource::GeneratedSchema,
        generated_kind: None,
    }
}

fn assert_body_label_readable(output: &egui::FullOutput, prefix: &str) {
    let label = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(value) if value.galley.job.text.starts_with(prefix) => Some(value),
            _ => None,
        })
        .expect("body explanation must be visible");
    assert!(
        label
            .galley
            .job
            .sections
            .iter()
            .all(|section| section.format.font_id.size >= 12.5)
    );
}

fn render(width: f32, mut draw: impl FnMut(&mut egui::Ui)) -> (egui::FullOutput, f32) {
    let ctx = egui::Context::default();
    let mut overflow = 0.0_f32;
    let mut output = egui::FullOutput::default();
    for _ in 0..2 {
        output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 1600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    workbench_style(ui);
                    let right = ui.max_rect().right();
                    draw(ui);
                    overflow = (ui.min_rect().right() - right).max(0.0);
                });
            },
        );
    }
    (output, overflow)
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

fn text_origin(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    text_origins(output, label)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("Missing rendered label: {label}"))
}

fn text_origins(output: &egui::FullOutput, label: &str) -> Vec<egui::Pos2> {
    fn find(shape: &egui::Shape, label: &str, positions: &mut Vec<egui::Pos2>) {
        match shape {
            egui::Shape::Text(value) if value.galley.job.text == label => positions.push(value.pos),
            egui::Shape::Vec(values) => {
                for value in values {
                    find(value, label, positions);
                }
            }
            _ => {}
        }
    }
    let mut positions = Vec::new();
    for shape in &output.shapes {
        find(&shape.shape, label, &mut positions);
    }
    positions
}

#[test]
fn socket_options_plug_safety_selection_survives_menu_close() {
    fn frame(
        ctx: &egui::Context,
        app: &mut PackageAuthoringApp,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 640.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    workbench_style(ui);
                    ui.horizontal(|ui| app.draw_socket_options(ui, false));
                });
            },
        )
    }
    fn click(ctx: &egui::Context, app: &mut PackageAuthoringApp, pos: egui::Pos2) {
        for pressed in [true, false] {
            frame(
                ctx,
                app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    let ctx = egui::Context::default();
    let mut app = PackageAuthoringApp {
        plug_selection_mode: PlugSelectionMode::SocketAndGearType,
        ..Default::default()
    };
    let before = app.recipe.clone();
    for target in [PlugSelectionMode::AnyPlug, PlugSelectionMode::Supported] {
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        click(
            &ctx,
            &mut app,
            text_origin(&output, "Socket Options") + egui::vec2(8.0, 6.0),
        );
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        let current = app.plug_selection_mode.label();
        click(
            &ctx,
            &mut app,
            text_origin(&output, current) + egui::vec2(8.0, 6.0),
        );
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        click(
            &ctx,
            &mut app,
            text_origin(&output, target.label()) + egui::vec2(8.0, 6.0),
        );
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        assert_eq!(app.plug_selection_mode, target);
        assert_eq!(app.recipe, before);
    }
}

#[test]
fn technical_visibility_does_not_restrict_workbench_plug_combinations() {
    for mode in PlugSelectionMode::ALL {
        let mut app = PackageAuthoringApp {
            plug_selection_mode: mode,
            ..Default::default()
        };
        app.set_show_experimental_options(true);
        app.set_show_experimental_options(false);
        assert_eq!(app.plug_selection_mode, mode);
    }
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
        app.advanced_gameplay_page = AdvancedGameplayPage::PerksTraits;
        let (output, overflow) = render(width, |ui| app.draw_gameplay_workspace(ui, None));
        let labels = text(&output);
        assert!(labels.contains("Firing & Runtime Baseline"));
        assert!(labels.contains("experimental"));
        assert!(!labels.contains("Runtime Component Donors"));
        assert!(!labels.contains("Perks & Traits"));
        assert!(overflow < 1.0, "width {width}: {overflow}");
        assert_eq!(app.advanced_gameplay_page, AdvancedGameplayPage::Runtime);
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
    assert!(text(&output).contains("Runtime Component Donors"));
    assert!(text(&output).contains("Perks & Traits"));
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
        assert!(text(&output).contains("not applied"), "{:?}", field.kind);
        assert!(
            saved.is_empty(),
            "invalid draft must not replace the last valid recipe value"
        );
    }
}

#[test]
fn runtime_bytes_and_reset_fit_a_narrow_editor() {
    let field = field(
        WeaponRuntimeValueKind::FixedBytes { size: 128 },
        WeaponRuntimeValue::Bytes(vec![0xFF; 128]),
    );
    for width in [280.0, 480.0, 900.0] {
        let mut saved = vec![WeaponRuntimeValueOverride {
            locator: field.locator.clone(),
            value: field.value.clone(),
        }];
        let mut drafts = BTreeMap::new();
        let (output, overflow) = render(width, |ui| {
            draw_runtime_value_override_field(ui, &field, &mut saved, &mut drafts)
        });
        assert!(overflow < 1.0, "width={width}, overflow={overflow}");
        assert!(text(&output).contains("Reset to donor"));
        assert_eq!(saved.len(), 1);
    }
}

#[test]
fn definition_layout_fits_and_drawing_never_changes_a_recipe() {
    for width in [420.0, 640.0, 900.0, 1280.0] {
        let mut app = PackageAuthoringApp::default();
        app.recipe.overrides.ammo_type = Some(RecipeAmmoType::Special);
        let before = app.recipe.clone();
        let (_, overflow) = render(width, |ui| {
            app.draw_definition_panel(ui, None);
        });
        assert!(overflow < 1.0, "width={width}, overflow={overflow}");
        assert_eq!(app.recipe.clone(), before);
    }
}

#[test]
fn ammo_selection_without_a_native_donor_is_read_only_and_preserves_saved_data() {
    let mut overrides = WeaponRecipeOverrides {
        ammo_type: Some(RecipeAmmoType::Special),
        ..Default::default()
    };
    let (output, _) = render(420.0, |ui| draw_ammo_type_control(ui, &mut overrides, None));
    assert!(text(&output).contains("Choose a gameplay donor"));
    assert_eq!(overrides.ammo_type, Some(RecipeAmmoType::Special));
}

#[test]
fn a_private_clone_without_value_edits_still_exists_and_can_be_removed() {
    let mut recipe = WeaponRecipe::every_end();
    let key = PerkEditorKey {
        socket_index: 0,
        choice_index: 0,
        source_plug_hash: 10,
        source_perk_index: 1178,
    };
    upsert_private_perk_runtime_values(&mut recipe, key, vec![]);
    assert_eq!(private_perk_runtime_values(&recipe, key), Some(&vec![]));
    remove_private_perk_runtime_values(&mut recipe, key);
    assert!(private_perk_runtime_values(&recipe, key).is_none());
}

#[test]
fn duplicate_preserves_draft_mechanics_and_allocates_a_fresh_identity() {
    let mut app = PackageAuthoringApp {
        recipe: WeaponRecipe::from_json_str(include_str!(
            "../../recipes/breach-notice.parhelion.json"
        ))
        .unwrap(),
        ..Default::default()
    };
    let original = app.recipe.clone();
    app.recipe_path = Some(PathBuf::from("original.parhelion.json"));
    app.recipe_entries.clear();
    assert!(app.duplicate_recipe());
    assert_ne!(app.recipe.namespace, original.namespace);
    assert_ne!(app.recipe.identity, original.identity);
    assert_eq!(app.recipe.donor, original.donor);
    assert_eq!(app.recipe.presentation_donor, original.presentation_donor);
    assert_eq!(app.recipe.overrides, original.overrides);
    assert!(app.recipe_path.is_none());
    let first_copy = app.recipe.clone();
    app.recipe_entries.push(RecipeLibraryEntry {
        path: PathBuf::from("existing-copy.parhelion.json"),
        name: first_copy.name.clone(),
        namespace: first_copy.namespace.clone(),
        bundled: false,
        donor_hash: first_copy.donor.item_hash.parse_u32().unwrap(),
        type_name: first_copy.type_name.clone(),
        ammo_type: first_copy.overrides.ammo_type,
        damage_type: first_copy.overrides.modern_damage_type,
        rarity: first_copy.overrides.rarity,
        icon_hash: first_copy.donor.item_hash.parse_u32().unwrap(),
        icon_edit: first_copy.overrides.icon_edit.clone(),
    });
    app.recipe = original;
    assert!(app.duplicate_recipe());
    assert_ne!(app.recipe.namespace, first_copy.namespace);
    assert!(app.recipe.name.ends_with("Copy 2"));
}

#[test]
fn opening_a_recipe_after_the_main_panel_does_not_mark_it_modified() {
    let mut app = PackageAuthoringApp::default();
    app.recipe_library = None;
    app.catalog_load_requested = true;
    // A modal opens the recipe after that frame's normal recipe-change check.
    app.recipe = WeaponRecipe::every_end();
    app.recipe_baseline = app.recipe.clone();
    app.recipe_path = Some(PathBuf::from("saved.parhelion.json"));
    app.recipe_dirty = false;
    let ctx = egui::Context::default();
    let frame = |app: &mut PackageAuthoringApp| {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1320.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| app.update_ui(ctx),
        );
    };
    frame(&mut app);
    assert!(!app.recipe_dirty);
    app.recipe.flavor.push_str(" Edited.");
    frame(&mut app);
    assert!(
        app.recipe_dirty,
        "real edits must still trigger save/discard protection"
    );
}

#[test]
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES; package-backed headless layout check"]
#[expect(
    clippy::cognitive_complexity,
    reason = "Integration matrix keeps per-recipe rendering and mutation assertions together"
)]
fn real_workbench_socket_layout_is_read_only_and_fits() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap());
    let mut app = PackageAuthoringApp::default();
    let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
    app.donor_summaries = catalog.weapon_donors();
    app.sandbox_perk_choices = catalog.weapon_sandbox_perk_choices();
    app.catalog = Some(catalog);
    app.packages = packages;
    app.show_experimental_options = false;
    app.recipe = WeaponRecipe::every_end();
    let donor = app.current_donor().unwrap();
    let catalog = app.catalog.as_ref().unwrap();
    assert_eq!(
        socket_editor::socket_role_label(catalog, &donor, 0, None),
        donor.sockets[0].label
    );
    assert_eq!(
        socket_editor::socket_role_label(catalog, &donor, 0, Some(176)),
        "1. Intrinsic"
    );
    assert_eq!(
        socket_editor::socket_role_label(catalog, &donor, 0, Some(92)),
        "1. Trait"
    );
    let pellet = app
        .donor_summaries
        .iter()
        .find(|donor| donor.hash == 0xEFA7_A89F)
        .unwrap();
    let slug = app
        .donor_summaries
        .iter()
        .find(|donor| donor.name == "First In, Last Out")
        .unwrap();
    let horseman = app
        .donor_summaries
        .iter()
        .find(|donor| donor.hash == 0x634C_6957)
        .unwrap();
    assert!(presentation_donor_candidate_is_compatible(
        horseman,
        pellet,
        WeaponInventorySlot::Energy
    ));
    assert_eq!(
        crate::capabilities::appearance_compatibility(horseman, slug, WeaponInventorySlot::Energy),
        crate::capabilities::AppearanceCompatibility::Blocked("Incompatible weapon animations")
    );
    for (name, hash, donor_name, damage) in [
        (
            "Arc in Kinetic",
            0x4CE3_CE93,
            "Breachlight",
            crate::recipe::RecipeDamageType::Arc,
        ),
        (
            "Kinetic in Energy",
            0xA25B_8F8F,
            "Arc Logic",
            crate::recipe::RecipeDamageType::Kinetic,
        ),
    ] {
        app.recipe = WeaponRecipe::new_named_weapon_for_donor(name, hash, donor_name).unwrap();
        app.recipe.overrides.modern_damage_type = Some(damage);
        let donor = app.current_donor().unwrap();
        let before = app.recipe.clone();
        for width in [480.0, 900.0, 1320.0] {
            let (output, overflow) = render(width, |ui| {
                draw_combat_profile_control(ui, &mut app.recipe.overrides, Some(&donor), true);
                draw_combat_profile_control(ui, &mut app.recipe.overrides, Some(&donor), false);
                draw_combat_profile_diagnostics(ui, &app.recipe.overrides, Some(&donor));
            });
            assert!(text(&output).contains("Experimental slot and damage combination"));
            assert!(!text(&output).contains("Reset it before building"));
            assert!(overflow <= 1.0, "{name} at {width}: overflow {overflow}");
            assert_eq!(app.recipe, before);
        }
    }
    for (_, json) in crate::recipe_library::BUNDLED_RECIPES.iter().skip(2) {
        app.recipe = WeaponRecipe::from_json_str(json).unwrap();
        let donor = app.current_donor().unwrap();
        let before = app.recipe.clone();
        let (profile, _) = render(900.0, |ui| {
            draw_combat_profile_control(ui, &mut app.recipe.overrides, Some(&donor), true);
            draw_combat_profile_control(ui, &mut app.recipe.overrides, Some(&donor), false);
            draw_combat_profile_diagnostics(ui, &app.recipe.overrides, Some(&donor));
        });
        assert!(
            !text(&profile).contains("Unsupported recipe combination"),
            "{} profile",
            app.recipe.name
        );
        assert!(
            !text(&profile).contains("Reset it before building"),
            "{} profile",
            app.recipe.name
        );
        assert_eq!(app.recipe.clone(), before);
        for width in [480.0, 860.0, 1280.0] {
            let (output, overflow) =
                render(width, |ui| app.draw_socket_columns_panel(ui, Some(&donor)));
            if overflow >= 1.0 {
                fn outside(shape: &egui::Shape, width: f32) {
                    match shape {
                        egui::Shape::Text(value)
                            if value.pos.x + value.galley.rect.right() > width - 8.0 =>
                        {
                            eprintln!(
                                "overflow text {:?} at {}",
                                value.galley.job.text,
                                value.pos.x + value.galley.rect.right()
                            );
                        }
                        egui::Shape::Vec(values) => {
                            for value in values {
                                outside(value, width);
                            }
                        }
                        _ => {}
                    }
                }
                for shape in &output.shapes {
                    outside(&shape.shape, width);
                }
            }
            assert!(
                overflow < 1.0,
                "{} width={width}, overflow={overflow}",
                app.recipe.name
            );
            assert!(text(&output).contains("Perks & Sockets"));
            if app.recipe.namespace == "parhelion.breach-notice" && width >= 860.0 {
                let baseline = |label: &str| {
                    output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.job.text == label => {
                                Some(text.pos.y)
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| panic!("Missing perk {label}"))
                };
                assert!(
                    (baseline("Rifled Barrel") - baseline("Smoothbore")).abs() < 0.5,
                    "Primary and secondary perk labels must share a baseline"
                );
                assert!(
                    (baseline("Steady Rounds") - baseline("High-Caliber Rounds")).abs() < 0.5,
                    "Magazine choices must share a baseline"
                );
            }
            assert!(!text(&output).contains("finished perk"));
            assert_eq!(app.recipe.clone(), before);
        }
        let mut page_height = 0.0;
        let page_before = app.recipe.clone();
        let (output, overflow) = render(1320.0, |ui| {
            app.draw_recipe_editor(ui);
            page_height = ui.cursor().top() - ui.max_rect().top();
        });
        eprintln!(
            "Single-page {}: height {page_height}, overflow {overflow}",
            app.recipe.name
        );
        assert!(
            overflow < 1.0,
            "single-page {} overflow {overflow}",
            app.recipe.name
        );
        for (left, right, horizontal) in [
            ("Base Weapon", "Weapon Stats", true),
            ("Equipment Slot", "Perks & Sockets", true),
            ("Equipment Slot", "Damage Type", false),
            ("Equipment Slot", "Ammo Type", false),
            ("Rarity", "Power Cap", false),
        ] {
            let first = text_origin(&output, left);
            let second = text_origin(&output, right);
            let difference = if horizontal {
                first.x - second.x
            } else {
                first.y - second.y
            };
            assert!(
                difference.abs() <= 1.0,
                "{}: {left} / {right} misaligned by {difference}",
                app.recipe.name
            );
        }
        for label in ["+ Add Choice", "…"] {
            let positions = text_origins(&output, label);
            assert!(positions.len() >= 5);
            assert!(
                positions
                    .iter()
                    .all(|position| (position.x - positions[0].x).abs() <= 1.0),
                "{}: {label} columns diverged: {positions:?}",
                app.recipe.name
            );
        }
        assert_eq!(
            app.recipe, page_before,
            "full-page rendering changed {}",
            app.recipe.name
        );
        for width in [480.0, 900.0, 1320.0] {
            for technical in [false, true] {
                app.show_experimental_options = technical;
                let (output, overflow) = render(width, |ui| app.draw_appearance_workspace(ui));
                assert!(
                    overflow < 1.0,
                    "{} appearance width={width}: {overflow}",
                    app.recipe.name
                );
                let labels = text(&output);
                assert!(labels.contains("Inventory Icon"));
                assert!(labels.contains("Colors & Materials"));
                let edit_icon = text_origin(&output, "Edit Icon…");
                let change_icon = text_origin(&output, "Change Icon");
                assert!(
                    (edit_icon.y - change_icon.y).abs() < 1.0,
                    "icon actions must share a row"
                );
                assert!(edit_icon.x < change_icon.x, "icon actions must not overlap");
                assert_eq!(labels.contains("Technical Appearance Data"), technical);
                assert_eq!(app.recipe, page_before);
            }
        }
        app.show_experimental_options = false;
    }
    assert_private_window_survives_tab_changes(&mut app);
    app.recipe = WeaponRecipe::new_unbound("Layout test").unwrap();
    app.recipe.set_donor(0x23DB_942F, "Age-Old Bond".to_owned());
    let donor = app.current_donor().unwrap();
    let stats_before = app.recipe.clone();
    for width in [480.0, 900.0, 1320.0] {
        let (output, overflow) = render(width, |ui| {
            app.draw_investment_stats_panel(ui, Some(&donor))
        });
        assert!(overflow < 1.0);
        let stat_x = text_origin(&output, "Stat").x;
        for stat in donor
            .investment_stats
            .iter()
            .filter(|stat| !is_internal_weapon_stat(stat.definition_index))
        {
            assert!(
                (text_origin(&output, &stat.name).x - stat_x).abs() < 1.0,
                "stat names must be left aligned"
            );
        }
        let first = donor
            .investment_stats
            .iter()
            .find(|stat| !is_internal_weapon_stat(stat.definition_index))
            .unwrap();
        let raw_x = text_origin(&output, "Raw Value").x + 7.0;
        let preview_x = text_origin(&output, "Preview").x;
        assert!(
            text_origins(&output, &first.value.to_string())
                .iter()
                .any(|pos| (pos.x - raw_x).abs() < 1.0),
            "raw values must be left aligned"
        );
        assert!(
            text_origins(&output, &first.in_game_display_label(first.value))
                .iter()
                .any(|pos| (pos.x - preview_x).abs() < 1.0),
            "previews must be left aligned"
        );
        assert_eq!(app.recipe, stats_before);
    }
    for width in [900.0, 1320.0] {
        let (_, overflow) = render(width, |ui| app.draw_core_recipe_editor(ui));
        if overflow >= 1.0 {
            let donor = app.current_donor().unwrap();
            for section in 0..4 {
                let (_, section_overflow) = render(width, |ui| match section {
                    0 => app.draw_donor_section(ui),
                    1 => app.draw_definition_panel(ui, Some(&donor)),
                    2 => app.draw_investment_stats_panel(ui, Some(&donor)),
                    _ => app.draw_socket_columns_panel(ui, Some(&donor)),
                });
                eprintln!("section {section}, overflow {section_overflow}");
            }
        }
        assert!(
            overflow < 1.0,
            "base weapon layout width={width}, overflow={overflow}"
        );
    }
}

fn assert_private_window_survives_tab_changes(app: &mut PackageAuthoringApp) {
    let before = app.recipe.clone();
    app.private_perk_socket = Some(0);
    for page in WorkbenchPage::ALL {
        app.workbench_page = page;
        let (output, _) = render(1320.0, |ui| app.draw_custom_perks_window(ui.ctx()));
        assert!(
            text(&output).contains("Custom Perks"),
            "window missing on {page:?}"
        );
        assert!(text(&output).contains("Custom perk editing is planned for a future release."));
        assert!(text(&output).contains("Use Existing Custom Perk"));
        assert_eq!(app.private_perk_socket, Some(0));
        assert_eq!(
            app.recipe, before,
            "opening a private window must be read-only"
        );
    }
    app.private_perk_socket = None;
    app.workbench_page = WorkbenchPage::Weapon;
}

#[test]
fn ui_ammo_and_combat_profile_choices_round_trip_into_native_compiler_overrides() {
    use crate::ModernDamageType;
    use crate::capabilities::CombatProfile;
    use sundial::investment::WeaponDamageType;
    let donor = WeaponDonorSummary {
        hash: 0x4CE3_CE93,
        name: "Breachlight".into(),
        type_name: "Sidearm".into(),
        bucket_hash: 0,
        collection_backed: true,
        power_cap: None,
        damage_type: Some(WeaponDamageType::Kinetic),
        inventory_slot: Some(WeaponInventorySlot::Kinetic),
        damage_profile: WeaponDamageProfile::KineticEmpty,
        rarity: WeaponRarity::Legendary,
        ammo_type: Some(WeaponAmmoType::Primary),
        weapon_pattern_index: Some(1),
        weapon_translation_group: Some(1),
        stat_group_index: None,
    };
    for ammo in [
        RecipeAmmoType::Primary,
        RecipeAmmoType::Special,
        RecipeAmmoType::Heavy,
    ] {
        for (element, native_element) in [
            (WeaponDamageType::Arc, ModernDamageType::Arc),
            (WeaponDamageType::Solar, ModernDamageType::Solar),
            (WeaponDamageType::Void, ModernDamageType::Void),
        ] {
            let mut recipe = WeaponRecipe::every_end();
            recipe.overrides.ammo_type = Some(ammo); // Same recipe field bound by the Weapon tab.
            apply_combat_profile_action(
                &mut recipe.overrides,
                &donor,
                CombatProfileAction::Set(CombatProfile {
                    inventory_slot: WeaponInventorySlot::Energy,
                    damage_type: element,
                }),
            );
            let decoded: WeaponRecipe =
                serde_json::from_str(&serde_json::to_string(&recipe).unwrap()).unwrap();
            let spec = decoded.to_spec().unwrap();
            assert_eq!(
                spec.overrides.ammo_type,
                Some(crate::weapon::WeaponAmmoType::from(ammo))
            );
            assert_eq!(spec.overrides.modern_damage_type, Some(native_element));
            assert_eq!(
                spec.overrides.inventory_slot,
                Some(crate::weapon::WeaponInventorySlot::Energy)
            );
            apply_combat_profile_action(
                &mut recipe.overrides,
                &donor,
                CombatProfileAction::Preserve,
            );
            assert!(recipe.overrides.modern_damage_type.is_none());
            assert!(recipe.overrides.inventory_slot.is_none());
            assert_eq!(
                recipe.overrides.ammo_type,
                Some(ammo),
                "damage reset must not reset ammo"
            );
        }
    }
}
