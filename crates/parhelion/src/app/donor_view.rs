//! Installed donor selection and provenance controls.
use super::*;
use sundial::package_authoring::{WeaponDyeColors, load_weapon_dye_colors};

pub(super) mod markers;
pub(super) mod ornaments;
pub(super) mod parts;
pub(in crate::app) mod preview;

const DYE_ARRAY_NAMES: [&str; 3] = ["Custom Dyes", "Default Dyes", "Locked Dyes"];

type DyeColorResults = BTreeMap<u16, Result<WeaponDyeColors, String>>;

#[derive(Default)]
pub(super) struct DyeColors {
    colors: DyeColorResults,
    job: Option<DyeColorJob>,
}

struct DyeColorJob {
    indices: Vec<u16>,
    worker: thread::JoinHandle<DyeColorResults>,
}

impl Drop for DyeColors {
    fn drop(&mut self) {
        // Match the library-preview lifetime: no package handles may survive catalog release.
        if let Some(job) = self.job.take() {
            let _ = job.worker.join();
        }
    }
}

impl DyeColors {
    pub(super) fn update(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        rows: &[Vec<WeaponDyeReferenceRecipe>; 3],
    ) {
        if let Some(job) = self.job.take_if(|job| job.worker.is_finished()) {
            let colors = job.worker.join().unwrap_or_else(|_| {
                job.indices
                    .into_iter()
                    .map(|index| (index, Err("Dye color loading stopped unexpectedly".into())))
                    .collect()
            });
            self.colors.extend(colors);
        }
        let indices: BTreeSet<_> = rows
            .iter()
            .flatten()
            .filter(|row| row.channel_index >= 0)
            .map(|row| row.dye_reference_index)
            .collect();
        self.colors.retain(|index, _| indices.contains(index));
        if self.job.is_none() && !packages.as_os_str().is_empty() {
            let missing: Vec<_> = indices
                .into_iter()
                .filter(|index| !self.colors.contains_key(index))
                .collect();
            if !missing.is_empty() {
                let packages = packages.to_owned();
                let ctx = ctx.clone();
                let indices = missing.clone();
                let worker = thread::spawn(move || {
                    let colors =
                        load_weapon_dye_colors(&packages, &missing).unwrap_or_else(|error| {
                            missing
                                .into_iter()
                                .map(|index| (index, Err(error.clone())))
                                .collect()
                        });
                    ctx.request_repaint();
                    colors
                });
                self.job = Some(DyeColorJob { indices, worker });
            }
        }
        if self.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    pub(super) fn draw(&self, ui: &mut egui::Ui, row: &WeaponDyeReferenceRecipe) {
        if row.channel_index < 0 {
            ui.weak("Disabled");
            return;
        }
        match self.colors.get(&row.dye_reference_index) {
            Some(Ok(colors)) => {
                ui.horizontal(|ui| {
                    for (label, rgb) in
                        [("Primary", colors.primary), ("Secondary", colors.secondary)]
                    {
                        let color = egui::Color32::from(egui::Rgba::from_rgb(
                            rgb[0].min(1.0),
                            rgb[1].min(1.0),
                            rgb[2].min(1.0),
                        ));
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(25.0, 20.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 3.0, color);
                        ui.painter().rect_stroke(
                            rect,
                            3.0,
                            ui.visuals().widgets.noninteractive.bg_stroke,
                            egui::StrokeKind::Inside,
                        );
                        response.on_hover_ui(|ui| {
                            sundial::investment::tooltip_title(ui, format!("{label} Albedo"));
                            ui.label(format!(
                                "#{:02X}{:02X}{:02X}\nLinear RGB: {:.4}, {:.4}, {:.4}",
                                color.r(),
                                color.g(),
                                color.b(),
                                rgb[0],
                                rgb[1],
                                rgb[2]
                            ));
                        });
                    }
                });
            }
            Some(Err(error)) => {
                ui.weak("Unavailable").on_hover_text(error);
            }
            None => {
                ui.weak("Loading…");
            }
        }
    }
}

impl PackageAuthoringApp {
    /// The width of Gameplay's Parts labels, which the Reload row shares.
    pub(super) fn component_label_width(ui: &egui::Ui) -> f32 {
        parts::label_width(ui, &parts::GAMEPLAY_COLUMN)
    }

    /// The width of the Appearance tab's Model and Animations labels.
    pub(super) fn appearance_label_width(ui: &egui::Ui) -> f32 {
        parts::label_width(ui, &parts::APPEARANCE_COLUMN)
    }

    /// Shows `page`, from its top.
    pub(super) fn open_page(&mut self, page: WorkbenchPage) {
        self.workbench_page = page;
        self.scroll_recipe_to_top = true;
    }

    /// The Gameplay parts another weapon supplies, by their row names.
    fn borrowed_gameplay_parts(&self) -> Vec<&'static str> {
        use sundial::package_authoring::entity::{
            WEAPON_BARREL_COMPONENT_KEY, WEAPON_MAGAZINE_COMPONENT_KEY,
            WEAPON_TRIGGER_COMPONENT_KEY,
        };
        let overrides = &self.recipe.overrides;
        // Behavior has its own dropdown on the Weapon tab, so the note leaves it out.
        let mut parts = Vec::new();
        if overrides.type_marker_donor.is_some() {
            parts.push("Type Markers");
        }
        for (binding, name) in [
            (WEAPON_TRIGGER_COMPONENT_KEY, "Firing Behavior"),
            (WEAPON_BARREL_COMPONENT_KEY, "Barrel"),
            (WEAPON_MAGAZINE_COMPONENT_KEY, "Magazine"),
        ] {
            if self.recipe.component_splice(binding).is_some() {
                parts.push(name);
            }
        }
        if !self.recipe.runtime_component_donors.is_empty() {
            parts.push("Runtime Components");
        }
        parts
    }

    /// The Weapon tab's Behavior dropdown, in the profile grid. It sets the same choice as
    /// Gameplay's Behavior row, whose details stay on Gameplay.
    pub(super) fn draw_behavior_cell(&mut self, ui: &mut egui::Ui, donor: Option<&WeaponDonor>) {
        let behaviors = unique_behavior_sources(donor);
        let base_name = donor.map_or("Base Weapon", |donor| donor.summary.name.as_str());
        if behaviors.is_empty() {
            ui.label("Behavior");
            ui.add_enabled(
                false,
                egui::Button::new(format!("{base_name} (base weapon)"))
                    .truncate()
                    .min_size(egui::vec2(ui.available_width(), 0.0)),
            );
            return;
        }
        draw_unique_behavior_control(
            ui,
            &mut self.recipe.overrides,
            &behaviors,
            (self.catalog.as_ref(), &self.donor_summaries),
            &mut self.behavior_query,
            base_name,
            true,
        );
    }

    /// Gameplay's Behavior row, and what the chosen behavior brings under it.
    pub(super) fn draw_behavior_part(&mut self, ui: &mut egui::Ui, donor: Option<&WeaponDonor>) {
        let behaviors = unique_behavior_sources(donor);
        if behaviors.is_empty() {
            return;
        }
        let base_name = donor.map_or("Base Weapon", |donor| donor.summary.name.as_str());
        draw_unique_behavior_control(
            ui,
            &mut self.recipe.overrides,
            &behaviors,
            (self.catalog.as_ref(), &self.donor_summaries),
            &mut self.behavior_query,
            base_name,
            false,
        );
        // What the choice brings sits under it, in line with the row's value. Nothing is drawn
        // for the base weapon's own, so the rows keep one rhythm.
        if selected_unique_behavior(&self.recipe.overrides).is_some() {
            ui.horizontal(|ui| {
                ui.add_space(parts::label_width(ui, &parts::GAMEPLAY_COLUMN));
                ui.vertical(|ui| draw_unique_behavior_details(ui, &mut self.recipe.overrides));
            });
        }
    }

