//! A catalog selection commits to the exact document and graph that were shown.
use super::*;
use sundial::investment::discovery::kinds::Family;
use sundial::package_authoring::sandbox_perk::{
    action,
    program::{NativeNode, native_draft},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Placement {
    Trigger,
    Requirement,
    #[default]
    Action,
}

impl Placement {
    pub fn label(self) -> &'static str {
        match self {
            Self::Trigger => "Replace Trigger",
            Self::Requirement => "Add Requirement",
            Self::Action => "Add Action",
        }
    }
}

#[derive(Clone)]
pub(super) struct Request {
    document: String,
    fingerprint: String,
    effect: u16,
    group: usize,
    placement: Placement,
    node: NativeNode,
    stock_perk: Option<u16>,
}

impl Request {
    pub fn new(
        recipe: &PerkRecipe,
        effect: u16,
        group: usize,
        placement: Placement,
        node: NativeNode,
        stock_perk: Option<u16>,
    ) -> Result<Self, String> {
        if !recipe
            .effects
            .iter()
            .any(|entry| entry.source_perk_index == effect && entry.program.is_some())
        {
            return Err("Choose an authored effect in the open perk.".into());
        }
        Ok(Self {
            document: recipe.id.clone(),
            fingerprint: crate::perk::verification::recipe_hash(recipe)?,
            effect,
            group,
            placement,
            node,
            stock_perk,
        })
    }

    pub fn apply(&self, recipe: &mut PerkRecipe) -> Result<(), String> {
        if recipe.id != self.document
            || crate::perk::verification::recipe_hash(recipe)? != self.fingerprint
        {
            return Err(
                "The destination perk changed. Select the catalog configuration again.".into(),
            );
        }
        let mut changed = recipe.clone();
        let program = changed
            .effects
            .iter_mut()
            .find(|effect| effect.source_perk_index == self.effect)
            .and_then(|effect| effect.program.as_mut())
            .ok_or("The destination effect no longer exists.")?;
        program::native::insert_catalog_node(
            program,
            self.group,
            self.placement,
            self.node.clone(),
        )?;
        use sha2::{Digest, Sha256};
        changed.sources.push(crate::perk::CatalogSource {
            effect: self.effect,
            group: self.group,
            role: self.placement.label().into(),
            kind: self.node.kind,
            stock_perk: self.stock_perk,
            native_sha256: hex::encode_upper(Sha256::digest(&self.node.bytes)),
            client_build: sundial::package_authoring::sandbox_perk::nodes::CLIENT_BUILD.into(),
        });
        *recipe = changed;
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct Picker {
    pub destination: Option<PerkRecipe>,
    pub requested: Option<Request>,
    effect: usize,
    group: usize,
    configuration: usize,
    placement: Placement,
    source: Option<(Family, u8, Option<u16>)>,
    error: Option<String>,
}

impl Picker {
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        selected: Option<(Family, u8)>,
        stock: Option<u16>,
        discovery: &discovery::Discovery,
    ) {
        let Some(recipe) = self.destination.as_ref() else {
            return;
        };
        let Some((family, kind)) = selected else {
            return;
        };
        if self.source != Some((family, kind, stock)) {
            self.source = Some((family, kind, stock));
            self.configuration = 0;
            self.placement = if family == Family::Effects {
                Placement::Action
            } else {
                Placement::Requirement
            };
        }
        let nodes = if let Some(stock) = stock {
            discovery
                .behavior(stock)
                .and_then(|behavior| behavior.program.as_ref())
                .and_then(|program| native_draft(program).ok())
                .and_then(|native| native.graph.emit().ok())
                .and_then(|payload| action::decode(&payload).ok())
                .map(|decoded| match family {
                    Family::Effects => decoded
                        .effects()
                        .filter(|node| node.kind == kind)
                        .map(|node| {
                            (
                                NativeNode {
                                    kind,
                                    bytes: node.native.clone(),
                                },
                                node.description(),
                            )
                        })
                        .collect::<Vec<_>>(),
                    Family::Conditions => decoded
                        .conditions()
                        .into_iter()
                        .filter(|node| node.kind == kind)
                        .map(|node| {
                            (
                                NativeNode {
                                    kind,
                                    bytes: node.native.clone(),
                                },
                                node.description(),
                            )
                        })
                        .collect::<Vec<_>>(),
                })
                .unwrap_or_default()
        } else {
            match family {
                Family::Effects => NativeNode::effect(kind),
                Family::Conditions => NativeNode::condition(kind),
            }
            .map(|node| vec![(node, "Default Template".into())])
            .unwrap_or_default()
        };
        ui.group(|ui| {
            ui.strong(format!("Insert into {}", recipe.name));
            let effects = recipe.effects.iter().enumerate().filter(|(_, effect)| effect.program.is_some()).collect::<Vec<_>>();
            if !effects.iter().any(|(index, _)| *index == self.effect) { self.effect = effects.first().map_or(0, |(index, _)| *index); }
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("catalog-effect").selected_text(format!("Effect {}", self.effect + 1)).show_ui(ui, |ui| {
                    for (index, _) in &effects { ui.selectable_value(&mut self.effect, *index, format!("Effect {}", index + 1)); }
                });
                let groups = recipe.effects.get(self.effect).and_then(|effect| effect.program.as_ref()).and_then(|program| crate::perk::preflight::decoded(program).ok()).map_or(0, |decoded| decoded.groups.len());
                self.group = self.group.min(groups.saturating_sub(1));
                egui::ComboBox::from_id_salt("catalog-group").selected_text(format!("Behavior {}", self.group + 1)).show_ui(ui, |ui| {
                    for group in 0..groups { ui.selectable_value(&mut self.group, group, format!("Behavior {}", group + 1)); }
                });
                if family == Family::Conditions {
                    egui::ComboBox::from_id_salt("catalog-placement").selected_text(self.placement.label()).show_ui(ui, |ui| {
                        for placement in [Placement::Requirement, Placement::Trigger] { ui.selectable_value(&mut self.placement, placement, placement.label()); }
                    });
                }
                if ui.add_enabled(!effects.is_empty() && !nodes.is_empty() && groups > 0, egui::Button::new("Insert Selected Configuration")).clicked() {
                    let node = nodes[self.configuration.min(nodes.len() - 1)].0.clone();
                    match Request::new(recipe, recipe.effects[self.effect].source_perk_index, self.group, self.placement, node, stock) {
                        Ok(request) => { self.requested = Some(request); self.error = None; }
                        Err(error) => self.error = Some(error),
                    }
                }
            });
            self.configuration = self.configuration.min(nodes.len().saturating_sub(1));
            if let Some((_, description)) = nodes.get(self.configuration) {
                egui::ComboBox::from_id_salt("catalog-configuration").width(ui.available_width()).selected_text(description).show_ui(ui, |ui| {
                    for (index, (_, description)) in nodes.iter().enumerate() { ui.selectable_value(&mut self.configuration, index, format!("{} · {description}", index + 1)); }
                });
                ui.weak(stock.map_or_else(|| "Template configuration. Choose a stock use below to copy its native values.".into(), |stock| format!("Native configuration from stock effect {stock}. Source and content hash travel with the perk.")));
            } else { ui.weak("No editable configuration is available for this selection."); }
            if let Some(error) = &self.error { ui.colored_label(ui.visuals().error_fg_color, error); }
        });
    }
}
