//! The detail panel at the right of the page: the selected ability or node with its own values
//! and every stock one it can be based on, or the selected attunement with its name and every
//! stock attunement that fits its place.
use super::*;

/// The quiet icon that restores one value.
pub(super) fn reset_icon(ui: &mut egui::Ui) -> bool {
    let hover = "Restore the original value";
    let button = ui.add(
        egui::Button::new(style::light_icon(
            ui,
            egui_phosphor::regular::ARROW_COUNTER_CLOCKWISE,
        ))
        .frame(false)
        // A hit area larger than the glyph, which stays quiet.
        .min_size(egui::Vec2::splat(20.0)),
    );
    style::named_control(button, hover)
        .on_hover_text(hover)
        .clicked()
}

/// A tab that carries the page's edit dot once what it shows has edits.
pub(super) fn marked_tab(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    edited: bool,
) -> egui::Response {
    if edited {
        ui.selectable_label(selected, format!("{label} •"))
            .on_hover_text("Changed")
    } else {
        ui.selectable_label(selected, label)
    }
}

/// The detail panel's heading: what it shows, small, then its name, then where it comes from and
/// what it does, with a button that restores the base's own at the right. An ability or node
/// leads with its icon. Returns whether the restore button was clicked.
fn detail_header(
    ui: &mut egui::Ui,
    (kind, name, icon): (&str, &str, Option<Option<&egui::TextureHandle>>),
    lines: &[String],
    restore: Option<&str>,
) -> bool {
    let mut clicked = false;
    ui.horizontal_top(|ui| {
        if let Some(icon) = icon {
            let (rect, _) =
                ui.allocate_exact_size(egui::Vec2::splat(HEADING_ICON), egui::Sense::hover());
            if let Some(icon) = icon {
                paint_icon(ui, icon, rect);
            }
        }
        let restore_width = if restore.is_some() { 220.0 } else { 0.0 };
        let width = (ui.available_width() - restore_width).max(120.0);
        ui.allocate_ui_with_layout(
            egui::vec2(width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(width);
                ui.label(quiet(ui, kind));
                ui.label(egui::RichText::new(name).size(18.0).strong());
                for line in lines {
                    ui.add(egui::Label::new(quiet(ui, line.as_str())).wrap());
                }
            },
        );
        if let Some(restore) = restore {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                clicked = ui.button(restore).clicked();
            });
        }
    });
    ui.add_space(6.0);
    clicked
}

/// A labeled field in the detail panel, with a quiet reset beside its label once it differs.
/// Returns the control's result and whether the reset was clicked.
pub(super) fn field<R>(
    ui: &mut egui::Ui,
    label: &str,
    modified: bool,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, bool) {
    marked_field(ui, label, (modified, modified), control)
}

/// As [`field`], for a field whose reset restores only part of what it edits: the label marks
/// every edit, and the reset shows only while there is something for it to restore.
pub(super) fn marked_field<R>(
    ui: &mut egui::Ui,
    label: &str,
    (modified, resettable): (bool, bool),
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, bool) {
    ui.horizontal_top(|ui| {
        // The label and its reset share a column of their own, so every field starts at one edge
        // and the reset sits by the value it restores.
        let reset = ui
            .allocate_ui_with_layout(
                egui::vec2(LABEL_WIDTH, 22.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(LABEL_WIDTH);
                    let text = egui::RichText::new(label).size(12.0);
                    let text = if modified {
                        text
                    } else {
                        text.color(style::secondary(ui.visuals()))
                    };
                    ui.label(text);
                    resettable && reset_icon(ui)
                },
            )
            .inner;
        let width = ui.available_width().max(80.0);
        let result = ui
            .allocate_ui_with_layout(
                egui::vec2(width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(width);
                    control(ui)
                },
            )
            .inner;
        (result, reset)
    })
    .inner
}

