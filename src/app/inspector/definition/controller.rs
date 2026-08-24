use super::*;

pub(in crate::app) fn draw_catalog_hash_window(
    ctx: &egui::Context,
    catalog: &Catalog,
    document: Option<&Value>,
    hash_inspection: &mut HashInspectionState,
    viewport_salt: &'static str,
) {
    let Some(hash) = hash_inspection.current else {
        return;
    };
    let match_index = hash_inspection.match_index(catalog, hash);
    let matches = CatalogHashMatches::from_index(catalog, hash, &match_index);
    let match_count = matches.count();
    let resolved_name = if matches.item_package_metadata.is_some() {
        catalog.package_item_name(hash).map(str::to_owned)
    } else if let Some(definition) = matches.item_stat_definition {
        (!definition.name.trim().is_empty()).then(|| definition.name.clone())
    } else if !matches.progression_definitions.is_empty() {
        matches
            .progression_definitions
            .iter()
            .find_map(|(_, definition)| progression_display_name(definition))
    } else {
        catalog.display_name(hash).map(str::to_owned).or_else(|| {
            matches
                .bucket_items
                .iter()
                .find_map(|item| catalog.inventory_metadata(item.hash))
                .map(|metadata| metadata.bucket_label())
        })
    };
    let title = resolved_name.as_deref().map_or_else(
        || format!("Definition inspector: 0x{hash:08X}"),
        |name| format!("Definition inspector: {name}"),
    );
    let default_size = hash_inspector_default_size(&matches);
    let history = hash_inspection.history.clone();
    let forward = hash_inspection.forward.clone();
    let mut lookup = hash_inspection.lookup.clone();
    let mut lookup_error = hash_inspection.lookup_error;
    let content = HashInspectorContent {
        catalog,
        document,
        hash,
        resolved_name: &resolved_name,
        history: &history,
        forward: &forward,
        matches: &matches,
        match_count,
    };
    let viewport_id = egui::ViewportId::from_hash_of(("catalog_hash_inspector", viewport_salt));
    let (action, close_requested) = ctx.show_viewport_immediate(
        viewport_id,
        egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size(default_size)
            .with_min_inner_size([560.0, 360.0])
            .with_max_inner_size([1_200.0, 900.0])
            .with_resizable(true),
        |child_ctx, class| {
            let mut action = HashInspectorAction::default();
            let mut embedded_open = true;
            if class == egui::ViewportClass::Embedded {
                egui::Window::new(&title)
                    .id(egui::Id::new((
                        "embedded_catalog_hash_inspector",
                        viewport_salt,
                    )))
                    .open(&mut embedded_open)
                    .resizable(true)
                    .default_size(default_size)
                    .show(child_ctx, |ui| {
                        draw_hash_inspector_contents(
                            ui,
                            &content,
                            &mut action,
                            &mut lookup,
                            &mut lookup_error,
                        );
                    });
            } else {
                egui::CentralPanel::default().show(child_ctx, |ui| {
                    draw_hash_inspector_contents(
                        ui,
                        &content,
                        &mut action,
                        &mut lookup,
                        &mut lookup_error,
                    );
                });
            }
            action.open_hash = action
                .open_hash
                .or_else(|| take_hash_inspection_request(child_ctx));
            let close_requested = !embedded_open
                || child_ctx.input(|input| {
                    input.viewport().close_requested() || input.key_pressed(egui::Key::Escape)
                });
            (action, close_requested)
        },
    );
    hash_inspection.lookup = lookup;
    hash_inspection.lookup_error = lookup_error;

    if close_requested {
        hash_inspection.close();
    } else if let Some(history_index) = action.history_index {
        hash_inspection.navigate_history(history_index);
    } else if action.navigate_back {
        hash_inspection.back();
    } else if action.navigate_forward {
        hash_inspection.forward();
    } else if let Some(requested_hash) = action.open_hash {
        hash_inspection.open(requested_hash);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct HashInspectorAction {
    navigate_back: bool,
    navigate_forward: bool,
    history_index: Option<usize>,
    open_hash: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HashInspectorSection {
    SandboxPerk,
    Item,
    Progression,
    Collections,
    Unlocks,
}

struct HashInspectorContent<'a> {
    catalog: &'a Catalog,
    document: Option<&'a Value>,
    hash: u64,
    resolved_name: &'a Option<String>,
    history: &'a [u64],
    forward: &'a [u64],
    matches: &'a CatalogHashMatches<'a>,
    match_count: usize,
}

impl HashInspectorSection {
    const fn label(self) -> &'static str {
        match self {
            Self::SandboxPerk => "Sandbox perk",
            Self::Item => "Items & references",
            Self::Progression => "Progression",
            Self::Collections => "Collections",
            Self::Unlocks => "Unlocks",
        }
    }
}

fn draw_hash_inspector_contents(
    ui: &mut egui::Ui,
    content: &HashInspectorContent<'_>,
    action: &mut HashInspectorAction,
    lookup: &mut String,
    lookup_error: &mut bool,
) {
    let HashInspectorContent {
        catalog,
        document,
        hash,
        resolved_name,
        history,
        forward,
        matches,
        match_count,
    } = content;
    if !history.is_empty()
        && ui.input(|input| input.modifiers.alt && input.key_pressed(egui::Key::ArrowLeft))
    {
        action.navigate_back = true;
    }
    if !forward.is_empty()
        && ui.input(|input| input.modifiers.alt && input.key_pressed(egui::Key::ArrowRight))
    {
        action.navigate_forward = true;
    }
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Inspect hash").strong());
        let response = ui.add(
            egui::TextEdit::singleline(lookup)
                .hint_text("0x00000000")
                .desired_width(150.0),
        );
        if response.changed() {
            *lookup_error = false;
        }
        let submitted = ui.button("Open").clicked()
            || (response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
        if submitted {
            if let Some(requested_hash) = parse_hash_hex(lookup).filter(|hash| *hash != 0) {
                action.open_hash = Some(requested_hash);
                *lookup_error = false;
            } else {
                *lookup_error = true;
            }
        }
    });
    if *lookup_error {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "Enter a non-zero, 0x-prefixed hexadecimal hash.",
        );
    }
    ui.separator();

    let sections = hash_inspector_sections(matches);
    let mut jump_to = None;
    if !history.is_empty() || !forward.is_empty() || sections.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            if let Some(previous) = history.last().copied() {
                let previous_label = hash_history_label(catalog, previous);
                if ui
                    .button("Back")
                    .on_hover_text(format!("Return to {previous_label} (Alt+Left)"))
                    .clicked()
                {
                    action.navigate_back = true;
                }
            }
            if let Some(next) = forward.last().copied() {
                let next_label = hash_history_label(catalog, next);
                if ui
                    .button("Forward")
                    .on_hover_text(format!("Go to {next_label} (Alt+Right)"))
                    .clicked()
                {
                    action.navigate_forward = true;
                }
            }
            if history.len() > 1 {
                let mut selected_history = None;
                egui::ComboBox::from_id_salt("hash_inspector_history")
                    .selected_text(format!("History ({})", history.len()))
                    .show_ui(ui, |ui| {
                        for (index, previous_hash) in history.iter().copied().enumerate().rev() {
                            ui.selectable_value(
                                &mut selected_history,
                                Some(index),
                                hash_history_label(catalog, previous_hash),
                            );
                        }
                    });
                if let Some(index) = selected_history {
                    action.history_index = Some(index);
                }
            }
            if sections.len() > 1 {
                ui.label(egui::RichText::new("Sections").weak());
                for section in &sections {
                    if ui.small_button(section.label()).clicked() {
                        jump_to = Some(*section);
                    }
                }
            }
        });
        ui.separator();
    }

    egui::ScrollArea::vertical()
        .id_salt(("catalog_hash_metadata_scroll", *hash))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if matches.item_package_metadata.is_none() && matches.item.is_none() {
                egui::Grid::new("catalog_hash_identity")
                    .num_columns(2)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        hash_detail_field(ui, "Definition hash", format_hash_hex(*hash), true);
                    });
            }

            if sections.contains(&HashInspectorSection::SandboxPerk) {
                scroll_to_hash_section(ui, jump_to, HashInspectorSection::SandboxPerk);
                if let Some(definition) = matches.sandbox_perk_definition {
                    draw_hash_sandbox_perk_definition(ui, definition);
                }
            }
            if sections.contains(&HashInspectorSection::Item) {
                scroll_to_hash_section(ui, jump_to, HashInspectorSection::Item);
                draw_hash_item_matches(ui, catalog, *hash, resolved_name, matches);
            }
            if sections.contains(&HashInspectorSection::Progression) {
                scroll_to_hash_section(ui, jump_to, HashInspectorSection::Progression);
                draw_hash_progression_matches(ui, catalog, *document, *hash, matches);
            }
            if sections.contains(&HashInspectorSection::Collections) {
                scroll_to_hash_section(ui, jump_to, HashInspectorSection::Collections);
                draw_hash_collection_matches(ui, catalog, *hash, matches);
            }
            if sections.contains(&HashInspectorSection::Unlocks) {
                scroll_to_hash_section(ui, jump_to, HashInspectorSection::Unlocks);
                draw_hash_unlock_matches(
                    ui,
                    catalog,
                    "Unlock flag definitions",
                    &matches.flag_definitions,
                );
                draw_hash_unlock_matches(
                    ui,
                    catalog,
                    "Unlock value definitions",
                    &matches.value_definitions,
                );
            }
            if *match_count == 0 {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("No directly indexed package entity uses this hash.")
                        .weak(),
                );
            }
        });
}

