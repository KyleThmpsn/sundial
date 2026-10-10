use super::item::{ItemInspection, ItemPage};
use super::state::{DefinitionSearch, HOME, InspectionTarget};
use super::*;
use crate::app::inspector::{
    DefinitionInspectionContext, MetadataSelection, definition_name, look,
    request_progression_selection,
};
use crate::app::{
    glyphs::Glyph,
    ui::{glyph_button, toolbar},
};

mod related;
mod search;
use related::*;
mod report;
use report::*;

pub(in crate::app) fn draw_catalog_hash_window(
    ctx: &egui::Context,
    catalog: &Catalog,
    document: Option<&mut Value>,
    progression_editable: bool,
    hash_inspection: &mut HashInspectionState,
    viewport_salt: &'static str,
) -> bool {
    let Some(hash) = hash_inspection.current else {
        return false;
    };
    if hash == HOME {
        search::draw_search_window(ctx, catalog, hash_inspection, viewport_salt);
        return false;
    }
    hash_inspection
        .runtime
        .prepare(catalog.install_path(), hash, catalog.inspection_access());
    hash_inspection.runtime.poll(ctx);
    let match_index = hash_inspection.match_index(catalog, hash);
    let matches = CatalogHashMatches::from_index(catalog, hash, &match_index);
    let match_groups = matches.match_groups();
    let match_count = match_groups.iter().map(|group| group.count).sum();
    let resolved_name = definition_title(catalog, hash);
    let title = format!(
        "Inspector: {}",
        resolved_name
            .clone()
            .unwrap_or_else(|| format_hash_hex(hash))
    );
    let default_size = *hash_inspection
        .default_size
        .get_or_insert_with(|| hash_inspector_default_size(&matches));
    let history = navigation_hashes(&hash_inspection.history);
    let forward = navigation_hashes(&hash_inspection.forward);
    let reverse_kind = super::reverse::reverse_kind(catalog, hash);
    let window = InspectorWindow {
        title: &title,
        default_size,
        viewport_salt,
        focus: hash_inspection.search.focus,
    };
    let (action, close_requested) = {
        let document_ref = document.as_deref();
        let blocked_reason = document_ref.and_then(|document| document["_blocked"].as_str());
        let account = document_ref.filter(|document| document.get("_blocked").is_none());
        let content = HashInspectorContent {
            catalog,
            document: account,
            blocked_reason,
            collection_state: std::cell::OnceCell::new(),
            progression_editable: progression_editable && account.is_some(),
            mutation_feedback: hash_inspection.mutation_feedback.as_ref(),
            hash,
            resolved_name: &resolved_name,
            history: &history,
            forward: &forward,
            matches: &matches,
            match_groups: &match_groups,
            match_count,
            source_context: hash_inspection.source_context.as_ref(),
            sections: hash_inspector_sections(&matches),
            kind: answer_layer_kind(hash, &matches, reverse_kind),
            reverse_kind,
            reverse_count: super::reverse::reverse_count(catalog, hash),
            base_id: inspector_base_id(viewport_salt),
        };
        show_inspector_window(ctx, &window, |ui, action| {
            draw_hash_inspector_contents(
                ui,
                &content,
                action,
                &mut hash_inspection.search,
                &mut hash_inspection.runtime,
            );
        })
    };
    let changed = progression_editable
        && apply_requested_progression_edit(
            document,
            catalog,
            hash_inspection,
            action.progression_edit,
        );
    apply_navigation(hash_inspection, action, close_requested);
    changed
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct HashInspectorAction {
    navigate_back: bool,
    navigate_forward: bool,
    history_index: Option<usize>,
    forward_index: Option<usize>,
    open_hash: Option<u64>,
    open_home: bool,
    menu_open: bool,
    pub(super) progression_edit: Option<InspectorProgressionEdit>,
}

/// What the inspector window shows around its contents.
struct InspectorWindow<'a> {
    title: &'a str,
    default_size: egui::Vec2,
    viewport_salt: &'static str,
    /// Bring a native window forward because its search field was asked for.
    focus: bool,
}

/// Ids for state that must stay stable inside wrapped layouts, one set per host window.
fn inspector_base_id(viewport_salt: &'static str) -> egui::Id {
    egui::Id::new(("definition_inspector", viewport_salt))
}

