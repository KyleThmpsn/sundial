//! Resolve property summaries from package values, including when a saved perk is reopened.
use super::*;
use sundial::package_authoring::sandbox_perk::program::Asset;

mod hud;

/// One Properties entry point for action and condition settings.
pub(super) struct Panel {
    id: egui::Id,
    open: bool,
}

impl Panel {
    pub fn new(ui: &egui::Ui, scope: impl std::hash::Hash + std::fmt::Debug) -> Self {
        let id = ui.make_persistent_id(("properties", scope));
        Self {
            id,
            open: ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false)),
        }
    }

    pub fn reveal(&mut self, ui: &egui::Ui) {
        self.open = true;
        ui.data_mut(|data| data.insert_temp(self.id, true));
    }

    /// The switch in an item's menu. The panel opens under the item, so the header carries
    /// no button of its own.
    pub fn menu_item(&mut self, ui: &mut egui::Ui) {
        let label = if self.open {
            "Hide Properties"
        } else {
            "Show Properties"
        };
        if ui.button(label).clicked() {
            self.open = !self.open;
            ui.data_mut(|data| data.insert_temp(self.id, self.open));
            ui.close();
        }
    }

    pub fn show(self, ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
        if self.open {
            ui.separator();
            let top = ui.cursor().top();
            ui.push_id(self.id, contents);
            // An item with nothing more to show says so, rather than opening an empty pane.
            if ui.cursor().top() - top < 1.0 {
                ui.weak("No More Properties");
            }
        }
    }
}

/// How much weight a label row carries. Authored settings read as plain, source-backed
/// readings read as weak. Add a strong weight here when a row needs one.
#[derive(Clone, Copy)]
pub(super) enum Emphasis {
    Plain,
    Weak,
}

/// A shared label/value row for decoded native properties. The label column is one line
/// high and truncates rather than wrapping, so a long label in a narrow pane cannot spill
/// into the row below it. The full label and the hint stay on hover.
pub(super) fn field<R>(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    value: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    row(ui, label, hint, Emphasis::Plain, value)
}

/// The shared row with the label weight chosen by the caller.
pub(super) fn row<R>(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    emphasis: Emphasis,
    value: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    row_with(ui, label, hint, emphasis, |_| false, value)
}

/// The shared row, letting a caller fill the label column itself. The label closure returns
/// whether it drew anything, matching the verified label helpers, and the plain label is
/// drawn whenever it declines. Every row shares one width rule, so the value column lines up
/// at any nesting depth and in any pane width.
pub(super) fn row_with<R>(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    emphasis: Emphasis,
    custom_label: impl FnOnce(&mut egui::Ui) -> bool,
    value: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    // One width for every labelled row, the same one the fixed width cells use. A
    // proportional column moved with the pane and shrank at each nesting level, so controls
    // that belong to one card started at a different place in every block of it.
    let stacked = ui.available_width() < 340.0;
    let width = if stacked {
        ui.available_width()
    } else {
        controls::CELL_LABEL_WIDTH.min((ui.available_width() - 100.0).max(100.0))
    };
    // A label in its own column reads inward toward the value beside it, so it is right
    // aligned. A stacked label has no column and sits above its control instead, so it leads
    // from the left: right aligning it across the whole row put the name against the far edge
    // with its control on the next line, a card's width away from what it names.
    let (layout, halign) = if stacked {
        (
            egui::Layout::left_to_right(egui::Align::Center),
            egui::Align::Min,
        )
    } else {
        (
            egui::Layout::right_to_left(egui::Align::Center),
            egui::Align::Max,
        )
    };
    let draw = |ui: &mut egui::Ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(width, ui.spacing().interact_size.y),
            layout,
            |ui| {
                ui.set_min_width(width);
                if custom_label(ui) {
                    return;
                }
                let text = match emphasis {
                    Emphasis::Plain => egui::RichText::new(label),
                    Emphasis::Weak => egui::RichText::new(label).weak(),
                };
                let response = ui.add(crate::app::style::cut_label(ui, text).halign(halign));
                let hover = match (label, hint) {
                    ("", hint) => hint.to_owned(),
                    (label, "") => label.to_owned(),
                    (label, hint) => format!("{label}\n{hint}"),
                };
                if !hover.is_empty() {
                    response.on_hover_text(hover);
                }
            },
        );
        value(ui)
    };
    if stacked {
        ui.vertical(draw).inner
    } else {
        ui.horizontal(draw).inner
    }
}

