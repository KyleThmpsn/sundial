use super::*;

#[derive(Default, PartialEq, Eq, Clone, Copy, serde::Deserialize, serde::Serialize)]
pub(super) enum Order {
    #[default]
    Catalog,
    #[serde(alias = "Status")]
    Name,
    Type,
}

impl Order {
    pub const ALL: [Order; 3] = [Order::Catalog, Order::Name, Order::Type];

    pub fn label(self) -> &'static str {
        match self {
            Order::Catalog => "Catalog Order",
            Order::Name => "Name",
            Order::Type => "Item Type",
        }
    }
}

/// Filter state kept between sessions.
#[derive(Default, Clone, serde::Deserialize, serde::Serialize)]
pub(super) struct View {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub kind: String,
    #[serde(flatten)]
    pub filters: filters::Filters,
    #[serde(default)]
    pub order: Order,
    #[serde(default)]
    pub show_installed: bool,
    #[serde(default)]
    pub show_dummy: bool,
}

impl View {
    pub fn load(path: &std::path::Path) -> View {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        sundial::package_authoring::replace_authoring_file(path, &bytes)
            .map_err(|error| error.to_string())
    }
}

pub(super) struct Browser {
    pub query: String,
    pub kind: String,
    pub show_dummy: bool,
    pub show_installed: bool,
    pub order: Order,
    pub dirty: bool,
    /// The list returns to the top on its next draw.
    pub reset_scroll: bool,
    /// The view differs from the saved view file.
    pub view_changed: bool,
    pub visible: Vec<usize>,
    pub filters: filters::Filters,
    /// Weapons with no native donor of their type and no profile donor installed.
    pub no_donor: BTreeSet<u32>,
    /// Shift+click range anchor.
    pub anchor: Option<u32>,
    /// Keyboard cursor, as a row of `visible`.
    pub cursor: Option<usize>,
    scroll_to: Option<usize>,
    last_rows: std::ops::Range<usize>,
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            query: String::new(),
            kind: String::new(),
            show_dummy: false,
            show_installed: false,
            order: Order::Catalog,
            dirty: true,
            reset_scroll: false,
            view_changed: false,
            visible: Vec::new(),
            filters: filters::Filters::default(),
            no_donor: BTreeSet::new(),
            anchor: None,
            cursor: None,
            scroll_to: None,
            last_rows: 0..0,
        }
    }
}

pub(super) const ROW_HEIGHT: f32 = 52.0;
/// Rows without artwork drop the icon tile and its height.
const TEXT_ROW_HEIGHT: f32 = 40.0;

fn row_height(icons: bool) -> f32 {
    if icons { ROW_HEIGHT } else { TEXT_ROW_HEIGHT }
}

impl Browser {
    pub fn view(&self) -> View {
        View {
            query: self.query.clone(),
            kind: self.kind.clone(),
            filters: self.filters.clone(),
            order: self.order,
            show_installed: self.show_installed,
            show_dummy: self.show_dummy,
        }
    }

    pub fn apply_view(&mut self, view: View) {
        self.query = view.query;
        self.kind = view.kind;
        self.filters = view.filters;
        self.order = view.order;
        self.show_installed = view.show_installed;
        self.show_dummy = view.show_dummy;
        self.dirty = true;
    }

    fn matches(&self, weapon: &Weapon, query: &str) -> bool {
        (self.show_dummy || !weapon.dummy)
            && (self.show_installed || !weapon.present_in_native)
            && (self.kind.is_empty() || weapon.weapon_type == self.kind)
            && self.filters.matches(weapon)
            && query.split_whitespace().all(|word| {
                weapon.name.to_lowercase().contains(word)
                    || format!("{:08x}", weapon.hash)
                        .contains(word.strip_prefix("0x").unwrap_or(word))
            })
    }

    /// The listing's donor check, before source extraction and conversion validation.
    pub fn importable(&self, weapon: &Weapon) -> bool {
        !self.no_donor.contains(&weapon.hash)
    }

    pub fn filters_active(&self) -> bool {
        !self.query.trim().is_empty()
            || !self.kind.is_empty()
            || self.filters.active()
            || self.show_installed
            || self.show_dummy
    }

