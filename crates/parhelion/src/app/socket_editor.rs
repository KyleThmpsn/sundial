//! Focused socket editor controls; recipe mutation occurs on user actions.
use super::*;
use crate::recipe::RecipeDamageType;
use sundial::investment::WeaponDamageType;

mod row;
use row::{SocketRowContext, draw_socket_picker_row};

#[cfg(test)]
mod tests;

pub(super) fn socket_editor_donor<'a>(
    donor: &'a WeaponDonor,
    recipe: &WeaponRecipe,
) -> std::borrow::Cow<'a, WeaponDonor> {
    if recipe.overrides.socket_columns.len() <= donor.sockets.len() {
        return std::borrow::Cow::Borrowed(donor);
    }
    let mut expanded = donor.clone();
    for index in donor.sockets.len()..recipe.overrides.socket_columns.len() {
        let socket_type = recipe.overrides.socket_columns[index]
            .as_ref()
            .and_then(|column| column.socket_type)
            .unwrap_or(u16::MAX);
        expanded.sockets.push(sundial::investment::WeaponSocket {
            index,
            socket_type,
            label: format!("{}. Added Socket", index + 1),
            native_default: None,
            ordered_embedded_choices: Vec::new(),
            max_authored_choices: authored_socket_choice_limit(socket_type),
            compatible_plug_count: 0,
            reusable_plug_set_index: None,
            randomized_plug_set_index: None,
        });
    }
    std::borrow::Cow::Owned(expanded)
}

fn append_socket(recipe: &mut WeaponRecipe, native_socket_count: usize, socket_type: u16) -> bool {
    let socket_count = recipe
        .overrides
        .socket_columns
        .len()
        .max(native_socket_count);
    if socket_count >= sundial::investment::MAX_WEAPON_SOCKETS
        || authored_socket_choice_limit(socket_type) == 0
    {
        return false;
    }
    recipe
        .overrides
        .socket_columns
        .resize_with(socket_count, || None);
    recipe
        .overrides
        .socket_columns
        .push(Some(WeaponSocketColumnRecipe {
            socket_type: Some(socket_type),
            ..WeaponSocketColumnRecipe::default()
        }));
    true
}

fn remove_last_added_socket(recipe: &mut WeaponRecipe, socket_index: usize) -> bool {
    if socket_index + 1 != recipe.overrides.socket_columns.len() {
        return false;
    }
    recipe.overrides.socket_columns.pop();
    recipe
        .overrides
        .socket_plug_variants
        .retain(|variant| usize::from(variant.socket_index) != socket_index);
    if recipe.overrides.socket_columns.iter().all(Option::is_none) {
        recipe.overrides.socket_columns.clear();
    }
    true
}

fn remove_base_socket(recipe: &mut WeaponRecipe, socket_count: usize, socket_index: usize) {
    recipe.overrides.socket_columns.resize_with(
        recipe.overrides.socket_columns.len().max(socket_count),
        || None,
    );
    recipe.overrides.socket_columns[socket_index] = Some(WeaponSocketColumnRecipe {
        socket_type: Some(u16::MAX),
        ..Default::default()
    });
    recipe
        .overrides
        .socket_plug_variants
        .retain(|variant| usize::from(variant.socket_index) != socket_index);
}