fn navigation_hashes(targets: &[InspectionTarget]) -> Vec<u64> {
    targets.iter().map(|target| target.hash).collect()
}

/// Shows the window in its own viewport, or as an embedded window where viewports are not
/// supported, and reports what the user asked for and whether the window should close.
fn show_inspector_window(
    ctx: &egui::Context,
    window: &InspectorWindow<'_>,
    mut draw: impl FnMut(&mut egui::Ui, &mut HashInspectorAction),
) -> (HashInspectorAction, bool) {
    let viewport_id =
        egui::ViewportId::from_hash_of(("catalog_hash_inspector", window.viewport_salt));
    let escape_guard_id =
        egui::Id::new(("catalog_hash_inspector_escape_guard", window.viewport_salt));
    if window.focus && !ctx.embed_viewports() {
        ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Focus);
    }
    ctx.show_viewport_immediate(
        viewport_id,
        egui::ViewportBuilder::default()
            .with_title(crate::ui::native_title(window.title))
            .with_icon(crate::ui::window_icon())
            .with_inner_size(window.default_size)
            .with_min_inner_size([720.0, 520.0])
            .with_max_inner_size([1_600.0, 1_100.0])
            .with_resizable(true),
        |child_ctx, class| {
            // An Escape that closes a popup, a menu or a focused field is not for the window.
            let escape_claimed = egui::Popup::is_any_open(child_ctx)
                || child_ctx
                    .data(|data| data.get_temp::<bool>(escape_guard_id))
                    .unwrap_or(false);
            let mut action = HashInspectorAction::default();
            let mut embedded_open = true;
            if class == egui::ViewportClass::EmbeddedWindow {
                egui::Window::new(window.title)
                    .id(egui::Id::new((
                        "embedded_catalog_hash_inspector",
                        window.viewport_salt,
                    )))
                    .open(&mut embedded_open)
                    .resizable(true)
                    .default_size(window.default_size)
                    .show(child_ctx, |ui| {
                        look::readable_small_text(ui);
                        draw(ui, &mut action);
                    });
            } else {
                egui::CentralPanel::default().show(child_ctx, |ui| {
                    look::readable_small_text(ui);
                    draw(ui, &mut action);
                });
            }
            action.open_hash = action
                .open_hash
                .or_else(|| take_hash_inspection_request(child_ctx));
            let escape_busy = action.menu_open || egui::Popup::is_any_open(child_ctx);
            child_ctx.data_mut(|data| data.insert_temp(escape_guard_id, escape_busy));
            let close_requested = !embedded_open
                || child_ctx.input(|input| input.viewport().close_requested())
                || (!escape_claimed
                    && child_ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                    }));
            (action, close_requested)
        },
    )
}

fn apply_navigation(
    state: &mut HashInspectionState,
    action: HashInspectorAction,
    close_requested: bool,
) {
    if close_requested {
        state.close();
    } else if let Some(index) = action.history_index {
        state.navigate_history(index);
    } else if let Some(index) = action.forward_index {
        state.navigate_forward(index);
    } else if action.navigate_back {
        state.back();
    } else if action.navigate_forward {
        state.forward();
    } else if action.open_home {
        state.go_home();
    } else if let Some(hash) = action.open_hash {
        state.open(hash);
    }
}

fn apply_requested_progression_edit(
    document: Option<&mut Value>,
    catalog: &Catalog,
    hash_inspection: &mut HashInspectionState,
    edit: Option<InspectorProgressionEdit>,
) -> bool {
    let Some(edit) = edit else {
        return false;
    };
    let result = document
        .ok_or_else(|| "No editable Sunrise state is loaded".to_owned())
        .and_then(|document| apply_inspector_progression_edit(document, catalog, edit));
    let changed = result.is_ok();
    hash_inspection.mutation_feedback = Some(match result {
        Ok(message) => (false, message),
        Err(error) => (true, error),
    });
    changed
}

