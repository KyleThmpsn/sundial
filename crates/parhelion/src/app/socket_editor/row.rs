//! Row state, command dispatch, and optional technical controls.
use super::{
    ActivityLog, HexHash, InvestmentCatalog, PlugSelectionMode, SOCKET_CHOICE_PAGE_SIZE,
    SocketTechnicalFields, WeaponDonor, WeaponRecipe, authored_socket_choice_limit,
    draw_socket_technical_fields, inherited_socket_choices,
};
use std::collections::BTreeMap;
mod commands;
mod controls;

pub(super) struct SocketRowContext<'a> {
    pub catalog: &'a InvestmentCatalog,
    pub recipe: &'a mut WeaponRecipe,
    pub queries: &'a mut BTreeMap<usize, String>,
    pub page: &'a mut usize,
    pub plug_selection_mode: PlugSelectionMode,
    pub donor: &'a WeaponDonor,
    pub socket_index: usize,
    pub is_added: bool,
    pub can_remove_added: bool,
    pub show_experimental_options: bool,
    pub show_technical_row: &'a mut bool,
    pub private_perk_socket: &'a mut Option<usize>,
    pub log: &'a mut ActivityLog,
}

struct RowChoices {
    socket_type_override: Option<u16>,
    max_authored_choices: usize,
    inherited: Vec<u32>,
    current_len: usize,
    is_overridden: bool,
    page_count: usize,
    page_start: usize,
    page_end: usize,
    current_page: Vec<u32>,
    can_add: bool,
}

impl RowChoices {
    fn read(context: &mut SocketRowContext<'_>) -> Result<Self, String> {
        let recipe = &*context.recipe;
        let donor = context.donor;
        let socket_index = context.socket_index;
        let page = &mut *context.page;
        let socket = &donor.sockets[socket_index];
        let socket_type_override = recipe
            .overrides
            .socket_columns
            .get(socket.index)
            .and_then(Option::as_ref)
            .and_then(|column| column.socket_type);
        let effective_socket_type = socket_type_override.unwrap_or(socket.socket_type);
        let max_authored_choices = authored_socket_choice_limit(effective_socket_type);
        let inherited = inherited_socket_choices(
            socket.native_default,
            &socket.ordered_embedded_choices,
            max_authored_choices,
        );
        let current_override = recipe
            .overrides
            .socket_columns
            .get(socket.index)
            .and_then(Option::as_ref);
        let current_len = current_override.map_or(inherited.len(), |column| column.choices.len());
        let is_overridden = current_override.is_some();
        let page_count = current_len.max(1).div_ceil(SOCKET_CHOICE_PAGE_SIZE);
        *page = (*page).min(page_count - 1);
        let page_start = *page * SOCKET_CHOICE_PAGE_SIZE;
        let page_end = (page_start + SOCKET_CHOICE_PAGE_SIZE).min(current_len);
        let current_page = match current_override {
            Some(column) => column.choices[page_start..page_end]
                .iter()
                .map(HexHash::parse_u32)
                .collect::<Result<Vec<_>, _>>(),
            None => Ok(inherited[page_start..page_end].to_vec()),
        };
        let current_page = current_page
            .map_err(|error| format!("{} contains an invalid plug hash: {error}", socket.label))?;
        let can_add = current_len < max_authored_choices && page_end == current_len;
        Ok(Self {
            socket_type_override,
            max_authored_choices,
            inherited,
            current_len,
            is_overridden,
            page_count,
            page_start,
            page_end,
            current_page,
            can_add,
        })
    }
}

enum RowCommand {
    ChangeRole(Option<u16>),
    Reset,
    RemoveAdded,
    Activate,
    EditChoice { index: usize, hash: Option<u32> },
}

enum RowContinuation {
    Finished,
    TechnicalFields { scroll_to_header: bool },
}

pub(super) fn draw_socket_picker_row(ui: &mut egui::Ui, mut context: SocketRowContext<'_>) {
    let choices = match RowChoices::read(&mut context) {
        Ok(choices) => choices,
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            return;
        }
    };
    let command = if choices.max_authored_choices == 0 {
        controls::draw_disabled(ui, &mut context, &choices)
    } else {
        controls::draw_active(ui, &mut context, &choices)
    };
    let RowContinuation::TechnicalFields { scroll_to_header } =
        commands::apply(&mut context, &choices, command)
    else {
        return;
    };
    if context.show_experimental_options && *context.show_technical_row {
        draw_socket_technical_fields(
            ui,
            SocketTechnicalFields {
                catalog: context.catalog,
                recipe: context.recipe,
                donor: context.donor,
                socket_index: context.socket_index,
                is_added: context.is_added,
                inherited: &choices.inherited,
                page: context.page,
                queries: context.queries,
                scroll_to_header,
            },
        );
    }
}