fn draw_add_socket(
    ui: &mut egui::Ui,
    catalog: &InvestmentCatalog,
    recipe: &mut WeaponRecipe,
    donor: &WeaponDonor,
) {
    let socket_count = donor
        .sockets
        .len()
        .max(recipe.overrides.socket_columns.len());
    let can_add = socket_count < sundial::investment::MAX_WEAPON_SOCKETS;
    ui.horizontal(|ui| {
        ui.add_enabled_ui(can_add, |ui| {
            ui.menu_button("+ Add Socket", |ui| {
                ui.weak("Choose a role, then select the socket's first plug.");
                match catalog.weapon_socket_type_choices(donor.summary.hash) {
                    Ok(mut choices) => {
                        choices.sort_by_key(|choice| match choice.socket_type {
                            176 => 0,
                            92 => 1,
                            _ => 2,
                        });
                        egui::ScrollArea::vertical()
                            .max_height(320.0)
                            .show(ui, |ui| {
                                for choice in choices.into_iter().filter(|choice| {
                                    authored_socket_choice_limit(choice.socket_type) > 0
                                }) {
                                    if ui.button(&choice.label).clicked() {
                                        append_socket(
                                            recipe,
                                            donor.sockets.len(),
                                            choice.socket_type,
                                        );
                                        ui.close_menu();
                                    }
                                }
                            });
                    }
                    Err(error) => {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                }
            })
            .response
            .on_hover_text("Append a new socket to this weapon's native socket list");
        });
        ui.weak(format!(
            "{socket_count} / {} sockets",
            sundial::investment::MAX_WEAPON_SOCKETS
        ));
    });
}

pub(super) fn draw_socket_pickers(ui: &mut egui::Ui, context: SocketPickerContext<'_>) {
    let SocketPickerContext {
        catalog,
        recipe_library,
        recipe,
        queries,
        pages,
        plug_selection_mode,
        show_plug_safety_warnings,
        show_experimental_options,
        show_technical_rows,
        private_perk_socket,
        donor,
        log,
    } = context;
    ui.add_space(3.0);
    draw_socket_override_diagnostics(ui, catalog, recipe, donor, show_plug_safety_warnings);
    let bank = crate::weapon::perk_bank::project(recipe, donor, |hash| {
        catalog.item_sandbox_perk_indices(hash)
    });
    ui.label(format!(
        "Replicated Effect Bank: {} / 16",
        bank.default_count
    ));
    let solar = recipe.overrides.modern_damage_type.map_or(
        donor.summary.damage_type == Some(WeaponDamageType::Solar),
        |damage| damage == RecipeDamageType::Solar,
    );
    if bank.wave_frame && !solar {
        ui.colored_label(ui.visuals().warn_fg_color,
            "Wave Frame compatibility: native Wave spawning was verified with Solar damage on Mountaintop and Truthteller. The matched Arc Mountaintop controls spawned projectiles without Waves. Use Solar as the baseline for this effect. Other damage types remain unverified.");
    }
    if bank.default_count > 16 {
        ui.colored_label(ui.visuals().warn_fg_color, format!(
            "{} exceeds Sunrise's replicated effect capacity with the default plugs. Omitted from that path: {}. Authored effects are preserved.",
            recipe.name, bank.omitted.join(", ")
        ));
    } else if bank.maximum_count > 16 {
        ui.colored_label(ui.visuals().warn_fg_color, format!(
            "Some selectable plug combinations use up to {} replicated effects and exceed Sunrise's 16-entry bank. Later socket effects may be omitted from that path.", bank.maximum_count
        ));
    }
    if !recipe.overrides.socket_columns.is_empty()
        && recipe.overrides.socket_columns.len() < donor.sockets.len()
    {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!(
                    "This recipe has {} socket rows but {} has {}. The next selection will restore missing donor rows.",
                recipe.overrides.socket_columns.len(),
                donor.summary.name,
                donor.sockets.len()
            ),
        );
    }
    let native_socket_count = donor.sockets.len();
    let expanded_donor = socket_editor_donor(donor, recipe);
    let effective_donor = expanded_donor.as_ref();
    queries.resize_with(effective_donor.sockets.len(), BTreeMap::new);
    pages.resize(effective_donor.sockets.len(), 0);
    let is_inherited = |index| {
        recipe
            .overrides
            .socket_columns
            .get(index)
            .is_none_or(Option::is_none)
            && !recipe
                .overrides
                .socket_plug_variants
                .iter()
                .any(|variant| usize::from(variant.socket_index) == index)
    };
    let unused = donor
        .sockets
        .iter()
        .filter(|socket| {
            authored_socket_choice_limit(socket.socket_type) == 0 && is_inherited(socket.index)
        })
        .map(|socket| socket.index)
        .collect::<BTreeSet<_>>();
    let mut draw_row = |ui: &mut egui::Ui, socket_index: usize| {
        draw_socket_picker_row(
            ui,
            SocketRowContext {
                catalog,
                recipe_library,
                recipe,
                queries: &mut queries[socket_index],
                page: &mut pages[socket_index],
                plug_selection_mode: *plug_selection_mode,
                donor: effective_donor,
                socket_index,
                is_added: socket_index >= native_socket_count,
                can_remove_added: socket_index >= native_socket_count
                    && socket_index + 1 == effective_donor.sockets.len(),
                show_experimental_options,
                show_technical_row: &mut *show_technical_rows,
                private_perk_socket,
                log,
            },
        );
    };
    for socket in effective_donor
        .sockets
        .iter()
        .filter(|socket| !unused.contains(&socket.index))
    {
        draw_row(ui, socket.index);
    }
    if !unused.is_empty() {
        egui::CollapsingHeader::new(format!(
            "Unused Donor Sockets · Available: {}",
            unused.len()
        ))
        .id_salt("unused-weapon-sockets")
        .show(ui, |ui| {
            ui.label("Choose an unused slot's role to add another perk combination.");
            for &socket_index in &unused {
                draw_row(ui, socket_index);
            }
        });
    }
    draw_add_socket(ui, catalog, recipe, donor);
}