/// Extra charges as a stepper: fewer, the count, more. Returns the count once it changes.
fn charges_stepper(ui: &mut egui::Ui, count: u8) -> Option<u8> {
    let mut next = count;
    ui.horizontal(|ui| {
        let fewer = ui.add_enabled(count > 0, egui::Button::new("−"));
        if style::named_control(fewer, "Fewer Charges").clicked() {
            next = count - 1;
        }
        let reading = if count == 0 {
            "Stock".to_owned()
        } else {
            format!("+{count}")
        };
        ui.add_sized(
            egui::vec2(44.0, ui.spacing().interact_size.y),
            egui::Label::new(reading),
        );
        let more = ui.add_enabled(count < MOST_CHARGES, egui::Button::new("+"));
        if style::named_control(more, "More Charges").clicked() {
            next = count + 1;
        }
    });
    (next != count).then_some(next)
}

/// The heading of the detail panel's choices, a line to each of `lines` subclasses, and their
/// search where the list is long enough to want one. Returns the search to apply, empty when there
/// is none. The original is always among the choices, so picking it again restores it.
fn choices_heading<'a>(
    ui: &mut egui::Ui,
    name: &str,
    search: &'a mut String,
    lines: usize,
) -> &'a str {
    ui.add_space(8.0);
    ui.separator();
    ui.label(egui::RichText::new(name).strong());
    choices_search(ui, search, lines)
}

/// The search over a list of choices with a line to each of `lines` subclasses, where the list is
/// long enough to want one. Returns the search to apply, empty when there is none.
fn choices_search<'a>(ui: &mut egui::Ui, search: &'a mut String, lines: usize) -> &'a str {
    // One line to each subclass reads at a glance, so a search shows only past the filter rule.
    if !crate::app::pickers::wants_filter(lines) {
        return "";
    }
    ui.add(
        egui::TextEdit::singleline(search)
            .hint_text(format!(
                "{} Search",
                egui_phosphor::regular::MAGNIFYING_GLASS
            ))
            .desired_width(f32::INFINITY),
    );
    search
}

/// A text field that shows `hint` while empty. Returns the text once it changes, `None` for an
/// emptied one.
fn text_field(
    ui: &mut egui::Ui,
    (label, hint): (&str, &str),
    value: Option<&str>,
    multiline: bool,
) -> Option<Option<String>> {
    let (changed, reset) = field(ui, label, value.is_some(), |ui| {
        let mut text = value.unwrap_or_default().to_owned();
        let edit = if multiline {
            egui::TextEdit::multiline(&mut text).desired_rows(2)
        } else {
            egui::TextEdit::singleline(&mut text)
        };
        ui.add(edit.hint_text(hint).desired_width(f32::INFINITY))
            .changed()
            .then_some(text)
    });
    if let Some(text) = changed {
        Some((!text.trim().is_empty()).then_some(text))
    } else {
        reset.then_some(None)
    }
}

impl PackageAuthoringApp {
    /// The selected ability, node or attunement.
    pub(super) fn draw_subclass_detail(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
        page: &mut PageState,
    ) -> Option<AbilityEdit> {
        style::card(ui, |ui| match page.selection {
            SubclassSelection::Entry(place) => {
                self.draw_entry_detail(ui, base, abilities, place, page)
            }
            SubclassSelection::Path(path) => {
                self.draw_attunement_detail(ui, base, abilities, (path, &mut page.search))
            }
        })
    }

