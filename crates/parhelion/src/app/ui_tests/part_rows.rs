//! The part rows and the Markers section against the installed packages. Each background read
//! (the weapon index, the marker sets and the model) is waited out, so the captures show what an
//! author sees once the page settles.
use super::*;
use crate::recipe::{MarkerOffsetRecipe, WeaponDonorReference};

const AUSTRINGER: u32 = 0x90D4_2801;
const LUNAS_HOWL: u32 = 0x092D_8A04;
const ANCIENT_GOSPEL: u32 = 0x02E6_3C72;
const DIRE_PROMISE: u32 = 0x22B5_BC70;

/// Runs `draw` until nothing on the page is still reading, then returns the settled frame.
fn settle(
    ctx: &egui::Context,
    width: f32,
    draw: &mut dyn FnMut(&mut egui::Ui),
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut events = Some(events);
    let started = std::time::Instant::now();
    loop {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 1100.0),
                )),
                events: events.take().unwrap_or_default(),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    workbench_style(ui);
                    draw(ui);
                });
            },
        );
        custom_perks::workbench::tests::capture::record(&output);
        let rendered = text(&output);
        let busy = rendered.contains("Reading\u{2026}")
            || rendered.contains("Loading Model")
            || rendered.contains("Checking the rig");
        if !busy || started.elapsed() > std::time::Duration::from_secs(180) {
            assert!(!busy, "the page never finished reading:\n{rendered}");
            return output;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES; package-backed headless layout check"]
fn real_part_rows_and_markers_settle_and_fit() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap());
    let mut app = PackageAuthoringApp::default();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    app.donor_summaries = catalog.weapon_donors();
    app.catalog = Some(catalog);
    app.packages = packages;
    app.recipe =
        WeaponRecipe::new_weapon_for_donor("parhelion.part-rows-layout", AUSTRINGER, "Austringer")
            .unwrap();
    app.recipe
        .set_presentation_donor(Some(WeaponDonorReference {
            item_hash: LUNAS_HOWL.into(),
            expected_name: Some("Luna's Howl".to_owned()),
        }));

    // Gameplay's Parts and the Appearance tab's Animations, as those pages draw them.
    fn parts(app: &mut PackageAuthoringApp, ui: &mut egui::Ui) {
        let donor = app.current_donor();
        app.draw_gameplay_parts(ui, donor.as_ref());
        app.draw_type_marker_part(ui);
        ui.separator();
        app.draw_animation_part(ui);
    }

    // Inherited parts read as the base weapon's and the appearance's own.
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let output = settle(&ctx, 900.0, &mut |ui| parts(&mut app, ui), Vec::new());
    custom_perks::workbench::tests::capture::write(&ctx, &output, "part-rows-inherited");
    let rendered = text(&output);
    for label in ["Parts", "Type Markers", "Animations", "hand_cannon"] {
        assert!(rendered.contains(label), "missing {label}:\n{rendered}");
    }

    // Chosen parts read as the entry they belong to, a type or an animation profile.
    app.recipe.overrides.animation_donor = Some(WeaponDonorReference {
        item_hash: ANCIENT_GOSPEL.into(),
        expected_name: Some("Ancient Gospel".to_owned()),
    });
    app.recipe.overrides.type_marker_donor = Some(WeaponDonorReference {
        item_hash: DIRE_PROMISE.into(),
        expected_name: Some("Dire Promise".to_owned()),
    });
    let output = settle(&ctx, 900.0, &mut |ui| parts(&mut app, ui), Vec::new());
    custom_perks::workbench::tests::capture::write(&ctx, &output, "part-rows-chosen");
    let rendered = text(&output);
    assert!(rendered.contains("hand_cannon"), "{rendered}");
    assert!(!rendered.contains("Does not fit"), "{rendered}");
    assert!(!rendered.contains("Not in this runtime"), "{rendered}");

    // The Animations list holds the profiles the model's rig plays, each named by its frame or
    // its one weapon, not every weapon.
    let trigger =
        crate::test_support::driver::accessible_starting(&output, "Animations: ").unwrap();
    for events in crate::test_support::driver::tap(trigger.center()) {
        settle(&ctx, 900.0, &mut |ui| parts(&mut app, ui), events);
    }
    let opened = settle(&ctx, 900.0, &mut |ui| parts(&mut app, ui), Vec::new());
    custom_perks::workbench::tests::capture::write(&ctx, &opened, "part-rows-animations");
    let rendered = text(&opened);
    assert!(rendered.contains("Follow Appearance"), "{rendered}");
    assert!(rendered.contains("Adaptive Frame"), "{rendered}");

    // Actions the frames play differently sit in a closed disclosure until one is mixed, which
    // opens it with a count and the action rows.
    let rendered = text(&settle(
        &egui::Context::default(),
        900.0,
        &mut |ui| parts(&mut app, ui),
        Vec::new(),
    ));
    assert!(rendered.contains("Actions"), "{rendered}");
    app.recipe.overrides.animation_actions.insert(
        crate::recipe::AnimationAction::Fire,
        WeaponDonorReference {
            item_hash: DIRE_PROMISE.into(),
            expected_name: Some("Dire Promise".to_owned()),
        },
    );
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mixed = settle(&ctx, 900.0, &mut |ui| parts(&mut app, ui), Vec::new());
    custom_perks::workbench::tests::capture::write(&ctx, &mixed, "part-rows-actions");
    let rendered = text(&mixed);
    for label in ["1 Mixed", "Hip Fire", "Aim Fire"] {
        assert!(
            rendered.contains(label),
            "missing {label}:
{rendered}"
        );
    }
    app.recipe.overrides.animation_actions.clear();

    // The Placement section, with the model a centimetre forward in the hand and the trigger
    // marker moved half a centimetre up.
    let trigger = sundial::package_authoring::fnv1_name_hash("primary_trigger");
    app.recipe.overrides.marker_offsets = vec![MarkerOffsetRecipe {
        marker: trigger.into(),
        offset_um: [0, 0, 5_000],
    }];
    app.recipe.overrides.held_offset_um = [10_000, 0, 0];
    for width in [1280.0, 700.0] {
        check_placement(&mut app, width);
    }
}