pub(super) fn draw_socket_override_diagnostics(
    ui: &mut egui::Ui,
    catalog: &InvestmentCatalog,
    recipe: &WeaponRecipe,
    donor: &WeaponDonor,
    show_plug_safety_warnings: bool,
) {
    if !recipe.overrides.socket_columns.is_empty() {
        let authored_choice_count = recipe
            .overrides
            .socket_columns
            .iter()
            .filter_map(Option::as_ref)
            .map(|column| column.choices.len())
            .sum::<usize>();
        if authored_choice_count > LIVE_SOCKET_DIAGNOSTIC_CHOICE_LIMIT {
            ui.weak(format!(
                "Live socket diagnostics paused for {authored_choice_count} authored choices; Build & stage still performs complete validation."
            ));
            return;
        }
        let parsed = recipe
            .overrides
            .socket_columns
            .iter()
            .enumerate()
            .map(|(socket_index, column)| {
                column
                    .as_ref()
                    .map(|column| {
                        column
                            .choices
                            .iter()
                            .map(HexHash::parse_u32)
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()
                    .map_err(|error| format!("Socket {socket_index}: {error}"))
            })
            .collect::<Result<Vec<_>, _>>();
        let socket_types = recipe
            .overrides
            .socket_columns
            .iter()
            .map(|column| column.as_ref().and_then(|column| column.socket_type))
            .collect::<Vec<_>>();
        let pending_socket_rows = socket_types
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(socket_index, socket_type)| {
                (socket_type == Some(u16::MAX)
                    && donor
                        .sockets
                        .get(socket_index)
                        .is_some_and(|socket| socket.socket_type == u16::MAX))
                .then_some(socket_index)
            })
            .collect::<BTreeSet<_>>();
        match (
            parsed,
            catalog.weapon_supported_plug_sets_with_socket_types(donor.summary.hash, &socket_types),
        ) {
            (Ok(parsed), Ok(sets)) => {
                let sets = sets
                    .into_iter()
                    .map(|set| SupportedPlugSet {
                        socket_index: set.socket_index,
                        plug_hashes: set.plug_hashes,
                    })
                    .collect::<Vec<_>>();
                let mut compatibility_warnings = Vec::new();
                for diagnostic in validate_socket_column_overrides_with_socket_types(
                    donor,
                    &parsed,
                    &socket_types,
                    &sets,
                ) {
                    if diagnostic.code == AuthoringDiagnosticCode::DisabledSocketOverride
                        && matches!(
                            diagnostic.field,
                            AuthoringField::SocketColumn { socket_index }
                                if pending_socket_rows.contains(&socket_index)
                        )
                    {
                        continue;
                    }
                    if !diagnostic.is_build_blocking() && !show_plug_safety_warnings {
                        continue;
                    }
                    if diagnostic.is_build_blocking() {
                        ui.colored_label(ui.visuals().error_fg_color, diagnostic.message);
                    } else {
                        compatibility_warnings.push(diagnostic.message);
                    }
                }
                if !compatibility_warnings.is_empty() {
                    let count = compatibility_warnings.len();
                    let noun = if count == 1 { "warning" } else { "warnings" };
                    egui::CollapsingHeader::new(
                        egui::RichText::new(format!(
                            "{count} plug compatibility {noun}. Test these choices in game."
                        ))
                        .color(ui.visuals().warn_fg_color),
                    )
                    .id_salt("socket-compatibility-warnings")
                    .show(ui, |ui| {
                        for warning in compatibility_warnings {
                            ui.colored_label(ui.visuals().warn_fg_color, warning);
                        }
                    });
                }
                for (socket_index, socket_type) in socket_types.iter().copied().enumerate() {
                    if socket_type == Some(u16::MAX) && pending_socket_rows.contains(&socket_index)
                    {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            format!(
                                "Socket {} is pending setup; choose an installed socket type and plug below",
                                socket_index + 1
                            ),
                        );
                        continue;
                    }
                    if let Some(socket_type) = socket_type
                        && socket_type != u16::MAX
                        && !catalog.weapon_socket_type_is_known(donor.summary.hash, socket_type)
                    {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            format!(
                                "Socket {socket_index} uses unknown native socket type {socket_type}"
                            ),
                        );
                    }
                }
            }
            (Err(error), _) | (_, Err(error)) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        }
    }
}

