//! Library navigation and transactional build selection are intentionally separate.
use super::*;

mod actions;
mod browser;
mod delete;
mod restore;
mod sharing;
mod state;
use actions::{EntryAction, LibraryAction};
pub(crate) use state::LibraryState;
use state::SortOrder;

struct LibraryRowResponse {
    activated: bool,
    action: Option<EntryAction>,
}

#[derive(Clone, Copy, Default)]
struct LibraryRowState {
    current: bool,
    inclusion: Option<bool>,
    highlighted: bool,
    reveal: bool,
    can_restore: bool,
    /// The installation carries the item this recipe builds.
    installed: bool,
}

/// What a library row's icon is drawn from. The watermark goes by its fingerprint, and the
/// worker reads its artwork from the recipe, so the list holds none.
#[derive(Clone, Debug, Eq, PartialEq)]
struct IconKey {
    corner_icon: Option<u64>,
    item_hash: u32,
    container_tag: u32,
    rarity: crate::AuthoredWeaponRarity,
    edit: crate::WeaponIconEdit,
    /// A subclass icon, shown without a rarity plate or watermark.
    plain: bool,
    /// The art the icon shows in place of its image: a vehicle's silhouette or a generated icon.
    art: Option<crate::icon_art::Request>,
}

type LibraryIconResult = (PathBuf, IconKey, Result<egui::ColorImage, String>);

mod icons;
pub(super) use icons::Icons as LibraryIcons;

fn library_entry_type<'a>(
    entry: &'a RecipeLibraryEntry,
    donor: Option<&'a WeaponDonorSummary>,
) -> &'a str {
    entry
        .type_name
        .as_deref()
        .or(entry.class_type_name)
        .or_else(|| donor.map(|donor| donor.type_name.as_str()))
        .unwrap_or(entry.kind.label())
}

fn library_entry_details(entry: &RecipeLibraryEntry, donor: Option<&WeaponDonorSummary>) -> String {
    let type_name = library_entry_type(entry, donor);
    let rarity = entry
        .rarity
        .map(|value| format!("{value:?}"))
        .or_else(|| donor.map(|donor| donor.rarity.label().to_owned()));
    let damage = entry
        .damage_type
        .map(|value| format!("{value:?}"))
        .or_else(|| {
            donor
                .and_then(|donor| donor.damage_type)
                .map(|value| value.label().to_owned())
        });
    let ammo = entry
        .ammo_type
        .map(|value| format!("{value:?}"))
        .or_else(|| {
            donor
                .and_then(|donor| donor.ammo_type)
                .map(|value| value.label().to_owned())
        });
    std::iter::once(type_name.to_owned())
        .chain(rarity)
        .chain(damage)
        .chain(ammo)
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Search terms may span fields: "arc special" matches "Arc · Special".
fn library_entry_matches(entry: &RecipeLibraryEntry, details: &str, query: &str) -> bool {
    let searchable = format!("{} {} {details}", entry.name, entry.namespace).to_lowercase();
    query
        .split_whitespace()
        .all(|term| searchable.contains(term))
}

/// "1 recipe", "2 recipes".
fn recipe_count(count: usize) -> String {
    format!("{count} {}", if count == 1 { "recipe" } else { "recipes" })
}

/// Height a vertical `egui::Separator` allocates for itself.
const SEPARATOR_HEIGHT: f32 = 6.0;

/// Height a footer pinned under a windowed list needs, so the list can claim everything else:
/// a gap, the separator's own row, another gap, and the row of controls.
fn pinned_footer_height(ui: &egui::Ui) -> f32 {
    ui.spacing().item_spacing.y * 2.0 + SEPARATOR_HEIGHT + super::style::list_row_height(ui)
}

/// The build selection adds an error line above its buttons when a commit fails.
fn build_selection_footer_height(ui: &egui::Ui, error: &Option<String>) -> f32 {
    pinned_footer_height(ui)
        + if error.is_some() {
            ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y
        } else {
            0.0
        }
}

/// Height a windowed list should claim so its footer lands just under the last row. A window
/// sizes itself as though it sat at the top of the screen, so its own available height can
/// reach past the bottom edge: take whichever limit is nearer, leaving room for the bottom
/// margin and border the window frame draws below the contents.
fn windowed_list_height(ui: &egui::Ui, footer_height: f32) -> f32 {
    let style = ui.ctx().global_style();
    let frame_bottom =
        f32::from(style.spacing.window_margin.bottom) + style.visuals.window_stroke.width;
    let room_on_screen = ui.ctx().content_rect().bottom() - ui.cursor().top() - frame_bottom;
    (ui.available_height() - footer_height)
        .min(room_on_screen - footer_height)
        .max(120.0)
}

/// One checkbox covering every row the search shows, the same bulk control Sundial uses for
/// Collections and Triumphs: checked when they are all selected, mixed when only some are.
fn draw_select_all_shown<'a>(
    ui: &mut egui::Ui,
    shown: impl Iterator<Item = &'a PathBuf> + Clone,
    selected: &mut BTreeSet<PathBuf>,
) {
    let total = shown.clone().count();
    let count = shown
        .clone()
        .filter(|path| selected.contains(*path))
        .count();
    let mut all = count == total && count > 0;
    let response = ui
        .add_enabled(
            total > 0,
            egui::Checkbox::new(&mut all, "Select All Shown")
                .indeterminate(count > 0 && count < total),
        )
        .on_hover_text("Selects or clears every recipe this search shows.");
    if response.changed() {
        for path in shown {
            if all {
                selected.insert(path.clone());
            } else {
                selected.remove(path);
            }
        }
    }
}

