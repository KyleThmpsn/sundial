use eframe::egui;
use serde_json::Value;

use super::{
    super::{CollectionStateSnapshot, collection_state_snapshot},
    Edit, apply, rules,
};
use crate::{catalog::Catalog, investment::seasonal::Definition};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    Experience,
    #[default]
    Artifact,
    Rewards,
}

#[derive(Debug, Default)]
pub(in crate::app) struct UiState {
    source: Option<(usize, i32)>,
    xp: i32,
    tab: Tab,
    feedback: Option<String>,
}

impl UiState {
    pub(in crate::app::progression) fn draw_navigation(&mut self, ui: &mut egui::Ui) -> bool {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Artifact, "Artifact Mods");
            ui.selectable_value(&mut self.tab, Tab::Experience, "Seasonal XP");
            ui.selectable_value(&mut self.tab, Tab::Rewards, "Season Pass");
        });
        ui.separator();
        self.tab == Tab::Artifact
    }

    pub(in crate::app::progression) fn invalidate(&mut self) {
        self.source = None;
        self.feedback = None;
    }
}

pub(in crate::app) fn draw(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    state: &mut super::super::UiState,
) -> bool {
    let changed = draw_season(ui, document, catalog, &mut state.seasonal, state.read_only);
    if changed {
        state.cached_progression = None;
    }
    if let Some(hash) = super::super::take_hash_inspection_request(ui.ctx()) {
        let context = super::super::take_hash_inspection_context(ui.ctx(), hash);
        state.hash_inspection.open_with_context(hash, context);
    }
    changed
        | super::super::draw_catalog_hash_window(
            ui.ctx(),
            catalog,
            Some(document),
            !state.read_only,
            &mut state.hash_inspection,
            "seasonal",
        )
}

fn draw_season(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    state: &mut UiState,
    read_only: bool,
) -> bool {
    let Some(definition) = catalog.seasonal() else {
        ui.heading("Seasonal Progression Unavailable");
        ui.label("The installed seasonal definitions could not be validated.");
        if let Some(error) = catalog.progression_package_error() {
            ui.collapsing("Package Details", |ui| {
                ui.label(error);
            });
        }
        return false;
    };
    if document.get("_native_progression").is_none() {
        ui.heading("Seasonal Progression");
        ui.label("Seasonal authoring requires a current Sunrise SQLite account. The existing JSON account controls remain available in Unlocks and Investment.");
        return false;
    }
    let Some(snapshot) = collection_state_snapshot(document) else {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "The account progression state is invalid",
        );
        return false;
    };
    let character = document["_native_progression"]["character_slot"]
        .as_u64()
        .unwrap_or(0) as usize;
    let source = (character, snapshot.seasonal_xp());
    if state.source != Some(source) {
        state.source = Some(source);
        state.xp = source.1;
    }
    let mut requested = None;
    egui::ScrollArea::vertical().id_salt("seasonal_content").show(ui, |ui| {
        if state.tab == Tab::Experience {
        if let Ok(experience) = snapshot.seasonal_experience(definition) {
            let used = snapshot.artifact_mask(definition, true).count_ones();
            ui.horizontal_wrapped(|ui| {
                draw_summary(ui, &snapshot, definition);
            });
            if used > u32::from(experience.points_earned) {
                ui.colored_label(ui.visuals().warn_fg_color, "Owned mods exceed the earned point budget. Remove or reset mods, or increase seasonal XP.");
            }
        } else {
            ui.colored_label(ui.visuals().warn_fg_color, "Saved seasonal XP is negative. Set a nonnegative XP total to repair it.");
        }
        }
        ui.add_space(8.0);
        let editable = !read_only;
        match state.tab {
            Tab::Experience => draw_experience(ui, document, definition, &snapshot, &mut state.xp, editable, &mut requested),
            Tab::Artifact => super::artifact::draw(ui, catalog, definition, &snapshot, editable, &mut requested),
            Tab::Rewards => {
                if let Some(pass) = catalog.progression_definition(rules::PASS_PROGRESSION) {
                    super::rewards::draw_pass(ui, catalog, document, pass);
                }
            }
        }
        if state.tab != Tab::Rewards && let Some(message) = &state.feedback {
            ui.add_space(8.0);
            ui.colored_label(ui.visuals().error_fg_color, message);
        }
    });
    let Some(edit) = requested else { return false };
    match apply(document, catalog, edit) {
        Ok(changed) => {
            state.feedback = None;
            changed
        }
        Err(error) => {
            state.feedback = Some(error);
            false
        }
    }
}

