//! Outside tools each source needs, checked before an import starts. Missing required tools
//! block importing and show under the header with where to get them. The Import Tools window
//! lists every tool of the source with the file in use.
use super::*;
use parhelion_import::tools::{self, Status, Tool};

/// The shown source's tools, checked on a worker. Loading a library runs its startup code and
/// the decompiler check hashes its files, so neither runs on the UI thread.
pub(super) struct Checked {
    source: Source,
    native: PathBuf,
    /// `None` while the worker runs.
    rows: Option<Vec<(Tool, Status)>>,
    receiver: Option<Receiver<Vec<(Tool, Status)>>>,
}

impl Source {
    fn requires(self, tool: Tool) -> bool {
        // Browser weapon imports translate source firing audio and require its decoder.
        self == Source::HaloReach || tool.required()
    }

    fn tools(self) -> &'static [Tool] {
        match self {
            Source::D2Modern => tools::MONUMENT,
            Source::D1RiseOfIron => tools::RISE_OF_IRON,
            Source::HaloReach => tools::REACH,
            Source::Marathon => tools::MARATHON,
        }
    }
}

impl Importer {
    /// Required tools of the shown source that were not found.
    fn missing_tools(&self) -> Vec<Tool> {
        self.tools
            .as_ref()
            .and_then(|checked| checked.rows.as_ref())
            .map_or_else(Vec::new, |rows| {
                rows.iter()
                    .filter(|(tool, status)| {
                        self.source.requires(*tool) && *status == Status::Missing
                    })
                    .map(|(tool, _)| *tool)
                    .collect()
            })
    }

    /// Whether the shown source's tools are still being checked.
    pub(super) fn tools_checking(&self) -> bool {
        self.tools
            .as_ref()
            .is_none_or(|checked| checked.rows.is_none())
    }

    pub(super) fn tools_ready(&self) -> bool {
        self.fetching.is_none() && !self.tools_checking() && self.missing_tools().is_empty()
    }

    fn choose_tool(&mut self, tool: Tool) {
        let Some(path) = rfd::FileDialog::new()
            .set_title(tool.name())
            .set_file_name(tool.file())
            .pick_file()
        else {
            return;
        };
        if let Some(slot) = self.settings.tool_mut(tool) {
            *slot = Some(path);
        }
        self.tools = None;
        self.icons = icons::Icons::default();
        if let Err(error) = self.save() {
            self.notice = error;
        }
    }

    fn clear_tool(&mut self, tool: Tool) {
        if let Some(slot) = self.settings.tool_mut(tool) {
            *slot = None;
        }
        self.tools = None;
        self.icons = icons::Icons::default();
        if let Err(error) = self.save() {
            self.notice = error;
        }
    }

    /// Downloads the decompiler, or unpacks a release archive the user picked, on a worker.
    fn fetch_decompiler(&mut self, ctx: &egui::Context, archive: Option<PathBuf>) {
        let (sender, receiver) = mpsc::channel();
        self.fetching = Some(receiver);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = tools::fetch_decompiler(archive.as_deref())
                .map(|_| ())
                .map_err(|error| format!("{error:#}"));
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }
}

