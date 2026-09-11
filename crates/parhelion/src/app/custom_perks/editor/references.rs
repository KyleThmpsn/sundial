//! Compact native references with the complete path available for inspection.
use sundial::package_authoring::tft::{Reference, asset_label};

pub(super) fn draw(ui: &mut egui::Ui, references: &[Reference]) {
    egui::CollapsingHeader::new(format!("Asset References · {}", references.len()))
        .id_salt("private-perk-asset-references")
        .show(ui, |ui| {
            if references.is_empty() {
                ui.weak("No native asset references were found for this effect.");
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("private-perk-reference-list")
                .max_height(240.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (index, reference) in references.iter().enumerate() {
                        ui.push_id(index, |ui| {
                            ui.horizontal(|ui| {
                                ui.set_min_height(ui.spacing().interact_size.y);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.small_button("Copy Path").clicked() {
                                            ui.ctx().copy_text(reference.path.clone());
                                        }
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.add(
                                                    egui::Label::new(asset_label(&reference.path))
                                                        .selectable(true)
                                                        .truncate(),
                                                )
                                                .on_hover_text(format!(
                                                    "{}\nSource: {:08X} + {:X}\nTarget: {:08X}",
                                                    reference.path,
                                                    reference.source,
                                                    reference.offset,
                                                    reference.target,
                                                ));
                                            },
                                        );
                                    },
                                );
                            });
                        });
                    }
                });
        });
}
