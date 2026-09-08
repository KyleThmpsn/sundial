//! Random item builder window and controls.

use crate::app::account_workspace as account;

use super::*;

pub(super) fn draw_item_workspace(
    app: &mut SundialApp,
    context: &egui::Context,
    character_index: usize,
    open_requested: bool,
) {
    let dialog_id = egui::Id::new(("equipment-randomize", character_index));
    let state_id = dialog_id.with("state");
    let window_generation_id = dialog_id.with("window-generation");
    let builder_request = context.data_mut(|data| {
        data.remove_temp::<ItemBuilderRequest>(item_builder_request_id(character_index))
    });
    if open_requested || builder_request.is_some() {
        context.data_mut(|data| {
            data.insert_temp(dialog_id, true);
            let generation = data
                .get_temp::<u64>(window_generation_id)
                .unwrap_or_default()
                .wrapping_add(1);
            data.insert_temp(window_generation_id, generation);
        });
    }
    let mut open = context
        .data_mut(|data| data.get_temp::<bool>(dialog_id))
        .unwrap_or(false);
    if !open {
        return;
    }
    let window_generation = context
        .data_mut(|data| data.get_temp::<u64>(window_generation_id))
        .unwrap_or_default();
    let window_size = crate::app::equipment::dialog_size_constraints(
        context,
        egui::vec2(980.0, 720.0),
        WINDOW_MIN_SIZE,
    );

    let normal_open_requested = open_requested && builder_request.is_none();
    let mut state = if normal_open_requested {
        WorkspaceState::default()
    } else {
        context
            .data_mut(|data| data.get_temp::<WorkspaceState>(state_id))
            .unwrap_or_default()
    };
    if let Some(request) = builder_request {
        match open_builder_request(&app.manifest, &mut state, &request) {
            Ok(()) => {}
            Err(error) => {
                state.feedback = Some(Feedback {
                    text: error,
                    is_error: true,
                });
            }
        }
    } else if normal_open_requested
        && let Err(error) = roll_family(
            &app.manifest,
            character_class(&app.document, character_index),
            app.show_dummy_items,
            app.plug_selection_mode,
            &mut state,
            ItemFamily::Weapon,
        )
    {
        state.feedback = Some(Feedback {
            text: error,
            is_error: true,
        });
    }
    let mut apply_requested = None;

    egui::Window::new("Random Item Builder")
        .id(dialog_id.with(("window", window_generation, window_size.compact)))
        .collapsible(false)
        .resizable(true)
        .default_size(window_size.default)
        .min_size(window_size.min)
        .max_size(window_size.max)
        .open(&mut open)
        .show(context, |ui| {
            let scroll_height = (ui.available_height() - FOOTER_RESERVE_HEIGHT).max(1.0);
            egui::ScrollArea::vertical()
                .id_salt(dialog_id.with(("scroll", window_generation, window_size.compact)))
                .max_height(scroll_height)
                .min_scrolled_height(scroll_height)
                .auto_shrink([false, false])
                .show_viewport(ui, |ui, viewport| {
                    let content_top = ui.cursor().top();
                    let plug_mode = app.plug_selection_mode;
                    draw_base_section(
                        ui,
                        RandomizerSource {
                            document: &app.document,
                            catalog: &app.manifest,
                            character_index,
                        },
                        app.show_dummy_items,
                        plug_mode,
                        &mut state,
                    );
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                    draw_plug_heading(ui, &app.manifest, plug_mode, &mut state);
                    app.draw_plug_safety_controls(ui);
                    let content_used = ui.cursor().top() - content_top;
                    let plug_list_height =
                        (viewport.height() - content_used - PLUG_SECTION_CHROME_HEIGHT)
                            .max(MIN_PLUG_LIST_HEIGHT);
                    draw_plug_section_contents(
                        ui,
                        &app.manifest,
                        app.plug_selection_mode,
                        &mut state,
                        plug_list_height,
                    );
                });
            ui.add_space(6.0);
            let inventory_blocker = inventory_add_blocker(
                &app.document,
                &app.manifest,
                character_index,
                state.candidate.as_ref(),
            );
            let equip_warning = equip_replacement_warning(
                &app.document,
                &app.manifest,
                character_index,
                state.candidate.as_ref(),
            );
            draw_footer(
                ui,
                &mut state,
                inventory_blocker.as_deref(),
                equip_warning.as_ref(),
                &mut apply_requested,
            );
        });

    if let Some(candidate) = state.pending_destructive_equip.clone() {
        if let Some(warning) = equip_replacement_warning(
            &app.document,
            &app.manifest,
            character_index,
            Some(&candidate),
        ) {
            let mut replace = false;
            let mut cancel = false;
            let response = egui::Modal::new(dialog_id.with("replace-equipped-confirmation")).show(
                context,
                |ui| {
                    ui.set_width(440.0);
                    ui.heading("Replace equipped item?");
                    ui.add_space(6.0);
                    ui.colored_label(ui.visuals().warn_fg_color, warning.message());
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Delete old item and equip").clicked() {
                            replace = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                },
            );
            cancel |= response.should_close();
            if replace {
                state.pending_destructive_equip = None;
                apply_requested = Some(CandidateAction::Equip {
                    candidate,
                    discard_replaced: true,
                });
            } else if cancel {
                state.pending_destructive_equip = None;
            }
        } else {
            state.pending_destructive_equip = None;
            apply_requested = Some(CandidateAction::Equip {
                candidate,
                discard_replaced: false,
            });
        }
    }

    if let Some(action) = apply_requested {
        let (result, failure_prefix, unchanged) = match action {
            CandidateAction::Equip {
                candidate,
                discard_replaced,
            } => (
                apply_candidate(
                    &mut app.document,
                    &app.manifest,
                    character_index,
                    &candidate,
                    discard_replaced,
                ),
                "Randomized item not equipped",
                None,
            ),
            CandidateAction::AddToInventory(candidate) => (
                add_candidate_to_inventory(
                    &mut app.document,
                    &app.manifest,
                    character_index,
                    &candidate,
                ),
                "Randomized item not added",
                Some("Equipped gear was not changed."),
            ),
        };
        match result {
            Ok(message) => {
                app.dirty = true;
                app.set_status(format!("{message}. Click Save to write it"), false);
                state.feedback = Some(Feedback {
                    text: unchanged.map_or_else(
                        || message.clone(),
                        |unchanged| format!("{message}. {unchanged}"),
                    ),
                    is_error: false,
                });
            }
            Err(error) => {
                app.set_status(format!("{failure_prefix}: {error}"), true);
                state.feedback = Some(Feedback {
                    text: error,
                    is_error: true,
                });
            }
        }
    }
    context.data_mut(|data| {
        data.insert_temp(dialog_id, open);
        data.insert_temp(state_id, state);
    });
}