    pub(super) fn draw_runtime_source_summary(&self, ui: &mut egui::Ui) {
        let gameplay = self
            .recipe
            .donor
            .expected_name
            .as_deref()
            .unwrap_or("Gameplay donor");
        let runtime = self
            .recipe
            .overrides
            .weapon_pattern_donor_hash
            .as_ref()
            .and_then(|hash| hash.parse_u32().ok())
            .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash))
            .map_or(gameplay, |donor| donor.name.as_str());
        let borrowed = self.recipe.runtime_component_donors.len()
            + self.recipe.overrides.component_splices.len();
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("Base weapon: {gameplay}"));
            ui.separator();
            ui.label(format!("Runtime source: {runtime}"));
            // A moved rig is the appearance's, and stats still convert as the base weapon's type.
            if let Some(name) = self.runtime_rig_appearance.map(|hash| {
                self.donor_summaries
                    .iter()
                    .find(|donor| donor.hash == hash)
                    .map_or_else(|| format!("0x{hash:08X}"), |donor| donor.name.clone())
            }) {
                ui.separator();
                ui.label(format!("Rig: {name}"));
            }
            if borrowed > 0 {
                ui.separator();
                ui.label(if borrowed == 1 {
                    "1 component from another weapon".to_owned()
                } else {
                    format!("{borrowed} components from other weapons")
                });
            }
        });
    }

    pub(super) fn draw_translation_overrides(
        &mut self,
        ui: &mut egui::Ui,
        geometry_donor: Option<&WeaponDonor>,
        render_gear_donor: Option<&WeaponDonor>,
    ) {
        draw_donor_section_label(
            ui,
            "Art Variants",
            Some("Class -1 applies to every class. 0, 1 and 2 are per class."),
        );
        let inherited_art = geometry_donor
            .map(|donor| donor.art_arrangements.as_slice())
            .unwrap_or_default();
        if self.recipe.overrides.art_arrangements.is_none() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "Inheriting {} art {}",
                    inherited_art.len(),
                    if inherited_art.len() == 1 {
                        "row"
                    } else {
                        "rows"
                    }
                ));
                if ui.button("Edit Art Rows").clicked() {
                    self.recipe.overrides.art_arrangements = Some(
                        inherited_art
                            .iter()
                            .map(|row| WeaponArtArrangementRecipe {
                                character_class: row.character_class,
                                arrangement: row.arrangement,
                            })
                            .collect(),
                    );
                }
            });
        } else {
            let mut remove = None;
            let restore = ui.button("Restore Geometry Donor").clicked();
            if restore {
                self.recipe.overrides.art_arrangements = None;
            } else {
                let rows = self
                    .recipe
                    .overrides
                    .art_arrangements
                    .as_mut()
                    .expect("checked above");
                if rows.len() < 4 && ui.button("+ Add Art Row").clicked() {
                    let class = (-1..=2)
                        .find(|class| !rows.iter().any(|row| row.character_class == *class))
                        .unwrap_or(-1);
                    rows.push(WeaponArtArrangementRecipe {
                        character_class: class,
                        arrangement: 0,
                    });
                }
            }
            if let Some(rows) = self.recipe.overrides.art_arrangements.as_mut() {
                egui::Grid::new("translation_art_rows")
                    .num_columns(3)
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        ui.weak("Class");
                        ui.weak("Art-Variant Index");
                        ui.end_row();
                        for (index, row) in rows.iter_mut().enumerate() {
                            ui.add(egui::DragValue::new(&mut row.character_class).range(-1..=2));
                            ui.add(
                                egui::DragValue::new(&mut row.arrangement).range(0..=u16::MAX - 1),
                            );
                            if ui.button("×").on_hover_text("Remove art row").clicked() {
                                remove = Some(index);
                            }
                            ui.end_row();
                        }
                    });
                if let Some(index) = remove {
                    rows.remove(index);
                }
            }
        }

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(5.0);
        draw_donor_section_label(
            ui,
            "Render Dyes",
            Some(
                "Custom, default and locked dye rows. Channel -1 is disabled. Mismatched rows can lose materials.",
            ),
        );
        let inherited_dyes: [Vec<WeaponDyeReferenceRecipe>; 3] = std::array::from_fn(|array| {
            render_gear_donor
                .map(|donor| donor.render_dye_rows[array].as_slice())
                .unwrap_or_default()
                .iter()
                .map(|row| WeaponDyeReferenceRecipe {
                    channel_index: row.channel_index,
                    dye_reference_index: row.dye_reference_index,
                })
                .collect()
        });
        self.dye_colors.update(
            ui.ctx(),
            &self.packages,
            self.recipe
                .overrides
                .render_dye_rows
                .as_ref()
                .unwrap_or(&inherited_dyes),
        );
        ui.label("Swatches show base colors before lighting.");
        if self.recipe.overrides.render_dye_rows.is_none() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "Inheriting dye counts {}/{}/{}",
                    inherited_dyes[0].len(),
                    inherited_dyes[1].len(),
                    inherited_dyes[2].len()
                ));
                if ui.button("Edit Dye Rows").clicked() {
                    self.recipe.overrides.render_dye_rows = Some(inherited_dyes.clone());
                }
            });
            for (array, rows) in inherited_dyes.iter().enumerate() {
                if rows.is_empty() {
                    continue;
                }
                ui.strong(DYE_ARRAY_NAMES[array]);
                for row in rows {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "Channel {} · reference {}",
                            row.channel_index, row.dye_reference_index
                        ));
                        self.dye_colors.draw(ui, row);
                    });
                }
            }
            return;
        }

        if ui.button("Restore Render-Gear Donor").clicked() {
            self.recipe.overrides.render_dye_rows = None;
            return;
        }
        let arrays = self
            .recipe
            .overrides
            .render_dye_rows
            .as_mut()
            .expect("checked above");
        for (array, rows) in arrays.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.strong(DYE_ARRAY_NAMES[array]);
                ui.weak(format!(
                    "{} {}",
                    rows.len(),
                    if rows.len() == 1 { "row" } else { "rows" }
                ));
                if rows.len() < 32 && ui.small_button("+ Add Row").clicked() {
                    rows.push(WeaponDyeReferenceRecipe {
                        channel_index: -1,
                        dye_reference_index: 0,
                    });
                }
            });
            let mut remove = None;
            egui::Grid::new(("translation_dye_array", array))
                .num_columns(4)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.weak("Channel Index");
                    ui.weak("Dye Reference");
                    ui.weak("Base Colors");
                    ui.end_row();
                    for (index, row) in rows.iter_mut().enumerate() {
                        ui.add(egui::DragValue::new(&mut row.channel_index));
                        ui.add(egui::DragValue::new(&mut row.dye_reference_index));
                        self.dye_colors.draw(ui, row);
                        if ui
                            .button("×")
                            .on_hover_text("Remove dye-reference row")
                            .clicked()
                        {
                            remove = Some(index);
                        }
                        ui.end_row();
                    }
                });
            if let Some(index) = remove {
                rows.remove(index);
            }
            ui.add_space(3.0);
        }
    }

    pub(super) fn draw_weapon_pattern_picker(
        &mut self,
        ui: &mut egui::Ui,
        gameplay_donor: Option<&WeaponDonor>,
    ) {
        draw_donor_section_label(
            ui,
            "Firing & Runtime Baseline",
            Some("Sets firing and weapon behavior. The appearance is kept. Test in game."),
        );
        let inherited_summary = gameplay_donor.map(|donor| &donor.summary);
        let override_index = self.recipe.overrides.weapon_pattern_index;
        let source_hash = self
            .recipe
            .overrides
            .weapon_pattern_donor_hash
            .as_ref()
            .and_then(|hash| hash.parse_u32().ok());
        let selected_summary = override_index
            .and_then(|index| {
                source_hash.and_then(|hash| {
                    self.donor_summaries.iter().find(|donor| {
                        donor.hash == hash && donor.weapon_pattern_index == Some(index)
                    })
                })
            })
            .or_else(|| {
                override_index
                    .is_none()
                    .then_some(inherited_summary)
                    .flatten()
            });
        let selected_text = match override_index {
            Some(index) => selected_summary.map_or_else(
                || {
                    let representatives = self
                        .donor_summaries
                        .iter()
                        .filter(|donor| donor.weapon_pattern_index == Some(index))
                        .count();
                    if representatives == 0 {
                        format!("Runtime row {index} · not in catalog")
                    } else {
                        format!(
                            "Runtime row {index} · {representatives} stock {}",
                            if representatives == 1 {
                                "weapon"
                            } else {
                                "weapons"
                            }
                        )
                    }
                },
                |donor| {
                    format!(
                        "{} · Runtime row {index} · 0x{:08X}",
                        donor.name, donor.hash
                    )
                },
            ),
            None => inherited_summary.map_or_else(
                || "Follow Base Weapon".to_owned(),
                |donor| {
                    donor.weapon_pattern_index.map_or_else(
                        || format!("Preserve {} · unknown row", donor.name),
                        |index| format!("Follow {} · Runtime row {index}", donor.name),
                    )
                },
            ),
        };
        let selection = self.catalog.as_ref().and_then(|catalog| {
            catalog.draw_weapon_donor_header_picker(
                ui,
                "weapon-pattern-donor",
                &mut self.weapon_pattern_query,
                self.donor_summaries
                    .iter()
                    .filter(|candidate| candidate.weapon_pattern_index.is_some()),
                WeaponDonorPickerOptions {
                    selected_hash: selected_summary.map(|donor| donor.hash),
                    selected_label: &selected_text,
                    header_label: None,
                    action_label: "Swap Runtime",
                    selected_icon_override: None,
                    secondary_action_label: None,
                    row_detail: None,
                    clear: Some(WeaponDonorPickerClearChoice {
                        label: "Follow Base Weapon",
                        tooltip: "Use the base weapon's runtime.",
                        selected: override_index.is_none(),
                    }),
                    selected_detail: None,
                },
            )
        });
        match selection {
            Some(WeaponDonorPickerAction::Clear) => {
                self.recipe.overrides.weapon_pattern_index = None;
                self.recipe.overrides.weapon_pattern_donor_hash = None;
            }
            Some(WeaponDonorPickerAction::Select(hash)) => {
                self.recipe.overrides.weapon_pattern_index = self
                    .donor_summaries
                    .iter()
                    .find(|donor| donor.hash == hash)
                    .and_then(|donor| donor.weapon_pattern_index);
                self.recipe.overrides.weapon_pattern_donor_hash = self
                    .recipe
                    .overrides
                    .weapon_pattern_index
                    .map(|_| HexHash::new(hash));
            }
            Some(WeaponDonorPickerAction::Secondary) | None => {}
        }

        if let Some(index) = self.recipe.overrides.weapon_pattern_index {
            let pattern_donor = source_hash
                .and_then(|hash| {
                    self.donor_summaries.iter().find(|donor| {
                        donor.hash == hash && donor.weapon_pattern_index == Some(index)
                    })
                })
                .or_else(|| {
                    self.donor_summaries
                        .iter()
                        .find(|donor| donor.weapon_pattern_index == Some(index))
                });
            if pattern_donor.is_none() {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("No installed weapon uses runtime row {index}."),
                );
            } else if let (Some(pattern), Some(gameplay)) = (pattern_donor, gameplay_donor) {
                if pattern.type_name != gameplay.summary.type_name
                    || pattern.inventory_slot != gameplay.summary.inventory_slot
                {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "Runtime from a different weapon type or slot. Test in game.",
                    );
                }
            }
        }
    }

    pub(super) fn draw_stat_group_picker(
        &mut self,
        ui: &mut egui::Ui,
        gameplay_donor: Option<&WeaponDonor>,
    ) {
        draw_donor_section_label(
            ui,
            "Stat Display Scaling",
            Some(
                "Sets how raw stats display, such as RPM. Adds missing stats at stock values. Firing is unchanged.",
            ),
        );
        let override_index = self.recipe.overrides.stat_group_index;
        ui.strong(if override_index.is_some() {
            "Explicit display scaling"
        } else {
            "Inherited from base weapon"
        });
        let source_hash = self
            .recipe
            .overrides
            .stat_group_donor_hash
            .as_ref()
            .and_then(|hash| hash.parse_u32().ok());
        let selected_summary = override_index
            .and_then(|index| {
                source_hash.and_then(|hash| {
                    self.donor_summaries
                        .iter()
                        .find(|donor| donor.hash == hash && donor.stat_group_index == Some(index))
                })
            })
            .or_else(|| {
                override_index
                    .is_none()
                    .then(|| gameplay_donor.map(|d| &d.summary))
                    .flatten()
            });
        let selected_text = match override_index {
            Some(index) => selected_summary.map_or_else(
                || {
                    let representatives = self
                        .donor_summaries
                        .iter()
                        .filter(|donor| donor.stat_group_index == Some(index))
                        .count();
                    if representatives == 0 {
                        format!("Stat group {index} · not in catalog")
                    } else {
                        format!(
                            "Stat group {index} · {representatives} stock {}",
                            if representatives == 1 {
                                "weapon"
                            } else {
                                "weapons"
                            }
                        )
                    }
                },
                |donor| format!("{} · Group {index} · 0x{:08X}", donor.name, donor.hash),
            ),
            None => gameplay_donor.map_or_else(
                || "Follow Base Weapon Scaling".to_owned(),
                |donor| {
                    donor.summary.stat_group_index.map_or_else(
                        || format!("Preserve {} · unknown group", donor.summary.name),
                        |index| format!("Preserve {} · Group {index}", donor.summary.name),
                    )
                },
            ),
        };
        let selection = self.catalog.as_ref().and_then(|catalog| {
            catalog.draw_weapon_donor_header_picker(
                ui,
                "weapon-stat-group-donor",
                &mut self.stat_group_query,
                self.donor_summaries
                    .iter()
                    .filter(|candidate| candidate.stat_group_index.is_some()),
                WeaponDonorPickerOptions {
                    selected_hash: selected_summary.map(|donor| donor.hash),
                    selected_label: &selected_text,
                    header_label: None,
                    action_label: "Swap Scaling",
                    selected_icon_override: None,
                    secondary_action_label: None,
                    row_detail: None,
                    clear: Some(WeaponDonorPickerClearChoice {
                        label: "Follow Base Weapon Scaling",
                        tooltip: "Use the base weapon's stat scaling.",
                        selected: override_index.is_none(),
                    }),
                    selected_detail: None,
                },
            )
        });
        match selection {
            Some(WeaponDonorPickerAction::Clear) => {
                self.recipe.overrides.stat_group_index = None;
                self.recipe.overrides.stat_group_donor_hash = None;
            }
            Some(WeaponDonorPickerAction::Select(hash)) => {
                let stat_group_index = self
                    .donor_summaries
                    .iter()
                    .find(|donor| donor.hash == hash)
                    .and_then(|donor| donor.stat_group_index);
                self.recipe.overrides.stat_group_index = stat_group_index;
                self.recipe.overrides.stat_group_donor_hash =
                    stat_group_index.map(|_| HexHash::new(hash));
                if let Some(profile_donor) = self
                    .catalog
                    .as_ref()
                    .and_then(|catalog| catalog.weapon_donor(hash))
                {
                    merge_stat_profile_investment_rows(
                        &mut self.recipe.overrides,
                        gameplay_donor.map_or(&[], |donor| donor.investment_stats.as_slice()),
                        &profile_donor.investment_stats,
                    );
                }
            }
            Some(WeaponDonorPickerAction::Secondary) | None => {}
        }
        if let Some(index) = self.recipe.overrides.stat_group_index
            && !self
                .donor_summaries
                .iter()
                .any(|donor| donor.stat_group_index == Some(index))
        {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("No installed weapon uses stat group {index}."),
            );
        }
    }

    fn appearance_donor_hash(&self) -> Option<u32> {
        self.recipe
            .presentation_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok())
            .or_else(|| {
                self.recipe
                    .donor
                    .item_hash
                    .parse_u32()
                    .ok()
                    .filter(|hash| *hash != 0)
            })
    }

    fn appearance_donor_label(
        &self,
        reference: Option<&WeaponDonorReference>,
        inherited_hash: Option<u32>,
        unknown_label: &str,
    ) -> String {
        let find_donor = |hash| self.donor_summaries.iter().find(|donor| donor.hash == hash);
        match reference {
            Some(reference) => reference
                .item_hash
                .parse_u32()
                .ok()
                .and_then(find_donor)
                .map_or_else(
                    || {
                        format!(
                            "{} · {}",
                            reference.expected_name.as_deref().unwrap_or(unknown_label),
                            reference.item_hash
                        )
                    },
                    |donor| {
                        format!(
                            "{} · {} · 0x{:08X}",
                            donor.name, donor.type_name, donor.hash
                        )
                    },
                ),
            None => inherited_hash.and_then(find_donor).map_or_else(
                || "Use Weapon Appearance".to_owned(),
                |donor| format!("Follow {} · 0x{:08X}", donor.name, donor.hash),
            ),
        }
    }

    pub(super) fn draw_render_gear_donor_picker(&mut self, ui: &mut egui::Ui) {
        draw_donor_section_label(
            ui,
            "Colors & Materials",
            Some("Changes only colors and materials."),
        );
        let inherited_hash = self.appearance_donor_hash();
        let inherits_appearance = self.recipe.render_gear_donor.is_none();
        let current_hash = self
            .recipe
            .render_gear_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok());
        let displayed_hash = current_hash.or(inherited_hash);
        let selected_text = self.appearance_donor_label(
            self.recipe.render_gear_donor.as_ref(),
            inherited_hash,
            "Unknown color donor",
        );
        let selection = self.catalog.as_ref().and_then(|catalog| {
            catalog.draw_weapon_donor_header_picker(
                ui,
                "weapon-render-gear-donor",
                &mut self.render_gear_donor_query,
                self.donor_summaries.iter(),
                WeaponDonorPickerOptions {
                    selected_hash: displayed_hash,
                    selected_label: &selected_text,
                    header_label: None,
                    action_label: "Change Colors",
                    selected_icon_override: None,
                    secondary_action_label: None,
                    row_detail: None,
                    clear: Some(WeaponDonorPickerClearChoice {
                        label: "Use Weapon Appearance",
                        tooltip: "Use the appearance's colors.",
                        selected: inherits_appearance,
                    }),
                    selected_detail: None,
                },
            )
        });
        if self.catalog.is_none() {
            ui.add_enabled(false, egui::Button::new(selected_text));
        }
        match selection {
            Some(WeaponDonorPickerAction::Clear) => self.recipe.render_gear_donor = None,
            Some(WeaponDonorPickerAction::Select(item_hash)) => {
                if inherited_hash == Some(item_hash) {
                    self.recipe.render_gear_donor = None;
                } else if let Some(donor) = self
                    .donor_summaries
                    .iter()
                    .find(|donor| donor.hash == item_hash)
                {
                    self.recipe.render_gear_donor = Some(WeaponDonorReference {
                        item_hash: item_hash.into(),
                        expected_name: Some(donor.name.clone()),
                    });
                }
            }
            Some(WeaponDonorPickerAction::Secondary) | None => {}
        }
    }

    pub(super) fn draw_icon_donor_picker(&mut self, ui: &mut egui::Ui) {
        let kind = self.recipe.kind;
        let imported_source = {
            #[cfg(feature = "d2-model-importer")]
            {
                crate::imported::kind(kind).is_some()
                    && self.recipe.overrides.imported_graph.is_some()
            }
            #[cfg(not(feature = "d2-model-importer"))]
            {
                false
            }
        };
        draw_donor_section_label(
            ui,
            "Inventory Icon",
            Some("Changes only the inventory icon."),
        );
        let inherited_hash = self.appearance_donor_hash();
        let inherits_appearance = self.recipe.icon_donor.is_none();
        let current_hash = self
            .recipe
            .icon_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok());
        let displayed_hash = current_hash.or(inherited_hash);
        let selected_text = if imported_source && self.recipe.icon_donor.is_none() {
            "Source Item Icon".to_owned()
        } else if kind.is_weapon() {
            self.appearance_donor_label(
                self.recipe.icon_donor.as_ref(),
                inherited_hash,
                "Unknown icon donor",
            )
        } else {
            current_hash
                .and_then(|hash| {
                    self.gear_donors_for(kind)
                        .iter()
                        .find(|donor| donor.hash == hash)
                })
                .map(|donor| format!("{} · 0x{:08X}", donor.name, donor.hash))
                .unwrap_or_else(|| format!("Use Base {} Icon", kind.label()))
        };
        let icon_editor_target = self.catalog.as_ref().and_then(|catalog| {
            let item_hash = displayed_hash?;
            let container_tag = catalog.weapon_icon_container(item_hash)?;
            let donors = if kind.is_weapon() {
                self.donor_summaries.as_slice()
            } else {
                self.gear_donors_for(kind)
            };
            let donor_name = donors
                .iter()
                .find(|donor| donor.hash == item_hash)
                .map(|donor| donor.name.clone())
                // An ornament lends its icon without being an authoring donor.
                .or_else(|| catalog.item_display_name(item_hash).map(str::to_owned))
                .unwrap_or_else(|| format!("Item 0x{item_hash:08X}"));
            let donor_name = if imported_source && inherits_appearance {
                self.recipe.name.clone()
            } else {
                donor_name
            };
            Some((item_hash, donor_name, TagHash(container_tag)))
        });
        let (authored_icon_override, authored_icon_error) =
            if let Some((item_hash, _, container_tag)) = icon_editor_target.as_ref() {
                match self.authored_icon_preview(ui.ctx(), *item_hash, *container_tag) {
                    Ok(texture) => (texture, None),
                    Err(error) => (None, Some(error)),
                }
            } else {
                self.authored_icon_preview = None;
                (None, None)
            };
        let donors = if kind.is_weapon() {
            self.donor_summaries.as_slice()
        } else {
            self.gear_donors.get(&kind).map_or(&[][..], Vec::as_slice)
        };
        let selection = self.catalog.as_ref().and_then(|catalog| {
            catalog.draw_weapon_donor_header_picker(
                ui,
                "weapon-icon-donor",
                &mut self.icon_donor_query,
                donors.iter(),
                WeaponDonorPickerOptions {
                    selected_hash: if imported_source && inherits_appearance {
                        None
                    } else {
                        displayed_hash
                    },
                    selected_label: &selected_text,
                    header_label: None,
                    action_label: "Change Icon",
                    selected_icon_override: authored_icon_override.as_ref(),
                    secondary_action_label: icon_editor_target.as_ref().map(|_| "Edit Icon…"),
                    row_detail: None,
                    clear: Some(WeaponDonorPickerClearChoice {
                        label: if imported_source {
                            "Use Source Icon"
                        } else if kind.is_weapon() {
                            "Use Weapon Appearance"
                        } else {
                            "Use Base Icon"
                        },
                        tooltip: if imported_source {
                            "Use the imported source icon."
                        } else if kind.is_weapon() {
                            "Use the appearance's icon."
                        } else {
                            "Use the base item's icon."
                        },
                        selected: inherits_appearance,
                    }),
                    selected_detail: None,
                },
            )
        });
        if self.catalog.is_none() {
            ui.add_enabled(false, egui::Button::new(selected_text));
        }
        if let Some(error) = authored_icon_error {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!("Icon preview unavailable: {error}"),
            );
        }
        match selection {
            Some(WeaponDonorPickerAction::Clear) => {
                #[cfg(feature = "d2-model-importer")]
                if imported_source {
                    match crate::imported::source_icon(&self.recipe) {
                        Ok(icon) => {
                            self.recipe.overrides.icon_edit.imported_image = Some(icon);
                            self.recipe.icon_donor = None;
                        }
                        Err(error) => self.log.push(LogEntry::error(error)),
                    }
                } else {
                    self.recipe.icon_donor = None;
                }
                #[cfg(not(feature = "d2-model-importer"))]
                {
                    self.recipe.icon_donor = None;
                }
            }
            Some(WeaponDonorPickerAction::Select(item_hash)) => {
                if imported_source {
                    self.recipe.overrides.icon_edit.imported_image = None;
                }
                if inherited_hash == Some(item_hash) {
                    self.recipe.icon_donor = None;
                } else if let Some(donor) = donors.iter().find(|donor| donor.hash == item_hash) {
                    self.recipe.icon_donor = Some(WeaponDonorReference {
                        item_hash: item_hash.into(),
                        expected_name: Some(donor.name.clone()),
                    });
                }
            }
            Some(WeaponDonorPickerAction::Secondary) => {
                if let Some((item_hash, donor_name, container_tag)) = icon_editor_target
                    && let Some(rarity) = self.authored_icon_rarity()
                {
                    self.icon_editor = Some(
                        WeaponIconEditor::open(
                            &self.packages,
                            item_hash,
                            donor_name,
                            container_tag,
                            rarity,
                            self.recipe.overrides.icon_edit.clone(),
                            self.recipe.kind == crate::ItemKind::Subclass,
                        )
                        .with_corner(self.recipe.overrides.corner_icon.as_ref()),
                    );
                }
            }
            None => {}
        }
    }

    pub(super) fn draw_donor_section(&mut self, ui: &mut egui::Ui) {
        let donor = self.current_donor();
        if donor_section_column_count(ui.available_width()) >= 2 {
            ui.columns(2, |columns| {
                self.draw_donor_picker(&mut columns[0]);
                self.draw_presentation_donor_picker(&mut columns[1], donor.as_ref());
            });
        } else {
            self.draw_donor_picker(ui);
            draw_stacked_section_break(ui);
            self.draw_presentation_donor_picker(ui, donor.as_ref());
        }
    }

    pub(super) fn draw_donor_picker(&mut self, ui: &mut egui::Ui) {
        draw_donor_section_label(
            ui,
            "Base Weapon",
            Some(
                "Sets the weapon type and the starting stats, perks, sockets, damage type, ammo \
                 type and rarity.\n\
                 Firing, reload speed and the firing sound come from it.\n\
                 Its model, colors and icon apply unless Appearance names another weapon.\n\
                 Changing it starts these over and clears Appearance.",
            ),
        );
        let current_hash = self
            .recipe
            .donor
            .item_hash
            .parse_u32()
            .ok()
            .filter(|hash| *hash != 0);
        let selected_text = current_hash
            .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash))
            .map_or_else(
                || {
                    self.recipe
                        .donor
                        .expected_name
                        .clone()
                        .unwrap_or_else(|| self.recipe.donor.item_hash.to_string())
                },
                |donor| {
                    format!(
                        "{} · {} · 0x{:08X}",
                        donor.name, donor.type_name, donor.hash
                    )
                },
            );
        if let Some(entry) = current_hash.and_then(|hash| {
            self.recipe_entries
                .iter()
                .find(|entry| entry.identity_hash == hash && entry.identity_hash != 0)
        }) {
            ui.weak(format!(
                "Library weapon. Builds on {}'s recipe.",
                entry.name
            ))
            .on_hover_text("Built from that recipe's stock donor and changes.");
        }
        let selection = self.catalog.as_ref().and_then(|catalog| {
            catalog.draw_weapon_donor_header_picker(
                ui,
                "weapon-gameplay-donor",
                &mut self.donor_query,
                self.donor_summaries.iter(),
                WeaponDonorPickerOptions {
                    selected_hash: current_hash,
                    selected_label: &selected_text,
                    header_label: None,
                    action_label: "Change Base",
                    selected_icon_override: None,
                    secondary_action_label: None,
                    row_detail: None,
                    clear: None,
                    selected_detail: None,
                },
            )
        });
        if self.catalog.is_none() {
            ui.add_enabled(false, egui::Button::new(selected_text.as_str()));
        }
        // Parts from other weapons are chosen on Gameplay. The card says when any are, and leads
        // there.
        let borrowed = self.borrowed_gameplay_parts();
        if !borrowed.is_empty() {
            draw_part_note(
                ui,
                &format!("{} from other weapons", spoken_list(&borrowed)),
                "Open Gameplay",
            )
            .then(|| self.open_page(WorkbenchPage::Advanced));
        }
        if let Some(WeaponDonorPickerAction::Select(hash)) = selection
            && current_hash != Some(hash)
            && let Some(donor) = self.donor_summaries.iter().find(|donor| donor.hash == hash)
        {
            self.recipe.set_donor(hash, donor.name.clone());
            self.clear_dependent_picker_queries();
        }
    }

    pub(super) fn draw_presentation_donor_picker(
        &mut self,
        ui: &mut egui::Ui,
        gameplay_donor: Option<&WeaponDonor>,
    ) {
        let effective_base = gameplay_donor.map(|donor| {
            let mut summary = donor.summary.clone();
            summary.weapon_translation_group =
                crate::capabilities::effective_weapon_translation_group(
                    &donor.summary,
                    self.recipe.overrides.weapon_pattern_index,
                    &self.donor_summaries,
                );
            summary
        });
        let gameplay_summary = effective_base.as_ref();
        let target_slot = gameplay_summary
            .and_then(|donor| authored_inventory_slot(&self.recipe.overrides, donor));
        let slot_changed = gameplay_summary.is_some_and(|donor| {
            target_slot.is_some_and(|target| donor.inventory_slot != Some(target))
        });
        let slot_warning = gameplay_summary
            .zip(target_slot)
            .filter(|_| slot_changed && self.recipe.presentation_donor.is_none())
            .map(|(gameplay, target)| {
                format!(
                    "Keeps the base {} model in {}. Test in game.",
                    gameplay.type_name,
                    target.label(),
                )
            });
        let current_reference = self.recipe.presentation_donor.clone();
        let current_hash = current_reference
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok());
        let displayed_hash = if current_reference.is_none() {
            gameplay_summary.map(|donor| donor.hash)
        } else {
            current_hash
        };
        let current_summary = current_hash
            .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash));
        // Which of the two cross-family paths the build takes is decided by trying the rig move,
        // so this follows the finished runtime scan rather than guessing. The Animations row says
        // where the animations come from, so the warning only names the risk. It sits behind the
        // heading's warning icon with the slot's.
        let warning = current_summary
            .zip(gameplay_summary)
            .and_then(|(current, gameplay)| {
                let checked = self
                    .runtime_graph
                    .as_ref()
                    .is_some_and(|(loaded, _)| self.runtime_graph_key().as_ref() == Some(loaded));
                let carried = checked.then_some(self.runtime_rig_appearance == Some(current.hash));
                match (
                    crate::capabilities::appearance_type_differs(current, gameplay),
                    crate::capabilities::appearance_animations_differ(current, gameplay),
                    carried,
                ) {
                    (true, _, Some(true)) => Some("Different weapon type. The game can crash."),
                    (true, _, Some(false)) => {
                        Some("Different weapon type. Moving parts stay still. The game can crash.")
                    }
                    (false, true, Some(true)) => Some("Different rig. The game can crash."),
                    (false, true, Some(false)) => {
                        Some("Different rig. Moving parts stay still. The game can crash.")
                    }
                    // While the rig is checked there is nothing to warn about yet.
                    (true, _, None) | (false, true, None) | (false, false, _) => None,
                }
            });
        draw_donor_section_label_with_warning(
            ui,
            "Appearance",
            Some(
                "Replaces the model, its colors and the icon. The Appearance tab can change the \
                 icon and colors.\n\
                 Keeps the base weapon's stats, perks, firing, reload speed and firing sound.",
            ),
            match (slot_warning.as_deref(), warning) {
                (Some(slot), Some(rig)) => Some(format!("{rig}\n{slot}")),
                (slot, rig) => slot.or(rig).map(str::to_owned),
            }
            .as_deref(),
        );
        let current_is_compatible = current_summary.is_some_and(|candidate| {
            gameplay_summary
                .zip(target_slot)
                .is_some_and(|(gameplay, target)| {
                    presentation_donor_candidate_is_compatible(candidate, gameplay, target)
                })
        });
        let selected_text = match current_reference.as_ref() {
            None => "Follow Base Weapon".to_owned(),
            Some(reference) => match current_hash {
                None => format!("Invalid donor hash · {}", reference.item_hash),
                Some(hash) => current_summary.map_or_else(
                    || {
                        format!(
                            "{} · 0x{hash:08X} · not in catalog",
                            reference
                                .expected_name
                                .as_deref()
                                .unwrap_or("Unknown appearance")
                        )
                    },
                    |donor| {
                        format!(
                            "{} · 0x{:08X}{}",
                            donor.name,
                            donor.hash,
                            if current_is_compatible {
                                ""
                            } else {
                                " · incompatible"
                            }
                        )
                    },
                ),
            },
        };
        let can_choose = gameplay_summary.is_some() && target_slot.is_some();
        let selection = self.catalog.as_ref().and_then(|catalog| {
            ui.add_enabled_ui(can_choose, |ui| {
                catalog.draw_weapon_appearance_picker(
                    ui,
                    "weapon-presentation-donor",
                    &mut self.presentation_donor_query,
                    self.donor_summaries.iter().filter(|candidate| {
                        gameplay_summary
                            .zip(target_slot)
                            .is_some_and(|(gameplay, target)| {
                                presentation_donor_candidate_is_compatible(
                                    candidate, gameplay, target,
                                )
                            })
                    }),
                    WeaponDonorPickerOptions {
                        selected_hash: displayed_hash,
                        selected_label: &selected_text,
                        header_label: None,
                        action_label: "Change Appearance",
                        selected_icon_override: None,
                        secondary_action_label: None,
                        row_detail: None,
                        clear: Some(WeaponDonorPickerClearChoice {
                            label: "Follow Base Weapon",
                            tooltip: "Use the base weapon's model, colors and icon.",
                            selected: current_reference.is_none(),
                        }),
                        selected_detail: None,
                    },
                    gameplay_summary.map(|summary| summary.type_name.as_str()),
                    |ui, hash| {
                        let mut candidate = self.recipe.clone();
                        candidate.set_presentation_donor(hash.map(|hash| WeaponDonorReference {
                            item_hash: hash.into(),
                            expected_name: None,
                        }));
                        if let Some(loadout) = preview::loadout(catalog, &candidate) {
                            let name = hash
                                .and_then(|hash| {
                                    self.donor_summaries.iter().find(|donor| donor.hash == hash)
                                })
                                .map_or("Base Weapon Appearance", |donor| donor.name.as_str());
                            sundial::ui::model_preview::chooser::preview(
                                ui,
                                &self.packages,
                                catalog.preview_appearance(&loadout),
                                name,
                            );
                        } else {
                            ui.label("No model for this appearance.");
                        }
                    },
                )
            })
            .inner
        });
        if self.catalog.is_none() {
            ui.add_enabled(false, egui::Button::new(&selected_text));
        }
        // Animations are chosen on Appearance. The card says when they come from another weapon.
        if self.recipe.kind.is_weapon() && self.recipe.overrides.animation_donor.is_some() {
            draw_part_note(ui, "Animations from another weapon", "Open Appearance")
                .then(|| self.open_page(WorkbenchPage::Appearance));
        }
        match selection {
            Some(WeaponDonorPickerAction::Clear) => {
                self.recipe.set_presentation_donor(None);
                self.clear_presentation_picker_queries();
            }
            Some(WeaponDonorPickerAction::Select(item_hash)) => {
                if let Some(donor) = self
                    .donor_summaries
                    .iter()
                    .find(|donor| donor.hash == item_hash)
                {
                    self.recipe
                        .set_presentation_donor(Some(WeaponDonorReference {
                            item_hash: item_hash.into(),
                            expected_name: Some(donor.name.clone()),
                        }));
                    self.clear_presentation_picker_queries();
                }
            }
            Some(WeaponDonorPickerAction::Secondary) | None => {}
        }

        let (Some(gameplay_summary), Some(target_slot)) = (gameplay_summary, target_slot) else {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Choose a base weapon with a known slot first.",
            );
            return;
        };
        if self.recipe.presentation_donor.is_some()
            && !selected_presentation_donor_is_compatible(
                &self.recipe,
                gameplay_summary,
                &self.donor_summaries,
            )
        {
            ui.colored_label(
                ui.visuals().error_fg_color,
                self.recipe
                    .presentation_donor
                    .as_ref()
                    .and_then(|reference| reference.item_hash.parse_u32().ok())
                    .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash))
                    .map_or("Appearance not installed".to_owned(), |appearance| {
                        match crate::capabilities::appearance_compatibility(
                            appearance,
                            gameplay_summary,
                            target_slot,
                        ) {
                            crate::capabilities::AppearanceCompatibility::Blocked(reason) => {
                                reason.to_owned()
                            }
                            crate::capabilities::AppearanceCompatibility::Compatible => {
                                "Choose a separate appearance or restore the base appearance."
                                    .to_owned()
                            }
                        }
                    }),
            );
        }
        self.draw_appearance_ornament_button(ui);
    }
}

