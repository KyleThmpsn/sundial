use super::*;

fn real_workbench() -> PackageAuthoringApp {
    let packages = PathBuf::from(std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap());
    let mut app = PackageAuthoringApp::default();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    app.donor_summaries = catalog.weapon_donors();
    app.library_state.refresh_donors(&app.donor_summaries);
    app.sandbox_perk_choices = catalog.weapon_sandbox_perk_choices_from(|_| true);
    app.catalog = Some(catalog);
    app.packages = packages;
    app.show_experimental_options = false;
    app
}

#[test]
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES; package-backed headless layout check"]
fn real_appearance_layout_is_read_only_and_fits() {
    let mut app = real_workbench();
    for (_, json) in crate::recipe_library::BUNDLED_RECIPES.iter().skip(2) {
        app.recipe = WeaponRecipe::from_json_str(json).unwrap();
        let page_before = app.recipe.clone();
        for width in [480.0, 900.0, 1320.0] {
            for technical in [false, true] {
                app.show_experimental_options = technical;
                let capture_name = (!technical).then(|| format!("appearance-{width}"));
                let (output, overflow) =
                    render_with_capture(width, capture_name.as_deref(), |ui| {
                        app.draw_appearance_workspace(ui);
                    });
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
}

#[test]
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES; package-backed headless layout check"]
fn real_empty_socket_warnings_stay_with_their_controls() {
    let mut app = real_workbench();
    let donor = app
        .donor_summaries
        .iter()
        .filter_map(|summary| app.catalog.as_ref().unwrap().weapon_donor(summary.hash))
        .find(|donor| {
            donor.sockets.iter().any(|socket| {
                socket.randomized_plug_set_index.is_some()
                    && socket.native_default.is_none()
                    && socket.ordered_embedded_choices.is_empty()
                    && authored_socket_choice_limit(socket.socket_type) > 0
            })
        })
        .expect("A stock weapon with a random socket and no default");
    app.recipe = WeaponRecipe::new_named_weapon_for_donor(
        "Socket Warning Layout",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    app.recipe
        .overrides
        .socket_columns
        .resize(donor.sockets.len(), None);
    let before = app.recipe.clone();
    for width in [480.0, 900.0, 1320.0] {
        let name = format!("socket-warning-{width}");
        let (output, overflow) = render_with_capture(width, Some(&name), |ui| {
            app.draw_socket_columns_panel(ui, Some(&donor));
        });
        let labels = crate::test_support::driver::texts(&output);
        let warnings = labels
            .iter()
            .filter(|(text, _)| text.starts_with("Rolls at random with no default."))
            .collect::<Vec<_>>();
        assert!(!warnings.is_empty(), "No random socket warning at {width}");
        for (_, warning) in warnings {
            assert!(
                labels.iter().any(|(text, button)| {
                    text == "+ Set Plug"
                        && (button.center().y - warning.center().y).abs() < 1.0
                        && warning.right() < button.left()
                }),
                "The warning must sit beside its socket's action at {width}"
            );
        }
        assert!(
            overflow < 1.0,
            "Socket warning overflow at {width}: {overflow}"
        );
        assert_eq!(app.recipe, before, "Rendering changed the socket choices");
    }
}

#[test]
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES; package-backed headless layout check"]
#[expect(
    clippy::cognitive_complexity,
    reason = "Integration matrix keeps per-recipe rendering and mutation assertions together"
)]
fn real_workbench_socket_layout_is_read_only_and_fits() {
    let mut app = real_workbench();
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
        crate::capabilities::AppearanceCompatibility::Compatible
    );
    assert!(crate::capabilities::appearance_animations_differ(
        horseman, slug
    ));
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
                draw_combat_profile_control(
                    ui,
                    &mut app.recipe.overrides,
                    Some(&donor),
                    true,
                    false,
                    false,
                );
                draw_combat_profile_control(
                    ui,
                    &mut app.recipe.overrides,
                    Some(&donor),
                    false,
                    false,
                    false,
                );
                draw_combat_profile_diagnostics(ui, &app.recipe.overrides, Some(&donor));
            });
            assert!(text(&output).contains(egui_phosphor::regular::WARNING));
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
            draw_combat_profile_control(
                ui,
                &mut app.recipe.overrides,
                Some(&donor),
                true,
                false,
                false,
            );
            draw_combat_profile_control(
                ui,
                &mut app.recipe.overrides,
                Some(&donor),
                false,
                false,
                false,
            );
            draw_combat_profile_diagnostics(ui, &app.recipe.overrides, Some(&donor));
        });
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
            if app.recipe.namespace == "parhelion.vaultbreaker" && width >= 860.0 {
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
        for label in ["+ Add Choice", crate::app::style::MORE] {
            let positions = text_origins(&output, label);
            assert!(positions.len() >= 2, "alignment needs two visible controls");
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
    }
    assert_private_window_survives_tab_changes(&mut app);
    app.recipe = WeaponRecipe::new_unbound("Layout test").unwrap();
    app.recipe.set_donor(0x23DB_942F, "Age-Old Bond".to_owned());
    let donor = app.current_donor().unwrap();
    let stats_before = app.recipe.clone();
    for width in [480.0, 900.0, 1320.0] {
        // The raw value is a framed DragValue, so its text sits one button padding inside the
        // column its header starts at. Read the padding rather than pinning today's number.
        let mut value_inset = 0.0;
        let (output, overflow) = render(width, |ui| {
            value_inset = ui.spacing().button_padding.x;
            app.draw_investment_stats_panel(ui, Some(&donor));
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
        let raw_x = text_origin(&output, "Raw Value").x + value_inset;
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

#[test]
fn recipe_combat_profile_actions_round_trip_into_compiler_overrides() {
    use crate::ModernDamageType;
    use crate::capabilities::CombatProfile;
    use sundial::investment::WeaponDamageType;
    let donor = WeaponDonorSummary {
        damage_type: Some(WeaponDamageType::Kinetic),
        inventory_slot: Some(WeaponInventorySlot::Kinetic),
        damage_profile: WeaponDamageProfile::KineticEmpty,
        ammo_type: Some(WeaponAmmoType::Primary),
        weapon_pattern_index: Some(1),
        weapon_translation_group: Some(1),
        ..crate::test_support::donor_summary(0x4CE3_CE93, "Breachlight", "Sidearm")
    };
    for (ammo, native_ammo) in [
        (
            RecipeAmmoType::Primary,
            crate::item::WeaponAmmoType::Primary,
        ),
        (
            RecipeAmmoType::Special,
            crate::item::WeaponAmmoType::Special,
        ),
        (RecipeAmmoType::Heavy, crate::item::WeaponAmmoType::Heavy),
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
            assert_eq!(spec.overrides.ammo_type, Some(native_ammo));
            assert_eq!(spec.overrides.modern_damage_type, Some(native_element));
            assert_eq!(
                spec.overrides.inventory_slot,
                Some(crate::item::WeaponInventorySlot::Energy)
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

/// Behavior is a dropdown in the Weapon tab's profile grid beside Rarity and Power Cap, setting
/// the same choice as Gameplay's Behavior row, which keeps what the choice brings. The grid and
/// the donor section fold without running past the panel.
#[test]
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES; package-backed headless layout check"]
fn real_behavior_dropdown_sits_in_the_weapon_profile() {
    use crate::weapon::behavior::CATALOG;
    /// Bygones, a pulse rifle in the owner that also holds Hard Light's records.
    const BYGONES: u32 = 0xA1A9_9205;
    let packages = PathBuf::from(std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap());
    let mut app = PackageAuthoringApp::default();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let donor = catalog.weapon_donor(BYGONES).unwrap();
    app.donor_summaries = catalog.weapon_donors();
    app.catalog = Some(catalog);
    app.packages = packages;
    app.recipe =
        WeaponRecipe::new_weapon_for_donor("parhelion.behavior-layout", BYGONES, "Bygones")
            .unwrap();

    for width in [560.0, 900.0, 1320.0] {
        let (_, overflow) = render(width, |ui| app.draw_donor_section(ui));
        assert!(
            overflow < 1.0,
            "donor section width={width}, overflow={overflow}"
        );
        let (output, overflow) = render(width, |ui| app.draw_definition_panel(ui, Some(&donor)));
        let rendered = text(&output);
        for label in ["Damage Type", "Rarity", "Behavior", "Bygones (base weapon)"] {
            assert!(rendered.contains(label), "missing {label}:\n{rendered}");
        }
        assert!(
            overflow < 1.0,
            "definition panel width={width}, overflow={overflow}"
        );
        if width == 900.0 {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let mut frame = |events| {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 1200.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            workbench_style(ui);
                            app.draw_definition_panel(ui, Some(&donor));
                        });
                    },
                );
                custom_perks::workbench::tests::capture::record(&output);
                output
            };
            frame(Vec::new());
            let mut captured = frame(Vec::new());
            crate::app::custom_perks::workbench::tests::capture::write(
                &ctx,
                &captured,
                "behavior-dropdown",
            );
            let trigger =
                crate::test_support::driver::accessible(&captured, "Bygones (base weapon)")
                    .unwrap();
            for events in crate::test_support::driver::tap(trigger.center())
                .into_iter()
                .chain([Vec::new(), Vec::new()])
            {
                captured = frame(events);
            }
            custom_perks::workbench::tests::capture::write(&ctx, &captured, "behavior-choices");
            assert!(text(&captured).contains("Follow Base Weapon"));
        }
    }

    // A chosen behavior reads in the grid, and Gameplay's row shows what it brings.
    let source = CATALOG
        .iter()
        .find(|entry| entry.id == "graviton-lance")
        .unwrap();
    app.recipe.overrides.additional_behaviors = vec![crate::recipe::AdditionalBehaviorRecipe {
        behavior: source.id.to_owned(),
    }];
    let (output, _) = render(900.0, |ui| app.draw_definition_panel(ui, Some(&donor)));
    // The value names the perks the choice puts in the sockets, not just the weapon.
    assert!(
        text(&output).contains("Graviton Lance ("),
        "{}",
        text(&output)
    );
    let (output, overflow) = render(900.0, |ui| app.draw_gameplay_parts(ui, Some(&donor)));
    let rendered = text(&output);
    assert!(rendered.contains("Include Its Perks"), "{rendered}");
    assert!(overflow < 1.0);
}
