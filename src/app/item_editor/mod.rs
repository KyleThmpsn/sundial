//! Shared item editing controls used by equipment and inventory views.

mod catalog_picker;
mod context_menu;
mod definition_picker;
mod filters;
mod flags;
pub(crate) use context_menu::draw_context_menu;
pub(crate) use flags::{draw_seen_flag, draw_state_flags};
mod header;
mod layout;
mod model;
mod numeric;
mod plug_picker;

#[cfg(test)]
mod tests;

pub(crate) use super::components::{draw_lock_button, draw_trash_button, draw_unlock_button};
pub(crate) use catalog_picker::{
    catalog_item_tooltip, catalog_item_tooltip_available, catalog_item_tooltip_immediate,
    draw_catalog_item_tooltip,
};
pub(crate) use definition_picker::{
    draw_definition_picker_with_open_request, draw_definition_picker_with_open_request_and_footer,
    draw_definition_picker_with_open_request_and_item_filter,
};
pub(crate) use filters::{ItemFilter, ItemFilterScope, draw_item_filter_bar};
pub(crate) use header::{
    draw_catalog_item_header_with_trailing, draw_item_badge,
    draw_item_header_with_trailing_at_icon_size, muted_item_header_fill,
};
pub(crate) use layout::{draw_responsive_item_cards, draw_virtualized_responsive_item_cards};
pub(crate) use model::{
    ClearDefinitionChoice, DefinitionChoice, DefinitionPickerChoices, DefinitionSummary,
    ExistingInventoryChoice, ItemEditorAction, ItemHeader, NativePlugDefault, NumericItemFields,
    PickerHeight, PlugChoice, PlugPickerSnapshot,
};
#[cfg(test)]
use numeric::{authored_item_level, effective_power_input_max, item_power_input_max};
pub(crate) use numeric::{displayed_item_power, draw_level_and_quantity, new_inventory_item_level};
pub(crate) use plug_picker::{
    SOCKET_PICKER_RESET_WIDTH, draw_plug_icon_picker, draw_plug_icon_picker_with_footer,
    draw_plug_picker, draw_socket_picker_label, draw_socket_picker_reset, measured_button_width,
    plug_choices_for_socket, plug_choices_for_socket_type, plug_picker_snapshot,
    socket_picker_label_width, socket_picker_reset_width,
};

pub(crate) use catalog_picker::{
    CatalogPickerRow, catalog_button, draw_catalog_picker_row, draw_picker_row,
};
use catalog_picker::{picker_secondary_text, single_line_text};
#[cfg(test)]
use layout::responsive_item_card_layout;
use layout::{picker_list_height, popup_direction, spaced_picker_list_height};

use std::hash::Hash;

use eframe::egui;

use crate::{
    catalog::{Catalog, CatalogSearchQuery, ItemDef},
    hash::format_hash_hex,
};

use super::{
    PlugSelectionMode,
    inspector::{
        DefinitionInspectionContext, request_definition as request_hash_inspection,
        request_definition_with_context as request_hash_inspection_with_context,
    },
    ui::single_line_galley,
};