pub(super) fn set_socket_role(
    recipe: &mut WeaponRecipe,
    donor: &WeaponDonor,
    socket_index: usize,
    role: Option<u16>,
) {
    let socket = &donor.sockets[socket_index];
    if recipe
        .overrides
        .socket_columns
        .get(socket.index)
        .and_then(Option::as_ref)
        .is_none()
    {
        let inherited = inherited_socket_choices(
            socket.native_default,
            &socket.ordered_embedded_choices,
            authored_socket_choice_limit(role.unwrap_or(socket.socket_type)),
        );
        materialize_socket_column(recipe, donor.sockets.len(), socket.index, &inherited, false);
    }
    recipe.overrides.socket_columns[socket.index]
        .as_mut()
        .unwrap()
        .socket_type = role;
}

pub(super) fn socket_role_label(
    catalog: &InvestmentCatalog,
    donor: &WeaponDonor,
    socket_index: usize,
    role: Option<u16>,
) -> String {
    let socket = &donor.sockets[socket_index];
    role.map_or_else(
        || socket.label.clone(),
        |value| {
            let choices = if matches!(value, 176 | 92) {
                Vec::new()
            } else {
                catalog
                    .weapon_socket_type_choices(donor.summary.hash)
                    .unwrap_or_default()
            };
            let name = match value {
                176 => "Intrinsic",
                92 => "Trait",
                _ => choices
                    .iter()
                    .find(|choice| choice.socket_type == value)
                    .map_or("Custom socket", |choice| choice.label.as_str()),
            };
            format!("{}. {name}", socket_index + 1)
        },
    )
}

pub(super) fn draw_socket_role_label(
    ui: &mut egui::Ui,
    catalog: &InvestmentCatalog,
    donor: &WeaponDonor,
    socket_index: usize,
    is_added: bool,
    role: &mut Option<u16>,
    width: f32,
) {
    let label = socket_role_label(catalog, donor, socket_index, *role);
    let display_label = label
        .split_once(". ")
        .map_or(label.as_str(), |(_, role)| role);
    let display_label = if is_added {
        format!("{display_label} · Added")
    } else {
        display_label.to_owned()
    };
    ui.allocate_ui_with_layout(egui::vec2(width, ui.spacing().interact_size.y),
        egui::Layout::left_to_right(egui::Align::Center), |ui| {
        egui::ComboBox::from_id_salt(("socket-role", socket_index))
            .selected_text(display_label).width(width).truncate().show_ui(ui, |ui| {
                let choices = catalog.weapon_socket_type_choices(donor.summary.hash).unwrap_or_default();
                ui.weak("Socket role");
                if !is_added {
                    ui.selectable_value(role, None, "Keep Base Weapon Role");
                }
                for (value, label) in [(176, "Intrinsic"), (92, "Trait")] {
                    if choices.iter().any(|choice| choice.socket_type == value) {
                        ui.selectable_value(role, Some(value), label);
                    }
                }
                ui.separator();
                for choice in &choices {
                    if ![176, 92, u16::MAX].contains(&choice.socket_type) {
                        ui.selectable_value(role, Some(choice.socket_type), &choice.label);
                    }
                }
            }).response.on_hover_ui(|ui| {
                sundial::investment::tooltip_title(ui, label);
                ui.label("Change this socket's native role, for example Trait to Intrinsic. Existing custom perks retain their saved display type.");
            });
    });
}

pub(super) fn socket_choice_columns(available_width: f32, button_count: usize) -> usize {
    const TARGET_BUTTON_WIDTH: f32 = 168.0;
    const MAX_COLUMNS: usize = 3;

    let fitting = if available_width.is_finite() {
        (available_width / TARGET_BUTTON_WIDTH).floor().max(1.0) as usize
    } else {
        1
    };
    button_count.max(1).min(fitting).min(MAX_COLUMNS)
}

pub(super) fn draw_numeric_program_editor(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    program: &mut Vec<WeaponNumericInstructionRecipe>,
    allow_empty: bool,
) {
    let mut remove = None;
    egui::Grid::new(id)
        .num_columns(3)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            ui.weak("Opcode");
            ui.weak("Operand");
            ui.end_row();
            let program_len = program.len();
            for (index, instruction) in program.iter_mut().enumerate() {
                ui.add(egui::DragValue::new(&mut instruction.opcode).range(1..=27))
                    .on_hover_text(
                        "RPN opcode: 1 flag, 2 NOT, 3 OR, 4 AND, 5 NOR, 6/9 NE, 7 NAND, 8 EQ, 10 value, 11 constant, 12 shared pool, 13–21 comparison/arithmetic, 22 numeric coercion, 24–27 hash/bitwise.",
                    );
                ui.add(egui::DragValue::new(&mut instruction.operand));
                if ui
                    .add_enabled(
                        allow_empty || program_len > 1,
                        egui::Button::new("×").frame(false),
                    )
                    .on_hover_text("Remove instruction")
                    .clicked()
                {
                    remove = Some(index);
                }
                ui.end_row();
            }
        });
    if let Some(index) = remove {
        program.remove(index);
    }
    if ui.small_button("+ Add Instruction").clicked() {
        program.push(WeaponNumericInstructionRecipe {
            opcode: 11,
            operand: 1,
        });
    }
}

