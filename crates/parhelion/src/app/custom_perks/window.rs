//! Custom perk naming, native classification and optional runtime tuning in a dedicated window.
use super::*;
mod activation;
mod display;
mod stats;
#[cfg(test)]
mod tests;

impl PackageAuthoringApp {
    pub(in crate::app) fn draw_custom_perks_window(&mut self, ctx: &egui::Context) {
        if !authoring_available() {
            self.draw_custom_perk_notice(ctx);
            return;
        }
        // Parameter details replace this view; never draw a disabled parent over them.
        if self.build_receiver.is_some()
            || self.install_receiver.is_some()
            || self.perk_editor.is_some()
        {
            return;
        }
        let Some(socket_index) = self.private_perk_socket else {
            return;
        };
        let Some(donor) = self.current_donor() else {
            return;
        };
        let Some(socket) = donor.sockets.get(socket_index) else {
            self.private_perk_socket = None;
            return;
        };
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        let socket_type = self
            .recipe
            .overrides
            .socket_columns
            .get(socket_index)
            .and_then(Option::as_ref)
            .and_then(|column| column.socket_type)
            .unwrap_or(socket.socket_type);
        let inherited = inherited_socket_choices(
            socket.native_default,
            &socket.ordered_embedded_choices,
            authored_socket_choice_limit(socket_type),
        );
        let choices = match recipe_socket_choices(&self.recipe, socket_index, &inherited) {
            Ok(choices) => choices,
            Err(error) => {
                self.log.push(LogEntry::error(error));
                self.private_perk_socket = None;
                return;
            }
        };
        let mut open = true;
        let mut selected_socket = socket_index;
        draw_private_socket_variants(
            ctx,
            PrivateSocketContext {
                catalog,
                packages: &self.packages,
                sandbox_perk_choices: &self.sandbox_perk_choices,
                socket_index,
                donor: &donor,
                experimental: self.show_experimental_options,
            },
            &mut self.recipe,
            &choices,
            &mut self.perk_editor,
            &mut open,
            &mut selected_socket,
        );
        self.private_perk_socket = open.then_some(selected_socket);
    }
}

struct PrivateSocketContext<'a> {
    catalog: &'a InvestmentCatalog,
    packages: &'a Path,
    sandbox_perk_choices: &'a [WeaponSandboxPerkChoice],
    socket_index: usize,
    donor: &'a WeaponDonor,
    experimental: bool,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum CustomPerkPage {
    #[default]
    Display,
    Effects,
    Parameters,
}

fn draw_private_socket_variants(
    ctx: &egui::Context,
    context: PrivateSocketContext<'_>,
    recipe: &mut WeaponRecipe,
    choices: &[u32],
    editor: &mut Option<PerkEditor>,
    detail_open: &mut bool,
    selected_socket: &mut usize,
) {
    let detail_id = custom_perks_window_id();
    let mut done = false;
    egui::Window::new(format!("Custom Perks · {}", recipe.name))
        .id(detail_id)
        .open(detail_open)
        .default_width(620.0)
        .default_height(360.0)
        .vscroll(true)
        .min_height(
            (if recipe.overrides.socket_plug_variants.is_empty() {
                320.0_f32
            } else {
                560.0_f32
            })
            .min((ctx.screen_rect().height() - 96.0).max(200.0)),
        )
        .max_width((ctx.screen_rect().width() - 32.0).max(320.0))
        .max_height((ctx.screen_rect().height() - 64.0).max(240.0))
        .show(ctx, |ui| {
            done = draw_contents(ui, context, recipe, choices, editor, selected_socket);
        });
    if done {
        *detail_open = false;
    }
}

