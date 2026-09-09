use super::*;

fn donor() -> WeaponDonor {
    WeaponDonor {
        summary: WeaponDonorSummary {
            hash: 1,
            name: "Test Weapon".into(),
            type_name: "Trace Rifle".into(),
            bucket_hash: 0,
            collection_backed: true,
            power_cap: Some(1060),
            damage_type: Some(sundial::investment::WeaponDamageType::Arc),
            inventory_slot: Some(WeaponInventorySlot::Energy),
            ammo_type: Some(WeaponAmmoType::Special),
            weapon_pattern_index: Some(1),
            weapon_translation_group: None,
            stat_group_index: None,
            damage_profile: WeaponDamageProfile::ModernFixed(
                sundial::investment::WeaponDamageType::Arc,
            ),
            rarity: WeaponRarity::Exotic,
        },
        power_cap_groups: vec![7, 8],
        equipment_slot: Some(WeaponInventorySlot::Energy),
        sockets: vec![],
        investment_stats: vec![WeaponInvestmentStat {
            definition_index: 30,
            definition_hash: None,
            name: "Aim Assistance".into(),
            value: 80,
            minimum_value: None,
            maximum_value: Some(100),
            display_as_numeric: false,
            is_linear: false,
            display_interpolation: vec![],
        }],
        addable_investment_stats: vec![],
        base_sandbox_perks: vec![],
        trait_indices: vec![],
        max_stack_size: Some(1),
        socket_entry_list_index: None,
        plug_category_hash: None,
        roll_set_index: None,
        linked_plug_index: None,
        linked_plug_hash: None,
        art_arrangements: vec![],
        render_dye_rows: Default::default(),
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
                egui::vec2(900.0, 1000.0),
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

fn click(ctx: &egui::Context, pos: egui::Pos2, mut draw: impl FnMut(&mut egui::Ui)) {
    for pressed in [true, false] {
        frame(
            ctx,
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
}

#[test]
fn advanced_power_cap_mode_preserves_the_effective_selection() {
    let donor = donor();
    for scalar in [None, Some(15)] {
        let ctx = egui::Context::default();
        let mut app = PackageAuthoringApp::default();
        app.recipe.overrides.power_cap_group = scalar;
        app.recipe.overrides.power_cap_groups = None;
        let expected = scalar.map_or_else(|| vec![7, 8], |group| vec![group; 2]);
        let before = app.recipe.clone();
        let mut draw = |ui: &mut egui::Ui| app.draw_native_inventory_fields(ui, Some(&donor));
        frame(&ctx, vec![], &mut draw);
        let output = frame(&ctx, vec![], &mut draw);
        let pos = text_origin(&output, "Customize each row") + egui::vec2(8.0, 6.0);
        assert_eq!(app.recipe, before);
        assert!(
            text(&output).contains(
                &expected
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        );
        click(&ctx, pos, |ui| {
            app.draw_native_inventory_fields(ui, Some(&donor))
        });
        assert_eq!(app.recipe.overrides.power_cap_group, None);
        assert_eq!(
            app.recipe.overrides.power_cap_groups,
            Some(expected.clone())
        );
        assert_eq!(
            effective_power_cap_rows(&app.recipe.overrides, &[1, 2]),
            expected
        );
    }
}

#[test]
fn trace_rifle_rarity_choices_reject_unsupported_tiers_without_coercing_imports() {
    let mut donor = donor();
    assert!(rarity_is_supported(Some(&donor), None));
    assert!(!rarity_is_supported(None, Some(RecipeRarity::Exotic)));
    for rarity in [
        RecipeRarity::Common,
        RecipeRarity::Uncommon,
        RecipeRarity::Rare,
        RecipeRarity::Legendary,
        RecipeRarity::Exotic,
    ] {
        assert_eq!(
            rarity_is_supported(Some(&donor), Some(rarity)),
            rarity == RecipeRarity::Exotic
        );
        donor.summary.type_name = "Auto Rifle".into();
        assert!(rarity_is_supported(Some(&donor), Some(rarity)));
        donor.summary.type_name = "trace rifle".into();
    }
    donor.summary.rarity = WeaponRarity::Legendary;
    assert!(!rarity_is_supported(Some(&donor), None));
    donor.summary.rarity = WeaponRarity::Exotic;

    let mut overrides = WeaponRecipeOverrides {
        rarity: Some(RecipeRarity::Legendary),
        ..Default::default()
    };
    let before = overrides.clone();
    let (output, _) = render(420.0, |ui| {
        draw_rarity_control(ui, &mut overrides, Some(&donor))
    });
    assert_eq!(overrides, before);
    assert!(text(&output).contains("Choose Exotic rarity"));

    overrides.rarity = Some(RecipeRarity::Exotic);
    let ctx = egui::Context::default();
    let mut draw = |ui: &mut egui::Ui| draw_rarity_control(ui, &mut overrides, Some(&donor));
    frame(&ctx, vec![], &mut draw);
    let output = frame(&ctx, vec![], &mut draw);
    click(
        &ctx,
        text_origin(&output, "Exotic") + egui::vec2(8.0, 6.0),
        &mut draw,
    );
    frame(&ctx, vec![], &mut draw);
    let output = frame(&ctx, vec![], &mut draw);
    click(
        &ctx,
        text_origin(&output, "Legendary") + egui::vec2(8.0, 6.0),
        &mut draw,
    );
    assert_eq!(overrides.rarity, Some(RecipeRarity::Exotic));
}

#[test]
fn stat_maximum_without_minimum_clamps_edits_but_not_existing_values() {
    let donor = donor();
    let mut values = vec![WeaponStatOverride {
        definition_index: 30,
        value: 150,
    }];
    let before = values.clone();
    let mut removed = vec![];
    render(640.0, |ui| {
        draw_investment_stats(ui, &mut values, &mut removed, &donor, false)
    });
    assert_eq!(values, before);
    values[0].value = 99;
    let ctx = egui::Context::default();
    let mut draw =
        |ui: &mut egui::Ui| draw_investment_stats(ui, &mut values, &mut removed, &donor, false);
    frame(&ctx, vec![], &mut draw);
    let output = frame(&ctx, vec![], &mut draw);
    let pos = text_origin(&output, "99") + egui::vec2(8.0, 6.0);
    frame(
        &ctx,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        &mut draw,
    );
    let end = pos + egui::vec2(200.0, 0.0);
    frame(&ctx, vec![egui::Event::PointerMoved(end)], &mut draw);
    frame(
        &ctx,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
        &mut draw,
    );
    assert_eq!(values[0].value, 100);
    assert!(removed.is_empty());
}