    pub fn clear_filters(&mut self) {
        self.query.clear();
        self.kind.clear();
        self.filters = self.filters.cleared();
        self.show_installed = false;
        self.show_dummy = false;
        self.dirty = true;
        self.reset_scroll = true;
    }

    fn sort_key(&self, weapon: &Weapon) -> (String, String, u32) {
        let name = weapon.name.to_lowercase();
        match self.order {
            Order::Catalog => (String::new(), String::new(), 0),
            Order::Name => (String::new(), name, weapon.hash),
            Order::Type => (weapon.weapon_type.clone(), name, weapon.hash),
        }
    }

    pub fn refresh(&mut self, weapons: &[Weapon]) {
        if !self.dirty {
            return;
        }
        let cursor = self.cursor.map(|row| (row, self.visible.get(row).copied()));
        let query = self.query.trim().to_lowercase();
        self.visible = weapons
            .iter()
            .enumerate()
            .filter_map(|(index, weapon)| self.matches(weapon, &query).then_some(index))
            .collect();
        if self.order != Order::Catalog {
            let keys: Vec<_> = self
                .visible
                .iter()
                .map(|&index| self.sort_key(&weapons[index]))
                .collect();
            let mut positions: Vec<usize> = (0..self.visible.len()).collect();
            positions.sort_by(|&a, &b| keys[a].cmp(&keys[b]));
            self.visible = positions.iter().map(|&p| self.visible[p]).collect();
        }
        // Without a scroll reset the cursor follows its weapon or keeps its row.
        self.cursor = match cursor {
            Some((row, weapon)) if !self.reset_scroll && !self.visible.is_empty() => Some(
                weapon
                    .and_then(|weapon| self.visible.iter().position(|&index| index == weapon))
                    .unwrap_or(row.min(self.visible.len() - 1)),
            ),
            _ => None,
        };
        self.dirty = false;
    }

    /// Moves the cursor and asks the list to scroll it into view.
    fn move_cursor(&mut self, to: usize) {
        if self.visible.is_empty() {
            return;
        }
        let to = to.min(self.visible.len() - 1);
        self.cursor = Some(to);
        if !self.last_rows.contains(&to) || to + 1 == self.last_rows.end {
            self.scroll_to = Some(to);
        }
    }
}

enum RowEvent {
    Toggle { shift: bool },
}

struct Row<'a> {
    weapon: &'a Weapon,
    selected: bool,
    no_donor: bool,
    cursor: bool,
    idle: bool,
    stripe: bool,
    icon: bool,
    /// Source lines a Halo: Reach or Marathon row adds to its hover.
    source: Option<String>,
}

fn row_background(
    ui: &egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    row: &Row<'_>,
    active: bool,
) {
    let fill = if row.selected {
        ui.visuals().selection.bg_fill.gamma_multiply(0.35)
    } else if response.hovered() && active {
        ui.visuals().widgets.hovered.bg_fill
    } else if row.stripe {
        ui.visuals().faint_bg_color
    } else {
        egui::Color32::TRANSPARENT
    };
    let body = rect.shrink2(egui::vec2(0.0, 1.0));
    ui.painter().rect_filled(body, 4.0, fill);
    if row.selected {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                rect.min + egui::vec2(0.0, 1.0),
                egui::vec2(3.0, rect.height() - 2.0),
            ),
            2.0,
            ui.visuals().selection.bg_fill,
        );
    }
    if row.cursor {
        ui.painter().rect_stroke(
            body,
            4.0,
            ui.visuals().widgets.active.fg_stroke,
            egui::StrokeKind::Inside,
        );
    }
}

