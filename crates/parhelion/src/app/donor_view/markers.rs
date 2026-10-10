//! Placement, on the Appearance tab: where the model sits in the hand, and Markers, the named
//! points the appearance's gear art carries, such as `iron_sight` and `primary_fire`, drawn on
//! the model, selected by double-clicking and moved by dragging, nudging or typing. The build
//! moves both in private copies of the parts that carry them. The hand holds the model at its
//! origin, so moving the model moves it against the hands, which first person shows most.
use super::*;
use crate::recipe::{
    HELD_OFFSET_LIMIT_UM, MARKER_OFFSET_LIMIT_UM, MarkerOffsetRecipe, WeaponRecipeOverrides,
};
use sundial::package_authoring::gear_markers::{MarkerSet, marker_name, nearest_named};
use sundial::ui::model_preview::still::{self, Pin, View};

/// Micrometres in a metre: offsets are kept in whole micrometres, positions in metres.
const MICRONS: f32 = 1_000_000.0;
/// What one arrow press moves a marker, and with Shift held.
const NUDGE: f32 = 0.001;
const SHIFT_NUDGE: f32 = 0.01;
const AXES: [&str; 3] = ["Forward", "Side", "Up"];
/// Wide enough for "-50.00 cm", so a field never grows with its value.
const FIELD_WIDTH: f32 = 92.0;
const PREVIEW_ID: &str = "appearance-markers";
/// The selection standing for the grip, the point the hand holds. No marker name hashes to 0.
const GRIP: u32 = 0;

/// The editor's view and selection, and its read of every part's markers.
#[derive(Default)]
pub(in crate::app) struct MarkerEditor {
    view: View,
    selected: Option<u32>,
    show_all: bool,
    read: Option<MarkerRead>,
    job: Option<ReadJob>,
}

struct ReadJob {
    arrangements: Vec<u16>,
    receiver: Receiver<Result<Vec<MarkerSet>, String>>,
    worker: thread::JoinHandle<()>,
}

impl Drop for MarkerEditor {
    fn drop(&mut self) {
        // No package handle may outlive the catalog or survive into an install.
        if let Some(job) = self.job.take() {
            let _ = job.worker.join();
        }
    }
}

impl MarkerEditor {
    /// Takes a finished read. Called every frame, so a read never holds packages after it ends.
    pub(in crate::app) fn poll(&mut self) {
        let Some(job) = &self.job else {
            return;
        };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err("The marker reader stopped without a result".to_owned())
            }
        };
        let ReadJob {
            arrangements,
            worker,
            ..
        } = self.job.take().expect("job was checked");
        let result = if worker.join().is_err() {
            Err("The marker reader crashed".to_owned())
        } else {
            result
        };
        self.read = Some((arrangements, result));
    }

    /// Whether the read still holds package handles, which an installation has to wait out.
    pub(in crate::app) fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Starts reading every part of `arrangements` unless that read is done or running. One
    /// read runs at a time, so a stale one finishes before the next starts.
    fn request(&mut self, ctx: &egui::Context, packages: &Path, arrangements: &[u16]) {
        self.poll();
        if self.job.is_some()
            || self
                .read
                .as_ref()
                .is_some_and(|(read, _)| read == arrangements)
        {
            return;
        }
        let packages = packages.to_owned();
        let wanted = arrangements.to_vec();
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        let worker = {
            let wanted = wanted.clone();
            thread::spawn(move || {
                let mut sets = Vec::new();
                let mut result = Ok(());
                for arrangement in wanted {
                    use sundial::package_authoring::gear_markers::{
                        read_appearance, read_appearance_parts,
                    };
                    // The default parts' sets lead, so a name that several swap-in parts carry
                    // is placed where the previewed model puts it.
                    let read = read_appearance(&packages, arrangement).and_then(|shown| {
                        let mut every = read_appearance_parts(&packages, arrangement)?;
                        every.sort_by_key(|set| {
                            !shown
                                .iter()
                                .any(|default| default.component == set.component)
                        });
                        Ok(every)
                    });
                    match read {
                        Ok(read) => sets.extend(read),
                        Err(error) => {
                            result = Err(error);
                            break;
                        }
                    }
                }
                let _ = sender.send(result.map(|()| sets));
                ctx.request_repaint();
            })
        };
        self.job = Some(ReadJob {
            arrangements: wanted,
            receiver,
            worker,
        });
    }
}

/// One marker name on the appearance. Rows that share a name move together, so the editor
/// lists names, placed where the first of their rows sits.
struct Point {
    name: u32,
    label: String,
    position: [f32; 3],
    recovered: bool,
    /// For a name that is not recovered, the named marker it sits beside.
    near: Option<String>,
}

