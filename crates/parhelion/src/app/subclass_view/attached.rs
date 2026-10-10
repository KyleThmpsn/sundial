//! Presentation controls for descendant abilities that already have native HUD controllers.
use super::*;
use sundial::package_authoring::ability_hud::{self, AttachedGlyph};

type Loaded = Result<Arc<Vec<AttachedGlyph>>, String>;

#[derive(Default)]
pub(super) struct Abilities {
    loaded: BTreeMap<u32, Loaded>,
    pending: Option<(u32, Receiver<Loaded>)>,
}

impl Abilities {
    fn poll(&mut self, ctx: &egui::Context, packages: &Path, entity: u32) -> Option<Loaded> {
        if let Some((source, receiver)) = &self.pending {
            let result = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => {
                    Some(Err("The attached ability reader stopped.".into()))
                }
                Err(TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                self.loaded.insert(*source, result);
                self.pending = None;
            }
        }
        if let Some(loaded) = self.loaded.get(&entity) {
            return Some(loaded.clone());
        }
        if self.pending.is_none() {
            let packages = packages.to_path_buf();
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let result = open_shadowkeep_package_manager(&packages)
                    .and_then(|manager| {
                        ability_hud::attached_glyphs(&manager, entity, crate::subclass::SPAWN_DEPTH)
                    })
                    .map(Arc::new);
                let _ = sender.send(result);
            });
            self.pending = Some((entity, receiver));
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        None
    }
}

impl PackageAuthoringApp {
    pub(super) fn draw_attached_abilities(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        source: (Option<&SubclassSummary>, u8),
        edits: &mut EntryEdits,
        page: &mut PageState,
    ) -> bool {
        let (summary, entry) = source;
        let Some(entity) = summary.and_then(|summary| summary.entry_entities.get(&entry).copied())
        else {
            return false;
        };
        let groups = match page.attached.poll(ui.ctx(), &self.packages, entity) {
            Some(Ok(groups)) => groups,
            Some(Err(error)) => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Could not read attached abilities: {error}"),
                );
                return false;
            }
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("Reading attached abilities…");
                });
                return false;
            }
        };
        if groups.is_empty() && edits.attached_abilities.is_empty() {
            return false;
        }
        ui.add_space(12.0);
        ui.label(egui::RichText::new("Attached Abilities").strong());
        ui.label(quiet(
            ui,
            "Edit the icon and HUD color of an ability this one provides.",
        ));
        let before = edits.clone();
        for (index, group) in groups.iter().enumerate() {
            let kind = group
                .target
                .and_then(|target| target.label())
                .unwrap_or("Attached Ability");
            let repeated = groups
                .iter()
                .filter(|each| each.target == group.target)
                .count()
                > 1;
            let title = if repeated {
                let number = groups
                    .iter()
                    .take(index + 1)
                    .filter(|each| each.target == group.target)
                    .count();
                format!("{kind} {number}")
            } else {
                kind.to_owned()
            };
            ui.push_id(("attached-hud", entity, &group.graphs), |ui| {
                self.draw_attached_group(ui, base, source, edits, page, (group, &title));
            });
        }
        // A manually edited or older recipe can name a graph this source no longer reaches.
        // Keep that saved edit visible and removable, while the compiler rejects applying it.
        let missing = edits
            .attached_abilities
            .iter()
            .filter(|edit| {
                !groups
                    .iter()
                    .any(|group| group.graphs.contains(&edit.graph))
            })
            .map(|edit| edit.graph)
            .collect::<Vec<_>>();
        for graph in missing {
            ui.push_id(("missing-attached", graph), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("A saved attached ability is unavailable for this source.");
                    if ui.button("Remove").clicked() {
                        edits.set_attached(crate::subclass::AttachedAbility {
                            graph,
                            icon: None,
                            color: None,
                        });
                    }
                });
            });
        }
        *edits != before
    }

    fn draw_attached_group(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        source: (Option<&SubclassSummary>, u8),
        edits: &mut EntryEdits,
        page: &mut PageState,
        (group, title): (&AttachedGlyph, &str),
    ) {
        style::card(ui, |ui| {
            let modified = group.graphs.iter().any(|graph| {
                edits
                    .attached_abilities
                    .iter()
                    .any(|edit| edit.graph == *graph)
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(title).strong());
                if modified && ui.small_button("Restore Attached Ability").clicked() {
                    for &graph in &group.graphs {
                        edits.set_attached(crate::subclass::AttachedAbility {
                            graph,
                            icon: None,
                            color: None,
                        });
                    }
                    page.icons.cancel();
                }
            });
            let first = edits.attached(group.graphs[0]);
            let mixed = group.graphs.iter().any(|&graph| {
                let other = edits.attached(graph);
                other.icon != first.icon || other.color != first.color
            });
            if mixed {
                ui.label(quiet(ui, "These variants have different saved settings. Choosing a value applies it to all of them."));
            }
            let (picked, reset) = detail::field(ui, "Icon", first.icon.is_some(), |ui| {
                self.draw_icon_choices(ui, base, source, (first.icon.as_ref(), &group.graphs), page)
            });
            if picked.is_some() || reset {
                for &graph in &group.graphs {
                    let mut attached = edits.attached(graph);
                    attached.icon = if reset { None } else { picked.clone() };
                    edits.set_attached(attached);
                }
                if reset {
                    page.icons.cancel();
                }
            }
            if let Some(color) = self.draw_attached_color(ui, first.color) {
                for &graph in &group.graphs {
                    let mut attached = edits.attached(graph);
                    attached.color = color;
                    edits.set_attached(attached);
                }
            }
        });
    }
}