fn apply_inspector_progression_edit(
    document: &mut Value,
    catalog: &Catalog,
    edit: InspectorProgressionEdit,
) -> Result<String, String> {
    match edit {
        InspectorProgressionEdit::Flag {
            definition_index,
            set,
        } => {
            if crate::persistence::progression::supports_seasonal_authoring(document)
                && let Some(entry) = catalog
                    .seasonal()
                    .and_then(|season| season.mod_for_flag(definition_index))
            {
                crate::app::progression::seasonal::apply(
                    document,
                    catalog,
                    crate::app::progression::seasonal::Edit::Mod {
                        sale_index: entry.sale_index,
                        owned: set,
                    },
                )?;
                return Ok("Artifact mod and seasonal counters updated".into());
            }
            let definition = catalog
                .unlock_flag_definition(definition_index)
                .ok_or_else(|| {
                    format!("Unlock Flag Definition #{definition_index} is unavailable")
                })?;
            set_collection_flag(document, definition_index, definition, set)
                .changed()
                .then(|| {
                    format!(
                        "{} {}",
                        definition
                            .name
                            .as_deref()
                            .filter(|name| !name.trim().is_empty())
                            .unwrap_or("Unlock flag"),
                        if set { "set" } else { "unset" }
                    )
                })
                .ok_or_else(|| "The unlock flag could not be updated".to_owned())
        }
        InspectorProgressionEdit::Value {
            definition_index,
            value,
        } => {
            if crate::persistence::progression::supports_seasonal_authoring(document)
                && crate::app::progression::seasonal::is_derived_value(definition_index)
            {
                return Err(
                    "Use Seasonal XP or Artifact Mods to update this runtime-derived value".into(),
                );
            }
            let definition = catalog
                .unlock_value_definition(definition_index)
                .ok_or_else(|| {
                    format!("Unlock Value Definition #{definition_index} is unavailable")
                })?;
            set_collection_value(document, definition_index, definition, value)
                .changed()
                .then(|| {
                    format!(
                        "{} set to {value}",
                        definition
                            .name
                            .as_deref()
                            .filter(|name| !name.trim().is_empty())
                            .unwrap_or("Unlock value")
                    )
                })
                .ok_or_else(|| "The unlock value could not be updated".to_owned())
        }
        InspectorProgressionEdit::Collectible {
            collectible_index,
            acquired,
        } => {
            let definition = catalog
                .collectibles()
                .iter()
                .find(|definition| definition.index == collectible_index)
                .ok_or_else(|| format!("Collectible #{collectible_index} is unavailable"))?;
            let snapshot = collection_state_snapshot(document)
                .ok_or_else(|| "The loaded progression settings are invalid".to_owned())?;
            crate::app::collections_page::set_collectible_acquisition_state(
                document, definition, &snapshot, catalog, acquired,
            )?;
            Ok(format!(
                "{} marked {}",
                collectible_item_name(catalog, definition),
                if acquired { "Acquired" } else { "Not Acquired" }
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HashInspectorSection {
    Item,
    Progression,
    Collections,
    Unlocks,
}

struct HashInspectorContent<'a> {
    catalog: &'a Catalog,
    /// The loaded account, or `None` when there is none or it is blocked.
    document: Option<&'a Value>,
    blocked_reason: Option<&'a str>,
    collection_state: std::cell::OnceCell<Option<CollectionStateSnapshot>>,
    progression_editable: bool,
    mutation_feedback: Option<&'a (bool, String)>,
    hash: u64,
    resolved_name: &'a Option<String>,
    history: &'a [u64],
    forward: &'a [u64],
    matches: &'a CatalogHashMatches<'a>,
    match_groups: &'a [CatalogMatchGroup],
    match_count: usize,
    source_context: Option<&'a DefinitionInspectionContext>,
    sections: Vec<HashInspectorSection>,
    /// What the hash is, as the header's kind chip names it.
    kind: &'static str,
    reverse_kind: Option<&'static str>,
    reverse_count: usize,
    base_id: egui::Id,
}

impl HashInspectorContent<'_> {
    /// Built on first use, since most pages never read account state.
    fn collection_state(&self) -> Option<&CollectionStateSnapshot> {
        self.collection_state
            .get_or_init(|| self.document.and_then(collection_state_snapshot))
            .as_ref()
    }

    fn is_item(&self) -> bool {
        self.matches.item.is_some() || self.matches.item_package_metadata.is_some()
    }
}

impl HashInspectorSection {
    const fn label(self) -> &'static str {
        match self {
            Self::Item => "Item",
            Self::Progression => "Progression",
            Self::Collections => "Collections",
            Self::Unlocks => "Unlocks",
        }
    }
}

/// Groups of content on a page, drawn in an order that puts what the hash is first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PageGroup {
    Reverse,
    Item,
    Structure,
    Progression,
    Collections,
    Unlocks,
}

fn draw_hash_inspector_contents(
    ui: &mut egui::Ui,
    content: &HashInspectorContent<'_>,
    action: &mut HashInspectorAction,
    search: &mut DefinitionSearch,
    runtime: &mut super::runtime::RuntimeInspectionState,
) {
    navigation_input(ui, content.history, content.forward, action);
    search_shortcut(ui, search);
    if let Some(reason) = content.blocked_reason {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!("Account state is unavailable: {reason}"),
        );
    }
    toolbar(ui, |ui| {
        navigation_buttons(
            ui,
            content.catalog,
            content.history,
            content.forward,
            action,
        );
        home_button(ui, action);
        ui.separator();
        search::toolbar_search(ui, content.catalog, content.base_id, search, action);
        more_menu(ui, content, action);
    });
    if let Some((error, message)) = content.mutation_feedback {
        if *error {
            ui.colored_label(ui.visuals().error_fg_color, message);
        } else {
            ui.weak(message);
        }
    }
    ui.separator();
    ui.add_space(4.0);
    let page = if content.is_item() {
        super::item::draw_item_header(ui, &item_inspection(content));
        Some(draw_item_tabs(ui, content))
    } else {
        draw_definition_header(ui, content);
        crate::ui::model_preview::inspected_unavailable(ui);
        None
    };
    ui.add_space(6.0);
    egui::ScrollArea::vertical()
        .id_salt(("catalog_hash_metadata_scroll", content.hash, page))
        .auto_shrink([false, false])
        .show(ui, |ui| match page {
            Some(page) => draw_item_page(ui, content, action, runtime, page),
            None => draw_definition_page(ui, content, action, runtime),
        });
}

