//! The detail panel at the right of the page: the selected ability or node, headed by what it is
//! and the stock one it is based on, its sections in tabs, or the selected attunement with its
//! name and every stock attunement that fits its place.
use super::*;

/// The Based On choices' widest and tallest.
const CHOICES_WIDTH: f32 = 720.0;
const CHOICES_HEIGHT: f32 = 420.0;

/// A section of an ability or node, shown one at a time as a tab.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Section {
    /// Its name, description and icon.
    #[default]
    Ability,
    /// The perks it grants while equipped.
    Perks,
    /// How it behaves: its charges, recharge and parameters, what it changes about the
    /// subclass's abilities, what it spawns, then its raw values.
    Gameplay,
    Visuals,
}

impl Section {
    const ALL: [Self; 4] = [Self::Ability, Self::Perks, Self::Gameplay, Self::Visuals];

    const fn label(self) -> &'static str {
        match self {
            Self::Ability => "Ability",
            Self::Perks => "Perks",
            Self::Gameplay => "Gameplay",
            Self::Visuals => "Visuals",
        }
    }

    /// What the tab holds, in its tooltip.
    const fn hint(self) -> &'static str {
        match self {
            Self::Ability => "Its name, description and icon",
            Self::Perks => "The perks it grants while equipped",
            Self::Gameplay => "How it behaves and what it spawns",
            Self::Visuals => "How its effects look",
        }
    }

    /// Whether the section has edits of its own.
    fn edited(self, edits: &EntryEdits, based_elsewhere: bool) -> bool {
        match self {
            Self::Ability => {
                based_elsewhere
                    || edits.name.is_some()
                    || edits.description.is_some()
                    || edits.icon.is_some()
            }
            Self::Perks => {
                !edits.added_perks.is_empty()
                    || !edits.removed_perks.is_empty()
                    || !edits.custom_perks.is_empty()
            }
            Self::Gameplay => {
                edits.extra_charges > 0
                    || edits.recharge().is_some()
                    || !edits.parameters.is_empty()
                    || !edits.modifiers.is_empty()
                    || !edits.removed_modifiers.is_empty()
                    || !edits.ability_values.is_empty()
                    || !edits.spawn_swaps.is_empty()
                    || !edits.bank_values.is_empty()
            }
            Self::Visuals => edits.recolors(),
        }
    }
}

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

/// A tab that carries the page's edit dot once what it shows has edits. A screen reader hears
/// the dot as "Changed".
pub(super) fn marked_tab(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    edited: bool,
) -> egui::Response {
    if edited {
        let tab = ui.selectable_label(selected, format!("{label} •"));
        style::named_control(tab, format!("{label}, Changed")).on_hover_text("Changed")
    } else {
        ui.selectable_label(selected, label)
    }
}

/// The detail panel's heading: what it shows, small, then its name, then what `body` adds under
/// it, with an overflow menu at the right holding Restore when there is anything to restore. An
/// ability or node leads with its icon. Returns whether Restore was clicked, and what `body`
/// returned.
fn detail_header<R>(
    ui: &mut egui::Ui,
    (kind, name, icon): (&str, &str, Option<Option<&egui::TextureHandle>>),
    restore: Option<&str>,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> (bool, R) {
    let mut clicked = false;
    let inner = ui
        .horizontal_top(|ui| {
            if let Some(icon) = icon {
                let (rect, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(HEADING_ICON), egui::Sense::hover());
                if let Some(icon) = icon {
                    paint_icon(ui, icon, rect);
                }
            }
            let menu_width = if restore.is_some() { 32.0 } else { 0.0 };
            let width = (ui.available_width() - menu_width).max(120.0);
            let inner = ui
                .allocate_ui_with_layout(
                    egui::vec2(width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(width);
                        ui.label(quiet(ui, kind));
                        ui.label(egui::RichText::new(name).size(18.0).strong());
                        body(ui)
                    },
                )
                .inner;
            if let Some(restore) = restore {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    style::more_menu(ui, name, |ui| {
                        if ui.button(restore).clicked() {
                            clicked = true;
                            ui.close_menu();
                        }
                    });
                });
            }
            inner
        })
        .inner;
    ui.add_space(6.0);
    (clicked, inner)
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

