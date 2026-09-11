//! A rank-first reward track. Native bookkeeping stays in the inspector.

use eframe::egui;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::{
    app::progression::collection_state_snapshot,
    catalog::{Catalog, ProgressionDefinition},
    investment::seasonal::{Experience, XP_PER_RANK},
};

use super::card::{self, Reward, Status};

const GAP: f32 = 8.0;
const MIN_COLUMN_WIDTH: f32 = 96.0;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    All,
    Unclaimed,
    Reached,
}

impl Filter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All Rewards",
            Self::Unclaimed => "Unclaimed",
            Self::Reached => "Rank Reached",
        }
    }

    fn includes(self, status: Status) -> bool {
        match self {
            Self::All => true,
            Self::Unclaimed => status != Status::Claimed,
            Self::Reached => status == Status::Reached,
        }
    }
}

#[derive(Clone, Default)]
struct State {
    query: String,
    filter: Filter,
    anchor_rank: i32,
    selected: Option<usize>,
}

pub(in crate::app::progression::seasonal) fn draw(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    document: &Value,
    progression: &ProgressionDefinition,
) {
    let snapshot = collection_state_snapshot(document);
    let experience = snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.seasonal_experience(catalog.seasonal()?).ok());
    let rank = experience.map(|experience| experience.rank);
    let rewards: Vec<_> = progression
        .reward_items
        .iter()
        .enumerate()
        .map(|(index, definition)| Reward {
            index,
            definition,
            status: Status::from_claim(
                super::claimed(catalog, snapshot.as_ref(), definition),
                rank,
                definition.rewarded_at_progression_level,
            ),
        })
        .collect();
    let claimed = rewards
        .iter()
        .filter(|reward| reward.status == Status::Claimed)
        .count();
    draw_progress(ui, experience, claimed, rewards.len());

    let id = ui
        .id()
        .with(("season_pass_track", progression.definition_index));
    let mut state = ui
        .data_mut(|data| data.get_temp::<State>(id))
        .unwrap_or_else(|| State {
            anchor_rank: rank.unwrap_or(1),
            ..Default::default()
        });
    draw_filters(ui, &mut state);
    let groups = filtered_ranks(catalog, &rewards, &state);
    if groups.is_empty() {
        ui.add_space(16.0);
        ui.weak("No rewards match these filters.");
        if ui.button("Clear Filters").clicked() {
            state.query.clear();
            state.filter = Filter::All;
        }
    } else {
        let per_page = ((ui.available_width() + GAP) / (MIN_COLUMN_WIDTH + GAP))
            .floor()
            .clamp(1.0, 10.0) as usize;
        let page = draw_pages(ui, &groups, per_page, rank, &mut state);
        let visible: Vec<_> = groups.iter().skip(page * per_page).take(per_page).collect();
        draw_track(ui, catalog, &visible, rank, &mut state);
    }
    if let Some(selected) = state.selected.and_then(|index| rewards.get(index)) {
        ui.add_space(12.0);
        card::draw_details(ui, catalog, snapshot.as_ref(), *selected);
    }
    ui.data_mut(|data| data.insert_temp(id, state));
}

fn draw_progress(ui: &mut egui::Ui, experience: Option<Experience>, claimed: usize, total: usize) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(experience.map_or_else(
            || "Season Pass".into(), |experience| format!("Rank {}", experience.rank),
        )).size(22.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            crate::ui_help::info(ui, "Checkmarks show claimed rewards. Highlighted rewards have reached their required rank. Claim rewards in Sunrise.");
            ui.weak(format!("{claimed} / {total} Claimed"));
        });
    });
    if let Some(experience) = experience {
        let progress = if experience.rank >= 100 {
            1.0
        } else {
            (experience.total % XP_PER_RANK) as f32 / XP_PER_RANK as f32
        };
        let label = if experience.rank >= 100 {
            "Rank 100 Reached".into()
        } else {
            format!(
                "{} XP to Rank {}",
                XP_PER_RANK - experience.total % XP_PER_RANK,
                experience.rank + 1
            )
        };
        ui.add(egui::ProgressBar::new(progress).desired_height(5.0));
        ui.label(egui::RichText::new(label).size(12.0).weak());
    }
    ui.add_space(10.0);
}