#[derive(Clone, Copy)]
struct RandomizerSource<'a> {
    document: &'a account::WorkspaceDocument,
    catalog: &'a Catalog,
    character_index: usize,
}

fn draw_base_section(
    ui: &mut egui::Ui,
    source: RandomizerSource<'_>,
    show_dummy_items: bool,
    plug_mode: PlugSelectionMode,
    state: &mut WorkspaceState,
) {
    let RandomizerSource {
        document,
        catalog,
        character_index,
    } = source;
    let class_type = character_class(document, character_index);
    ui.vertical(|ui| {
        ui.set_width(ui.available_width());

        let mut action = None;
        let previous_family = state.active_family;
        if state
            .last_plug_mode
            .replace(plug_mode)
            .is_some_and(|previous| previous != plug_mode)
        {
            state.feedback = None;
            state.armor_stat_allocation.clear_feedback();
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut state.active_family, ItemFamily::Weapon, "Weapon");
            ui.selectable_value(&mut state.active_family, ItemFamily::Armor, "Armor");
            if state.active_family != previous_family {
                state.base_query.clear();
                state.socket_query.clear();
                state.selected_socket = 0;
                state.candidate = None;
                state.feedback = None;
                state.armor_stat_allocation.clear_feedback();
                action = Some(BaseAction::Random(state.active_family));
            }
            let status = state
                .feedback
                .as_ref()
                .map(|feedback| (feedback.text.clone(), feedback.is_error))
                .or_else(|| {
                    state
                        .armor_stat_allocation
                        .feedback()
                        .map(|(text, is_error)| (text.to_owned(), is_error))
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some((text, is_error)) = &status {
                    let color = if *is_error {
                        ui.visuals().error_fg_color
                    } else {
                        ui.visuals().strong_text_color()
                    };
                    ui.add(egui::Label::new(egui::RichText::new(text).color(color)).truncate())
                        .on_hover_text(text);
                }
            });
        });
        ui.separator();

        ui.add_space(4.0);
        let family = state.active_family;
        let has_allocation =
            family == ItemFamily::Armor && armor_allocation_available(catalog, state);
        let inline_width = BASE_FILTER_INLINE_WIDTH
            + ui.spacing().item_spacing.x
            + armor_stat_allocation::INLINE_CONTENT_WIDTH;
        let can_place_allocation_inline = has_allocation && ui.available_width() >= inline_width;
        if can_place_allocation_inline {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(BASE_FILTER_INLINE_WIDTH, 45.0),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        draw_base_filters(
                            ui,
                            catalog,
                            class_type,
                            show_dummy_items,
                            family,
                            state,
                            &mut action,
                        );
                    },
                );
                draw_armor_allocation(ui, catalog, plug_mode, state);
            });
        } else {
            draw_base_filters(
                ui,
                catalog,
                class_type,
                show_dummy_items,
                family,
                state,
                &mut action,
            );
            if has_allocation {
                ui.add_space(4.0);
                draw_armor_allocation(ui, catalog, plug_mode, state);
            }
        }

        ui.add_space(6.0);
        let search_hint = match family {
            ItemFamily::Weapon => "Search for an exact weapon…",
            ItemFamily::Armor => "Search for exact armor…",
        };
        let search = ui.add(
            egui::TextEdit::singleline(&mut state.base_query)
                .hint_text(search_hint)
                .desired_width(f32::INFINITY),
        );
        let search_popup_id = ui.make_persistent_id("randomize-base-results-popup");
        if state.base_query.trim().is_empty() {
            if ui.memory(|memory| memory.is_popup_open(search_popup_id)) {
                ui.memory_mut(|memory| memory.close_popup());
            }
        } else if search.changed() || search.gained_focus() {
            ui.memory_mut(|memory| memory.open_popup(search_popup_id));
        }
        if !state.base_query.trim().is_empty() {
            let definition_results = matching_items(
                catalog,
                class_type,
                show_dummy_items,
                state.base_query.trim(),
                family,
                state.filter(family),
            );
            let matching_hashes = definition_results
                .iter()
                .map(|item| item.hash)
                .collect::<HashSet<_>>();
            let instance_results =
                matching_item_instances(document, character_index, &matching_hashes);
            egui::popup::popup_below_widget(
                ui,
                search_popup_id,
                &search,
                egui::PopupCloseBehavior::CloseOnClickOutside,
                |ui| {
                    ui.set_width(search.rect.width());
                    if instance_results.is_empty() && definition_results.is_empty() {
                        ui.label(
                            egui::RichText::new("No items match the search and filters.").weak(),
                        );
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("randomize-base-results")
                            .max_height(230.0)
                            .show(ui, |ui| {
                                if !instance_results.is_empty() {
                                    for choice in instance_results
                                        .into_iter()
                                        .take(MAX_VISIBLE_SEARCH_RESULTS)
                                    {
                                        let Some(item) = catalog.item(choice.request.item_hash)
                                        else {
                                            continue;
                                        };
                                        let secondary =
                                            format!("{} · {}", item.type_name, choice.location);
                                        let response = item_editor::draw_catalog_picker_row(
                                            ui,
                                            catalog,
                                            item_editor::CatalogPickerRow {
                                                hash: item.hash,
                                                primary: &item.name,
                                                primary_max_rows: 1,
                                                secondary: Some(&secondary),
                                                icon_size: 34.0,
                                                row_height: 46.0,
                                                selected: false,
                                            },
                                        );
                                        let response = item_editor::catalog_item_tooltip(
                                            response, catalog, item.hash,
                                        );
                                        if response.clicked() {
                                            action =
                                                Some(BaseAction::SelectInstance(choice.request));
                                            ui.memory_mut(|memory| memory.close_popup());
                                        }
                                    }
                                    if !definition_results.is_empty() {
                                        ui.separator();
                                    }
                                }
                                for item in definition_results
                                    .into_iter()
                                    .take(MAX_VISIBLE_SEARCH_RESULTS)
                                {
                                    let slot_label = slot_for_bucket(item.bucket_hash)
                                        .map_or("Unknown slot", |(_, label)| label);
                                    let secondary = format!(
                                        "{} · {slot_label} · {}",
                                        item.type_name,
                                        format_hash_hex(item.hash)
                                    );
                                    let response = item_editor::draw_catalog_picker_row(
                                        ui,
                                        catalog,
                                        item_editor::CatalogPickerRow {
                                            hash: item.hash,
                                            primary: &item.name,
                                            primary_max_rows: 1,
                                            secondary: Some(&secondary),
                                            icon_size: 34.0,
                                            row_height: 46.0,
                                            selected: state
                                                .candidate
                                                .as_ref()
                                                .map(|roll| roll.item_hash)
                                                == Some(item.hash),
                                        },
                                    );
                                    let response = item_editor::catalog_item_tooltip(
                                        response, catalog, item.hash,
                                    );
                                    if response.clicked() {
                                        action = Some(BaseAction::SelectDefinition(item.hash));
                                        ui.memory_mut(|memory| memory.close_popup());
                                    }
                                }
                            });
                    }
                },
            );
        }

        if let Some(action) = action {
            let result = match action {
                BaseAction::Random(family) => roll_family(
                    catalog,
                    class_type,
                    show_dummy_items,
                    plug_mode,
                    state,
                    family,
                ),
                BaseAction::SelectDefinition(hash) => select_base(catalog, plug_mode, state, hash),
                BaseAction::SelectInstance(request) => {
                    open_builder_request(catalog, state, &request)
                }
            };
            if let Err(error) = result {
                state.feedback = Some(Feedback {
                    text: error,
                    is_error: true,
                });
            }
        }

        if let Some(candidate) = state.candidate.as_ref()
            && let Some(item) = catalog.item(candidate.item_hash)
        {
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
            draw_candidate_header(ui, catalog, item);
        }
    });
}

