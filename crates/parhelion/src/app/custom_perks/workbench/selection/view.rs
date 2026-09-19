//! Draws the selection dialog and returns user intent without changing a weapon.
use super::*;

pub(super) fn show(
    ctx: &egui::Context,
    picker: &mut Picker,
    donor: &WeaponDonor,
    catalog: &InvestmentCatalog,
    stale: Option<&str>,
) -> Option<Action> {
    let mut open = true;
    let mut action = None;
    let screen = ctx.screen_rect();
    egui::Window::new("Select Custom Perk")
        .id(egui::Id::new("custom-perk-picker"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(560.0_f32.min((screen.width() - 40.0).max(280.0)))
        .show(ctx, |ui| {
            workbench_style(ui);
            ui.label(picker.target.label(donor, catalog));
            ui.label("Choose a perk to use in this choice. Custom perks do not need to be installed first.");
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(stale.is_none(), egui::Button::new("Create Custom Perk…")).clicked() {
                    action = Some(Action::Create);
                }
                if ui.button("Cancel").clicked() {
                    action = Some(Action::Cancel);
                }
            });
            if let Some(error) = stale.or(picker.error.as_deref()) {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            draw_warnings(ui, &picker.warnings);
            ui.separator();
            let response = ui.add(egui::TextEdit::singleline(&mut picker.query)
                .hint_text("Search Custom Perks").desired_width(f32::INFINITY));
            crate::app::style::named_control(response, "Search Custom Perks");
            if let Some(index) = draw_choices(ui, picker, catalog, stale.is_none(), screen.height()) {
                action = Some(Action::Use(index));
            }
        });
    if open { action } else { Some(Action::Cancel) }
}

fn draw_warnings(ui: &mut egui::Ui, warnings: &[String]) {
    if warnings.is_empty() {
        return;
    }
    ui.collapsing("Some Perk Sources Could Not Be Loaded", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("custom-perk-source-warnings")
            .max_height(90.0)
            .show(ui, |ui| {
                for warning in warnings {
                    ui.small(warning);
                }
            });
    });
}

fn draw_choices(
    ui: &mut egui::Ui,
    picker: &Picker,
    catalog: &InvestmentCatalog,
    enabled: bool,
    screen_height: f32,
) -> Option<usize> {
    let query = picker.query.trim().to_lowercase();
    let mut selected = None;
    let mut visible = false;
    egui::ScrollArea::vertical()
        .id_salt("custom-perk-results")
        .max_height((screen_height - 300.0).clamp(100.0, 380.0))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (index, choice) in picker
                .choices
                .iter()
                .enumerate()
                .filter(|(_, choice)| choice.matches(&query))
            {
                visible = true;
                let detail = if choice.recipe.description.trim().is_empty() {
                    choice.source.clone()
                } else {
                    format!("{} · {}", choice.source, choice.recipe.description)
                };
                let response = ui
                    .add_enabled_ui(enabled && choice.issue.is_none(), |ui| {
                        catalog.draw_authoring_choice_row(
                            ui,
                            choice.recipe.template_plug.parse_u32().ok(),
                            &choice.recipe.name,
                            Some(&detail),
                            false,
                        )
                    })
                    .inner;
                if response.clicked() {
                    selected = Some(index);
                }
                if let Some(issue) = &choice.issue {
                    response.on_disabled_hover_text(issue);
                    ui.small(issue);
                }
            }
            if !visible {
                ui.label(if picker.choices.is_empty() {
                    "No custom perks yet. Create one to use it in this choice."
                } else {
                    "No matching custom perks."
                });
            }
        });
    selected
}