pub(super) fn draw_socket_technical_fields(ui: &mut egui::Ui, fields: SocketTechnicalFields<'_>) {
    let SocketTechnicalFields {
        catalog,
        recipe,
        donor,
        socket_index,
        is_added,
        inherited,
        page,
        queries,
        scroll_to_header,
    } = fields;
    let socket = &donor.sockets[socket_index];
    let plug_set_count = catalog.reusable_plug_set_count();
    let plug_set_max = plug_set_count
        .saturating_sub(1)
        .min(usize::from(u16::MAX - 1)) as u16;
    let configuring_disabled_socket = socket.socket_type == u16::MAX
        && recipe
            .overrides
            .socket_columns
            .get(socket_index)
            .and_then(Option::as_ref)
            .is_some();
    let mut removed_choice = None;
    let header_response = ui.indent(("socket_technical_indent", socket_index), |ui| {
        egui::CollapsingHeader::new(format!("{} native row", socket.label))
            .id_salt(("socket-native-row", socket_index))
            .default_open(configuring_disabled_socket)
            .show(ui, |ui| {
            let existing = recipe
                .overrides
                .socket_columns
                .get(socket_index)
                .and_then(Option::as_ref);
            if existing.is_none() {
                ui.horizontal_wrapped(|ui| {
                    ui.weak(format!(
                        "Inherited type {} · reusable {} · randomized {}",
                        socket.socket_type,
                        socket
                            .reusable_plug_set_index
                            .map_or_else(|| "disabled".to_owned(), |value| value.to_string()),
                        socket
                            .randomized_plug_set_index
                            .map_or_else(|| "disabled".to_owned(), |value| value.to_string())
                    ));
                    if socket.socket_type == u16::MAX {
                        ui.weak("Activate this socket from its main row to edit native fields.");
                    } else if ui
                        .button("Edit Native Fields")
                        .on_hover_text("Expose every native field for this socket row")
                        .clicked()
                    {
                        materialize_socket_column(
                            recipe,
                            donor.sockets.len(),
                            socket_index,
                            inherited,
                            false,
                        );
                    }
                });
                return;
            }

            let restore = !is_added && ui
                .button("Restore Donor Row")
                .on_hover_text(
                    "Remove native row overrides; randomized donor rows still use Parhelion's stable collection roll",
                )
                .clicked();
            if restore {
                recipe.overrides.socket_columns[socket_index] = None;
                if recipe.overrides.socket_columns.iter().all(Option::is_none) {
                    recipe.overrides.socket_columns.clear();
                }
                queries.clear();
                return;
            }
            let column = recipe.overrides.socket_columns[socket_index]
                .as_mut()
                .expect("checked above");
            let max_embedded_choices =
                authored_socket_choice_limit(column.socket_type.unwrap_or(socket.socket_type));
            let raw_choice_count = column.choices.len();
            let raw_page_count = raw_choice_count.max(1).div_ceil(SOCKET_CHOICE_PAGE_SIZE);
            *page = (*page).min(raw_page_count - 1);
            let raw_page_start = *page * SOCKET_CHOICE_PAGE_SIZE;
            let raw_page_end = (raw_page_start + SOCKET_CHOICE_PAGE_SIZE).min(raw_choice_count);

            ui.collapsing("Raw embedded plug hashes", |ui| {
                let mut remove = None;
                for (choice, hash) in column
                    .choices
                    .iter_mut()
                    .enumerate()
                    .skip(raw_page_start)
                    .take(SOCKET_CHOICE_PAGE_SIZE)
                {
                    ui.horizontal(|ui| {
                        ui.label(format!("Choice {}", choice + 1));
                        let mut text = hash.to_string();
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut text)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(118.0),
                            )
                            .changed()
                            && let Ok(parsed) = text.parse::<HexHash>()
                        {
                            *hash = parsed;
                        }
                        if raw_choice_count > 1
                            && ui.button("×").on_hover_text("Remove choice").clicked()
                        {
                            remove = Some(choice);
                        }
                    });
                }
                if let Some(choice) = remove {
                    column.choices.remove(choice);
                    if choice < column.choice_weight_bits.len() {
                        column.choice_weight_bits.remove(choice);
                    }
                    if choice < column.choice_conditions.len() {
                        column.choice_conditions.remove(choice);
                    }
                    shift_socket_choice_queries_after_removal(queries, choice);
                    removed_choice = Some(choice);
                }
                if column.choices.len() < max_embedded_choices
                    && raw_page_end == raw_choice_count
                    && ui.small_button("+ Add Embedded Choice").clicked()
                {
                    column.choices.push(HexHash::new(0));
                    if !column.choice_weight_bits.is_empty() {
                        column.choice_weight_bits.push(1.0_f32.to_bits());
                    }
                    if !column.choice_conditions.is_empty() {
                        column.choice_conditions.push(Vec::new());
                    }
                }
            });
            let choice_count = column.choices.len();
            let technical_page_count = choice_count.max(1).div_ceil(SOCKET_CHOICE_PAGE_SIZE);
            *page = (*page).min(technical_page_count - 1);
            let technical_page_start = *page * SOCKET_CHOICE_PAGE_SIZE;
            let technical_page_end =
                (technical_page_start + SOCKET_CHOICE_PAGE_SIZE).min(choice_count);

            ui.horizontal(|ui| {
                let mut override_type = column.socket_type.is_some();
                if ui.add_enabled(!is_added, egui::Checkbox::new(&mut override_type, "Socket Type")).changed() {
                    column.socket_type = override_type.then_some(socket.socket_type);
                }
                if let Some(value) = &mut column.socket_type {
                    ui.add(egui::DragValue::new(value).range(0..=u16::MAX));
                    let socket_type_choices = catalog
                        .weapon_socket_type_choices(donor.summary.hash)
                        .unwrap_or_default();
                    egui::ComboBox::from_id_salt(("socket-type-choice", socket_index))
                        .selected_text(
                            socket_type_choices
                                .iter()
                                .find(|choice| choice.socket_type == *value)
                                .map_or_else(
                                    || "Choose installed type…".to_owned(),
                                    |choice| {
                                        format!(
                                            "{} · {} plugs",
                                            choice.display_label(),
                                            choice.compatible_plug_count
                                        )
                                    },
                                ),
                        )
                        .show_ui(ui, |ui| {
                            for choice in socket_type_choices {
                                ui.selectable_value(
                                    value,
                                    choice.socket_type,
                                    format!(
                                        "{} · {} plugs",
                                        choice.display_label(),
                                        choice.compatible_plug_count
                                    ),
                                );
                            }
                        });
                    ui.weak(format!("donor {}", socket.socket_type));
                    if !catalog.weapon_socket_type_is_known(donor.summary.hash, *value) {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            "Select a socket type observed in the installed packages",
                        );
                    }
                }
            });

            let mut custom_weights = !column.choice_weight_bits.is_empty();
            if ui
                .checkbox(&mut custom_weights, "Embedded Choice Weights")
                .on_hover_text("Exact native float32 weights stored at plug-member +0x18")
                .changed()
            {
                if custom_weights {
                    column.choice_weight_bits = vec![1.0_f32.to_bits(); choice_count];
                } else {
                    column.choice_weight_bits.clear();
                }
            }
            if custom_weights {
                column
                    .choice_weight_bits
                    .resize(choice_count, 1.0_f32.to_bits());
                for (choice, bits) in column
                    .choice_weight_bits
                    .iter_mut()
                    .enumerate()
                    .skip(technical_page_start)
                    .take(SOCKET_CHOICE_PAGE_SIZE)
                {
                    ui.horizontal(|ui| {
                        ui.label(format!("Choice {}", choice + 1));
                        let mut decoded = f32::from_bits(*bits);
                        if decoded.is_finite()
                            && ui
                                .add(egui::DragValue::new(&mut decoded).speed(0.05))
                                .changed()
                        {
                            *bits = decoded.to_bits();
                        }
                        ui.add(egui::DragValue::new(bits).speed(1));
                        ui.monospace(format!("0x{bits:08X}"));
                    });
                }
            }

            let mut custom_conditions = !column.choice_conditions.is_empty();
            if ui
                .checkbox(&mut custom_conditions, "Per-Choice Conditions")
                .on_hover_text("Native RPN availability conditions aligned with embedded choices")
                .changed()
            {
                if custom_conditions {
                    column.choice_conditions = vec![Vec::new(); choice_count];
                } else {
                    column.choice_conditions.clear();
                }
            }
            if custom_conditions {
                column.choice_conditions.resize_with(choice_count, Vec::new);
                for choice in technical_page_start..technical_page_end {
                    let label = column
                        .choices
                        .get(choice)
                        .and_then(|hash| hash.parse_u32().ok())
                        .map_or_else(
                            || format!("Choice {} condition", choice + 1),
                            |hash| {
                                format!(
                                    "Choice {} condition · {}",
                                    choice + 1,
                                    catalog.plug_label(hash, false)
                                )
                            },
                        );
                    ui.collapsing(label, |ui| {
                        if column.choice_conditions[choice].is_empty() {
                            ui.weak("Unconditional");
                        }
                        draw_numeric_program_editor(
                            ui,
                            ("socket_choice_condition", socket_index, choice),
                            &mut column.choice_conditions[choice],
                            true,
                        );
                    });
                }
            }

            ui.horizontal(|ui| {
                let mut enabled = column.reusable_plug_set_index.is_some();
                if ui.checkbox(&mut enabled, "Reusable Plug Set").changed() {
                    column.reusable_plug_set_index =
                        enabled.then_some(socket.reusable_plug_set_index.unwrap_or(0));
                }
                if let Some(index) = &mut column.reusable_plug_set_index {
                    ui.add(egui::DragValue::new(index).range(0..=plug_set_max));
                    ui.weak(format!("{plug_set_count} installed rows"));
                }
            });

            ui.horizontal(|ui| {
                let mut enabled = column.randomized_plug_set_index.is_some();
                if ui.checkbox(&mut enabled, "Randomized Plug Set").changed() {
                    if enabled {
                        column.randomized_plug_set_index =
                            Some(socket.randomized_plug_set_index.unwrap_or(0));
                        if column.randomized_selection_program.is_empty() {
                            column.randomized_selection_program.push(
                                WeaponNumericInstructionRecipe {
                                    opcode: 11,
                                    operand: 1,
                                },
                            );
                        }
                    } else {
                        column.randomized_plug_set_index = None;
                        column.randomized_selection_program.clear();
                    }
                }
                if let Some(index) = &mut column.randomized_plug_set_index {
                    ui.add(egui::DragValue::new(index).range(0..=plug_set_max));
                    ui.weak(format!("{plug_set_count} installed rows"));
                }
            });
            if column.randomized_plug_set_index.is_some() {
                ui.label("Randomized selection-count program");
                draw_numeric_program_editor(
                    ui,
                    ("socket_random_program", socket_index),
                    &mut column.randomized_selection_program,
                    false,
                );
            }
            })
            .header_response
    }).inner;
    if scroll_to_header {
        header_response.scroll_to_me(Some(egui::Align::Center));
    }
    if let Ok(choices) = recipe_socket_choices(recipe, socket_index, inherited) {
        reconcile_socket_plug_variants(recipe, socket_index, &choices, removed_choice);
    }
}

