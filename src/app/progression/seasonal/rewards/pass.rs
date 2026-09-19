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
    job: Option<super::claims::Job>,
    ready: Option<super::claims::Job>,
    feedback: Option<String>,
}

pub(in crate::app::progression::seasonal) fn draw(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    document: &mut Value,
    progression: &ProgressionDefinition,
    editable: bool,
) -> bool {
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
        .data_mut(|data| data.remove_temp::<State>(id))
        .unwrap_or_else(|| State {
            anchor_rank: rank.unwrap_or(1),
            ..Default::default()
        });
    if !editable {
        state.job = None;
        state.ready = None;
    }
    let mut request = None;
    ui.horizontal_wrapped(|ui| {
        draw_filters(ui, &mut state);
        let enabled = editable && state.job.is_none() && state.ready.is_none();
        if ui.add_enabled(enabled && rewards.iter().any(|reward| reward.status == Status::Reached), egui::Button::new("Claim Available")).clicked() {
            request = Some((rewards.iter().filter(|reward| reward.status == Status::Reached).map(|reward| reward.index).collect(), None));
        }
        if ui.add_enabled(enabled && (rank != Some(100) || claimed < rewards.len()), egui::Button::new("Complete Season Pass"))
            .on_hover_text("Reach Rank 100 and queue unclaimed rewards for this character. Review any rewards that cannot be delivered before applying.").clicked() {
            request = Some(((0..rewards.len()).collect(), Some(100)));
        }
    });
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
        if ui
            .add_enabled(
                editable
                    && state.job.is_none()
                    && state.ready.is_none()
                    && selected.status != Status::Claimed,
                egui::Button::new(if selected.status == Status::Locked {
                    "Complete Rank and Claim"
                } else {
                    "Claim Reward"
                }),
            )
            .on_hover_text("Add this reward to Pending Rewards for the selected character")
            .clicked()
        {
            request = Some((
                vec![selected.index],
                (selected.status == Status::Locked)
                    .then_some(selected.definition.rewarded_at_progression_level),
            ));
        }
    }
    let changed = draw_job(ui, document, catalog, progression, &mut state);
    if let Some((indices, rank)) = request {
        match super::claims::Job::new(document, catalog, indices, rank) {
            Ok(job) => {
                state.job = Some(job);
                state.feedback = None;
                ui.ctx().request_repaint();
            }
            Err(error) => state.feedback = Some(error),
        }
    }
    if let Some(message) = &state.feedback {
        ui.colored_label(ui.visuals().error_fg_color, message);
    }
    ui.data_mut(|data| data.insert_temp(id, state));
    changed
}

fn draw_job(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    pass: &ProgressionDefinition,
    state: &mut State,
) -> bool {
    if state.job.is_none() && state.ready.is_none() {
        return false;
    }
    let (changed, close) = crate::app::ui::edit_modal(ui, "season_pass_claim", |ui| {
        if let Some(mut job) = state.job.take() {
            let (done, total) = job.progress();
            if crate::app::ui::modal_progress(ui, "Preparing Rewards", done, total) {
                return false;
            }
            if job.step(catalog, pass) {
                state.ready = Some(job);
            } else {
                state.job = Some(job);
            }
            ui.ctx().request_repaint();
            return false;
        }
        let job = state.ready.as_ref().expect("prepared pass claims");
        let mut counts = vec![(false, format!("{} Claims", job.claimed))];
        if let Some((before, after)) = job.rank_change {
            counts.push((false, format!("Rank {before} → {after}")));
        }
        if job.queued() > 0 {
            counts.push((true, format!("{} Pending Rewards", job.queued())));
        }
        if !job.issues.is_empty() {
            counts.push((true, format!("{} Unclaimed", job.issues.len())));
        }
        crate::app::ui::review_header(ui, "Review Season Pass Changes", &counts);
        crate::app::ui::review_body(ui, "season_pass_review_body", |ui| {
            job.draw_consumables(ui, catalog);
            if job.queues_armor(catalog) {
                ui.label(
                    "Queued armor uses standard rolls. Claim in Sunrise for the fixed Season Pass rolls.",
                );
            }
            if !job.issues.is_empty() {
                egui::CollapsingHeader::new(format!("{} Rewards Left Unclaimed", job.issues.len()))
                    .id_salt("season_pass_review_unclaimed")
                    .show(ui, |ui| {
                        // A full season can leave a long tail, so keep the rows virtualized.
                        egui::ScrollArea::vertical()
                            .id_salt("pass_issues")
                            .max_height(220.0)
                            .show_rows(ui, 42.0, job.issues.len(), |ui, range| {
                                for index in range {
                                    let (name, reason) = &job.issues[index];
                                    ui.add(
                                        egui::Label::new(crate::app::ui::destiny_text(ui, name))
                                            .truncate(),
                                    );
                                    ui.add(egui::Label::new(reason).truncate())
                                        .on_hover_text(reason);
                                }
                            });
                    });
            }
        });
        let (apply, cancel) = crate::app::ui::review_actions(
            ui,
            if job.direct_count() > 0 {
                "Apply Anyway"
            } else {
                "Apply Changes"
            },
            job.changed(),
            "No supported changes to apply.",
        );
        if cancel {
            state.ready = None;
        }
        if apply {
            match state
                .ready
                .take()
                .expect("prepared pass claims")
                .finish(document)
            {
                Ok(changed) => return changed,
                Err(error) => state.feedback = Some(error),
            }
        }
        false
    });
    if close {
        state.job = None;
        state.ready = None;
    }
    changed
}

fn draw_progress(ui: &mut egui::Ui, experience: Option<Experience>, claimed: usize, total: usize) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(experience.map_or_else(
            || "Season Pass".into(), |experience| format!("Rank {}", experience.rank),
        )).size(22.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            crate::ui_help::info(ui, "Checkmarks show claimed rewards. Highlighted rewards have reached their required rank. Claims add supported items to Pending Rewards for the selected character.");
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
