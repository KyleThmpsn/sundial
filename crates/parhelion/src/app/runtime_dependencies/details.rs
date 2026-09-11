use super::*;

impl Browser {
    pub(super) fn navigate(&mut self, page: Page, index: usize) {
        if self.page != page || self.selected != index {
            self.history.push((self.page, self.selected));
        }
        self.page = page;
        self.selected = index;
        self.query.clear();
        self.reveal_selection = true;
    }

    pub(super) fn draw_perk(
        &mut self,
        ui: &mut egui::Ui,
        index: &Index,
        choices: &[WeaponSandboxPerkChoice],
        donors: &[WeaponDonorSummary],
        _target: Option<u16>,
    ) {
        let Some(perk) = index.perks.get(self.selected) else {
            return;
        };
        ui.heading(perk_label(perk.index, choices));
        ui.strong(if perk.error.is_some() {
            "Inspection Incomplete"
        } else if perk.action.is_none() {
            "Marker Only"
        } else {
            "Has an Action"
        });
        if let Some(error) = &perk.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.label(perk_explanation(perk));
        ui.add_space(12.0);
        self.draw_host_requirements(ui, index, perk, donors);
        ui.add_space(12.0);
        self.draw_stock_defaults(ui, perk, donors);
        ui.add_space(12.0);
        egui::CollapsingHeader::new("Technical Details")
            .id_salt(("perk-technical", perk.index))
            .show(ui, |ui| {
                if let Some(action) = perk.action {
                    ui.monospace(format!(
                        "Action {action:08X} · Runtime Key {:08X}",
                        perk.runtime_key
                    ));
                }
                for graph in &perk.graphs {
                    draw_entity(ui, graph);
                }
            });
    }

    fn draw_stock_defaults(
        &mut self,
        ui: &mut egui::Ui,
        perk: &Perk,
        donors: &[WeaponDonorSummary],
    ) {
        ui.horizontal(|ui| {
            ui.strong("Used By");
            sundial::investment::draw_authoring_info_icon(ui, "Stock weapons that use this perk by default. These are examples, not required pairings. Optional socket choices are excluded.");
        });
        let patterns = self
            .uses
            .iter()
            .filter(|usage| usize::from(usage.perk_index) == perk.index)
            .map(|usage| usage.pattern_index)
            .collect::<BTreeSet<_>>();
        if patterns.is_empty() {
            ui.weak("No stock examples found.");
        }
        for pattern in patterns {
            if ui
                .link(pattern_label(usize::from(pattern), donors))
                .clicked()
            {
                self.navigate(Page::Patterns, usize::from(pattern));
            }
        }
    }

    fn draw_host_requirements(
        &mut self,
        ui: &mut egui::Ui,
        index: &Index,
        perk: &Perk,
        donors: &[WeaponDonorSummary],
    ) {
        ui.strong("What It Needs");
        let Some(caster) = index
            .caster
            .as_ref()
            .filter(|caster| caster.perk_index == perk.index)
        else {
            ui.label("Unknown. Test with your weapon in game.");
            return;
        };
        ui.label("Needs the caster sword setup from Temptation's Hook. The Frame marker alone does not launch projectiles.");
        ui.label("Use its runtime source or sword component, then test in game.");
        if ui
            .link(pattern_label(caster.pattern_index, donors))
            .clicked()
        {
            self.navigate(Page::Patterns, caster.pattern_index);
        }
        egui::CollapsingHeader::new("Projectile Resources").show(ui, |ui| {
            for graph in &caster.projectile_graphs {
                draw_entity(ui, graph);
            }
        });
    }

    pub(super) fn draw_pattern(
        &mut self,
        ui: &mut egui::Ui,
        index: &Index,
        choices: &[WeaponSandboxPerkChoice],
        donors: &[WeaponDonorSummary],
        target: Option<u16>,
    ) {
        let Some(pattern) = index.patterns.get(self.selected) else {
            return;
        };
        ui.heading(pattern_label(pattern.index, donors));
        if target.map(usize::from) == Some(pattern.index) {
            ui.strong("Current Recipe Pattern");
        }
        let weapons = donors
            .iter()
            .filter(|donor| donor.weapon_pattern_index.map(usize::from) == Some(pattern.index))
            .map(|donor| donor.name.as_str())
            .collect::<BTreeSet<_>>();
        if weapons.len() > 1 {
            egui::CollapsingHeader::new(format!("Stock Weapons ({})", weapons.len()))
                .id_salt(("pattern-weapons", pattern.index))
                .show(ui, |ui| {
                    for name in weapons {
                        ui.label(name);
                    }
                });
        }
        if let Some(error) = &pattern.error {
            ui.colored_label(ui.visuals().warn_fg_color, error);
        }
        if let Some(entity) = &pattern.entity {
            ui.add_space(12.0);
            ui.strong("Components");
            let names = entity
                .components
                .iter()
                .map(|component| {
                    runtime_component_control(component.binding).map_or_else(
                        || "Unmapped Component".to_owned(),
                        |control| control.label.to_owned(),
                    )
                })
                .collect::<BTreeSet<_>>();
            for name in names {
                ui.label(name);
            }
            if entity.components.is_empty() {
                ui.weak("No components found.");
            }
            let peers = index
                .patterns
                .iter()
                .filter(|peer| {
                    peer.index != pattern.index
                        && peer
                            .entity
                            .as_ref()
                            .is_some_and(|other| other.tag == entity.tag)
                })
                .map(|peer| peer.index)
                .collect::<Vec<_>>();
            if !peers.is_empty() {
                egui::CollapsingHeader::new(format!("Related Patterns ({})", peers.len()))
                    .id_salt(("pattern-peers", pattern.index))
                    .show(ui, |ui| {
                        for peer in peers {
                            if ui.link(pattern_label(peer, donors)).clicked() {
                                self.navigate(Page::Patterns, peer);
                            }
                        }
                    });
            }
        }
        ui.add_space(12.0);
        ui.strong("Default Perks");
        let perks = self
            .uses
            .iter()
            .filter(|usage| usize::from(usage.pattern_index) == pattern.index)
            .map(|usage| usage.perk_index)
            .collect::<BTreeSet<_>>();
        if perks.is_empty() {
            ui.weak("No stock examples found.");
        }
        for perk in perks {
            if ui.link(perk_label(usize::from(perk), choices)).clicked() {
                self.navigate(Page::Perks, usize::from(perk));
            }
        }
        ui.add_space(12.0);
        egui::CollapsingHeader::new("Technical Details")
            .id_salt(("pattern-technical", pattern.index))
            .show(ui, |ui| {
                ui.monospace(format!(
                    "Item {:08X}\nRuntime Key {:08X}\nTranslation Group {:08X}",
                    pattern.item_hash, pattern.runtime_key, pattern.translation_group
                ));
                if let Some(entity) = &pattern.entity {
                    draw_entity(ui, entity);
                }
            });
    }
}
