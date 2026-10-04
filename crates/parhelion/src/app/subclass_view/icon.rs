//! An ability's or node's icon: its source's, another stock ability's, or artwork of its own
//! from the artwork browser the perk workbench uses.
use super::*;

/// The icons in the Ability Icon menu.
const MENU_ICON: f32 = 28.0;

impl PackageAuthoringApp {
    /// What the entry shows now, then the menu of stock ability icons and the artwork browser.
    /// Returns the icon picked.
    pub(super) fn draw_icon_choices(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        (summary, entry): (Option<&SubclassSummary>, u8),
        current: Option<&EntryIcon>,
        page: &mut PageState,
    ) -> Option<EntryIcon> {
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            let reading = match current {
                Some(EntryIcon::Ability { subclass, entry }) => {
                    find_subclass(&self.subclasses, *subclass).map_or_else(
                        || "Another Ability".to_owned(),
                        |subclass| format!("{} · {}", entry_name(subclass, *entry), subclass.name),
                    )
                }
                Some(EntryIcon::Artwork { .. }) => "Artwork".to_owned(),
                None => summary.map_or_else(
                    || "Unknown Ability".to_owned(),
                    |summary| entry_name(summary, entry).to_owned(),
                ),
            };
            // The icon itself leads, so the field reads as the picture it sets.
            let shown = match current {
                Some(EntryIcon::Ability { subclass, entry }) => {
                    self.entry_icon(ui.ctx(), find_subclass(&self.subclasses, *subclass), *entry)
                }
                Some(EntryIcon::Artwork { .. }) => None,
                None => self.entry_icon(ui.ctx(), summary, entry),
            };
            if let Some(icon) = shown {
                ui.add(egui::Image::new(&icon).fit_to_exact_size(egui::Vec2::splat(CHOICE_ICON)));
            }
            ui.label(reading);
            ui.menu_button("Ability Icon", |ui| {
                workbench_style(ui);
                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        if let Some(icon) = self.draw_ability_icon_menu(ui, base, current) {
                            picked = Some(icon);
                            ui.close_menu();
                        }
                    });
            });
            let artwork = match current {
                Some(EntryIcon::Artwork { artwork }) => Some(artwork),
                _ => None,
            };
            let selection = crate::app::pickers::browser_with_toolbar(
                ui,
                "subclass-artwork",
                "Artwork…",
                "Choose Artwork",
                &mut page.artwork_query,
                |ui, query, opened, height| {
                    page.artwork.draw(
                        ui,
                        query,
                        opened,
                        height,
                        crate::artwork_browser::Browser {
                            packages: Some(self.packages.as_path()),
                            catalog: self.catalog.as_ref(),
                            current: artwork,
                        },
                    )
                },
            );
            if let Some(crate::artwork_browser::Selection::Icon(artwork)) = selection {
                picked = Some(EntryIcon::Artwork { artwork });
            }
        });
        picked
    }

    /// Every stock ability's and node's icon, a line to each subclass, grouped by class. Returns
    /// the one clicked.
    fn draw_ability_icon_menu(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        current: Option<&EntryIcon>,
    ) -> Option<EntryIcon> {
        let mut picked = None;
        for (class_type, members) in choices::class_groups(&self.subclasses, base.class_type) {
            ui.label(quiet(
                ui,
                gear_view::class_label(class_type).unwrap_or("Other"),
            ));
            for subclass in members {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(LABEL_WIDTH, MENU_ICON),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(LABEL_WIDTH);
                            ui.add(egui::Label::new(subclass.name.as_str()).truncate());
                        },
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.set_max_width(MENU_ICON * 9.0);
                        for &entry in subclass.entry_icons.keys() {
                            let Some(icon) = self.entry_icon(ui.ctx(), Some(subclass), entry)
                            else {
                                continue;
                            };
                            let key = EntryIcon::Ability {
                                subclass: subclass.hash,
                                entry,
                            };
                            let button = egui::Button::image(
                                egui::Image::new(&icon)
                                    .fit_to_exact_size(egui::Vec2::splat(MENU_ICON)),
                            )
                            .selected(current == Some(&key));
                            let name = entry_name(subclass, entry);
                            if ui.add(button).on_hover_text(name).clicked() {
                                picked = Some(key);
                            }
                        }
                    });
                });
            }
            ui.separator();
        }
        picked
    }
}