impl PackageAuthoringApp {
    /// Hands the chosen paths to the importer and checks the shown source's tools when the
    /// source, the Shadowkeep install or a chosen path changed.
    pub(super) fn check_importer_tools(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.importer.fetching {
            match receiver.try_recv() {
                Ok(result) => {
                    self.importer.fetching = None;
                    self.importer.tools = None;
                    if let Err(error) = result {
                        self.importer.notice = error;
                    }
                }
                Err(TryRecvError::Disconnected) => self.importer.fetching = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some(checked) = &mut self.importer.tools
            && let Some(receiver) = &checked.receiver
        {
            match receiver.try_recv() {
                Ok(rows) => {
                    checked.rows = Some(rows);
                    checked.receiver = None;
                }
                Err(TryRecvError::Disconnected) => checked.receiver = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        let source = self.importer.source;
        if self
            .importer
            .tools
            .as_ref()
            .is_some_and(|checked| checked.source == source && checked.native == self.packages)
        {
            return;
        }
        let native = (!self.packages.as_os_str().is_empty()).then(|| self.packages.clone());
        tools::configure(self.importer.settings.tools(native));
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = sender.send(tools::check_all(source.tools()));
            ctx.request_repaint();
        });
        // A superseded worker's result is dropped with its receiver.
        self.importer.tools = Some(Checked {
            source,
            native: self.packages.clone(),
            rows: None,
            receiver: Some(receiver),
        });
    }

    /// One line per missing required tool, with where to get it and a way to pick its file.
    pub(super) fn draw_missing_tools(&mut self, ui: &mut egui::Ui, idle: bool) {
        let missing = self.importer.missing_tools();
        if missing.is_empty() {
            return;
        }
        let fetching = self.importer.fetching.is_some();
        for tool in missing {
            ui.horizontal_wrapped(|ui| {
                let warn = ui.visuals().warn_fg_color;
                // Painted, so a screen reader announces the name and not the glyph.
                let glyph = egui::WidgetText::from(
                    style::icon(ui, egui_phosphor::regular::WARNING).color(warn),
                )
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Body,
                );
                let (rect, _) = ui.allocate_exact_size(glyph.size(), egui::Sense::hover());
                ui.painter().galley(rect.min, glyph, warn);
                ui.label(
                    egui::RichText::new(format!("{} Not Found", tool.name()))
                        .color(warn)
                        .strong(),
                );
                ui.label(egui::RichText::new(tool.purpose()).weak());
                ui.add_enabled_ui(idle && !fetching, |ui| {
                    self.draw_tool_actions(ui, tool, true);
                });
                if fetching && tool == Tool::Decompiler {
                    ui.spinner();
                }
            });
        }
    }

    /// Get link and file choice for one tool. A found tool chosen by hand can be cleared.
    fn draw_tool_actions(&mut self, ui: &mut egui::Ui, tool: Tool, missing: bool) {
        if missing {
            match (tool.url(), tool.origin()) {
                (Some(url), _) => {
                    ui.hyperlink_to(format!("Get {}", tool.name()), url);
                }
                (None, Some(origin)) => {
                    style::hint(ui, origin).on_hover_text(tool.file());
                }
                (None, None) => {
                    ui.weak(tool.file());
                }
            }
        }
        if tool == Tool::Decompiler {
            if missing && ui.button("Download").clicked() {
                self.importer.fetch_decompiler(ui.ctx(), None);
            }
            if missing
                && ui
                    .button("Choose Archive…")
                    .on_hover_text(tool.file())
                    .clicked()
                && let Some(archive) = rfd::FileDialog::new()
                    .set_title(tool.name())
                    .add_filter("Zip Archive", &["zip"])
                    .pick_file()
            {
                self.importer.fetch_decompiler(ui.ctx(), Some(archive));
            }
            return;
        }
        if ui
            .button("Choose File…")
            .on_hover_text(tool.file())
            .clicked()
        {
            self.importer.choose_tool(tool);
        }
        let chosen = self
            .importer
            .settings
            .tool_mut(tool)
            .is_some_and(|slot| slot.is_some());
        if chosen && ui.button("Clear").clicked() {
            self.importer.clear_tool(tool);
        }
    }

    /// Opens the Import Tools window once the shown source's tools are checked. The label is muted
    /// red while any of them is missing.
    pub(super) fn draw_importer_tools_button(&mut self, ui: &mut egui::Ui) {
        let checked = !self.importer.tools_checking();
        let missing = self
            .importer
            .tools
            .as_ref()
            .and_then(|checked| checked.rows.as_ref())
            .is_some_and(|rows| rows.iter().any(|(_, status)| *status == Status::Missing));
        let mut label = egui::RichText::new("Import Tools");
        if missing {
            label = label.color(style::muted_error(ui.visuals()));
        }
        let button = ui
            .add_enabled(
                checked,
                egui::Button::new(label).selected(self.importer.tools_open),
            )
            .on_disabled_hover_text("Checking tools.");
        if button.clicked() {
            self.importer.tools_open = !self.importer.tools_open;
        }
    }

    /// Every tool of the shown source: what it is for, the file in use or that it was not found,
    /// and its actions.
    pub(super) fn draw_importer_tools_window(&mut self, ctx: &egui::Context) {
        if !self.importer.tools_open {
            return;
        }
        let rows = self
            .importer
            .tools
            .as_ref()
            .and_then(|checked| checked.rows.clone());
        let mut open = true;
        egui::Window::new("Import Tools")
            .id(egui::Id::new("parhelion-importer-tools"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                workbench_style(ui);
                let Some(rows) = rows else {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Checking tools.");
                    });
                    return;
                };
                egui::Grid::new("importer-tools")
                    .num_columns(4)
                    .spacing([16.0, 8.0])
                    .show(ui, |ui| {
                        if self.importer.source == Source::D1RiseOfIron {
                            ui.strong("Xbox Textures");
                            ui.weak("Textures");
                            ui.weak("Built In");
                            sundial::ui::info_link(ui,
                                "Texture conversion is built in using AMD AddrLib. No XG DLL or setup is needed.",
                                "AMD AddrLib Source", parhelion_import::d1_roi::texture::SOURCE_URL,
                                Some(parhelion_import::d1_roi::texture::LICENSE));
                            ui.end_row();
                        }
                        for (tool, status) in rows {
                            self.draw_tool_row(ui, tool, &status);
                            ui.end_row();
                        }
                    });
                ui.add_space(4.0);
                if ui.button("Check Again").clicked() {
                    self.importer.tools = None;
                }
            });
        if !open {
            self.importer.tools_open = false;
        }
    }

    fn draw_tool_row(&mut self, ui: &mut egui::Ui, tool: Tool, status: &Status) {
        ui.add(egui::Label::new(egui::RichText::new(tool.name()).strong()).extend());
        ui.add(egui::Label::new(egui::RichText::new(tool.purpose()).weak()).extend());
        // A fixed column, so a long path neither widens the window nor squeezes the names.
        let size = egui::vec2(320.0, ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(
            size,
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_width(size.x);
                match status {
                    Status::Found(path) => {
                        let text = shown_path(path);
                        ui.add(
                            egui::Label::new(egui::RichText::new(&text).weak())
                                .truncate()
                                .show_tooltip_when_elided(false),
                        )
                        .on_hover_text(&text);
                    }
                    Status::Missing => {
                        let color = if self.importer.source.requires(tool) {
                            ui.visuals().warn_fg_color
                        } else {
                            ui.visuals().weak_text_color()
                        };
                        ui.colored_label(color, "Not Found");
                    }
                }
            },
        );
        ui.horizontal(|ui| {
            ui.add_enabled_ui(
                self.importer.fetching.is_none() && !self.importer.busy(),
                |ui| {
                    self.draw_tool_actions(ui, tool, *status == Status::Missing);
                },
            );
            if self.importer.fetching.is_some() && tool == Tool::Decompiler {
                ui.spinner();
            }
        });
    }
}

/// `path` as Explorer shows it, without the `\\?\` prefix a canonical Windows path carries.
fn shown_path(path: &Path) -> String {
    let text = path.display().to_string();
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{share}")
    } else {
        text.strip_prefix(r"\\?\")
            .map_or(text.clone(), str::to_owned)
    }
}