/// The behaviors this base weapon can borrow, leaving out the element switch that the Variable
/// damage type applies and the weapon's own behavior.
pub(super) fn unique_behavior_sources(
    gameplay_donor: Option<&WeaponDonor>,
) -> Vec<&'static crate::weapon::behavior::Behavior> {
    let Some(gameplay_donor) = gameplay_donor else {
        return Vec::new();
    };
    // Two weapons can name one graph: Borealis and D.A.R.C.I. share theirs. Excluding only the
    // donor's own catalogue entry left the other weapon's entry offering the tag already in the
    // block, which reported a behavior applied and changed nothing.
    let own_graphs = crate::weapon::behavior::CATALOG
        .iter()
        .filter(|entry| entry.source_item_hash == gameplay_donor.summary.hash)
        .filter_map(crate::weapon::behavior::Behavior::graph_tag)
        .collect::<Vec<_>>();
    crate::weapon::behavior::catalog_for_type(&gameplay_donor.summary.type_name)
        .filter(|entry| !entry.switches_element())
        .filter(|entry| entry.source_item_hash != gameplay_donor.summary.hash)
        .filter(|entry| {
            entry
                .graph_tag()
                .is_none_or(|tag| !own_graphs.contains(&tag))
        })
        .collect()
}

/// The behavior currently borrowed, if any.
fn selected_unique_behavior(
    overrides: &crate::recipe::WeaponRecipeOverrides,
) -> Option<&'static crate::weapon::behavior::Behavior> {
    use crate::weapon::behavior::ELEMENT_SWITCH;
    overrides
        .additional_behaviors
        .iter()
        .find(|chosen| chosen.behavior != ELEMENT_SWITCH)
        .and_then(|chosen| crate::weapon::behavior::behavior(&chosen.behavior))
}

