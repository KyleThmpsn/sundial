//! Before a build starts, the installed items its build set leaves out. Installing a build
//! removes every authored item it does not carry, and the install review says so, but by then
//! the build has run for minutes, so the question is asked first, with a way to add them.
//! The installation's identities cover plugs and frames as well as items, so only the ones a
//! library recipe builds are asked about; an item whose recipe left the library is the
//! review's to name.
use super::*;

pub(super) struct Missing {
    pub hash: u32,
    pub label: String,
    /// The library recipe that builds it.
    pub recipe: PathBuf,
}

struct Job {
    receiver: Receiver<Result<Items, String>>,
    worker: thread::JoinHandle<()>,
}

struct Items {
    installed: BTreeSet<u32>,
    /// The items the build set builds, its required shaders included.
    included: BTreeSet<u32>,
    installed_elapsed: Duration,
    included_elapsed: Duration,
}

#[derive(Default)]
pub(super) struct Check {
    job: Option<Job>,
    /// The installed items the build set leaves out, once the read found some.
    pub missing: Option<Vec<Missing>>,
}

impl Check {
    pub(super) fn busy(&self) -> bool {
        self.job.is_some()
    }
}

impl PackageAuthoringApp {
    /// Build & Stage: reads the installed items first and asks about any the build set leaves
    /// out. Without an installation to read, the build starts at once.
    pub(super) fn start_build_checked(&mut self, ctx: &egui::Context) {
        if self.build_check.busy() {
            return;
        }
        let packages = self.packages.clone();
        if !packages.is_dir() {
            self.start_build();
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        let inputs = self.build_inputs();
        let worker = thread::spawn(move || {
            let started = Instant::now();
            let result = crate::install::installed_item_hashes(&packages).and_then(|installed| {
                let installed_elapsed = started.elapsed();
                let selection_started = Instant::now();
                let included = if installed.is_empty() {
                    BTreeSet::new()
                } else {
                    inputs.included_item_hashes()?
                };
                Ok(Items {
                    installed,
                    included,
                    installed_elapsed,
                    included_elapsed: selection_started.elapsed(),
                })
            });
            // The window releases package access by dropping its catalog, so this copy goes first.
            drop(inputs);
            let _ = sender.send(result);
            ctx.request_repaint();
        });
        self.build_check.job = Some(Job { receiver, worker });
        self.log
            .push(LogEntry::info("Checking installed items before building"));
    }

    pub(super) fn poll_build_check(&mut self) {
        let Some(job) = &self.build_check.job else {
            return;
        };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err("The installed item read stopped without a result".to_owned())
            }
        };
        if let Some(job) = self.build_check.job.take() {
            let _ = job.worker.join();
        }
        match result {
            // The install review reads the same items again and names them, so a failed
            // read here delays nothing.
            Err(error) => {
                self.log.push(LogEntry::error(format!(
                    "Could not check the installed items before building: {error}"
                )));
                self.start_build();
            }
            Ok(Items {
                installed,
                included,
                installed_elapsed,
                included_elapsed,
            }) => {
                self.log.push(LogEntry::info(format!(
                    "Checked installed items in {:.2}s (native tables {:.2}s, build selection {:.2}s)",
                    (installed_elapsed + included_elapsed).as_secs_f64(),
                    installed_elapsed.as_secs_f64(),
                    included_elapsed.as_secs_f64(),
                )));
                self.installed.replace(installed.clone());
                let missing = self.missing_from_build(&installed, &included);
                if missing.is_empty() {
                    self.start_build();
                } else {
                    self.build_check.missing = Some(missing);
                }
            }
        }
    }

    /// The installed items a library recipe builds that no enabled recipe does.
    fn missing_from_build(
        &self,
        installed: &BTreeSet<u32>,
        included: &BTreeSet<u32>,
    ) -> Vec<Missing> {
        let mut seen = BTreeSet::new();
        self.recipe_entries
            .iter()
            .filter(|entry| {
                installed.contains(&entry.identity_hash)
                    && !included.contains(&entry.identity_hash)
                    && !self.enabled_recipe_paths.contains(&entry.path)
                    && !self.recipe_entries.iter().any(|other| {
                        other.identity_hash == entry.identity_hash
                            && self.enabled_recipe_paths.contains(&other.path)
                    })
                    && seen.insert(entry.identity_hash)
            })
            .map(|entry| Missing {
                hash: entry.identity_hash,
                label: entry.name.clone(),
                recipe: entry.path.clone(),
            })
            .collect()
    }

    pub(super) fn draw_build_check(&mut self, ctx: &egui::Context) {
        let Some(missing) = self.build_check.missing.as_ref() else {
            return;
        };
        let mut add = false;
        let mut without = false;
        let mut cancel = false;
        let response = egui::Modal::new("build-check".into()).show(ctx, |ui| {
            crate::app::style::workbench_style(ui);
            ui.set_width(440.0);
            ui.heading("Installed Items Not in This Build");
            ui.label(if missing.len() == 1 {
                "Installing this build removes it from the game and the account.".to_owned()
            } else {
                format!(
                    "Installing this build removes all {} from the game and the account.",
                    missing.len()
                )
            });
            ui.add_space(4.0);
            // A modal sits in the middle of the screen and cannot scroll, so a long list scrolls
            // on its own and leaves the heading and buttons on screen.
            egui::ScrollArea::vertical()
                .id_salt("build-check-items")
                .max_height(list_height(ctx))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for item in missing {
                        ui.label(&item.label).on_hover_text(format!(
                            "{}\n0x{:08X}",
                            item.recipe.display(),
                            item.hash
                        ));
                    }
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                add = ui
                    .add(crate::app::style::primary(ui, "Add to Build"))
                    .clicked();
                without = ui.button("Build Without Them").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        cancel |= response.should_close();
        if add {
            let mut selected = self.enabled_recipe_paths.clone();
            selected.extend(missing.iter().map(|item| item.recipe.clone()));
            match self.apply_build_selection(selected) {
                Ok(()) => {
                    self.build_check.missing = None;
                    self.start_build();
                }
                Err(error) => self.log.push(LogEntry::error(error)),
            }
        } else if without {
            self.build_check.missing = None;
            self.start_build();
        } else if cancel {
            self.build_check.missing = None;
        }
    }
}

/// The most the item list takes: the screen less room for the heading, the text, the buttons and
/// the modal's margins, and never under a few rows.
fn list_height(ctx: &egui::Context) -> f32 {
    (ctx.content_rect().height() - 240.0).clamp(80.0, 360.0)
}