fn draw_contents(
    ui: &mut egui::Ui,
    context: PrivateSocketContext<'_>,
    recipe: &mut WeaponRecipe,
    choices: &[u32],
    editor: &mut Option<PerkEditor>,
    selected_socket: &mut usize,
) -> bool {
    let PrivateSocketContext {
        catalog,
        packages,
        sandbox_perk_choices,
        socket_index,
        donor,
        experimental,
    } = context;
    let plug_perks = choices
        .iter()
        .copied()
        .enumerate()
        .map(|(choice_index, plug_hash)| {
            (
                choice_index,
                plug_hash,
                catalog.item_sandbox_perk_indices(plug_hash),
            )
        })
        .collect::<Vec<_>>();
    let mut done = false;
    workbench_style(ui);
    ui.horizontal_wrapped(|ui| {
        if ui.button("Done").clicked() {
            done = true;
        }
        ui.label("Edits update this recipe. Save Recipe to keep them.");
    });
    let role_label = |index| custom_socket_label(catalog, donor, recipe, index);
    ui.label("Perk to Customize");
    egui::ComboBox::from_id_salt("private-perks-socket")
        .width(ui.available_width())
        .truncate()
        .selected_text(role_label(socket_index))
        .show_ui(ui, |ui| {
            for socket in &donor.sockets {
                ui.selectable_value(selected_socket, socket.index, role_label(socket.index));
            }
        });
    if *selected_socket != socket_index {
        return done;
    }
    ui.label("Stock perks and other weapons are unchanged.")
            .on_hover_text("A custom perk is a cloned plug item. It can supply several sandbox perks; tuning one preserves its siblings. Stock perks and other weapons are unchanged.");
    editor::guided_support_notice(ui);
    ui.separator();
    if plug_perks.is_empty() {
        ui.label("This socket has no selected perks. Add a choice on the Weapon tab first.");
    }
    let mut open_request = None;
    let mut copy_request = None;
    let mut remove_plug_request = None;
    for (choice_index, plug_hash, perk_indices) in &plug_perks {
        ui.add_space(3.0);
        let variant = recipe
            .overrides
            .socket_plug_variants
            .iter()
            .find(|variant| {
                usize::from(variant.socket_index) == socket_index
                    && usize::from(variant.choice_index) == *choice_index
                    && variant.source_plug_hash.parse_u32().ok() == Some(*plug_hash)
            });
        if let Some(warning) = custom_perk_projection_warning(perk_indices.len(), variant) {
            ui.colored_label(ui.visuals().warn_fg_color, warning);
        }
        ui.horizontal_wrapped(|ui| {
            if choices.len() > 1 {
                ui.strong(format!("Choice {}", choice_index + 1));
            }
            ui.label(format!(
                "Original perk: {}",
                catalog.plug_label(*plug_hash, false)
            ));
        });
        if perk_indices.is_empty() {
            ui.weak("This plug has no editable gameplay perks.");
            continue;
        }
        let has_private_copy = variant.is_some();
        let page_id = ui.id().with((
            "custom-perk-page",
            &recipe.namespace,
            socket_index,
            choice_index,
            plug_hash,
        ));
        let mut page = ui
            .ctx()
            .data_mut(|data| data.get_temp::<CustomPerkPage>(page_id).unwrap_or_default());
        if has_private_copy {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut page, CustomPerkPage::Display, "Display");
                ui.selectable_value(&mut page, CustomPerkPage::Effects, "Effects");
                ui.selectable_value(
                    &mut page,
                    CustomPerkPage::Parameters,
                    "Parameters & Support",
                );
            });
            ui.ctx().data_mut(|data| data.insert_temp(page_id, page));
            ui.separator();
        }
        if !has_private_copy {
            copy_request =
                draw_create_custom_perk(ui, socket_index, *choice_index, *plug_hash, perk_indices)
                    .or(copy_request);
        }
        draw_custom_perk_page(
            ui,
            &context,
            recipe,
            (*choice_index, *plug_hash),
            perk_indices,
            page,
        );
        if !has_private_copy || page == CustomPerkPage::Parameters {
            ui.strong("Parameters & Support");
            for (effect_position, &perk_index) in perk_indices.iter().enumerate() {
                let (Ok(socket_index_u16), Ok(choice_index_u16)) =
                    (u16::try_from(socket_index), u16::try_from(*choice_index))
                else {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        "Socket or choice index exceeds the private-variant format",
                    );
                    continue;
                };
                let key = PerkEditorKey {
                    socket_index: socket_index_u16,
                    choice_index: choice_index_u16,
                    source_plug_hash: *plug_hash,
                    source_perk_index: perk_index,
                };
                let existing = private_perk(recipe, key);
                let edit_count = existing.map_or(0, |perk| {
                    perk.runtime_values.len()
                        + perk.action_float_values.len()
                        + usize::from(perk.activation.is_some())
                });
                ui.push_id(("parameter-support", socket_index, choice_index, perk_index), |ui| {
                    let effect = sandbox_perk_choices.iter().find(|effect| effect.perk_index == perk_index);
                    let name = effect.map_or_else(|| format!("Effect {perk_index}"), |effect| effect.representative_name.clone());
                    let label = if perk_indices.len() > 1 { format!("Effect {} · {name}", effect_position + 1) } else { name };
                    ui.strong(label).on_hover_text(format!("Sandbox effect index: {perk_index}"));
                    if existing.is_some() {
                        ui.label(format!(
                            "Custom perk · {edit_count} runtime edit{}",
                            if edit_count == 1 { "" } else { "s" }
                        ));
                    }
                    let guided = editor::has_guided_profile(perk_index);
                    ui.label(if guided { "Projectile speed mapping available; package data is checked when opened." } else { "No guided parameters mapped for this effect." });
                    let label = if guided { "Check & Edit Parameters…" } else { "Inspect Advanced Parameters…" };
                    let can_inspect = guided || experimental || edit_count > 0;
                    if ui
                        .add_enabled(can_inspect, egui::Button::new(label))
                        .on_disabled_hover_text("No guided controls are mapped. Enable advanced technical controls in Preferences to inspect unverified package fields.")
                        .clicked()
                    {
                        ui.ctx().data_mut(|data| data.insert_temp(page_id, CustomPerkPage::Parameters));
                        open_request = Some((
                            key,
                            catalog.plug_label(*plug_hash, false),
                            existing.map(|perk| perk.runtime_values.clone()).unwrap_or_default(),
                        ));
                    }
                });
                ui.add_space(6.0);
            }
        }
        if has_private_copy {
            ui.separator();
            ui.push_id(("custom-copy-restore", socket_index, choice_index, plug_hash), |ui| {
                    let confirm_id = ui.id().with("restore-original");
                    let mut confirming = ui.ctx().data_mut(|data| data.get_temp::<bool>(confirm_id).unwrap_or(false));
                    if ui.button("Restore Original Perk…").clicked() { confirming = true; }
                    if confirming {
                        ui.label("Remove this choice's custom text, classification, stat bonuses, added effects, conditions and parameter edits?");
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Restore Original Perk").clicked() {
                                remove_plug_request = Some(*choice_index);
                                confirming = false;
                            }
                            if ui.button("Keep Custom Perk").clicked() { confirming = false; }
                        });
                    }
                    ui.ctx().data_mut(|data| data.insert_temp(confirm_id, confirming));
                });
        }
    }
    if let Some(choice_index) = remove_plug_request {
        recipe.overrides.socket_plug_variants.retain(|variant| {
            usize::from(variant.socket_index) != socket_index
                || usize::from(variant.choice_index) != choice_index
        });
    }
    if let Some(key) = copy_request {
        upsert_private_perk_runtime_values(recipe, key, Vec::new());
    }
    if let Some((key, plug_label, draft)) = open_request {
        let action_values = private_perk(recipe, key)
            .map(|perk| perk.action_float_values.clone())
            .unwrap_or_default();
        *editor = Some(PerkEditor::open(
            packages.to_path_buf(),
            key,
            plug_label,
            draft,
            action_values,
            ui.ctx(),
        ));
    }
    done
}

