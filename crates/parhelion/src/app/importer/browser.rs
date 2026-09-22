use super::*;

#[derive(Default, PartialEq, Eq, Clone, Copy, serde::Deserialize, serde::Serialize)]
pub(super) enum Order {
    #[default]
    Catalog,
    Name,
    Type,
    Status,
}

impl Order {
    pub const ALL: [Order; 4] = [Order::Catalog, Order::Name, Order::Type, Order::Status];

    pub fn label(self) -> &'static str {
        match self {
            Order::Catalog => "Catalog Order",
            Order::Name => "Name",
            Order::Type => "Weapon Type",
            Order::Status => "Status",
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
    #[serde(default)]
    pub status: status::Filter,
    #[serde(default)]
    pub order: Order,
    #[serde(default)]
    pub show_installed: bool,
    #[serde(default)]
    pub show_dummy: bool,
}

impl View {
    pub fn load() -> View {
        data_root()
            .ok()
            .and_then(|root| std::fs::read(root.join("importer-view.json")).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let root = data_root()?;
        std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        sundial::package_authoring::replace_authoring_file(&root.join("importer-view.json"), &bytes)
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
    pub visible: Vec<usize>,
    pub types: BTreeMap<String, usize>,
    pub records: BTreeMap<u32, status::Record>,
    pub status: status::Filter,
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
            visible: Vec::new(),
            types: BTreeMap::new(),
            records: BTreeMap::new(),
            status: status::Filter::All,
            no_donor: BTreeSet::new(),
            anchor: None,
            cursor: None,
            scroll_to: None,
            last_rows: 0..0,
        }
    }
}

pub(super) const ROW_HEIGHT: f32 = 64.0;

impl Browser {
    pub fn view(&self) -> View {
        View {
            query: self.query.clone(),
            kind: self.kind.clone(),
            status: self.status,
            order: self.order,
            show_installed: self.show_installed,
            show_dummy: self.show_dummy,
        }
    }

    pub fn apply_view(&mut self, view: View) {
        self.query = view.query;
        self.kind = view.kind;
        self.status = view.status;
        self.order = view.order;
        self.show_installed = view.show_installed;
        self.show_dummy = view.show_dummy;
        self.dirty = true;
    }

    fn matches(&self, weapon: &Weapon, query: &str) -> bool {
        (self.show_dummy || !weapon.dummy)
            && (self.show_installed || !weapon.present_in_native)
            && (self.kind.is_empty() || weapon.weapon_type == self.kind)
            && match self.status {
                status::Filter::All => true,
                status::Filter::Working => self.is_working(weapon.hash),
                status::Filter::NotTested => !self.is_working(weapon.hash),
            }
            && (query.is_empty()
                || weapon.name.to_lowercase().contains(query)
                || format!("{:08x}", weapon.hash).contains(query.trim_start_matches("0x")))
    }

    pub fn is_working(&self, hash: u32) -> bool {
        self.records.get(&hash).is_some_and(|record| record.working)
    }

    pub fn importable(&self, weapon: &Weapon) -> bool {
        !self.no_donor.contains(&weapon.hash)
    }

    pub fn filters_active(&self) -> bool {
        !self.query.trim().is_empty() || !self.kind.is_empty() || self.status != status::Filter::All
    }

    pub fn clear_filters(&mut self) {
        self.query.clear();
        self.kind.clear();
        self.status = status::Filter::All;
        self.dirty = true;
    }

    fn sort_key(&self, weapon: &Weapon) -> (u8, String, String, u32) {
        let name = weapon.name.to_lowercase();
        match self.order {
            Order::Catalog => (0, String::new(), String::new(), 0),
            Order::Name => (0, String::new(), name, weapon.hash),
            Order::Type => (0, weapon.weapon_type.clone(), name, weapon.hash),
            Order::Status => (
                u8::from(!self.is_working(weapon.hash)),
                String::new(),
                name,
                weapon.hash,
            ),
        }
    }