/// Alt+Arrow and the mouse's back and forward buttons move through history.
fn navigation_input(
    ui: &mut egui::Ui,
    history: &[u64],
    forward: &[u64],
    action: &mut HashInspectorAction,
) {
    // Alt+Arrow belongs to a focused field or an open popup.
    let keyboard_free = !egui::Popup::is_any_open(ui);
    let pointer_over = ui.rect_contains_pointer(ui.max_rect());
    let (back, next) = ui.input_mut(|input| {
        let back = !history.is_empty()
            && ((keyboard_free && input.consume_key(egui::Modifiers::ALT, egui::Key::ArrowLeft))
                || (pointer_over && input.pointer.button_pressed(egui::PointerButton::Extra1)));
        let next = !forward.is_empty()
            && ((keyboard_free && input.consume_key(egui::Modifiers::ALT, egui::Key::ArrowRight))
                || (pointer_over && input.pointer.button_pressed(egui::PointerButton::Extra2)));
        (back, next)
    });
    action.navigate_back |= back;
    action.navigate_forward |= next;
}

/// Ctrl+I focuses the search field of the window it is pressed in.
fn search_shortcut(ui: &mut egui::Ui, search: &mut DefinitionSearch) {
    if ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::I)) {
        search.focus = true;
    }
}

fn navigation_buttons(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    history: &[u64],
    forward: &[u64],
    action: &mut HashInspectorAction,
) {
    let previous_label = history.last().copied().map_or_else(
        || "No previous definition".to_owned(),
        |previous| format!("Back to {} (Alt+Left)", history_label(catalog, previous)),
    );
    if ui
        .add_enabled_ui(!history.is_empty(), |ui| {
            glyph_button(ui, Glyph::ChevronLeft, &previous_label)
        })
        .inner
        .clicked()
    {
        action.navigate_back = true;
    }
    let next_label = forward.last().copied().map_or_else(
        || "No next definition".to_owned(),
        |next| format!("Forward to {} (Alt+Right)", history_label(catalog, next)),
    );
    if ui
        .add_enabled_ui(!forward.is_empty(), |ui| {
            glyph_button(ui, Glyph::ChevronRight, &next_label)
        })
        .inner
        .clicked()
    {
        action.navigate_forward = true;
    }
}

