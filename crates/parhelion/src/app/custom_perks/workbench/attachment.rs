//! Resolve exact socket choices and guard against edits made while a perk draft is open.
use super::*;

mod ability;
mod mod_item;
#[cfg(test)]
mod tests;

pub(super) use ability::Target as AbilityTarget;

/// What an apply from the footer changes: a socket choice, a subclass ability or node, or a mod,
/// whose perk is the mod itself. Each is boxed, since a socket target carries the socket's whole
/// plug variant and the others a perk.
pub(super) enum Applied {
    Socket(Box<Change>),
    Ability(Box<ability::Change>),
    Mod(Box<PerkRecipe>),
}

fn stock_variant(hash: u32) -> WeaponSocketPlugVariantRecipe {
    WeaponSocketPlugVariantRecipe {
        offer_everywhere: false,
        socket_index: 0,
        choice_index: 0,
        source_plug_hash: hash.into(),
        replace_effects: false,
        name: None,
        description: None,
        icon: None,
        classification_donor_hash: None,
        investment_stats: Vec::new(),
        additional_sandbox_perks: Vec::new(),
        sandbox_perks: Vec::new(),
    }
}

/// A stock perk opened from its socket, named the way New from Perk names a copy.
fn stock_copy(hash: u32, catalog: &InvestmentCatalog) -> PerkRecipe {
    let mut recipe = templates::from_variant(&stock_variant(hash), catalog);
    recipe.name = format!("Custom {}", recipe.name);
    recipe
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Target {
    namespace: String,
    donor: HexHash,
    socket: usize,
    choice: usize,
    socket_type: u16,
    choices: Arc<[u32]>,
    variant: Option<WeaponSocketPlugVariantRecipe>,
    /// Gear sockets take only plugs of their own kind, so a gear perk is built on the plug in its
    /// choice and never adds a choice.
    stock_based: bool,
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
        let stock_based = !weapon.kind.is_weapon();
        if stock_based && !crate::app::gear_view::perk_destination(row) {
            return Err("This socket takes no custom perk.".into());
        }
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
        if choice > choices.len() || choice >= limit || (stock_based && choice == choices.len()) {
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
            choices: choices.into(),
            variant,
            stock_based,
        })
    }

    /// The stock plug a gear perk replaces. A new perk for gear starts from it, since a gear
    /// socket only takes plugs of its own kind.
    pub(super) fn gear_plug(&self) -> Option<u32> {
        self.stock_based
            .then(|| self.choices.get(self.choice).copied())
            .flatten()
    }

    /// The socket role and the name of the perk in this choice.
    fn reading(&self, donor: &WeaponDonor, catalog: &InvestmentCatalog) -> (String, String) {
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
        (role, name)
    }

    pub(super) fn label(&self, donor: &WeaponDonor, catalog: &InvestmentCatalog) -> String {
        let (role, name) = self.reading(donor, catalog);
        if self.choice == self.choices.len() {
            format!("Add an Alternative · {role}")
        } else if self.stock_based && self.choices.len() == 1 {
            format!("Replace {name} · {role}")
        } else {
            format!("Replace {name} · {role} · Choice {}", self.choice + 1)
        }
    }

    /// The reading of a row that opens this choice's perk for editing.
    fn edit_label(&self, donor: &WeaponDonor, catalog: &InvestmentCatalog) -> String {
        let (role, name) = self.reading(donor, catalog);
        format!("Edit {name} · {role}")
    }

    /// Whether both name the same socket choice of the same item.
    fn same_choice(&self, other: &Self) -> bool {
        self.namespace == other.namespace
            && self.donor == other.donor
            && self.socket == other.socket
            && self.choice == other.choice
    }

    /// Whether this is the choice that would be added after the last one.
    fn adds(&self) -> bool {
        self.choice == self.choices.len()
    }

    pub(super) fn check(&self, weapon: &WeaponRecipe, donor: &WeaponDonor) -> Result<(), String> {
        if Self::capture(weapon, donor, self.socket, self.choice).as_ref() != Ok(self) {
            return Err(format!(
                "The {} changed. Select the destination again.",
                weapon.kind.noun()
            ));
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
            if let Some(issue) = crate::perk::preflight::check(perk)
                .into_iter()
                .find(|issue| issue.blocking)
            {
                return Err(issue.message);
            }
            let mut variant = perk.at_socket(target.socket as u16, target.choice as u16);
            if let Some(plug) = target.gear_plug() {
                // A gear socket only takes plugs of its own kind, so the perk is built on the
                // plug in its choice. Its text, stats and effects are its own. The socket keeps
                // its choices, pinned so a random roll cannot replace the perk.
                variant.source_plug_hash = plug.into();
                if weapon
                    .overrides
                    .socket_columns
                    .get(target.socket)
                    .and_then(Option::as_ref)
                    .is_none()
                {
                    socket_editor::materialize_socket_column(
                        weapon,
                        donor.sockets.len(),
                        target.socket,
                        &target.choices,
                        false,
                    );
                }
                weapon.overrides.socket_plug_variants.retain(|entry| {
                    usize::from(entry.socket_index) != target.socket
                        || usize::from(entry.choice_index) != target.choice
                });
                weapon.overrides.socket_plug_variants.push(variant);
                return Ok(());
            }
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
            let mut choices = target.choices.to_vec();
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
            if let Some(&hash) = target.choices.get(target.choice)
                && choice_conflicts(
                    weapon,
                    target.socket,
                    target.choice,
                    hash,
                    None,
                    &target.choices,
                )
            {
                return Err(
                    "This socket already holds the stock perk. Remove one choice first.".into(),
                );
            }
            weapon.overrides.socket_plug_variants.retain(|entry| {
                usize::from(entry.socket_index) != target.socket
                    || usize::from(entry.choice_index) != target.choice
            });
        }
        Ok(())
    }
}

