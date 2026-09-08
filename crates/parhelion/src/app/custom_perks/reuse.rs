//! Existing custom perks remain reusable while authoring is unavailable.
use super::*;
#[cfg(test)]
mod tests;

#[derive(Clone)]
struct SavedPerk {
    weapon: String,
    variant: WeaponSocketPlugVariantRecipe,
}

#[derive(Default)]
pub(in crate::app) struct ReusePicker {
    entries: Vec<SavedPerk>,
    errors: Vec<String>,
    query: String,
}

impl ReusePicker {
    fn add_recipe(&mut self, recipe: &WeaponRecipe) {
        for variant in &recipe.overrides.socket_plug_variants {
            let mut variant = variant.clone();
            variant.socket_index = 0;
            variant.choice_index = 0;
            if !self.entries.iter().any(|entry| entry.variant == variant) {
                self.entries.push(SavedPerk {
                    weapon: recipe.name.clone(),
                    variant,
                });
            }
        }
    }

    fn load(library: Option<&RecipeLibrary>, draft: &WeaponRecipe) -> Self {
        let mut picker = Self::default();
        picker.add_recipe(draft);
        if let Some(library) = library {
            match library.scan() {
                Ok(scan) => {
                    picker.errors.extend(scan.errors);
                    for entry in scan.entries {
                        match WeaponRecipe::load_json(&entry.path) {
                            Ok(recipe) => picker.add_recipe(&recipe),
                            Err(error) => picker.errors.push(error.to_string()),
                        }
                    }
                }
                Err(error) => picker.errors.push(error),
            }
        }
        picker
    }
}

fn apply_saved_perk(
    recipe: &mut WeaponRecipe,
    donor: &WeaponDonor,
    socket_index: usize,
    saved: &WeaponSocketPlugVariantRecipe,
) -> Result<(), String> {
    let expanded = super::super::socket_editor::socket_editor_donor(donor, recipe);
    let donor = expanded.as_ref();
    let socket = donor
        .sockets
        .get(socket_index)
        .ok_or("Select a socket first")?;
    let socket_type = recipe
        .overrides
        .socket_columns
        .get(socket_index)
        .and_then(Option::as_ref)
        .and_then(|column| column.socket_type)
        .unwrap_or(socket.socket_type);
    let limit = authored_socket_choice_limit(socket_type);
    if limit == 0 {
        return Err("This socket does not accept perk choices. Change its role first.".into());
    }
    let inherited = inherited_socket_choices(
        socket.native_default,
        &socket.ordered_embedded_choices,
        limit,
    );
    let mut choices = recipe_socket_choices(recipe, socket_index, &inherited)?;
    let hash = saved
        .source_plug_hash
        .parse_u32()
        .map_err(|error| error.to_string())?;
    if choices.iter().skip(1).any(|choice| *choice == hash) {
        return Err(
            "This perk's source already appears as an extra choice. Remove that choice first."
                .into(),
        );
    }
    let target = u16::try_from(socket_index).map_err(|_| "Socket index is too large")?;
    if choices.is_empty() {
        choices.push(hash);
    } else {
        choices[0] = hash;
    }
    set_recipe_socket_column(
        recipe,
        donor.sockets.len(),
        socket_index,
        &inherited,
        choices,
        None,
    );
    let mut variant = saved.clone();
    variant.socket_index = target;
    variant.choice_index = 0;
    recipe
        .overrides
        .socket_plug_variants
        .retain(|entry| entry.socket_index != target || entry.choice_index != 0);
    recipe.overrides.socket_plug_variants.push(variant);
    Ok(())
}

impl PackageAuthoringApp {
    pub(super) fn draw_custom_perk_notice(&mut self, ctx: &egui::Context) {
        let Some(mut socket_index) = self.private_perk_socket else {
            self.custom_perk_reuse = None;
            return;
        };
        if self.build_receiver.is_some() || self.install_receiver.is_some() {
            return;
        }
        let donor = self.current_donor().map(|donor| {
            super::super::socket_editor::socket_editor_donor(&donor, &self.recipe).into_owned()
        });
        let mut open = true;
        let mut close = false;
        let mut browse = false;
        let mut selected = None;
        egui::Window::new(if self.custom_perk_reuse.is_some() { "Use Existing Custom Perk" } else { "Custom Perks" })
            .id(egui::Id::new("custom-perk-availability"))
            .open(&mut open).collapsible(false).default_width(440.0)
            .show(ctx, |ui| {
                if let Some(picker) = &mut self.custom_perk_reuse {
                    ui.label("Replaces the first choice in the selected socket. The source recipe stays unchanged.");
                    if let Some(donor) = &donor {
                        egui::ComboBox::from_id_salt("reuse-socket")
                            .selected_text(donor.sockets.get(socket_index).map_or("Select Socket", |socket| socket.label.as_str()))
                            .show_ui(ui, |ui| {
                                for socket in &donor.sockets {
                                    ui.selectable_value(&mut socket_index, socket.index, &socket.label);
                                }
                            });
                    }
                    ui.add(egui::TextEdit::singleline(&mut picker.query).hint_text("Search perks or source weapons…"));
                    let query = picker.query.trim().to_lowercase();
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for (index, entry) in picker.entries.iter().enumerate() {
                            let name = entry.variant.name.as_deref().unwrap_or("Custom Perk");
                            let label = format!("{name} · {}", entry.weapon);
                            if !label.to_lowercase().contains(&query) { continue; }
                            if ui.add_enabled(donor.is_some(), egui::Button::new(label)).clicked() { selected = Some(index); }
                        }
                        if picker.entries.is_empty() { ui.label("No saved custom perks found. Existing recipe perks appear here automatically."); }
                        for error in &picker.errors { ui.colored_label(ui.visuals().error_fg_color, error); }
                    });
                } else {
                    ui.label("Custom perk editing is planned for a future release.");
                    ui.label("You can build existing recipes or reuse a saved custom perk.");
                    browse = ui.button("Use Existing Custom Perk…").clicked();
                }
                close = ui.button("Close").clicked();
            });
        if browse {
            self.custom_perk_reuse = Some(ReusePicker::load(
                self.recipe_library.as_ref(),
                &self.recipe,
            ));
        }
        if let (Some(index), Some(donor), Some(picker)) =
            (selected, donor, self.custom_perk_reuse.as_mut())
        {
            match apply_saved_perk(
                &mut self.recipe,
                &donor,
                socket_index,
                &picker.entries[index].variant,
            ) {
                Ok(()) => {
                    self.plug_queries.clear();
                    close = true;
                }
                Err(error) => picker.errors.push(error),
            }
        }
        self.private_perk_socket = (open && !close).then_some(socket_index);
        if self.private_perk_socket.is_none() {
            self.custom_perk_reuse = None;
        }
    }
}
