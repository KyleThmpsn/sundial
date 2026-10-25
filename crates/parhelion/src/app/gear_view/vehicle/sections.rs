//! The Vehicle cards: driving, durability, weapons and handling, each its values as tiles.
use super::controls::*;
use crate::{
    app::style,
    vehicle::{Driving, Durability, Handling, Projectile, Sparrow, Summon, Weapons},
};

pub(super) fn driving(ui: &mut egui::Ui, settings: &mut Sparrow, hover: bool) {
    const PRESETS: [(&str, &str); 4] = [
        ("0.5× Speed", "Half its own speed"),
        ("2× Speed", "Twice its own speed"),
        ("3× Speed", "Three times its own speed"),
        (
            "Match Speed",
            "Acceleration, braking and boost follow Driving Speed",
        ),
    ];
    let presets: &[(&str, &str)] = if hover { &PRESETS } else { &[] };
    match header(
        ui,
        "Driving",
        "How it drives",
        settings.motion_changes(),
        presets,
    ) {
        Some(Action::Reset) => {
            settings.speed_percent = 100;
            settings.driving = Driving::default();
        }
        Some(Action::Preset(3)) => {
            settings.driving = Driving {
                acceleration_percent: settings.speed_percent,
                braking_percent: settings.speed_percent,
                boost_percent: settings.speed_percent,
            };
        }
        Some(Action::Preset(index)) => settings.speed_percent = [50, 200, 300][index],
        None => {}
    }
    // A vehicle that does not hover, such as the Tank, keeps its own driving.
    if !hover {
        unavailable(ui, "Fixed for this vehicle");
        return;
    }
    style::tiles(ui, |ui, width| {
        percent(
            ui,
            width,
            (
                "Driving Speed",
                "Top speed, forward and reverse. The Speed stat is display only.",
            ),
            (&mut settings.speed_percent, 1),
        );
        percent(
            ui,
            width,
            ("Acceleration", "Acceleration under throttle"),
            (&mut settings.driving.acceleration_percent, 1),
        );
        percent(
            ui,
            width,
            ("Braking", "Slowing once the throttle is released"),
            (&mut settings.driving.braking_percent, 1),
        );
        percent(
            ui,
            width,
            ("Boost Strength", "Force of both boosts"),
            (&mut settings.driving.boost_percent, 1),
        );
    });
}

pub(super) fn handling(ui: &mut egui::Ui, settings: &mut Sparrow) {
    if let Some(Action::Reset) = header(
        ui,
        "Handling",
        "Stock traits added beside the item's perks",
        settings.handling.has_changes(),
        &[],
    ) {
        settings.handling = Handling::default();
    }
    let sparrow = settings.summon == Summon::Sparrow;
    let handling = &mut settings.handling;
    ui.horizontal_wrapped(|ui| {
        trait_toggle(
            ui,
            &mut handling.side_dodges,
            ("Improved Side Dodges", "Adds Vernier Thrusters"),
            sparrow,
        );
        trait_toggle(
            ui,
            &mut handling.air_control,
            ("Air Control", "Adds Air Control"),
            sparrow,
        );
        trait_toggle(
            ui,
            &mut handling.roll_tricks,
            ("Roll Tricks", "Adds Destabilizers"),
            sparrow,
        );
        trait_toggle(
            ui,
            &mut handling.fast_summon,
            ("Faster Summoning", "Adds Improved Assembler"),
            true,
        );
    });
}

