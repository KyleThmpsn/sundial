//! The HUD name and icon an attachment's Status Icon shows. A name of the author's own makes the
//! build give the status a hash, a string and a HUD status table row of its own. The icon is a
//! stock status's, by default the one the graph already shows, or an image of the author's own
//! drawn in that stock icon's place.
use super::*;
use crate::item::StockStatus;
use std::sync::mpsc::TryRecvError;
use sundial::package_authoring::sandbox_perk::program::HudStatus;

/// The Status Icon component's class.
const STATUS_ICON: u32 = 0x8080_4211;
/// The square an imported HUD image is fitted to, past the largest stock HUD status texture
/// (75 pixels). The build fits it again to each texture it replaces.
const IMAGE_EDGE: u32 = 128;

type Read = Result<(Option<Vec<StockStatus>>, Vec<u32>), String>;
type Import = Option<Result<String, String>>;

/// The stock HUD statuses with names, and the statuses each asset's graph shows, read in the
/// background. The names come from every string bank, so they are read once.
#[derive(Default)]
pub(super) struct HudStatuses {
    stock: Option<Arc<Vec<StockStatus>>>,
    shown: BTreeMap<u32, Result<Vec<u32>, String>>,
    pending: Option<(u32, Receiver<Read>)>,
    query: String,
    /// An image being imported, for the asset with this graph.
    import: Option<(u32, Receiver<Import>)>,
    import_error: Option<String>,
    /// The preview of the last image drawn, by a hash of its PNG.
    preview: Option<(u64, egui::TextureHandle)>,
}

impl HudStatuses {
    pub(super) fn poll(&mut self) {
        let Some((graph, receiver)) = &self.pending else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Disconnected) => {
                Err("The HUD status reader stopped before finishing.".into())
            }
            Err(TryRecvError::Empty) => return,
        };
        let graph = *graph;
        self.pending = None;
        let shown = result.map(|(stock, shown)| {
            if let Some(stock) = stock {
                self.stock = Some(Arc::new(stock));
            }
            shown
        });
        self.shown.insert(graph, shown);
    }

    fn request(&mut self, ui: &egui::Ui, packages: &Path, graph: u32) {
        if self.pending.is_none() && packages.is_dir() && !self.shown.contains_key(&graph) {
            let (sender, receiver) = std::sync::mpsc::channel();
            let packages = packages.to_owned();
            let stock = self.stock.is_none();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let read = || -> Read {
                    let manager = open_shadowkeep_package_manager(&packages)?;
                    let stock = stock
                        .then(|| crate::item::stock_statuses(&manager))
                        .transpose()
                        .map_err(|error| error.to_string())?;
                    let shown = crate::item::shown_statuses(&manager, graph)
                        .map_err(|error| error.to_string())?;
                    Ok((stock, shown))
                };
                let _ = sender.send(read());
                ctx.request_repaint();
            });
            self.pending = Some((graph, receiver));
        }
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }

    fn start_import(&mut self, ui: &egui::Ui, graph: u32) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let ctx = ui.ctx().clone();
        std::thread::spawn(move || {
            let result = rfd::FileDialog::new()
                .set_title("Import HUD Icon")
                .add_filter("Image", &["png", "jpg", "jpeg"])
                .pick_file()
                .map(|path| import_image(&path));
            let _ = sender.send(result);
            ctx.request_repaint();
        });
        self.import = Some((graph, receiver));
        self.import_error = None;
    }

    /// The finished import for the asset with `graph`, once it arrives.
    fn finished_import(&mut self, ui: &egui::Ui, graph: u32) -> Option<String> {
        let (from, receiver) = self.import.as_ref()?;
        if *from != graph {
            return None;
        }
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Disconnected) => {
                Some(Err("The image import stopped before finishing.".into()))
            }
            Err(TryRecvError::Empty) => {
                ui.ctx().request_repaint_after(Duration::from_millis(100));
                return None;
            }
        };
        self.import = None;
        match result? {
            Ok(image) => Some(image),
            Err(error) => {
                self.import_error = Some(error);
                None
            }
        }
    }

    fn preview(&mut self, ctx: &egui::Context, image: &str) -> Option<egui::TextureHandle> {
        use base64::Engine as _;
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        image.hash(&mut hasher);
        let key = hasher.finish();
        if let Some((cached, texture)) = &self.preview
            && *cached == key
        {
            return Some(texture.clone());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(image)
            .ok()?;
        let rgba = crate::image_import::decode_png(&bytes).ok()?;
        let texture = ctx.load_texture(
            "hud-status-image",
            egui::ColorImage::from_rgba_unmultiplied(
                [rgba.width() as usize, rgba.height() as usize],
                rgba.as_raw(),
            ),
            egui::TextureOptions::LINEAR,
        );
        self.preview = Some((key, texture.clone()));
        Some(texture)
    }
}

/// An image file as the base64 PNG a HUD status keeps, fitted to [`IMAGE_EDGE`] with its
/// transparency.
fn import_image(path: &Path) -> Result<String, String> {
    use base64::Engine as _;
    let source = crate::image_import::decode_source(&crate::image_import::read_path(path)?)?;
    let icon = crate::image_import::fit(&source, IMAGE_EDGE, IMAGE_EDGE);
    let mut png = std::io::Cursor::new(Vec::new());
    icon.write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| format!("Could not encode image: {error}"))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(png.into_inner()))
}

