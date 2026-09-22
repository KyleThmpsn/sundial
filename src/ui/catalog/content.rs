//! TFT paths and resolved perk references, using the shared catalog inspector.
use super::{BrowserList, assets};
use crate::{
    investment::{
        InvestmentCatalog, PerkSources, WeaponSandboxPerkChoice,
        discovery::{Catalog, kinds::Family},
    },
    package_runtime::tft,
    sandbox_perk::{dependencies, program::Program},
};
use eframe::egui;
use std::{collections::BTreeMap, sync::Arc};
mod navigation;
mod reference;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Perks,
    Paths,
    References,
}

impl View {
    fn search_hint(self) -> &'static str {
        match self {
            Self::Perks => "Search Perks, Effect Numbers, or Paths",
            Self::Paths => "Search Paths or Source Tags",
            Self::References => "Search Paths, Source Tags, or Target Tags",
        }
    }
}

use std::hash::{Hash, Hasher};

struct Row {
    index: usize,
    label: String,
    detail: String,
    search: String,
}

/// A request to show something that lives on another catalog tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jump {
    Kind(Family, u8),
}

/// What a detail page asked for: drill into a resource, switch tabs, or pick another row.
enum Outcome {
    Open(navigation::Destination),
    Jump(Jump),
    Select(u64),
}

#[derive(Default)]
pub struct Browser {
    queries: [String; 3],
    view: View,
    source: Option<(Arc<tft::Index>, Arc<dependencies::Index>, u64)>,
    rows: Vec<Row>,
    /// Display name per effect position, for pages that name other effects.
    names: Vec<String>,
    filtered: Vec<usize>,
    filter_query: Option<String>,
    resource_names: BTreeMap<u32, Vec<String>>,
    navigation: navigation::Navigation,
    /// An effect number another tab asked to show.
    pending_effect: Option<usize>,
    select: Option<u64>,
}

impl Browser {
    /// Show one effect on the Perk References tab the next time it draws.
    pub fn open_effect(&mut self, effect: usize) {
        self.pending_effect = Some(effect);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        view: View,
        choices: &[WeaponSandboxPerkChoice],
        data: &Catalog,
        packages: Option<&std::path::Path>,
        sources: &PerkSources,
        catalog: Option<&InvestmentCatalog>,
    ) -> Option<Jump> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        if view == View::Perks {
            for choice in choices {
                (choice.perk_index, &choice.representative_name).hash(&mut hasher);
            }
        }
        let labels = hasher.finish();
        let same_source = self
            .source
            .as_ref()
            .is_some_and(|(names, perks, previous)| {
                Arc::ptr_eq(names, &data.names)
                    && Arc::ptr_eq(perks, &data.perks)
                    && *previous == labels
            });
        if self.view != view || !same_source {
            if !same_source {
                self.resource_names = data.names.names();
                self.navigation = navigation::Navigation::new(&data.names);
            } else {
                self.navigation.clear();
            }
            self.rows = rows(data, choices, view, sources);
            self.names = if view == View::Perks {
                let mut names = vec![String::new(); data.perks.perks.len()];
                for row in &self.rows {
                    names[row.index] = row.label.clone();
                }
                names
            } else {
                Vec::new()
            };
            self.view = view;
            self.source = Some((data.names.clone(), data.perks.clone(), labels));
            self.filter_query = None;
        }
        if view == View::Perks
            && let Some(effect) = self.pending_effect.take()
            && let Some(position) = data
                .perks
                .perks
                .iter()
                .position(|perk| perk.index == effect)
        {
            self.navigation.clear();
            self.queries[view as usize].clear();
            self.select = Some(position as u64);
        }
        if self
            .navigation
            .show(ui, data, &self.resource_names, packages)
        {
            return None;
        }
        let mut reset = false;
        let count_rect = ui
            .horizontal(|ui| {
                reset |= super::search(
                    ui,
                    &mut self.queries[view as usize],
                    false,
                    ui.available_width() - 150.0,
                    view.search_hint(),
                );
                ui.allocate_exact_size(
                    egui::vec2(86.0, ui.spacing().interact_size.y),
                    egui::Sense::hover(),
                )
                .0
            })
            .inner;
        let query = self.queries[view as usize].trim().to_ascii_lowercase();
        reset |= self.filter_query.as_ref() != Some(&query);
        if reset {
            self.filtered = self
                .rows
                .iter()
                .enumerate()
                .filter_map(|(index, row)| {
                    query
                        .split_whitespace()
                        .all(|word| row.search.contains(word.strip_prefix("0x").unwrap_or(word)))
                        .then_some(index)
                })
                .collect();
            self.filter_query = Some(query);
        }
        super::toolbar_status(
            ui,
            count_rect,
            format!(
                "{} {}",
                self.filtered.len(),
                if self.filtered.len() == 1 {
                    "Result"
                } else {
                    "Results"
                }
            ),
        );
        ui.separator();
        let keys = self
            .filtered
            .iter()
            .map(|&row| self.rows[row].index as u64)
            .collect::<Vec<_>>();
        let outcome = BrowserList {
            keys: &keys,
            height: (ui.available_height() - 4.0).max(110.0),
            reset,
            row_height: crate::investment::authoring_choice_row_height(ui),
            select: self.select.take(),
        }
        .draw_body(
            ui,
            |ui, index, selected| {
                let row = &self.rows[self.filtered[index]];
                crate::investment::draw_asset_choice_row_plain(
                    ui,
                    &row.label,
                    &row.detail,
                    selected,
                )
            },
            |ui, index| {
                let row = &self.rows[self.filtered[index]];
                ui.heading(&row.label);
                details(
                    ui,
                    data,
                    view,
                    row.index,
                    &self.resource_names,
                    &self.names,
                    sources,
                    catalog,
                )
            },
        );
        match outcome {
            Some(Outcome::Open(destination)) => self.navigation.open(destination),
            Some(Outcome::Jump(jump)) => return Some(jump),
            Some(Outcome::Select(key)) => self.select = Some(key),
            None => {}
        }
        None
    }
}