    /// An ability or node: the stock one it is based on first, since every field starts from it,
    /// with its choices opening below that row, then its own name, description, icon and perks.
    fn draw_entry_detail(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
        place: Place,
        page: &mut PageState,
    ) -> Option<AbilityEdit> {
        let (source, entry) = source_of(abilities, base.hash, place);
        let own = own_source(abilities, base.hash, place);
        let summary = find_subclass(&self.subclasses, source);
        let stock_name = summary.map_or("Unknown Ability", |summary| entry_name(summary, entry));
        let edits = abilities.edits(base.hash, place);
        let restore_name = find_subclass(&self.subclasses, own.0)
            .map_or("Ability", |subclass| entry_name(subclass, own.1));
        let restore = is_own(abilities, place).then(|| format!("Restore {restore_name}"));
        let icon = self.place_icon(ui.ctx(), abilities, base.hash, place);
        let name = edits.name.as_deref().unwrap_or(stock_name);
        if detail_header(
            ui,
            (&place.label(), name, Some(icon.as_ref())),
            &[],
            restore.as_deref(),
        ) {
            return Some(match place {
                Place::Ability(entry) => AbilityEdit::ResetAbility(entry),
                Place::Node(path, position) => AbilityEdit::ResetNode(path, position),
            });
        }
        let source_label = summary.map_or_else(
            || stock_name.to_owned(),
            |summary| format!("{stock_name} · {}", summary.name),
        );
        let source_icon = self.entry_icon(ui.ctx(), summary, entry);
        let open = page.choosing == Some(page.selection);
        let (toggled, _) = marked_field(ui, "Based On", ((source, entry) != own, false), |ui| {
            let text = egui::RichText::new(source_label.as_str());
            let button = match &source_icon {
                Some(icon) => egui::Button::image_and_text(
                    egui::Image::new(icon).fit_to_exact_size(egui::Vec2::splat(CHOICE_ICON)),
                    text,
                ),
                None => egui::Button::new(text),
            };
            let response = ui.add(button.selected(open));
            // The name says what it does and the value which ability it is now.
            response.widget_info(|| {
                let mut info = egui::WidgetInfo::selected(
                    egui::WidgetType::Button,
                    true,
                    open,
                    "Change Based On",
                );
                info.current_text_value = Some(source_label.clone());
                info
            });
            response.clicked()
        });
        if toggled {
            page.choosing = (!open).then_some(page.selection);
        }
        let mut picked = None;
        if page.choosing == Some(page.selection) {
            let search = choices_search(ui, &mut page.search, self.subclasses.len());
            picked = match place {
                Place::Ability(entry) => self
                    .draw_ability_choices(ui, base, abilities, entry, search)
                    .map(|source| AbilityEdit::AbilitySource(entry, source)),
                Place::Node(path, position) => self
                    .draw_node_choices(ui, base, abilities, (path, position), search)
                    .map(|source| AbilityEdit::NodeSource(path, position, source)),
            };
            if picked.is_some() {
                page.choosing = None;
            }
            ui.add_space(4.0);
            ui.separator();
        }
        let edit =
            self.draw_entry_fields(ui, (base, abilities), (summary, entry), place, &edits, page);
        picked.or(edit)
    }

