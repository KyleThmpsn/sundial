//! The Projectile row of a card: the projectile it fires, the browser that swaps it and the
//! names the browser shows.
use super::*;

/// What the Projectile browser lists: the stock abilities' projectiles, named by the ability that
/// fires each, and the engine catalog, once loaded, with the names the perk workbench gives.
pub(super) struct Sources<'a> {
    pub(super) donors: Option<&'a Donors>,
    pub(super) catalog:
        Option<&'a sundial::package_authoring::sandbox_perk::entity::catalog::Catalog>,
    pub(super) name: &'a dyn Fn(u32) -> Option<String>,
}

/// The projectile a card's graphs fire: the original, or another one chosen in the browser the
/// perk workbench picks projectiles with. It lists the stock abilities' projectiles until its
/// filter asks for all of them.
pub(super) fn swap_row(
    ui: &mut egui::Ui,
    (card, own_graphs): (&Card, &BTreeSet<u32>),
    sources: &Sources<'_>,
    properties: &mut Properties,
    edits: &mut EntryEdits,
) {
    let swaps = card
        .graphs
        .iter()
        .filter_map(|(tag, parent)| Some(((*parent)?, *tag)))
        .collect::<Vec<_>>();
    let Some(&(parent, tag)) = swaps.first() else {
        return;
    };
    let current = edits.swap(parent, tag);
    let named = |each: u32| projectile_name(sources, each);
    // A swap to a projectile no stock ability fires takes its name from the engine catalog.
    if current.is_some_and(|each| named(each).is_none()) {
        properties.wants_catalog = true;
    }
    // The projectile it fires before any swap, by the ability that fires it.
    let original = named(tag).unwrap_or_else(|| card.title.clone());
    let shown = current.map_or_else(
        || original.clone(),
        |each| named(each).unwrap_or_else(|| format!("Projectile 0x{each:08X}")),
    );
    let mut chosen = current;
    ui.horizontal(|ui| {
        ui.label(quiet(ui, "Projectile"));
        let picked = crate::app::pickers::browser(
            ui,
            ("subclass-swap", parent, tag),
            &shown,
            "Choose a Projectile",
            &mut String::new(),
            |ui, query, reset, height| {
                swap_choices(
                    ui,
                    (card, sources, &mut *properties),
                    (&original, current),
                    (query, reset, height),
                )
            },
        );
        if let Some(picked) = picked {
            chosen = picked;
        }
        if current.is_some() && detail::reset_icon(ui) {
            chosen = None;
        }
    });
    if chosen != current {
        // The values of the projectile no longer swapped in go with it, unless the ability's own
        // tree holds the same graph.
        if let Some(old) = current {
            let graphs = properties
                .trees
                .get(&old)
                .and_then(|tree| tree.as_ref().ok())
                .map_or_else(|| BTreeSet::from([old]), |tree| tree_graphs(tree));
            edits.ability_values.retain(|value| {
                value.locator.graph_tag.is_none_or(|tag| {
                    !graphs.contains(&tag.get()) || own_graphs.contains(&tag.get())
                })
            });
        }
        for &(parent, tag) in &swaps {
            edits.set_swap(parent, tag, chosen);
        }
    }
    // The projectile swapped in deals the ability's damage type unless one is set for it here.
    if edits.swap(parent, tag).is_none() {
        return;
    }
    let damage = edits.swap_damage(parent, tag);
    let mut choice = damage;
    let label = |choice: Option<crate::recipe::RecipeDamageType>| {
        choice
            .and_then(|choice| {
                detail::DAMAGE_TYPES
                    .iter()
                    .find(|(each, ..)| *each == choice)
            })
            .map_or("Ability's", |(_, _, label)| label)
    };
    ui.horizontal(|ui| {
        ui.label(quiet(ui, "Damage Type"));
        let response = egui::ComboBox::from_id_salt(("subclass-swap-damage", parent, tag))
            .selected_text(label(damage))
            .show_ui(ui, |ui| {
                style::workbench_style(ui);
                ui.selectable_value(&mut choice, None, "Ability's");
                for (each, _, name) in detail::DAMAGE_TYPES {
                    ui.selectable_value(&mut choice, Some(each), name);
                }
            })
            .response;
        let _ = style::named_control(response, "Projectile Damage Type");
        if damage.is_some() && detail::reset_icon(ui) {
            choice = None;
        }
    });
    if choice != damage {
        for &(parent, tag) in &swaps {
            edits.set_swap_damage(parent, tag, choice);
        }
    }
}

