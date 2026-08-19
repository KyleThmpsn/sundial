use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread,
    time::Duration,
};

use eframe::egui;

use crate::{catalog::CatalogProgress, game_settings};

use super::{
    DISPLAY_VERSION, InstallSelection, PendingFutureSchemaLoad, Preferences, SettingsLayout,
    SettingsPathResolution, SundialApp, draw_future_schema_warning, load_logo_texture,
    settings::{
        load_json, missing_settings_message, resolve_settings_path, settings_path_for_install,
    },
};

enum StartupEvent {
    Progress(CatalogProgress),
    SettingsChoice(PathBuf),
    FutureSchema(PendingFutureSchemaLoad),
    Finished(Box<Result<SundialApp, String>>),
}

pub(super) struct StartupApp {
    editor: Option<SundialApp>,
    receiver: Option<Receiver<StartupEvent>>,
    install_path: Option<PathBuf>,
    progress: CatalogProgress,
    error: Option<String>,
    logo: Option<egui::TextureHandle>,
    pending_settings_choice: Option<PathBuf>,
    pending_future_schema: Option<PendingFutureSchemaLoad>,
    preferences: Preferences,
}

impl StartupApp {
    pub(super) fn new(selection: Option<InstallSelection>, preferences: Preferences) -> Self {
        let mut app = Self {
            editor: None,
            receiver: None,
            install_path: selection
                .as_ref()
                .map(|selection| selection.install_path.clone()),
            progress: CatalogProgress {
                message: "Waiting for a Shadowkeep installation…",
                completed: 0,
                total: 0,
            },
            error: None,
            logo: None,
            pending_settings_choice: None,
            pending_future_schema: None,
            preferences,
        };
        if let Some(selection) = selection {
            app.begin_loading(selection.install_path, selection.preferred_layout);
        }
        app
    }

    fn begin_loading(&mut self, install_path: PathBuf, preferred_layout: Option<SettingsLayout>) {
        let (sender, receiver) = mpsc::channel();
        let preferences = self.preferences.clone();
        self.install_path = Some(install_path.clone());
        self.receiver = Some(receiver);
        self.error = None;
        self.pending_settings_choice = None;
        self.pending_future_schema = None;
        self.progress = CatalogProgress {
            message: "Checking the saved installation…",
            completed: 0,
            total: 0,
        };
        thread::spawn(
            move || match resolve_settings_path(&install_path, preferred_layout) {
                SettingsPathResolution::Found(settings_layout, settings_path) => {
                    match load_json(&settings_path) {
                        Ok(document) => {
                            if let Some(schema_version) =
                                game_settings::future_schema_version(&document)
                            {
                                let _ = sender.send(StartupEvent::FutureSchema(
                                    PendingFutureSchemaLoad {
                                        install_path,
                                        settings_path,
                                        settings_layout,
                                        schema_version,
                                    },
                                ));
                            } else {
                                load_editor(
                                    &sender,
                                    install_path,
                                    settings_path,
                                    settings_layout,
                                    preferences,
                                );
                            }
                        }
                        Err(error) => send_startup_error(&sender, error),
                    }
                }
                SettingsPathResolution::Missing => {
                    send_startup_error(&sender, missing_settings_message(&install_path));
                }
                SettingsPathResolution::Ambiguous => {
                    let _ = sender.send(StartupEvent::SettingsChoice(install_path));
                }
            },
        );
    }

    fn start_loading_at(
        &mut self,
        install_path: PathBuf,
        settings_path: PathBuf,
        settings_layout: SettingsLayout,
    ) {
        let (sender, receiver) = mpsc::channel();
        let preferences = self.preferences.clone();
        self.install_path = Some(install_path.clone());
        self.receiver = Some(receiver);
        self.error = None;
        self.pending_settings_choice = None;
        self.pending_future_schema = None;
        self.progress = CatalogProgress {
            message: "Starting the local catalog…",
            completed: 0,
            total: 0,
        };
        thread::spawn(move || {
            load_editor(
                &sender,
                install_path,
                settings_path,
                settings_layout,
                preferences,
            );
        });
    }