    /// An ability's or node's own name, description, icon and perks, what it changes about the
    /// subclass's abilities, and an ability's charges and tuning. Returns the change once one
    /// changes, or the request to open one of its custom perks.
    fn draw_entry_fields(
        &self,
        ui: &mut egui::Ui,
        (base, abilities): (&SubclassSummary, &SubclassAbilities),
        (summary, entry): (Option<&SubclassSummary>, u8),
        place: Place,
        edits: &EntryEdits,
        page: &mut PageState,
    ) -> Option<AbilityEdit> {
        let stock_name = summary.map_or("Unknown Ability", |summary| entry_name(summary, entry));
        let mut changed = None;
        if let Some(name) = text_field(ui, ("Name", stock_name), edits.name.as_deref(), false) {
            changed = Some(EntryEdits {
                name,
                ..edits.clone()
            });
        }
        let stock_description = self.entry_description(summary, entry).unwrap_or_default();
        if let Some(description) = text_field(
            ui,
            ("Description", &stock_description),
            edits.description.as_deref(),
            true,
        ) {
            changed = Some(EntryEdits {
                description,
                ..edits.clone()
            });
        }
        let (icon, reset) = field(ui, "Icon", edits.icon.is_some(), |ui| {
            self.draw_icon_choices(ui, base, (summary, entry), edits.icon.as_ref(), page)
        });
        if let Some(icon) = icon {
            changed = Some(EntryEdits {
                icon: Some(icon),
                ..edits.clone()
            });
        } else if reset {
            changed = Some(EntryEdits {
                icon: None,
                ..edits.clone()
            });
        }
        if let Some(colored) = self.draw_effect_colors(ui, (summary, entry), edits, page) {
            changed = Some(colored);
        }
        let stock_perks = summary
            .and_then(|summary| summary.entry_perks.get(&entry))
            .cloned()
            .unwrap_or_default();
        // The reset restores the stock perks. Custom perks leave one at a time, and still mark
        // the field.
        let perks_edited = !edits.added_perks.is_empty() || !edits.removed_perks.is_empty();
        let perks_marked = perks_edited || !edits.custom_perks.is_empty();
        let (perked, reset) = marked_field(ui, "Perks", (perks_marked, perks_edited), |ui| {
            self.draw_entry_perks(ui, base, edits, &stock_perks)
        });
        match perked {
            Some(perks::Change::Edits(edits)) => changed = Some(*edits),
            Some(perks::Change::Open(perk)) => return Some(AbilityEdit::OpenPerk(place, perk)),
            None if reset => {
                changed = Some(EntryEdits {
                    added_perks: Vec::new(),
                    removed_perks: Vec::new(),
                    ..edits.clone()
                });
            }
            None => {}
        }
        // Only an ability whose bank shows where a charge row goes takes extra charges.
        let row = summary
            .and_then(|summary| summary.entry_rows.get(&entry))
            .and_then(|row| self.catalog.as_ref()?.ability_row(*row));
        if row.is_some_and(|row| row.charges) {
            let (charged, reset) = field(ui, "Charges", edits.extra_charges > 0, |ui| {
                charges_stepper(ui, edits.extra_charges)
            });
            if let Some(extra_charges) = charged.or(reset.then_some(0)) {
                changed = Some(EntryEdits {
                    extra_charges,
                    ..changed.unwrap_or_else(|| edits.clone())
                });
            }
        }
        // What the ability changes about others, then its script and entity values, in view with
        // the everyday fields.
        let modifiers_edited = !edits.modifiers.is_empty() || !edits.removed_modifiers.is_empty();
        let (modified, reset) = field(ui, "Modifiers", modifiers_edited, |ui| {
            self.draw_entry_modifiers(ui, (base, abilities), (summary, entry, place), edits, page)
        });
        if let Some(modified) = modified {
            changed = Some(modified);
        } else if reset {
            changed = Some(EntryEdits {
                modifiers: Vec::new(),
                removed_modifiers: Vec::new(),
                ..changed.clone().unwrap_or_else(|| edits.clone())
            });
        }
        if let Some(tuned) = self.draw_tuning(ui, (summary, entry, place), edits, page) {
            changed = Some(tuned);
        }
        changed.map(|edits| AbilityEdit::Edits(place, Box::new(edits)))
    }

    /// An attunement: its name, then every stock attunement that fits its place.
    fn draw_attunement_detail(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
        (path, search): (AttunementPath, &mut String),
    ) -> Option<AbilityEdit> {
        let (source, source_path) = abilities.attunement_source(base.hash, path);
        let summary = find_subclass(&self.subclasses, source);
        let stock_name = summary.map_or(path.label(), |summary| {
            attunement_name(summary, source_path)
        });
        let own = abilities.attunement(path);
        let name = own.and_then(|attunement| attunement.name.clone());
        let lines = from_label(summary, base.hash)
            .into_iter()
            .collect::<Vec<_>>();
        let restore = own
            .is_some()
            .then(|| format!("Restore {}", attunement_name(base, path)));
        let mut edit = None;
        if detail_header(
            ui,
            (
                &format!("{} Attunement", path.label()),
                name.as_deref().unwrap_or(stock_name),
                None,
            ),
            &lines,
            restore.as_deref(),
        ) {
            edit = Some(AbilityEdit::ResetAttunement(path));
        }
        if let Some(name) = text_field(ui, ("Name", stock_name), name.as_deref(), false) {
            edit = Some(AbilityEdit::PathName(path, name));
        }
        let search = choices_heading(ui, "Replace With", search, self.subclasses.len());
        if let Some(pick) = self.draw_attunement_choices(ui, base, abilities, path, search) {
            edit = Some(AbilityEdit::PathSource(path, pick));
        }
        edit
    }
}