fn targets(weapon: &WeaponRecipe, donor: &WeaponDonor, include_new: bool) -> Vec<Target> {
    let variants = weapon
        .overrides
        .socket_plug_variants
        .iter()
        .map(|variant| {
            (
                (
                    usize::from(variant.socket_index),
                    usize::from(variant.choice_index),
                ),
                variant,
            )
        })
        .collect::<BTreeMap<_, _>>();
    donor
        .sockets
        .iter()
        .flat_map(|socket| {
            let Ok(mut base) = Target::capture(weapon, donor, socket.index, 0) else {
                return Vec::new();
            };
            base.variant = None;
            let count = if base.stock_based {
                base.choices.len()
            } else {
                (base.choices.len() + usize::from(include_new))
                    .min(authored_socket_choice_limit(base.socket_type))
            };
            // Every destination shares the same immutable choice snapshot, even for large columns.
            (0..count)
                .map(|choice| Target {
                    choice,
                    variant: variants
                        .get(&(socket.index, choice))
                        .map(|variant| (*variant).clone()),
                    ..base.clone()
                })
                .collect()
        })
        .collect()
}

impl Workbench {
    pub(super) fn open_target(&mut self, target: Target, catalog: &InvestmentCatalog) {
        self.picker = None;
        self.initialize();
        self.message = None;
        self.message_path = None;
        // The copy already open for this socket choice, whatever the choice holds now. Its
        // destination follows the choice, so applying from it updates the same place.
        if let Some(index) = self.documents.iter().position(|document| {
            document.from_socket
                && document
                    .target
                    .as_ref()
                    .is_some_and(|open| open.same_choice(&target))
        }) {
            self.documents[index].target = Some(target);
            self.select_document(index);
        } else if let Some(&hash) = target.choices.get(target.choice) {
            let recipe = match &target.variant {
                Some(variant) => templates::from_variant(variant, catalog),
                None => stock_copy(hash, catalog),
            };
            let mut document = Document::new(recipe, None);
            document.target = Some(target);
            document.from_socket = true;
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
        ui.strong(format!("{} Sockets", weapon.kind.label()));
        crate::app::style::hint(ui, &weapon.name);
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
                            .filter(|document| document.from_socket)
                            .and_then(|document| document.target.as_ref())
                            == Some(&target);
                        let label = target.edit_label(donor, catalog);
                        if crate::app::style::list_row(ui, selected, &label).clicked() {
                            picked = Some(target);
                        }
                    }
                });
            });
        if let Some(target) = picked {
            self.open_target(target, catalog);
        }
    }

    /// Gives the selected perk the last destination when it has none of its own, and follows
    /// its destination when the weapon changed under it.
    fn sync_destination(&mut self, weapon: &WeaponRecipe, donor: &WeaponDonor) {
        let Some(document) = self.documents.get_mut(self.selected) else {
            return;
        };
        if document.target.is_none()
            && let Some(last) = &self.destination
        {
            // A perk with no destination of its own takes the last one, but not the custom
            // perk another document just put there: it takes the next free choice of that
            // socket, so applying it adds beside the other perk rather than over it.
            let taken = last.variant.is_some() && !last.stock_based;
            document.target = taken
                .then(|| Target::capture(weapon, donor, last.socket, last.choices.len()).ok())
                .flatten()
                .or_else(|| Some(last.clone()));
        }
        if let Some(target) = &document.target
            && target.check(weapon, donor).is_err()
        {
            // The weapon changed under this perk, such as a Recipe discard or an apply from
            // another perk. Follow the same socket choice so the caption names what sits
            // there now, and a choice that was to be added stays the one to be added. A
            // choice that no longer exists clears the destination.
            let choice = if target.adds() {
                Target::capture(weapon, donor, target.socket, 0)
                    .map_or(target.choice, |now| now.choices.len())
            } else {
                target.choice
            };
            document.target = Target::capture(weapon, donor, target.socket, choice).ok();
        }
        self.destination = document.target.clone();
    }

    pub(super) fn draw_attachment(
        &mut self,
        ui: &mut egui::Ui,
        weapon: &WeaponRecipe,
        donor: Option<&WeaponDonor>,
        catalog: Option<&InvestmentCatalog>,
    ) -> Option<Change> {
        self.documents.get(self.selected)?;
        let issue = self.selected_issue();
        if let Some(issue) = &issue {
            self.draw_issue(ui, issue);
        }
        let (Some(donor), Some(catalog)) = (donor, catalog) else {
            if issue.is_none() {
                ui.weak("No recipe open.");
            }
            return None;
        };
        let edit_issue = self.edit_issue();
        self.sync_destination(weapon, donor);
        let document = self.documents.get_mut(self.selected)?;
        let mut result = None;
        ui.add_enabled_ui(self.editor.is_none(), |ui| {
            ui.horizontal(|ui| {
                let kind = weapon.kind.label();
                ui.label(format!("{kind} Socket"));
                // A destination names a socket, a choice and the perk that sits there, so
                // the reading runs long. A combo takes the width of its selected text, and
                // an unbounded one here pushed the actions off the row. A fixed allocation
                // plus truncation holds it, and the whole reading stays on hover. The floor
                // is the width of the unset reading.
                let width = (ui.available_width() * 0.45).clamp(200.0, 340.0);
                let destination = document.target.as_ref().map_or_else(
                    || "Select Socket and Choice".to_owned(),
                    |target| target.label(donor, catalog),
                );
                controls::sized(ui, width, |ui| {
                    egui::ComboBox::from_id_salt("perk-destination")
                        .width(width)
                        .truncate()
                        .selected_text(destination.clone())
                        .show_ui(ui, |ui| {
                            for target in targets(weapon, donor, true) {
                                let label = target.label(donor, catalog);
                                if ui
                                    .selectable_label(
                                        document.target.as_ref() == Some(&target),
                                        label,
                                    )
                                    .clicked()
                                {
                                    document.target = Some(target);
                                }
                            }
                        })
                        .response
                        .on_hover_text(destination);
                    pickers::name_combo(ui, "perk-destination", "Destination Socket");
                });
                // The action and anything standing in its way sit together at the right. Save
                // Recipe closes the flow, since an applied perk lives in the unsaved recipe.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(self.recipe_unsaved, egui::Button::new("Save Recipe"))
                        .on_disabled_hover_text("Recipe saved.")
                        .clicked()
                    {
                        self.save_recipe_requested = true;
                    }
                    ui.separator();
                    // A warning is shown above but does not hold the perk back.
                    let blocking = issue.as_ref().filter(|issue| issue.blocking);
                    let ready =
                        document.target.is_some() && blocking.is_none() && edit_issue.is_none();
                    let apply = crate::app::style::primary(ui, &format!("Apply to {kind}"));
                    if ui
                        .add_enabled(ready, apply)
                        .on_disabled_hover_text(
                            edit_issue
                                .or_else(|| blocking.map(|issue| issue.message.as_str()))
                                .unwrap_or("Choose a socket and choice."),
                        )
                        .clicked()
                    {
                        result = document.target.clone().map(|target| Change {
                            target,
                            perk: Some(document.recipe.clone()),
                        });
                    }
                    if let Some(target) = document
                        .target
                        .as_ref()
                        .filter(|target| target.variant.is_some())
                        && let Some(hash) = target.choices.get(target.choice)
                    {
                        let source = catalog.plug_label(*hash, false);
                        if ui
                            .add(egui::Button::new(format!("Use Stock {source}")).truncate())
                            .on_hover_text(format!("Use Stock {source}"))
                            .clicked()
                        {
                            result = Some(Change {
                                target: target.clone(),
                                perk: None,
                            });
                        }
                    }
                });
            });
        });
        self.destination = document.target.clone();
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
            // Only an untouched copy of the socket's perk follows the socket back to stock.
            // A copy that was edited, and any other perk sent there, stays open as it is.
            if restored
                && document.untouched_copy()
                && let Some(target) = &next
                && let Some(hash) = target.choices.get(target.choice)
            {
                *document = Document::new(stock_copy(*hash, catalog), None);
                document.from_socket = true;
            }
            document.target = next;
        }
        self.error = None;
        self.message_path = None;
        self.message = Some(
            if restored {
                "Replaced the custom perk with its stock source."
            } else {
                "Applied to the selected choice."
            }
            .into(),
        );
        self.persist_drafts();
    }
}

