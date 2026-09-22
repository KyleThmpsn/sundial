//! On-demand component inspection shares the main inspector's decoder and renderer.
use super::*;
use crate::weapon_runtime::WeaponRuntimeGraph;
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};

type ResultGraph = Result<WeaponRuntimeGraph, String>;
#[derive(Default)]
pub(super) struct Inspector {
    packages: Option<PathBuf>,
    pending: BTreeMap<u32, mpsc::Receiver<ResultGraph>>,
    loaded: BTreeMap<u32, ResultGraph>,
    options: BTreeMap<u32, crate::ui::catalog::runtime::RuntimeViewOptions>,
}
impl Inspector {
    pub(super) fn sync(&mut self, packages: Option<&Path>) {
        if self.packages.as_deref() != packages {
            *self = Self {
                packages: packages.map(Path::to_path_buf),
                ..Default::default()
            };
        }
    }
    pub(super) fn show(&mut self, ui: &mut egui::Ui, data: &Catalog, tag: u32) {
        if !draw_structure(ui, data, tag) {
            return;
        }
        self.show_fields(ui, tag);
    }

    fn show_fields(&mut self, ui: &mut egui::Ui, tag: u32) {
        if let Some(receiver) = self.pending.get(&tag) {
            match receiver.try_recv() {
                Ok(result) => {
                    self.loaded.insert(tag, result);
                    self.pending.remove(&tag);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.loaded.insert(
                        tag,
                        Err("The component reader stopped before returning a result.".into()),
                    );
                    self.pending.remove(&tag);
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(result) = self.loaded.get(&tag) {
            match result {
                Ok(graph) => crate::ui::catalog::runtime::draw_graph(
                    ui,
                    graph,
                    self.options.entry(tag).or_default(),
                ),
                Err(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            }
        } else if self.pending.contains_key(&tag) {
            ui.spinner();
            ui.label("Reading Component Fields…");
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        } else if self.packages.is_none() {
            ui.label("Choose a game installation to read component field values.");
        } else if ui
            .add_enabled(
                self.packages.is_some(),
                egui::Button::new("Load Component Fields"),
            )
            .on_disabled_hover_text("Choose a game installation to read component values.")
            .clicked()
        {
            let packages = self.packages.clone().expect("enabled with packages");
            let (sender, receiver) = mpsc::channel();
            self.pending.insert(tag, receiver);
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let result = (|| {
                    let manager = crate::investment::discovery::open_packages(&packages)?;
                    let payload = manager
                        .read_tag(tiger_pkg::TagHash(tag))
                        .map_err(|e| e.to_string())?;
                    crate::weapon_entity::validate_weapon_entity(&payload)?;
                    crate::weapon_runtime::load_weapon_runtime_graph_for_entity(
                        &manager, 0, 0, tag, &payload,
                    )
                })();
                let _ = sender.send(result);
                ctx.request_repaint();
            });
        }
    }
}

fn draw_structure(ui: &mut egui::Ui, data: &Catalog, tag: u32) -> bool {
    let entity = data
        .perks
        .patterns
        .iter()
        .filter_map(|p| p.entity.as_ref())
        .chain(data.perks.perks.iter().flat_map(|p| &p.graphs))
        .find(|e| e.tag == tag);
    let entry = data.effects.entries.iter().find(|e| e.graph == tag);
    let mut components = BTreeSet::new();
    if let Some(entity) = entity {
        for c in &entity.components {
            components.insert((c.owner, c.binding, c.class));
        }
    }
    if let Some(entry) = entry {
        for owner in &entry.owners {
            if let Some(resource) = data.effects.owners.get(owner) {
                for c in &resource.components {
                    components.insert((*owner, c.binding, c.class));
                }
            }
        }
    }
    if let Some(owner) = data.effects.owners.get(&tag) {
        for c in &owner.components {
            components.insert((tag, c.binding, c.class));
        }
    }
    if components.is_empty() {
        ui.label("No component structure was recovered for this resource.");
    }
    for (owner, binding, class) in components {
        let title = crate::weapon_runtime::native_type_name(class)
            .map(str::to_owned)
            .unwrap_or_else(|| crate::weapon_runtime::component_binding_label(binding));
        egui::CollapsingHeader::new(title)
            .id_salt((owner, binding, class))
            .show(ui, |ui| {
                ui.label(crate::weapon_runtime::component_binding_label(binding));
                ui.monospace(format!("Resource 0x{owner:08X} · Type 0x{class:08X}"));
                for field in crate::weapon_runtime::native_member_names(class) {
                    ui.label(field);
                }
            });
    }
    entity.is_some() || entry.is_some()
}
