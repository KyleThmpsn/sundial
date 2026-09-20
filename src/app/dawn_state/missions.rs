use super::*;
use crate::persistence::dawn_account::ActivityState;

pub(super) fn draw(
    ui: &mut egui::Ui,
    _catalog: &crate::catalog::Catalog,
    state: &mut ActivityState,
    owner: &str,
) {
    let mut count = 0;
    for mission in state
        .missions
        .iter_mut()
        .filter(|r| r.owner.eq_ignore_ascii_case(owner))
    {
        count += 1;
        let name = mission_name(mission.hash);
        egui::CollapsingHeader::new(name).id_salt((&mission.owner,mission.hash)).show(ui,|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut mission.completed,"Completed").on_hover_text("Changes the saved completion record only. Does not deliver rewards or replay mission scripts.");
                if ui.button("Clear Checkpoint").on_hover_text("Clears the stored checkpoint and progress. Does not restart an activity or change completion and reward history.").clicked() {
                    mission.checkpoint=0;mission.slice=0;mission.progress=0;
                }
            });
            egui::Grid::new(("mission-fields",mission.hash)).spacing([16.0,6.0]).show(ui,|ui| {
                ui.label("Progress");ui.add(egui::DragValue::new(&mut mission.progress).range(0..=i32::MAX));ui.end_row();
                ui.label("Activity Index");ui.label(mission.activity.to_string());ui.end_row();
                ui.label("Checkpoint");ui.label(format!("{:08X}",mission.checkpoint));ui.end_row();
                ui.label("Slice Set");ui.label(mission.slice.to_string());ui.end_row();
            });
        });
    }
    if count == 0 {
        ui.weak("No saved missions for this character.");
    }
}

// Dawn mission_progress.cpp hashes the scenario package name, not an activity definition hash.
// Keep native package names verbatim, including records shared by multiple activity variants.
fn mission_name(hash: u32) -> String {
    mission_scenario(hash)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Mission {hash:08X}"))
}

/// The scenario package whose FNV-1a name hash Dawn stores for a mission, when it is one this
/// build knows.
pub(in crate::app) fn mission_scenario(hash: u32) -> Option<&'static str> {
    [
        "mission_scot",
        "mission_abs",
        "adventure_ginger",
        "adventure_vod",
        "mission_ember",
        "adventure_whisk",
        "strike_bond",
        "mission_bond",
        "strike_pact",
        "mission_pact",
        "mission_launchpad",
        "adventure_rumba",
    ]
    .into_iter()
    .find(|name| {
        name.bytes().fold(2166136261_u32, |h, b| {
            (h ^ u32::from(b)).wrapping_mul(16777619)
        }) == hash
    })
}