/// The search and sort row shared by the library and the build selection.
///
/// Both browse the same recipes, so they offer the same controls and the same wording. The sort
/// order is one setting: changing it in either place changes what the other shows.
fn draw_recipe_search(
    ui: &mut egui::Ui,
    id: &str,
    query: &mut String,
    sort: &mut SortOrder,
    kind: &mut Option<ItemKind>,
    activities_only: &mut bool,
    focus: &mut bool,
) {
    ui.horizontal(|ui| {
        let search = named_control(
            ui.add(
                egui::TextEdit::singleline(query)
                    .hint_text("Search Recipes…")
                    .desired_width((ui.available_width() - 315.0).max(100.0)),
            ),
            "Search recipes",
        )
        .on_hover_text("Search by name, item type, element or ammo.");
        if std::mem::take(focus) {
            search.request_focus();
        }
        egui::ComboBox::from_id_salt((id, "kind"))
            .width(132.0)
            .selected_text(if *activities_only {
                "Activities"
            } else {
                kind.map_or("All Items", ItemKind::plural)
            })
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(kind.is_none() && !*activities_only, "All Items")
                    .clicked()
                {
                    *kind = None;
                    *activities_only = false;
                }
                #[cfg(feature = "d2-model-importer")]
                if ui
                    .selectable_label(*activities_only, "Activities")
                    .clicked()
                {
                    *kind = None;
                    *activities_only = true;
                }
                for option in ItemKind::ALL {
                    if ui
                        .selectable_label(
                            *kind == Some(option) && !*activities_only,
                            option.plural(),
                        )
                        .clicked()
                    {
                        *kind = Some(option);
                        *activities_only = false;
                    }
                }
            });
        egui::ComboBox::from_id_salt(id)
            .width(155.0)
            .selected_text(format!(
                "Sort: {}",
                match *sort {
                    SortOrder::RecentlyModified => "Recent",
                    order => order.label(),
                }
            ))
            .show_ui(ui, |ui| {
                for order in SortOrder::ALL {
                    ui.selectable_value(sort, order, order.label());
                }
            });
    });
}