impl PackageAuthoringApp {
    /// Opens what a page asked the workbench for: a perk of a subclass ability or node, or a
    /// socket choice to edit or to pick a perk for.
    fn open_perk_request(
        &self,
        workbench: &mut Workbench,
        request: Option<Request>,
        donor: Option<&WeaponDonor>,
    ) {
        match request {
            Some(Request::Ability { place, perk }) => {
                let copy = match perk {
                    AbilityPerk::Stock(index) => Some(self.stock_perk_copy(index)),
                    AbilityPerk::New | AbilityPerk::Custom(_) => None,
                };
                workbench.open_ability(&self.recipe, (place, perk), copy);
            }
            Some(Request::Mod) => {
                if let Some(catalog) = &self.catalog {
                    workbench.open_mod(&self.recipe, catalog);
                }
            }
            Some(
                request @ (Request::EditChoice { socket, choice }
                | Request::SelectChoice { socket, choice }),
            ) => {
                let (Some(donor), Some(catalog)) = (donor, &self.catalog) else {
                    return;
                };
                match Target::capture(&self.recipe, donor, socket, choice) {
                    Ok(target) if matches!(request, Request::EditChoice { .. }) => {
                        workbench.open_target(target, catalog);
                    }
                    Ok(target) => workbench.open_picker(
                        target,
                        catalog,
                        self.recipe_library.as_ref(),
                        &self.recipe,
                    ),
                    Err(error) => {
                        workbench.error = Some(error);
                        workbench.open = true;
                    }
                }
            }
            None => {}
        }
    }