    fn choose_install(&mut self) {
        let mut dialog =
            rfd::FileDialog::new().set_title("Select the Destiny 2 Shadowkeep installation");
        if let Some(path) = self.install_path.as_ref().filter(|path| path.is_dir()) {
            dialog = dialog.set_directory(path);
        }
        if let Some(path) = dialog.pick_folder() {
            self.begin_loading(path, None);
        }
    }

    fn receive_events(&mut self) {
        let mut events = Vec::new();
        let mut disconnected = false;
        if let Some(receiver) = &self.receiver {
            loop {
                match receiver.try_recv() {
                    Ok(event) => events.push(event),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        for event in events {
            match event {
                StartupEvent::Progress(progress) => self.progress = progress,
                StartupEvent::SettingsChoice(install_path) => {
                    self.pending_settings_choice = Some(install_path);
                    self.receiver = None;
                }
                StartupEvent::FutureSchema(pending) => {
                    self.pending_future_schema = Some(pending);
                    self.receiver = None;
                }
                StartupEvent::Finished(result) => match *result {
                    Ok(mut editor) => {
                        editor.logo.clone_from(&self.logo);
                        if let Err(error) = editor.save_preferences() {
                            editor.set_status(
                                format!(
                                    "Loaded successfully, but the install location could not be remembered: {error}"
                                ),
                                true,
                            );
                        }
                        self.editor = Some(editor);
                        self.receiver = None;
                    }
                    Err(error) => {
                        self.error = Some(error);
                        self.receiver = None;
                    }
                },
            }
        }
        if disconnected && self.receiver.is_some() {
            self.error = Some(
                "The startup task stopped unexpectedly. Try again or choose another folder"
                    .to_owned(),
            );
            self.receiver = None;
        }
    }

    fn draw_startup(&mut self, ctx: &egui::Context) {
        let logo = self
            .logo
            .get_or_insert_with(|| load_logo_texture(ctx))
            .clone();
        egui::CentralPanel::default().show(ctx, |ui| {
            let top_space = ((ui.available_height() - 440.0) / 2.0).max(16.0);
            ui.add_space(top_space);
            ui.vertical_centered(|ui| {
                egui::Frame::group(ui.style())
                    .inner_margin(28.0)
                    .show(ui, |ui| {
                        ui.set_width(500.0_f32.min(ui.available_width()));
                        ui.vertical_centered(|ui| {
                            ui.image((logo.id(), egui::vec2(72.0, 72.0)));
                            ui.heading("Sundial");
                            ui.label(egui::RichText::new(DISPLAY_VERSION).weak());
                            ui.add_space(18.0);

                            if let Some(install_path) = self.pending_settings_choice.clone() {
                                ui.heading("Choose Sunrise settings");
                                ui.add_space(6.0);
                                ui.label("Two existing settings.json files were found. Choose the one Project Sunrise uses for this installation.");
                                ui.add_space(14.0);
                                for layout in SettingsLayout::ALL {
                                    let path = settings_path_for_install(&install_path, layout);
                                    if ui
                                        .add_sized(
                                            [400.0, 34.0],
                                            egui::Button::new(format!(
                                                "Use {}",
                                                layout.relative_path()
                                            )),
                                        )
                                        .clicked()
                                    {
                                        self.begin_loading(install_path.clone(), Some(layout));
                                    }
                                    ui.label(
                                        egui::RichText::new(path.display().to_string())
                                            .weak()
                                            .small(),
                                    );
                                    ui.add_space(8.0);
                                }
                                if ui.button("Choose another folder").clicked() {
                                    self.choose_install();
                                }
                                return;
                            }

                            if let Some(pending) = self.pending_future_schema.clone() {
                                draw_future_schema_warning(ui, &pending);
                                ui.add_space(16.0);
                                ui.horizontal(|ui| {
                                    if ui.button("Proceed with caution").clicked() {
                                        self.start_loading_at(
                                            pending.install_path.clone(),
                                            pending.settings_path.clone(),
                                            pending.settings_layout,
                                        );
                                    }
                                    if ui.button("Choose another folder").clicked() {
                                        self.pending_future_schema = None;
                                        self.choose_install();
                                    }
                                });
                                return;
                            }

                            if let Some(error) = self.error.clone() {
                                ui.colored_label(
                                    ui.visuals().error_fg_color,
                                    "Could not load that installation",
                                );
                                ui.add_space(6.0);
                                ui.label(error);
                                ui.add_space(16.0);
                                ui.horizontal(|ui| {
                                    if ui.button("Choose another folder").clicked() {
                                        self.choose_install();
                                    }
                                    if let Some(path) = self.install_path.clone() {
                                        if ui.button("Try again").clicked() {
                                            self.begin_loading(path, None);
                                        }
                                    }
                                });
                                return;
                            }

                            if self.receiver.is_none() {
                                ui.heading("Choose your Shadowkeep installation");
                                ui.add_space(6.0);
                                ui.label("Select the Destiny 2 Shadowkeep installation you use with Project Sunrise to begin.");
                                ui.add_space(10.0);
                                ui.label(
                                    egui::RichText::new(
                                        "Sundial will read the installed packages once to build its local item catalog. Nothing is downloaded.",
                                    )
                                    .weak(),
                                );
                                ui.add_space(18.0);
                                if ui
                                    .add_sized(
                                        [240.0, 36.0],
                                        egui::Button::new("Choose Shadowkeep folder…"),
                                    )
                                    .clicked()
                                {
                                    self.choose_install();
                                }
                                return;
                            }

                            ui.spinner();
                            ui.strong(self.progress.message);
                            ui.add_space(10.0);
                            let mut bar = egui::ProgressBar::new(self.progress.fraction())
                                .desired_width(400.0)
                                .corner_radius(egui::CornerRadius::same(3));
                            if self.progress.total > 0 {
                                bar = bar.show_percentage();
                            } else {
                                bar = bar.animate(true);
                            }
                            ui.add(bar);
                            if let Some(path) = &self.install_path {
                                ui.add_space(10.0);
                                ui.label(
                                    egui::RichText::new(path.display().to_string())
                                        .weak()
                                        .small(),
                                );
                            }
                        });
                    });
            });
        });
    }
}

fn load_editor(
    sender: &Sender<StartupEvent>,
    install_path: PathBuf,
    settings_path: PathBuf,
    settings_layout: SettingsLayout,
    preferences: Preferences,
) {
    let progress_sender = sender.clone();
    let result = SundialApp::new_with_progress(
        settings_path,
        settings_layout,
        install_path,
        preferences,
        move |progress| {
            let _ = progress_sender.send(StartupEvent::Progress(progress));
        },
    );
    let _ = sender.send(StartupEvent::Finished(Box::new(result)));
}

fn send_startup_error(sender: &Sender<StartupEvent>, error: String) {
    let _ = sender.send(StartupEvent::Finished(Box::new(Err(error))));
}

impl eframe::App for StartupApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.receive_events();
        if let Some(editor) = &mut self.editor {
            editor.update(ctx, frame);
        } else {
            self.draw_startup(ctx);
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_construction_does_not_probe_the_saved_installation() {
        let selection = InstallSelection {
            install_path: std::env::temp_dir().join("sundial-unavailable-install"),
            preferred_layout: None,
        };

        let app = StartupApp::new(Some(selection), Preferences::default());

        assert!(app.receiver.is_some());
        assert!(app.error.is_none());
        assert_eq!(app.progress.message, "Checking the saved installation…");
    }

    #[test]
    fn disconnected_startup_worker_becomes_a_recoverable_error() {
        let mut app = StartupApp::new(None, Preferences::default());
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        app.receiver = Some(receiver);

        app.receive_events();

        assert!(app.receiver.is_none());
        assert!(
            app.error
                .as_deref()
                .is_some_and(|error| error.contains("stopped unexpectedly"))
        );
    }
}