fn draw_filters(ui: &mut egui::Ui, state: &mut State) {
    let before = (state.query.clone(), state.filter);
    ui.horizontal_wrapped(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.query)
                .desired_width(240.0)
                .hint_text("Search Rewards or Rank"),
        );
        egui::ComboBox::from_id_salt("season_pass_filter")
            .selected_text(state.filter.label())
            .show_ui(ui, |ui| {
                for filter in [Filter::All, Filter::Unclaimed, Filter::Reached] {
                    ui.selectable_value(&mut state.filter, filter, filter.label());
                }
            });
    });
    if before != (state.query.clone(), state.filter) {
        state.anchor_rank = 1;
        state.selected = None;
    }
}

fn filtered_ranks<'a>(
    catalog: &Catalog,
    rewards: &[Reward<'a>],
    state: &State,
) -> BTreeMap<i32, Vec<Reward<'a>>> {
    let query = state.query.trim().to_lowercase();
    let mut groups = BTreeMap::<_, Vec<_>>::new();
    for &reward in rewards {
        let level = reward.definition.rewarded_at_progression_level;
        let matches = query.is_empty()
            || level.to_string() == query
            || catalog
                .package_item_name(reward.definition.item_hash)
                .is_some_and(|name| name.to_lowercase().contains(&query));
        if matches && state.filter.includes(reward.status) {
            groups.entry(level).or_default().push(reward);
        }
    }
    groups
}

fn draw_pages(
    ui: &mut egui::Ui,
    groups: &BTreeMap<i32, Vec<Reward<'_>>>,
    per_page: usize,
    rank: Option<i32>,
    state: &mut State,
) -> usize {
    let ranks: Vec<_> = groups.keys().copied().collect();
    let page_count = ranks.len().div_ceil(per_page);
    let mut page = ranks
        .partition_point(|&rank| rank < state.anchor_rank)
        .min(ranks.len() - 1)
        / per_page;
    let range = |page: usize| {
        format!(
            "Ranks {}-{}",
            ranks[page * per_page],
            ranks[((page + 1) * per_page).min(ranks.len()) - 1]
        )
    };
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(page > 0, egui::Button::new("‹"))
            .on_hover_text("Previous Ranks")
            .clicked()
        {
            page -= 1;
        }
        egui::ComboBox::from_id_salt("season_pass_page")
            .selected_text(range(page))
            .show_ui(ui, |ui| {
                for option in 0..page_count {
                    ui.selectable_value(&mut page, option, range(option));
                }
            });
        if ui
            .add_enabled(page + 1 < page_count, egui::Button::new("›"))
            .on_hover_text("Next Ranks")
            .clicked()
        {
            page += 1;
        }
        if ui
            .add_enabled(rank.is_some(), egui::Button::new("Current Rank"))
            .clicked()
        {
            page = ranks
                .partition_point(|&level| level < rank.unwrap_or(1))
                .min(ranks.len() - 1)
                / per_page;
        }
    });
    state.anchor_rank = ranks[page * per_page];
    page
}

fn draw_track(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    groups: &[(&i32, &Vec<Reward<'_>>)],
    rank: Option<i32>,
    state: &mut State,
) {
    ui.add_space(8.0);
    let width =
        ((ui.available_width() - GAP * (groups.len() - 1) as f32) / groups.len() as f32).min(160.0);
    egui::Grid::new("season_pass_track_grid")
        .num_columns(groups.len())
        .spacing([GAP, GAP])
        .show(ui, |ui| {
            for &(level, _) in groups {
                card::draw_rank(ui, width, *level, rank);
            }
            ui.end_row();
            let rows = groups
                .iter()
                .map(|(_, rewards)| rewards.len())
                .max()
                .unwrap_or(0);
            for row in 0..rows {
                for &(_, rewards) in groups {
                    if let Some(&reward) = rewards.get(row) {
                        if card::draw(
                            ui,
                            catalog,
                            reward,
                            width,
                            state.selected == Some(reward.index),
                        )
                        .clicked()
                        {
                            state.selected = Some(reward.index);
                        }
                    } else {
                        ui.allocate_space(egui::vec2(width, card::HEIGHT));
                    }
                }
                ui.end_row();
            }
        });
}