/// The source weapon's name, followed by the perks choosing it puts in the sockets.
///
/// The row named only the weapon the behavior is borrowed from, which does not say what turns up
/// in the sockets afterwards. Several of these keep half of the behavior in a perk, so naming
/// them here is what tells an author that taking Hard Light also takes The Fundamentals.
fn behavior_label(
    entry: &crate::weapon::behavior::Behavior,
    catalog: Option<&InvestmentCatalog>,
) -> String {
    let mut perks: Vec<&str> = Vec::new();
    if let Some(catalog) = catalog {
        for plug in [entry.intrinsic_plug, entry.trait_plug]
            .into_iter()
            .flatten()
        {
            // An installation that cannot name the plug says nothing rather than a bare hash.
            if let Some(name) = catalog.item_display_name(plug)
                && !perks.contains(&name)
            {
                perks.push(name);
            }
        }
    }
    if perks.is_empty() {
        return entry.source_name.to_owned();
    }
    format!("{} ({})", entry.source_name, perks.join(", "))
}

fn behavior_tooltip(
    entry: &crate::weapon::behavior::Behavior,
    catalog: Option<&InvestmentCatalog>,
) -> String {
    let mut sections = vec![entry.source_name.to_owned(), entry.summary.to_owned()];
    if let Some(catalog) = catalog {
        let mut seen = std::collections::BTreeSet::new();
        for plug in [entry.intrinsic_plug, entry.trait_plug]
            .into_iter()
            .flatten()
        {
            if !seen.insert(plug) {
                continue;
            }
            let name = catalog.plug_label(plug, false);
            let description = catalog
                .perk_description(plug)
                .unwrap_or("No perk description.");
            sections.push(format!("{name}\n{description}"));
        }
    }
    if let Some(record) = crate::weapon::behavior::paired_record_source(entry) {
        sections.push(if record == entry.source_name {
            "Also brings its handling behavior.".to_owned()
        } else {
            format!("Also brings the handling behavior it shares with {record}.")
        });
    }
    if let Some(caution) = entry.caution {
        sections.push(caution.to_owned());
    }
    sections.push("Perk text describes the original weapon. Test in game.".into());
    sections.join("\n\n")
}

