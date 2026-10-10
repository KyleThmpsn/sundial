//! The supported plug choice picker and perk rows Parhelion draws through Sundial's catalog.
use super::*;

pub(crate) fn draw_supported_plug_choice_picker(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &ItemDef,
    query: &mut String,
    options: crate::investment::PlugChoicePickerOptions<'_>,
    leading_action: impl FnOnce(&mut egui::Ui) -> bool,
) -> Option<(usize, Option<u64>)> {
    let crate::investment::PlugChoicePickerOptions {
        socket_index,
        socket_type_override,
        choice_index,
        current_hash,
        button,
        mode,
        ..
    } = options;
    let current_hash = current_hash.map(u64::from);
    let icon_override = button.icon_override;
    let button_text = button.text;
    let button_icon_hash = button.icon_hash.map(u64::from);
    let button_tooltip = button.tooltip;
    let button_width = f32::from(button.width);
    let mut snapshot = plug_picker_snapshot_for_mode(
        catalog,
        item,
        socket_index,
        socket_type_override,
        current_hash,
        Some(button_text),
        *mode,
    );
    snapshot.preview = options.preview.cloned();
    snapshot.preview_guarded = false;
    snapshot.custom_current = current_hash.is_none() && button_tooltip.is_some();
    let row_height = ui
        .spacing()
        .interact_size
        .y
        .max(16.0 + 2.0 * ui.spacing().button_padding.y);
    let button = match icon_override {
        Some(crate::investment::IconOverride::Texture(id)) => egui::Button::image_and_text(
            egui::Image::new((
                id,
                egui::Vec2::splat(row_height - 2.0 * ui.spacing().button_padding.y),
            )),
            button_text,
        ),
        Some(crate::investment::IconOverride::Pending) => egui::Button::new(button_text),
        None => button_icon_hash.map_or_else(
            || egui::Button::new(button_text),
            |hash| {
                catalog_button(
                    ui,
                    catalog,
                    hash,
                    button_text,
                    row_height - 2.0 * ui.spacing().button_padding.y,
                )
            },
        ),
    };
    let button = button
        .truncate()
        .min_size(egui::vec2(button_width.max(0.0), row_height));
    let left_aligned =
        egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min);
    // The button already fills the width, so it goes in as it is: `add_sized` would centre the
    // label of a plug with no icon, while every plug with an icon starts at the left.
    let anchor = if button_width > 0.0 {
        ui.allocate_ui_with_layout(egui::vec2(button_width, row_height), left_aligned, |ui| {
            ui.add(button)
        })
        .inner
    } else {
        ui.with_layout(left_aligned, |ui| ui.add(button)).inner
    };
    let anchor = if let Some(tooltip) = button_tooltip {
        anchor.on_hover_ui(|ui| {
            crate::app::item_editor::draw_item_tooltip_with_icon(
                ui,
                catalog,
                button_icon_hash.unwrap_or_default(),
                Some(tooltip),
                icon_override,
            );
        })
    } else {
        button_icon_hash.map_or(anchor.clone(), |hash| {
            catalog_item_tooltip(anchor, catalog, hash)
        })
    };
    match draw_plug_icon_picker_with_action(
        ui,
        catalog,
        (
            "investment-authoring-choice",
            item.hash,
            socket_index,
            choice_index,
        ),
        query,
        &snapshot,
        PickerHeight {
            min: 180.0,
            max: 420.0,
        },
        &anchor,
        leading_action,
    ) {
        Some(ItemEditorAction::SetPlug { socket_index, hash }) => Some((socket_index, hash)),
        Some(ItemEditorAction::SetPlugSelectionMode { mode: requested }) => {
            *mode = requested;
            None
        }
        Some(_) | None => None,
    }
}