fn hash_history_label(catalog: &Catalog, hash: u64) -> String {
    catalog.display_name(hash).map_or_else(
        || format_hash_hex(hash),
        |name| format!("{} · {name}", format_hash_hex(hash)),
    )
}

fn scroll_to_hash_section(
    ui: &mut egui::Ui,
    requested: Option<HashInspectorSection>,
    section: HashInspectorSection,
) {
    if requested == Some(section) {
        ui.scroll_to_cursor(Some(egui::Align::Min));
    }
}

fn hash_inspector_sections(matches: &CatalogHashMatches<'_>) -> Vec<HashInspectorSection> {
    let mut sections = Vec::with_capacity(5);
    if matches.sandbox_perk_definition.is_some() {
        sections.push(HashInspectorSection::SandboxPerk);
    }
    if matches.item.is_some()
        || matches.item_package_metadata.is_some()
        || matches.item_stat_definition.is_some()
        || !matches.investment_stat_references.is_empty()
        || !matches.intrinsic_perk_item_references.is_empty()
        || matches.inventory_metadata.is_some()
        || !matches.bucket_items.is_empty()
    {
        sections.push(HashInspectorSection::Item);
    }
    if !matches.progression_definitions.is_empty()
        || !matches.progression_reward_matches.is_empty()
        || !matches.progression_faction_matches.is_empty()
        || !matches.objectives.is_empty()
        || !matches.owner_matches.is_empty()
        || !matches.trait_matches.is_empty()
        || !matches.context_matches.is_empty()
    {
        sections.push(HashInspectorSection::Progression);
    }
    if !matches.collectible_matches.is_empty()
        || !matches.material_requirement_set_matches.is_empty()
    {
        sections.push(HashInspectorSection::Collections);
    }
    if !matches.flag_definitions.is_empty() || !matches.value_definitions.is_empty() {
        sections.push(HashInspectorSection::Unlocks);
    }
    sections
}

fn hash_inspector_default_size(matches: &CatalogHashMatches<'_>) -> egui::Vec2 {
    if matches.item.is_some()
        || matches.item_package_metadata.is_some()
        || matches.sandbox_perk_definition.is_some()
    {
        return egui::vec2(900.0, 720.0);
    }
    if matches.progression_definitions.len() == 1 && matches.count() == 1 {
        let steps = matches.progression_definitions[0].1.steps.len() as f32;
        return egui::vec2(760.0, (430.0 + steps.min(12.0) * 24.0).clamp(520.0, 720.0));
    }
    if matches.count() <= 2 {
        egui::vec2(720.0, 400.0)
    } else {
        egui::vec2(900.0, 720.0)
    }
}
