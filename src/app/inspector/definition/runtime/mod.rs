//! On-demand, read-only package inspection. No recipe or account state is involved.

mod dyes;
mod loader;
mod perks;
#[cfg(test)]
mod tests;
mod view;

use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::Duration,
};

use eframe::egui;

use crate::catalog::{Catalog, ItemPackageMetadata, PackageInspectionAccess};
pub(super) use dyes::draw_dye_colors;
use loader::{LoadedDetails, RuntimeTarget};

const CACHE_LIMIT: usize = 4;
type LoadResult = Result<LoadedDetails, String>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct InspectionScope {
    install: PathBuf,
    item_hash: u64,
    access_generation: u64,
}

#[derive(Debug)]
struct PendingLoad {
    scope: InspectionScope,
    generation: u64,
    target: RuntimeTarget,
    receiver: mpsc::Receiver<LoadResult>,
}

#[derive(Debug, Default)]
pub(super) struct RuntimeInspectionState {
    scope: Option<InspectionScope>,
    access: Option<Arc<PackageInspectionAccess>>,
    generation: u64,
    pending: Option<PendingLoad>,
    cache: BTreeMap<RuntimeTarget, Arc<LoadResult>>,
    cache_order: VecDeque<RuntimeTarget>,
    view: view::RuntimeViewOptions,
}

impl RuntimeInspectionState {
    pub(super) fn prepare(
        &mut self,
        install: &Path,
        item_hash: u64,
        access: Arc<PackageInspectionAccess>,
    ) {
        let scope = InspectionScope {
            install: install.to_owned(),
            item_hash,
            access_generation: access.generation(),
        };
        if self.scope.as_ref() != Some(&scope) {
            self.clear();
            self.scope = Some(scope);
        }
        self.access = Some(access);
    }

    pub(super) fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.scope = None;
        self.access = None;
        self.cache.clear();
        self.cache_order.clear();
        self.view = view::RuntimeViewOptions::default();
        // Retain the in-flight receiver: navigating/closing must not spawn unbounded workers.
        // Its result is discarded unless it still belongs to the current scope.
    }

    pub(super) fn poll(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending else {
            return;
        };
        let result = match pending.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The package reader stopped before returning a result. You can retry.".into())
            }
        };
        let pending = self.pending.take().expect("pending load was checked");
        if self.scope.as_ref() == Some(&pending.scope) && self.generation == pending.generation {
            self.remember(pending.target, result);
        }
    }

    fn remember(&mut self, target: RuntimeTarget, result: LoadResult) {
        self.cache_order.retain(|key| *key != target);
        self.cache_order.push_back(target);
        self.cache.insert(target, Arc::new(result));
        while self.cache_order.len() > CACHE_LIMIT {
            if let Some(oldest) = self.cache_order.pop_front() {
                self.cache.remove(&oldest);
            }
        }
    }

    fn request(&mut self, target: RuntimeTarget, ctx: &egui::Context) {
        if self.pending.is_some() {
            return;
        }
        let Some(scope) = self.scope.clone() else {
            return;
        };
        let Some(access) = self.access.clone() else {
            return;
        };
        let install = scope.install.clone();
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        match std::thread::Builder::new()
            .name("sundial-runtime-inspector".into())
            .spawn(move || {
                let result = access.read(|| loader::load(&install, target));
                let _ = sender.send(result);
                repaint.request_repaint();
            }) {
            Ok(_) => {
                self.cache.remove(&target);
                self.pending = Some(PendingLoad {
                    scope,
                    generation: self.generation,
                    target,
                    receiver,
                });
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            Err(error) => self.remember(
                target,
                Err(format!("Could not start package reader: {error}")),
            ),
        }
    }

    fn draw_request(
        &mut self,
        ui: &mut egui::Ui,
        target: RuntimeTarget,
    ) -> Option<Arc<LoadResult>> {
        let loaded = self.cache.get(&target).cloned();
        let paused = self
            .access
            .as_ref()
            .is_none_or(|access| access.is_suspended());
        let loading = self.pending.as_ref().is_some_and(|pending| {
            pending.target == target
                && self.scope.as_ref() == Some(&pending.scope)
                && self.generation == pending.generation
        });
        ui.horizontal_wrapped(|ui| {
            let label = match loaded.as_deref() {
                Some(Ok(_)) => "Reload from Packages",
                Some(Err(_)) => "Retry",
                None => "Load from Packages",
            };
            if ui
                .add_enabled(!paused && self.pending.is_none(), egui::Button::new(label))
                .clicked()
            {
                self.request(target, ui.ctx());
            }
            if paused {
                ui.weak("Package inspection is paused while Parhelion is open.");
            } else if loading {
                ui.spinner();
                ui.weak("Reading package data…");
            } else if self.pending.is_some() {
                ui.weak("Another package read is finishing.");
            }
        });
        if let Some(Err(error)) = loaded.as_deref() {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        loaded
    }
}

pub(super) fn draw_item_runtime(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item_hash: u64,
    metadata: &ItemPackageMetadata,
    state: &mut RuntimeInspectionState,
) {
    if let Some(pattern) = metadata
        .weapon_pattern_index
        .filter(|index| *index != u16::MAX)
    {
        egui::CollapsingHeader::new("Weapon Runtime")
            .id_salt(("inspector-weapon-runtime", item_hash))
            .default_open(true)
            .show(ui, |ui| {
                ui.weak("Installed package values, not live game state or final stats after equipped perks.");
                ui.weak(format!("Weapon pattern row {pattern}"));
                let loaded = state.draw_request(ui, RuntimeTarget::Weapon(pattern));
                if let Some(Ok(LoadedDetails::Weapon(graph))) = loaded.as_deref() {
                    view::draw_graph(ui, graph, &mut state.view);
                }
            });
    }
    perks::draw_perks(ui, catalog, item_hash, metadata, state);
}
