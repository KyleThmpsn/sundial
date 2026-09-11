//! Resolve exact socket choices and guard against edits made while a perk draft is open.
use super::*;

fn stock_variant(hash: u32) -> WeaponSocketPlugVariantRecipe {
    WeaponSocketPlugVariantRecipe {
        socket_index: 0,
        choice_index: 0,
        source_plug_hash: hash.into(),
        replace_effects: false,
        name: None,
        description: None,
        classification_donor_hash: None,
        investment_stats: Vec::new(),
        additional_sandbox_perks: Vec::new(),
        sandbox_perks: Vec::new(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Target {
    namespace: String,
    donor: HexHash,
    socket: usize,
    choice: usize,
    socket_type: u16,
    choices: Vec<u32>,
    variant: Option<WeaponSocketPlugVariantRecipe>,
}

impl Target {
    pub(super) fn capture(
        weapon: &WeaponRecipe,
        donor: &WeaponDonor,
        socket: usize,
        choice: usize,
    ) -> Result<Self, String> {
        let row = donor
            .sockets
            .get(socket)
            .ok_or("Select an available socket")?;
        let socket_type = weapon
            .overrides
            .socket_columns
            .get(socket)
            .and_then(Option::as_ref)
            .and_then(|column| column.socket_type)
            .unwrap_or(row.socket_type);
        let limit = authored_socket_choice_limit(socket_type);
        let inherited =
            inherited_socket_choices(row.native_default, &row.ordered_embedded_choices, limit);
        let choices = recipe_socket_choices(weapon, socket, &inherited)?;
        if choice > choices.len() || choice >= limit {
            return Err("This socket has no room for that choice.".into());
        }
        let variant = weapon
            .overrides
            .socket_plug_variants
            .iter()
            .find(|variant| {
                usize::from(variant.socket_index) == socket
                    && usize::from(variant.choice_index) == choice
            })
            .cloned();
        Ok(Self {
            namespace: weapon.namespace.clone(),
            donor: weapon.donor.item_hash.clone(),
            socket,
            choice,
            socket_type,
            choices,
            variant,
        })
    }

    fn label(&self, donor: &WeaponDonor, catalog: &InvestmentCatalog) -> String {
        let role =
            socket_editor::socket_role_label(catalog, donor, self.socket, Some(self.socket_type));
        let name = self
            .variant
            .as_ref()
            .and_then(|variant| variant.name.clone())
            .or_else(|| {
                self.choices
                    .get(self.choice)
                    .map(|hash| catalog.plug_label(*hash, false))
            })
            .unwrap_or_else(|| "New Choice".into());
        format!("{role} · Choice {} · {name}", self.choice + 1)
    }

    fn check(&self, weapon: &WeaponRecipe, donor: &WeaponDonor) -> Result<(), String> {
        if Self::capture(weapon, donor, self.socket, self.choice).as_ref() != Ok(self) {
            return Err("The weapon or socket choices changed while this perk was open. Select the destination again before applying.".into());
        }
        Ok(())
    }
}

pub(super) struct Change {
    pub(super) target: Target,
    pub(super) perk: Option<PerkRecipe>,
}

impl Change {
    pub(super) fn apply(
        &self,
        weapon: &mut WeaponRecipe,
        donor: &WeaponDonor,
    ) -> Result<(), String> {
        let expanded = socket_editor::socket_editor_donor(donor, weapon);
        let donor = expanded.as_ref();
        self.target.check(weapon, donor)?;
        let target = &self.target;
        if let Some(perk) = &self.perk {
            perk.validate()?;
            let variant = perk.at_socket(target.socket as u16, target.choice as u16);
            let hash = perk
                .template_plug
                .parse_u32()
                .map_err(|error| error.to_string())?;
            if choice_conflicts(
                weapon,
                target.socket,
                target.choice,
                hash,
                Some(&variant),
                &target.choices,
            ) {
                return Err("This perk already appears in another choice in this socket.".into());
            }
            let mut choices = target.choices.clone();
            if target.choice == choices.len() {
                choices.push(hash);
            } else {
                choices[target.choice] = hash;
            }
            let socket = &donor.sockets[target.socket];
            let inherited = inherited_socket_choices(
                socket.native_default,
                &socket.ordered_embedded_choices,
                authored_socket_choice_limit(target.socket_type),
            );
            set_recipe_socket_column(
                weapon,
                donor.sockets.len(),
                target.socket,
                &inherited,
                choices,
                None,
            );
            weapon.overrides.socket_plug_variants.retain(|entry| {
                usize::from(entry.socket_index) != target.socket
                    || usize::from(entry.choice_index) != target.choice
            });
            weapon.overrides.socket_plug_variants.push(variant);
        } else {
            weapon.overrides.socket_plug_variants.retain(|entry| {
                usize::from(entry.socket_index) != target.socket
                    || usize::from(entry.choice_index) != target.choice
            });
        }
        Ok(())
    }
}

fn targets(weapon: &WeaponRecipe, donor: &WeaponDonor, include_new: bool) -> Vec<Target> {
    donor
        .sockets
        .iter()
        .flat_map(|socket| {
            let mut rows = Vec::new();
            for choice in 0..sundial::investment::MAX_WEAPON_SOCKETS {
                let Ok(target) = Target::capture(weapon, donor, socket.index, choice) else {
                    break;
                };
                let new = choice == target.choices.len();
                if !new || include_new {
                    rows.push(target);
                }
                if new {
                    break;
                }
            }
            rows
        })
        .collect()
}

impl Workbench {
    pub(super) fn open_target(&mut self, target: Target, catalog: &InvestmentCatalog) {
        self.initialize();
        self.message = None;
        self.message_path = None;
        if let Some(index) = self
            .documents
            .iter()
            .position(|document| document.target.as_ref() == Some(&target))
        {
            self.selected = index;
        } else if let Some(&hash) = target.choices.get(target.choice) {
            let variant = target
                .variant
                .clone()
                .unwrap_or_else(|| stock_variant(hash));
            let mut document = Document::new(templates::from_variant(&variant, catalog), None);
            document.target = Some(target);
            self.add_document(document);
        }
        self.open = true;
    }

    pub(super) fn draw_weapon_perks(
        &mut self,
        ui: &mut egui::Ui,
        weapon: &WeaponRecipe,
        donor: &WeaponDonor,
        catalog: &InvestmentCatalog,
    ) {
        ui.strong("This Weapon");
        ui.small(&weapon.name);
        let mut picked = None;
        egui::ScrollArea::vertical()
            .id_salt("weapon-perk-choices")
            .max_height(160.0)
            .show(ui, |ui| {
                ui.add_enabled_ui(self.editor.is_none(), |ui| {
                    for target in targets(weapon, donor, false) {
                        let selected = self
                            .documents
                            .get(self.selected)
                            .and_then(|document| document.target.as_ref())
                            == Some(&target);
                        if crate::app::style::list_row(ui, selected, &target.label(donor, catalog))
                            .clicked()
                        {
                            picked = Some(target);
                        }
                    }
                });
            });
        if let Some(target) = picked {
            self.open_target(target, catalog);
        }
    }

    pub(super) fn draw_attachment(
        &mut self,
        ui: &mut egui::Ui,
        weapon: &WeaponRecipe,
        donor: Option<&WeaponDonor>,
        catalog: Option<&InvestmentCatalog>,
    ) -> Option<Change> {
        let (Some(donor), Some(catalog)) = (donor, catalog) else {
            ui.weak("Open a weapon recipe to apply this perk.");
            return None;
        };
        let document = self.documents.get_mut(self.selected)?;
        let mut result = None;
        ui.add_enabled_ui(self.editor.is_none(), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Destination");
                egui::ComboBox::from_id_salt("perk-destination").width(340.0)
                    .selected_text(document.target.as_ref().map_or_else(|| "Select Socket and Choice".into(), |target| target.label(donor, catalog)))
                    .show_ui(ui, |ui| {
                        for target in targets(weapon, donor, true) {
                            let label = target.label(donor, catalog);
                            if ui.selectable_label(document.target.as_ref() == Some(&target), label).clicked() { document.target = Some(target); }
                        }
                    });
                let ready = document.target.is_some() && document.recipe.validate().is_ok()
                    && document.recipe.effects.iter().all(|effect| self.discovery.perk_issue(effect.source_perk_index).is_none()
                        && effect.program.as_ref().is_none_or(|program| program.validate().is_ok()));
                if ui.add_enabled(ready, egui::Button::new("Apply to Weapon"))
                    .on_hover_text("Update only the selected socket choice. Save Recipe to keep the weapon changes.").clicked() {
                    result = document.target.clone().map(|target| Change { target, perk: Some(document.recipe.clone()) });
                }
                if document.target.as_ref().is_some_and(|target| target.variant.is_some())
                    && ui.button("Restore Original Perk").on_hover_text("Remove the selected choice's custom text, stats and effects. Other choices remain unchanged.").clicked() {
                    result = document.target.clone().map(|target| Change { target, perk: None });
                }
            });
        });
        result
    }

    pub(super) fn applied(
        &mut self,
        weapon: &WeaponRecipe,
        donor: &WeaponDonor,
        catalog: &InvestmentCatalog,
        restored: bool,
    ) {
        let Some(document) = self.documents.get_mut(self.selected) else {
            return;
        };
        if let Some(target) = &document.target {
            let next = Target::capture(weapon, donor, target.socket, target.choice).ok();
            if restored
                && let Some(target) = &next
                && let Some(hash) = target.choices.get(target.choice)
            {
                let recipe = templates::from_variant(&stock_variant(*hash), catalog);
                *document = Document::new(recipe, None);
            }
            document.target = next;
        }
        self.error = None;
        self.message_path = None;
        self.message = Some(
            if restored {
                "Restored the original perk. Save Recipe to keep it."
            } else {
                "Applied to the selected choice. Save Recipe to keep it."
            }
            .into(),
        );
        self.persist_drafts();
    }
}