fn draw_base_filters(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    class_type: u64,
    show_dummy_items: bool,
    family: ItemFamily,
    state: &mut WorkspaceState,
    action: &mut Option<BaseAction>,
) {
    ui.horizontal_wrapped(|ui| {
        draw_family_roll(ui, state, family, class_type, action);
        if family == ItemFamily::Armor {
            ui.separator();
            ui.label("Class");
            ui.label(egui::RichText::new(class_name(class_type)).strong());
        }
    });

    ui.add_space(6.0);
    let filter_candidates = random_item_candidates(
        catalog,
        class_type,
        show_dummy_items,
        family.slots().iter().copied(),
    );
    let filter_scope = match family {
        ItemFamily::Weapon => item_editor::ItemFilterScope::Weapon,
        ItemFamily::Armor => item_editor::ItemFilterScope::Armor,
    };
    item_editor::draw_item_filter_bar(
        ui,
        ("random-item", family),
        filter_scope,
        &filter_candidates,
        state.filter_mut(family),
    );
}

fn armor_allocation_available(catalog: &Catalog, state: &WorkspaceState) -> bool {
    state.candidate.as_ref().is_some_and(|candidate| {
        catalog.item(candidate.item_hash).is_some_and(|item| {
            armor_stat_allocation::is_available(catalog, item, candidate.plugs.len())
        })
    })
}

