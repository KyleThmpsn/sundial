//! File selection, decoding and model rendering stay off the UI thread; only successful imports
//! replace the draft.

use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
};

use sundial::ui::model_preview::{
    Appearance,
    icon::{self as render, Frame, Lighting, Pose},
};

use super::{ImportedIcon, WeaponIconEdit};

type ImportResult = Result<Option<ImportedIcon>, String>;

/// The model a weapon recipe shows: its arrangement with the dyes its shader and plugs give it,
/// as the appearance preview draws it. The editor can render it as the icon's artwork.
pub(crate) struct Model {
    pub(crate) packages: PathBuf,
    pub(crate) appearance: Appearance,
}

/// The edge of the render the artwork is scaled down from.
const RENDER_EDGE: usize = 768;
/// The camera imported weapons' generated icons use, framed to keep the whole weapon in view.
const RENDER_POSE: Pose = Pose {
    yaw: 2.5,
    pitch: 0.25,
    roll: 0.35,
};
const RENDER_ZOOM: f32 = 1.3;

/// Renders the model and scales it to a recipe's icon. Blocking work for a background thread.
fn render_model(model: &Model) -> Result<ImportedIcon, String> {
    let art = render::appearance(
        &model.packages,
        &model.appearance,
        RENDER_POSE,
        Frame {
            zoom: RENDER_ZOOM,
            offset: [0.0; 2],
        },
        Lighting::default(),
        RENDER_EDGE,
    )?;
    let edge = u32::try_from(art.size).map_err(|_| "The model render is too large.".to_owned())?;
    let image = image::RgbaImage::from_raw(edge, edge, art.rgba)
        .ok_or_else(|| "The model render has the wrong size.".to_owned())?;
    ImportedIcon::from_render(image)
}

#[derive(Default)]
pub(super) struct ImageImport {
    pending: Option<Receiver<ImportResult>>,
    /// What the pending work is, shown beside its spinner.
    activity: &'static str,
    error: Option<String>,
    /// The recipe's model, when it has one the editor can render.
    pub(super) model: Option<Model>,
}

impl ImageImport {
    pub(super) fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(super) fn poll(&mut self, draft: &mut WeaponIconEdit) {
        let Some(receiver) = &self.pending else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("Image import stopped unexpectedly.".to_owned()),
        };
        self.pending = None;
        self.accept(result, draft);
    }

    fn accept(&mut self, result: ImportResult, draft: &mut WeaponIconEdit) {
        match result {
            Ok(Some(image)) => {
                draft.imported_image = Some(image);
                self.error = None;
            }
            Ok(None) => {}
            Err(error) => self.error = Some(error),
        }
    }

    pub(super) fn draw(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut WeaponIconEdit,
        enabled: bool,
    ) -> bool {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    enabled && !self.is_pending(),
                    egui::Button::new("Import Image…"),
                )
                .clicked()
            {
                self.start(ui.ctx().clone());
            }
            if self.model.is_some()
                && ui
                    .add_enabled(
                        enabled && !self.is_pending(),
                        egui::Button::new("Render from Model"),
                    )
                    .on_hover_text(
                        "Draws the weapon's model with its current ornament and shader, and uses the picture as the icon.",
                    )
                    .clicked()
            {
                self.start_render(ui.ctx().clone());
            }
            if draft.imported_image.is_some()
                && ui
                    .add_enabled(!self.is_pending(), egui::Button::new("Use Donor Artwork"))
                    .clicked()
            {
                draft.imported_image = None;
                self.error = None;
                changed = true;
            }
            if self.is_pending() {
                ui.spinner();
                ui.label(self.activity);
            }
        });
        ui.label(egui::RichText::new(if draft.imported_image.is_some() {
            "Imported image · saved in this recipe. Proportions and transparency are preserved."
        } else {
            "PNG or JPEG · up to 16 MiB / 4096×4096. Fitted without cropping. Transparent PNG recommended."
        }).color(super::secondary_text_color(ui)));
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        changed
    }

    fn start(&mut self, context: egui::Context) {
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(receiver);
        self.activity = "Importing…";
        self.error = None;
        let spawned = std::thread::Builder::new()
            .name("weapon-icon-import".to_owned())
            .spawn(move || {
                let result = rfd::FileDialog::new()
                    .set_title("Import Primary Icon Artwork")
                    .add_filter("PNG / JPEG images", &["png", "jpg", "jpeg"])
                    .pick_file()
                    .map(|path| ImportedIcon::from_path(&path))
                    .transpose();
                let _ = sender.send(result);
                context.request_repaint();
            });
        if let Err(error) = spawned {
            self.pending = None;
            self.error = Some(format!("Could not start image import: {error}"));
        }
    }

    /// Renders the recipe's model on a background thread. The model stays with the editor, so the
    /// button can render again after the draft changes.
    fn start_render(&mut self, context: egui::Context) {
        let Some(model) = &self.model else {
            return;
        };
        let model = Model {
            packages: model.packages.clone(),
            appearance: model.appearance.clone(),
        };
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(receiver);
        self.activity = "Rendering…";
        self.error = None;
        // An install waits for the read, which keeps package files open until it returns.
        let read = sundial::ui::model_preview::PackageRead::start();
        let spawned = std::thread::Builder::new()
            .name("weapon-icon-render".to_owned())
            .spawn(move || {
                let _read = read;
                let _ = sender.send(render_model(&model).map(Some));
                context.request_repaint();
            });
        if let Err(error) = spawned {
            self.pending = None;
            self.error = Some(format!("Could not start the model render: {error}"));
        }
    }
}