fn points(sets: &[MarkerSet]) -> Vec<Point> {
    let mut points: Vec<Point> = Vec::new();
    for marker in sets.iter().flat_map(|set| &set.markers) {
        if points.iter().any(|point| point.name == marker.name) {
            continue;
        }
        points.push(Point {
            name: marker.name,
            label: marker.label(),
            position: marker.position,
            recovered: marker_name(marker.name).is_some(),
            near: nearest_named(sets, marker).map(|near| near.to_string()),
        });
    }
    // Recovered names first, in name order, then the rest by hash.
    points.sort_by(|a, b| (!a.recovered, &a.label, a.name).cmp(&(!b.recovered, &b.label, b.name)));
    points
}

fn offset(overrides: &WeaponRecipeOverrides, name: u32) -> [f32; 3] {
    overrides
        .marker_offsets
        .iter()
        .find(|offset| offset.marker.parse_u32().ok() == Some(name))
        .map_or([0.0; 3], |offset| {
            offset.offset_um.map(|um| um as f32 / MICRONS)
        })
}

/// Records `name`'s offset, dropping it when the marker is back where it started.
fn set_offset(overrides: &mut WeaponRecipeOverrides, name: u32, offset: [f32; 3]) {
    let um = offset.map(|metres| {
        ((metres * MICRONS).round() as i32).clamp(-MARKER_OFFSET_LIMIT_UM, MARKER_OFFSET_LIMIT_UM)
    });
    overrides
        .marker_offsets
        .retain(|kept| kept.marker.parse_u32().ok() != Some(name));
    if um != [0; 3] {
        overrides.marker_offsets.push(MarkerOffsetRecipe {
            marker: name.into(),
            offset_um: um,
        });
        overrides
            .marker_offsets
            .sort_by_key(|kept| kept.marker.parse_u32().ok());
    }
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|axis| a[axis] + b[axis])
}

/// How far the model sits from the hand, in metres.
fn held(overrides: &WeaponRecipeOverrides) -> [f32; 3] {
    overrides.held_offset_um.map(|um| um as f32 / MICRONS)
}

fn set_held(overrides: &mut WeaponRecipeOverrides, metres: [f32; 3]) {
    overrides.held_offset_um = metres.map(|metres| {
        ((metres * MICRONS).round() as i32).clamp(-HELD_OFFSET_LIMIT_UM, HELD_OFFSET_LIMIT_UM)
    });
}

/// The grip dragged by `moved` on the model, which is the model moving the other way in the hand.
fn move_grip(overrides: &mut WeaponRecipeOverrides, moved: [f32; 3]) {
    let current = held(overrides);
    set_held(
        overrides,
        std::array::from_fn(|axis| current[axis] - moved[axis]),
    );
}

/// Forward, Side and Up in centimetres side by side, each captioned, with Reset after them once
/// any is moved. Returns the edited offset in metres when it changed.
fn offset_fields(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    current: [f32; 3],
    limit_um: i32,
) -> Option<[f32; 3]> {
    let limit = limit_um as f32 / 10_000.0;
    let mut edited = current;
    ui.push_id(id, |ui| {
        ui.horizontal(|ui| {
            for (axis, name) in AXES.into_iter().enumerate() {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(egui::RichText::new(name).size(11.0).weak());
                    let mut centimetres = edited[axis] * 100.0;
                    let response = ui.add_sized(
                        [FIELD_WIDTH, ui.spacing().interact_size.y],
                        egui::DragValue::new(&mut centimetres)
                            .speed(0.01)
                            .fixed_decimals(2)
                            .range(-limit..=limit)
                            .clamp_existing_to_range(false)
                            .suffix(" cm"),
                    );
                    if response.changed() {
                        edited[axis] = centimetres / 100.0;
                    }
                    let _ = crate::app::style::named_control(response, name);
                });
            }
            if current != [0.0; 3] {
                // Under an empty caption, so Reset lines up with the fields.
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(egui::RichText::new(" ").size(11.0));
                    if ui.button("Reset").clicked() {
                        edited = [0.0; 3];
                    }
                });
            }
        });
    });
    (edited != current).then_some(edited)
}

