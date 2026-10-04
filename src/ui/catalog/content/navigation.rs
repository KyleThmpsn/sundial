//! Read-only drill-down through recovered links and native resource types.
use super::*;
use crate::ui::catalog::content::{Uses, UsesState};
use std::collections::BTreeSet;
mod graph;
mod structure;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Destination {
    Resource(u32),
    Class(u32),
}

/// A linked resource's tag, its first reference and how many references link it.
type Link = (u32, usize, usize);

#[derive(Default)]
pub(super) struct Navigation {
    history: Vec<Destination>,
    graph_views: BTreeMap<u32, graph::View>,
    structure: structure::Inspector,
    incoming: BTreeMap<u32, Vec<Link>>,
    outgoing: BTreeMap<u32, Vec<Link>>,
    classes: BTreeMap<u32, BTreeSet<u32>>,
    types: BTreeMap<u32, BTreeSet<u32>>,
    paths: BTreeMap<u32, Vec<usize>>,
}

/// One row per linked resource, in the order it is first linked.
fn group_links(
    references: &[tft::Reference],
    lists: BTreeMap<u32, Vec<usize>>,
    incoming: bool,
) -> BTreeMap<u32, Vec<Link>> {
    let mut slots = std::collections::HashMap::<u32, usize>::new();
    lists
        .into_iter()
        .map(|(tag, indices)| {
            slots.clear();
            let mut links: Vec<Link> = Vec::new();
            for index in indices {
                let reference = &references[index];
                let linked = if incoming {
                    reference.source
                } else {
                    reference.target
                };
                let slot = *slots.entry(linked).or_insert(links.len());
                if slot == links.len() {
                    links.push((linked, index, 0));
                }
                links[slot].2 += 1;
            }
            (tag, links)
        })
        .collect()
}

impl Navigation {
    pub(super) fn new(index: &tft::Index) -> Self {
        let mut navigation = Self::default();
        let mut incoming = BTreeMap::<u32, Vec<usize>>::new();
        let mut outgoing = BTreeMap::<u32, Vec<usize>>::new();
        for (index, reference) in index.references.iter().enumerate() {
            incoming.entry(reference.target).or_default().push(index);
            outgoing.entry(reference.source).or_default().push(index);
            for (tag, class) in [
                (reference.source, reference.source_class),
                (reference.target, reference.target_class),
            ] {
                if class != 0 {
                    navigation.classes.entry(class).or_default().insert(tag);
                    navigation.types.entry(tag).or_default().insert(class);
                }
            }
        }
        navigation.incoming = group_links(&index.references, incoming, true);
        navigation.outgoing = group_links(&index.references, outgoing, false);
        for (row, path) in index.paths.iter().enumerate() {
            navigation.paths.entry(path.source).or_default().push(row);
        }
        navigation
    }

    pub(super) fn clear(&mut self) {
        self.history.clear();
    }

