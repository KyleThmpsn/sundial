//! Read-only drill-down through recovered links and native resource types.
use super::*;
use std::collections::BTreeSet;
mod graph;
mod structure;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Destination {
    Resource(u32),
    Class(u32),
}

#[derive(Default)]
pub(super) struct Navigation {
    history: Vec<Destination>,
    graph_views: BTreeMap<u32, graph::View>,
    structure: structure::Inspector,
    incoming: BTreeMap<u32, Vec<usize>>,
    outgoing: BTreeMap<u32, Vec<usize>>,
    classes: BTreeMap<u32, BTreeSet<u32>>,
    types: BTreeMap<u32, BTreeSet<u32>>,
    paths: BTreeMap<u32, Vec<usize>>,
}

impl Navigation {
    pub(super) fn new(index: &tft::Index) -> Self {
        let mut navigation = Self::default();
        for (index, reference) in index.references.iter().enumerate() {
            navigation
                .incoming
                .entry(reference.target)
                .or_default()
                .push(index);
            navigation
                .outgoing
                .entry(reference.source)
                .or_default()
                .push(index);
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
                    for (index, item) in self.history.iter().take(first).enumerate() {
                        let title = destination_title(*item, names);
                        if ui.button(title).clicked() {
                            return_to = Some(index + 1);
                            ui.close_menu();
                        }
                    }
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
                        Destination::Resource(tag) => self.resource(ui, data, names, tag),
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
            destination = self.links(ui, data, names, tag, false).or(destination);
            destination = self.links(ui, data, names, tag, true).or(destination);
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
    ) -> Option<Destination> {
        let indices = if incoming {
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
        ui.strong(format!("{label} ({})", indices.len()));
        if indices.is_empty() {
            ui.label(if incoming {
                "No incoming links were recovered."
            } else {
                "No outgoing links were recovered."
            });
            return None;
        }
        // One row per linked resource. The same target is often linked from several offsets
        // of one resource; the count says so instead of repeating the row.
        let mut linked: Vec<(u32, &tft::Reference, usize)> = Vec::new();
        for &index in indices {
            let reference = &data.names.references[index];
            let tag = if incoming {
                reference.source
            } else {
                reference.target
            };
            match linked.iter_mut().find(|(other, _, _)| *other == tag) {
                Some((_, _, count)) => *count += 1,
                None => linked.push((tag, reference, 1)),
            }
        }
        let mut destination = None;
        let height = crate::investment::authoring_choice_row_height(ui);
        let rows = linked.len().min(8) as f32;
        egui::ScrollArea::vertical()
            .id_salt(("resource-links", incoming))
            .max_height(rows * (height + ui.spacing().item_spacing.y) + 4.0)
            .auto_shrink([false, true])
            .show_rows(ui, height, linked.len(), |ui, range| {
                for (tag, reference, count) in &linked[range] {
                    let name = if incoming {
                        reference::resource_name(names, *tag)
                            .unwrap_or("Unnamed Resource")
                            .to_owned()
                    } else {
                        reference.path.clone()
                    };
                    let role = self.types.get(tag).and_then(|types| {
                        types
                            .iter()
                            .find_map(|class| crate::weapon_runtime::native_type_name(*class))
                    });
                    let mut detail = role.map_or_else(
                        || format!("0x{tag:08X}"),
                        |role| format!("{role} · 0x{tag:08X}"),
                    );
                    if *count > 1 {
                        detail.push_str(&format!(" · {count} links"));
                    }
                    if reference::resource_row(ui, &name, &detail) {
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

fn destination_title(item: Destination, names: &BTreeMap<u32, Vec<String>>) -> String {
    match item {
        Destination::Resource(tag) => reference::resource_name(names, tag)
            .map(tft::asset_label)
            .unwrap_or_else(|| format!("Resource 0x{tag:08X}")),
        Destination::Class(class) => crate::weapon_runtime::native_type_name(class)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Type 0x{class:08X}")),
    }
}

#[cfg(test)]
mod tests;
