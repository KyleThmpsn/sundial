//! The Details window beside a model: its counts and notes, then the Assets and Effects browser,
//! what the object uses, listed beside one card for the chosen entry. The browser alone is the
//! whole preview of an object with nothing to draw.
use super::*;

/// A list longer than this gets a filter.
const FILTER_ABOVE: usize = 12;
/// Below this width the list sits above the card instead of beside it.
const SIDE_BY_SIDE: f32 = 560.0;

/// One entry, by its place in the model's lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Entry {
    Texture,
    Node(usize),
    Particle(usize),
    Sound(usize),
    Light(usize),
    Clip(usize),
    Child(usize),
    Component(usize),
    Reference(usize),
}

impl Entry {
    fn kind(self) -> &'static str {
        match self {
            Self::Texture => "Texture",
            Self::Node(_) => "Effect Node",
            Self::Particle(_) => "Particle System",
            Self::Sound(_) => "Sound Event",
            Self::Light(_) => "Light",
            Self::Clip(_) => "Animation Clip",
            Self::Child(_) => "Child Object",
            Self::Component(_) => "Component",
            Self::Reference(_) => "Resource Handle",
        }
    }

    /// Whether its label is a name worth carrying into the preview's heading.
    fn named(self) -> bool {
        matches!(
            self,
            Self::Particle(_) | Self::Sound(_) | Self::Clip(_) | Self::Reference(_)
        )
    }
}

/// A line of the list: a section's name and count, or one of its entries.
enum Row {
    Section(&'static str, usize),
    Entry(Entry, String),
}

fn hex(tag: u32) -> String {
    format!("0x{tag:08X}")
}

/// The model's entries by section, in the order the list shows them. Clips are listed only for
/// an object without a mesh, since a model plays them from its transport.
fn sections(model: &Model) -> Vec<(&'static str, Vec<Entry>)> {
    let assets = &model.assets;
    let clips = if model.triangles.is_empty() {
        model.clips.len()
    } else {
        0
    };
    let mut sections: Vec<(&'static str, Vec<Entry>)> = Vec::new();
    let mut add = |name: &'static str, count: usize, entry: fn(usize) -> Entry| {
        if count > 0 {
            sections.push((name, (0..count).map(entry).collect()));
        }
    };
    add("Texture", usize::from(assets.image.is_some()), |_| {
        Entry::Texture
    });
    add("Effect Sequence", assets.effect_nodes.len(), Entry::Node);
    add("Particle Systems", assets.particles.len(), Entry::Particle);
    add("Sound Events", assets.sounds.len(), Entry::Sound);
    add("Lights", assets.lights.len(), Entry::Light);
    add("Animation Clips", clips, Entry::Clip);
    add("Child Objects", assets.children.len(), Entry::Child);
    add("Components", assets.components.len(), Entry::Component);
    add(
        "Resource Handles",
        assets.references.len(),
        Entry::Reference,
    );
    sections
}

/// How many entries the list holds.
pub(super) fn count(model: &Model) -> usize {
    sections(model)
        .iter()
        .map(|(_, entries)| entries.len())
        .sum()
}

/// How an entry reads in the list and at the top of its card.
fn label(model: &Model, entry: Entry) -> String {
    let assets = &model.assets;
    let named = |name: &Option<String>, tag: u32| name.clone().unwrap_or_else(|| hex(tag));
    match entry {
        Entry::Texture => assets
            .image
            .as_ref()
            .map_or_else(|| "Texture".to_owned(), |texture| hex(texture.tag)),
        Entry::Node(index) => {
            let node = &assets.effect_nodes[index];
            format!("{}. {}", node.index + 1, node.kind())
        }
        Entry::Particle(index) => {
            let particle = &assets.particles[index];
            named(&particle.name, particle.tag)
        }
        Entry::Sound(index) => {
            let sound = &assets.sounds[index];
            named(&sound.name, sound.tag)
        }
        Entry::Light(index) => format!("Light {}", index + 1),
        Entry::Clip(index) => model.clips[index].name.clone(),
        Entry::Child(index) => hex(assets.children[index]),
        Entry::Component(index) => hex(assets.components[index].tag),
        Entry::Reference(index) => {
            let reference = &assets.references[index];
            named(&reference.name, reference.tag)
        }
    }
}

/// The resource an entry opens in the preview. A texture is the object's own image.
fn target(model: &Model, entry: Entry) -> Option<u32> {
    let assets = &model.assets;
    match entry {
        Entry::Texture => None,
        Entry::Node(index) => assets.effect_nodes[index].target,
        Entry::Particle(index) => Some(assets.particles[index].tag),
        Entry::Sound(index) => Some(assets.sounds[index].tag),
        Entry::Light(index) => Some(assets.lights[index].tag),
        Entry::Clip(index) => Some(model.clips[index].tag),
        Entry::Child(index) => Some(assets.children[index]),
        Entry::Component(index) => Some(assets.components[index].tag),
        Entry::Reference(index) => Some(assets.references[index].tag),
    }
}

/// Name and value rows, names muted.
fn facts(ui: &mut egui::Ui, id: impl std::hash::Hash, rows: &[(&str, String)]) {
    if rows.is_empty() {
        return;
    }
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            for (name, value) in rows {
                ui.weak(*name);
                ui.label(value);
                ui.end_row();
            }
        });
}

