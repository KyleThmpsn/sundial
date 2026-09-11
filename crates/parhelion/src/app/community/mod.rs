//! A single community window for discovery, local copies and sharing.
mod browse;
mod service;
mod sharing;
#[cfg(test)]
mod tests;

use self::service::{Catalog, Client, Downloaded, Receipt};
use super::*;

#[derive(Default, PartialEq, Eq, Clone, Copy)]
enum Tab {
    #[default]
    Browse,
    Downloads,
    Share,
}

enum Outcome {
    Catalog(Catalog),
    Downloaded(Box<Downloaded>),
    Submitted(String),
}

enum Action {
    Install(Box<Downloaded>),
    Remix(Box<Downloaded>, String),
    Open(PathBuf),
}

#[derive(Default)]
pub(super) struct Window {
    pub open: bool,
    tab: Tab,
    catalog: Option<Catalog>,
    search: String,
    sort: browse::Sort,
    tag: String,
    selected: Option<String>,
    downloaded: Option<Downloaded>,
    remix_name: String,
    receipts: BTreeMap<String, Receipt>,
    remix_origins: BTreeMap<String, String>,
    refresh_local: bool,
    worker: Option<Receiver<Result<Outcome, String>>>,
    notice: String,
    error: bool,
    action: Option<Action>,
    share: sharing::Form,
}

impl Window {
    fn start(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce(Client) -> Result<Outcome, String> + Send + 'static,
    ) {
        if self.worker.is_some() {
            return;
        }
        let client = Client::new();
        let ctx = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        self.worker = Some(receiver);
        self.notice.clear();
        thread::spawn(move || {
            let _ = sender.send(work(client));
            ctx.request_repaint();
        });
    }

    fn poll(&mut self) {
        let Some(worker) = &self.worker else {
            return;
        };
        let result = match worker.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The community request stopped unexpectedly. Refresh before retrying".into())
            }
        };
        self.worker = None;
        match result {
            Err(error) => self.message(error, true),
            Ok(outcome) => self.accept(outcome),
        }
    }

    fn accept(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Catalog(catalog) => {
                self.catalog = Some(catalog);
                self.downloaded = None;
                self.selected = None;
                self.refresh_local = true;
                self.message("Community recipes are up to date", false);
            }
            Outcome::Downloaded(downloaded) => {
                self.remix_name = format!("{} Remix", downloaded.recipe.name);
                self.downloaded = Some(*downloaded);
            }
            Outcome::Submitted(id) => {
                self.share.permission = false;
                self.message(
                    format!("Recipe submitted for publication. Reference: {id}"),
                    false,
                );
            }
        }
    }

    fn message(&mut self, text: impl Into<String>, error: bool) {
        self.notice = text.into();
        self.error = error;
    }

    fn show(&mut self, ctx: &egui::Context, current: &WeaponRecipe, can_edit: bool) {
        let mut open = self.open;
        egui::Window::new("Community Recipes")
            .id(egui::Id::new("parhelion-community"))
            .open(&mut open)
            .default_size(egui::vec2(980.0, 690.0))
            .min_size(egui::vec2(620.0, 420.0))
            .resizable(true)
            .show(ctx, |ui| {
                workbench_style(ui);
                ui.heading("Guardians Make Their Own Fate");
                ui.label("Discover community weapons, make a remix, and share your own creations.");
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    for (tab, name) in [
                        (Tab::Browse, "Browse"),
                        (Tab::Downloads, "My Downloads"),
                        (Tab::Share, "Share A Recipe"),
                    ] {
                        if ui.selectable_label(self.tab == tab, name).clicked() {
                            self.tab = tab;
                            if tab == Tab::Share && self.share.recipe.is_none() {
                                self.prepare_share(current);
                            }
                        }
                    }
                });
                ui.separator();
                if self.worker.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Loading Community Recipes...");
                    });
                    ctx.request_repaint_after(Duration::from_millis(100));
                }
                if !self.notice.is_empty() {
                    let color = if self.error {
                        ui.visuals().error_fg_color
                    } else {
                        ui.visuals().text_color()
                    };
                    ui.colored_label(color, &self.notice);
                    ui.add_space(6.0);
                }
                match self.tab {
                    Tab::Browse | Tab::Downloads => self.browse(ui, ctx, can_edit),
                    Tab::Share => self.sharing(ui, ctx, current),
                }
            });
        self.open = open;
    }
}