/// Prefer the firing graph over the separate record for the same weapon.
///
/// Safe because a graph choice now applies that weapon's behavior record as well, so one pick
/// brings both halves. Keep an already selected record choice visible without changing the
/// saved recipe.
fn offered_behaviors(
    sources: &[&'static crate::weapon::behavior::Behavior],
    selected: Option<&'static crate::weapon::behavior::Behavior>,
) -> Vec<&'static crate::weapon::behavior::Behavior> {
    let mut offered: Vec<&'static crate::weapon::behavior::Behavior> = Vec::new();
    for entry in sources {
        if let Some(kept) = offered
            .iter_mut()
            .find(|kept| kept.source_item_hash == entry.source_item_hash)
        {
            if entry.has_graph() {
                *kept = entry;
            }
        } else {
            offered.push(entry);
        }
    }
    if let Some(selected) = selected
        && !offered.iter().any(|entry| entry.id == selected.id)
    {
        offered.push(selected);
    }
    offered.sort_by_key(|entry| (entry.source_name, !entry.has_graph()));
    offered
}

fn behavior_choice_label(
    entry: &crate::weapon::behavior::Behavior,
    catalog: Option<&InvestmentCatalog>,
) -> String {
    let label = behavior_label(entry, catalog);
    let paired_graph = crate::weapon::behavior::CATALOG
        .iter()
        .any(|other| other.source_item_hash == entry.source_item_hash && other.has_graph());
    if entry.has_graph() || !paired_graph {
        label
    } else if entry.carries_behavior_record() {
        // Calling this "State Only" was wrong: it carries the owner's behavior array. Only the
        // firing and projectile side is missing, which is what the graph choice adds.
        format!("{label} · No Firing Graph")
    } else {
        format!("{label} · State Only")
    }
}