fn home_button(ui: &mut egui::Ui, action: &mut HashInspectorAction) {
    let side = ui
        .text_style_height(&egui::TextStyle::Body)
        .max(ui.spacing().interact_size.y);
    let response = ui
        .add(
            egui::Button::new(egui::RichText::new(egui_phosphor::regular::HOUSE))
                .small()
                .min_size(egui::Vec2::splat(side)),
        )
        .on_hover_text("Home");
    if response.clicked() {
        action.open_home = true;
    }
}

fn more_menu(
    ui: &mut egui::Ui,
    content: &HashInspectorContent<'_>,
    action: &mut HashInspectorAction,
) {
    let more = ui.menu_button("More", |ui| {
        if ui.button("Copy Technical Report").clicked() {
            ui.ctx().copy_text(hash_inspector_report(content));
            ui.close();
        }
        recent_menu(
            ui,
            content.catalog,
            content.history,
            content.forward,
            action,
        );
    });
    action.menu_open = more.inner.is_some();
}

/// Back and forward history, furthest forward first, each opening its definition.
fn recent_menu(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    history: &[u64],
    forward: &[u64],
    action: &mut HashInspectorAction,
) {
    let forward_entries = forward
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, hash)| *hash != HOME)
        .collect::<Vec<_>>();
    let back_entries = history
        .iter()
        .copied()
        .enumerate()
        .rev()
        .filter(|(_, hash)| *hash != HOME)
        .collect::<Vec<_>>();
    if forward_entries.is_empty() && back_entries.is_empty() {
        return;
    }
    ui.separator();
    look::subheading(ui, "Recent");
    for (index, hash) in forward_entries {
        let entry = egui::Button::new(history_label(catalog, hash)).shortcut_text("Forward");
        if ui.add(entry).clicked() {
            action.forward_index = Some(index);
            ui.close();
        }
    }
    for (index, hash) in back_entries {
        if ui.button(history_label(catalog, hash)).clicked() {
            action.history_index = Some(index);
            ui.close();
        }
    }
}

fn item_inspection<'a>(content: &HashInspectorContent<'a>) -> ItemInspection<'a> {
    ItemInspection {
        catalog: content.catalog,
        hash: content.hash,
        resolved_name: content.resolved_name,
        matches: content.matches,
        source_context: content.source_context,
    }
}

/// The item tabs that have something to show, with the stored tab or Overview selected.
fn draw_item_tabs(ui: &mut egui::Ui, content: &HashInspectorContent<'_>) -> ItemPage {
    let related = related_count(content);
    let pages = super::item::available_pages(&item_inspection(content), related);
    let page_id = content.base_id.with(("item_page", content.hash));
    let mut page = ui
        .data(|data| data.get_temp::<ItemPage>(page_id))
        .filter(|page| pages.contains(page))
        .unwrap_or_default();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        super::item::draw_page_tabs(ui, &pages, &mut page, related);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            super::item::draw_preview_button(ui, item_inspection(content));
        });
    });
    ui.data_mut(|data| data.insert_temp(page_id, page));
    page
}

fn draw_item_page(
    ui: &mut egui::Ui,
    content: &HashInspectorContent<'_>,
    action: &mut HashInspectorAction,
    runtime: &mut super::runtime::RuntimeInspectionState,
    page: ItemPage,
) {
    if page != ItemPage::Related {
        draw_hash_item_matches(ui, item_inspection(content), runtime, page);
        // Where a plug is offered and which triumphs grant an item belong beside what it is.
        if page == ItemPage::Overview {
            super::reverse::draw_reverse_matches(
                ui,
                content.catalog,
                content.hash,
                content.collection_state(),
            );
        }
        return;
    }
    for group in [
        PageGroup::Structure,
        PageGroup::Progression,
        PageGroup::Collections,
        PageGroup::Unlocks,
    ] {
        draw_page_group(ui, content, action, runtime, group);
    }
    draw_related_records(ui, content, records_open_by_default(content, true));
}

fn draw_definition_page(
    ui: &mut egui::Ui,
    content: &HashInspectorContent<'_>,
    action: &mut HashInspectorAction,
    runtime: &mut super::runtime::RuntimeInspectionState,
) {
    for group in page_order(content) {
        draw_page_group(ui, content, action, runtime, group);
    }
    draw_related_records(ui, content, records_open_by_default(content, false));
}