fn rows(
    data: &Catalog,
    choices: &[WeaponSandboxPerkChoice],
    view: View,
    sources: &PerkSources,
) -> Vec<Row> {
    let labels = choices
        .iter()
        .map(|choice| {
            (
                usize::from(choice.perk_index),
                choice.representative_name.as_str(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut rows: Vec<Row> = match view {
        View::Perks => data
            .perks
            .perks
            .iter()
            .enumerate()
            .map(|(index, perk)| {
                // Weapon perks carry their plug name. Anything else is named by the first
                // item that references it, so armor and ability effects read as words too.
                let name = labels
                    .get(&perk.index)
                    .map(|name| (*name).to_owned())
                    .unwrap_or_else(|| match sources.names(perk.index).as_slice() {
                        [] => format!("Effect {}", perk.index),
                        [name] => (*name).to_owned(),
                        [name, rest @ ..] => format!("{name} +{}", rest.len()),
                    });
                let paths = u16::try_from(perk.index)
                    .ok()
                    .and_then(|index| data.perk_search.get(&index))
                    .map_or("", String::as_str);
                let detail = match &perk.behavior {
                    Some(behavior) => format!("Effect {} · {}", perk.index, behavior.headline),
                    None => format!("Effect {} · 0x{:08X}", perk.index, perk.hash),
                };
                let search = format!("{name} {} {:08X} {paths} {detail}", perk.index, perk.hash);
                Row {
                    index,
                    label: name,
                    detail,
                    search,
                }
            })
            .collect(),
        View::References => data
            .names
            .references
            .iter()
            .enumerate()
            .map(|(index, reference)| Row {
                index,
                label: tft::asset_label(&reference.path),
                detail: reference.path.clone(),
                search: format!(
                    "{} {:08X} {:08X}",
                    reference.path, reference.source, reference.target
                ),
            })
            .collect(),
        View::Paths => data
            .names
            .paths
            .iter()
            .enumerate()
            .map(|(index, path)| Row {
                index,
                label: tft::asset_label(&path.path),
                detail: path.path.clone(),
                search: format!("{} {:08X}", path.path, path.source),
            })
            .collect(),
    };
    rows.sort_by_cached_key(|row| (row.label.to_ascii_lowercase(), row.index));
    for row in &mut rows {
        row.search.make_ascii_lowercase();
    }
    rows
}

#[allow(clippy::too_many_arguments)]
fn details(
    ui: &mut egui::Ui,
    data: &Catalog,
    view: View,
    index: usize,
    names: &BTreeMap<u32, Vec<String>>,
    effect_names: &[String],
    sources: &PerkSources,
    catalog: Option<&InvestmentCatalog>,
) -> Option<Outcome> {
    let mut destination = None;
    let mut outcome = None;
    match view {
        View::Perks => {
            outcome = perk_details(
                ui,
                data,
                index,
                names,
                effect_names,
                sources,
                catalog,
                &mut destination,
            )?;
        }
        View::References => {
            if let Some(reference) = data.names.references.get(index) {
                destination = reference::draw(ui, reference, names);
            }
        }
        View::Paths => {
            if let Some(path) = data.names.paths.get(index) {
                assets::draw_path(ui, &path.path);
                destination = reference::source(ui, names, path.source);
                ui.label("This resource stores the path text. TFT References shows links that resolve to an asset.");
                egui::CollapsingHeader::new("Technical Details").show(ui, |ui| {
                    ui.label(format!("Source Tag: 0x{:08X}", path.source));
                    ui.label(format!(
                        "Path Location: {} bytes from the start of the resource (0x{:X})",
                        path.offset, path.offset
                    ));
                });
                reference::copy_tag(ui, "Copy Source Tag", path.source);
            }
        }
    }
    outcome.or(destination.map(Outcome::Open))
}

/// The native reading: trigger, actions, end and rearm conditions with their values.
fn detail_sections(ui: &mut egui::Ui, sections: &[dependencies::DetailSection]) {
    let groups = sections
        .iter()
        .map(|section| section.group.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut last_group = "";
    for (section_index, section) in sections.iter().enumerate() {
        if groups.len() > 1 && last_group != section.group {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(&section.group).strong().small());
            last_group = &section.group;
        }
        let title = match section.heading.as_str() {
            "Starts When" => "Trigger",
            "Then" => "Actions",
            "Ends When" => "End Condition",
            "Ready Again When" => "Reactivation",
            other => other,
        };
        ui.add_space(2.0);
        ui.weak(title);
        for (line_index, line) in section.lines.iter().enumerate() {
            ui.push_id((section_index, line_index), |ui| {
                ui.horizontal_top(|ui| {
                    ui.add_space(12.0 + line.depth as f32 * 12.0);
                    ui.vertical(|ui| {
                        let name = if line.kind.is_empty() {
                            line.text.as_str()
                        } else {
                            line.kind.as_str()
                        };
                        ui.add(egui::Label::new(name).wrap());
                        for field in &line.fields {
                            ui.add(egui::Label::new(egui::RichText::new(field).weak()).wrap());
                        }
                    });
                });
            });
        }
    }
}

/// The effect page: origin, description, decoded behavior, similar effects and references.
#[allow(clippy::too_many_arguments, clippy::cognitive_complexity)]
fn perk_details(
    ui: &mut egui::Ui,
    data: &Catalog,
    index: usize,
    names: &BTreeMap<u32, Vec<String>>,
    effect_names: &[String],
    sources: &PerkSources,
    catalog: Option<&InvestmentCatalog>,
    destination: &mut Option<navigation::Destination>,
) -> Option<Option<Outcome>> {
    let mut outcome = None;
    let perk = data.perks.perks.get(index)?;
    ui.horizontal_wrapped(|ui| {
        ui.weak(format!("Effect {} · 0x{:08X}", perk.index, perk.hash));
        if let Some(behavior) = &perk.behavior {
            super::support_badge(ui, behavior.support);
        }
    });
    origin_line(ui, sources.get(perk.index));
    if let Some(description) = catalog.and_then(|catalog| {
        u16::try_from(perk.index)
            .ok()
            .and_then(|index| catalog.perk_component_description(index))
    }) && !description.trim().is_empty()
    {
        ui.add(egui::Label::new(description).wrap());
    }
    ui.add_space(6.0);
    ui.strong("Behavior");
    match &perk.behavior {
        Some(behavior) => {
            ui.label(&behavior.headline);
            if behavior.details.is_empty() {
                if let Some(program) = &behavior.program {
                    program_lines(ui, program);
                }
            } else {
                detail_sections(ui, &behavior.details);
            }
            for note in &behavior.notes {
                if !note.starts_with("Conditions in one list are alternatives.")
                    && !note.starts_with("This describes the compiled action.")
                {
                    ui.weak(note);
                }
            }
            for (family, kinds) in [
                (Family::Effects, &behavior.effect_kinds),
                (Family::Conditions, &behavior.condition_kinds),
            ] {
                if let Some(jump) = kind_chips(ui, family, kinds) {
                    outcome = Some(Outcome::Jump(jump));
                }
            }
        }
        None => {
            ui.weak(perk.error.as_deref().unwrap_or("Not decoded."));
        }
    }
    if let Some(key) = similar_effects(ui, data, index, effect_names, sources) {
        outcome = Some(Outcome::Select(key));
    }
    ui.add_space(6.0);
    ui.strong(perk.status());
    if let Some(action) = perk.action {
        ui.label(format!("Action 0x{action:08X}"));
    }
    if let Some(error) = &perk.error {
        ui.colored_label(ui.visuals().warn_fg_color, error);
    }
    if let Some(assets) = data.perk_assets.get(index) {
        for (label, references) in [
            ("Action References", &assets.action),
            ("Graph References", &assets.graphs),
            ("Component References", &assets.components),
        ] {
            if references.is_empty() {
                continue;
            }
            egui::CollapsingHeader::new(format!("{label} ({})", references.len()))
                .default_open(true)
                .show(ui, |ui| {
                    for &index in references {
                        if let Some(next) =
                            reference::draw(ui, &data.names.references[index], names)
                        {
                            *destination = Some(next);
                        }
                    }
                });
        }
    }
    for graph in &perk.graphs {
        *destination = reference::resource_link(
            ui,
            &format!(
                "Graph 0x{:08X} · {} Components",
                graph.tag,
                graph.components.len()
            ),
            graph.tag,
        )
        .or(*destination);
    }
    Some(outcome)
}

/// Which items and plugs carry the effect, on one line.
fn origin_line(ui: &mut egui::Ui, origins: &[crate::investment::PerkSource]) {
    if origins.is_empty() {
        ui.weak("No named item or plug references this effect.");
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.strong(if origins.len() == 1 {
            "From"
        } else {
            "Shared by"
        });
        let named = origins
            .iter()
            .map(|origin| {
                if origin.type_name.is_empty() {
                    origin.name.clone()
                } else {
                    format!("{} ({})", origin.name, origin.type_name)
                }
            })
            .collect::<Vec<_>>();
        let shown = named.iter().take(6).cloned().collect::<Vec<_>>().join(", ");
        ui.label(if named.len() > 6 {
            format!("{shown} and {} more", named.len() - 6)
        } else {
            shown
        });
    });
}

/// Trigger, timing and the recovered actions, one line each. Used when the index predates
/// the native reading.
fn program_lines(ui: &mut egui::Ui, program: &Program) {
    let seconds = |ms: u32| {
        let seconds = f64::from(ms) / 1000.0;
        if seconds.fract() == 0.0 {
            format!("{seconds:.0} s")
        } else {
            format!("{seconds:.1} s")
        }
    };
    let mut timing = vec![program.trigger.label().to_owned()];
    if program.duration_ms > 0 {
        timing.push(seconds(program.duration_ms));
    }
    if program.cooldown_ms > 0 {
        timing.push(format!("{} cooldown", seconds(program.cooldown_ms)));
    }
    if program.chance_permyriad > 0 && program.chance_permyriad < 10_000 {
        timing.push(format!(
            "{}% chance",
            f64::from(program.chance_permyriad) / 100.0
        ));
    }
    ui.label(timing.join(" · "));
    for action in &program.actions {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(action.label());
        });
    }
}

/// One small button per kind, opening the Kinds page for it.
fn kind_chips(ui: &mut egui::Ui, family: Family, kinds: &[u8]) -> Option<Jump> {
    if kinds.is_empty() {
        return None;
    }
    let mut jump = None;
    ui.horizontal_wrapped(|ui| {
        ui.weak(family.label());
        for &kind in kinds {
            let name = family
                .nodes()
                .iter()
                .find(|node| node.kind == kind)
                .map_or_else(|| format!("Kind {kind}"), |node| node.name.to_owned());
            if ui
                .add(egui::Button::new(egui::RichText::new(name).small()).small())
                .on_hover_text(format!("{} kind {kind} · Open in Kinds", family.label()))
                .clicked()
            {
                jump = Some(Jump::Kind(family, kind));
            }
        }
    });
    jump
}

/// Effects built from the same kinds, closest first. Returns the row key of a clicked one.
fn similar_effects(
    ui: &mut egui::Ui,
    data: &Catalog,
    position: usize,
    names: &[String],
    sources: &PerkSources,
) -> Option<u64> {
    let perk = data.perks.perks.get(position)?;
    let behavior = perk.behavior.as_ref()?;
    if behavior.effect_kinds.is_empty() {
        return None;
    }
    let mut similar = data
        .perks
        .perks
        .iter()
        .enumerate()
        .filter(|(other, _)| *other != position)
        .filter_map(|(other, candidate)| {
            let theirs = candidate.behavior.as_ref()?;
            let effects = theirs
                .effect_kinds
                .iter()
                .filter(|kind| behavior.effect_kinds.contains(kind))
                .count();
            if effects == 0 {
                return None;
            }
            let conditions = theirs
                .condition_kinds
                .iter()
                .filter(|kind| behavior.condition_kinds.contains(kind))
                .count();
            let exact = theirs.effect_kinds == behavior.effect_kinds
                && theirs.condition_kinds == behavior.condition_kinds;
            let extra = theirs.effect_kinds.len() - effects;
            let score =
                (usize::from(exact) * 1000 + effects * 10 + conditions * 2).saturating_sub(extra);
            Some((other, score, effects, exact))
        })
        .collect::<Vec<_>>();
    if similar.is_empty() {
        return None;
    }
    similar.sort_by(|a, b| {
        b.1.cmp(&a.1).then_with(|| {
            names
                .get(a.0)
                .map(|n| n.to_lowercase())
                .cmp(&names.get(b.0).map(|n| n.to_lowercase()))
        })
    });
    let exact = similar.iter().filter(|row| row.3).count();
    ui.add_space(6.0);
    ui.strong(if exact > 0 {
        format!(
            "Similar Effects · {exact} same kinds, {} related",
            similar.len() - exact
        )
    } else {
        format!("Similar Effects · {} related", similar.len())
    });
    let total = behavior.effect_kinds.len();
    let height = crate::investment::authoring_choice_row_height(ui);
    let rows = similar.len().min(6) as f32;
    let mut picked = None;
    egui::ScrollArea::vertical()
        .id_salt(("similar-effects", position))
        .max_height(rows * (height + ui.spacing().item_spacing.y) + 4.0)
        .auto_shrink([false, true])
        .show_rows(ui, height, similar.len(), |ui, range| {
            for (other, _, effects, exact) in &similar[range] {
                let candidate = &data.perks.perks[*other];
                let name = names.get(*other).map_or("", String::as_str);
                let how = if *exact {
                    "Same effects and conditions".to_owned()
                } else {
                    format!("Shares {effects} of {total} effects")
                };
                let detail = if sources.has_names(candidate.index) {
                    format!(
                        "Effect {} · {how} · {}",
                        candidate.index,
                        sources.summary(candidate.index)
                    )
                } else {
                    format!("Effect {} · {how}", candidate.index)
                };
                if crate::investment::draw_asset_choice_row_plain(ui, name, &detail, false)
                    .clicked()
                {
                    picked = Some(*other as u64);
                }
            }
        });
    picked
}