pub(super) fn recipe_socket_choices(
    recipe: &WeaponRecipe,
    socket_index: usize,
    inherited: &[u32],
) -> Result<Vec<u32>, String> {
    recipe
        .overrides
        .socket_columns
        .get(socket_index)
        .and_then(Option::as_ref)
        .map_or_else(
            || Ok(inherited.to_vec()),
            |column| {
                column
                    .choices
                    .iter()
                    .map(HexHash::parse_u32)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())
            },
        )
}

pub(super) fn materialize_socket_column(
    recipe: &mut WeaponRecipe,
    socket_count: usize,
    socket_index: usize,
    inherited: &[u32],
    activate_disabled: bool,
) {
    let socket_count = socket_count
        .max(recipe.overrides.socket_columns.len())
        .max(socket_index + 1);
    recipe
        .overrides
        .socket_columns
        .resize_with(socket_count, || None);
    recipe.overrides.socket_columns[socket_index] = Some(WeaponSocketColumnRecipe {
        choices: if inherited.is_empty() {
            vec![HexHash::new(0)]
        } else {
            inherited.iter().copied().map(HexHash::new).collect()
        },
        socket_type: activate_disabled.then_some(u16::MAX),
        ..WeaponSocketColumnRecipe::default()
    });
}

pub(super) fn shift_socket_choice_queries_after_removal(
    queries: &mut BTreeMap<usize, String>,
    removed_index: usize,
) {
    let later = queries.split_off(&removed_index.saturating_add(1));
    queries.remove(&removed_index);
    for (index, query) in later {
        queries.insert(index - 1, query);
    }
}

