//! Node icons use the shared artwork browser, initially filtered to native ability icons.
use super::*;
use crate::artwork_browser::{Abilities, Browser, Picked, Selection};

pub(super) struct Picker {
    browser: crate::artwork_browser::Picker,
    query: String,
    pending: Option<Pending>,
    error: Option<(Place, Vec<u32>, String)>,
}

struct Pending {
    recipe: egui::Id,
    place: Place,
    source: (u32, u8),
    graphs: Vec<u32>,
    before: Vec<Option<EntryIcon>>,
    result: Receiver<Result<crate::perk::Icon, String>>,
}

impl Default for Picker {
    fn default() -> Self {
        Self {
            browser: crate::artwork_browser::Picker::for_abilities(),
            query: String::new(),
            pending: None,
            error: None,
        }
    }
}

fn recipe_key(recipe: &WeaponRecipe, base: u32) -> egui::Id {
    egui::Id::new((&recipe.namespace, &recipe.identity.item_hash, base))
}

fn current_icons(edits: &EntryEdits, graphs: &[u32]) -> Vec<Option<EntryIcon>> {
    if graphs.is_empty() {
        vec![edits.icon.clone()]
    } else {
        graphs
            .iter()
            .map(|&graph| edits.attached(graph).icon)
            .collect()
    }
}

impl Picker {
    pub(super) fn cancel(&mut self) {
        self.pending = None;
        self.error = None;
    }

    /// A deferred perk texture belongs to the recipe and node that selected it. Other edits on
    /// that node survive, while a newer icon, changed source or different recipe cancels it.
    pub(super) fn poll(&mut self, recipe: &mut WeaponRecipe, base: u32, ctx: &egui::Context) {
        self.browser.poll();
        if self.browser.busy() || self.pending.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let Some(pending) = &self.pending else { return };
        let mut abilities = recipe
            .overrides
            .subclass_abilities
            .clone()
            .unwrap_or_default();
        let mut edits = abilities.edits(base, pending.place);
        if pending.recipe != recipe_key(recipe, base)
            || pending.source != source_of(&abilities, base, pending.place)
            || pending.before != current_icons(&edits, &pending.graphs)
        {
            self.cancel();
            return;
        }
        match pending.result.try_recv() {
            Ok(Ok(artwork)) => {
                let icon = Some(EntryIcon::Artwork { artwork });
                if pending.graphs.is_empty() {
                    edits.icon = icon;
                } else {
                    for &graph in &pending.graphs {
                        let mut attached = edits.attached(graph);
                        attached.icon = icon.clone();
                        edits.set_attached(attached);
                    }
                }
                abilities.set_edits(base, pending.place, edits);
                recipe.overrides.subclass_abilities = Some(abilities);
            }
            Ok(Err(error)) => self.error = Some((pending.place, pending.graphs.clone(), error)),
            Err(TryRecvError::Disconnected) => {
                self.error = Some((
                    pending.place,
                    pending.graphs.clone(),
                    "The icon reader stopped. Choose the icon again.".into(),
                ));
            }
            Err(TryRecvError::Empty) => return,
        }
        self.pending = None;
    }
}

impl PackageAuthoringApp {
    pub(super) fn draw_icon_choices(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        (summary, entry): (Option<&SubclassSummary>, u8),
        (current, graphs): (Option<&EntryIcon>, &[u32]),
        page: &mut PageState,
    ) -> Option<EntryIcon> {
        let SubclassSelection::Entry(place) = page.selection else {
            return None;
        };
        let state = &mut page.icons;
        let selected = match current {
            Some(EntryIcon::Ability { subclass, entry }) => Some((*subclass, *entry)),
            Some(EntryIcon::Artwork { .. }) => None,
            None if graphs.is_empty() => summary.map(|subclass| (subclass.hash, entry)),
            None => None,
        };
        let artwork = match current {
            Some(EntryIcon::Artwork { artwork }) => Some(artwork),
            _ => None,
        };
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            let shown = match selected {
                Some((subclass, entry)) => {
                    self.entry_icon(ui.ctx(), find_subclass(&self.subclasses, subclass), entry)
                }
                None => self.catalog.as_ref().and_then(|catalog| {
                    crate::artwork_browser::preview::texture(ui.ctx(), catalog, artwork?)
                }),
            };
            if let Some(icon) = shown {
                ui.add(egui::Image::new(&icon).fit_to_exact_size(egui::Vec2::splat(CHOICE_ICON)));
            }
            let reading = if let Some((subclass, entry)) = selected {
                find_subclass(&self.subclasses, subclass).map_or_else(
                    || "Unknown Ability".to_owned(),
                    |subclass| format!("{} · {}", entry_name(subclass, entry), subclass.name),
                )
            } else {
                match artwork {
                    Some(crate::perk::Icon::Image { name, .. }) => name.clone(),
                    None => "Original Icon".to_owned(),
                    _ => "Artwork".to_owned(),
                }
            };
            ui.label(reading);
            let selection = crate::app::pickers::browser_with_toolbar(
                ui,
                (
                    "subclass-node-icon",
                    recipe_key(&self.recipe, base.hash),
                    place.key(),
                    graphs,
                ),
                if graphs.is_empty() {
                    "Change Icon…"
                } else {
                    "Change Attached Ability Icon…"
                },
                "Choose Icon",
                &mut state.query,
                |ui, query, opened, height| {
                    state.browser.draw_abilities(
                        ui,
                        query,
                        opened,
                        height,
                        (
                            Browser {
                                packages: Some(self.packages.as_path()),
                                catalog: self.catalog.as_ref(),
                                current: artwork,
                            },
                            Abilities {
                                choices: &self.subclasses,
                                current: selected,
                            },
                        ),
                    )
                },
            );
            match selection {
                Some(Picked::Ability { subclass, entry }) => {
                    state.cancel();
                    picked = Some(EntryIcon::Ability { subclass, entry });
                }
                Some(Picked::Artwork(Selection::Icon(artwork))) => {
                    state.cancel();
                    picked = Some(EntryIcon::Artwork { artwork });
                }
                Some(Picked::Artwork(selection)) => {
                    state.cancel();
                    state.pending = Some(Pending {
                        recipe: recipe_key(&self.recipe, base.hash),
                        place,
                        source: (summary.map_or(base.hash, |summary| summary.hash), entry),
                        graphs: graphs.to_vec(),
                        before: current_icons(
                            &self
                                .recipe
                                .overrides
                                .subclass_abilities
                                .clone()
                                .unwrap_or_default()
                                .edits(base.hash, place),
                            graphs,
                        ),
                        result: state.browser.icon(
                            selection,
                            &self.packages,
                            self.catalog.as_ref(),
                            ui.ctx(),
                        ),
                    });
                }
                None => {}
            }
            if state
                .pending
                .as_ref()
                .is_some_and(|pending| pending.place == place && pending.graphs == graphs)
            {
                ui.spinner();
                ui.weak("Loading icon…");
            }
        });
        if let Some((target, targets, error)) = &state.error
            && *target == place
            && targets == graphs
        {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        picked
    }
}
