//! An ability's or node's perks: its stock perks less the ones it removes, the stock perks it
//! adds, and its custom perks from the perk workbench, each as a chip. A stock perk's menu edits
//! it as a custom perk, which takes its place once applied.
use super::*;
use crate::app::custom_perks::workbench::AbilityPerk;

/// What the perks field changes.
pub(super) enum Change {
    Edits(Box<EntryEdits>),
    /// Opens a custom perk of the entry in the workbench.
    Open(AbilityPerk),
}

/// A perk as a chip: its icon and name in a button's frame, and a remove icon at its end. It is
/// measured before it is placed, so a full line wraps it whole. Returns the chip's response,
/// whether the remove icon was clicked, and whether the pointer is on it.
pub(super) fn perk_chip(
    ui: &mut egui::Ui,
    icon: Option<&egui::TextureHandle>,
    name: &str,
    sense: egui::Sense,
) -> (egui::Response, bool, bool) {
    let text_color = ui.visuals().text_color();
    let galley = ui.painter().layout_no_wrap(
        name.to_owned(),
        egui::TextStyle::Button.resolve(ui.style()),
        text_color,
    );
    let padding = ui.spacing().button_padding;
    let icon_width = if icon.is_some() {
        CHOICE_ICON + 4.0
    } else {
        0.0
    };
    let remove_width = 20.0;
    let height = ui
        .spacing()
        .interact_size
        .y
        .max(galley.size().y + 2.0 * padding.y)
        .max(CHOICE_ICON + 2.0);
    let width = padding.x + icon_width + galley.size().x + 2.0 + remove_width;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), sense);
    let remove_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - remove_width, rect.top()),
        rect.max,
    );
    let remove = ui.interact(
        remove_rect,
        response.id.with("remove"),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let painter = ui.painter();
        painter.rect(
            rect,
            visuals.corner_radius,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let mut left = rect.left() + padding.x;
        if let Some(icon) = icon {
            paint_icon(
                ui,
                icon,
                egui::Rect::from_min_size(
                    egui::pos2(left, rect.center().y - CHOICE_ICON / 2.0),
                    egui::Vec2::splat(CHOICE_ICON),
                ),
            );
            left += icon_width;
        }
        painter.galley(
            egui::pos2(left, rect.center().y - galley.size().y / 2.0),
            galley,
            text_color,
        );
        let mark = if remove.hovered() {
            text_color
        } else {
            style::secondary(ui.visuals())
        };
        painter.text(
            remove_rect.center(),
            egui::Align2::CENTER_CENTER,
            egui_phosphor::regular::X,
            egui::FontId::proportional(12.0),
            mark,
        );
    }
    let on_remove = remove.hovered();
    let removed = style::named_control(remove, "Remove Perk")
        .on_hover_text("Remove Perk")
        .clicked();
    (response, removed, on_remove)
}

/// Where a perk the entry grants itself comes from: the entry, with the perk's number among its
/// perks when it grants several.
fn own_source(
    (summary, entry): (Option<&SubclassSummary>, u8),
    perk: u16,
) -> Option<(&SubclassSummary, u8, Option<usize>)> {
    let summary = summary?;
    let perks = summary.entry_perks.get(&entry)?;
    let ordinal = perks.iter().position(|each| *each == perk)?;
    Some((summary, entry, (perks.len() > 1).then_some(ordinal + 1)))
}

/// A perk by the entry that grants it, numbered when the entry grants several.
fn source_label((subclass, entry, number): (&SubclassSummary, u8, Option<usize>)) -> String {
    let name = entry_name(subclass, entry);
    number.map_or_else(|| name.to_owned(), |number| format!("{name} {number}"))
}