pub(super) fn set_recipe_socket_column(
    recipe: &mut WeaponRecipe,
    socket_count: usize,
    socket_index: usize,
    inherited: &[u32],
    choices: Vec<u32>,
    removed_choice: Option<usize>,
) {
    let socket_count = socket_count
        .max(recipe.overrides.socket_columns.len())
        .max(socket_index + 1);
    recipe
        .overrides
        .socket_columns
        .resize_with(socket_count, || None);
    let mut column = recipe.overrides.socket_columns[socket_index]
        .clone()
        .unwrap_or_default();
    column.choices = choices.iter().copied().map(HexHash::new).collect();
    if !column.choice_weight_bits.is_empty() {
        if let Some(index) = removed_choice.filter(|index| *index < column.choice_weight_bits.len())
        {
            column.choice_weight_bits.remove(index);
        }
        column
            .choice_weight_bits
            .resize(choices.len(), 1.0_f32.to_bits());
    }
    if !column.choice_conditions.is_empty() {
        if let Some(index) = removed_choice.filter(|index| *index < column.choice_conditions.len())
        {
            column.choice_conditions.remove(index);
        }
        column
            .choice_conditions
            .resize_with(choices.len(), Vec::new);
    }
    let has_technical_overrides = column.socket_type.is_some()
        || !column.choice_weight_bits.is_empty()
        || !column.choice_conditions.is_empty()
        || column.reusable_plug_set_index.is_some()
        || column.randomized_plug_set_index.is_some()
        || !column.randomized_selection_program.is_empty();
    recipe.overrides.socket_columns[socket_index] =
        (choices != inherited || has_technical_overrides).then_some(column);
    if recipe.overrides.socket_columns.iter().all(Option::is_none) {
        recipe.overrides.socket_columns.clear();
    }
    reconcile_socket_plug_variants(recipe, socket_index, &choices, removed_choice);
}