fn draw_armor_allocation(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    plug_mode: PlugSelectionMode,
    state: &mut WorkspaceState,
) {
    let Some((item_hash, mut plugs)) = state
        .candidate
        .as_ref()
        .map(|candidate| (candidate.item_hash, candidate.plugs.clone()))
    else {
        return;
    };
    let Some(item) = catalog.item(item_hash) else {
        return;
    };
    let changed = armor_stat_allocation::draw(
        ui,
        catalog,
        item,
        &mut plugs,
        plug_mode,
        &mut state.armor_stat_allocation,
    );
    if changed && let Some(candidate) = state.candidate.as_mut() {
        candidate.plugs = plugs;
        state.feedback = None;
    }
}

fn draw_family_roll(
    ui: &mut egui::Ui,
    state: &mut WorkspaceState,
    family: ItemFamily,
    class_type: u64,
    action: &mut Option<BaseAction>,
) {
    let selection = match family {
        ItemFamily::Weapon => &mut state.weapon_slot,
        ItemFamily::Armor => &mut state.armor_slot,
    };
    let (selection_label, any_label) = match family {
        ItemFamily::Weapon => ("Weapon slot", "Any weapon slot"),
        ItemFamily::Armor => ("Armor type", "Any armor type"),
    };
    let selected_text = selection
        .and_then(|index| family.slots().get(index).copied())
        .and_then(slot_definition)
        .map_or_else(|| any_label.to_owned(), |(_, label, _)| label.to_owned());
    ui.label(selection_label);
    egui::ComboBox::from_id_salt(("randomize-slot", family))
        .selected_text(selected_text)
        .width(130.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(selection, None, any_label);
            for (index, &slot) in family.slots().iter().enumerate() {
                if let Some((_, label, _)) = slot_definition(slot) {
                    ui.selectable_value(selection, Some(index), label);
                }
            }
        });
    if ui
        .add_enabled(
            class_type <= 2,
            egui::Button::new(format!("Roll {}", family.label())),
        )
        .clicked()
    {
        *action = Some(BaseAction::Random(family));
    }
}

