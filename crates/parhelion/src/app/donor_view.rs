//! Installed donor selection and provenance controls.
use super::*;
use sundial::package_authoring::{WeaponDyeColors, load_weapon_dye_colors};

type DyeColorResults = BTreeMap<u16, Result<WeaponDyeColors, String>>;

#[derive(Default)]
pub(super) struct DyeColors {
    colors: DyeColorResults,
    receiver: Option<Receiver<DyeColorResults>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for DyeColors {
    fn drop(&mut self) {
        // Match the library-preview lifetime: no package handles may survive catalog release.
        self.receiver = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl DyeColors {
    fn update(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        rows: &[Vec<WeaponDyeReferenceRecipe>; 3],
    ) {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            let _ = self.worker.take().unwrap().join();
            if let Some(receiver) = self.receiver.take() {
                if let Ok(colors) = receiver.try_recv() {
                    self.colors.extend(colors);
                }
            }
        }
        let indices: BTreeSet<_> = rows
            .iter()
            .flatten()
            .filter(|row| row.channel_index >= 0)
            .map(|row| row.dye_reference_index)
            .collect();
        self.colors.retain(|index, _| indices.contains(index));
        if self.worker.is_none() && !packages.as_os_str().is_empty() {
            let missing: Vec<_> = indices
                .into_iter()
                .filter(|index| !self.colors.contains_key(index))
                .collect();
            if !missing.is_empty() {
                let packages = packages.to_owned();
                let ctx = ctx.clone();
                let (sender, receiver) = mpsc::channel();
                self.receiver = Some(receiver);
                self.worker = Some(thread::spawn(move || {
                    let colors =
                        load_weapon_dye_colors(&packages, &missing).unwrap_or_else(|error| {
                            missing
                                .into_iter()
                                .map(|index| (index, Err(error.clone())))
                                .collect()
                        });
                    let _ = sender.send(colors);
                    ctx.request_repaint();
                }));
            }
        }
        if self.worker.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn draw(&self, ui: &mut egui::Ui, row: &WeaponDyeReferenceRecipe) {
        if row.channel_index < 0 {
            ui.weak("Disabled");
            return;
        }
        match self.colors.get(&row.dye_reference_index) {
            Some(Ok(colors)) => {
                ui.horizontal(|ui| {
                for (label, rgb) in [("Primary", colors.primary), ("Secondary", colors.secondary)] {
                    let color = egui::Color32::from(egui::Rgba::from_rgb(rgb[0].min(1.0), rgb[1].min(1.0), rgb[2].min(1.0)));
                    let (rect, response) = ui.allocate_exact_size(egui::vec2(25.0, 20.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 3.0, color);
                    ui.painter().rect_stroke(rect, 3.0, ui.visuals().widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
                    response.on_hover_text(format!("{label} albedo · #{:02X}{:02X}{:02X}\nLinear RGB: {:.4}, {:.4}, {:.4}\nBase material tint, before textures, lighting and shader overrides.", color.r(), color.g(), color.b(), rgb[0], rgb[1], rgb[2]));
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
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("Base weapon: {gameplay}"));
            ui.separator();
            ui.label(format!("Runtime source: {runtime}"));
            if !self.recipe.runtime_component_donors.is_empty() {
                ui.separator();
                ui.label(format!(
                    "{} custom component sources",
                    self.recipe.runtime_component_donors.len()
                ));
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
            "Art variants",
            Some(
                "Ordered native {character class, art-variant index} rows. Class -1 is the shared fallback; 0, 1, and 2 are class-specific rows.",
            ),
        );
        let inherited_art = geometry_donor
            .map(|donor| donor.art_arrangements.as_slice())
            .unwrap_or_default();
        if self.recipe.overrides.art_arrangements.is_none() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Inheriting {} art row(s)", inherited_art.len()));
                if ui.button("Edit art rows").clicked() {
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
            let restore = ui.button("Restore geometry donor").clicked();
            if restore {
                self.recipe.overrides.art_arrangements = None;
            } else {
                let rows = self
                    .recipe
                    .overrides
                    .art_arrangements
                    .as_mut()
                    .expect("checked above");
                if rows.len() < 4 && ui.button("+ Add art row").clicked() {
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
                        ui.weak("Art-variant index");
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
            "Render dyes",
            Some(
                "Complete ordered native {channel index, art-dye reference index} arrays for custom, default, and locked dyes. Channel -1 is the disabled sentinel. These are raw package indices; incompatible combinations can intentionally produce missing materials.",
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
        ui.label("Swatches: primary / secondary base material colors. Textures, lighting and applied shaders can change the final appearance.");
        if self.recipe.overrides.render_dye_rows.is_none() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "Inheriting dye counts {}/{}/{}",
                    inherited_dyes[0].len(),
                    inherited_dyes[1].len(),
                    inherited_dyes[2].len()
                ));
                if ui.button("Edit dye rows").clicked() {
                    self.recipe.overrides.render_dye_rows = Some(inherited_dyes.clone());
                }
            });
            for (array, rows) in inherited_dyes.iter().enumerate() {
                if rows.is_empty() {
                    continue;
                }
                ui.strong(["Custom dyes", "Default dyes", "Locked dyes"][array]);
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

        if ui.button("Restore render-gear donor").clicked() {
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
            const DYE_ARRAY_NAMES: [&str; 3] = ["Custom dyes", "Default dyes", "Locked dyes"];
            ui.horizontal(|ui| {
                ui.strong(DYE_ARRAY_NAMES[array]);
                ui.weak(format!("{} row(s)", rows.len()));
                if rows.len() < 32 && ui.small_button("+ Add row").clicked() {
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
                    ui.weak("Channel index");
                    ui.weak("Dye reference");
                    ui.weak("Base colors");
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
            Some(
                "Selects the stock runtime entity used for firing and weapon behavior. Parhelion keeps the appearance donor's gear-art data and combines the two only when their native translation groups are compatible. Individual component sources let you customize this baseline further; test their combined behavior in-game.",
            ),
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
                        format!("Runtime row {index} · not represented in catalog")
                    } else {
                        format!("Runtime row {index} · {representatives} stock representative(s)")
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
                || "Follow gameplay donor".to_owned(),
                |donor| {
                    donor.weapon_pattern_index.map_or_else(
                        || format!("Preserve {} · undecoded", donor.name),
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
                    header_label: Some(if override_index.is_some() {
                        "Custom runtime source"
                    } else {
                        "Follows base weapon"
                    }),
                    action_label: "Swap Runtime",
                    selected_icon_override: None,
                    secondary_action_label: None,
                    clear: Some(WeaponDonorPickerClearChoice {
                        label: "Follow gameplay donor",
                        tooltip: "Use the gameplay donor's complete native runtime entity.",
                        selected: override_index.is_none(),
                    }),
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
            Some(WeaponDonorPickerAction::Secondary) => {}
            None => {}
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
                    format!("Runtime row {index} is not represented by an installed stock weapon."),
                );
            } else if let (Some(pattern), Some(gameplay)) = (pattern_donor, gameplay_donor) {
                if pattern.type_name != gameplay.summary.type_name
                    || pattern.inventory_slot != gameplay.summary.inventory_slot
                {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "Cross-family combination: this runtime was built for a different weapon type or slot. Check the complete combination in-game; changing a source does not automatically adapt its components.",
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
            "Stat display scaling",
            Some(
                "Selects the installed stat-group bounds and display curves used to turn raw investment values into values such as RPM. Stat rows present on the selected weapon but absent from the gameplay donor are added with their stock values; existing recipe values are preserved. This does not change firing behavior.",
            ),
        );
        let override_index = self.recipe.overrides.stat_group_index;
        ui.label(
            egui::RichText::new(if override_index.is_some() {
                "Explicit display scaling"
            } else {
                "Inherited from gameplay donor"
            })
            .strong(),
        );
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
                        format!("Stat group {index} · not represented in catalog")
                    } else {
                        format!("Stat group {index} · {representatives} compatible stock weapon(s)")
                    }
                },
                |donor| format!("{} · Group {index} · 0x{:08X}", donor.name, donor.hash),
            ),
            None => gameplay_donor.map_or_else(
                || "Follow gameplay donor scaling".to_owned(),
                |donor| {
                    donor.summary.stat_group_index.map_or_else(
                        || format!("Preserve {} · undecoded", donor.summary.name),
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
                    header_label: override_index
                        .is_none()
                        .then_some("Inherited from gameplay donor"),
                    action_label: "Swap scaling",
                    selected_icon_override: None,
                    secondary_action_label: None,
                    clear: Some(WeaponDonorPickerClearChoice {
                        label: "Follow gameplay donor scaling",
                        tooltip:
                            "Use the gameplay donor's installed stat bounds and display curves.",
                        selected: override_index.is_none(),
                    }),
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
            Some(WeaponDonorPickerAction::Secondary) => {}
            None => {}
        }
        if let Some(index) = self.recipe.overrides.stat_group_index
            && !self
                .donor_summaries
                .iter()
                .any(|donor| donor.stat_group_index == Some(index))
        {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Stat group {index} is not represented by an installed stock weapon."),
            );
        }
    }

    pub(super) fn draw_render_gear_donor_picker(&mut self, ui: &mut egui::Ui) {
        draw_donor_section_label(
            ui,
            "Colors & Materials",
            Some(
                "Selects only the stock custom, default, and locked dye-reference arrays. Geometry, icon definition, runtime baseline, and component bindings remain independent.",
            ),
        );
        let gameplay_hash = self
            .recipe
            .donor
            .item_hash
            .parse_u32()
            .ok()
            .filter(|hash| *hash != 0);
        let inherited_hash = self
            .recipe
            .presentation_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok())
            .or(gameplay_hash);
        let current_reference = self.recipe.render_gear_donor.clone();
        let current_hash = current_reference
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok());
        let displayed_hash = current_hash.or(inherited_hash);
        let selected_text = current_reference.as_ref().map_or_else(
            || {
                inherited_hash
                    .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash))
                    .map_or_else(
                        || "Use weapon appearance".to_owned(),
                        |donor| format!("Follow {} · 0x{:08X}", donor.name, donor.hash),
                    )
            },
            |reference| {
                current_hash
                    .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash))
                    .map_or_else(
                        || {
                            format!(
                                "{} · {}",
                                reference
                                    .expected_name
                                    .as_deref()
                                    .unwrap_or("Unknown render-gear donor"),
                                reference.item_hash
                            )
                        },
                        |donor| {
                            format!(
                                "{} · {} · 0x{:08X}",
                                donor.name, donor.type_name, donor.hash
                            )
                        },
                    )
            },
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
                    header_label: current_reference.is_none().then_some("Uses weapon appearance"),
                    action_label: "Change Colors",
                    selected_icon_override: None,
                    secondary_action_label: None,
                    clear: Some(WeaponDonorPickerClearChoice {
                        label: "Use weapon appearance",
                        tooltip: "Use the geometry donor's render dyes, or the gameplay donor when geometry is inherited.",
                        selected: current_reference.is_none(),
                    }),
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
        draw_donor_section_label(
            ui,
            "Inventory Icon",
            Some(
                "Selects only the stock icon container that Parhelion clones and watermarks. Geometry, client classification, and render gear remain sourced independently.",
            ),
        );
        let gameplay_hash = self
            .recipe
            .donor
            .item_hash
            .parse_u32()
            .ok()
            .filter(|hash| *hash != 0);
        let inherited_hash = self
            .recipe
            .presentation_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok())
            .or(gameplay_hash);
        let current_reference = self.recipe.icon_donor.clone();
        let current_hash = current_reference
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok());
        let displayed_hash = current_hash.or(inherited_hash);
        let selected_text = current_reference.as_ref().map_or_else(
            || {
                inherited_hash
                    .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash))
                    .map_or_else(
                        || "Use weapon appearance".to_owned(),
                        |donor| format!("Follow {} · 0x{:08X}", donor.name, donor.hash),
                    )
            },
            |reference| {
                current_hash
                    .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash))
                    .map_or_else(
                        || {
                            format!(
                                "{} · {}",
                                reference
                                    .expected_name
                                    .as_deref()
                                    .unwrap_or("Unknown icon donor"),
                                reference.item_hash
                            )
                        },
                        |donor| {
                            format!(
                                "{} · {} · 0x{:08X}",
                                donor.name, donor.type_name, donor.hash
                            )
                        },
                    )
            },
        );
        let icon_editor_target = self.catalog.as_ref().and_then(|catalog| {
            let item_hash = displayed_hash?;
            let container_tag = catalog.weapon_icon_container(item_hash)?;
            let donor_name = self
                .donor_summaries
                .iter()
                .find(|donor| donor.hash == item_hash)
                .map(|donor| donor.name.clone())
                .unwrap_or_else(|| format!("Weapon 0x{item_hash:08X}"));
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
        let selection = self.catalog.as_ref().and_then(|catalog| {
            catalog.draw_weapon_donor_header_picker(
                ui,
                "weapon-icon-donor",
                &mut self.icon_donor_query,
                self.donor_summaries.iter(),
                WeaponDonorPickerOptions {
                    selected_hash: displayed_hash,
                    selected_label: &selected_text,
                    header_label: current_reference.is_none().then_some("Uses weapon appearance"),
                    action_label: "Change Icon",
                    selected_icon_override: authored_icon_override.as_ref(),
                    secondary_action_label: icon_editor_target.as_ref().map(|_| "Edit Icon…"),
                    clear: Some(WeaponDonorPickerClearChoice {
                        label: "Use weapon appearance",
                        tooltip: "Use the geometry donor's icon, or the gameplay donor when geometry is inherited.",
                        selected: current_reference.is_none(),
                    }),
                },
            )
        });
        if self.catalog.is_none() {
            ui.add_enabled(false, egui::Button::new(selected_text));
        }
        if let Some(error) = authored_icon_error {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!("Authored icon preview unavailable: {error}"),
            );
        }
        match selection {
            Some(WeaponDonorPickerAction::Clear) => self.recipe.icon_donor = None,
            Some(WeaponDonorPickerAction::Select(item_hash)) => {
                if inherited_hash == Some(item_hash) {
                    self.recipe.icon_donor = None;
                } else if let Some(donor) = self
                    .donor_summaries
                    .iter()
                    .find(|donor| donor.hash == item_hash)
                {
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
                    self.icon_editor = Some(WeaponIconEditor::open(
                        &self.packages,
                        item_hash,
                        donor_name,
                        container_tag,
                        rarity,
                        self.recipe.overrides.icon_edit.clone(),
                    ));
                }
            }
            None => {}
        }
    }

    pub(super) fn draw_donor_section(&mut self, ui: &mut egui::Ui) {
        if ui.available_width() >= 780.0 {
            ui.columns(2, |columns| {
                self.draw_donor_picker(&mut columns[0]);
                let donor = self.current_donor();
                self.draw_presentation_donor_picker(&mut columns[1], donor.as_ref());
            });
        } else {
            self.draw_donor_picker(ui);
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
            let donor = self.current_donor();
            self.draw_presentation_donor_picker(ui, donor.as_ref());
        }
    }

    pub(super) fn draw_donor_picker(&mut self, ui: &mut egui::Ui) {
        draw_donor_section_label(
            ui,
            "Base Weapon",
            Some(
                "The starting weapon for stats, sockets and gameplay. You can customize those independently below.",
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
                    clear: None,
                },
            )
        });
        if self.catalog.is_none() {
            ui.horizontal(|ui| {
                if self.catalog_receiver.is_some() {
                    ui.spinner();
                    ui.label("Loading Sundial weapon catalog…");
                }
            });
            ui.add_enabled(false, egui::Button::new(selected_text));
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
        draw_donor_section_label(
            ui,
            "Appearance",
            Some(
                "The weapon model and its compatible animation/classification data. Customize the inventory icon and colors on the Appearance tab; this does not select firing behavior.",
            ),
        );
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
        let current_is_compatible = current_summary.is_some_and(|candidate| {
            gameplay_summary
                .zip(target_slot)
                .is_some_and(|(gameplay, target)| {
                    presentation_donor_candidate_is_compatible(candidate, gameplay, target)
                })
        });
        let selected_text = match current_reference.as_ref() {
            None => "Inherit gameplay donor".to_owned(),
            Some(reference) => match current_hash {
                None => format!("Invalid donor hash · {}", reference.item_hash),
                Some(hash) => current_summary.map_or_else(
                    || {
                        format!(
                            "{} · 0x{hash:08X} · not in catalog",
                            reference
                                .expected_name
                                .as_deref()
                                .unwrap_or("Unknown geometry donor")
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
                                " · incompatible with target"
                            }
                        )
                    },
                ),
            },
        };
        let can_choose = gameplay_summary.is_some() && target_slot.is_some();
        let selection = self.catalog.as_ref().and_then(|catalog| {
            ui.add_enabled_ui(can_choose, |ui| {
                catalog.draw_weapon_donor_header_picker(
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
                        header_label: if current_reference.is_none() {
                            Some("Uses base weapon appearance")
                        } else {
                            None
                        },
                        action_label: "Change Appearance",
                        selected_icon_override: None,
                        secondary_action_label: None,
                        clear: Some(
                            WeaponDonorPickerClearChoice {
                                label: "Inherit gameplay donor",
                                tooltip: "Use the gameplay donor's model/art arrangement and client-classification tuple.",
                                selected: current_reference.is_none(),
                            },
                        ),
                    },
                )
            })
            .inner
        });
        if self.catalog.is_none() {
            ui.add_enabled(false, egui::Button::new(&selected_text));
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
                "Load a donor with a decoded inventory slot before choosing geometry data.",
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
                self.recipe.presentation_donor.as_ref().and_then(|reference| reference.item_hash.parse_u32().ok())
                    .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash))
                    .map_or("Appearance is unavailable in this installation".to_owned(), |appearance| {
                    match crate::capabilities::appearance_compatibility(appearance, gameplay_summary, target_slot) {
                        crate::capabilities::AppearanceCompatibility::Blocked(reason) => reason.to_owned(),
                        crate::capabilities::AppearanceCompatibility::Unchecked => "Appearance animations have not been verified. Refresh the catalog or choose a verified appearance.".to_owned(),
                        crate::capabilities::AppearanceCompatibility::Compatible => "Choose a separate appearance or restore the base appearance.".to_owned(),
                    }
                }),
            );
        } else if slot_changed && self.recipe.presentation_donor.is_none() {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "Retains the base {} model and animations in {}. This slot conversion needs an in-game test.",
                    gameplay_summary.type_name,
                    target_slot.label(),
                ),
            );
        }
    }
}
