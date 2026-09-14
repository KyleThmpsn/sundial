//! The native structure view describes stored values without inventing editable semantics.
use super::*;

#[cfg(test)]
mod tests;

pub(super) fn draw(ui: &mut egui::Ui, loaded: &PrivatePerkRuntimeGraph, query: &str) {
    egui::CollapsingHeader::new("Decoded Native Structure")
        .id_salt("private-perk-native-structure")
        .show(ui, |ui| {
            ui.small("Read-only values and links from native declarations. These are asset defaults. Unmapped entries identify the known wire operation without guessing its package layout.");
            for (tag, graph) in &loaded.graphs {
                let mut fields = BTreeMap::new();
                let mut issues = BTreeSet::new();
                let roots = graph.resources.iter().flat_map(|resource| {
                    std::iter::once(&resource.instance).chain(resource.definition.iter())
                        .map(move |root| (resource.owner_tag, root))
                }).chain(graph.owners.iter().flat_map(|owner| {
                    owner.roots.iter().map(move |root| (owner.owner_tag, root))
                }));
                for (owner, root) in roots {
                    for field in &root.structure.fields {
                        let matches = query.is_empty()
                            || format!("{} {} {} 0x{:08X}", field.label, field.representation, field.value, field.schema)
                                .to_ascii_lowercase().contains(query);
                        if matches {
                            fields.entry((owner, field.owner_offset, &field.representation)).or_insert(field);
                        }
                    }
                    issues.extend(root.structure.issues.iter().map(|issue| format!("Owner 0x{owner:08X}: {issue}")));
                }
                let unmapped = fields.values().filter(|field| field.representation.starts_with("Unmapped")).count();
                ui.label(format!("Asset 0x{tag:08X}: {} Readable Entries, {unmapped} Unmapped", fields.len() - unmapped));
                let fields = fields.into_iter().collect::<Vec<_>>();
                egui::ScrollArea::vertical().id_salt(("native-structure-values", tag))
                    .max_height(280.0).show_rows(ui, ui.text_style_height(&egui::TextStyle::Body), fields.len(), |ui, range| {
                        for &((owner, offset, _), field) in &fields[range] {
                            let text = format!("{} · {} · {}", field.label, field.representation, field.value);
                            ui.add(egui::Label::new(text).truncate().selectable(true)).on_hover_text(format!(
                                "{}\n{}\n{}\nOwner 0x{owner:08X} at +0x{offset:X}\nDeclaring Type 0x{:08X} at +0x{:X}",
                                field.label, field.representation, field.value, field.schema, field.schema_offset));
                        }
                    });
                for issue in issues { ui.colored_label(ui.visuals().warn_fg_color, issue); }
            }
        });
}
