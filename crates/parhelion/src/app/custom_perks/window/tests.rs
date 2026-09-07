use super::*;

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES; read-only native layout"]
fn native_custom_perk_window_names_choices_and_preserves_recipe() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
    let donor = catalog
        .weapon_donor_with_stat_group_index(0x23DB_942F, None)
        .unwrap();
    let mut recipe =
        WeaponRecipe::new_weapon_for_donor("parhelion.ui-test", 0x23DB_942F, "Age-Old Bond")
            .unwrap();
    let socket = &donor.sockets[0];
    let choices = inherited_socket_choices(
        socket.native_default,
        &socket.ordered_embedded_choices,
        authored_socket_choice_limit(socket.socket_type),
    );
    assert!(!choices.is_empty());
    assert!(
        custom_socket_label(&catalog, &donor, &recipe, 0)
            .contains(&catalog.plug_label(choices[0], false))
    );
    let key = PerkEditorKey {
        socket_index: 0,
        choice_index: 0,
        source_plug_hash: choices[0],
        source_perk_index: catalog.item_sandbox_perk_indices(choices[0])[0],
    };
    upsert_private_perk_runtime_values(&mut recipe, key, Vec::new());
    recipe.overrides.socket_plug_variants[0].name = Some("My Custom Intrinsic".into());
    assert!(custom_socket_label(&catalog, &donor, &recipe, 0).contains("My Custom Intrinsic"));
    let before = recipe.clone();
    for (width, height) in [(640.0, 480.0), (1000.0, 720.0), (1322.0, 932.0)] {
        for dark in [false, true] {
            let ctx = egui::Context::default();
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ctx| {
                        let mut editor = None;
                        let mut open = true;
                        let mut selected = 0;
                        draw_private_socket_variants(
                            ctx,
                            PrivateSocketContext {
                                catalog: &catalog,
                                packages: &packages,
                                sandbox_perk_choices: &[],
                                socket_index: 0,
                                donor: &donor,
                                experimental: true,
                            },
                            &mut recipe,
                            &choices,
                            &mut editor,
                            &mut open,
                            &mut selected,
                        );
                        assert!(editor.is_none());
                        assert!(open);
                        assert_eq!(selected, 0);
                    },
                );
            }
            let done = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "Done" => {
                        Some(text.galley.rect.translate(text.pos.to_vec2()))
                    }
                    _ => None,
                })
                .expect("Done visible");
            assert!(screen.contains_rect(done));
            assert_eq!(recipe, before);
        }
    }
}

#[test]
fn projection_warning_counts_added_effects_not_runtime_clones() {
    let mut variant: WeaponSocketPlugVariantRecipe = serde_json::from_str(
        r#"{"socket_index":0,"choice_index":0,"source_plug_hash":"0xDD5CB37A","sandbox_perks":[{"source_perk_index":1178,"runtime_values":[]}]}"#,
    ).unwrap();
    assert!(custom_perk_projection_warning(4, Some(&variant)).is_none());
    variant.additional_sandbox_perks.push(405);
    assert!(custom_perk_projection_warning(4, Some(&variant)).is_some());
    assert!(custom_perk_projection_warning(3, Some(&variant)).is_none());
    assert!(custom_perk_projection_warning(5, None).is_some());
}