pub(super) fn custom_perks_window_id() -> egui::Id {
    egui::Id::new("parhelion-custom-perks")
}

fn draw_custom_perk_page(
    ui: &mut egui::Ui,
    context: &PrivateSocketContext<'_>,
    recipe: &mut WeaponRecipe,
    choice: (usize, u32),
    perk_indices: &[u16],
    page: CustomPerkPage,
) {
    let (choice_index, plug_hash) = choice;
    let socket_index = context.socket_index;
    let Ok(donor_hash) = recipe.donor.item_hash.parse_u32() else {
        return;
    };
    let socket_type = recipe
        .overrides
        .socket_columns
        .get(socket_index)
        .and_then(Option::as_ref)
        .and_then(|column| column.socket_type);
    let Some(variant) = recipe
        .overrides
        .socket_plug_variants
        .iter_mut()
        .find(|variant| {
            usize::from(variant.socket_index) == socket_index
                && usize::from(variant.choice_index) == choice_index
                && variant.source_plug_hash.parse_u32().ok() == Some(plug_hash)
        })
    else {
        return;
    };
    ui.push_id(
        (
            "custom-perk-page-controls",
            socket_index,
            choice_index,
            plug_hash,
        ),
        |ui| match page {
            CustomPerkPage::Display => {
                display::draw(ui, context.catalog, donor_hash, socket_type, variant)
            }
            CustomPerkPage::Effects => {
                stats::draw(ui, context.catalog, context.donor, plug_hash, variant);
                activation::draw(ui, variant, context.experimental);
                draw_additional_effects(ui, variant, perk_indices, context.sandbox_perk_choices);
            }
            CustomPerkPage::Parameters => {}
        },
    );
}