fn missing_donor_text(row: &Row<'_>) -> &'static str {
    let weapon = row.weapon;
    if row.source.is_some() {
        if weapon.family() == service::Family::Sparrow {
            "No installed native Sparrow to build on."
        } else {
            "No installed native weapon of this type to build on."
        }
    } else if weapon.is_shader() {
        "No installed native shader to convert with."
    } else if weapon.family() == service::Family::Ornament {
        "No installed universal ornament of this class and slot to build on."
    } else if weapon.family() == service::Family::WeaponOrnament {
        "Its stock weapon lists ornaments only in its own definition, which imports cannot add to."
    } else if weapon.family().is_model_gear() {
        "No installed native item matches this equipment slot and class."
    } else if weapon.family() == service::Family::Emblem {
        "No installed native emblem to convert with."
    } else {
        "No installed native weapon of this type to convert with."
    }
}

fn draw_row(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    icons: &icons::Icons,
    row: Row<'_>,
) -> Option<RowEvent> {
    let weapon = row.weapon;
    let active = row.idle && !row.no_donor;
    let mut event = None;
    // Children must not advance the virtual list's fixed-height row cursor.
    let mut row_ui = ui.new_child(egui::UiBuilder::new().id_salt(weapon.hash).max_rect(rect));
    let ui = &mut row_ui;
    let response = ui.interact(
        rect,
        ui.id().with(weapon.hash),
        if active {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            active,
            row.selected,
            &weapon.name,
        )
    });
    row_background(ui, rect, &response, &row, active);
    let height = rect.height();
    let check_rect = egui::Rect::from_min_size(
        rect.min + egui::vec2(10.0, (height - 20.0) / 2.0),
        egui::vec2(20.0, 20.0),
    );
    let mut checked = row.selected;
    let check = ui
        .add_enabled_ui(active, |ui| {
            ui.put(check_rect, egui::Checkbox::without_text(&mut checked))
        })
        .inner;
    let mut text_left = rect.min.x + 40.0;
    if row.icon {
        let icon_rect = egui::Rect::from_min_size(
            rect.min + egui::vec2(40.0, (height - 48.0) / 2.0),
            egui::vec2(48.0, 48.0),
        );
        ui.painter()
            .rect_filled(icon_rect, 4.0, ui.visuals().extreme_bg_color);
        icons.draw(ui, icon_rect, weapon.icon_index);
        text_left = rect.min.x + 98.0;
    }

    let badge = if row.no_donor {
        Some(("No Donor", ui.visuals().error_fg_color))
    } else if weapon.present_in_native {
        Some(("Installed", ui.visuals().weak_text_color()))
    } else {
        None
    };
    let mut text_right = rect.right() - 12.0;
    if let Some((text, color)) = badge {
        let badge_rect = egui::Rect::from_min_size(
            egui::pos2(rect.right() - 112.0, rect.top() + (height - 22.0) / 2.0),
            egui::vec2(100.0, 22.0),
        );
        text_right = badge_rect.left() - 12.0;
        let badge = ui
            .new_child(
                egui::UiBuilder::new()
                    .max_rect(badge_rect)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            )
            .add(egui::Label::new(egui::RichText::new(text).color(color)).selectable(false));
        if row.no_donor {
            badge.on_hover_text(missing_donor_text(&row));
        }
    }
    let text_top = rect.top() + (height - 36.0) / 2.0;
    let title_rect = egui::Rect::from_min_max(
        egui::pos2(text_left, text_top),
        egui::pos2(text_right, text_top + 18.0),
    );
    let subtitle = filters::details(weapon);
    // `put` centres a label; rows need left-aligned text.
    for (rect, text) in [
        (title_rect, egui::RichText::new(&weapon.name).strong()),
        (
            title_rect.translate(egui::vec2(0.0, 18.0)),
            egui::RichText::new(&subtitle).weak(),
        ),
    ] {
        ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        )
        .add(egui::Label::new(text).truncate().selectable(false));
    }
    let clicked = response.clicked() && !check.hovered();
    response.on_hover_text(match &row.source {
        Some(source) => format!("{}\n{}\n{source}", weapon.name, subtitle),
        None => format!(
            "{}\n{}\nSource Item: {:08X}",
            weapon.name, subtitle, weapon.hash
        ),
    });
    if clicked || check.changed() {
        event = Some(RowEvent::Toggle {
            shift: clicked && ui.input(|input| input.modifiers.shift),
        });
    }
    event
}