/// The group describing what the hash is comes first, after the reverse lookups when only
/// they recognise it, then the remaining groups in their usual order.
fn page_order(content: &HashInspectorContent<'_>) -> Vec<PageGroup> {
    let primary = primary_group(content.kind);
    let reverse_first = content.reverse_kind.is_some();
    let mut order = Vec::with_capacity(6);
    if reverse_first {
        order.push(PageGroup::Reverse);
    }
    order.extend(primary);
    if !reverse_first {
        order.push(PageGroup::Reverse);
    }
    for group in [
        PageGroup::Item,
        PageGroup::Structure,
        PageGroup::Progression,
        PageGroup::Collections,
        PageGroup::Unlocks,
    ] {
        if !order.contains(&group) {
            order.push(group);
        }
    }
    order
}

fn primary_group(kind: &str) -> Option<PageGroup> {
    match kind {
        "Progression Definition"
        | "Objective"
        | "Record"
        | "Artifact Mod"
        | "Season Pass Reward"
        | "Dawn Mission" => Some(PageGroup::Progression),
        "Item Stat Definition" | "Inventory Bucket" => Some(PageGroup::Item),
        "Item Stat Group" | "Power Cap Definition" => Some(PageGroup::Structure),
        "Collectible" | "Material Requirement Set" => Some(PageGroup::Collections),
        "Unlock Flag" | "Unlock Value" => Some(PageGroup::Unlocks),
        _ => None,
    }
}

fn draw_page_group(
    ui: &mut egui::Ui,
    content: &HashInspectorContent<'_>,
    action: &mut HashInspectorAction,
    runtime: &mut super::runtime::RuntimeInspectionState,
    group: PageGroup,
) {
    let catalog = content.catalog;
    let matches = content.matches;
    let has = |section: HashInspectorSection| content.sections.contains(&section);
    match group {
        PageGroup::Reverse => super::reverse::draw_reverse_matches(
            ui,
            catalog,
            content.hash,
            content.collection_state(),
        ),
        PageGroup::Item if has(HashInspectorSection::Item) => {
            draw_hash_item_matches(ui, item_inspection(content), runtime, ItemPage::Overview);
        }
        PageGroup::Structure
            if matches.item_stat_group.is_some() || matches.power_cap_definition.is_some() =>
        {
            super::item_details::draw_structure_matches(ui, catalog, matches);
        }
        PageGroup::Progression if has(HashInspectorSection::Progression) => {
            draw_hash_progression_matches(
                ui,
                catalog,
                content.document,
                content.collection_state(),
                content.hash,
                matches,
            );
        }
        PageGroup::Collections if has(HashInspectorSection::Collections) => {
            draw_hash_collection_matches(
                ui,
                catalog,
                content.hash,
                matches,
                content.collection_state(),
                content.progression_editable,
                action,
            );
        }
        PageGroup::Unlocks if has(HashInspectorSection::Unlocks) => {
            draw_hash_unlock_matches(
                ui,
                catalog,
                matches,
                content.collection_state(),
                content.progression_editable,
                action,
            );
        }
        _ => {}
    }
}

/// Related Records opens by default when nothing else on the page or tab has content.
fn records_open_by_default(content: &HashInspectorContent<'_>, is_item: bool) -> bool {
    (is_item || (content.reverse_count == 0 && content.reverse_kind.is_none()))
        && !content
            .sections
            .iter()
            .any(|section| !is_item || *section != HashInspectorSection::Item)
}

/// What an item's Related Records tab lists: related records and the progression rows that
/// have no record of their own. Reverse relationships are on the Overview tab.
fn related_count(content: &HashInspectorContent<'_>) -> usize {
    let matches = content.matches;
    related_catalog_records(content).len()
        + matches.record_matches.len()
        + matches.record_references.len()
        + matches.artifact_mods.len()
        + usize::from(matches.season_pass_reward.is_some())
        + usize::from(matches.mission_scenario.is_some())
        + matches.flag_definitions.len()
        + matches.value_definitions.len()
}