/// Both library navigation and build selection use the authored weapon preview.
fn draw_library_row(
    ui: &mut egui::Ui,
    catalog: Option<&InvestmentCatalog>,
    icons: &mut LibraryIcons,
    entry: &RecipeLibraryEntry,
    details: &str,
    state: LibraryRowState,
) -> LibraryRowResponse {
    ui.push_id(&entry.path, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let inclusion = state.inclusion;
        let current = inclusion.unwrap_or(state.current);
        let mut checkbox_changed = false;
        let mut checkbox_focus = false;
        let body_size = egui::TextStyle::Body.resolve(ui.style()).size;
        let name_size = (body_size + 2.0).max(16.0);
        let detail_size = body_size.max(14.0);
        let row_height = (name_size + detail_size + 14.0).max(52.0);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), row_height),
            egui::Sense::hover(),
        );
        if ui.is_rect_visible(rect) {
            icons.want(ui.ctx(), &entry.path);
        }
        let response = ui.interact(
            rect,
            ui.make_persistent_id("weapon-row"),
            if inclusion.is_some() {
                egui::Sense::CLICK
            } else {
                egui::Sense::click()
            },
        );
        let visuals = ui.style().interact_selectable(&response, current);
        let fill = if current {
            ui.visuals().selection.bg_fill
        } else if state.highlighted {
            ui.visuals().selection.bg_fill.gamma_multiply(0.35)
        } else if response.hovered() || response.has_focus() {
            visuals.weak_bg_fill
        } else {
            ui.visuals().faint_bg_color
        };
        ui.painter().rect_filled(rect, visuals.corner_radius, fill);
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 6.0, rect.center().y - 20.0),
            egui::vec2(40.0, 40.0),
        );
        match icons.previews.get(&entry.path) {
            Some(AuthoredIconPreview::Ready { texture, key })
                if key.edit == entry.icon_edit
                    && key.item_hash == entry.icon_hash
                    && entry.rarity.is_none_or(|rarity| {
                        crate::AuthoredWeaponRarity::from(rarity) == key.rarity
                    }) =>
            {
                egui::Image::new(texture).paint_at(ui, icon_rect);
            }
            Some(AuthoredIconPreview::Failed { error, .. }) => {
                ui.put(icon_rect, egui::Label::new("No icon").wrap())
                    .on_hover_text(error);
            }
            _ if icons.worker.is_some() => {
                ui.put(icon_rect, egui::Spinner::new());
            }
            _ => {
                ui.put(icon_rect, egui::Label::new("No icon").wrap());
            }
        }
        let text_rect = egui::Rect::from_min_max(
            rect.min + egui::vec2(54.0, 3.0),
            rect.max - egui::vec2(8.0, 3.0),
        );
        let mut menu_action = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(text_rect), |ui| {
            ui.horizontal(|ui| {
                let trailing_width = 34.0
                    + if entry.bundled { 54.0 } else { 0.0 }
                    + if inclusion.is_none() && (current || state.highlighted) {
                        40.0
                    } else {
                        0.0
                    };
                ui.allocate_ui_with_layout(
                    egui::vec2(
                        (ui.available_width() - trailing_width).max(40.0),
                        ui.spacing().interact_size.y,
                    ),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&entry.name).size(name_size).strong(),
                            )
                            .truncate()
                            .selectable(false),
                        )
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if inclusion.is_none() {
                        let menu = ui
                            .scope(|ui| {
                                crate::app::style::quiet(ui);
                                let icon = crate::app::style::more_icon(ui);
                                ui.menu_button(icon, |ui| {
                                    menu_action = actions::entry_menu(ui, entry, state.can_restore);
                                })
                            })
                            .inner;
                        named_control(menu.response, format!("Recipe Actions for {}", entry.name))
                            .on_hover_text("Recipe actions");
                    }
                    if let Some(mut included) = inclusion {
                        let checkbox = ui.checkbox(&mut included, "");
                        checkbox.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Checkbox,
                                ui.is_enabled(),
                                included,
                                format!("Select {} · {details}", entry.name),
                            )
                        });
                        checkbox_changed = checkbox.changed();
                        checkbox_focus = checkbox.has_focus();
                        if checkbox.gained_focus() {
                            checkbox.scroll_to_me(None);
                        }
                    } else if current {
                        ui.add(egui::Label::new("Open").selectable(false));
                    } else if state.highlighted {
                        ui.add(
                            egui::Label::new(egui::RichText::new("New").small()).selectable(false),
                        );
                    }
                    if entry.bundled {
                        ui.add(
                            egui::Label::new(egui::RichText::new("Default").weak())
                                .selectable(false),
                        );
                    }
                    if state.installed {
                        ui.add(
                            egui::Label::new(egui::RichText::new("Installed").weak())
                                .selectable(false),
                        )
                        .on_hover_text("Installed. A build without this recipe removes it.");
                    }
                });
            });
            ui.add(
                egui::Label::new(
                    egui::RichText::new(details)
                        .size(detail_size)
                        .color(ui.visuals().text_color().gamma_multiply(0.8)),
                )
                .truncate()
                .selectable(false),
            );
        });

        if response.has_focus() || checkbox_focus {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                2.0,
                egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                egui::StrokeKind::Inside,
            );
        }
        if response.gained_focus() {
            response.scroll_to_me(None);
        }
        if state.reveal {
            response.scroll_to_me(Some(egui::Align::Center));
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                current,
                format!("{} · {details}", entry.name),
            )
        });
        let action = if inclusion.is_some() {
            "Toggle selection"
        } else {
            "Open recipe to edit"
        };
        if inclusion.is_none() {
            response.context_menu(|ui| {
                menu_action = actions::entry_menu(ui, entry, state.can_restore);
            });
        }
        let clicked = library_tooltip(response, catalog, icons, entry, details, action).clicked();
        LibraryRowResponse {
            activated: clicked || checkbox_changed,
            action: menu_action,
        }
    })
    .inner
}