/// A projectile's name: the stock ability that fires it, else the engine catalog's.
pub(super) fn projectile_name(sources: &Sources<'_>, graph: u32) -> Option<String> {
    sources
        .donors
        .and_then(|donors| donors.as_ref().ok())
        .and_then(|donors| donors.iter().find(|(donor, _)| *donor == graph))
        .map(|(_, name)| name.clone())
        .or_else(|| (sources.name)(graph))
}

/// The Projectile browser's listing: the original first, then each projectile the filter and
/// search
/// take, the stock abilities' by default or every one in the engine catalog. Names that repeat,
/// from abilities two subclasses both name, are numbered as the workbench numbers its variants.
/// Returns the choice once used: `None` for the original.
pub(super) fn swap_choices(
    ui: &mut egui::Ui,
    (card, sources, properties): (&Card, &Sources<'_>, &mut Properties),
    (original, current): (&str, Option<u32>),
    (query, reset, height): (&str, bool, f32),
) -> Option<Option<u32>> {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut properties.all_projectiles, false, "Abilities");
        ui.selectable_value(&mut properties.all_projectiles, true, "All Projectiles")
            .on_hover_text("Weapons', vehicles' and enemies' too");
    });
    let own = |donor: u32| card.graphs.iter().any(|(tag, _)| *tag == donor);
    let mut rows = vec![(None, original.to_owned())];
    if properties.all_projectiles {
        properties.wants_catalog = true;
        let Some(catalog) = sources.catalog else {
            ui.weak("Loading…");
            return None;
        };
        let mut others = catalog
            .entries
            .iter()
            .filter(|entry| {
                entry.kind == sundial::package_authoring::sandbox_perk::entity::Kind::Projectile
                    && !own(entry.graph)
            })
            .map(|entry| {
                let title = projectile_name(sources, entry.graph).unwrap_or_else(|| entry.label());
                (Some(entry.graph), title)
            })
            .collect::<Vec<_>>();
        others.sort_by(|(a_tag, a), (b_tag, b)| a.cmp(b).then(a_tag.cmp(b_tag)));
        rows.extend(others);
    } else {
        let donors = match sources.donors {
            Some(Ok(donors)) => donors,
            Some(Err(error)) => {
                ui.weak("Projectiles unavailable.")
                    .on_hover_text(error.as_str());
                return None;
            }
            None => {
                ui.weak("Loading…");
                return None;
            }
        };
        let others = donors
            .iter()
            .filter(|(donor, _)| !own(*donor))
            .collect::<Vec<_>>();
        let mut counts = BTreeMap::<&str, usize>::new();
        for (_, label) in &others {
            *counts.entry(label.as_str()).or_default() += 1;
        }
        let mut seen = BTreeMap::<&str, usize>::new();
        for (donor, label) in others {
            let title = if counts.get(label.as_str()).is_some_and(|count| *count > 1) {
                let number = seen.entry(label.as_str()).or_default();
                *number += 1;
                format!("{label} · Variant {number}")
            } else {
                label.clone()
            };
            rows.push((Some(*donor), title));
        }
    }
    rows.retain(|(_, title)| query.is_empty() || title.to_lowercase().contains(query));
    // Donor tags are never zero, so zero stands for the original.
    let key = |tag: Option<u32>| tag.map_or(0, u64::from);
    let keys = rows.iter().map(|(tag, _)| key(*tag)).collect::<Vec<_>>();
    crate::app::pickers::BrowserList {
        keys: &keys,
        height,
        reset,
        row_height: sundial::investment::authoring_choice_row_height(ui),
        select: reset.then(|| key(current)),
    }
    .draw_with_actions_activating(
        ui,
        |ui, index, selected| {
            let (tag, title) = &rows[index];
            let detail = tag.map_or_else(|| "Original".to_owned(), |tag| format!("0x{tag:08X}"));
            sundial::investment::draw_asset_choice_row(ui, title, &detail, selected)
        },
        |ui, index, activated| (ui.button("Use").clicked() || activated).then(|| rows[index].0),
    )
}