pub(super) fn durability(ui: &mut egui::Ui, settings: &mut Sparrow) {
    const PRESETS: [(&str, &str); 1] = [(
        "Resilient",
        "Double health and repair rate, half the repair delay",
    )];
    match header(
        ui,
        "Durability",
        "Health and existing self-repair",
        settings.durability != Durability::default(),
        &PRESETS,
    ) {
        Some(Action::Reset) => settings.durability = Durability::default(),
        Some(Action::Preset(_)) => {
            settings.durability = Durability {
                health_percent: 200,
                repair_delay_percent: 50,
                repair_rate_percent: 200,
            };
        }
        None => {}
    }
    let durability = &mut settings.durability;
    style::tiles(ui, |ui, width| {
        percent(
            ui,
            width,
            ("Health", "Health and shields of every region"),
            (&mut durability.health_percent, 1),
        );
        percent(
            ui,
            width,
            ("Repair Delay", "Wait before self-repair. 0% removes it."),
            (&mut durability.repair_delay_percent, 0),
        );
        percent(
            ui,
            width,
            ("Repair Rate", "Self-repair speed"),
            (&mut durability.repair_rate_percent, 1),
        );
    });
}

/// Returns whether Choose Other Projectile… was picked, which opens the projectile picker.
pub(super) fn weapons(
    ui: &mut egui::Ui,
    settings: &mut Sparrow,
    armed: bool,
    projectile_label: &str,
) -> bool {
    const PRESETS: [(&str, &str); 2] = [
        ("Rapid Fire", "Double firing rate"),
        ("Double Damage", "Double weapon damage"),
    ];
    let presets: &[(&str, &str)] = if armed { &PRESETS } else { &[] };
    match header(
        ui,
        "Weapons",
        "The vehicle's guns",
        settings.weapons != Weapons::default(),
        presets,
    ) {
        Some(Action::Reset) => settings.weapons = Weapons::default(),
        Some(Action::Preset(0)) => settings.weapons.firing_rate_percent = 200,
        Some(Action::Preset(_)) => settings.weapons.damage_percent = 200,
        None => {}
    }
    if !armed {
        unavailable(ui, "Unarmed");
        return false;
    }
    let weapons = &mut settings.weapons;
    style::tiles(ui, |ui, width| {
        percent(
            ui,
            width,
            ("Weapon Damage", "Damage per shot"),
            (&mut weapons.damage_percent, 1),
        );
        percent(
            ui,
            width,
            (
                "Firing Rate",
                "Rate of fire. Charge times can still limit it.",
            ),
            (&mut weapons.firing_rate_percent, 1),
        );
        projectiles(ui, width, &mut weapons.projectile, projectile_label)
    })
}

/// The Projectiles tile: the rounds the vehicle fires, its own or another vehicle's. Returns
/// whether Choose Other Projectile… was picked.
fn projectiles(
    ui: &mut egui::Ui,
    width: f32,
    projectile: &mut Projectile,
    other_label: &str,
) -> bool {
    let current = projectile.clone();
    let modified = current != Projectile::Stock;
    let ((choice, choose_other), reset) = style::tile(
        ui,
        width,
        "vehicle-projectiles",
        "Projectiles",
        "Rounds it fires",
        modified,
        |ui| {
            let mut choice = current.clone();
            let mut choose_other = false;
            let selected = if matches!(current, Projectile::Other { .. }) {
                other_label
            } else {
                current.label()
            };
            let combo = egui::ComboBox::from_id_salt("vehicle-projectiles")
                .selected_text(selected)
                .width(width)
                .truncate()
                .show_ui(ui, |ui| {
                    style::workbench_style(ui);
                    for option in [
                        Projectile::Stock,
                        Projectile::Pike,
                        Projectile::HeavyPike,
                        Projectile::Interceptor,
                        Projectile::SuperInterceptor,
                        Projectile::Tank,
                    ] {
                        let label = option.label();
                        ui.selectable_value(&mut choice, option, label);
                    }
                    if ui
                        .selectable_label(false, "Choose Other Projectile…")
                        .on_hover_text("Search and preview projectiles")
                        .clicked()
                    {
                        choose_other = true;
                        ui.close();
                    }
                });
            style::named_control(combo.response, "Vehicle Projectiles");
            (choice, choose_other)
        },
    );
    if reset {
        *projectile = Projectile::Stock;
    } else if choice != current {
        *projectile = choice;
    }
    choose_other
}