    pub(super) fn open(&mut self, destination: Destination) {
        if self.history.last() != Some(&destination) {
            self.history.push(destination);
        }
    }

    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        data: &Catalog,
        names: &BTreeMap<u32, Vec<String>>,
        packages: Option<&std::path::Path>,
        uses: &mut Uses<'_>,
    ) -> bool {
        self.structure.sync(packages);
        if self.history.is_empty() {
            return false;
        }
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                self.history.pop();
            }
            if ui.button("Back to Results").clicked() {
                self.clear();
            }
        });
        let Some(destination) = self.history.last().copied() else {
            return false;
        };
        let mut return_to = None;
        ui.horizontal_wrapped(|ui| {
            let first = self.history.len().saturating_sub(3);
            if first > 0 {
                ui.menu_button("Earlier", |ui| {
                    // Every drill-down adds a step and a menu does not scroll, so a long history
                    // scrolls here, opening on the steps nearest the current page.
                    egui::ScrollArea::vertical()
                        .id_salt("earlier-destinations")
                        .max_height(320.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for (index, item) in self.history.iter().take(first).enumerate() {
                                let title = destination_title(*item, names);
                                if ui.button(title).clicked() {
                                    return_to = Some(index + 1);
                                    ui.close_menu();
                                }
                            }
                        });
                });
            }
            for (index, item) in self.history.iter().enumerate().skip(first) {
                if index > 0 {
                    ui.label("›");
                }
                let title = destination_title(*item, names);
                if index + 1 == self.history.len() {
                    ui.add(egui::Label::new(egui::RichText::new(&title).strong()).truncate())
                        .on_hover_text(&title);
                } else if ui
                    .add(egui::Link::new(egui::RichText::new(&title).underline()))
                    .on_hover_text(&title)
                    .clicked()
                {
                    return_to = Some(index + 1);
                }
            }
        });
        if let Some(length) = return_to {
            self.history.truncate(length);
        }
        let destination = *self.history.last().unwrap_or(&destination);
        if let Destination::Resource(tag) = destination {
            crate::ui::model_preview::selection(
                ui,
                packages,
                tag,
                &destination_title(destination, names),
            );
        }
        ui.separator();
        let next = ui
            .push_id(destination, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("resource-page")
                    .auto_shrink([false, false])
                    .show(ui, |ui| match destination {
                        Destination::Resource(tag) => self.resource(ui, data, names, tag, uses),
                        Destination::Class(class) => self.class(ui, names, class),
                    })
                    .inner
            })
            .inner;
        if let Some(next) = next {
            self.open(next);
        }
        true
    }

    fn resource(
        &mut self,
        ui: &mut egui::Ui,
        data: &Catalog,
        names: &BTreeMap<u32, Vec<String>>,
        tag: u32,
        uses: &mut Uses<'_>,
    ) -> Option<Destination> {
        let mut destination = None;
        ui.heading(
            reference::resource_name(names, tag)
                .map(tft::asset_label)
                .unwrap_or_else(|| "Unnamed Resource".into()),
        );
        if let Some(paths) = names.get(&tag) {
            for path in paths {
                assets::draw_path(ui, path);
            }
        }
        if let Some(classes) = self.types.get(&tag) {
            for &class in classes {
                destination = reference::type_link(ui, "Resource Type", class).or(destination);
            }
        } else if let Some(kind) = registered_kind(tag) {
            ui.label(format!("Resource Type: {kind}"));
        } else {
            ui.label("Resource Type: Not Identified");
        }
        ui.monospace(format!("0x{tag:08X}"));
        reference::copy_tag(ui, "Copy Resource Tag", tag);
        if let Some(paths) = self.paths.get(&tag) {
            egui::CollapsingHeader::new(format!("Stored Paths ({})", paths.len())).show(ui, |ui| {
                for &index in paths {
                    assets::draw_path(ui, &data.names.paths[index].path);
                }
            });
        }
        if let Some(entry) = data.effects.entries.iter().find(|entry| entry.graph == tag) {
            ui.label(entry.kind.label());
            assets::resource_details(ui, entry, &data.effects);
        }
        ui.separator();
        ui.collapsing("Component Structure", |ui| {
            self.structure.show(ui, data, tag);
        });
        let view = self.graph_views.entry(tag).or_default();
        ui.horizontal_wrapped(|ui| {
            ui.strong("Connections");
            ui.selectable_value(&mut view.graph, false, "List");
            ui.selectable_value(&mut view.graph, true, "Graph");
        });
        if view.graph {
            destination = view.show(ui, &data.names, names, tag).or(destination);
        } else {
            destination = self
                .links(ui, data, names, tag, false, None)
                .or(destination);
            destination = self
                .links(ui, data, names, tag, true, Some(uses))
                .or(destination);
        }
        destination
    }

    fn links(
        &self,
        ui: &mut egui::Ui,
        data: &Catalog,
        names: &BTreeMap<u32, Vec<String>>,
        tag: u32,
        incoming: bool,
        // The reverse reference index, which the incoming list draws from as well: the
        // path links are the uses the name scan recovered, and the index has every use.
        uses: Option<&mut Uses<'_>>,
    ) -> Option<Destination> {
        // One row per linked resource. The same target is often linked from several offsets
        // of one resource; the count says so instead of repeating the row.
        let linked = if incoming {
            self.incoming.get(&tag)
        } else {
            self.outgoing.get(&tag)
        }
        .map_or(&[][..], Vec::as_slice);
        let label = if incoming {
            "Used By"
        } else {
            "Referenced Assets"
        };
        let role_of = |tag: u32| {
            self.types.get(&tag).and_then(|types| {
                types
                    .iter()
                    .find_map(|class| crate::runtime::native_type_name(*class))
            })
        };
        let detail_of = |tag: u32, count: usize| {
            let mut detail = role_of(tag).map_or_else(
                || format!("0x{tag:08X}"),
                |role| format!("{role} · 0x{tag:08X}"),
            );
            if count > 1 {
                detail.push_str(&format!(" · {count} links"));
            }
            detail
        };
        let mut rows = linked
            .iter()
            .map(|&(tag, reference, count)| {
                let name = if incoming {
                    reference::resource_name(names, tag)
                        .unwrap_or("Unnamed Resource")
                        .to_owned()
                } else {
                    data.names.references[reference].path.clone()
                };
                (tag, name, detail_of(tag, count))
            })
            .collect::<Vec<_>>();
        if let Some(uses) = &uses
            && let UsesState::Ready(index) = uses.state
        {
            for &source in index.of(tag) {
                if rows.iter().all(|(listed, _, _)| *listed != source) {
                    let name = reference::resource_name(names, source)
                        .unwrap_or("Unnamed Resource")
                        .to_owned();
                    rows.push((source, name, detail_of(source, 1)));
                }
            }
        }
        let total = if incoming
            && uses
                .as_ref()
                .is_some_and(|uses| matches!(uses.state, UsesState::Ready(_)))
        {
            rows.len()
        } else {
            linked.iter().map(|(_, _, count)| count).sum::<usize>()
        };
        let mut uses = uses;
        ui.horizontal(|ui| {
            ui.strong(format!("{label} ({total})"));
            if let Some(uses) = uses.as_deref_mut() {
                uses_controls(ui, uses);
            }
        });
        if let Some(Uses {
            state: UsesState::Failed(error),
            ..
        }) = uses.as_deref()
        {
            ui.colored_label(ui.visuals().error_fg_color, *error);
        }
        if rows.is_empty() {
            ui.label(if incoming {
                "No Incoming Links"
            } else {
                "No Outgoing Links"
            });
            return None;
        }
        let mut destination = None;
        let height = crate::investment::authoring_choice_row_height(ui);
        let visible = rows.len().min(8) as f32;
        egui::ScrollArea::vertical()
            .id_salt(("resource-links", incoming))
            .max_height(visible * (height + ui.spacing().item_spacing.y) + 4.0)
            .auto_shrink([false, true])
            .show_rows(ui, height, rows.len(), |ui, range| {
                for (tag, name, detail) in &rows[range] {
                    if reference::resource_row(ui, name, detail) {
                        destination = Some(Destination::Resource(*tag));
                    }
                }
            });
        destination
    }

    fn class(
        &self,
        ui: &mut egui::Ui,
        names: &BTreeMap<u32, Vec<String>>,
        class: u32,
    ) -> Option<Destination> {
        ui.heading(reference::type_name(class));
        ui.label("Resources that share this engine data type.");
        ui.monospace(format!("Class 0x{class:08X}"));
        let tags = self
            .classes
            .get(&class)
            .map(|tags| tags.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        ui.label(format!(
            "{} {}",
            tags.len(),
            if tags.len() == 1 {
                "Resource"
            } else {
                "Resources"
            }
        ));
        let mut destination = None;
        let height = crate::investment::authoring_choice_row_height(ui);
        egui::ScrollArea::vertical()
            .id_salt("type-resources")
            .show_rows(ui, height, tags.len(), |ui, range| {
                for index in range {
                    let tag = tags[index];
                    let name = reference::resource_name(names, tag)
                        .map(tft::asset_label)
                        .unwrap_or_else(|| "Unnamed Resource".into());
                    if reference::resource_row(ui, &name, &format!("0x{tag:08X}")) {
                        destination = Some(Destination::Resource(tag));
                    }
                }
            });
        destination
    }
}