fn draw_additional_effects(
    ui: &mut egui::Ui,
    variant: &mut WeaponSocketPlugVariantRecipe,
    source_perks: &[u16],
    choices: &[WeaponSandboxPerkChoice],
) {
    ui.strong("Additional Effects");
    ui.label("Each effect keeps its original behavior and activation conditions.");
    let mut remove_effect = None;
    for (index, &perk) in variant.additional_sandbox_perks.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.label(sandbox_perk_choice_label(perk, choices));
            if ui
                .small_button("Remove")
                .on_hover_text("Remove this additional effect from the recipe")
                .clicked()
            {
                remove_effect = Some(index);
            }
        });
    }
    if let Some(index) = remove_effect {
        variant.additional_sandbox_perks.remove(index);
    }
    egui::ComboBox::from_id_salt(("additional-private-effect", variant.choice_index))
        .selected_text("+ Add Effect")
        .show_ui(ui, |ui| {
            for effect in choices
                .iter()
                .filter(|effect| !source_perks.contains(&effect.perk_index))
            {
                if !variant
                    .additional_sandbox_perks
                    .contains(&effect.perk_index)
                    && ui
                        .selectable_label(
                            false,
                            sandbox_perk_choice_label(effect.perk_index, choices),
                        )
                        .clicked()
                {
                    variant.additional_sandbox_perks.push(effect.perk_index);
                }
            }
        });
}

fn draw_create_custom_perk(
    ui: &mut egui::Ui,
    socket_index: usize,
    choice_index: usize,
    plug_hash: u32,
    perk_indices: &[u16],
) -> Option<PerkEditorKey> {
    if !ui
        .button("Create Custom Perk")
        .on_hover_text("Create a recipe-only custom copy. No runtime edits are required.")
        .clicked()
    {
        return None;
    }
    Some(PerkEditorKey {
        socket_index: u16::try_from(socket_index).ok()?,
        choice_index: u16::try_from(choice_index).ok()?,
        source_plug_hash: plug_hash,
        source_perk_index: *perk_indices.first()?,
    })
}

fn custom_socket_label(
    catalog: &InvestmentCatalog,
    donor: &WeaponDonor,
    recipe: &WeaponRecipe,
    index: usize,
) -> String {
    let role = socket_editor::socket_role_label(
        catalog,
        donor,
        index,
        recipe
            .overrides
            .socket_columns
            .get(index)
            .and_then(Option::as_ref)
            .and_then(|column| column.socket_type),
    );
    let Some(socket) = donor.sockets.get(index) else {
        return role;
    };
    let socket_type = recipe
        .overrides
        .socket_columns
        .get(index)
        .and_then(Option::as_ref)
        .and_then(|column| column.socket_type)
        .unwrap_or(socket.socket_type);
    let inherited = inherited_socket_choices(
        socket.native_default,
        &socket.ordered_embedded_choices,
        authored_socket_choice_limit(socket_type),
    );
    match recipe_socket_choices(recipe, index, &inherited) {
        Ok(choices) if !choices.is_empty() => format!(
            "{role} · {}{}",
            recipe
                .overrides
                .socket_plug_variants
                .iter()
                .find(|variant| usize::from(variant.socket_index) == index
                    && variant.choice_index == 0
                    && variant.source_plug_hash.parse_u32().ok() == Some(choices[0]))
                .and_then(|variant| variant.name.clone())
                .unwrap_or_else(|| catalog.plug_label(choices[0], false)),
            if choices.len() > 1 {
                " (Alternatives)"
            } else {
                ""
            }
        ),
        Ok(_) => format!("{role} · Empty"),
        Err(_) => format!("{role} · Invalid Choices"),
    }
}

fn custom_perk_projection_warning(
    source_count: usize,
    variant: Option<&WeaponSocketPlugVariantRecipe>,
) -> Option<&'static str> {
    sundial::package_authoring::sandbox_perk::sunrise_perk_projection_warning(
        source_count + variant.map_or(0, |variant| variant.additional_sandbox_perks.len()),
    )
}