impl Properties {
    /// The HUD name and icon of an attachment whose graph has a Status Icon. Nothing for one
    /// without, or while its properties are still being read.
    pub fn hud_status(&mut self, ui: &mut egui::Ui, asset: &mut Asset) {
        if matches!(asset.graph, 0 | u32::MAX) {
            return;
        }
        let Some(Ok(graph)) = self.graphs.get(&asset.graph) else {
            return;
        };
        let has_icon = graph.graphs.iter().any(|(_, graph)| {
            graph
                .resources
                .iter()
                .any(|resource| resource.concrete_class == STATUS_ICON)
        });
        if !has_icon && asset.hud_status.is_none() {
            return;
        }
        let shown = match self.hud.shown.get(&asset.graph) {
            Some(Ok(shown)) => shown.clone(),
            Some(Err(error)) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
                return;
            }
            None => {
                let packages = self.packages.clone();
                self.hud.request(ui, &packages, asset.graph);
                ui.small("Reading HUD status…");
                return;
            }
        };
        if let Some(image) = self.hud.finished_import(ui, asset.graph)
            && let Some(status) = &mut asset.hud_status
        {
            status.image = Some(image);
        }
        let stock = self.hud.stock.clone().unwrap_or_default();
        let name_of = |hash: u32| {
            stock
                .iter()
                .find(|status| status.hash == hash)
                .map(|status| status.name.clone())
        };
        let stock_name = match shown.as_slice() {
            [hash] => name_of(*hash),
            _ => None,
        };
        let preview = asset
            .hud_status
            .as_ref()
            .and_then(|status| status.image.as_deref())
            .and_then(|image| self.hud.preview(ui.ctx(), image));
        let importing = self.hud.import.is_some();
        let hud = &mut self.hud;
        let import = crate::app::style::tiles(ui, |ui, width| {
            let mut name = asset
                .hud_status
                .as_ref()
                .map(|status| status.name.clone())
                .unwrap_or_default();
            let (edited, reset) = crate::app::style::tile(
                ui,
                width,
                "hud-name",
                "HUD Name",
                "Shown on the HUD",
                asset.hud_status.is_some(),
                |ui| {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut name)
                            .hint_text(stock_name.as_deref().unwrap_or_default())
                            .desired_width(f32::INFINITY),
                    );
                    crate::app::pickers::name_response(ui, &response, "HUD Name");
                    response.changed()
                },
            );
            if reset {
                asset.hud_status = None;
            } else if edited {
                let kept = asset.hud_status.take();
                asset.hud_status = (!name.trim().is_empty()).then(|| HudStatus {
                    name,
                    icon: kept.as_ref().and_then(|status| status.icon),
                    image: kept.and_then(|status| status.image),
                });
            }
            let named = asset.hud_status.is_some();
            let current = asset.hud_status.as_ref().and_then(|status| status.icon);
            let selected = match current {
                Some(hash) => name_of(hash).unwrap_or_else(|| format!("0x{hash:08X}")),
                None if stock_name.is_some() => "Same as Stock".to_owned(),
                None => "Choose an Icon".to_owned(),
            };
            let (choice, reset) = crate::app::style::tile(
                ui,
                width,
                "hud-icon",
                "HUD Icon",
                "Icon on the HUD",
                current.is_some(),
                |ui| {
                    let picked = ui.add_enabled_ui(named, |ui| {
                        let mut choice = None;
                        egui::ComboBox::from_id_salt("hud-icon")
                            .selected_text(selected)
                            .width(ui.available_width())
                            .show_ui(ui, |ui| {
                                if crate::app::pickers::wants_filter(stock.len()) {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut hud.query)
                                            .hint_text(format!(
                                                "{} Filter",
                                                egui_phosphor::regular::MAGNIFYING_GLASS
                                            ))
                                            .desired_width(f32::INFINITY),
                                    );
                                }
                                egui::ScrollArea::vertical()
                                    .max_height(320.0)
                                    .show(ui, |ui| {
                                        if stock_name.is_some()
                                            && ui
                                                .selectable_label(
                                                    current.is_none(),
                                                    "Same as Stock",
                                                )
                                                .clicked()
                                        {
                                            choice = Some(None);
                                        }
                                        for status in stock.iter().filter(|status| {
                                            crate::app::pickers::matches(&hud.query, &status.name)
                                        }) {
                                            if ui
                                                .selectable_label(
                                                    current == Some(status.hash),
                                                    &status.name,
                                                )
                                                .clicked()
                                            {
                                                choice = Some(Some(status.hash));
                                            }
                                        }
                                    });
                            });
                        crate::app::pickers::name_combo(ui, "hud-icon", "HUD Icon");
                        choice
                    });
                    picked.response.on_disabled_hover_text("Name it first");
                    picked.inner
                },
            );
            if let Some(status) = &mut asset.hud_status {
                if reset {
                    status.icon = None;
                } else if let Some(icon) = choice {
                    status.icon = icon;
                }
            }
            let has_image = asset
                .hud_status
                .as_ref()
                .is_some_and(|status| status.image.is_some());
            let (import, reset) = crate::app::style::tile(
                ui,
                width,
                "hud-image",
                "HUD Image",
                "Your own icon",
                has_image,
                |ui| {
                    let row = ui.add_enabled_ui(named && !importing, |ui| {
                        ui.horizontal(|ui| {
                            if let Some(texture) = &preview {
                                ui.add(
                                    egui::Image::new(texture)
                                        .fit_to_exact_size(egui::vec2(24.0, 24.0)),
                                );
                            }
                            let label = if has_image { "Replace…" } else { "Import…" };
                            let clicked = ui.button(label).clicked();
                            if importing {
                                ui.spinner();
                            }
                            clicked
                        })
                        .inner
                    });
                    if !named {
                        row.response.on_disabled_hover_text("Name it first");
                    }
                    row.inner
                },
            );
            if reset && let Some(status) = &mut asset.hud_status {
                status.image = None;
            }
            import
        });
        if import {
            self.hud.start_import(ui, asset.graph);
        }
        if let Some(error) = &self.hud.import_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }
}