impl PackageAuthoringApp {
    pub(super) fn draw_community_window(&mut self, ctx: &egui::Context) {
        let mut window = std::mem::take(&mut self.community);
        window.poll();
        if window.refresh_local {
            window.refresh_local = false;
            window.receipts.clear();
            window.remix_origins.clear();
            if let Some(library) = &self.recipe_library {
                for entry in &self.recipe_entries {
                    match service::remix_origin(library, &entry.namespace) {
                        Ok(Some(original)) => {
                            window
                                .remix_origins
                                .insert(entry.namespace.clone(), original);
                        }
                        Ok(None) => {}
                        Err(error) => window.message(error, true),
                    }
                }
            }
            if let (Some(library), Some(catalog)) = (&self.recipe_library, &window.catalog) {
                for entry in &catalog.recipes {
                    match service::load_receipt(library, &entry.listing.id) {
                        Ok(Some(receipt)) => {
                            window.receipts.insert(entry.listing.id.clone(), receipt);
                        }
                        Ok(None) => {}
                        Err(error) => {
                            window.notice = error;
                            window.error = true;
                        }
                    }
                }
            }
        }
        let can_edit = self.build_receiver.is_none()
            && self.install_receiver.is_none()
            && !self.perk_workbench.editing();
        if window.open {
            window.show(ctx, &self.recipe, can_edit);
        }
        if let Some(action) = window.action.take() {
            let result = self.apply_community_action(action, &mut window, can_edit);
            if let Err(error) = result {
                window.message(error, true);
            }
        }
        self.community = window;
    }

    fn apply_community_action(
        &mut self,
        action: Action,
        window: &mut Window,
        can_edit: bool,
    ) -> Result<(), String> {
        if !can_edit {
            return Err("Wait for the current build or editor operation to finish".into());
        }
        let library = self
            .recipe_library
            .as_ref()
            .ok_or("The local recipe library is unavailable")?;
        let (path, message) = match action {
            Action::Open(path) => {
                self.request_recipe_action(PendingRecipeAction::Open(library.root().join(path)));
                return Ok(());
            }
            Action::Install(downloaded) => {
                if self.recipe.namespace == downloaded.recipe.namespace
                    && (self.recipe != self.recipe_baseline || self.recipe_requires_initial_save)
                {
                    return Err(
                        "Save or make a copy of your open edits before updating this recipe".into(),
                    );
                }
                let previous = service::load_receipt(library, &downloaded.entry.listing.id)?;
                let path = service::install(library, &downloaded)?;
                if previous.is_none_or(|receipt| receipt.version < downloaded.entry.listing.version)
                {
                    let entry = downloaded.entry.clone();
                    // Popularity reporting is best effort and never blocks a local save.
                    thread::spawn(move || {
                        let _ = Client::new().record_download(&entry);
                    });
                }
                if self.recipe_path.as_ref() == Some(&path) {
                    self.open_recipe_path(&path);
                }
                (
                    path,
                    "Recipe saved to your library. Use Open In Workbench to customize or build it",
                )
            }
            Action::Remix(downloaded, name) => {
                let path = service::remix(library, &downloaded, &name)?;
                self.request_recipe_action(PendingRecipeAction::Open(path.clone()));
                (path, "Remix saved with its own weapon identity")
            }
        };
        self.refresh_recipe_library();
        window.refresh_local = true;
        window.message(format!("{message}: {}", path.display()), false);
        Ok(())
    }
}