impl PackageAuthoringApp {
    pub(in crate::app) fn draw_perk_workbench(&mut self, ctx: &egui::Context) {
        let mut workbench = std::mem::take(&mut self.perk_workbench);
        let donor = self
            .current_donor()
            .map(|donor| socket_editor::socket_editor_donor(&donor, &self.recipe).into_owned());
        if let Some(socket) = self.private_perk_socket.take()
            && let (Some(donor), Some(catalog)) = (&donor, &self.catalog)
        {
            match Target::capture(&self.recipe, donor, socket, 0) {
                Ok(target) => workbench.open_target(target, catalog),
                Err(error) => {
                    workbench.error = Some(error);
                    workbench.open = true;
                }
            }
        }
        if workbench.open && workbench.authored_templates.is_none() {
            workbench.load_authored_templates(self.recipe_library.as_ref(), &self.recipe);
        }
        let change = workbench.show(
            ctx,
            &self.packages,
            self.catalog.as_ref(),
            &self.sandbox_perk_choices,
            self.show_experimental_options,
            (&self.recipe, donor.as_ref()),
        );
        if let (Some(change), Some(donor), Some(catalog)) = (change, donor, &self.catalog) {
            match change.apply(&mut self.recipe, &donor) {
                Ok(()) => {
                    workbench.applied(&self.recipe, &donor, catalog, change.perk.is_none());
                    self.plug_queries.clear();
                }
                Err(error) => workbench.error = Some(error),
            }
        }
        self.perk_workbench = workbench;
    }
}