/// Behavior, under the Base Weapon card: whose built-in behavior the weapon has. Another
/// weapon's can be copied onto it, even across weapon types.
pub(super) fn draw_unique_behavior_control(
    ui: &mut egui::Ui,
    overrides: &mut crate::recipe::WeaponRecipeOverrides,
    sources: &[&'static crate::weapon::behavior::Behavior],
    (catalog, donors): (Option<&InvestmentCatalog>, &[WeaponDonorSummary]),
    query: &mut String,
    base_name: &str,
    stacked: bool,
) {
    use crate::recipe::AdditionalBehaviorRecipe;
    use crate::weapon::behavior::ELEMENT_SWITCH;
    let selected = selected_unique_behavior(overrides);
    let offered = offered_behaviors(sources, selected);
    // A grid dropdown names an inherited value as the others there do.
    let selected_text = selected.map_or_else(
        || {
            if stacked {
                format!("{base_name} (base weapon)")
            } else {
                base_name.to_owned()
            }
        },
        |entry| behavior_choice_label(entry, catalog),
    );
    // The browser lists weapons, so a behavior is found by the weapon it came from. Each offered
    // entry names a distinct weapon after `offered_behaviors` collapses a weapon's two halves.
    let entry_for_hash = |hash: u32| {
        offered
            .iter()
            .copied()
            .find(|entry| entry.source_item_hash == hash)
    };
    // The weapon name alone does not say what a choice brings, so each row carries the perks it
    // would pin underneath it.
    let detail = |hash: u32| {
        let catalog = catalog?;
        let entry = entry_for_hash(hash)?;
        let perks = [entry.intrinsic_plug, entry.trait_plug]
            .into_iter()
            .flatten()
            .map(|plug| catalog.plug_label(plug, false))
            .collect::<Vec<_>>();
        (!perks.is_empty()).then(|| perks.join(" · "))
    };
    let candidates = offered.iter().filter_map(|entry| {
        donors
            .iter()
            .find(|donor| donor.hash == entry.source_item_hash)
    });
    // The value names the weapon. What the current choice brings, its perk text and any caution,
    // stays behind the info icon: what the row changes, then what the current choice brings.
    const BEHAVIOR: &str = "Another weapon's built-in behavior: the projectile it fires, its \
                            behavior record and firing values, and the intrinsic and trait perks \
                            that drive them. Replaces the base weapon's own. Stats, type markers \
                            and animations stay. Test in game.";
    let hint = crate::app::style::destiny_text(
        ui,
        selected.map_or_else(
            || BEHAVIOR.to_owned(),
            |entry| format!("{BEHAVIOR}\n\n{}", behavior_tooltip(entry, catalog)),
        ),
    );
    let part = parts::Part {
        column: &parts::GAMEPLAY_COLUMN,
        label: "Behavior",
        hint: hint.into(),
        value: &selected_text,
        chosen: selected.map(|entry| entry.source_item_hash),
        follow: "Follow Base Weapon",
        blocked: None,
        detail: Some(&detail),
    };
    let scope = "weapon-unique-behavior";
    let selection = if stacked {
        parts::draw_stacked_part(ui, catalog, scope, query, candidates, part)
    } else {
        parts::draw_part(ui, catalog, scope, query, candidates, part)
    };
    // Element switch is requested by its own control, so it survives every change made here.
    let keep_element_switch = |overrides: &mut crate::recipe::WeaponRecipeOverrides| {
        overrides
            .additional_behaviors
            .retain(|kept| kept.behavior == ELEMENT_SWITCH);
    };
    match selection {
        Some(WeaponDonorPickerAction::Clear) => {
            if selected.is_some_and(crate::weapon::behavior::source_switches_element) {
                overrides.variable_damage = None;
            }
            keep_element_switch(overrides);
        }
        Some(WeaponDonorPickerAction::Select(hash)) => {
            if let Some(entry) = entry_for_hash(hash) {
                if selected.is_some_and(crate::weapon::behavior::source_switches_element)
                    && !crate::weapon::behavior::source_switches_element(entry)
                {
                    overrides.variable_damage = None;
                }
                keep_element_switch(overrides);
                overrides
                    .additional_behaviors
                    .push(AdditionalBehaviorRecipe {
                        behavior: entry.id.to_owned(),
                    });
                // Hard Light and Borealis switch damage as well as fire differently, so choosing
                // them turns that on rather than leaving it to be found.
                if crate::weapon::behavior::source_switches_element(entry) {
                    overrides.variable_damage = Some(crate::recipe::VariableDamageRecipe::all());
                }
            }
        }
        Some(WeaponDonorPickerAction::Secondary) | None => {}
    }
}

/// What the borrowed behavior brings with it, drawn under the Behavior row: a caution and the
/// controls that only apply once something is chosen.
pub(super) fn draw_unique_behavior_details(
    ui: &mut egui::Ui,
    overrides: &mut crate::recipe::WeaponRecipeOverrides,
) {
    let Some(entry) = selected_unique_behavior(overrides) else {
        return;
    };
    if let Some(caution) = entry.caution {
        let caution =
            crate::app::style::destiny_text(ui, caution).color(ui.visuals().warn_fg_color);
        ui.label(caution);
    }
    let mut with_perks = !overrides.skip_behavior_perks;
    if ui
        .checkbox(&mut with_perks, "Include Its Perks")
        .on_hover_text(
            "Replaces the intrinsic and pins the source trait. Some behavior lives in these perks.",
        )
        .changed()
    {
        overrides.skip_behavior_perks = !with_perks;
    }
    // The firing pattern only matters while the perks that change the burst come along.
    if with_perks && entry.changes_burst() {
        draw_unique_behavior_firing(ui, overrides);
    }
    if entry.launches_projectiles() {
        draw_unique_behavior_projectile_speed(ui, overrides);
    }
    ui.weak(entry.summary);
}

/// Chooses whose firing pattern the weapon uses: the borrowed behavior's, or the base weapon's
/// own. A borrowed perk's burst change is written for its own weapon type, so the build rebases
/// it onto this one or sets it aside.
fn draw_unique_behavior_firing(
    ui: &mut egui::Ui,
    overrides: &mut crate::recipe::WeaponRecipeOverrides,
) {
    use crate::recipe::RecipeBehaviorFiring;
    const BEHAVIOR: &str = "Unique Behavior";
    const WEAPON: &str = "Base Weapon";
    ui.horizontal_wrapped(|ui| {
        let label = ui.label("Firing Pattern");
        let selected = match overrides.behavior_firing {
            Some(RecipeBehaviorFiring::Weapon) => WEAPON,
            None | Some(RecipeBehaviorFiring::Behavior) => BEHAVIOR,
        };
        egui::ComboBox::from_id_salt("weapon-unique-behavior-firing")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                crate::app::style::workbench_style(ui);
                ui.selectable_value(&mut overrides.behavior_firing, None, BEHAVIOR);
                ui.selectable_value(
                    &mut overrides.behavior_firing,
                    Some(RecipeBehaviorFiring::Weapon),
                    WEAPON,
                );
            })
            .response
            .labelled_by(label.id)
            .on_hover_text("Whose burst and fire rate the weapon uses.");
    });
}