/// The perk editor windows sit beside this module tree rather than inside it, so the shared
/// row reaches them through the workbench type they are matching. Both entry points forward
/// to the row above, keeping the width rule and the label style in one place.
impl Workbench {
    pub(in crate::app::custom_perks) fn property_row<R>(
        ui: &mut egui::Ui,
        label: &str,
        hint: &str,
        value: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        field(ui, label, hint, value)
    }
}

/// A combo over `choices`, showing the current value's name, or its number when none names it.
/// Returns a new choice.
fn choice(
    ui: &mut egui::Ui,
    salt: &str,
    choices: &[(i64, &'static str)],
    current: i64,
) -> Option<i64> {
    let text = choices
        .iter()
        .find(|(value, _)| *value == current)
        .map_or_else(|| current.to_string(), |(_, name)| (*name).to_owned());
    let mut picked = None;
    egui::ComboBox::from_id_salt(salt)
        .selected_text(text)
        .show_ui(ui, |ui| {
            for (value, name) in choices {
                if ui.selectable_label(*value == current, *name).clicked() {
                    picked = Some(*value);
                }
            }
        });
    picked
}

/// Component editing keeps the existing checked asset editor and draft flow.
pub(super) fn edit_object(ui: &mut egui::Ui, asset: &Asset) -> bool {
    ui.add_enabled(
        !matches!(asset.graph, 0 | u32::MAX),
        egui::Button::new("Edit Components…"),
    )
    .on_disabled_hover_text("Choose an object to edit its components.")
    .clicked()
}

#[derive(Default)]
pub(super) struct Properties {
    packages: PathBuf,
    source: Option<Arc<entity::catalog::Catalog>>,
    graphs: BTreeMap<u32, Result<Arc<PrivatePerkRuntimeGraph>, String>>,
    pending: Option<(u32, Receiver<Result<PrivatePerkRuntimeGraph, String>>)>,
    /// Summaries of recent assets' property changes.
    changes: Vec<Changes>,
    /// Text typed into an asset's value rows and not yet parsed, by asset.
    text: BTreeMap<u32, BTreeMap<(WeaponRuntimeFieldLocator, u8), String>>,
    hud: hud::HudStatuses,
}

/// One asset's property change lines, read from a loaded graph and the asset's values.
struct Changes {
    graph: Arc<PrivatePerkRuntimeGraph>,
    values: Vec<WeaponRuntimeValueOverride>,
    /// Whether every named value was in view, so only the rest are lines.
    named: bool,
    lines: Result<Vec<String>, String>,
}

/// How many assets' summaries are kept.
const CHANGES_KEPT: usize = 32;

impl Properties {
    fn load_error(&mut self, ui: &mut egui::Ui, tag: u32, error: String) {
        let retry = ui
            .horizontal_wrapped(|ui| {
                ui.colored_label(ui.visuals().error_fg_color, "Could not load properties.");
                ui.small_button("Retry").clicked()
            })
            .inner;
        ui.add(egui::Label::new(error).wrap());
        if retry {
            self.graphs.remove(&tag);
            self.request(ui, tag);
        }
    }

    fn request(&mut self, ui: &egui::Ui, tag: u32) {
        if self.pending.is_none() && self.packages.is_dir() && !self.graphs.contains_key(&tag) {
            let (sender, receiver) = std::sync::mpsc::channel();
            let packages = self.packages.clone();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let _ = sender.send(super::parameters::load_entity_parameters(&packages, tag));
                ctx.request_repaint();
            });
            self.pending = Some((tag, receiver));
        }
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }

    /// Change Weapon Properties' attachment: `tag` with every modifier row neutral, once its
    /// rows are read. Until then, or when they could not be, why not.
    pub fn neutral_attachment(&mut self, ui: &egui::Ui, tag: u32) -> Result<Asset, &'static str> {
        if !self.packages.is_dir() {
            return Err("Its rows need the game's packages.");
        }
        let graph = match self.graphs.get(&tag) {
            Some(Ok(graph)) => graph.clone(),
            Some(Err(_)) => return Err("Its rows could not be read."),
            None => {
                self.request(ui, tag);
                return Err("Its rows are still being read.");
            }
        };
        let values =
            parameters::modifiers::neutral(&graph).map_err(|_| "Its rows could not be written.")?;
        let path = self
            .source
            .as_ref()
            .and_then(|catalog| catalog.entries.iter().find(|entry| entry.graph == tag))
            .and_then(|entry| entry.native_paths.first().cloned())
            .unwrap_or_default();
        Ok(Asset {
            graph: tag,
            path,
            values,
            ..Asset::default()
        })
    }

    /// The Damage Type tile of a projectile asset: the damage type every damage profile its
    /// private copy's graphs name deals, its own first.
    pub fn damage_type(&self, ui: &mut egui::Ui, asset: &mut Asset) {
        use sundial::package_authoring::sandbox_perk::program::DamageMode;
        const TYPES: [(DamageMode, &str); 4] = [
            (DamageMode::Kinetic, "Kinetic"),
            (DamageMode::Arc, "Arc"),
            (DamageMode::Solar, "Solar"),
            (DamageMode::Void, "Void"),
        ];
        if matches!(asset.graph, 0 | u32::MAX) {
            return;
        }
        let current = asset.damage_type;
        let label = |choice: Option<DamageMode>| {
            choice
                .and_then(|choice| TYPES.iter().find(|(each, _)| *each == choice))
                .map_or("Original", |(_, label)| label)
        };
        let (picked, reset) = crate::app::style::tiles(ui, |ui, width| {
            crate::app::style::stock_tile(
                ui,
                (width, ("projectile-damage-type", asset.graph)),
                ("Damage Type", "Changes its damage, not its colors"),
                current.map(|_| "Original"),
                |ui| {
                    let mut choice = current;
                    let response =
                        egui::ComboBox::from_id_salt(("projectile-damage-type", asset.graph))
                            .selected_text(label(current))
                            .width(width)
                            .truncate()
                            .show_ui(ui, |ui| {
                                crate::app::workbench_style(ui);
                                ui.selectable_value(&mut choice, None, "Original");
                                for (each, name) in TYPES {
                                    ui.selectable_value(&mut choice, Some(each), name);
                                }
                            })
                            .response;
                    let _ = crate::app::style::named_control(response, "Damage Type");
                    (choice != current).then_some(choice)
                },
            )
        });
        if let Some(choice) = picked {
            asset.damage_type = choice;
        } else if reset {
            asset.damage_type = None;
        }
    }

    pub fn movement(&mut self, ui: &mut egui::Ui, asset: &mut Asset) {
        if matches!(asset.graph, 0 | u32::MAX) {
            return;
        }
        match self.graphs.get(&asset.graph) {
            Some(Ok(loaded)) => {
                let loaded = loaded.clone();
                let parameters = parameters::movement::mapped(&loaded);
                let mut groups = BTreeMap::<_, Vec<_>>::new();
                for (tag, parameter) in parameters {
                    groups
                        .entry((tag, parameter.owner_tag))
                        .or_default()
                        .push(parameter);
                }
                let several = groups.len() > 1;
                for (part, ((tag, owner), parameters)) in groups.into_iter().enumerate() {
                    // A projectile with several movement components reads them as numbered
                    // parts. The component's identifier is for inspection, so it stays on hover.
                    if several {
                        let color = crate::app::style::secondary(ui.visuals());
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(format!("Projectile Part {}", part + 1))
                                .color(color),
                        )
                        .on_hover_text(format!("Component 0x{owner:08X}"));
                    }
                    let id = ui.id().with(("projectile-error", tag, owner));
                    crate::app::style::tiles(ui, |ui, width| {
                        for parameter in &parameters {
                            if let Some(result) = parameters::movement::draw_parameter(
                                ui,
                                width,
                                &loaded,
                                tag,
                                parameter,
                                &mut asset.values,
                            ) {
                                ui.ctx().data_mut(|data| data.insert_temp(id, result.err()));
                            }
                        }
                    });
                    if let Some(error) = ui
                        .ctx()
                        .data(|data| data.get_temp::<Option<String>>(id))
                        .flatten()
                    {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                }
                self.card_values(ui, asset, &loaded);
            }
            Some(Err(error)) => {
                self.load_error(ui, asset.graph, error.clone());
            }
            None => {
                ui.small("Reading projectile properties…");
                self.request(ui, asset.graph);
            }
        }
    }
    pub fn sync(&mut self, packages: &Path, source: Option<&Arc<entity::catalog::Catalog>>) {
        let same_source = match (&self.source, source) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if self.packages != packages || !same_source {
            self.packages = packages.to_owned();
            self.source = source.cloned();
            self.graphs.clear();
            self.pending = None;
            self.changes.clear();
            self.text.clear();
            self.hud = hud::HudStatuses::default();
        }
        self.hud.poll();
        if let Some((tag, receiver)) = &self.pending {
            let result = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The property reader stopped before finishing.".into()))
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                self.graphs.insert(*tag, result.map(Arc::new));
                self.pending = None;
            }
        }
    }

    /// Whether a property or HUD status read is still going.
    pub fn busy(&self) -> bool {
        self.pending.is_some() || self.hud.busy()
    }

    pub fn remember(&mut self, tag: u32, graph: Arc<PrivatePerkRuntimeGraph>) {
        self.graphs.insert(tag, Ok(graph));
    }

    /// The property change lines for these values on a loaded graph, read once per pair.
    fn change_lines(
        &mut self,
        graph: Arc<PrivatePerkRuntimeGraph>,
        values: &[WeaponRuntimeValueOverride],
        named: bool,
    ) -> &Result<Vec<String>, String> {
        let position = self.changes.iter().position(|cached| {
            Arc::ptr_eq(&cached.graph, &graph) && cached.values == values && cached.named == named
        });
        let position = position.unwrap_or_else(|| {
            if self.changes.len() >= CHANGES_KEPT {
                self.changes.remove(0);
            }
            let lines = super::parameters::unlisted_changes(&graph, values, named);
            self.changes.push(Changes {
                graph,
                values: values.to_vec(),
                named,
                lines,
            });
            self.changes.len() - 1
        });
        &self.changes[position].lines
    }

    /// Proven values and the More Properties fold. Returns whether the fold is open.
    fn card_values(
        &mut self,
        ui: &mut egui::Ui,
        asset: &mut Asset,
        graph: &PrivatePerkRuntimeGraph,
    ) -> bool {
        ui.push_id(("asset-values", asset.graph), |ui| {
            let id = ui.id().with("error");
            let text = self.text.entry(asset.graph).or_default();
            let (edited, open) = parameters::draw_card_values(ui, graph, &mut asset.values, text);
            if let Some(result) = edited {
                ui.ctx().data_mut(|data| data.insert_temp(id, result.err()));
            }
            if let Some(error) = ui
                .ctx()
                .data(|data| data.get_temp::<Option<String>>(id))
                .flatten()
            {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            open
        })
        .inner
    }

    /// The attachment's length, a tile at `width` beside the action's Lifetime. Nothing while
    /// the asset has no length or its properties are still being read.
    pub fn length_tile(&mut self, ui: &mut egui::Ui, width: f32, asset: &mut Asset) {
        if matches!(asset.graph, 0 | u32::MAX) {
            return;
        }
        let graph = match self.graphs.get(&asset.graph) {
            Some(Ok(graph)) => graph.clone(),
            Some(Err(_)) => return,
            None => {
                self.request(ui, asset.graph);
                return;
            }
        };
        let lengths = parameters::effect_length::discover(&graph);
        if lengths.is_empty() {
            return;
        }
        let id = ui.id().with(("attachment-length-error", asset.graph));
        for length in &lengths {
            if let Some(result) =
                parameters::effect_length::draw(ui, width, &graph, length, &mut asset.values)
            {
                ui.ctx().data_mut(|data| data.insert_temp(id, result.err()));
            }
        }
        if let Some(error) = ui
            .ctx()
            .data(|data| data.get_temp::<Option<String>>(id))
            .flatten()
        {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }

    /// Rows added to an attachment's modifiers, under its table: one more record each, with
    /// the table's columns, then Add Row. Only an attachment with modifier records takes rows.
    fn added_rows(
        &mut self,
        ui: &mut egui::Ui,
        asset: &mut Asset,
        graph: &PrivatePerkRuntimeGraph,
    ) {
        use sundial::package_authoring::{
            runtime::modifiers,
            sandbox_perk::{entity::modifiers as records, program::ModifierRow},
        };
        if graph
            .graphs
            .iter()
            .all(|(_, graph)| records::discover(graph).is_empty())
        {
            return;
        }
        let components =
            modifiers::field_meaning(modifiers::SETTINGS_SCHEMA, modifiers::COMPONENT_OFFSET)
                .map_or(&[][..], |meaning| meaning.choices);
        let abilities = modifiers::field_meaning(modifiers::SETTINGS_SCHEMA, 0x48)
            .map_or(&[][..], |meaning| meaning.choices);
        let operations: &[(i64, &str)] = &[
            (modifiers::OPERATION_ADD, "Add"),
            (modifiers::OPERATION_MULTIPLY, "Multiply"),
        ];
        let mut remove = None;
        if !asset.rows.is_empty() {
            ui.add_space(4.0);
            ui.strong("Added Rows");
        }
        for (index, row) in asset.rows.iter_mut().enumerate() {
            ui.push_id(("added-row", asset.graph, index), |ui| {
                ui.horizontal_wrapped(|ui| {
                    if let Some(component) =
                        choice(ui, "component", components, i64::from(row.component))
                    {
                        row.component = component as u8;
                        row.input = modifiers::input_choices(component)
                            .first()
                            .map_or(0, |(input, _)| *input as i16);
                    }
                    if i64::from(row.component) == modifiers::ABILITIES_COMPONENT
                        && let Some(ability) =
                            choice(ui, "ability", abilities, i64::from(row.ability))
                    {
                        row.ability = ability as i16;
                    }
                    let inputs = modifiers::input_choices(i64::from(row.component));
                    if inputs.is_empty() {
                        let mut input = i64::from(row.input);
                        if ui
                            .add(egui::DragValue::new(&mut input).range(0..=255))
                            .changed()
                        {
                            row.input = input as i16;
                        }
                    } else if let Some(input) = choice(ui, "input", inputs, i64::from(row.input)) {
                        row.input = input as i16;
                    }
                    if let Some(operation) =
                        choice(ui, "operation", operations, i64::from(row.operation))
                    {
                        row.operation = operation as u8;
                    }
                    let mut amount = row.amount();
                    if ui
                        .add(
                            egui::DragValue::new(&mut amount)
                                .speed(0.05)
                                .max_decimals(3),
                        )
                        .changed()
                    {
                        row.set_amount(amount);
                    }
                    if ui.small_button("Remove").clicked() {
                        remove = Some(index);
                    }
                });
            });
        }
        if let Some(index) = remove {
            asset.rows.remove(index);
        }
        if ui.button("Add Row").clicked() {
            asset.rows.push(ModifierRow::neutral());
        }
    }

    /// The asset's named values as rows, then a summary of the changes those rows do not show.
    /// An untouched asset reads quietly, since its rows are extra.
    pub fn values(&mut self, ui: &mut egui::Ui, asset: &mut Asset) {
        if matches!(asset.graph, 0 | u32::MAX) {
            return;
        }
        let untouched = asset.values.is_empty();
        let graph = match self.graphs.get(&asset.graph) {
            Some(Ok(graph)) => graph.clone(),
            Some(Err(_)) if untouched => return,
            Some(Err(error)) => {
                self.load_error(ui, asset.graph, error.clone());
                return;
            }
            None => {
                if !untouched {
                    ui.small("Reading properties…");
                }
                self.request(ui, asset.graph);
                return;
            }
        };
        let named = self.card_values(ui, asset, &graph);
        self.added_rows(ui, asset, &graph);
        if asset.values.is_empty() {
            return;
        }
        match self.change_lines(graph, &asset.values, named) {
            Ok(lines) => {
                let (technical, named): (Vec<&str>, Vec<&str>) =
                    lines.iter().map(String::as_str).partition(|line| {
                        line.starts_with("Type 0x") || line.starts_with("Native Bytes:")
                    });
                if !named.is_empty() {
                    ui.add(egui::Label::new(named.join(" · ")).wrap());
                }
                if !technical.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.small("Advanced Properties Modified");
                        sundial::investment::draw_authoring_info_icon(ui, technical.join("\n"));
                    });
                }
            }
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_reopens_a_failed_load_and_keeps_other_cached_results() {
        let ctx = egui::Context::default();
        let mut properties = Properties::default();
        properties
            .graphs
            .insert(10, Err("Temporary read failure".into()));
        properties.graphs.insert(20, Err("Separate failure".into()));
        let mut asset = Asset {
            graph: 10,
            ..Default::default()
        };
        let frame = |properties: &mut Properties, asset: &mut Asset, events| {
            ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| properties.movement(ui, asset));
                },
            )
        };
        let output = frame(&mut properties, &mut asset, vec![]);
        let position = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Retry" => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap();
        for pressed in [true, false] {
            frame(
                &mut properties,
                &mut asset,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        assert!(!properties.graphs.contains_key(&10));
        assert!(properties.graphs.contains_key(&20));
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(Ok(super::super::parameters::tests::fixture()))
            .unwrap();
        properties.pending = Some((10, receiver));
        properties.sync(Path::new(""), None);
        assert!(properties.graphs[&10].is_ok());
        assert!(properties.graphs[&20].is_err());
    }
}
