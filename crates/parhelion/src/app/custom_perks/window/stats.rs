//! Equipped stat bonuses are separate from conditional sandbox action parameters.
use super::*;

pub(super) fn draw(
    ui: &mut egui::Ui,
    catalog: &InvestmentCatalog,
    donor: &WeaponDonor,
    plug_hash: u32,
    variant: &mut WeaponSocketPlugVariantRecipe,
) {
    let source = catalog.item_stat_contributions(plug_hash);
    ui.push_id(("custom-perk-stats", variant.socket_index, variant.choice_index), |ui| {
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("Stat Bonuses While Equipped");
            draw_authoring_info_icon(ui,
                "These replace this custom plug's native contribution while equipped, not the weapon's total stat. Values are raw investment units; the weapon's stat group controls display and limits. They do not change conditional bonuses, cooldowns, damage multipliers or projectile speed. Other perks and the stock source are unchanged. Test new combinations in game.",
            );
        });
        ui.label("Always applied while equipped; activation conditions below do not control these bonuses.");
        let indices = source.iter().map(|stat| stat.definition_index)
            .chain(variant.investment_stats.iter().map(|stat| stat.definition_index))
            .collect::<BTreeSet<_>>();
        let mut reset = None;
        egui::Grid::new("contributions").num_columns(3).spacing([12.0, 5.0]).show(ui, |ui| {
            for &index in &indices {
                let original = source.iter().find(|stat| stat.definition_index == index);
                let metadata = original.or_else(|| donor.investment_stats.iter()
                    .chain(&donor.addable_investment_stats).find(|stat| stat.definition_index == index));
                let name = metadata.map_or_else(|| format!("Stat {index} (Unavailable)"), |stat| stat.name.clone());
                let override_value = variant.investment_stats.iter().find(|stat| stat.definition_index == index);
                let edited = override_value.is_some();
                let source_value = original.map_or(0, |stat| stat.value);
                let mut value = override_value.map_or(source_value, |stat| stat.value);
                ui.label(&name).on_hover_text(format!("Source contribution: {source_value}; native stat row: {index}"));
                if named_control(ui.add(egui::DragValue::new(&mut value).speed(1.0)),
                    format!("{name} Contribution")).changed() {
                    stat_editor::update_investment_stat_value(&mut variant.investment_stats,
                        index, source_value, value, original.is_none());
                }
                if ui.add_enabled(edited, egui::Button::new("Reset"))
                    .on_hover_text("Restore the source contribution, or remove this added stat.").clicked() {
                    reset = Some(index);
                }
                ui.end_row();
            }
        });
        if let Some(index) = reset {
            variant.investment_stats.retain(|stat| stat.definition_index != index);
        }
        if indices.is_empty() { ui.label("This perk has no built-in stat bonuses."); }
        ui.add_enabled_ui(indices.len() < crate::weapon::SUNRISE_STAT_CONTRIBUTION_CAPACITY, |ui| {
            egui::ComboBox::from_id_salt("add-contribution").selected_text("+ Add Stat Bonus")
                .show_ui(ui, |ui| {
                    for stat in donor.investment_stats.iter().chain(&donor.addable_investment_stats)
                        .filter(|stat| !indices.contains(&stat.definition_index)) {
                        if ui.selectable_label(false, &stat.name).clicked() {
                            variant.investment_stats.push(WeaponStatOverride {
                                definition_index: stat.definition_index, value: 0,
                            });
                        }
                    }
                });
        }).response.on_disabled_hover_text("Sunrise reads at most 16 stat contributions per definition.");
        ui.separator();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES; native catalog and headless layout"]
    fn native_custom_stat_controls_fit_and_do_not_mutate_on_open() {
        let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
        let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
        let donor = catalog
            .weapon_donor_with_stat_group_index(0x4CE3_CE93, None)
            .unwrap();
        let mut variant: WeaponSocketPlugVariantRecipe = serde_json::from_str(
            r#"{"socket_index":0,"choice_index":0,"source_plug_hash":"0xDD5CB37A","sandbox_perks":[{"source_perk_index":1178,"runtime_values":[]}]}"#,
        ).unwrap();
        // Include every candidate and an unavailable imported row; no saved edit may disappear.
        variant.investment_stats = donor
            .investment_stats
            .iter()
            .chain(&donor.addable_investment_stats)
            .map(|stat| WeaponStatOverride {
                definition_index: stat.definition_index,
                value: -5,
            })
            .collect();
        variant.investment_stats.push(WeaponStatOverride {
            definition_index: 255,
            value: 10,
        });
        let before = variant.clone();
        for width in [480.0, 620.0, 900.0] {
            for visuals in [egui::Visuals::light(), egui::Visuals::dark()] {
                let ctx = egui::Context::default();
                ctx.set_visuals(visuals);
                for _ in 0..2 {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 4000.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                workbench_style(ui);
                                let right = ui.max_rect().right();
                                draw(ui, &catalog, &donor, 0xDD5C_B37A, &mut variant);
                                assert!(
                                    ui.min_rect().right() <= right + 1.0,
                                    "stat rows overflow at {width}"
                                );
                            });
                        },
                    );
                    assert_eq!(variant, before);
                }
            }
        }
    }
}