pub(super) fn make_choice_default(
    recipe: &mut WeaponRecipe,
    socket_count: usize,
    socket_index: usize,
    inherited: &[u32],
    choice_index: usize,
) -> Result<(), String> {
    let mut choices = recipe_socket_choices(recipe, socket_index, inherited)?;
    if choice_index >= choices.len() {
        return Err("The selected choice no longer exists. Reopen the socket picker.".into());
    }
    if choice_index == 0 {
        return Ok(());
    }
    let mut column = recipe
        .overrides
        .socket_columns
        .get(socket_index)
        .and_then(Option::as_ref)
        .cloned()
        .unwrap_or_default();
    if (!column.choice_weight_bits.is_empty() && column.choice_weight_bits.len() != choices.len())
        || (!column.choice_conditions.is_empty() && column.choice_conditions.len() != choices.len())
    {
        return Err("The socket's weights or conditions do not match its choices. Correct them before changing the default.".into());
    }
    let selected = choices.remove(choice_index);
    choices.insert(0, selected);
    if !column.choice_weight_bits.is_empty() {
        let selected = column.choice_weight_bits.remove(choice_index);
        column.choice_weight_bits.insert(0, selected);
    }
    if !column.choice_conditions.is_empty() {
        let selected = column.choice_conditions.remove(choice_index);
        column.choice_conditions.insert(0, selected);
    }
    recipe.overrides.socket_columns.resize_with(
        socket_count
            .max(recipe.overrides.socket_columns.len())
            .max(socket_index + 1),
        || None,
    );
    recipe.overrides.socket_columns[socket_index] = Some(column);
    for variant in &mut recipe.overrides.socket_plug_variants {
        if usize::from(variant.socket_index) == socket_index {
            let index = usize::from(variant.choice_index);
            if index == choice_index {
                variant.choice_index = 0;
            } else if index < choice_index {
                variant.choice_index += 1;
            }
        }
    }
    set_recipe_socket_column(recipe, socket_count, socket_index, inherited, choices, None);
    Ok(())
}

pub(super) fn inherited_socket_choices(
    native_default: Option<u32>,
    ordered_embedded_choices: &[u32],
    max_authored_choices: usize,
) -> Vec<u32> {
    if max_authored_choices == 0 {
        return Vec::new();
    }
    let limit = max_authored_choices;
    let mut choices = Vec::with_capacity(
        ordered_embedded_choices
            .len()
            .saturating_add(usize::from(native_default.is_some()))
            .min(limit),
    );
    if let Some(native_default) = native_default {
        choices.push(native_default);
    }
    for choice in ordered_embedded_choices.iter().copied() {
        if choices.len() >= limit {
            break;
        }
        if !choices.contains(&choice) {
            choices.push(choice);
        }
    }
    choices
}
