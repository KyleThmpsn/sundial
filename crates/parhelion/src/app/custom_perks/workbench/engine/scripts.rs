//! Installed object behavior resources and their known stock perk uses.
use super::*;
use sundial::ui::catalog::BrowserList;

#[derive(Default)]
pub(super) struct Browser {
    query: String,
    source: Option<usize>,
    matches: Vec<usize>,
    filtered_query: Option<String>,
}

impl Browser {
    pub(super) fn draw(&mut self, ui: &mut egui::Ui, data: &discovery::Data) -> Option<u32> {
        let source = Arc::as_ptr(&data.scripts) as usize;
        if self.source != Some(source) {
            self.source = Some(source);
            self.filtered_query = None;
        }
        let (changed, count_slot) = sundial::ui::catalog::search_bar(
            ui,
            &mut self.query,
            "Search Object Behaviors, Paths, or Tags",
        );
        let query = self.query.trim().to_ascii_lowercase();
        if self.filtered_query.as_ref() != Some(&query) {
            self.matches = data
                .scripts
                .iter()
                .enumerate()
                .filter(|(_, script)| {
                    let text = format!(
                        "{} {} {} {:08x}",
                        script.title, script.path, script.stock_perks, script.tag
                    )
                    .to_ascii_lowercase();
                    query
                        .split_whitespace()
                        .all(|word| text.contains(word.strip_prefix("0x").unwrap_or(word)))
                })
                .map(|(index, _)| index)
                .collect();
            self.filtered_query = Some(query);
        }
        sundial::ui::catalog::toolbar_status(
            ui,
            count_slot,
            sundial::ui::catalog::result_count(self.matches.len()),
        );
        ui.separator();
        let keys = self
            .matches
            .iter()
            .map(|&index| u64::from(data.scripts[index].tag))
            .collect::<Vec<_>>();
        BrowserList {
            keys: &keys,
            height: ui.available_height().max(120.0),
            reset: changed,
            row_height: sundial::investment::authoring_choice_row_height(ui),
            select: None,
        }
        .draw_body(
            ui,
            |ui, row, selected| {
                let script = &data.scripts[self.matches[row]];
                sundial::investment::draw_asset_choice_row_plain(
                    ui,
                    &script.title,
                    &script.path,
                    selected,
                )
            },
            |ui, row| {
                let script = &data.scripts[self.matches[row]];
                ui.heading(&script.title);
                ui.label(&script.path);
                ui.monospace(format!("Resource 0x{:08X}", script.tag));
                let open = ui
                    .horizontal(|ui| {
                        let open = ui.button("Open Resource").clicked();
                        if ui.button("Copy Resource Tag").clicked() {
                            ui.ctx().copy_text(format!("0x{:08X}", script.tag));
                        }
                        open
                    })
                    .inner;
                if script.stock_perks.is_empty() {
                    ui.weak("No Stock Uses");
                } else {
                    ui.strong("Stock Uses");
                    ui.label(&script.stock_perks);
                }
                open.then_some(script.tag)
            },
        )
    }
}
