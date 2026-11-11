//! Native thumbnail and palette controls for the appearance editor.
use crate::character::{Appearance, Catalog, Group};
use eframe::egui;

pub(super) fn show(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    icons: &crate::catalog::Catalog,
    value: &mut Appearance,
    has_saved: bool,
) -> bool {
    let mut changed = false;
    ui.label(egui::RichText::new("Identity").strong().size(17.0));
    ui.add_space(10.0);
    let (mut race, mut gender) = (value.race, value.gender);
    ui.weak("Race");
    ui.horizontal(|ui| {
        for (i, label) in [(0, "Human"), (1, "Awoken"), (2, "Exo")] {
            ui.selectable_value(&mut race, i, label);
        }
    });
    ui.add_space(8.0);
    ui.weak("Gender");
    ui.horizontal(|ui| {
        ui.selectable_value(&mut gender, 0, "Male");
        ui.selectable_value(&mut gender, 1, "Female");
    });
    if (race, gender) != (value.race, value.gender)
        && let Ok(reset) = catalog.reset(value, race, gender)
    {
        *value = reset;
        changed = true;
    }
    ui.add_space(18.0);
    ui.separator();
    ui.add_space(12.0);
    if let Ok(identity) = catalog.identity(value.race, value.gender) {
        for group in identity.groups.iter().filter(|g| g.visible) {
            let selected = group
                .options
                .iter()
                .position(|o| o.value == value.value(group.field));
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&group.label).strong());
                if !group.field.is_color() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.weak(selected.map_or_else(
                            || "Custom Selection".into(),
                            |i| format!("{} of {}", i + 1, group.options.len()),
                        ));
                    });
                }
            });
            let choice = if group.field.is_color() {
                swatches(ui, group, selected)
            } else {
                selector(ui, icons, group, selected)
            };
            if let Some(choice) = choice {
                changed |= catalog.choose(value, group.field, choice).is_ok();
            }
            ui.add_space(14.0);
        }
    }
    ui.separator();
    ui.add_space(12.0);
    if ui
        .button("Reset Selections")
        .on_hover_text("Restore the default options. Undo restores your previous appearance.")
        .clicked()
        && let Ok(reset) = catalog.reset(value, value.race, value.gender)
    {
        *value = reset;
        changed = true;
    }
    ui.add_space(8.0);
    if !has_saved {
        ui.weak("This character uses Sunrise's default appearance. Choose a look to save a custom appearance.");
        if ui.button("Use This Appearance").clicked() {
            changed = true;
        }
    } else {
        ui.weak("Changes join your account edits. Click Save to apply them, then restart Destiny.");
    }
    if let Err(error) = catalog.validate(value) {
        ui.colored_label(ui.visuals().warn_fg_color, error);
    }
    changed
}

fn selector(
    ui: &mut egui::Ui,
    icons: &crate::catalog::Catalog,
    group: &Group,
    selected: Option<usize>,
) -> Option<usize> {
    let mut result = None;
    egui::Grid::new(group.field)
        .min_col_width(0.0)
        .spacing(egui::vec2(6.0, 6.0))
        .show(ui, |ui| {
            for (i, choice) in group.options.iter().enumerate() {
                let texture = choice
                    .icon_container
                    .and_then(|tag| icons.customization_icon_texture(ui.ctx(), tag));
                let button = match &texture {
                    Some(texture) => egui::Button::new(
                        egui::Image::new(texture).fit_to_exact_size(egui::vec2(44.0, 44.0)),
                    ),
                    None => egui::Button::new((i + 1).to_string()),
                };
                let active = selected == Some(i);
                let label = format!("{} Option {}", group.label, i + 1);
                let response = option(
                    ui,
                    button
                        .min_size(egui::vec2(52.0, 52.0))
                        .selected(active)
                        .corner_radius(5),
                    active,
                    &label,
                );
                if response.clicked() {
                    result = Some(i);
                }
                if (i + 1) % 5 == 0 {
                    ui.end_row();
                }
            }
        });
    result
}

fn swatches(ui: &mut egui::Ui, group: &Group, selected: Option<usize>) -> Option<usize> {
    let mut result = None;
    egui::Grid::new(group.field)
        .min_col_width(0.0)
        .spacing(egui::vec2(6.0, 6.0))
        .show(ui, |ui| {
            for (i, choice) in group.options.iter().enumerate() {
                let active = selected == Some(i);
                let color = choice
                    .swatch
                    .map(|[r, g, b]| egui::Color32::from_rgb(r, g, b))
                    .unwrap_or(ui.visuals().widgets.inactive.bg_fill);
                let label = format!("{} {}", group.label, i + 1);
                let response = option(
                    ui,
                    egui::Button::new("")
                        .min_size(egui::vec2(27.0, 27.0))
                        .fill(color)
                        .corner_radius(4),
                    active,
                    &label,
                );
                let rect = response.rect;
                if active {
                    ui.painter().circle_filled(
                        rect.right_bottom() - egui::vec2(5.0, 5.0),
                        2.5,
                        egui::Color32::WHITE,
                    );
                }
                if response.clicked() {
                    result = Some(i);
                }
                if (i + 1) % 9 == 0 {
                    ui.end_row();
                }
            }
        });
    result
}

fn option(
    ui: &mut egui::Ui,
    button: egui::Button<'_>,
    active: bool,
    label: &str,
) -> egui::Response {
    let stroke = egui::Stroke::new(
        if active { 2.0 } else { 1.0 },
        if active {
            ui.visuals().selection.stroke.color
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke.color
        },
    );
    let response = ui.add(button.stroke(stroke));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), label)
    });
    response.on_hover_text(label)
}
