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
        donors: &[WeaponDonorSummary],
        _target: Option<u16>,
    ) {
        let Some(perk) = index.perks.get(self.selected) else {
            return;
        };
        ui.heading(self.sources.label(perk.index));
        ui.strong(if perk.error.is_some() {
            "Inspection Incomplete"
        } else if perk.action.is_none() {
            "No Standalone Action"
        } else {
            "Has an Action"
        });
        if let Some(error) = &perk.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.label(perk_explanation(perk));
        if let Some(behavior) = &perk.behavior {
            ui.add_space(8.0);
            ui.strong("Decoded Behavior");
            ui.label(&behavior.headline);
        }
        draw_sources(ui, &self.sources, perk.index);
        ui.add_space(12.0);
        self.draw_host_requirements(ui, index, perk, donors);
        ui.add_space(12.0);
        self.draw_stock_defaults(ui, perk.index, None);
        ui.add_space(12.0);
        egui::CollapsingHeader::new("Technical Details")
            .id_salt(("perk-technical", perk.index))
            .show(ui, |ui| {
                ui.monospace(format!(
                    "Effect {} · Hash {:08X}\nRuntime Key {:08X}",
                    perk.index, perk.hash, perk.runtime_key
                ));
                if let Some(action) = perk.action {
                    ui.monospace(format!("Action {action:08X}"));
                }
                for graph in &perk.graphs {
                    draw_entity(ui, graph);
                }
            });
    }

    fn draw_stock_defaults(&mut self, ui: &mut egui::Ui, perk: usize, pattern: Option<usize>) {
        let uses = default_uses(&self.uses, perk, pattern);
        if pattern.is_none() {
            ui.strong("Referenced in Item Defaults");
            ui.small("Base items and default socket plugs that reference this effect. This does not establish activation or compatibility. Optional socket choices are excluded.");
        }
        if uses.is_empty() {
            ui.weak("No item default references found.");
        }
        for usage in uses {
            let source = usage.source_plug.map_or_else(
                || "Base Item".to_owned(),
                |hash| {
                    let name = self
                        .sources
                        .get(perk)
                        .iter()
                        .find(|source| source.hash == hash)
                        .map_or("Unnamed Plug", |source| source.name.as_str());
                    format!("Default Plug: {name} · {hash:08X}")
                },
            );
            if ui
                .link(format!(
                    "{} · Pattern {}",
                    usage.weapon_name, usage.pattern_index
                ))
                .on_hover_text(format!("Item {:08X}\n{source}", usage.weapon_hash))
                .clicked()
            {
                self.navigate(Page::Patterns, usize::from(usage.pattern_index));
            }
            ui.small(source);
        }
    }

    fn draw_host_requirements(
        &mut self,
        ui: &mut egui::Ui,
        index: &Index,
        perk: &Perk,
        donors: &[WeaponDonorSummary],
    ) {
        ui.strong("Host Evidence");
        let Some(caster) = index
            .caster
            .as_ref()
            .filter(|caster| caster.perk_index == perk.index)
        else {
            ui.label("Host requirements have not been established. Test with your weapon in game.");
            return;
        };
        ui.label("In the stock Temptation's Hook setup, the sword component supplies the projectile resources. The Frame marker has no standalone action.");
        ui.label("This is one observed setup. Other combinations need a gameplay test.");
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
            egui::CollapsingHeader::new(format!("Items Sharing This Pattern ({})", weapons.len()))
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
                egui::CollapsingHeader::new(format!(
                    "Patterns Sharing This Entity ({})",
                    peers.len()
                ))
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
        ui.strong("Effects in Item Defaults");
        ui.small("These effects occur on individual items that share this pattern. They are not defaults of the pattern itself.");
        let perks = self
            .uses
            .iter()
            .filter(|usage| usize::from(usage.pattern_index) == pattern.index)
            .map(|usage| usage.perk_index)
            .collect::<BTreeSet<_>>();
        if perks.is_empty() {
            ui.weak("No item default references found.");
        }
        for perk in perks {
            if ui.link(self.sources.label(usize::from(perk))).clicked() {
                self.navigate(Page::Perks, usize::from(perk));
            }
            egui::CollapsingHeader::new("Item References")
                .id_salt(("pattern-perk-uses", pattern.index, perk))
                .show(ui, |ui| {
                    self.draw_stock_defaults(ui, usize::from(perk), Some(pattern.index));
                });
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

pub(super) fn default_uses(
    uses: &[PerkPatternUse],
    perk: usize,
    pattern: Option<usize>,
) -> Vec<PerkPatternUse> {
    uses.iter()
        .filter(|usage| {
            usize::from(usage.perk_index) == perk
                && pattern.is_none_or(|pattern| usize::from(usage.pattern_index) == pattern)
        })
        .cloned()
        .collect()
}

use sundial::ui::catalog::draw_sources;
