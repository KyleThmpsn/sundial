//! Vehicle settings adapted to the workbench's shared native asset picker.
use super::*;
use crate::app::custom_perks::workbench::assets::AssetScope;
use crate::vehicle::catalog::Capabilities;

pub(super) fn prepare(app: &mut PackageAuthoringApp, ui: &egui::Ui) {
    if app.build_receiver.is_none() && app.install_receiver.is_none() {
        app.perk_workbench
            .prepare_assets(ui.ctx(), &app.packages, app.catalog.as_ref());
    }
}

pub(super) fn capabilities(app: &PackageAuthoringApp, summon: &Summon) -> Capabilities {
    if let Summon::Other { entity } = summon {
        return entity
            .parse_u32()
            .ok()
            .and_then(|graph| {
                let catalog = app.perk_workbench.asset_catalog()?;
                let entry = catalog.entries.iter().find(|entry| entry.graph == graph)?;
                crate::vehicle::catalog::capabilities(catalog, entry)
            })
            .unwrap_or(Capabilities {
                hover: false,
                armed: false,
            });
    }
    Capabilities {
        hover: summon.hover(),
        armed: summon.armed(),
    }
}

pub(super) fn vehicle_label(app: &PackageAuthoringApp, summon: &Summon) -> String {
    match summon {
        Summon::Other { entity } => entity
            .parse_u32()
            .ok()
            .map(|graph| app.perk_workbench.asset_name(graph, "Selected Vehicle"))
            .unwrap_or_else(|| "Selected Vehicle".into()),
        _ => summon.label().into(),
    }
}

pub(super) fn projectile_label(app: &PackageAuthoringApp, projectile: &Projectile) -> String {
    match projectile {
        Projectile::Other { entity } => entity
            .parse_u32()
            .ok()
            .map(|graph| app.perk_workbench.asset_name(graph, "Selected Projectile"))
            .unwrap_or_else(|| "Selected Projectile".into()),
        _ => projectile.label().into(),
    }
}

pub(super) fn vehicle(
    app: &mut PackageAuthoringApp,
    ui: &mut egui::Ui,
    settings: &mut Sparrow,
    opened: bool,
) {
    if app.build_receiver.is_some() || app.install_receiver.is_some() {
        return;
    }
    let current = settings.summon.entity().ok().flatten();
    let selected = ui
        .push_id((&app.recipe.namespace, "summon-choice"), |ui| {
            app.perk_workbench.choose_asset(
                ui,
                &app.packages,
                app.catalog.as_ref(),
                AssetScope::Vehicles,
                opened,
                current,
            )
        })
        .inner;
    if let Some(asset) = selected {
        settings.summon = Summon::Other {
            entity: HexHash::new(asset.graph),
        };
        let supported = capabilities(app, &settings.summon);
        if !supported.hover {
            settings.speed_percent = 100;
            settings.driving = Driving::default();
        }
        if !supported.armed {
            settings.weapons = Weapons::default();
        }
    }
}

pub(super) fn projectile(
    app: &mut PackageAuthoringApp,
    ui: &mut egui::Ui,
    settings: &mut Sparrow,
    opened: bool,
) {
    if app.build_receiver.is_some()
        || app.install_receiver.is_some()
        || !capabilities(app, &settings.summon).armed
    {
        return;
    }
    let current = match &settings.weapons.projectile {
        Projectile::Other { entity } => entity.parse_u32().ok(),
        _ => None,
    };
    let selected = ui
        .push_id(
            (
                &app.recipe.namespace,
                "projectile-choice",
                settings.summon.entity().ok().flatten(),
            ),
            |ui| {
                app.perk_workbench.choose_asset(
                    ui,
                    &app.packages,
                    app.catalog.as_ref(),
                    AssetScope::VehicleProjectiles,
                    opened,
                    current,
                )
            },
        )
        .inner;
    if let Some(asset) = selected {
        settings.weapons.projectile = Projectile::Other {
            entity: HexHash::new(asset.graph),
        };
    }
}
