use super::*;
use crate::test_support::driver::{control, label, tap, texts};
use sundial::investment::{WeaponSocket, WeaponStatDisplayPoint};
use sundial::test_support::capture;

fn stat(index: u16, name: &str, value: i32, numeric: bool) -> WeaponInvestmentStat {
    WeaponInvestmentStat {
        definition_index: index,
        definition_hash: None,
        name: name.into(),
        value,
        minimum_value: Some(0),
        maximum_value: Some(100),
        display_as_numeric: numeric,
        is_linear: !numeric,
        display_interpolation: [0, 100]
            .map(|value| WeaponStatDisplayPoint {
                investment_value: value,
                display_value: if numeric { 1100 - value * 10 } else { value },
            })
            .to_vec(),
    }
}

fn fixture() -> (PackageAuthoringApp, WeaponDonor) {
    let mut recipe =
        WeaponRecipe::new_weapon_for_donor("parhelion.stat-preview", 1, "Fixture").unwrap();
    recipe.overrides.socket_columns = vec![
        Some(WeaponSocketColumnRecipe {
            choices: vec![2.into(), 3.into()],
            ..Default::default()
        }),
        Some(WeaponSocketColumnRecipe {
            choices: vec![4.into()],
            socket_type: Some(1),
            ..Default::default()
        }),
    ];
    for (socket, choice, hash, values) in [
        (0, 0, 2, vec![(10, 20), (11, -15), (14, 10), (30, 5)]),
        (0, 1, 3, vec![(10, 99)]),
        (1, 0, 4, vec![(10, 5)]),
    ] {
        recipe
            .overrides
            .socket_plug_variants
            .push(WeaponSocketPlugVariantRecipe {
                projectile: None,
                replace_effects: true,
                investment_stats: values
                    .into_iter()
                    .map(|(definition_index, value)| WeaponStatOverride {
                        definition_index,
                        value,
                    })
                    .collect(),
                socket_index: socket,
                choice_index: choice,
                source_plug_hash: hash.into(),
                name: Some(format!("Perk {hash}")),
                classification_donor_hash: None,
                icon: None,
                description: None,
                offer_everywhere: false,
                additional_sandbox_perks: vec![],
                sandbox_perks: vec![],
            });
    }
    let donor = WeaponDonor {
        summary: crate::test_support::donor_summary(1, "Fixture", "Auto Rifle"),
        power_cap_groups: vec![],
        equipment_slot: None,
        sockets: vec![WeaponSocket {
            index: 0,
            socket_type: 1,
            label: "Barrel".into(),
            native_default: Some(2),
            ordered_embedded_choices: vec![2, 3],
            max_authored_choices: 2,
            compatible_plug_count: 2,
            reusable_plug_set_index: None,
            randomized_plug_set_index: None,
        }],
        investment_stats: vec![
            stat(10, "Range", 40, false),
            stat(11, "Stability", 60, false),
            stat(14, "Charge Time", 40, true),
        ],
        addable_investment_stats: vec![stat(30, "Aim Assistance", 0, false)],
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
    };
    (
        PackageAuthoringApp {
            recipe,
            ..Default::default()
        },
        donor,
    )
}

fn frame(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    donor: &WeaponDonor,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 320.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                style::workbench_style(ui);
                app.draw_investment_stats_panel(ui, Some(donor));
            });
        },
    );
    capture::record(&output);
    output
}

fn reading(output: &egui::FullOutput, name: &str) -> (String, egui::Rect) {
    let row = label(output, name).unwrap();
    texts(output)
        .into_iter()
        .find(|(_, rect)| {
            rect.left() > row.right() && (rect.center().y - row.center().y).abs() < 2.0
        })
        .unwrap()
}

fn text_color(output: &egui::FullOutput, at: egui::Rect) -> egui::Color32 {
    fn find(shape: &egui::Shape, at: egui::Rect) -> Option<egui::Color32> {
        match shape {
            egui::Shape::Text(text) if text.galley.rect.translate(text.pos.to_vec2()) == at => {
                Some(text.galley.job.sections[0].format.color)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, at)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, at))
        .unwrap()
}

#[test]
#[expect(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn weapon_stat_preview_follows_starting_perks_and_preserves_saved_raw_edits() {
    let temporary = tempfile::tempdir().unwrap();
    for dark in [false, true] {
        for width in [420.0, 900.0] {
            let (mut app, donor) = fixture();
            let ctx = egui::Context::default();
            ctx.set_theme(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
            ctx.enable_accesskit();
            let before = app.recipe.clone();
            frame(&ctx, &mut app, &donor, width, vec![]);
            let output = frame(&ctx, &mut app, &donor, width, vec![]);
            assert_eq!(app.recipe, before);
            capture::write(&ctx, &output, &format!("weapon-stats-{dark}-{width}"));
            for (name, expected) in [
                ("Range", "65"),
                ("Stability", "45"),
                ("Charge Time", "600"),
                ("Aim Assistance", "5"),
            ] {
                let (shown, rect) = reading(&output, name);
                assert_eq!(shown, expected, "{name}");
                assert!(rect.right() < width && rect.left() >= 0.0);
                let color = text_color(&output, rect);
                if name == "Range" || name == "Aim Assistance" {
                    assert!(color.g() > color.r());
                } else {
                    assert!(color.r() > color.g());
                }
            }
            assert!(
                control(&output, "Aim Assistance").is_none(),
                "a plug-only contribution must not create a raw editor"
            );
            let position = control(&output, "Range").unwrap().center();
            for _ in 0..2 {
                for events in tap(position) {
                    frame(&ctx, &mut app, &donor, width, events);
                }
            }
            let key = |key, modifiers| egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            };
            frame(
                &ctx,
                &mut app,
                &donor,
                width,
                vec![
                    key(egui::Key::A, egui::Modifiers::COMMAND),
                    egui::Event::Text("50".into()),
                    key(egui::Key::Enter, egui::Modifiers::NONE),
                ],
            );
            let output = frame(&ctx, &mut app, &donor, width, vec![]);
            assert_eq!(reading(&output, "Range").0, "75");
            let path = temporary.path().join(format!("weapon-{dark}-{width}.json"));
            app.recipe.save_json(&path).unwrap();
            app.recipe = WeaponRecipe::load_json(&path).unwrap();
            assert_eq!(
                app.recipe.overrides.investment_stats,
                vec![WeaponStatOverride {
                    definition_index: 10,
                    value: 50
                }]
            );
            crate::test_support::artifact(
                &format!("weapon-stats-{dark}-{width}.json"),
                &app.recipe,
            );
            let position = control(&output, "Restore Range to 40").unwrap().center();
            for events in tap(position) {
                frame(&ctx, &mut app, &donor, width, events);
            }
            let output = frame(&ctx, &mut app, &donor, width, vec![]);
            assert_eq!(reading(&output, "Range").0, "65");
            assert!(app.recipe.overrides.investment_stats.is_empty());
            app.recipe.overrides.socket_columns[1]
                .as_mut()
                .unwrap()
                .socket_type = Some(u16::MAX);
            let output = frame(&ctx, &mut app, &donor, width, vec![]);
            assert_eq!(reading(&output, "Range").0, "60");
            app.recipe.overrides.socket_columns[0]
                .as_mut()
                .unwrap()
                .choices
                .clear();
            let output = frame(&ctx, &mut app, &donor, width, vec![]);
            assert_eq!(reading(&output, "Range").0, "40");
            assert!(label(&output, "Aim Assistance").is_none());
            capture::write(
                &ctx,
                &output,
                &format!("weapon-stats-cleared-{dark}-{width}"),
            );
        }
    }
}