/// The Charges tile: the extra charges as a field across the tile, as every other tile's.
fn charges_tile(ui: &mut egui::Ui, width: f32, edits: &mut EntryEdits) {
    let count = edits.extra_charges;
    let (charged, reset) = style::tile(
        ui,
        width,
        "subclass-charges",
        "Charges",
        "Extra charges",
        count > 0,
        |ui| {
            ui.spacing_mut().interact_size.x = width;
            let mut value = count;
            let field = ui.add(
                egui::DragValue::new(&mut value)
                    .range(0..=MOST_CHARGES)
                    .speed(0.05)
                    .prefix("+"),
            );
            let field = style::named_control(field, "Extra Charges");
            (field.changed() && value != count).then_some(value)
        },
    );
    if let Some(count) = charged.or(reset.then_some(0)) {
        edits.extra_charges = count;
    }
}

/// The Recharge tile: the recharge multiplier across the tile.
fn recharge_tile(ui: &mut egui::Ui, width: f32, edits: &mut EntryEdits) {
    let current = edits.recharge();
    let (recharged, reset) = style::tile(
        ui,
        width,
        "subclass-recharge",
        "Recharge",
        "Higher recharges faster",
        current.is_some(),
        |ui| {
            // The field fills its tile, as every other tile's does.
            ui.spacing_mut().interact_size.x = width;
            let mut multiplier = current.unwrap_or(1.0);
            let response = super::modifiers::recharge_field(ui, &mut multiplier);
            let response = style::named_control(response, "Recharge");
            (response.changed() && multiplier.is_finite()).then_some(multiplier)
        },
    );
    if let Some(multiplier) = recharged.map(Some).or(reset.then_some(None)) {
        edits.set_recharge(multiplier);
    }
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

    /// An ability or node: its header, which names the stock one it is based on since every
    /// value starts from it, then its sections as tabs and the open one.
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
        let open = page.choosing == Some(page.selection);
        let (restored, based_on) = detail_header(
            ui,
            (&place.label(), name, Some(icon.as_ref())),
            restore.as_deref(),
            |ui| self.based_on_button(ui, (summary, entry), open),
        );
        if restored {
            return Some(match place {
                Place::Ability(entry) => AbilityEdit::ResetAbility(entry),
                Place::Node(path, position) => AbilityEdit::ResetNode(path, position),
            });
        }
        if based_on.clicked() {
            page.choosing = (!open).then_some(page.selection);
        }
        let picked = self.draw_based_on_choices(ui, (base, abilities), place, &based_on, page);
        let entity = summary.and_then(|summary| summary.entry_entities.get(&entry).copied());
        // A section with nothing to show for this entry has no tab. Gameplay always holds the
        // changes an entry makes to the subclass's abilities.
        let sections = Section::ALL
            .into_iter()
            .filter(|section| *section != Section::Visuals || entity.is_some())
            .collect::<Vec<_>>();
        if !sections.contains(&page.section) {
            page.section = Section::Ability;
        }
        let based_elsewhere = (source, entry) != own;
        ui.horizontal(|ui| {
            for section in &sections {
                let edited = section.edited(&edits, based_elsewhere);
                let tab = marked_tab(ui, page.section == *section, section.label(), edited);
                if tab.on_hover_text(section.hint()).clicked() {
                    page.section = *section;
                }
            }
        });
        ui.separator();
        ui.add_space(4.0);
        let edit = match page.section {
            Section::Ability => self
                .draw_ability_section(ui, base, (summary, entry), &edits, page)
                .map(|edits| AbilityEdit::Edits(place, Box::new(edits))),
            Section::Perks => self.draw_perks_section(ui, base, (summary, entry, place), &edits),
            Section::Gameplay => self
                .draw_gameplay_section(ui, (base, abilities), (summary, entry, place), &edits, page)
                .map(|edits| AbilityEdit::Edits(place, Box::new(edits))),
            Section::Visuals => self
                .draw_effect_colors(ui, (summary, entry), &edits, page)
                .map(|edits| AbilityEdit::Edits(place, Box::new(edits))),
        };
        picked.or(edit)
    }

    /// The header's Based On line: the stock ability the entry starts from, as a button that
    /// opens every one it can be based on instead.
    fn based_on_button(
        &self,
        ui: &mut egui::Ui,
        (summary, entry): (Option<&SubclassSummary>, u8),
        open: bool,
    ) -> egui::Response {
        let stock_name = summary.map_or("Unknown Ability", |summary| entry_name(summary, entry));
        let source_label = summary.map_or_else(
            || stock_name.to_owned(),
            |summary| format!("{stock_name} · {}", summary.name),
        );
        let source_icon = self.entry_icon(ui.ctx(), summary, entry);
        ui.horizontal(|ui| {
            ui.label(quiet(ui, "Based On"));
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
            response
        })
        .inner
    }

    /// Every stock ability or node the entry can be based on, over the page below the Based On
    /// button while it is open. A click outside it or Escape closes it. Returns the change once
    /// one is picked.
    fn draw_based_on_choices(
        &self,
        ui: &mut egui::Ui,
        (base, abilities): (&SubclassSummary, &SubclassAbilities),
        place: Place,
        button: &egui::Response,
        page: &mut PageState,
    ) -> Option<AbilityEdit> {
        if page.choosing != Some(page.selection) {
            return None;
        }
        let width = ui.available_width().min(CHOICES_WIDTH);
        let mut picked = None;
        let area = egui::Area::new(ui.id().with("subclass-based-on-choices"))
            .order(egui::Order::Foreground)
            .fixed_pos(button.rect.left_bottom() + egui::vec2(0.0, 4.0))
            .constrain(true)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(width);
                    let search = choices_search(ui, &mut page.search, self.subclasses.len());
                    egui::ScrollArea::vertical()
                        .max_height(CHOICES_HEIGHT)
                        .show(ui, |ui| {
                            picked = match place {
                                Place::Ability(entry) => self
                                    .draw_ability_choices(ui, base, abilities, entry, search)
                                    .map(|source| AbilityEdit::AbilitySource(entry, source)),
                                Place::Node(path, position) => self
                                    .draw_node_choices(
                                        ui,
                                        base,
                                        abilities,
                                        (path, position),
                                        search,
                                    )
                                    .map(|source| AbilityEdit::NodeSource(path, position, source)),
                            };
                        });
                });
            });
        let outside = ui.input(|input| {
            input.pointer.any_pressed()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|at| !area.response.rect.contains(at) && !button.rect.contains(at))
        });
        let escape =
            ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if picked.is_some() || outside || escape {
            page.choosing = None;
        }
        picked
    }

    /// The Ability section: the entry's own name, description and icon.
    fn draw_ability_section(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        (summary, entry): (Option<&SubclassSummary>, u8),
        edits: &EntryEdits,
        page: &mut PageState,
    ) -> Option<EntryEdits> {
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
        changed
    }

    /// The Perks section: the perks the entry grants while equipped. Returns the change once one
    /// changes, or the request to open one of its custom perks.
    fn draw_perks_section(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        (summary, entry, place): (Option<&SubclassSummary>, u8, Place),
        edits: &EntryEdits,
    ) -> Option<AbilityEdit> {
        let stock_perks = summary
            .and_then(|summary| summary.entry_perks.get(&entry))
            .cloned()
            .unwrap_or_default();
        // The reset restores the stock perks. Custom perks leave one at a time, and still mark
        // the field.
        let perks_edited = !edits.added_perks.is_empty() || !edits.removed_perks.is_empty();
        let perks_marked = perks_edited || !edits.custom_perks.is_empty();
        let (perked, reset) = marked_field(ui, "Perks", (perks_marked, perks_edited), |ui| {
            self.draw_entry_perks(ui, base, (summary, entry), edits, &stock_perks)
        });
        let changed = match perked {
            Some(perks::Change::Edits(edits)) => Some(*edits),
            Some(perks::Change::Open(perk)) => return Some(AbilityEdit::OpenPerk(place, perk)),
            None => reset.then(|| EntryEdits {
                added_perks: Vec::new(),
                removed_perks: Vec::new(),
                ..edits.clone()
            }),
        };
        changed.map(|edits| AbilityEdit::Edits(place, Box::new(edits)))
    }

    /// The Gameplay section: the ability's own card, what it changes about the subclass's
    /// abilities, a card for each part it spawns, then its raw values, closed until opened.
    fn draw_gameplay_section(
        &self,
        ui: &mut egui::Ui,
        (base, abilities): (&SubclassSummary, &SubclassAbilities),
        (summary, entry, place): (Option<&SubclassSummary>, u8, Place),
        edits: &EntryEdits,
        page: &mut PageState,
    ) -> Option<EntryEdits> {
        // The ability's tree: none without an entity, and `Some(None)` while it loads.
        let tree = summary
            .and_then(|summary| summary.entry_entities.get(&entry))
            .map(|entity| {
                let keys = tuning::own_keys(summary, entry);
                self.load_properties(ui.ctx(), (*entity, keys), page)
            });
        let loaded = match &tree {
            Some(Some(Ok(loaded))) => Some(loaded),
            _ => None,
        };
        let mut changed = self.draw_ability_card(ui, (summary, entry), edits, page, loaded);
        if let Some(edited) =
            self.draw_ability_changes(ui, (base, abilities), (summary, entry, place), edits, page)
        {
            changed = Some(edited);
        }
        match &tree {
            Some(None) => {
                ui.weak("Loading…");
            }
            Some(Some(Err(error))) => {
                ui.colored_label(ui.visuals().warn_fg_color, "Properties unavailable.")
                    .on_hover_text(error);
            }
            Some(Some(Ok(loaded))) => {
                if let Some(edited) = self.draw_properties(ui, loaded, edits, page) {
                    changed = Some(edited);
                }
            }
            None => {}
        }
        if tree.is_some() {
            ui.add_space(8.0);
        }
        if let Some(edited) = self.draw_technical(ui, (summary, entry, place), edits, page) {
            changed = Some(edited);
        }
        changed
    }

    /// The ability's own card: its charges and recharge where its bank takes them, its bank's
    /// script parameters, then its own entity's values, all as tiles. Draws nothing for an entry
    /// with none of them.
    fn draw_ability_card(
        &self,
        ui: &mut egui::Ui,
        (summary, entry): (Option<&SubclassSummary>, u8),
        edits: &EntryEdits,
        page: &mut PageState,
        own: Option<&properties::Loaded>,
    ) -> Option<EntryEdits> {
        let row = summary
            .and_then(|summary| summary.entry_rows.get(&entry))
            .and_then(|row| self.catalog.as_ref()?.ability_row(*row));
        // Only an ability whose bank shows where a charge row goes takes extra charges, and only
        // one whose bank has numeric inputs takes a recharge rate of its own. Parameters with only
        // a hash sit in Technical.
        let charges = row.is_some_and(|row| row.charges);
        let recharge = row.is_some_and(|row| row.recharge);
        let named = row.map_or_else(Vec::new, |row| tuning::split_parameters(row).0);
        let own = own.filter(|own| own.has_own_values());
        if !charges && !recharge && named.is_empty() && own.is_none() {
            return None;
        }
        let mut next = edits.clone();
        style::card(ui, |ui| {
            let query = ui
                .horizontal(|ui| {
                    ui.label(egui::RichText::new("Ability").strong());
                    tuning::parameter_filter(ui, named.len(), page)
                })
                .inner;
            style::tiles(ui, |ui, width| {
                if charges {
                    charges_tile(ui, width, &mut next);
                }
                if recharge {
                    recharge_tile(ui, width, &mut next);
                }
                tuning::parameter_tiles(ui, width, (&named, &query), &mut next);
                if let Some(own) = own {
                    properties::own_tiles(ui, width, own, &mut next);
                }
            });
        });
        ui.add_space(8.0);
        (next != *edits).then_some(next)
    }

    /// A node's Ability Changes: what selecting it changes about the subclass's abilities, as a
    /// card of chips and Add Change, with a reset of them all once they differ. A node does what
    /// it does this way. An ability has none: its own values sit on its Ability card, and each
    /// other ability's on that ability's page.
    fn draw_ability_changes(
        &self,
        ui: &mut egui::Ui,
        (base, abilities): (&SubclassSummary, &SubclassAbilities),
        (summary, entry, place): (Option<&SubclassSummary>, u8, Place),
        edits: &EntryEdits,
        page: &mut PageState,
    ) -> Option<EntryEdits> {
        if crate::subclass::holds_ability(crate::subclass::place_entry(place)) {
            return None;
        }
        let edited = !edits.modifiers.is_empty() || !edits.removed_modifiers.is_empty();
        let changed = style::card(ui, |ui| {
            let reset = ui
                .horizontal(|ui| {
                    ui.label(egui::RichText::new("Ability Changes").strong())
                        .on_hover_text("What selecting it changes about the subclass's abilities");
                    edited && reset_icon(ui)
                })
                .inner;
            let changed = self.draw_entry_modifiers(
                ui,
                (base, abilities),
                (summary, entry, place),
                edits,
                page,
            );
            changed.or_else(|| {
                reset.then(|| EntryEdits {
                    modifiers: Vec::new(),
                    removed_modifiers: Vec::new(),
                    ..edits.clone()
                })
            })
        });
        ui.add_space(8.0);
        changed
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
        let from = from_label(summary, base.hash);
        let restore = own
            .is_some()
            .then(|| format!("Restore {}", attunement_name(base, path)));
        let mut edit = None;
        let (restored, ()) = detail_header(
            ui,
            (
                &format!("{} Attunement", path.label()),
                name.as_deref().unwrap_or(stock_name),
                None,
            ),
            restore.as_deref(),
            |ui| {
                if let Some(from) = &from {
                    ui.add(egui::Label::new(quiet(ui, from.as_str())).wrap());
                }
            },
        );
        if restored {
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
