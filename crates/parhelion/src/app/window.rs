//! The Parhelion window: its size on the monitor, its logo, and the utility the suite
//! opens and updates.
use super::*;

pub(super) const WINDOW_TITLE: &str = "Parhelion";

/// The window's size when it opens: most of the monitor the host window is on, within bounds,
/// so the workbench's columns and the model preview have their room from the first frame.
pub(super) const WINDOW_MIN_SIZE: [f32; 2] = [900.0, 640.0];
pub(super) const WINDOW_MAX_SIZE: [f32; 2] = [1_760.0, 1_080.0];
pub(super) const WINDOW_MONITOR_SHARE: f32 = 0.85;

pub(super) fn window_size(context: &egui::Context) -> [f32; 2] {
    let monitor = context.input(|input| input.viewport().monitor_size);
    let wanted = monitor.map_or([1_480.0, 940.0], |monitor| {
        [
            monitor.x * WINDOW_MONITOR_SHARE,
            monitor.y * WINDOW_MONITOR_SHARE,
        ]
    });
    std::array::from_fn(|axis| {
        wanted[axis]
            .clamp(WINDOW_MIN_SIZE[axis], WINDOW_MAX_SIZE[axis])
            .floor()
    })
}

pub(super) fn load_parhelion_logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let image = image::load_from_memory(include_bytes!("../../../../assets/sundial-alt.png"))
        .expect("bundled Parhelion logo must be a valid PNG")
        .into_rgba8();
    let (width, height) = image.dimensions();
    let image =
        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], image.as_raw());
    ctx.load_texture("parhelion-logo", image, egui::TextureOptions::LINEAR)
}

/// Parhelion's Sundial-hosted native window.
#[derive(Default)]
pub struct Parhelion {
    pub(super) app: PackageAuthoringApp,
    pub(super) open: bool,
    pub(super) close_pending: bool,
    pub(super) focus_requested: bool,
    /// The size the window opened at, held while it is open: the viewport builder is compared
    /// frame to frame, so a changing size would resize the window.
    pub(super) window_size: Option<[f32; 2]>,
}

impl PackageAuthoringUtility for Parhelion {
    fn open(
        &mut self,
        context: &egui::Context,
        install_directory: &Path,
        preferences: PackageAuthoringPreferences,
    ) -> Result<(), String> {
        let packages = install_directory.join("packages");
        let branding = crate::branding::Branding::detect(install_directory);
        self.app.perk_workbench.branding = branding;
        if self.app.presentation_editor.branding() != branding {
            self.app.presentation_editor.set_branding(branding);
            self.app.invalidate_results();
            self.app.authored_icon_preview = None;
            self.app.authored_icon_layers = None;
            self.app.authored_icon_loading = None;
            self.app.library_icons = library_view::LibraryIcons::default();
        }
        if !packages.is_dir() {
            return Err(format!(
                "The selected installation has no packages directory at {}",
                packages.display()
            ));
        }
        if self.app.packages != packages {
            self.app.packages = packages;
            self.app.reset_catalog_load();
            self.app.invalidate_results();
        }
        self.app.show_plug_safety_warnings = sundial::investment::show_plug_safety_warnings();
        self.app.initialize_storage();
        self.app.plug_selection_mode = sundial::investment::default_plug_selection_mode();
        self.app.show_experimental_options = preferences.show_parhelion_experimental_options;
        self.app.preferences_changed = false;
        self.app.scroll_recipe_to_top = true;
        sundial::investment::configure_authoring_fonts(context, install_directory)?;
        self.open = true;
        self.close_pending = false;
        self.focus_requested = true;
        Ok(())
    }

    fn update(&mut self, context: &egui::Context) -> PackageAuthoringUpdate {
        if !self.open {
            return PackageAuthoringUpdate::default();
        }

        let focus_requested = std::mem::take(&mut self.focus_requested);
        let size = *self.window_size.get_or_insert_with(|| window_size(context));
        let close_now = context.show_viewport_immediate(
            egui::ViewportId::from_hash_of("parhelion"),
            egui::ViewportBuilder::default()
                .with_title(WINDOW_TITLE)
                .with_app_id("io.github.kylethmpsn.Sundial.Parhelion")
                .with_inner_size(size)
                .with_min_inner_size(WINDOW_MIN_SIZE)
                .with_icon(sundial::ui::window_icon()),
            |viewport_context, _class| {
                if focus_requested {
                    viewport_context.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                self.app.update_ui(viewport_context);
                sundial::ui::model_preview::pause_source(
                    viewport_context,
                    self.app.install_receiver.is_some()
                        || self.app.uninstall.busy()
                        || self.app.catalog_is_loading(),
                );
                let close_requested =
                    viewport_context.input(|input| input.viewport().close_requested());
                if close_requested || self.close_pending {
                    // A view's read is not worth waiting on to close.
                    self.app.perk_workbench.stop_optional_reads();
                }
                let busy = self.app.has_background_work();
                if close_requested {
                    self.close_pending = true;
                    viewport_context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
                if self.close_pending && !busy {
                    self.close_pending = false;
                    self.app.request_recipe_action(PendingRecipeAction::Close);
                }
                if self.app.take_close_approved() {
                    viewport_context.send_viewport_cmd(egui::ViewportCommand::Close);
                    true
                } else {
                    false
                }
            },
        );

        if close_now {
            self.open = false;
            self.close_pending = false;
            self.window_size = None;
            self.app.release_package_access();
        }
        PackageAuthoringUpdate {
            open: self.open,
            busy: self.app.has_background_work(),
            dirty: self.app.recipe_dirty,
            packages_changed: self.app.take_packages_changed(),
            account_changed: std::mem::take(&mut self.app.account_changed),
            preferences_changed: self.app.take_preferences_changed(),
            open_sundial_preferences: std::mem::take(&mut self.app.open_sundial_preferences),
        }
    }

    fn save_on_exit(&mut self) {
        #[cfg(feature = "d2-model-importer")]
        self.app.importer.save_view();
    }
}