fn plug_picker_snapshot_for_mode(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    socket_type_override: Option<u16>,
    current_hash: Option<u64>,
    empty_label: Option<&str>,
    mode: PlugSelectionMode,
) -> crate::app::item_editor::PlugPickerSnapshot {
    let native_default = socket_type_override
        .filter(|socket_type| {
            item.sockets
                .get(socket_index)
                .is_none_or(|socket| socket.socket_type != *socket_type)
        })
        .map_or_else(
            || match item.default_plugs.get(socket_index) {
                Some(Some(hash)) => parse_hash_hex(hash).map(NativePlugDefault::Plug),
                Some(None) => Some(NativePlugDefault::Empty),
                None => None,
            },
            |_| None,
        );
    let current_label = current_hash.map_or_else(
        || empty_label.unwrap_or("None").to_owned(),
        |hash| catalog.plug_label(hash, true),
    );
    let Some(socket_type) = socket_type_override else {
        return plug_picker_snapshot(
            catalog,
            item,
            socket_index,
            current_hash,
            current_label,
            native_default,
            mode,
        );
    };
    let (choices, show_types) = crate::app::item_editor::plug_choices_for_socket_type(
        catalog,
        item,
        socket_index,
        Some(socket_type),
        mode,
    );
    crate::app::item_editor::PlugPickerSnapshot {
        preview: crate::app::item_editor::appearance::loadout(catalog, item.hash),
        preview_guarded: false,
        socket_index,
        socket_label: format!("Socket {} · type {socket_type}", socket_index + 1),
        current_hash,
        current_label,
        custom_current: false,
        native_default,
        native_default_label: native_default.and_then(|default| match default {
            crate::app::item_editor::NativePlugDefault::Plug(hash) => {
                Some(catalog.plug_label(hash, false))
            }
            crate::app::item_editor::NativePlugDefault::Empty => None,
        }),
        choices,
        show_types,
        mode,
        scope_labels: crate::app::item_editor::scope_labels(
            item,
            &catalog.socket_type_label_for_item(item, socket_type),
        ),
    }
}

/// A compact native icon-and-name catalog row.
pub(crate) fn draw_perk_row(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u32,
    name: &str,
    selected: bool,
    tooltip: crate::investment::PlugTooltip<'_>,
    icon: Option<crate::investment::IconOverride>,
) -> egui::Response {
    let height =
        ui.spacing().interact_size.y.max(
            ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y,
        );
    crate::app::item_editor::draw_compact_picker_row(
        ui,
        Some(catalog),
        crate::app::item_editor::CatalogPickerRow {
            hash: u64::from(hash),
            primary: name,
            primary_max_rows: 1,
            secondary: None,
            icon_size: height - 8.0,
            row_height: height,
            selected,
        },
        icon,
    )
    .on_hover_ui(|ui| {
        crate::app::item_editor::draw_item_tooltip_with_icon(
            ui,
            catalog,
            u64::from(hash),
            Some(tooltip),
            icon,
        );
    })
}

/// A perk in a list of perks, as a picker row: its icon, its name, and the first line of its
/// description under the name.
pub(crate) fn draw_perk_card_row(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u32,
    name: &str,
    selected: bool,
    tooltip: crate::investment::PlugTooltip<'_>,
    icon: Option<crate::investment::IconOverride>,
) -> egui::Response {
    let first_line = tooltip.description.and_then(|description| {
        description
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
    });
    crate::app::item_editor::draw_picker_row_with_icon(
        ui,
        Some(catalog),
        crate::app::item_editor::CatalogPickerRow {
            hash: u64::from(hash),
            primary: name,
            primary_max_rows: 1,
            secondary: first_line,
            icon_size: 32.0,
            row_height: crate::investment::authoring_choice_row_height(ui),
            selected,
        },
        icon,
    )
    .on_hover_ui(|ui| {
        crate::app::item_editor::draw_item_tooltip_with_icon(
            ui,
            catalog,
            u64::from(hash),
            Some(tooltip),
            icon,
        );
    })
}

/// Native icon with the same backdrop as the catalog controls.
pub(crate) fn draw_perk_icon(ui: &mut egui::Ui, catalog: &Catalog, hash: u32, size: f32) {
    if let Some(icon) = catalog.icon_texture(ui.ctx(), u64::from(hash)) {
        ui.add(
            egui::Image::new(&icon)
                .fit_to_exact_size(egui::vec2(size, size))
                .bg_fill(crate::app::ui::package_icon_backdrop(ui)),
        );
    } else {
        ui.allocate_space(egui::vec2(size, size));
    }
}
