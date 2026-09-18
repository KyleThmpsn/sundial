//! Resolve property summaries from package values, including when a saved perk is reopened.
use super::*;
use sundial::package_authoring::sandbox_perk::program::Asset;

/// One Properties entry point for action and condition settings.
pub(super) struct Panel {
    id: egui::Id,
    open: bool,
}

impl Panel {
    pub fn new(ui: &egui::Ui, scope: impl std::hash::Hash) -> Self {
        let id = ui.make_persistent_id(("properties", scope));
        Self {
            id,
            open: ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false)),
        }
    }

    pub fn button(&mut self, ui: &mut egui::Ui) {
        if ui
            .add(egui::Button::new("Properties…").selected(self.open))
            .clicked()
        {
            self.open = !self.open;
            ui.data_mut(|data| data.insert_temp(self.id, self.open));
        }
    }

    pub fn show(self, ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
        if self.open {
            ui.separator();
            ui.push_id(self.id, contents);
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
    let width = controls::CELL_LABEL_WIDTH.min((ui.available_width() - 100.0).max(100.0));
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(width, ui.spacing().interact_size.y),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.set_min_width(width);
                if custom_label(ui) {
                    return;
                }
                let text = match emphasis {
                    Emphasis::Plain => egui::RichText::new(label),
                    Emphasis::Weak => egui::RichText::new(label).weak(),
                };
                let response = ui.add(egui::Label::new(text).halign(egui::Align::Max).truncate());
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
    })
    .inner
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

    pub(in crate::app::custom_perks) fn property_row_with<R>(
        ui: &mut egui::Ui,
        label: &str,
        hint: &str,
        custom_label: impl FnOnce(&mut egui::Ui) -> bool,
        value: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        row_with(ui, label, hint, Emphasis::Plain, custom_label, value)
    }
}

/// Component editing keeps the existing checked asset editor and draft flow.
pub(super) fn edit_object(ui: &mut egui::Ui, asset: &Asset) -> bool {
    ui.add_enabled(asset.graph != 0, egui::Button::new("Edit Components…"))
        .on_disabled_hover_text("Choose an object to edit its components.")
        .clicked()
}

#[derive(Default)]
pub(super) struct Properties {
    packages: PathBuf,
    source: Option<Arc<projectile::catalog::Catalog>>,
    graphs: BTreeMap<u32, Result<Arc<PrivatePerkRuntimeGraph>, String>>,
    pending: Option<(u32, Receiver<Result<PrivatePerkRuntimeGraph, String>>)>,
}

impl Properties {
    pub fn sync(&mut self, packages: &Path, source: Option<&Arc<projectile::catalog::Catalog>>) {
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
        }
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

    pub fn remember(&mut self, tag: u32, graph: Arc<PrivatePerkRuntimeGraph>) {
        self.graphs.insert(tag, Ok(graph));
    }

    pub fn draw(&mut self, ui: &mut egui::Ui, asset: &Asset) {
        if asset.values.is_empty() {
            return;
        }
        match self.graphs.get(&asset.graph) {
            Some(Ok(graph)) => match super::super::editor::property_changes(graph, &asset.values) {
                Ok(lines) => {
                    let (technical, named): (Vec<_>, Vec<_>) =
                        lines.into_iter().partition(|line| {
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
            },
            Some(Err(error)) => {
                ui.small("Property Values Unavailable").on_hover_text(error);
            }
            None => {
                ui.small("Reading Properties…");
                ui.ctx().request_repaint_after(Duration::from_millis(100));
                if self.pending.is_none() && self.packages.is_dir() {
                    let (sender, receiver) = std::sync::mpsc::channel();
                    let packages = self.packages.clone();
                    let tag = asset.graph;
                    let ctx = ui.ctx().clone();
                    std::thread::spawn(move || {
                        let _ = sender
                            .send(super::super::editor::load_entity_parameters(&packages, tag));
                        ctx.request_repaint();
                    });
                    self.pending = Some((tag, receiver));
                }
            }
        }
    }
}