/// A recipe row's tooltip: the game's item tooltip on its base item's band once the catalog has
/// loaded, with what the row is and what clicking it does as its note.
fn library_tooltip(
    response: egui::Response,
    catalog: Option<&InvestmentCatalog>,
    icons: &LibraryIcons,
    entry: &RecipeLibraryEntry,
    details: &str,
    action: &str,
) -> egui::Response {
    let Some(catalog) = catalog.filter(|_| entry.donor_hash != 0) else {
        return response.on_hover_ui(|ui| {
            sundial::investment::tooltip_title(ui, &entry.namespace);
            ui.label(details);
            ui.label(action);
        });
    };
    let note = format!("{}\n{details}\n{action}", entry.namespace);
    // The recipe's own icon, never the base item's while it loads.
    let icon = match icons.previews.get(&entry.path) {
        Some(AuthoredIconPreview::Ready { texture, .. }) => {
            sundial::investment::IconOverride::Texture(texture.id())
        }
        _ => sundial::investment::IconOverride::Pending,
    };
    catalog.item_tooltip(
        response,
        entry.donor_hash,
        sundial::investment::ItemTooltip {
            name: Some(&entry.name),
            type_name: entry.type_name.as_deref().or(entry.class_type_name),
            // The base item's text and stats are not the recipe's.
            description: Some(""),
            rarity: entry.rarity.map(|rarity| rarity as u8 + 1),
            icon: Some(icon),
            note: Some(&note),
            template: true,
        },
    )
}

/// A map's row details: what it is, and the game its level comes from.
fn map_details(map: &crate::recipe_library::MapLibraryEntry) -> &'static str {
    if map.source {
        "Activity · Halo: Reach"
    } else {
        "Activity"
    }
}

/// The maps a search shows: every map under All Items, matched like recipes by name and details.
fn matching_maps<'a>(
    maps: &'a [crate::recipe_library::MapLibraryEntry],
    kind: Option<ItemKind>,
    query: &str,
) -> Vec<&'a crate::recipe_library::MapLibraryEntry> {
    if kind.is_some() {
        return Vec::new();
    }
    maps.iter()
        .filter(|map| {
            let searchable =
                format!("{} {} {}", map.title, map.name, map_details(map)).to_lowercase();
            query
                .split_whitespace()
                .all(|term| searchable.contains(term))
        })
        .collect()
}