fn draw_candidate_header(ui: &mut egui::Ui, catalog: &Catalog, item: &ItemDef) {
    ui.horizontal(|ui| {
        if let Some(icon) = catalog.icon_texture(ui.ctx(), item.hash) {
            ui.add(egui::Image::new((icon.id(), egui::vec2(52.0, 52.0))).corner_radius(3));
        }
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(&item.name).strong().size(16.0));
            let slot_label = slot_for_bucket(item.bucket_hash).map_or("Unknown slot", |(_, l)| l);
            ui.label(format!("{} · {slot_label}", item.type_name));
        });
    });
}

fn draw_plug_heading(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    plug_mode: PlugSelectionMode,
    state: &mut WorkspaceState,
) {
    let item = state
        .candidate
        .as_ref()
        .and_then(|candidate| catalog.item(candidate.item_hash));
    ui.horizontal(|ui| {
        ui.heading("Plugs");
        let reroll = ui.add_enabled(item.is_some(), egui::Button::new("Reroll plugs").small());
        if reroll.clicked()
            && let Some(item) = item
        {
            match rolled_candidate(catalog, item, plug_mode) {
                Ok(candidate) => {
                    state.candidate = Some(candidate);
                    state.feedback = None;
                    state.armor_stat_allocation.clear_feedback();
                }
                Err(error) => {
                    state.feedback = Some(Feedback {
                        text: error,
                        is_error: true,
                    });
                }
            }
        }
    });
}

fn draw_plug_section_contents(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    plug_mode: PlugSelectionMode,
    state: &mut WorkspaceState,
    plug_list_height: f32,
) {
    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        let Some(item_hash) = state
            .candidate
            .as_ref()
            .map(|candidate| candidate.item_hash)
        else {
            ui.label(
                egui::RichText::new("Choose a base item to generate and edit its plugs.").weak(),
            );
            return;
        };
        let Some(item) = catalog.item(item_hash) else {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "The selected base item is unavailable.",
            );
            return;
        };
        let socket_count = item.sockets.len().min(inventory::MAX_ITEM_PLUGS);
        if socket_count == 0 {
            ui.label(egui::RichText::new("This item has no configurable sockets.").weak());
            return;
        }
        state.selected_socket = state.selected_socket.min(socket_count - 1);

        ui.columns(2, |columns| {
            let (left, right) = columns.split_at_mut(1);
            draw_selected_plugs(&mut left[0], catalog, item, state, plug_list_height);
            draw_available_plugs(
                &mut right[0],
                catalog,
                item,
                plug_mode,
                state,
                plug_list_height,
            );
        });
    });
}