/// What a non-item hash is, by the first kind of definition that carries it.
fn answer_layer_kind(
    hash: u64,
    matches: &CatalogHashMatches<'_>,
    reverse_kind: Option<&'static str>,
) -> &'static str {
    [
        (
            !matches.progression_definitions.is_empty(),
            "Progression Definition",
        ),
        (!matches.objectives.is_empty(), "Objective"),
        (!matches.record_matches.is_empty(), "Record"),
        (!matches.artifact_mods.is_empty(), "Artifact Mod"),
        (matches.season_pass_reward.is_some(), "Season Pass Reward"),
        (matches.mission_scenario.is_some(), "Dawn Mission"),
        (
            matches.item_stat_definition.is_some(),
            "Item Stat Definition",
        ),
        (matches.item_stat_group.is_some(), "Item Stat Group"),
        (
            matches.power_cap_definition.is_some(),
            "Power Cap Definition",
        ),
        (!matches.bucket_items.is_empty(), "Inventory Bucket"),
        (
            matches
                .collectible_matches
                .iter()
                .any(|collectible| collectible.hash == hash),
            "Collectible",
        ),
        (
            matches
                .material_requirement_set_matches
                .iter()
                .any(|set| set.hash == hash),
            "Material Requirement Set",
        ),
        (!matches.flag_definitions.is_empty(), "Unlock Flag"),
        (!matches.value_definitions.is_empty(), "Unlock Value"),
    ]
    .into_iter()
    .find_map(|(found, kind)| found.then_some(kind))
    .or(reverse_kind)
    .unwrap_or("Catalog Hash")
}

/// The card at the top of a page for anything other than an item.
fn draw_definition_header(ui: &mut egui::Ui, content: &HashInspectorContent<'_>) {
    let title = content
        .resolved_name
        .clone()
        .unwrap_or_else(|| format_hash_hex(content.hash));
    let header = look::Header {
        title: &title,
        kind: content.kind,
        hash: content.hash,
        icon: content
            .matches
            .collectible_matches
            .iter()
            .find(|collectible| collectible.hash == content.hash)
            .map_or(content.hash, |collectible| collectible.item_hash),
        subtitle: None,
        path: look::presentation_crumbs(content.catalog, content.hash),
    };
    let facts = |ui: &mut egui::Ui| {
        for (label, count) in match_facts(content) {
            look::fact(ui, label, count.to_string());
        }
    };
    match progression_selection(content.matches) {
        Some(selection) => look::header_with_actions(ui, content.catalog, &header, facts, |ui| {
            open_in_progression_button(ui, content, selection);
        }),
        None => look::header(ui, content.catalog, &header, facts),
    }
    if let Some(context) = content.source_context {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format!("Opened from {}", context.source)).color(look::muted(ui)),
        );
    } else if content.match_count == 0
        && content.reverse_kind.is_none()
        && content.reverse_count == 0
    {
        ui.add_space(8.0);
        look::empty_state(ui, "No Catalog Matches");
    }
}

/// The unlock definition behind a flag or value hash, for the Progression page.
fn progression_selection(matches: &CatalogHashMatches<'_>) -> Option<MetadataSelection> {
    matches
        .flag_definitions
        .first()
        .map(|(index, _)| MetadataSelection::FlagDefinition(*index))
        .or_else(|| {
            matches
                .value_definitions
                .first()
                .map(|(index, _)| MetadataSelection::ValueDefinition(*index))
        })
}

fn open_in_progression_button(
    ui: &mut egui::Ui,
    content: &HashInspectorContent<'_>,
    selection: MetadataSelection,
) {
    let available = content.document.is_some();
    let response = ui.add_enabled(available, egui::Button::new("Open in Progression"));
    if response.clicked() {
        request_progression_selection(ui.ctx(), selection);
        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
    }
    response.on_disabled_hover_text("Account unavailable.");
}

/// The catalog match groups as short counts for the header, leaving out the definition itself.
fn match_facts(content: &HashInspectorContent<'_>) -> Vec<(&'static str, usize)> {
    let mut facts = Vec::new();
    for group in content.match_groups {
        let Some((label, defines)) = match_group_fact(group.label) else {
            continue;
        };
        let count = if defines == Some(content.kind) {
            group.count.saturating_sub(1)
        } else {
            group.count
        };
        push_fact(&mut facts, label, count);
    }
    let matches = content.matches;
    push_fact(
        &mut facts,
        "Items",
        matches.stat_group_items.len() + matches.power_cap_items.len(),
    );
    facts
}

