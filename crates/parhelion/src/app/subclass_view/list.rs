//! The subclass in the game's order at the left of the page: each ability slot's abilities, then
//! each attunement and its nodes.
use super::*;

/// The list's rows, the column that names each row's slot, and the icons in rows.
const ROW_HEIGHT: f32 = 28.0;
const GROUP_WIDTH: f32 = 96.0;
const ROW_ICON: f32 = 22.0;

/// A row of the list: its slot's name when it starts one, its number in a path, its name, the
/// subclass it comes from when that is another, and a dot when it has edits of its own. The
/// selected row is outlined.
struct ListRow<'a> {
    group: &'a str,
    number: Option<u8>,
    /// Whether the row has an icon column, and its icon once loaded. An attunement has none.
    icon_column: bool,
    icon: Option<&'a egui::TextureHandle>,
    name: &'a str,
    detail: Option<&'a str>,
    edited: bool,
    selected: bool,
    color: Option<[u8; 3]>,
}

impl ListRow<'_> {
    fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), ROW_HEIGHT),
            egui::Sense::click(),
        );
        if ui.is_rect_visible(rect) {
            let visuals = ui.visuals();
            let painter = ui.painter_at(rect);
            if self.selected {
                painter.rect(
                    rect,
                    3.0,
                    visuals.widgets.hovered.weak_bg_fill,
                    visuals.selection.stroke,
                    egui::StrokeKind::Inside,
                );
            } else if response.hovered() {
                painter.rect_filled(rect, 3.0, visuals.widgets.hovered.weak_bg_fill);
            }
            let secondary = style::secondary(visuals);
            let text = visuals.text_color();
            let middle = rect.center().y;
            let small = egui::FontId::proportional(12.0);
            if !self.group.is_empty() {
                let group = one_line(ui, self.group, small.clone(), secondary, GROUP_WIDTH - 12.0);
                let height = group.size().y;
                painter.galley(
                    egui::pos2(rect.left() + 8.0, middle - height / 2.0),
                    group,
                    secondary,
                );
            }
            let mut left = rect.left() + GROUP_WIDTH;
            if let Some(number) = self.number {
                painter.text(
                    egui::pos2(left, middle),
                    egui::Align2::LEFT_CENTER,
                    number.to_string(),
                    small.clone(),
                    secondary,
                );
                left += 16.0;
            }
            if self.icon_column {
                if let Some(icon) = self.icon {
                    paint_icon(
                        ui,
                        icon,
                        egui::Rect::from_min_size(
                            egui::pos2(left, middle - ROW_ICON / 2.0),
                            egui::Vec2::splat(ROW_ICON),
                        ),
                    );
                }
                left += ROW_ICON + 6.0;
            }
            // The edit dot keeps the right edge, so a row from another subclass shows it too.
            let mut right = rect.right() - 8.0;
            if let Some([red, green, blue]) = self.color {
                painter.rect(
                    egui::Rect::from_center_size(
                        egui::pos2(right - 6.0, middle),
                        egui::Vec2::splat(12.0),
                    ),
                    2.0,
                    egui::Color32::from_rgb(red, green, blue),
                    egui::Stroke::new(0.5, secondary),
                    egui::StrokeKind::Inside,
                );
                right -= 20.0;
            }
            if self.edited {
                painter.circle_filled(egui::pos2(right - 3.0, middle), 3.0, text);
                right -= 14.0;
            }
            let mut name_right = right;
            if let Some(detail) = self.detail {
                let detail = one_line(ui, detail, small, secondary, (right - left) * 0.4);
                let size = detail.size();
                painter.galley(
                    egui::pos2(right - size.x, middle - size.y / 2.0),
                    detail,
                    secondary,
                );
                name_right = right - size.x - 8.0;
            }
            let name = one_line(
                ui,
                self.name,
                egui::FontId::proportional(13.0),
                text,
                name_right - left,
            );
            let height = name.size().y;
            painter.galley(egui::pos2(left, middle - height / 2.0), name, text);
        }
        if self.edited {
            let name = format!("{} · Changed", self.name);
            return style::named_control(response, &name);
        }
        style::named_control(response, self.name)
    }
}

