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
    let screen = ctx.content_rect();
    // Escape leaves as Cancel does, once no dropdown is open to take it first.
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) && !egui::Popup::is_any_open(ctx) {
        return Some(Action::Cancel);
    }
    egui::Window::new("Select Custom Perk")
        .id(egui::Id::new("custom-perk-picker"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(screen.center())
        .default_width(560.0_f32.min((screen.width() - 40.0).max(280.0)))
        .show(ctx, |ui| {
            crate::app::style::perk_workbench_style(ui);
            ui.label(picker.target.label(donor, catalog));
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(stale.is_none(), egui::Button::new("Create Custom Perk…"))
                    .clicked()
                {
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
            let response = ui.add(
                egui::TextEdit::singleline(&mut picker.query)
                    .hint_text("Search Custom Perks")
                    .desired_width(f32::INFINITY),
            );
            // The search takes the keyboard when the window opens.
            if ui.memory(|memory| memory.focused().is_none()) {
                response.request_focus();
            }
            crate::app::style::named_control(response, "Search Custom Perks");
            if let Some(index) = draw_choices(ui, picker, catalog, stale.is_none(), screen.height())
            {
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
    // A source every row shares says nothing, so rows then show only their description.
    let shared_source = picker
        .choices
        .windows(2)
        .all(|pair| pair[0].source == pair[1].source);
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
                let description = choice.recipe.description.trim();
                let detail = match (shared_source, description.is_empty()) {
                    (true, _) => description.to_owned(),
                    (false, true) => choice.source.clone(),
                    (false, false) => format!("{} · {description}", choice.source),
                };
                let response = ui
                    .add_enabled_ui(enabled && choice.issue.is_none(), |ui| {
                        let icon = crate::artwork_browser::preview::icon(
                            ui,
                            catalog,
                            choice.recipe.icon.as_ref(),
                        );
                        catalog.draw_authoring_choice_row_with_icon(
                            ui,
                            choice.recipe.template_plug.parse_u32().ok(),
                            &choice.recipe.name,
                            (!detail.is_empty()).then_some(detail.as_str()),
                            false,
                            icon,
                        )
                    })
                    .inner;
                if response.clicked() {
                    selected = Some(index);
                }
                if let Some(issue) = &choice.issue {
                    response.on_disabled_hover_text(issue);
                    ui.small(issue);
                } else if let Some(warning) = &choice.warning {
                    let color = crate::app::style::secondary(ui.visuals());
                    ui.small(egui::RichText::new(warning).color(color));
                }
            }
            if !visible {
                ui.label(if picker.choices.is_empty() {
                    "No Custom Perks"
                } else {
                    "No Matching Results"
                });
            }
        });
    selected
}