/// Fate of All Fools: a scout rifle wearing a pulse rifle. The Animations browser offers both
/// rig families, the base family's choice keeps the scout rig, and the Type Markers browser
/// names each weapon's type.
#[test]
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES; package-backed headless layout check"]
fn real_cross_family_part_rows_offer_both_rigs() {
    const JADE_RABBIT: u32 = 0xE529_6126;
    const MACHINA_DEI_4: u32 = 0x09A0_DE64;
    let packages = PathBuf::from(std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap());
    let mut app = PackageAuthoringApp::default();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    app.donor_summaries = catalog.weapon_donors();
    app.catalog = Some(catalog);
    app.packages = packages;
    app.recipe = WeaponRecipe::new_weapon_for_donor(
        "parhelion.cross-family-part-rows",
        JADE_RABBIT,
        "The Jade Rabbit",
    )
    .unwrap();
    app.recipe
        .set_presentation_donor(Some(WeaponDonorReference {
            item_hash: MACHINA_DEI_4.into(),
            expected_name: Some("Machina Dei 4".to_owned()),
        }));
    // The runtime scan decides whether the pulse rifle's rig moves, as the app's frame does.
    fn section(app: &mut PackageAuthoringApp, ui: &mut egui::Ui) {
        app.ensure_runtime_graph(ui.ctx());
        app.poll_runtime_graph();
        app.draw_donor_section(ui);
        let donor = app.current_donor();
        app.draw_gameplay_parts(ui, donor.as_ref());
        app.draw_type_marker_part(ui);
        app.draw_animation_part(ui);
    }
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let moved = settle(&ctx, 900.0, &mut |ui| section(&mut app, ui), Vec::new());
    custom_perks::workbench::tests::capture::write(&ctx, &moved, "fate-moved");
    let rendered = text(&moved);
    assert!(rendered.contains("Machina Dei 4"), "{rendered}");
    assert!(rendered.contains("scout_rifle"), "{rendered}");

    let trigger = crate::test_support::driver::accessible_starting(&moved, "Animations: ").unwrap();
    let [press, release] = crate::test_support::driver::tap(trigger.center());
    settle(&ctx, 900.0, &mut |ui| section(&mut app, ui), press);
    settle(&ctx, 900.0, &mut |ui| section(&mut app, ui), release);
    let opened = settle(&ctx, 900.0, &mut |ui| section(&mut app, ui), Vec::new());
    custom_perks::workbench::tests::capture::write(&ctx, &opened, "fate-animations-picker");
    let rendered = text(&opened);
    assert!(rendered.contains("The Jade Rabbit"), "{rendered}");
    assert!(rendered.contains("(Pulse Rifle)"), "{rendered}");

    app.recipe.overrides.animation_donor = Some(WeaponDonorReference {
        item_hash: JADE_RABBIT.into(),
        expected_name: Some("The Jade Rabbit".to_owned()),
    });
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let kept = settle(&ctx, 900.0, &mut |ui| section(&mut app, ui), Vec::new());
    custom_perks::workbench::tests::capture::write(&ctx, &kept, "fate-kept");
    let rendered = text(&kept);
    assert!(rendered.contains("Moving parts stay still"), "{rendered}");
    assert!(!rendered.contains("Does not fit"), "{rendered}");

    let trigger =
        crate::test_support::driver::accessible_starting(&kept, "Type Markers: scout_rifle")
            .unwrap();
    let [press, release] = crate::test_support::driver::tap(trigger.center());
    settle(&ctx, 900.0, &mut |ui| section(&mut app, ui), press);
    settle(&ctx, 900.0, &mut |ui| section(&mut app, ui), release);
    let opened = settle(&ctx, 900.0, &mut |ui| section(&mut app, ui), Vec::new());
    custom_perks::workbench::tests::capture::write(&ctx, &opened, "fate-type-markers-picker");
    let rendered = text(&opened);
    // Every weapon in the rifle content owner, named by the type its markers carry.
    assert!(rendered.contains("auto_rifle"), "{rendered}");
    assert!(rendered.contains("Follow Base Weapon"), "{rendered}");
}

