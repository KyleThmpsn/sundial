use super::*;
use sundial::package_authoring::sandbox_perk::program::{
    Action, Asset, Position, Program, Trigger,
};

pub(super) struct AssetChoice {
    index: usize,
    name: String,
    detail: String,
    search: String,
}

pub(super) fn asset_choices(catalog: &projectile::catalog::Catalog) -> Vec<AssetChoice> {
    let mut rows = catalog
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let name = entry
                .native_paths
                .first()
                .map(|path| sundial::package_authoring::tft::asset_label(path))
                .or_else(|| entry.native_name.clone())
                .unwrap_or_else(|| {
                    format!(
                        "Unidentified {} · 0x{:08X}",
                        entry.kind.label(),
                        entry.graph
                    )
                });
            let detail = format!(
                "{} · {} · 0x{:08X}",
                entry.kind.label(),
                entry.package,
                entry.graph
            );
            let search = format!(
                "{name} {detail} {} {}",
                entry.native_paths.join(" "),
                entry
                    .contexts
                    .iter()
                    .map(|context| context.path.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .to_lowercase();
            AssetChoice {
                index,
                name,
                detail,
                search,
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by_cached_key(|row| {
        (
            catalog.entries[row.index].native_paths.is_empty(),
            row.name.to_lowercase(),
            row.index,
        )
    });
    rows
}

pub(super) fn new_effect(metadata_index: u16) -> WeaponSandboxPerkRuntimeRecipe {
    let mut effect = PerkRecipe::effect(metadata_index);
    effect.program = Some(Program::default());
    effect
}

impl Workbench {
    pub(super) fn draw_program(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        program: &mut Program,
    ) -> Option<usize> {
        ui.horizontal(|ui| {
            ui.label("Effect Name");
            ui.add(egui::TextEdit::singleline(&mut program.name).desired_width(f32::INFINITY));
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.strong("When");
            egui::ComboBox::from_id_salt("program-trigger")
                .selected_text(program.trigger.label())
                .show_ui(ui, |ui| {
                    crate::app::style::perk_style(ui);
                    for trigger in Trigger::ALL {
                        ui.selectable_value(&mut program.trigger, trigger, trigger.label());
                    }
                });
        });
        if program.trigger.is_event() {
            ui.horizontal_wrapped(|ui| {
                seconds(ui, "Duration", &mut program.duration_ms, 1);
                seconds(ui, "Cooldown", &mut program.cooldown_ms, 0);
                ui.label("Chance");
                let mut percent = f32::from(program.chance_permyriad) / 100.0;
                if ui
                    .add(
                        egui::DragValue::new(&mut percent)
                            .range(0.0..=100.0)
                            .suffix("%"),
                    )
                    .changed()
                {
                    program.chance_permyriad = (percent * 100.0).round() as u16;
                }
            });
        } else {
            ui.small(if program.trigger == Trigger::Equipped {
                "Actions start when equipped. Retained entities and pattern overrides are removed when unequipped."
            } else {
                "Actions start when drawn. Retained entities and pattern overrides are removed when holstered."
            });
        }
        ui.add_space(6.0);
        ui.strong("Then");
        if program.actions.is_empty() {
            ui.label(
                "Add a projectile or emitter action to begin. Each action belongs to this effect.",
            );
        }
        let mut edit = None;
        let mut remove = None;
        let mut movement = None;
        let count = program.actions.len();
        for (index, action) in program.actions.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                ui.group(|ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(format!("{}. {}", index + 1, action.label()));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.menu_button("More", |ui| {
                                crate::app::style::perk_style(ui);
                                if ui
                                    .add_enabled(index > 0, egui::Button::new("Move Up"))
                                    .clicked()
                                {
                                    movement = Some((index, index - 1));
                                    ui.close_menu();
                                }
                                if ui
                                    .add_enabled(index + 1 < count, egui::Button::new("Move Down"))
                                    .clicked()
                                {
                                    movement = Some((index, index + 1));
                                    ui.close_menu();
                                }
                                if ui.button("Remove Action").clicked() {
                                    remove = Some(index);
                                    ui.close_menu();
                                }
                            });
                            if ui
                                .add_enabled(
                                    action.asset().graph != 0,
                                    egui::Button::new("Properties…"),
                                )
                                .on_disabled_hover_text("Choose an asset to edit its properties.")
                                .clicked()
                            {
                                edit = Some(index);
                            }
                        });
                    });
                    let only_projectile = matches!(action, Action::Pattern { .. });
                    self.draw_asset_picker(ui, catalog, action.asset_mut(), only_projectile);
                    if let Action::Spawn { position, .. } = action {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Position");
                            egui::ComboBox::from_id_salt("spawn-position")
                                .selected_text(position.label())
                                .show_ui(ui, |ui| {
                                    crate::app::style::perk_style(ui);
                                    ui.selectable_value(
                                        position,
                                        Position::Owner,
                                        Position::Owner.label(),
                                    );
                                    ui.add_enabled_ui(program.trigger.is_event(), |ui| {
                                        ui.selectable_value(
                                            position,
                                            Position::Event,
                                            Position::Event.label(),
                                        );
                                    });
                                });
                        });
                        ui.small(
                            "Spawns once per activation. The entity controls its own lifetime.",
                        );
                    } else if matches!(action, Action::Attach { .. }) {
                        ui.small("Attaches to the weapon and is removed when the effect ends.");
                    } else {
                        ui.small("Uses this projectile pattern while the effect is active.");
                    }
                    let changes = action.asset().values.len();
                    if changes != 0 {
                        ui.small(format!("{changes} Asset Parameter Changes"));
                    }
                });
            });
        }
        if let Some(index) = remove {
            program.actions.remove(index);
            edit = None;
        }
        if let Some((from, to)) = movement {
            program.actions.swap(from, to);
            edit = None;
        }
        ui.add_enabled_ui(program.actions.len() < 16, |ui| {
            ui.menu_button("Add Action…", |ui| {
                crate::app::style::perk_style(ui);
                for (label, description, action) in [
                    (
                        "Spawn Projectile or Emitter",
                        "Create an entity at the owner or event position.",
                        Action::Spawn {
                            asset: Asset::default(),
                            position: if program.trigger.is_event() {
                                Position::Event
                            } else {
                                Position::Owner
                            },
                        },
                    ),
                    (
                        "Attach Projectile or Emitter",
                        "Keep an entity attached for the duration of the effect.",
                        Action::Attach {
                            asset: Asset::default(),
                        },
                    ),
                    (
                        "Override Weapon Pattern",
                        "Use a selected projectile pattern while the effect is active.",
                        Action::Pattern {
                            asset: Asset::default(),
                        },
                    ),
                ] {
                    let enabled = !matches!(action, Action::Pattern { .. })
                        || !program
                            .actions
                            .iter()
                            .any(|current| matches!(current, Action::Pattern { .. }));
                    if ui
                        .add_enabled(enabled, egui::Button::new(label))
                        .on_hover_text(description)
                        .clicked()
                    {
                        program.actions.push(action);
                        ui.close_menu();
                    }
                }
            });
        });
        if !program.actions.is_empty()
            && let Err(error) = program.validate()
        {
            ui.colored_label(ui.visuals().warn_fg_color, error);
        }
        edit
    }

    fn draw_asset_picker(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        asset: &mut Asset,
        only_projectile: bool,
    ) {
        let label = if asset.graph == 0 {
            "Choose Asset…".into()
        } else if asset.path.is_empty() {
            format!("Asset 0x{:08X}", asset.graph)
        } else {
            sundial::package_authoring::tft::asset_label(&asset.path)
        };
        let picked = pickers::popup(
            ui,
            "program-asset",
            &label,
            &mut self.asset_query,
            |ui, query, reset, height| {
                let Some(data) = &self.discovery.data else {
                    ui.spinner();
                    ui.label("Reading Native Assets…");
                    return None;
                };
                let filter_id = ui.make_persistent_id("asset-kind");
                let mut filter = ui.data_mut(|state| state.get_temp::<u8>(filter_id).unwrap_or(0));
                let before = filter;
                if !only_projectile {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut filter, 0, "All Assets");
                        ui.selectable_value(&mut filter, 1, "Projectiles");
                        ui.selectable_value(&mut filter, 2, "Emitters");
                    });
                    ui.data_mut(|state| state.insert_temp(filter_id, filter));
                }
                let choices = data
                    .asset_choices
                    .iter()
                    .filter(|row| {
                        let kind = data.effects.entries[row.index].kind;
                        (!only_projectile || kind == projectile::Kind::Projectile)
                            && (only_projectile
                                || filter == 0
                                || (filter == 1 && kind == projectile::Kind::Projectile)
                                || (filter == 2 && kind == projectile::Kind::Emitter))
                            && query
                                .split_whitespace()
                                .all(|word| row.search.contains(word))
                    })
                    .collect::<Vec<_>>();
                let row_height = sundial::investment::authoring_choice_row_height(ui);
                pickers::results(
                    ui,
                    "program-assets",
                    choices.len(),
                    (height - 38.0).max(60.0),
                    reset || filter != before,
                    row_height,
                    |ui, index| {
                        let row = choices[index];
                        let entry = &data.effects.entries[row.index];
                        let response = if let Some(catalog) = catalog {
                            catalog.draw_authoring_choice_row(
                                ui,
                                None,
                                &row.name,
                                Some(&row.detail),
                                asset.graph == entry.graph,
                            )
                        } else {
                            sundial::investment::draw_asset_choice_row(
                                ui,
                                &row.name,
                                &row.detail,
                                asset.graph == entry.graph,
                            )
                        };
                        response
                            .on_hover_text(entry.native_paths.join("\n"))
                            .clicked()
                            .then(|| Asset {
                                graph: entry.graph,
                                path: entry.native_paths.first().cloned().unwrap_or_default(),
                                values: Vec::new(),
                            })
                    },
                )
            },
        );
        if let Some(picked) = picked {
            if picked.graph != asset.graph {
                *asset = picked;
            }
        }
        if !asset.path.is_empty() {
            ui.add(egui::Label::new(egui::RichText::new(&asset.path).small().weak()).truncate())
                .on_hover_text(&asset.path);
        }
    }
}

fn seconds(ui: &mut egui::Ui, label: &str, millis: &mut u32, minimum: u32) {
    ui.label(label).on_hover_text(if label == "Duration" {
        "How long retained actions stay active after the trigger."
    } else {
        "The delay before this effect can activate again."
    });
    let mut value = *millis as f32 / 1000.0;
    if ui
        .add(
            egui::DragValue::new(&mut value)
                .speed(0.05)
                .range((minimum as f32 / 1000.0)..=3600.0)
                .suffix(" s"),
        )
        .changed()
    {
        *millis = (value * 1000.0).round() as u32;
    }
}
