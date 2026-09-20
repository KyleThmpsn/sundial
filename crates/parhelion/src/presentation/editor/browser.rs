use super::*;
use crate::artwork_browser::Browser;

impl Editor {
    pub(crate) fn show_with_sources(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        catalog: Option<&sundial::investment::InvestmentCatalog>,
    ) -> Option<Action> {
        if !self.browsing {
            return self.show(ctx);
        }
        self.poll();
        let size = ctx.screen_rect().size();
        let modal =
            egui::Modal::new(egui::Id::new("presentation-artwork-browser")).show(ctx, |ui| {
                crate::app::workbench_style(ui);
                ui.set_width((size.x - 48.0).clamp(280.0, 880.0));
                ui.horizontal(|ui| {
                    ui.heading(match self.kind {
                        Kind::Badge => "Choose Badge Artwork",
                        Kind::Watermark => "Choose Watermark Artwork",
                    });
                    if ui.button("Back").clicked() {
                        self.browsing = false;
                    }
                });
                ui.weak(match self.kind {
                    Kind::Badge => "Choose a source, then crop and position it on the badge.",
                    Kind::Watermark => {
                        "Choose a transparent source, then size and position the watermark."
                    }
                });
                let height = (size.y - 180.0).clamp(180.0, 600.0);
                if let Some(selection) = self.browser.draw(
                    ui,
                    &mut self.browser_query,
                    false,
                    height,
                    Browser {
                        packages: Some(packages),
                        catalog,
                        current: None,
                    },
                ) {
                    self.importing = Some(self.browser.artwork(selection, packages, catalog, ctx));
                    self.browsing = false;
                }
            });
        if modal.should_close() {
            self.browsing = false;
        }
        None
    }
}