impl PackageAuthoringApp {
    pub(super) fn draw_importer_toolbar(&mut self, ui: &mut egui::Ui) {
        let browser = &mut self.importer.browser;
        let mut changed = false;
        let mut query_changed = false;
        let mut query_left = false;
        ui.horizontal_wrapped(|ui| {
            style::compact_controls(ui);
            let search = ui.add_sized(
                [
                    ui.available_width().clamp(180.0, 320.0),
                    ui.spacing().interact_size.y,
                ],
                egui::TextEdit::singleline(&mut browser.query)
                    .hint_text("Search Items by Name or Hash"),
            );
            super::super::pickers::name_response(ui, &search, "Search Items");
            query_changed = search.changed();
            query_left = search.lost_focus();
            if ui
                .add_enabled(browser.filters_active(), egui::Button::new("Reset Filters"))
                .clicked()
            {
                browser.clear_filters();
                changed = true;
            }
            let order = egui::ComboBox::from_id_salt("d2-importer-order")
                .width(160.0)
                .selected_text(format!("Sort: {}", browser.order.label()))
                .show_ui(ui, |ui| {
                    for order in Order::ALL {
                        changed |= ui
                            .selectable_value(&mut browser.order, order, order.label())
                            .changed();
                    }
                })
                .response;
            super::super::pickers::name_response(ui, &order, "Sort Order");
        });
        let settings = &mut self.importer.settings;
        let preferences = |settings: &service::Settings| {
            (
                settings.marathon_animations,
                settings.marathon_ornaments,
                settings.modern_ornaments,
                settings.d1_roi_ornaments,
            )
        };
        let before = preferences(settings);
        changed |= filters::draw(
            ui,
            browser,
            &self.importer.weapons,
            self.importer.source,
            filters::Preferences {
                marathon_animations: &mut settings.marathon_animations,
                marathon_ornaments: &mut settings.marathon_ornaments,
                modern_ornaments: &mut settings.modern_ornaments,
                d1_roi_ornaments: &mut settings.d1_roi_ornaments,
            },
        );
        let toggled = preferences(settings) != before;
        if changed || query_changed {
            browser.dirty = true;
            browser.reset_scroll = true;
            browser.view_changed = true;
        }
        if changed || query_left {
            self.importer.save_view();
        }
        if toggled && let Err(error) = self.importer.save() {
            self.importer.notice = error;
        }
    }