/// A map's row, drawn as a recipe's is, with a map glyph for its icon. With `inclusion` it
/// carries a build selection checkbox, and without one it opens the activity editor.
/// Returns whether the row was activated.
fn draw_map_row(
    ui: &mut egui::Ui,
    map: &crate::recipe_library::MapLibraryEntry,
    inclusion: Option<bool>,
) -> bool {
    ui.push_id(&map.path, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let current = inclusion.unwrap_or(false);
        let details = map_details(map);
        let body_size = egui::TextStyle::Body.resolve(ui.style()).size;
        let name_size = (body_size + 2.0).max(16.0);
        let detail_size = body_size.max(14.0);
        let row_height = (name_size + detail_size + 14.0).max(52.0);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), row_height),
            egui::Sense::hover(),
        );
        let response = ui.interact(rect, ui.make_persistent_id("map-row"), egui::Sense::CLICK);
        let visuals = ui.style().interact_selectable(&response, current);
        let fill = if current {
            ui.visuals().selection.bg_fill
        } else if response.hovered() || response.has_focus() {
            visuals.weak_bg_fill
        } else {
            ui.visuals().faint_bg_color
        };
        ui.painter().rect_filled(rect, visuals.corner_radius, fill);
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 6.0, rect.center().y - 20.0),
            egui::vec2(40.0, 40.0),
        );
        ui.painter()
            .rect_filled(icon_rect, 2.0, ui.visuals().extreme_bg_color);
        ui.put(
            icon_rect,
            egui::Label::new(
                super::style::light_icon(ui, egui_phosphor::regular::MAP_TRIFOLD).size(24.0),
            )
            .selectable(false),
        );
        let text_rect = egui::Rect::from_min_max(
            rect.min + egui::vec2(54.0, 3.0),
            rect.max - egui::vec2(8.0, 3.0),
        );
        let mut checkbox_changed = false;
        let mut checkbox_focus = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(text_rect), |ui| {
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(
                        (ui.available_width() - 34.0).max(40.0),
                        ui.spacing().interact_size.y,
                    ),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&map.title).size(name_size).strong(),
                            )
                            .truncate()
                            .selectable(false),
                        )
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(mut included) = inclusion {
                        let checkbox = ui.checkbox(&mut included, "");
                        checkbox.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Checkbox,
                                ui.is_enabled(),
                                included,
                                format!("Select {} · {details}", map.title),
                            )
                        });
                        checkbox_changed = checkbox.changed();
                        checkbox_focus = checkbox.has_focus();
                        if checkbox.gained_focus() {
                            checkbox.scroll_to_me(None);
                        }
                    }
                });
            });
            ui.add(
                egui::Label::new(
                    egui::RichText::new(details)
                        .size(detail_size)
                        .color(ui.visuals().text_color().gamma_multiply(0.8)),
                )
                .truncate()
                .selectable(false),
            );
        });
        if response.has_focus() || checkbox_focus {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                2.0,
                egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                egui::StrokeKind::Inside,
            );
        }
        if response.gained_focus() {
            response.scroll_to_me(None);
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                current,
                format!("{} · {details}", map.title),
            )
        });
        let clicked = response
            .on_hover_ui(|ui| {
                ui.strong(&map.title);
                ui.label(details);
                ui.label(if inclusion.is_some() {
                    "Toggle selection"
                } else {
                    "Open activity recipe"
                });
            })
            .clicked();
        clicked || checkbox_changed
    })
    .inner
}

fn draw_bundled_recipe_selection(
    ui: &mut egui::Ui,
    entries: &[RecipeLibraryEntry],
    selected: &mut BTreeSet<PathBuf>,
) {
    let bundled: Vec<_> = entries.iter().filter(|entry| entry.bundled).collect();
    let count = bundled
        .iter()
        .filter(|entry| selected.contains(&entry.path))
        .count();
    let mut all = !bundled.is_empty() && count == bundled.len();
    if ui
        .add_enabled(
            !bundled.is_empty(),
            egui::Checkbox::new(
                &mut all,
                format!("Include Default Weapons ({}/{})", count, bundled.len()),
            )
            .indeterminate(count > 0 && count < bundled.len()),
        )
        .on_hover_text("Includes recipes hidden by the search.")
        .changed()
    {
        for entry in bundled {
            if all {
                selected.insert(entry.path.clone());
            } else {
                selected.remove(&entry.path);
            }
        }
    }
}

impl PackageAuthoringApp {
    pub(super) fn current_recipe_is_in_build(&self) -> bool {
        self.recipe_path
            .as_ref()
            .is_some_and(|path| self.enabled_recipe_paths.contains(path))
    }

    pub(super) fn open_build_selection(&mut self) {
        if self.build_selection_draft.is_none() {
            self.build_selection_draft = Some(self.enabled_recipe_paths.clone());
            self.build_selection_error = None;
            self.build_selection_query.clear();
            self.recipe_search_focus_pending = true;
        }
        self.library_open = false;
    }

