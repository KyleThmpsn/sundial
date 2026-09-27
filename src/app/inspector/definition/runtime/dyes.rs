//! Reuse the supported material decoder and the inspector's guarded background reader.
use super::*;

pub(in super::super) fn draw_dye_colors(
    ui: &mut egui::Ui,
    metadata: &ItemPackageMetadata,
    state: &mut RuntimeInspectionState,
) {
    let indices = dye_indices(metadata);
    if indices.is_empty() {
        return;
    }
    crate::app::inspector::look::section(
        ui,
        ("inspector-dye-colors", metadata.definition_index),
        "Dye Material Colors",
        Some(indices.len()),
        false,
        |ui| {
            let id = egui::Id::new(("dye-reference", metadata.definition_index));
            let mut selected = ui
                .data_mut(|data| data.get_temp::<u16>(id))
                .filter(|index| indices.contains(index))
                .unwrap_or(indices[0]);
            ui.horizontal_wrapped(|ui| {
                ui.label("Dye Reference");
                egui::ComboBox::from_id_salt(id)
                    .selected_text(selected.to_string())
                    .show_ui(ui, |ui| {
                        for index in &indices {
                            ui.selectable_value(&mut selected, *index, index.to_string());
                        }
                    });
            });
            ui.data_mut(|data| data.insert_temp(id, selected));
            let loaded = state.draw_request(ui, RuntimeTarget::Dye(selected));
            if let Some(Ok(LoadedDetails::Dye(colors))) = loaded.as_deref() {
                for (label, rgb) in [("Primary", colors.primary), ("Secondary", colors.secondary)] {
                    draw_color(ui, label, rgb);
                }
                if ui.button("Copy Material Colors").clicked() {
                    ui.ctx().copy_text(
                        serde_json::json!({
                            "source": "installed_package_base_material_tints",
                            "dye_reference_index": selected,
                            "primary_linear_rgb": colors.primary,
                            "secondary_linear_rgb": colors.secondary,
                        })
                        .to_string(),
                    );
                }
            }
        },
    );
}

fn dye_indices(metadata: &ItemPackageMetadata) -> Vec<u16> {
    let rows: Vec<_> = if metadata
        .translation_dye_rows
        .iter()
        .any(|lane| !lane.is_empty())
    {
        metadata.translation_dye_rows.iter().flatten().collect()
    } else {
        metadata.render_overrides.iter().collect()
    };
    rows.into_iter()
        .filter(|row| row.key >= 0 && row.value != u16::MAX)
        .map(|row| row.value)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn draw_color(ui: &mut egui::Ui, label: &str, rgb: [f32; 3]) {
    let color = egui::Color32::from(egui::Rgba::from_rgb(
        rgb[0].min(1.0),
        rgb[1].min(1.0),
        rgb[2].min(1.0),
    ));
    ui.horizontal_wrapped(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 22.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 3.0, color);
        ui.painter().rect_stroke(
            rect,
            3.0,
            ui.visuals().widgets.noninteractive.bg_stroke,
            egui::StrokeKind::Inside,
        );
        ui.label(label);
        ui.monospace(format!(
            "#{:02X}{:02X}{:02X}",
            color.r(),
            color.g(),
            color.b()
        ));
        ui.label("Linear RGB");
        ui.monospace(format!("{}, {}, {}", rgb[0], rgb[1], rgb[2]));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn color_targets_exclude_disabled_rows_and_deduplicate_references() {
        let metadata: ItemPackageMetadata = serde_json::from_value(serde_json::json!({
            "definition_index": 0, "definition_tag": 0,
            "translation_dye_rows": [[{"stage":0,"key":0,"value":80},{"stage":0,"key":-1,"value":90}],
                [{"stage":1,"key":1,"value":80},{"stage":1,"key":2,"value":65535}],[]]
        })).unwrap();
        assert_eq!(dye_indices(&metadata), [80]);
    }
}