    /// Arrow keys move the cursor, Space toggles it, Shift extends, Ctrl+A selects the list.
    fn importer_keyboard(&mut self, ui: &egui::Ui, idle: bool) {
        if !idle || ui.memory(|memory| memory.focused().is_some()) {
            return;
        }
        let Importer {
            browser,
            weapons,
            selected,
            ..
        } = &mut self.importer;
        let count = browser.visible.len();
        if count == 0 {
            return;
        }
        let (down, up, home, end, space, all, shift) = ui.input_mut(|input| {
            let shift = input.modifiers.shift;
            let key = |input: &mut egui::InputState, key| {
                input.consume_key(egui::Modifiers::NONE, key)
                    || input.consume_key(egui::Modifiers::SHIFT, key)
            };
            (
                key(input, egui::Key::ArrowDown),
                key(input, egui::Key::ArrowUp),
                key(input, egui::Key::Home),
                key(input, egui::Key::End),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Space),
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::A),
                shift,
            )
        });
        let current = browser.cursor;
        let target = if down {
            Some(current.map_or(0, |row| row + 1))
        } else if up {
            Some(current.map_or(0, |row| row.saturating_sub(1)))
        } else if home {
            Some(0)
        } else if end {
            Some(count - 1)
        } else {
            None
        };
        if let Some(target) = target {
            browser.move_cursor(target);
            if shift && let Some(row) = browser.cursor {
                let weapon = &weapons[browser.visible[row]];
                if browser.importable(weapon) {
                    selected.insert(weapon.hash);
                }
            }
        }
        if space && let Some(row) = browser.cursor {
            let weapon = &weapons[browser.visible[row]];
            if browser.importable(weapon) && !selected.remove(&weapon.hash) {
                selected.insert(weapon.hash);
            }
            browser.anchor = Some(weapon.hash);
        }
        if all {
            selected.extend(
                browser
                    .visible
                    .iter()
                    .map(|&index| &weapons[index])
                    .filter(|weapon| browser.importable(weapon))
                    .map(|weapon| weapon.hash),
            );
        }
    }

    pub(super) fn draw_importer_browser(&mut self, ui: &mut egui::Ui, idle: bool) {
        self.importer.browser.refresh(&self.importer.weapons);
        let reset_scroll = std::mem::take(&mut self.importer.browser.reset_scroll);
        self.importer_keyboard(ui, idle);
        let Importer {
            browser,
            weapons,
            selected,
            icons,
            settings,
            origins,
            source,
            ..
        } = &mut self.importer;
        let with_icons = source.icons();
        let row_height = row_height(with_icons);
        if browser.visible.is_empty() {
            let (title, detail) = if weapons.is_empty() {
                ("No Items", "")
            } else if browser.filters_active() {
                ("No Matches", "")
            } else {
                (
                    "All Installed",
                    "Enable Show Installed to browse these items.",
                )
            };
            ui.add_space(ui.available_height() * 0.3);
            let mut cleared = false;
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(title).heading().weak());
                if !detail.is_empty() {
                    style::hint(ui, detail);
                }
                ui.add_space(6.0);
                if browser.filters_active() && ui.button("Reset Filters").clicked() {
                    browser.clear_filters();
                    browser.view_changed = true;
                    cleared = true;
                }
            });
            if cleared {
                self.importer.save_view();
            }
            return;
        }
        let step = row_height + ui.spacing().item_spacing.y;
        let viewport = ui.available_height();
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("importer-weapons")
            .auto_shrink([false, false]);
        if reset_scroll {
            scroll = scroll.vertical_scroll_offset(0.0);
        } else if let Some(row) = browser.scroll_to.take() {
            let top = row as f32 * step;
            let offset = if row < browser.last_rows.start {
                top
            } else {
                (top + step - viewport).max(0.0)
            };
            scroll = scroll.vertical_scroll_offset(offset);
        }
        let mut icon_indices = Vec::new();
        let mut events = Vec::new();
        let mut seen = 0..0;
        scroll.show_rows(ui, row_height, browser.visible.len(), |ui, rows| {
            seen = rows.clone();
            for row in rows {
                let weapon = &weapons[browser.visible[row]];
                if let Some(index) = weapon.icon_index.filter(|_| with_icons) {
                    icon_indices.push(index);
                }
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), row_height),
                    egui::Sense::hover(),
                );
                let event = draw_row(
                    ui,
                    rect,
                    icons,
                    Row {
                        weapon,
                        selected: selected.contains(&weapon.hash),
                        no_donor: !browser.importable(weapon),
                        cursor: browser.cursor == Some(row),
                        idle,
                        stripe: row % 2 == 0,
                        icon: with_icons,
                        source: origins.get(&weapon.hash).map(Origin::hover),
                    },
                );
                if let Some(event) = event {
                    events.push((row, event));
                }
            }
        });
        browser.last_rows = seen;
        for (row, event) in events {
            let hash = weapons[browser.visible[row]].hash;
            match event {
                RowEvent::Toggle { shift } => {
                    browser.cursor = Some(row);
                    let anchor = browser.anchor.and_then(|anchor| {
                        browser
                            .visible
                            .iter()
                            .position(|&index| weapons[index].hash == anchor)
                    });
                    match anchor {
                        Some(anchor) if shift => {
                            let range = anchor.min(row)..=anchor.max(row);
                            selected.extend(
                                browser.visible[range]
                                    .iter()
                                    .map(|&index| &weapons[index])
                                    .filter(|weapon| browser.importable(weapon))
                                    .map(|weapon| weapon.hash),
                            );
                        }
                        _ => {
                            if !selected.remove(&hash) {
                                selected.insert(hash);
                            }
                            browser.anchor = Some(hash);
                        }
                    }
                }
            }
        }
        if with_icons {
            icons.request(ui.ctx(), *source, settings, &icon_indices);
        }
    }
}

impl Importer {
    /// Writes the view file if the view changed since the last write.
    pub(in crate::app) fn save_view(&mut self) {
        if !std::mem::take(&mut self.browser.view_changed) {
            return;
        }
        let saved = self
            .view_path
            .as_ref()
            .ok_or_else(|| "Could not locate the importer view file".to_owned())
            .and_then(|path| self.browser.view().save(path));
        if let Err(error) = saved {
            self.notice = error;
            self.browser.view_changed = true;
        }
    }
}