impl PackageAuthoringApp {
    /// The subclass in the game's order. Returns the row clicked, and whether Restore Base
    /// Abilities was.
    pub(super) fn draw_subclass_list(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
        page: &PageState,
    ) -> (Option<SubclassSelection>, bool) {
        let mut clicked = None;
        let mut restore = false;
        style::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.horizontal(|ui| {
                ui.label(quiet(ui, "Abilities"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    restore = ui
                        .add_enabled(
                            !abilities.is_empty(),
                            egui::Button::new("Restore Base Abilities").small(),
                        )
                        .clicked();
                });
            });
            for (index, slot) in AbilitySlot::ALL.into_iter().enumerate() {
                if index > 0 {
                    ui.add_space(4.0);
                }
                for (position, &entry) in slot.entries().iter().enumerate() {
                    let place = Place::Ability(entry);
                    let group = if position == 0 { slot.label() } else { "" };
                    if let Some(row) = self.draw_entry_row(
                        ui,
                        (base, abilities),
                        place,
                        (group, None),
                        page.selection,
                    ) {
                        clicked = Some(row);
                    }
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(quiet(ui, "Attunements"));
                draw_authoring_info_icon(
                    ui,
                    "Top and bottom attunements trade places. A middle one fills only the middle.",
                );
            });
            // One attunement at a time, as the game shows one tree, each tab marked once its
            // attunement has edits of its own.
            ui.horizontal(|ui| {
                for path in DISPLAY_PATHS {
                    let edited = abilities.attunement(path).is_some();
                    if detail::marked_tab(ui, page.path_tab == path, path.label(), edited).clicked()
                    {
                        clicked = Some(SubclassSelection::Path(path));
                    }
                }
            });
            ui.add_space(4.0);
            if let Some(selection) = self.draw_path_rows(ui, base, abilities, page) {
                clicked = Some(selection);
            }
            ui.add_space(8.0);
            ui.label(quiet(ui, "Always Active"));
            for entry in layout::FOUNDATIONS {
                if let Some(row) = self.draw_entry_row(
                    ui,
                    (base, abilities),
                    Place::Ability(entry),
                    ("", None),
                    page.selection,
                ) {
                    clicked = Some(row);
                }
            }
        });
        (clicked, restore)
    }

    /// One ability's or node's row. Returns its selection when clicked.
    fn draw_entry_row(
        &self,
        ui: &mut egui::Ui,
        (base, abilities): (&SubclassSummary, &SubclassAbilities),
        place: Place,
        (group, number): (&str, Option<u8>),
        selected: SubclassSelection,
    ) -> Option<SubclassSelection> {
        let (source, entry) = source_of(abilities, base.hash, place);
        let summary = find_subclass(&self.subclasses, source);
        let edits = abilities.edits(base.hash, place);
        let name = self.entry_title(abilities, base.hash, place);
        let icon = self.place_icon(ui.ctx(), abilities, base.hash, place);
        // A node names its subclass beside it when that is not its path's.
        let (path_source, _) = own_source(abilities, base.hash, place);
        let description = edits
            .description
            .clone()
            .or_else(|| self.entry_description(summary, entry));
        let kind = match place {
            Place::Ability(entry) => AbilitySlot::of_entry(entry)
                .map_or_else(|| place.label(), |slot| slot.label().to_owned()),
            Place::Node(..) => place.label(),
        };
        let row = ListRow {
            group,
            number,
            icon_column: true,
            icon: icon.as_ref(),
            name: &name,
            detail: summary
                .filter(|summary| summary.hash != path_source)
                .map(|summary| summary.name.as_str()),
            edited: is_own(abilities, place),
            selected: selected == SubclassSelection::Entry(place),
            color: edits.color.or(abilities.hud_color),
        }
        .show(ui)
        .on_hover_ui(|ui| {
            draw_display_tooltip(
                ui,
                DisplayTooltip {
                    icon: icon.as_ref(),
                    name: &name,
                    subtitle: Some(&from_subtitle(&kind, summary)),
                    description: description.as_deref(),
                },
            );
        });
        row.clicked().then_some(SubclassSelection::Entry(place))
    }

    /// An attunement's row, then a row to each of its nodes. Returns the one clicked.
    fn draw_path_rows(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
        page: &PageState,
    ) -> Option<SubclassSelection> {
        let path = page.path_tab;
        let (source, source_path) = abilities.attunement_source(base.hash, path);
        let summary = find_subclass(&self.subclasses, source);
        let own = abilities.attunement(path);
        let name = own
            .and_then(|attunement| attunement.name.as_deref())
            .unwrap_or_else(|| {
                summary.map_or(path.label(), |summary| {
                    attunement_name(summary, source_path)
                })
            });
        let mut clicked = None;
        let row = ListRow {
            group: "",
            number: None,
            icon_column: false,
            icon: None,
            name,
            detail: summary
                .filter(|summary| summary.hash != base.hash)
                .map(|summary| summary.name.as_str()),
            edited: (source, source_path) != (base.hash, path)
                || own.is_some_and(|attunement| attunement.name.is_some()),
            selected: page.selection == SubclassSelection::Path(path),
            color: None,
        }
        .show(ui)
        .on_hover_ui(|ui| {
            draw_display_tooltip(
                ui,
                DisplayTooltip {
                    icon: None,
                    name,
                    subtitle: Some(&from_subtitle(
                        &format!("{} Attunement", path.label()),
                        summary,
                    )),
                    description: Some(&self.path_nodes(base, abilities, path)),
                },
            );
        });
        if row.clicked() {
            clicked = Some(SubclassSelection::Path(path));
        }
        for position in 0..layout::PATH_NODES {
            if let Some(row) = self.draw_entry_row(
                ui,
                (base, abilities),
                Place::Node(path, position),
                ("", Some(position + 1)),
                page.selection,
            ) {
                clicked = Some(row);
            }
        }
        ui.add_space(4.0);
        clicked
    }

    /// An attunement's nodes by name, a line each, as its tooltip lists them.
    fn path_nodes(
        &self,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
        path: AttunementPath,
    ) -> String {
        (0..layout::PATH_NODES)
            .map(|position| self.entry_title(abilities, base.hash, Place::Node(path, position)))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