    pub(in crate::app) fn draw_perk_workbench(&mut self, ctx: &egui::Context) {
        if self.build_receiver.is_some() || self.install_receiver.is_some() {
            return;
        }
        let mut workbench = std::mem::take(&mut self.perk_workbench);
        let donor = self
            .current_item_donor()
            .map(|donor| socket_editor::socket_editor_donor(&donor, &self.recipe).into_owned());
        workbench.ability_places = self.subclass_places();
        let request = self.perk_request.take();
        self.open_perk_request(&mut workbench, request, donor.as_ref());
        if workbench.picker.is_some() {
            if let (Some(donor), Some(catalog)) = (&donor, &self.catalog) {
                if workbench.show_picker(ctx, &mut self.recipe, donor, catalog) {
                    self.plug_queries.clear();
                }
            } else {
                workbench.picker = None;
            }
            self.perk_workbench = workbench;
            return;
        }
        if workbench.open
            && workbench.authored_templates.is_none()
            && let Some(catalog) = &self.catalog
        {
            workbench.load_authored_templates(self.recipe_library.as_ref(), &self.recipe, catalog);
        }
        workbench.recipe_unsaved = self.recipe_dirty && self.invalid_weapon_name.is_none();
        let change = workbench.show(
            ctx,
            &self.packages,
            self.catalog.as_ref(),
            &self.sandbox_perk_choices,
            self.show_experimental_options,
            (&self.recipe, donor.as_ref()),
        );
        match change {
            Some(Applied::Socket(change)) => {
                if let (Some(donor), Some(catalog)) = (donor, &self.catalog) {
                    match change.apply(&mut self.recipe, &donor) {
                        Ok(()) => {
                            workbench.applied(&self.recipe, &donor, catalog, change.perk.is_none());
                            self.plug_queries.clear();
                            // Save Recipe reads this on the next frame of the workbench.
                            self.synchronize_recipe_dirty();
                        }
                        Err(error) => workbench.error = Some(error),
                    }
                }
            }
            Some(Applied::Ability(change)) => match change.apply(&mut self.recipe) {
                Ok(()) => {
                    workbench.applied_to_ability(&change);
                    self.synchronize_recipe_dirty();
                }
                Err(error) => workbench.error = Some(error),
            },
            Some(Applied::Mod(perk)) => {
                let applied = self.catalog.as_ref().map_or_else(
                    || Err("The catalog is still loading.".to_owned()),
                    |catalog| crate::app::mod_view::apply_perk(&mut self.recipe, &perk, catalog),
                );
                match applied {
                    Ok(()) => {
                        workbench.applied_to_mod(&self.recipe);
                        self.plug_queries.clear();
                        self.synchronize_recipe_dirty();
                    }
                    Err(error) => workbench.error = Some(error),
                }
            }
            None => {}
        }
        let save_recipe = std::mem::take(&mut workbench.save_recipe_requested);
        self.perk_workbench = workbench;
        if save_recipe {
            self.save_open_recipe();
        }
    }
}