/// Raises the borrowed projectiles' launch speed, up to the figure set here.
///
/// The field is a multiplier on the weapon's own launch speed, so a weapon that fires instantly
/// supplies almost nothing to multiply and the borrowed rounds crawl. A source whose own
/// multiplier already exceeds this figure came from a frame that supplies nothing either, so it
/// is left alone.
fn draw_unique_behavior_projectile_speed(
    ui: &mut egui::Ui,
    overrides: &mut crate::recipe::WeaponRecipeOverrides,
) {
    let mut boost = overrides.behavior_projectile_speed_bits.map_or(
        crate::weapon::behavior::DEFAULT_PROJECTILE_SPEED_BOOST,
        f32::from_bits,
    );
    ui.horizontal_wrapped(|ui| {
        ui.label("Projectile Speed Multiplier");
        let response = ui.add_sized(
            [100.0, ui.spacing().interact_size.y],
            egui::DragValue::new(&mut boost)
                .speed(0.05)
                .max_decimals(3)
                .range(1.0..=9_998.0)
                .suffix(" \u{d7}"),
        );
        let changed = response.changed();
        crate::app::style::named_control(response, "Projectile Speed Multiplier").on_hover_text(
            "Speeds up the behavior's projectiles when this weapon normally fires instantly. A faster source speed is kept. No safe maximum is known, so raise it gradually and test in game.",
        );
        if changed {
            overrides.behavior_projectile_speed_bits = Some(boost.to_bits());
        }
    });
}