impl PackageAuthoringApp {
    /// An entry's perks, each with a remove button, then a menu of every stock node's perks and
    /// the command that authors a new one in the workbench. A custom perk opens in the workbench
    /// when clicked.
    pub(super) fn draw_entry_perks(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        own: (Option<&SubclassSummary>, u8),
        edits: &EntryEdits,
        stock_perks: &[u16],
    ) -> Option<Change> {
        let mut changed = None;
        let perks = edits.perks(stock_perks);
        ui.horizontal_wrapped(|ui| {
            for &perk in &perks {
                // A perk the entry grants itself reads as the entry, whichever ability grants it
                // first.
                let source = own_source(own, perk).or_else(|| self.perk_source(perk));
                let label = source.map_or_else(|| format!("Perk {perk}"), source_label);
                let icon = source.and_then(|(subclass, entry, _)| {
                    self.entry_icon(ui.ctx(), Some(subclass), entry)
                });
                let (chip, removed, on_remove) =
                    perk_chip(ui, icon.as_ref(), &label, egui::Sense::click());
                let chip = if on_remove {
                    chip
                } else {
                    chip.on_hover_ui(|ui| {
                        draw_display_tooltip(
                            ui,
                            DisplayTooltip {
                                icon: icon.as_ref(),
                                name: &label,
                                subtitle: source.map(|(subclass, _, _)| subclass.name.as_str()),
                                description: Some(&self.perk_description(perk)),
                            },
                        );
                    })
                };
                chip.context_menu(|ui| {
                    workbench_style(ui);
                    if ui.button("Edit as Custom Perk…").clicked() {
                        changed = Some(Change::Open(AbilityPerk::Stock(perk)));
                        ui.close();
                    }
                });
                if removed {
                    let mut edited = edits.clone();
                    edited.remove_perk(perk);
                    changed = Some(Change::Edits(Box::new(edited)));
                }
            }
            for (index, perk) in edits.custom_perks.iter().enumerate() {
                let icon = perk.icon.as_ref().and_then(|icon| {
                    crate::artwork_browser::preview::texture(ui.ctx(), self.catalog.as_ref()?, icon)
                });
                let (chip, removed, on_remove) =
                    perk_chip(ui, icon.as_ref(), &perk.name, egui::Sense::click());
                let chip = if on_remove {
                    chip
                } else {
                    chip.on_hover_ui(|ui| {
                        draw_display_tooltip(
                            ui,
                            DisplayTooltip {
                                icon: icon.as_ref(),
                                name: &perk.name,
                                subtitle: Some("Custom Perk"),
                                description: (!perk.description.trim().is_empty())
                                    .then_some(perk.description.as_str()),
                            },
                        );
                    })
                };
                if removed {
                    let mut edited = edits.clone();
                    edited.custom_perks.remove(index);
                    changed = Some(Change::Edits(Box::new(edited)));
                } else if chip.clicked() {
                    changed = Some(Change::Open(AbilityPerk::Custom(index)));
                }
            }
            ui.menu_button("Add Perk", |ui| {
                workbench_style(ui);
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        if let Some(perk) = self.draw_perk_menu(ui, base, &perks) {
                            let mut edited = edits.clone();
                            edited.add_perk(perk);
                            changed = Some(Change::Edits(Box::new(edited)));
                            ui.close();
                        }
                    });
            });
            if ui.button("New Custom Perk").clicked() {
                changed = Some(Change::Open(AbilityPerk::New));
            }
        });
        changed
    }

    /// Every stock node's perks that an entry does not have yet, grouped by class. Returns the
    /// one clicked.
    fn draw_perk_menu(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        perks: &[u16],
    ) -> Option<u16> {
        let mut picked = None;
        for (class_type, members) in choices::class_groups(&self.subclasses, base.class_type) {
            ui.label(quiet(
                ui,
                gear_view::class_label(class_type).unwrap_or("Other"),
            ));
            for option in members {
                for (entry, entry_perks) in &option.entry_perks {
                    for (ordinal, perk) in entry_perks.iter().enumerate() {
                        if perks.contains(perk) {
                            continue;
                        }
                        let mut label = format!("{} · {}", entry_name(option, *entry), option.name);
                        if entry_perks.len() > 1 {
                            label = format!("{label} {}", ordinal + 1);
                        }
                        if ui
                            .selectable_label(false, label)
                            .on_hover_text(self.perk_description(*perk))
                            .clicked()
                        {
                            picked = Some(*perk);
                        }
                    }
                }
            }
            ui.separator();
        }
        picked
    }

    /// The first stock entry that grants a sandbox perk: its subclass, its entry, and the perk's
    /// number among that entry's perks when it grants several.
    fn perk_source(&self, perk: u16) -> Option<(&SubclassSummary, u8, Option<usize>)> {
        self.subclasses.iter().find_map(|subclass| {
            subclass.entry_perks.iter().find_map(|(entry, perks)| {
                let ordinal = perks.iter().position(|each| *each == perk)?;
                Some((subclass, *entry, (perks.len() > 1).then_some(ordinal + 1)))
            })
        })
    }

    /// A sandbox perk by the first stock entry that grants it, numbered as the Add Perk list
    /// numbers it when that entry grants several, or by its number.
    fn perk_label(&self, perk: u16) -> String {
        self.perk_source(perk)
            .map_or_else(|| format!("Perk {perk}"), source_label)
    }

    /// A new custom perk that copies a stock perk, named for it.
    pub(in crate::app) fn stock_perk_copy(&self, perk: u16) -> crate::perk::PerkRecipe {
        let mut copy = crate::perk::PerkRecipe::new();
        copy.name = format!("Custom {}", self.perk_label(perk));
        copy.description = self
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.perk_component_description(perk))
            .unwrap_or_default()
            .to_owned();
        copy.effects = vec![crate::perk::PerkRecipe::effect(perk)];
        copy
    }

    fn perk_description(&self, perk: u16) -> String {
        self.catalog
            .as_ref()
            .and_then(|catalog| catalog.perk_component_description(perk))
            .filter(|text| !text.trim().is_empty())
            .map_or_else(|| format!("Perk {perk}"), str::to_owned)
    }
}