impl Preview {
    /// The preview of an object with nothing to draw: its notices, then the browser.
    pub(super) fn draw_assets(&mut self, ui: &mut egui::Ui, model: &Model) {
        self.poll_saving(ui.ctx());
        self.poll_audio(ui.ctx());
        let assets = &model.assets;
        let silent = assets.sounds.is_empty() && assets.lights.is_empty();
        if model.triangles.is_empty()
            && model.particle_sources.is_empty()
            && assets.image.is_none()
            && (!assets.particles.is_empty() || silent)
        {
            ui.weak("No Visual Output");
        }
        for notice in &model.notices {
            ui.weak(notice);
        }
        ui.collapsing("Source Details", |ui| {
            facts(
                ui,
                "asset-source",
                &[
                    ("Resource", hex(assets.source)),
                    ("Class", hex(assets.class)),
                    ("Type", assets.file_type.to_string()),
                    ("Size", format!("{} bytes", assets.size)),
                ],
            );
        });
        ui.add_space(4.0);
        self.draw_asset_browser(ui, model);
    }

    /// The Details window, on the object the preview shows, with Back while the preview is
    /// inside one of its resources.
    pub(super) fn draw_details_window(&mut self, ui: &mut egui::Ui, name: &str) {
        self.poll_saving(ui.ctx());
        self.poll_audio(ui.ctx());
        ui.horizontal(|ui| {
            if !self.navigation.is_empty() && ui.button("Back").clicked() {
                self.navigation.pop();
                ui.ctx().request_repaint_of(window::viewport_id());
            }
            ui.add(egui::Label::new(egui::RichText::new(name).heading()).truncate())
                .on_hover_text(name);
        });
        ui.add_space(4.0);
        let Some(model) = self.model.clone() else {
            ui.weak(if self.request.is_none() || self.error.is_some() {
                "No Model"
            } else {
                "Loading"
            });
            return;
        };
        self.draw_model_details(ui, &model);
        if count(&model) > 0 {
            ui.add_space(8.0);
            ui.separator();
            ui.strong("Assets and Effects");
            ui.add_space(4.0);
            self.draw_asset_browser(ui, &model);
        }
    }

    /// The model's counts on one line, then what the preview leaves out of it.
    fn draw_model_details(&self, ui: &mut egui::Ui, model: &Model) {
        let mut counts = vec![
            format!("{} Triangles", model.triangles.len()),
            format!("{} Vertices", model.vertices.len()),
            format!("{} Meshes", model.tags.len()),
            format!("{} Textures", model.textures.len()),
        ];
        if let Some(elapsed) = self.load_time {
            counts.push(format!("Loaded in {:.2} s", elapsed.as_secs_f32()));
        }
        ui.label(counts.join(" · "));
        let mut notes = Vec::new();
        if model.particle_geometry {
            notes.push(if model.has_particle_material_study() {
                "One static instance evaluated"
            } else if model.particle_sources.is_empty() {
                "Spawn, motion and timing not shown"
            } else {
                "Packaged sprites · native timing not mapped"
            });
        } else if !model.particle_sources.is_empty() {
            notes.push("Packaged sprites at the point emitter · native timing not mapped");
        }
        if model.light_geometry {
            notes.push(if model.has_surface_mesh() {
                "Linked light volumes listed below"
            } else {
                "Outline only · illumination not simulated"
            });
        }
        if !model.particle_geometry && !model.light_geometry && model.particle_sources.is_empty() {
            notes.push("Approximate lighting · no runtime physics");
        }
        for note in notes {
            ui.weak(note);
        }
        for notice in model.animation_notice.iter().chain(&model.notices) {
            ui.weak(notice);
        }
    }