fn check_placement(app: &mut PackageAuthoringApp, width: f32) {
    // Each width starts as a fresh section would, on the side view with named markers.
    app.marker_editor = Default::default();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let output = settle(
        &ctx,
        width,
        &mut |ui| {
            app.marker_editor.poll();
            app.draw_appearance_placement(ui);
        },
        Vec::new(),
    );
    let rendered = text(&output);
    for label in [
        "Placement",
        "In Hand",
        "1.00 cm",
        "Markers",
        "primary_trigger",
        "primary_fire",
        "Moved",
        "Orbit",
        "Side",
    ] {
        assert!(rendered.contains(label), "missing {label}:\n{rendered}");
    }
    // The section opens on the side view, where markers move.
    let row = crate::test_support::driver::label(&output, "primary_trigger").unwrap();
    for events in crate::test_support::driver::tap(row.center()) {
        settle(
            &ctx,
            width,
            &mut |ui| {
                app.marker_editor.poll();
                app.draw_appearance_placement(ui);
            },
            events,
        );
    }
    let sided = settle(
        &ctx,
        width,
        &mut |ui| {
            app.marker_editor.poll();
            app.draw_appearance_placement(ui);
        },
        Vec::new(),
    );
    // The model's software image renders after the page settles, so the capture waits it
    // out rather than showing an empty preview.
    let mut sided = sided;
    for _ in 0..80 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        sided = settle(
            &ctx,
            width,
            &mut |ui| {
                app.marker_editor.poll();
                app.draw_appearance_placement(ui);
            },
            Vec::new(),
        );
    }
    custom_perks::workbench::tests::capture::write(&ctx, &sided, &format!("markers-side-{width}"));
    let rendered = text(&sided);
    for label in ["Forward", "Side", "Up", "0.50 cm", "Reset"] {
        assert!(rendered.contains(label), "missing {label}:\n{rendered}");
    }
    // The other angles, and every marker including the unnamed ones.
    let mut shown = sided;
    // One editor serves both widths, so the second pass turns the unnamed markers off again.
    for name in ["Top", "Rear", "Orbit", "Unnamed"] {
        let target = crate::test_support::driver::label(&shown, name).unwrap();
        let [press, release] = crate::test_support::driver::tap(target.center());
        let mut markers = |ui: &mut egui::Ui| {
            app.marker_editor.poll();
            app.draw_appearance_placement(ui);
        };
        settle(&ctx, width, &mut markers, press);
        settle(&ctx, width, &mut markers, release);
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            shown = settle(&ctx, width, &mut markers, Vec::new());
        }
        let slug = name.to_lowercase().replace(' ', "-");
        custom_perks::workbench::tests::capture::write(
            &ctx,
            &shown,
            &format!("markers-{slug}-{width}"),
        );
    }
}