fn draw_available_plugs(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &ItemDef,
    plug_mode: PlugSelectionMode,
    state: &mut WorkspaceState,
    plug_list_height: f32,
) {
    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        let socket_index = state.selected_socket;
        let socket_label = item.sockets[socket_index].display_label(socket_index);
        let (choices, show_types) =
            item_editor::plug_choices_for_socket(catalog, item, socket_index, plug_mode);
        let current_hash = state
            .candidate
            .as_ref()
            .and_then(|candidate| candidate.plugs.get(socket_index))
            .copied()
            .flatten();
        let default_hash = default_plug(item, socket_index).ok().flatten();
        let mut selection = None::<Option<u64>>;

        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Available plugs").strong().size(15.0));
            ui.label(format!("· {socket_label}"));
            if ui
                .add_enabled(!choices.is_empty(), egui::Button::new("Reroll").small())
                .clicked()
            {
                let hashes = choices.iter().map(|choice| choice.hash).collect::<Vec<_>>();
                selection = Rng::from_clock().pick_valid_hash(&hashes).map(Some);
            }
            if ui
                .add_enabled(
                    current_hash != default_hash,
                    egui::Button::new("Reset").small(),
                )
                .clicked()
            {
                selection = Some(default_hash);
            }
        });

        let searchable = choices.len() > 12;
        if searchable {
            ui.add(
                egui::TextEdit::singleline(&mut state.socket_query)
                    .hint_text("Search plugs…")
                    .desired_width(f32::INFINITY),
            );
        } else {
            state.socket_query.clear();
        }
        let query = state.socket_query.trim().to_ascii_lowercase();
        egui::ScrollArea::vertical()
            .id_salt(("randomize-plugs", socket_index))
            .max_height(plug_list_height)
            .min_scrolled_height(plug_list_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let mut visible = 0;
                for choice in choices.iter().filter(|choice| {
                    query.is_empty()
                        || choice.label.to_ascii_lowercase().contains(&query)
                        || choice.type_name.to_ascii_lowercase().contains(&query)
                        || catalog.description(choice.hash).is_some_and(|description| {
                            description.to_ascii_lowercase().contains(&query)
                        })
                }) {
                    visible += 1;
                    let description = catalog
                        .description(choice.hash)
                        .map(compact_text)
                        .unwrap_or_default();
                    let secondary = match (
                        show_types.then_some(choice.type_name.as_str()),
                        description.as_str(),
                    ) {
                        (Some(type_name), description)
                            if !type_name.is_empty() && !description.is_empty() =>
                        {
                            format!("{type_name} · {description}")
                        }
                        (Some(type_name), _) if !type_name.is_empty() => type_name.to_owned(),
                        (_, description) if !description.is_empty() => description.to_owned(),
                        _ => String::new(),
                    };
                    let response = item_editor::draw_catalog_picker_row(
                        ui,
                        catalog,
                        item_editor::CatalogPickerRow {
                            hash: choice.hash,
                            primary: &choice.label,
                            primary_max_rows: 1,
                            secondary: (!secondary.is_empty()).then_some(secondary.as_str()),
                            icon_size: PLUG_ICON_SIZE,
                            row_height: AVAILABLE_PLUG_ROW_HEIGHT,
                            selected: current_hash == Some(choice.hash),
                        },
                    );
                    let response =
                        item_editor::catalog_item_tooltip(response, catalog, choice.hash);
                    if response.clicked() {
                        selection = Some(Some(choice.hash));
                    }
                }
                if visible == 0 {
                    ui.label(egui::RichText::new("No matching plugs.").weak());
                }
            });

        if let Some(hash) = selection
            && let Some(candidate) = state.candidate.as_mut()
        {
            candidate.plugs[socket_index] = hash;
            state.feedback = None;
            state.armor_stat_allocation.clear_feedback();
        }
    });
}