    /// The list beside the card for its selection, or above it in a narrow space.
    fn draw_asset_browser(&mut self, ui: &mut egui::Ui, model: &Model) {
        let sections = sections(model);
        let Some(first) = sections
            .first()
            .and_then(|(_, entries)| entries.first().copied())
        else {
            ui.weak("No Previewable Components");
            return;
        };
        let listed = |entry: Entry| sections.iter().any(|(_, entries)| entries.contains(&entry));
        if !self.asset_entry.is_some_and(listed) {
            self.asset_entry = Some(first);
        }
        let id = ui.id().with("asset-browser");
        let width = ui.available_width();
        if width >= SIDE_BY_SIDE {
            egui::SidePanel::left(id.with("list"))
                .resizable(true)
                .default_width(280.0)
                .width_range(180.0..=(width * 0.5).max(180.0))
                .show_inside(ui, |ui| self.draw_asset_list(ui, model, &sections));
        } else {
            egui::TopBottomPanel::top(id.with("list-stacked"))
                .resizable(true)
                .default_height(ui.available_height() * 0.4)
                .show_inside(ui, |ui| self.draw_asset_list(ui, model, &sections));
        }
        egui::ScrollArea::vertical()
            .id_salt(id.with("card"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(entry) = self.asset_entry {
                    self.draw_asset_card(ui, model, entry);
                }
            });
    }

