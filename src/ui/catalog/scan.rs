//! Snapshot coverage and read failures, without interpreting missing data as absence.
use crate::investment::discovery::Catalog;
use eframe::egui;

pub fn show(ui: &mut egui::Ui, data: &Catalog) {
    egui::ScrollArea::vertical().max_height(500.0).show(ui, |ui| {
        egui::Grid::new("scan-counts").striped(true).show(ui, |ui| {
            for (label,count) in [
                ("Resources Scanned",data.names.scanned_resources),
                ("Stored TFT Paths",data.names.paths.len()),
                ("Resolved TFT Links",data.names.references.len()),
                ("Candidate Entity Links",data.names.entity_references.len()),
                ("Objects and Effects",data.effects.entries.len()),
                ("Component Resources",data.effects.owners.len()),
                ("Perk Records",data.perks.perks.len()),
                ("Decoded Perk Behaviors",data.perks.perks.iter().filter(|p|p.behavior.is_some()).count()),
                ("Pattern Records",data.perks.patterns.len()),
                ("Resolved Pattern Entities",data.perks.patterns.iter().filter(|p|p.entity.is_some()).count()),
            ] { ui.label(label); ui.label(count.to_string()); ui.end_row(); }
        });
        ui.label("Counts describe the loaded catalog snapshot. Missing links or fields may not have been decoded.");
        ui.label("Candidate entity links are matching package values, not confirmed TFT links. They are excluded from the resource graph.");
        let errors=data.names.errors.iter().chain(&data.effects.errors).map(String::as_str)
            .chain(data.perks.perks.iter().filter_map(|p|p.error.as_deref()))
            .chain(data.perks.patterns.iter().filter_map(|p|p.error.as_deref())).collect::<Vec<_>>();
        egui::CollapsingHeader::new(format!("Read Errors ({})",errors.len())).show(ui, |ui| {
            if errors.is_empty() { ui.label("No read errors were recorded."); }
            for error in errors { ui.label(error); }
        });
    });
}
