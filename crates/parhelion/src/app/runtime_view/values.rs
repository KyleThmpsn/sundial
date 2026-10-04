use super::*;

const RUNTIME_VALUES_HELP: &str =
    "Raw fields from the runtime and component donors. Patches apply at build time.";

impl PackageAuthoringApp {
    pub(in crate::app) fn draw_runtime_value_column(
        &mut self,
        ui: &mut egui::Ui,
        graph: Option<&Arc<WeaponRuntimeGraph>>,
    ) {
        if let Some(graph) = graph {
            self.draw_runtime_values(ui, graph);
        } else {
            draw_donor_section_label(ui, "Runtime Values", Some(RUNTIME_VALUES_HELP));
            ui.weak("Not loaded yet.");
        }
    }

    pub(in crate::app) fn draw_runtime_values(
        &mut self,
        ui: &mut egui::Ui,
        graph: &Arc<WeaponRuntimeGraph>,
    ) {
        let scope = egui::Id::new(("parhelion-runtime-values", self.recipe_panel_scope()));
        // The values end the Runtime page, so they scroll with it.
        draw_page_value_panel(
            ui,
            graph,
            ValuePanel {
                title: "Runtime Values",
                help: RUNTIME_VALUES_HELP,
                scope,
                query: &mut self.runtime_value_query,
                cache: &mut self.runtime_values_cache,
                text: &mut self.runtime_value_text,
                overrides: &mut self.recipe.overrides.runtime_values,
                show_experimental: self.show_experimental_options,
                show_technical: &mut self.show_technical_runtime_values,
                readable_first: false,
            },
        );
    }
}