    /// Every entry under its section, the filter above them once there are many. A double
    /// click opens the entry in the preview.
    fn draw_asset_list(
        &mut self,
        ui: &mut egui::Ui,
        model: &Model,
        sections: &[(&'static str, Vec<Entry>)],
    ) {
        let total = sections
            .iter()
            .map(|(_, entries)| entries.len())
            .sum::<usize>();
        let query = if total > FILTER_ABOVE {
            let filter = ui.add(
                egui::TextEdit::singleline(&mut self.asset_query)
                    .hint_text(format!(
                        "{} Filter",
                        egui_phosphor::regular::MAGNIFYING_GLASS
                    ))
                    .desired_width(f32::INFINITY),
            );
            // The hint is a placeholder, so the field takes its name here.
            filter.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Filter")
            });
            ui.add_space(4.0);
            self.asset_query.trim().to_lowercase()
        } else {
            String::new()
        };
        let mut rows = Vec::new();
        for (name, entries) in sections {
            let shown = entries
                .iter()
                .map(|&entry| (entry, label(model, entry)))
                .filter(|(entry, label)| {
                    query.is_empty()
                        || label.to_lowercase().contains(&query)
                        || target(model, *entry)
                            .is_some_and(|tag| hex(tag).to_lowercase().contains(&query))
                })
                .collect::<Vec<_>>();
            if shown.is_empty() {
                continue;
            }
            rows.push(Row::Section(name, shown.len()));
            rows.extend(
                shown
                    .into_iter()
                    .map(|(entry, label)| Row::Entry(entry, label)),
            );
        }
        if rows.is_empty() {
            ui.weak("No Matches");
            return;
        }
        let height = ui.spacing().interact_size.y;
        let mut opened = None;
        egui::ScrollArea::vertical()
            .id_salt("asset-list")
            .auto_shrink([false, false])
            .show_rows(ui, height, rows.len(), |ui, range| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                // Justified rows fill the list's width with their text at the left.
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                    for row in &rows[range] {
                        match row {
                            Row::Section(name, count) => {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(ui.available_width(), height),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.strong(*name);
                                        ui.weak(count.to_string());
                                    },
                                );
                            }
                            Row::Entry(entry, label) => {
                                let selected = self.asset_entry == Some(*entry);
                                let response = ui
                                    .add(egui::SelectableLabel::new(selected, label))
                                    .on_hover_text(label);
                                if response.clicked() {
                                    self.asset_entry = Some(*entry);
                                }
                                if response.double_clicked() {
                                    opened = Some(*entry);
                                }
                            }
                        }
                    }
                });
            });
        if let Some(entry) = opened
            && let Some(tag) = target(model, entry).filter(|tag| *tag != model.assets.source)
        {
            let name = label(model, entry);
            self.open_asset(ui.ctx(), tag, entry.named().then_some(name.as_str()));
        }
    }

    /// Shows `tag` in the preview, which Back returns from.
    fn open_asset(&mut self, ctx: &egui::Context, tag: u32, name: Option<&str>) {
        self.browse(tag, name);
        ctx.request_repaint_of(window::viewport_id());
    }

    /// The chosen entry: its name, kind and resource, Open in Preview, then what it holds.
    fn draw_asset_card(&mut self, ui: &mut egui::Ui, model: &Model, entry: Entry) {
        let assets = &model.assets;
        let title = label(model, entry);
        let opens = target(model, entry);
        ui.add(egui::Label::new(egui::RichText::new(&title).heading()).truncate())
            .on_hover_text(&title);
        ui.horizontal(|ui| {
            ui.weak(entry.kind());
            if let Some(tag) = opens.filter(|tag| hex(*tag) != title) {
                ui.weak(hex(tag));
            }
        });
        if let Some(tag) = opens.filter(|tag| *tag != assets.source) {
            ui.add_space(4.0);
            if ui.button("Open in Preview").clicked() {
                self.open_asset(ui.ctx(), tag, entry.named().then_some(title.as_str()));
            }
        }
        ui.add_space(8.0);
        match entry {
            Entry::Texture => {
                if let Some(texture) = &assets.image {
                    let caption = format!("{} × {} px", texture.size[0], texture.size[1]);
                    self.draw_asset_texture(ui, texture.tag, texture.size, &texture.rgba, &caption);
                }
            }
            Entry::Node(index) => {
                let node = &assets.effect_nodes[index];
                let mut rows = vec![("Class", hex(node.class))];
                if let Some(timing) = node.timing {
                    rows.push(("Start", format!("{:.4} s", timing.start)));
                    rows.push(("Duration", format!("{:.4} s", timing.duration)));
                }
                facts(ui, ("asset-node", index), &rows);
                let component = (node.source != assets.source).then_some(node.source);
                self.draw_links(
                    ui,
                    ("asset-node-links", index),
                    &[("Effect Component", component)],
                );
            }
            Entry::Particle(index) => self.draw_particle_card(ui, &assets.particles[index]),
            Entry::Sound(index) => self.draw_sound_card(ui, &assets.sounds[index]),
            Entry::Light(index) => {
                let light = &assets.lights[index];
                let mut rows = vec![
                    ("Range", format!("{:.2}", light.radius)),
                    (
                        "Volume Offset",
                        format!(
                            "{:.2}, {:.2}, {:.2}",
                            light.volume_offset[0], light.volume_offset[1], light.volume_offset[2]
                        ),
                    ),
                ];
                if light.half_fov > 0.0 {
                    rows.push((
                        "Half Field of View",
                        format!("{:.1}°", light.half_fov.to_degrees()),
                    ));
                }
                facts(ui, ("asset-light", index), &rows);
            }
            Entry::Clip(_) | Entry::Child(_) => {}
            Entry::Component(index) => {
                let component = &assets.components[index];
                let class = |class: Option<u32>| class.map_or_else(|| "Unknown".to_owned(), hex);
                facts(
                    ui,
                    ("asset-component", index),
                    &[
                        ("Header Class", class(component.header)),
                        ("Data Class", class(component.data)),
                    ],
                );
            }
            Entry::Reference(index) => {
                let reference = &assets.references[index];
                facts(
                    ui,
                    ("asset-reference", index),
                    &[
                        ("Class", hex(reference.class)),
                        ("Type", reference.file_type.to_string()),
                    ],
                );
            }
        }
    }

    /// Resources an entry names, each with its own Open. Missing ones are left out.
    fn draw_links(
        &mut self,
        ui: &mut egui::Ui,
        id: impl std::hash::Hash,
        links: &[(&str, Option<u32>)],
    ) {
        let links = links
            .iter()
            .filter_map(|&(name, tag)| Some((name, tag?)))
            .collect::<Vec<_>>();
        if links.is_empty() {
            return;
        }
        ui.add_space(8.0);
        ui.strong("Linked");
        egui::Grid::new(id)
            .num_columns(3)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                for (name, tag) in links {
                    ui.weak(name);
                    ui.label(hex(tag));
                    if ui.small_button("Open").clicked() {
                        self.open_asset(ui.ctx(), tag, None);
                    }
                    ui.end_row();
                }
            });
    }

    fn draw_particle_card(
        &mut self,
        ui: &mut egui::Ui,
        particle: &model_preview::assets::Particle,
    ) {
        self.draw_links(
            ui,
            ("asset-particle-links", particle.tag),
            &[
                ("Definition", particle.definition),
                ("Emitter", particle.emitter),
                ("Particle Mesh", particle.emitter_model),
                ("Material", particle.material),
            ],
        );
        if let Some(gradient) = &particle.gradient {
            ui.add_space(8.0);
            self.draw_asset_gradient(ui, gradient.tag, gradient.size, &gradient.rgba);
        }
        if !particle.material_textures.is_empty() {
            ui.add_space(8.0);
            ui.strong("Material Textures");
            ui.horizontal_wrapped(|ui| {
                for (slot, texture) in &particle.material_textures {
                    ui.vertical(|ui| {
                        self.draw_asset_texture(
                            ui,
                            texture.tag,
                            texture.size,
                            &texture.rgba,
                            &format!("Slot {slot} · {}", hex(texture.tag)),
                        );
                    });
                }
            });
        }
        if particle.material_slot_omissions > 0 {
            ui.weak(format!(
                "{} more texture slots not shown",
                particle.material_slot_omissions
            ));
        }
        ui.add_space(8.0);
        draw_particle_program(ui, particle);
        if !particle.compute_passes.is_empty() {
            ui.collapsing("Native Compute Passes", |ui| {
                for pass in &particle.compute_passes {
                    ui.label(format!(
                        "{}: shader {}, material {}, {} bytes",
                        pass.phase,
                        hex(pass.shader),
                        hex(pass.material),
                        pass.size
                    ));
                }
            });
        }
        if !particle.material_samplers.is_empty() {
            ui.collapsing("Pixel Samplers", |ui| {
                for (index, sampler) in particle.material_samplers.iter().enumerate() {
                    ui.label(format!(
                        "Sampler {}: U {:?}, V {:?}",
                        index + 1,
                        sampler.u,
                        sampler.v
                    ));
                }
            });
        }
        if let Some(notice) = &particle.notice {
            ui.weak(notice);
        }
    }

    /// A sound's clips, each with Play and its saves, then how the last save or playback went.
    fn draw_sound_card(&mut self, ui: &mut egui::Ui, sound: &model_preview::assets::Sound) {
        if !sound.clips.is_empty() {
            ui.strong("Clips");
        }
        for clip in &sound.clips {
            ui.horizontal_wrapped(|ui| {
                ui.label(clip.name.clone().unwrap_or_else(|| hex(clip.tag)));
                ui.weak(format!("{} · {} bytes", clip.format(), clip.size));
                if self.audio_tag == Some(clip.tag)
                    && (self.playback.is_some() || self.audio_pending.is_some())
                {
                    let label = if self.audio_pending.is_some() {
                        "Cancel"
                    } else {
                        "Stop"
                    };
                    if ui.button(label).clicked() {
                        self.stop_audio();
                        self.audio_status = None;
                    }
                } else if ui
                    .add_enabled(
                        cfg!(windows) && self.audio_pending.is_none(),
                        egui::Button::new("Play"),
                    )
                    .clicked()
                {
                    self.play_audio(ui.ctx(), clip.tag);
                }
                if ui
                    .add_enabled(self.saving.is_none(), egui::Button::new("Save WAV…"))
                    .clicked()
                {
                    self.save_audio(ui.ctx(), clip.tag, true);
                }
                if ui
                    .add_enabled(self.saving.is_none(), egui::Button::new("Save Source…"))
                    .clicked()
                {
                    self.save_audio(ui.ctx(), clip.tag, false);
                }
            });
        }
        if let Some(notice) = &sound.notice {
            ui.weak(notice);
        }
        if self.saving.is_some() {
            ui.weak("Saving…");
        }
        for status in self.status.iter().chain(&self.audio_status) {
            ui.label(status);
        }
    }
}