pub(super) fn draw_summary(
    ui: &mut egui::Ui,
    snapshot: &CollectionStateSnapshot,
    definition: &Definition,
) {
    let Ok(experience) = snapshot.seasonal_experience(definition) else {
        return;
    };
    let used = snapshot.artifact_mask(definition, true).count_ones();
    ui.strong(format!("Rank {}", experience.rank));
    ui.separator();
    ui.label(format!("{} XP", experience.total));
    ui.separator();
    ui.label(format!("+{} Artifact Power", experience.power_bonus));
    ui.separator();
    ui.label(format!(
        "{used} / {} Artifact Points Used",
        experience.points_earned
    ));
}

fn draw_experience(
    ui: &mut egui::Ui,
    document: &Value,
    definition: &Definition,
    snapshot: &CollectionStateSnapshot,
    xp: &mut i32,
    editable: bool,
    requested: &mut Option<Edit>,
) {
    ui.label("One XP total updates the season pass, HUD bar, artifact Power, and earned points together. XP above rank 100 continues to count toward the artifact.");
    let changed = ui
        .horizontal(|ui| {
            ui.strong("Seasonal XP");
            ui.add_enabled(
                editable,
                egui::DragValue::new(xp).range(0..=i32::MAX).speed(1_000),
            )
            .changed()
        })
        .inner;
    let Ok(preview) = definition.experience(*xp) else {
        return;
    };
    draw_current_values(ui, snapshot);
    let budget =
        super::editing::validate_character_budgets(document, definition, preview.points_earned);
    if let Err(error) = &budget {
        ui.colored_label(ui.visuals().warn_fg_color, error);
    }
    if changed && editable && budget.is_ok() {
        *requested = Some(Edit::Experience(*xp));
    }
}

fn draw_current_values(ui: &mut egui::Ui, snapshot: &CollectionStateSnapshot) {
    egui::Grid::new("seasonal_xp_breakdown")
        .num_columns(2)
        .striped(true)
        .spacing([28.0, 8.0])
        .show(ui, |ui| {
            ui.strong("Value");
            ui.strong("Current");
            ui.end_row();
            for (index, name, help) in [
                (rules::POWER_PROGRESSION, "Artifact Power XP", "The full seasonal XP total used to earn artifact Power."),
                (rules::POINTS_PROGRESSION, "Artifact Unlock XP", "The same XP total used to earn artifact unlock points."),
                (rules::PASS_PROGRESSION, "Season Pass XP", "Season-pass XP stops at 9,900,000 when rank 100 is reached."),
                (rules::HUD_PROGRESSION, "HUD XP", "The rank bar repeats every 100,000 XP before rank 100, then tracks XP above the pass cap."),
            ] {
                let current = snapshot.account_progressions.iter()
                    .find(|row| row.definition_index == index).map_or(0, |row| row.lanes[0]);
                draw_xp_row(ui, name, help, Some(current));
            }
            for (index, name, help) in [
                (rules::POWER_VALUE, "Artifact Power Bonus", "Bonus Power earned from the installed artifact XP ladder."),
                (rules::EARNED_VALUE, "Artifact Points Earned", "Total mod points earned from the installed artifact unlock ladder."),
                (rules::USED_VALUE, "Artifact Points Used", "Points spent on this character's unlocked artifact mods."),
            ] {
                draw_xp_row(ui, name, help, snapshot.value_overrides.get(&index).copied());
            }
            draw_xp_row(ui, "Season Pass Rank", "The season pass has 100 ranks. Additional XP still advances artifact Power.",
                Some((1 + snapshot.seasonal_xp().max(0) / rules::XP_PER_RANK).min(100)));
        });
}

fn draw_xp_row(ui: &mut egui::Ui, name: &str, help: &str, current: Option<i32>) {
    ui.label(name).on_hover_text(help);
    ui.monospace(current.map_or_else(|| "Not Published".into(), |value| value.to_string()));
    ui.end_row();
}