    pub fn refresh(&mut self, weapons: &[Weapon]) {
        if !self.dirty {
            return;
        }
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
        self.cursor = None;
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
    SetStatus(bool),
}

struct Row<'a> {
    weapon: &'a Weapon,
    selected: bool,
    working: bool,
    no_donor: bool,
    cursor: bool,
    note: Option<&'a str>,
    idle: bool,
    stripe: bool,
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
    let check_rect =
        egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 22.0), egui::vec2(20.0, 20.0));
    let mut checked = row.selected;
    let check = ui
        .add_enabled_ui(active, |ui| {
            ui.put(check_rect, egui::Checkbox::without_text(&mut checked))
        })
        .inner;
    let icon_rect =
        egui::Rect::from_min_size(rect.min + egui::vec2(40.0, 8.0), egui::vec2(48.0, 48.0));
    ui.painter()
        .rect_filled(icon_rect, 4.0, ui.visuals().extreme_bg_color);
    icons.draw(ui, icon_rect, weapon.icon_index);

    let status_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 112.0, rect.top() + 21.0),
        egui::vec2(100.0, 22.0),
    );
    let (status_text, status_color) = if row.no_donor {
        ("No Donor", ui.visuals().error_fg_color)
    } else {
        (
            status::name(row.working),
            status::color(ui.visuals(), row.working),
        )
    };
    let status = ui
        .new_child(
            egui::UiBuilder::new()
                .max_rect(status_rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        )
        .add(
            egui::Label::new(egui::RichText::new(status_text).color(status_color))
                .selectable(false),
        );
    if row.no_donor {
        status.on_hover_text("No installed native weapon of this type to convert with.");
    } else if let Some(note) = row.note {
        status.on_hover_text(note);
    }

    let text_left = rect.min.x + 98.0;
    let text_right = status_rect.left() - 12.0;
    let title_rect = egui::Rect::from_min_max(
        egui::pos2(text_left, rect.top() + 9.0),
        egui::pos2(text_right, rect.top() + 31.0),
    );
    let mut subtitle = format!("{} · {:08X}", weapon.weapon_type, weapon.hash);
    if weapon.dummy {
        subtitle.push_str(" · Dummy");
    }
    if weapon.present_in_native {
        subtitle.push_str(" · Installed");
    }
    // `put` centres a label; rows need left-aligned text.
    for (rect, text) in [
        (title_rect, egui::RichText::new(&weapon.name).strong()),
        (
            title_rect.translate(egui::vec2(0.0, 22.0)),
            egui::RichText::new(subtitle).weak(),
        ),
    ] {
        ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        )
        .add(egui::Label::new(text).truncate().selectable(false));
    }
    response.context_menu(|ui| {
        let flipped = !row.working;
        if ui
            .button(format!("Mark {}", status::name(flipped)))
            .clicked()
        {
            event = Some(RowEvent::SetStatus(flipped));
            ui.close_menu();
        }
    });
    let clicked = response.clicked() && !check.hovered();
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
        ui.horizontal_wrapped(|ui| {
            style::compact_controls(ui);
            changed |= sundial::ui::catalog::search(ui, &mut browser.query, false, 220.0, "Search");
            egui::ComboBox::from_id_salt("d2-importer-type")
                .width(150.0)
                .selected_text(if browser.kind.is_empty() {
                    "All Types"
                } else {
                    &browser.kind
                })
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(&mut browser.kind, String::new(), "All Types")
                        .changed();
                    let kinds: Vec<_> = browser
                        .types
                        .iter()
                        .map(|(kind, count)| (kind.clone(), *count))
                        .collect();
                    for (kind, count) in kinds {
                        changed |= ui
                            .selectable_value(
                                &mut browser.kind,
                                kind.clone(),
                                format!("{kind} ({count})"),
                            )
                            .changed();
                    }
                });
            egui::ComboBox::from_id_salt("d2-importer-status")
                .width(110.0)
                .selected_text(browser.status.label())
                .show_ui(ui, |ui| {
                    for filter in status::Filter::ALL {
                        changed |= ui
                            .selectable_value(&mut browser.status, filter, filter.label())
                            .changed();
                    }
                });
            egui::ComboBox::from_id_salt("d2-importer-order")
                .width(120.0)
                .selected_text(browser.order.label())
                .show_ui(ui, |ui| {
                    for order in Order::ALL {
                        changed |= ui
                            .selectable_value(&mut browser.order, order, order.label())
                            .changed();
                    }
                });
            changed |= ui
                .checkbox(&mut browser.show_installed, "Installed")
                .changed();
            changed |= ui.checkbox(&mut browser.show_dummy, "Dummy").changed();
        });
        if changed {
            browser.dirty = true;
            if let Err(error) = browser.view().save() {
                self.importer.notice = error;
            }
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
        let reset_scroll = self.importer.browser.dirty;
        self.importer.browser.refresh(&self.importer.weapons);
        self.importer_keyboard(ui, idle);
        let Importer {
            browser,
            weapons,
            selected,
            icons,
            settings,
            ..
        } = &mut self.importer;
        if browser.visible.is_empty() {
            let (title, detail) = if weapons.is_empty() {
                ("No weapons", "")
            } else if browser.filters_active() {
                ("No matches", "")
            } else {
                ("All installed", "Tick Installed to list them.")
            };
            ui.add_space(ui.available_height() * 0.3);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(title).heading().weak());
                if !detail.is_empty() {
                    style::hint(ui, detail);
                }
                ui.add_space(6.0);
                if browser.filters_active() && ui.button("Clear Filters").clicked() {
                    browser.clear_filters();
                }
            });
            return;
        }
        let step = ROW_HEIGHT + ui.spacing().item_spacing.y;
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
        scroll.show_rows(ui, ROW_HEIGHT, browser.visible.len(), |ui, rows| {
            seen = rows.clone();
            for row in rows {
                let weapon = &weapons[browser.visible[row]];
                if let Some(index) = weapon.icon_index {
                    icon_indices.push(index);
                }
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), ROW_HEIGHT),
                    egui::Sense::hover(),
                );
                let record = browser.records.get(&weapon.hash);
                let event = draw_row(
                    ui,
                    rect,
                    icons,
                    Row {
                        weapon,
                        selected: selected.contains(&weapon.hash),
                        working: record.is_some_and(|record| record.working),
                        no_donor: !browser.importable(weapon),
                        cursor: browser.cursor == Some(row),
                        note: record
                            .map(|record| record.note.as_str())
                            .filter(|note| !note.is_empty()),
                        idle,
                        stripe: row % 2 == 0,
                    },
                );
                if let Some(event) = event {
                    events.push((row, event));
                }
            }
        });
        browser.last_rows = seen;
        let mut status_change = None;
        for (row, event) in events {
            let hash = weapons[browser.visible[row]].hash;
            match event {
                RowEvent::SetStatus(working) => status_change = Some((vec![hash], working)),
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
        if let Some(modern) = &settings.modern_packages {
            icons.request(ui.ctx(), modern, &icon_indices);
        }
        if let Some((hashes, working)) = status_change {
            self.set_import_status(&hashes, working);
        }
    }

    pub(super) fn set_import_status(&mut self, hashes: &[u32], working: bool) {
        let mut records = self.importer.browser.records.clone();
        for &hash in hashes {
            let note = records
                .get(&hash)
                .filter(|record| record.working == working)
                .map(|record| record.note.clone())
                .unwrap_or_default();
            records.insert(hash, status::Record { working, note });
        }
        match status::save(&records) {
            Ok(()) => {
                self.importer.browser.records = records;
                self.importer.browser.dirty = true;
            }
            Err(error) => self.importer.notice = error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weapon(hash: u32, name: &str, kind: &str) -> Weapon {
        Weapon {
            hash,
            name: name.into(),
            weapon_type: kind.into(),
            present_in_native: false,
            dummy: false,
            icon_index: None,
        }
    }

    #[test]
    fn working_filter_never_promotes_an_untested_import() {
        let weapons = vec![
            weapon(1, "Tested", "Sword"),
            weapon(2, "New Import", "Sword"),
        ];
        let mut browser = Browser::default();
        browser.records.insert(
            1,
            status::Record {
                working: true,
                note: String::new(),
            },
        );
        browser.status = status::Filter::Working;
        browser.refresh(&weapons);
        assert_eq!(browser.visible, [0]);
        browser.status = status::Filter::NotTested;
        browser.dirty = true;
        browser.refresh(&weapons);
        assert_eq!(browser.visible, [1]);
    }

    #[test]
    fn sorting_reorders_without_hiding_and_ends_with_the_name() {
        let weapons = vec![
            weapon(3, "Zephyr", "Sword"),
            weapon(1, "Ace", "Hand Cannon"),
            weapon(2, "Midnight", "Sword"),
        ];
        let mut browser = Browser::default();
        browser.records.insert(
            2,
            status::Record {
                working: true,
                note: String::new(),
            },
        );
        browser.refresh(&weapons);
        assert_eq!(browser.visible, [0, 1, 2], "catalog order is untouched");
        for (order, expected) in [
            (Order::Name, [1, 2, 0]),
            (Order::Type, [1, 2, 0]),
            (Order::Status, [2, 1, 0]),
        ] {
            browser.order = order;
            browser.dirty = true;
            browser.refresh(&weapons);
            assert_eq!(browser.visible, expected, "{}", order.label());
        }
    }

    #[test]
    fn fixed_height_rows_scroll_without_rendering_the_whole_catalog() {
        let mut app = PackageAuthoringApp::default();
        app.importer.settings.modern_packages = Some(PathBuf::from("."));
        app.importer.read_requested = true;
        app.importer.weapons = (0..1000)
            .map(|hash| weapon(hash, &format!("Test Weapon {hash:04}"), "Auto Rifle"))
            .collect();
        let ctx = egui::Context::default();
        let mut names = Vec::new();
        for frame in 0..8 {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                time: Some(f64::from(frame) / 10.0),
                ..Default::default()
            };
            input
                .events
                .push(egui::Event::PointerMoved(egui::pos2(300.0, 300.0)));
            if frame == 3 {
                input.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -900.0),
                    modifiers: egui::Modifiers::default(),
                });
            }
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.draw_importer_contents(ui));
            });
            names = output
                .shapes
                .iter()
                .filter_map(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape
                        && text.galley.text().starts_with("Test Weapon")
                        && !text.galley.text().contains('\n')
                    {
                        Some((text.galley.text().to_owned(), text.pos.y))
                    } else {
                        None
                    }
                })
                .collect();
            assert!(
                !names.is_empty() && names.len() < 10,
                "Only viewport rows should be rendered: {names:?}"
            );
            for pair in names.windows(2) {
                assert!(
                    (pair[1].1 - pair[0].1 - ROW_HEIGHT - 4.0).abs() < 1.0,
                    "Rows must keep a stable height: {names:?}"
                );
            }
        }
        assert!(
            names.iter().all(|(name, _)| name != "Test Weapon 0000"),
            "Wheel scrolling must move the catalog"
        );
    }

    #[test]
    fn filters_keep_variants_and_exclude_dummy_and_installed_items_by_default() {
        let weapons: Vec<Weapon> = serde_json::from_value(serde_json::json!([
            {"hash":1,"name":"Same Name","weapon_type":"Auto Rifle","present_in_native":false},
            {"hash":2,"name":"Same Name","weapon_type":"Auto Rifle","present_in_native":false,"dummy":true},
            {"hash":3,"name":"Installed","weapon_type":"Auto Rifle","present_in_native":true}
        ])).unwrap();
        let mut browser = Browser::default();
        browser.refresh(&weapons);
        assert_eq!(browser.visible, [0]);
        browser.show_dummy = true;
        browser.show_installed = true;
        browser.query = "same".into();
        browser.dirty = true;
        browser.refresh(&weapons);
        assert_eq!(browser.visible, [0, 1]);
        browser.query = "0x00000003".into();
        browser.dirty = true;
        browser.refresh(&weapons);
        assert_eq!(browser.visible, [2]);
        assert!(browser.filters_active());
        browser.clear_filters();
        assert!(!browser.filters_active());
    }
}