/// What a resource outside the discovery set is, when the game's tables named it: an ability
/// bank or the entity an ability equips.
fn registered_kind(tag: u32) -> Option<&'static str> {
    if crate::ability::bank::bank_name(tag).is_some() {
        Some("Ability Bank")
    } else if crate::sandbox_perk::entity::catalog::game_name(tag).is_some() {
        Some("Ability Entity")
    } else {
        None
    }
}

/// Beside Used By: the whole-installation read to ask for, or its progress and a way to stop
/// it. A failed read is drawn under the heading, where it has the width, and asked for again
/// from here.
fn uses_controls(ui: &mut egui::Ui, uses: &mut Uses<'_>) {
    match uses.state {
        UsesState::Idle | UsesState::Failed(_) => {
            if ui
                .small_button("Find All Uses")
                .on_hover_text(
                    "Reads every package once for the resources that reference this one. Later reads come from the disk.",
                )
                .clicked()
            {
                uses.requested = true;
            }
        }
        UsesState::Reading(done, total) => {
            ui.weak(format!("Reading {done} of {total} packages"));
            if ui
                .small_button("Stop")
                .on_hover_text(
                    "The packages read so far stay on the disk, so asking again resumes after them.",
                )
                .clicked()
            {
                uses.stopped = true;
            }
        }
        UsesState::Ready(_) => {}
    }
}

fn destination_title(item: Destination, names: &BTreeMap<u32, Vec<String>>) -> String {
    match item {
        Destination::Resource(tag) => reference::resource_name(names, tag)
            .map(tft::asset_label)
            .unwrap_or_else(|| format!("Resource 0x{tag:08X}")),
        Destination::Class(class) => crate::runtime::native_type_name(class)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Type 0x{class:08X}")),
    }
}

#[cfg(test)]
mod tests;