fn draw_selected_plugs(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &ItemDef,
    state: &mut WorkspaceState,
    plug_list_height: f32,
) {
    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new("Selected plugs").strong().size(15.0));
        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt("randomize-selected-plugs")
            .max_height(plug_list_height)
            .min_scrolled_height(plug_list_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (socket_index, socket) in item
                    .sockets
                    .iter()
                    .take(inventory::MAX_ITEM_PLUGS)
                    .enumerate()
                {
                    let hash = state
                        .candidate
                        .as_ref()
                        .and_then(|candidate| candidate.plugs.get(socket_index))
                        .copied()
                        .flatten();
                    let plug_label = hash
                        .map_or_else(|| "None".to_owned(), |hash| catalog.plug_label(hash, false));
                    let socket_label = socket.display_label(socket_index);
                    let hover_text = format!("{socket_label}: {plug_label}");
                    let text = selected_plug_text(ui, &socket_label, &plug_label);
                    let icon = plug_icon_or_blank(ui, catalog, hash);
                    let button = egui::Button::image_and_text(
                        (icon.id(), egui::vec2(PLUG_ICON_SIZE, PLUG_ICON_SIZE)),
                        text,
                    )
                    .truncate()
                    .selected(state.selected_socket == socket_index);
                    let response = ui.add_sized([ui.available_width(), PLUG_ROW_HEIGHT], button);
                    let response = match hash {
                        Some(hash) => {
                            item_editor::catalog_item_tooltip_immediate(response, catalog, hash)
                        }
                        None => response.on_hover_text(hover_text),
                    };
                    if response.clicked() {
                        selected = Some(socket_index);
                    }
                }
            });
        if let Some(socket_index) = selected {
            state.selected_socket = socket_index;
            state.socket_query.clear();
        }
    });
}

fn selected_plug_text(
    ui: &egui::Ui,
    socket_label: &str,
    plug_label: &str,
) -> egui::text::LayoutJob {
    let font_id = egui::TextStyle::Button.resolve(ui.style());
    let mut text = egui::text::LayoutJob::default();
    text.append(
        socket_label,
        0.0,
        egui::TextFormat {
            font_id: font_id.clone(),
            color: ui.visuals().strong_text_color(),
            ..Default::default()
        },
    );
    text.append(
        &format!(": {plug_label}"),
        0.0,
        egui::TextFormat {
            font_id,
            color: egui::Color32::PLACEHOLDER,
            ..Default::default()
        },
    );
    text
}

fn compact_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn plug_icon_or_blank(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: Option<u64>,
) -> egui::TextureHandle {
    if let Some(icon) = hash.and_then(|hash| catalog.icon_texture(ui.ctx(), hash)) {
        return icon;
    }

    let texture_id = egui::Id::new("randomize-blank-plug-icon");
    if let Some(texture) = ui
        .ctx()
        .data_mut(|data| data.get_temp::<egui::TextureHandle>(texture_id))
    {
        return texture;
    }

    let texture = ui.ctx().load_texture(
        "randomize-blank-plug-icon",
        egui::ColorImage::new([1, 1], egui::Color32::TRANSPARENT),
        egui::TextureOptions::NEAREST,
    );
    ui.ctx()
        .data_mut(|data| data.insert_temp(texture_id, texture.clone()));
    texture
}

fn draw_footer(
    ui: &mut egui::Ui,
    state: &mut WorkspaceState,
    inventory_blocker: Option<&str>,
    equip_warning: Option<&EquipReplacementWarning>,
    apply_requested: &mut Option<CandidateAction>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let candidate = state.candidate.clone();
            let can_apply = candidate.is_some();
            let equip = ui.add_enabled(can_apply, egui::Button::new("Equip item"));
            let equip = if let Some(warning) = equip_warning {
                equip.on_hover_text(warning.message())
            } else {
                equip
            };
            if equip.clicked() {
                if equip_warning.is_some() {
                    state.pending_destructive_equip = candidate;
                } else {
                    *apply_requested = candidate.map(|candidate| CandidateAction::Equip {
                        candidate,
                        discard_replaced: false,
                    });
                }
            }
            let add = ui.add_enabled(
                can_apply && inventory_blocker.is_none(),
                egui::Button::new("Add to inventory"),
            );
            let add = if let Some(reason) = inventory_blocker {
                add.on_disabled_hover_text(reason)
            } else {
                add
            };
            if add.clicked() {
                *apply_requested = state.candidate.clone().map(CandidateAction::AddToInventory);
            }
        });
    });
}
