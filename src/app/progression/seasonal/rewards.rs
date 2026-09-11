//! Claim bookkeeping is shown separately from rank eligibility and actual reward delivery.
use eframe::egui;
use serde_json::Value;

use super::super::{CollectionStateSnapshot, collection_state_snapshot};
use crate::{
    catalog::{Catalog, ProgressionDefinition, ProgressionRewardDefinition},
    investment::seasonal::{PASS_PROGRESSION, RewardGrant},
};

mod card;
mod pass;

pub(super) use pass::draw as draw_pass;

#[derive(Clone, Default)]
struct Filter {
    query: String,
    unclaimed: bool,
}

pub(in crate::app) fn draw(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    document: Option<&Value>,
    progression: &ProgressionDefinition,
) {
    let snapshot = document.and_then(collection_state_snapshot);
    let native = snapshot
        .as_ref()
        .is_some_and(CollectionStateSnapshot::is_native);
    let season = usize::from(progression.definition_index) == PASS_PROGRESSION
        && catalog.seasonal().is_some();
    let rank = snapshot
        .as_ref()
        .filter(|_| native && season)
        .and_then(|snapshot| snapshot.seasonal_experience(catalog.seasonal()?).ok())
        .map(|value| value.rank);
    let id = ui
        .id()
        .with(("reward_filter", progression.definition_index));
    let mut filter = ui.data_mut(|data| data.get_temp::<Filter>(id).unwrap_or_default());
    ui.label(if season {
        "Claim flags record reward delivery. Changing a flag does not grant an item. Rank eligibility checks XP only. Claim in Sunrise to apply its delivery, class, and inventory-space checks."
    } else {
        "Claim references and native bank values are shown here. A flag edit does not grant the linked item."
    });
    ui.horizontal_wrapped(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut filter.query)
                .hint_text("Search Rewards, Rank, or Flag"),
        );
        if native && season {
            ui.checkbox(&mut filter.unclaimed, "Unclaimed Only");
        }
    });
    let query = filter.query.trim().to_lowercase();
    let rows = progression
        .reward_items
        .iter()
        .enumerate()
        .filter(|(_, reward)| {
            let matches = query.is_empty()
                || format!(
                    "{} {} {} {}",
                    catalog
                        .package_item_name(reward.item_hash)
                        .unwrap_or_default(),
                    reward.rewarded_at_progression_level,
                    reward.item_hash,
                    reward
                        .claim_flag
                        .map_or_else(String::new, |flag| flag.to_string())
                )
                .to_lowercase()
                .contains(&query);
            matches
                && (!filter.unclaimed
                    || !season
                    || !native
                    || claimed(catalog, snapshot.as_ref(), reward) != Some(true))
        })
        .collect::<Vec<_>>();
    ui.label(format!(
        "{} / {} Rewards",
        rows.len(),
        progression.reward_items.len()
    ));
    egui::ScrollArea::both()
        .id_salt(("seasonal_reward_rows", progression.definition_index))
        .max_height(460.0)
        .show(ui, |ui| {
            egui::Grid::new(("seasonal_rewards", progression.definition_index))
                .striped(true)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    for heading in [
                        "Rank",
                        "Reward",
                        "Quantity",
                        "Claim Flag",
                        "State",
                        "Delivery",
                    ] {
                        ui.strong(heading);
                    }
                    ui.end_row();
                    for (index, reward) in rows {
                        ui.monospace(reward.rewarded_at_progression_level.to_string())
                            .on_hover_text(format!("Reward Row {index}"));
                        let name = catalog
                            .package_item_name(reward.item_hash)
                            .unwrap_or("Unnamed Reward");
                        ui.vertical(|ui| {
                            ui.set_min_width(240.0);
                            ui.set_max_width(250.0);
                            crate::app::inspector::draw_catalog_hash_link(
                                ui,
                                catalog,
                                reward.item_hash,
                                name,
                            );
                        });
                        ui.monospace(reward.quantity.to_string());
                        draw_claim_flag(ui, catalog, snapshot.as_ref(), reward);
                        let saved = if native {
                            claimed(catalog, snapshot.as_ref(), reward)
                        } else {
                            None
                        };
                        let label = if season {
                            match (saved, rank) {
                                (Some(true), _) => "Claimed",
                                (Some(false), Some(rank))
                                    if rank >= reward.rewarded_at_progression_level =>
                                {
                                    "Rank Eligible"
                                }
                                (Some(false), Some(_)) => "Rank Locked",
                                _ => "Unavailable",
                            }
                        } else {
                            match saved {
                                Some(true) => "Native Flag Set",
                                Some(false) => "Native Flag Clear",
                                None => "Unavailable",
                            }
                        };
                        ui.label(label);
                        let grant = if season {
                            catalog.seasonal().and_then(|definition| {
                                definition.reward_grants.get(&reward.item_hash)
                            })
                        } else {
                            None
                        };
                        draw_grant(ui, catalog, grant);
                        ui.end_row();
                    }
                });
        });
    ui.data_mut(|data| data.insert_temp(id, filter));
}

fn claimed(
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
    reward: &ProgressionRewardDefinition,
) -> Option<bool> {
    let flag = catalog.unlock_flag_definition(usize::from(reward.claim_flag?))?;
    snapshot?.native_flag(flag)
}

fn draw_claim_flag(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    snapshot: Option<&CollectionStateSnapshot>,
    reward: &ProgressionRewardDefinition,
) {
    let Some(index) = reward.claim_flag else {
        ui.weak("None");
        return;
    };
    let Some(flag) = catalog.unlock_flag_definition(usize::from(index)) else {
        ui.label(format!("#{index} · Missing"));
        return;
    };
    ui.vertical(|ui| {
        ui.set_width(150.0);
        crate::app::inspector::draw_catalog_hash_link(ui, catalog, flag.hash, format!("#{index}"));
        ui.weak(flag.compact_slot.map_or_else(|| "No Native Slot".into(), |slot| format!("Bank {} · Slot {slot}", flag.bank())));
        if let Some(value) = snapshot.and_then(|snapshot| snapshot.flag_overrides.get(&usize::from(index))) {
            ui.colored_label(ui.visuals().warn_fg_color, format!("Override {value}"))
                .on_hover_text("Sunrise checks the native account flag for a season-pass claim. This override is a separate client condition input.");
        }
    });
}

fn draw_grant(ui: &mut egui::Ui, catalog: &Catalog, grant: Option<&RewardGrant>) {
    let Some(grant) = grant else {
        ui.weak("Not Classified");
        return;
    };
    let help = match grant {
        RewardGrant::ClassPackage(items) => format!("Sunrise selects the active character's class items from this package:\n{}", items.iter().map(|&hash| catalog.package_item_name(hash).map_or_else(|| format!("{hash:08X}"), str::to_owned)).collect::<Vec<_>>().join("\n")),
        RewardGrant::DestinationResources => "Sunrise grants its nine destination-resource stacks at 50 units each.".into(),
        RewardGrant::LegendaryEngram | RewardGrant::ExoticEngram => "Sunrise decrypts this reward into an item from its installed reward pool.".into(),
        RewardGrant::Item => "Sunrise grants the stated item and quantity, including its normal capacity checks and seasonal armor handling.".into(),
    };
    ui.label(grant.label()).on_hover_text(help);
}