impl PackageAuthoringApp {
    /// Placement: the model in the hand, then its markers.
    pub(in crate::app) fn draw_appearance_placement(&mut self, ui: &mut egui::Ui) {
        let overrides = &self.recipe.overrides;
        let placed = !overrides.marker_offsets.is_empty() || overrides.held_offset_um != [0; 3];
        ui.horizontal(|ui| {
            ui.strong("Placement")
                .on_hover_text("Where the model sits in the hand. Test in game");
            if placed {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    crate::app::style::more_menu(ui, "Placement", |ui| {
                        if ui.button("Reset All Placement").clicked() {
                            self.recipe.overrides.marker_offsets.clear();
                            self.recipe.overrides.held_offset_um = [0; 3];
                            ui.close();
                        }
                    });
                });
            }
        });
        ui.add_space(6.0);
        ui.label("In Hand");
        #[cfg(feature = "d2-model-importer")]
        if self.recipe.overrides.imported_graph.is_some() {
            ui.weak("Imported models keep their own place.");
            ui.add_space(12.0);
            self.draw_appearance_markers(ui);
            return;
        }
        let current = self
            .recipe
            .overrides
            .held_offset_um
            .map(|um| um as f32 / MICRONS);
        if let Some(edited) = offset_fields(ui, "in-hand", current, HELD_OFFSET_LIMIT_UM) {
            set_held(&mut self.recipe.overrides, edited);
        }
        ui.add_space(12.0);
        self.draw_appearance_markers(ui);
    }

    fn draw_appearance_markers(&mut self, ui: &mut egui::Ui) {
        #[cfg(feature = "d2-model-importer")]
        if self.recipe.overrides.imported_graph.is_some() {
            ui.label("Markers");
            ui.weak("Imported models keep their own markers.");
            return;
        }
        let geometry = self.current_geometry_donor();
        let Some(arrangements) = self.technical_marker_arrangements(geometry.as_ref()) else {
            ui.label("Markers");
            ui.weak("Choose a base weapon first.");
            return;
        };
        // No read starts while an installation replaces packages.
        if self.install_receiver.is_none() {
            self.marker_editor
                .request(ui.ctx(), &self.packages, &arrangements);
        }
        let read = self
            .marker_editor
            .read
            .as_ref()
            .filter(|(read, _)| *read == arrangements)
            .map(|(_, result)| result);
        let points = match read {
            None => {
                ui.label("Markers");
                ui.weak("Reading\u{2026}");
                return;
            }
            Some(Err(error)) => {
                ui.label("Markers");
                ui.colored_label(ui.visuals().error_fg_color, "Markers unavailable")
                    .on_hover_text(error.as_str());
                return;
            }
            Some(Ok(sets)) => points(sets),
        };
        if points.is_empty() {
            ui.label("Markers");
            ui.weak("This appearance carries none.");
            return;
        }
        let hidden = points.iter().filter(|point| !point.recovered).count();
        ui.label("Markers");
        let shown = points
            .iter()
            .filter(|point| point.recovered || self.marker_editor.show_all)
            .collect::<Vec<_>>();
        if self.marker_editor.selected.is_none_or(|selected| {
            selected != GRIP && !shown.iter().any(|point| point.name == selected)
        }) {
            self.marker_editor.selected = shown.first().map(|point| point.name);
        }
        let wide = ui.available_width() >= 760.0;
        let spacing = ui.spacing().item_spacing.x;
        let preview_width = if wide {
            ((ui.available_width() - spacing) * 0.62).floor()
        } else {
            ui.available_width()
        };
        if wide {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(preview_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(preview_width);
                        self.draw_marker_preview(ui, &shown);
                    },
                );
                let list_width = ui.available_width();
                ui.allocate_ui_with_layout(
                    egui::vec2(list_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(list_width);
                        self.draw_marker_list(ui, &shown, hidden);
                    },
                );
            });
        } else {
            self.draw_marker_preview(ui, &shown);
            ui.add_space(8.0);
            self.draw_marker_list(ui, &shown, hidden);
        }
    }

    /// The model with every listed marker on it, and the angle it is seen from.
    fn draw_marker_preview(&mut self, ui: &mut egui::Ui, shown: &[&Point]) {
        ui.horizontal(|ui| {
            for view in View::ALL {
                if ui
                    .selectable_label(self.marker_editor.view == view, view.label())
                    .clicked()
                    && self.marker_editor.view != view
                {
                    self.marker_editor.view = view;
                    // A new angle starts framed, not zoomed into wherever the last one was.
                    still::frame(ui.ctx(), egui::Id::new(PREVIEW_ID));
                }
            }
        });
        let appearance = self.catalog.as_ref().and_then(|catalog| {
            preview::loadout(catalog, &self.recipe)
                .map(|loadout| catalog.preview_appearance(&loadout))
        });
        let offsets = shown
            .iter()
            .map(|point| offset(&self.recipe.overrides, point.name))
            .collect::<Vec<_>>();
        let mut pins = shown
            .iter()
            .zip(&offsets)
            .map(|(point, moved)| Pin {
                position: add(point.position, *moved),
                label: &point.label,
                selected: self.marker_editor.selected == Some(point.name),
                origin: (*moved != [0.0; 3]).then_some(point.position),
            })
            .collect::<Vec<_>>();
        // The grip last: the model's origin, where the hand holds it. Moving the model in the
        // hand moves the grip the other way on the model.
        let held = held(&self.recipe.overrides);
        #[cfg(feature = "d2-model-importer")]
        let grip = self.recipe.overrides.imported_graph.is_none();
        #[cfg(not(feature = "d2-model-importer"))]
        let grip = true;
        if grip {
            pins.push(Pin {
                position: held.map(|metres| -metres),
                label: "Grip",
                selected: self.marker_editor.selected == Some(GRIP),
                origin: (held != [0.0; 3]).then_some([0.0; 3]),
            });
        }
        let width = ui.available_width();
        let height = (width * 0.56).clamp(240.0, 440.0);
        let view = self.marker_editor.view;
        let pinned = still::show_pins(
            ui,
            egui::Id::new(PREVIEW_ID),
            &self.packages,
            appearance,
            egui::vec2(width, height),
            view,
            &pins,
        );
        if let Some(index) = pinned.pressed {
            self.marker_editor.selected = shown
                .get(index)
                .map(|point| point.name)
                .or((index == shown.len()).then_some(GRIP));
        }
        if let Some((index, moved)) = pinned.moved {
            match shown.get(index) {
                Some(point) => {
                    let current = offset(&self.recipe.overrides, point.name);
                    set_offset(&mut self.recipe.overrides, point.name, add(current, moved));
                }
                None => move_grip(&mut self.recipe.overrides, moved),
            }
        }
        // Arrow keys nudge the selected marker along the screen's axes, the side view's in the
        // orbit, while the pointer is over the model.
        if pinned.response.hovered()
            && let Some(selected) = self.marker_editor.selected
        {
            let (step, keys) = ui.input(|input| {
                let step = if input.modifiers.shift {
                    SHIFT_NUDGE
                } else {
                    NUDGE
                };
                let across = f32::from(i8::from(input.key_pressed(egui::Key::ArrowRight)))
                    - f32::from(i8::from(input.key_pressed(egui::Key::ArrowLeft)));
                let rise = f32::from(i8::from(input.key_pressed(egui::Key::ArrowUp)))
                    - f32::from(i8::from(input.key_pressed(egui::Key::ArrowDown)));
                (step, (across, rise))
            });
            if keys != (0.0, 0.0) {
                let (right, up) = view.axes();
                let moved =
                    std::array::from_fn(|axis| (right[axis] * keys.0 + up[axis] * keys.1) * step);
                if selected == GRIP {
                    move_grip(&mut self.recipe.overrides, moved);
                } else {
                    let current = offset(&self.recipe.overrides, selected);
                    set_offset(&mut self.recipe.overrides, selected, add(current, moved));
                }
            }
        }
        ui.weak(match view {
            View::Orbit => "Double-click to select \u{b7} Drag to turn",
            _ => "Double-click to select \u{b7} Drag to move \u{b7} Arrows 1 mm, Shift 1 cm",
        });
    }

    /// Every listed marker as a row, the selected one opened to its offsets.
    fn draw_marker_list(&mut self, ui: &mut egui::Ui, shown: &[&Point], hidden: usize) {
        let moved = shown
            .iter()
            .filter(|point| offset(&self.recipe.overrides, point.name) != [0.0; 3])
            .count();
        // The count, and the unnamed markers' switch beside the list it changes.
        ui.horizontal(|ui| {
            ui.weak(if moved == 0 {
                format!("{} Markers", shown.len())
            } else {
                format!("{} Markers \u{b7} {moved} Moved", shown.len())
            });
            if hidden > 0 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut self.marker_editor.show_all, "Unnamed")
                        .on_hover_text(format!("{hidden} without a recovered name"));
                });
            }
        });
        for point in shown {
            let selected = self.marker_editor.selected == Some(point.name);
            let current = offset(&self.recipe.overrides, point.name);
            let changed = current != [0.0; 3];
            let response = ui
                .push_id(point.name, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        // A slot of its own for the dot that marks a moved marker, so it never
                        // sits on the preview's edge.
                        let (slot, _) = ui.allocate_exact_size(
                            egui::vec2(8.0, ui.spacing().interact_size.y),
                            egui::Sense::hover(),
                        );
                        if changed {
                            ui.painter().circle_filled(
                                slot.center(),
                                2.5,
                                ui.visuals().text_color(),
                            );
                        }
                        ui.selectable_label(selected, point.label.as_str())
                    })
                    .inner
                })
                .inner;
            let response = match &point.near {
                Some(near) if !point.recovered => response.on_hover_text(near.as_str()),
                _ => response,
            };
            if response.clicked() {
                self.marker_editor.selected = Some(point.name);
            }
            if selected {
                let edited = ui
                    .indent(("marker-offsets", point.name), |ui| {
                        offset_fields(ui, point.name, current, MARKER_OFFSET_LIMIT_UM)
                    })
                    .inner;
                if let Some(edited) = edited {
                    set_offset(&mut self.recipe.overrides, point.name, edited);
                }
            }
        }
    }
}

type MarkerRead = (Vec<u16>, Result<Vec<MarkerSet>, String>);