    fn draw_library_controls(
        &mut self,
        ui: &mut egui::Ui,
        busy: bool,
        action: &mut Option<LibraryAction>,
    ) -> bool {
        let busy = busy || self.library_state.export_selection.is_some();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !busy && self.recipe_library.is_some(),
                    egui::Button::new("Import…"),
                )
                .clicked()
            {
                *action = Some(LibraryAction::Import);
            }
            ui.add_enabled_ui(!busy && self.recipe_library.is_some(), |ui| {
                ui.menu_button("Export", |ui| {
                    if ui
                        .add_enabled(
                            self.invalid_weapon_name.is_none(),
                            egui::Button::new("Current Recipe…"),
                        )
                        .clicked()
                    {
                        *action = Some(LibraryAction::ExportOpen);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !self.recipe_entries.is_empty(),
                            egui::Button::new("Choose Recipes…"),
                        )
                        .clicked()
                    {
                        *action = Some(LibraryAction::ChooseExport);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !self.recipe_entries.is_empty(),
                            egui::Button::new("All Recipes…"),
                        )
                        .clicked()
                    {
                        *action = Some(LibraryAction::ExportAll);
                        ui.close();
                    }
                });
            });
            ui.separator();
            if ui
                .add_enabled(!busy, egui::Button::new("Refresh").small())
                .clicked()
            {
                self.refresh_recipe_library();
            }
            if ui
                .add_enabled(
                    !busy && !self.recipe_dirty && self.recipe_library.is_some(),
                    egui::Button::new("Restore Default Recipes…").small(),
                )
                .on_disabled_hover_text(if self.recipe_dirty {
                    "Save or discard open edits first."
                } else if self.recipe_library.is_none() {
                    "Recipe library unavailable."
                } else {
                    "Recipe library busy."
                })
                .clicked()
            {
                match self
                    .recipe_library
                    .as_ref()
                    .unwrap()
                    .prepare_restore_defaults()
                {
                    Ok(preview) => self.restore_defaults_preview = Some(preview),
                    Err(error) => self.log.push(LogEntry::error(error)),
                }
            }
        });
        if self.restore_defaults_preview.is_none() {
            return false;
        }
        ui.group(|ui| {
            ui.label("Restore Default Recipes?");
            ui.label("Replaces your edits to default recipes and restores missing ones. Changed files are backed up first. Custom recipes and installed files are unchanged.");
            ui.horizontal(|ui| {
                if ui.add_enabled(!busy && !self.recipe_dirty, egui::Button::new("Restore Defaults")).clicked() {
                    let preview = self.restore_defaults_preview.take().unwrap();
                    let library = self.recipe_library.as_ref().unwrap();
                    match library.restore_defaults(&preview) {
                        Ok(backup) => {
                            self.log.push(LogEntry::info(match backup {
                                Some(path) => format!("Default recipes restored. Backup: {}", path.display()),
                                None => "Default recipes are already up to date.".into(),
                            }));
                            let reload = self.recipe_path.clone().filter(|path|
                                self.recipe_entries.iter().any(|entry| entry.bundled && &entry.path == path));
                            self.refresh_recipe_library();
                            if let Some(path) = reload { self.open_recipe_path(&path); }
                        }
                        Err(error) => self.log.push(LogEntry::error(error)),
                    }
                }
                if ui.button("Cancel").clicked() { self.restore_defaults_preview = None; }
            });
        });
        true
    }

    pub(super) fn draw_library_windows(&mut self, ctx: &egui::Context) {
        self.poll_library_transfer();
        if self.library_state.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        // Escape abandons an uncommitted selection before closing library navigation.
        if (self.build_selection_draft.is_some() || self.library_open)
            && self.library_state.restore.is_none()
            && self.library_state.delete.is_none()
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            if self.library_state.export_selection.is_some() && !self.library_state.busy() {
                self.library_state.export_selection = None;
            } else if self.build_selection_draft.is_some() {
                self.build_selection_draft = None;
            } else {
                self.library_open = false;
                self.restore_defaults_preview = None;
            }
        }
        let busy = self.has_background_work();
        if (self.library_open || self.build_selection_draft.is_some())
            && let Some(catalog) = &self.catalog
        {
            self.library_icons.update(
                ctx,
                &self.packages,
                catalog,
                (&self.library_donors, &self.subclasses),
                &self.recipe_entries,
            );
        }
        self.draw_library_browser(ctx, busy);
        self.draw_recipe_restore(ctx, busy);
        self.draw_recipe_delete(ctx, busy);

        let Some(mut draft) = self.build_selection_draft.take() else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Items in This Build")
            .open(&mut open)
            .default_width(620.0)
            .default_height(720.0)
            .resizable(true)
            .show(ctx, |ui| {
                workbench_style(ui);
                draw_bundled_recipe_selection(ui, &self.recipe_entries, &mut draft);
                ui.weak("Shaders used in selected items' sockets are included automatically.");
                draw_recipe_search(
                    ui,
                    "build-selection-sort",
                    &mut self.build_selection_query,
                    &mut self.library_state.sort,
                    &mut self.library_state.kind,
                    &mut self.library_state.activities_only,
                    &mut self.recipe_search_focus_pending,
                );
                let query = self.build_selection_query.trim().to_lowercase();
                let mut shown = self.library_state.matching_entries(
                    &self.recipe_entries,
                    &self.library_donors,
                    &query,
                );
                self.library_state
                    .sort_entries(&mut shown, &self.library_donors);
                let shown_maps = matching_maps(&self.map_entries, self.library_state.kind, &query);
                ui.separator();
                draw_select_all_shown(
                    ui,
                    shown
                        .iter()
                        .map(|(entry, _)| &entry.path)
                        .chain(shown_maps.iter().map(|map| &map.path)),
                    &mut draft,
                );
                let footer_height = build_selection_footer_height(ui, &self.build_selection_error);
                egui::ScrollArea::vertical()
                    .id_salt("build-selection-results")
                    .max_height(windowed_list_height(ui, footer_height))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (entry, details) in &shown {
                            let included = draft.contains(&entry.path);
                            if draw_library_row(
                                ui,
                                self.catalog.as_ref(),
                                &mut self.library_icons,
                                entry,
                                details,
                                LibraryRowState {
                                    inclusion: Some(included),
                                    installed: self.installed.contains(entry.identity_hash),
                                    ..Default::default()
                                },
                            )
                            .activated
                            {
                                if included {
                                    draft.remove(&entry.path);
                                } else {
                                    draft.insert(entry.path.clone());
                                }
                            }
                        }
                        for map in &shown_maps {
                            let included = draft.contains(&map.path);
                            if draw_map_row(ui, map, Some(included)) {
                                if included {
                                    draft.remove(&map.path);
                                } else {
                                    draft.insert(map.path.clone());
                                }
                            }
                        }
                        if shown.is_empty() && shown_maps.is_empty() {
                            ui.label("No Matching Results");
                        }
                    });
                ui.separator();
                if let Some(error) = &self.build_selection_error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.horizontal(|ui| {
                    apply = ui
                        .add_enabled(!busy, egui::Button::new("Apply Selection"))
                        .clicked();
                    cancel = ui.button("Cancel").clicked();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.weak(format!(
                            "{} selected · {} shown",
                            draft.len(),
                            shown.len() + shown_maps.len()
                        ));
                    });
                });
            });
        if apply {
            if let Err(error) = self.apply_build_selection(draft.clone()) {
                self.build_selection_error = Some(error.clone());
                self.log.push(LogEntry::error(error));
                self.build_selection_draft = Some(draft);
            } else {
                self.build_selection_error = None;
            }
        } else if open && !cancel {
            self.build_selection_draft = Some(draft);
        }
    }

    pub(super) fn apply_build_selection(
        &mut self,
        selected: BTreeSet<PathBuf>,
    ) -> Result<(), String> {
        let library = self
            .recipe_library
            .as_ref()
            .ok_or("Recipe library is unavailable")?;
        // Commit storage first. A failed write must not change the active build in memory.
        library.save_enabled_selection(&selected, &self.recipe_entries, &self.map_entries)?;
        if self.enabled_recipe_paths != selected {
            self.enabled_recipe_paths = selected;
            self.invalidate_results();
        }
        Ok(())
    }
}