/// A quiet line under a card naming parts another tab sets, with a link there. Returns whether
/// the link was followed.
fn draw_part_note(ui: &mut egui::Ui, note: &str, link: &str) -> bool {
    ui.horizontal_wrapped(|ui| {
        ui.weak(note);
        ui.link(link).clicked()
    })
    .inner
}

/// `parts` as a reader says them: "A", "A and B", "A, B and C".
fn spoken_list(parts: &[&str]) -> String {
    match parts {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn picker_prefers_firing_graph_but_preserves_selected_state_record() {
        use crate::weapon::behavior::{behavior, catalog_for_type};
        let sources = catalog_for_type("Auto Rifle").collect::<Vec<_>>();
        let offered = super::offered_behaviors(&sources, None);
        assert!(offered.iter().any(|entry| entry.id == "cerberus-1-graph"));
        assert!(!offered.iter().any(|entry| entry.id == "cerberus-plus-one"));
        let original = behavior("cerberus-plus-one").unwrap();
        let offered = super::offered_behaviors(&sources, Some(original));
        assert!(offered.iter().any(|entry| entry.id == original.id));
        assert!(super::behavior_choice_label(original, None).ends_with("No Firing Graph"));
        assert!(!super::behavior_tooltip(original, None).contains("Shared with"));
        assert!(
            !super::behavior_tooltip(behavior("tarrabah").unwrap(), None)
                .contains("element switch")
        );
    }
}