fn push_fact(facts: &mut Vec<(&'static str, usize)>, label: &'static str, count: usize) {
    if count == 0 {
        return;
    }
    if let Some(fact) = facts.iter_mut().find(|(existing, _)| *existing == label) {
        fact.1 += count;
    } else {
        facts.push((label, count));
    }
}

/// A match group's fact label, and the page kind it defines, if any.
fn match_group_fact(group: &str) -> Option<(&'static str, Option<&'static str>)> {
    Some(match group {
        "Progression Definition" => ("Progressions", Some("Progression Definition")),
        "Progression Reward Reference" => ("Progression Rewards", None),
        "Faction Progression Reference" => ("Factions", None),
        "Unlock Flag Definition" => ("Unlock Flags", Some("Unlock Flag")),
        "Unlock Value Definition" => ("Unlock Values", Some("Unlock Value")),
        "Objective" => ("Objectives", Some("Objective")),
        "Objective Owner Reference" => ("Objectives", None),
        "Objective Trait Reference" => ("Objective Traits", None),
        "Progression Reader Reference" => ("Readers", None),
        "Record" => ("Records", Some("Record")),
        "Record Reference" => ("Records", None),
        "Artifact Mod" => ("Artifact Mods", Some("Artifact Mod")),
        "Collectible" => ("Collectibles", Some("Collectible")),
        "Material Requirement Set" => ("Material Sets", Some("Material Requirement Set")),
        "Investment Stat Reference" | "Inventory Bucket Item" => ("Items", None),
        _ => return None,
    })
}

/// A history entry's label: the definition's name with its hash.
fn history_label(catalog: &Catalog, hash: u64) -> String {
    if hash == HOME {
        return "Home".to_owned();
    }
    definition_title(catalog, hash).map_or_else(
        || format_hash_hex(hash),
        |name| format!("{name} · {}", format_hash_hex(hash)),
    )
}

/// The name the window title, history and search give a definition: the package item name,
/// then a stat or progression definition's name, then any catalog name, then a bucket's label.
fn definition_title(catalog: &Catalog, hash: u64) -> Option<String> {
    let stat_name = catalog
        .item_stat_definition_by_hash(hash)
        .map(|definition| definition.name.trim())
        .filter(|name| !name.is_empty());
    catalog
        .package_item_name(hash)
        .or(stat_name)
        .map(str::to_owned)
        .or_else(|| {
            catalog
                .progression_definitions()
                .iter()
                .filter(|definition| definition.hash == hash)
                .find_map(progression_display_name)
        })
        .or_else(|| catalog.display_name(hash).map(str::to_owned))
        .or_else(|| {
            catalog
                .items_for_bucket(hash)
                .find_map(|item| catalog.inventory_metadata(item.hash))
                .map(|metadata| metadata.bucket_label())
        })
}

fn hash_inspector_sections(matches: &CatalogHashMatches<'_>) -> Vec<HashInspectorSection> {
    let mut sections = Vec::with_capacity(4);
    if matches.item.is_some()
        || matches.item_package_metadata.is_some()
        || matches.item_stat_definition.is_some()
        || !matches.investment_stat_references.is_empty()
        || matches.inventory_metadata.is_some()
        || !matches.bucket_items.is_empty()
        || matches.item_stat_group.is_some()
        || matches.power_cap_definition.is_some()
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
        || !matches.record_matches.is_empty()
        || !matches.record_references.is_empty()
        || !matches.artifact_mods.is_empty()
        || matches.season_pass_reward.is_some()
        || matches.mission_scenario.is_some()
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
    if matches.item.is_some() || matches.item_package_metadata.is_some() {
        return egui::vec2(1_240.0, 860.0);
    }
    if matches.progression_definitions.len() == 1 && matches.count() == 1 {
        let steps = matches.progression_definitions[0].1.steps.len() as f32;
        return egui::vec2(
            1_040.0,
            (640.0 + steps.min(12.0) * 20.0).clamp(720.0, 900.0),
        );
    }
    if matches.count() <= 2 {
        egui::vec2(1_000.0, 720.0)
    } else {
        egui::vec2(1_160.0, 840.0)
    }
}

#[cfg(test)]
mod tests;
