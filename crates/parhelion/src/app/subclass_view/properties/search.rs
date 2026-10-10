//! Search the editable parts, including dormant values and closed groups.
use super::*;

pub(in crate::app::subclass_view) fn matches(query: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    query
        .split_whitespace()
        .all(|word| text.contains(&word.to_lowercase()))
}

pub(in crate::app::subclass_view) fn search_bar(ui: &mut egui::Ui, query: &mut String) {
    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(query)
                .hint_text("Find Properties")
                .desired_width((ui.available_width() - 65.0).max(100.0)),
        );
        style::named_control(response, "Find Properties");
        if !query.is_empty()
            && style::named_control(ui.small_button("Clear"), "Clear Property Search").clicked()
        {
            query.clear();
        }
    });
}

impl PackageAuthoringApp {
    pub(in crate::app::subclass_view) fn draw_property_search(
        &self,
        ui: &mut egui::Ui,
        loaded: &Loaded,
        edits: &EntryEdits,
        page: &mut PageState,
    ) -> Option<EntryEdits> {
        let mut next = edits.clone();
        let query = page.property_query.trim().to_owned();
        let mut trees = vec![(loaded.entity, Arc::clone(&loaded.tree))];
        let mut loading = false;
        let mut seen = BTreeSet::from([loaded.entity]);
        // Each replacement and its children carry their own graph scope. Old descendants are
        // excluded below, exactly as on the unfiltered page.
        for swap in &edits.spawn_swaps {
            if !seen.insert(swap.replacement) {
                continue;
            }
            match page
                .properties
                .poll(ui.ctx(), &self.packages, swap.replacement)
            {
                Some(Ok(tree)) => trees.push((swap.replacement, tree)),
                Some(Err(error)) => {
                    ui.colored_label(ui.visuals().warn_fg_color, error);
                }
                None => loading = true,
            }
        }
        let mut shown = 0;
        for (tree_index, (root, tree)) in trees.iter().enumerate() {
            for (card_index, card) in tree.cards.iter().enumerate() {
                let removed = |tag: u32| {
                    let mut at = tag;
                    while let Some(&parent) = tree.parents.get(&at) {
                        if edits.swap(parent, at).is_some() {
                            return true;
                        }
                        at = parent;
                    }
                    false
                };
                if card.graphs.iter().all(|(tag, _)| removed(*tag)) {
                    continue;
                }
                let lanes = if *root == loaded.entity && loaded.is_own(card) {
                    loaded.lanes()
                } else {
                    Vec::new()
                };
                let card_matches = matches(&query, &card.title);
                let properties = card
                    .properties
                    .iter()
                    .enumerate()
                    .filter(|(_, property)| {
                        !shadowed(&lanes, property)
                            && (card_matches
                                || matches(
                                    &query,
                                    &format!("{} {}", property.label, property.hint),
                                ))
                    })
                    .collect::<Vec<_>>();
                let lanes = lanes
                    .into_iter()
                    .filter(|lane| card_matches || matches(&query, lane.label))
                    .collect::<Vec<_>>();
                if properties.is_empty() && lanes.is_empty() {
                    continue;
                }
                shown += properties.len() + lanes.len();
                ui.push_id(("property-search", tree_index, card_index), |ui| {
                    style::card(ui, |ui| {
                        card_header(ui, card);
                        style::tiles(ui, |ui, width| {
                            for (index, property) in properties {
                                let inactive =
                                    idle(property, &card.properties, &next.ability_values);
                                let before = property.current(&next.ability_values);
                                property_tile_state(
                                    ui,
                                    width,
                                    index,
                                    property,
                                    &mut next.ability_values,
                                    inactive,
                                );
                                keep_ordered(
                                    property,
                                    before,
                                    &card.properties,
                                    &mut next.ability_values,
                                );
                            }
                            for (index, lane) in lanes.into_iter().enumerate() {
                                lane_tile(
                                    ui,
                                    width,
                                    card.properties.len() + index,
                                    lane,
                                    &mut next,
                                );
                            }
                        });
                    });
                });
            }
        }
        if loading {
            ui.weak("Loading replacement properties…");
        } else if shown == 0 {
            ui.weak("No matching part properties.");
        }
        (next != *edits).then_some(next)
    }
}
