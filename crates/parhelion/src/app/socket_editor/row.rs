//! Row state, command dispatch, and optional technical controls.
use super::{
    ActivityLog, HexHash, InvestmentCatalog, PlugSelectionMode, SOCKET_CHOICE_PAGE_SIZE,
    SocketTechnicalFields, SocketView, WeaponDonor, WeaponRecipe, authored_socket_choice_limit,
    base_roll, draw_socket_technical_fields, inherited_socket_choices,
};
use std::collections::BTreeMap;
mod commands;
mod controls;

pub(super) struct SocketRowContext<'a> {
    pub catalog: &'a InvestmentCatalog,
    /// The recipe's preview loadout, computed once per frame for every choice picker.
    pub preview: Option<&'a sundial::ui::model_preview::Loadout>,
    pub recipe_library: Option<&'a crate::RecipeLibrary>,
    pub recipe: &'a mut WeaponRecipe,
    pub queries: &'a mut BTreeMap<usize, String>,
    pub page: &'a mut usize,
    /// The plugs every picker offers, which each picker's Plugs Offered dropdown changes.
    pub plug_selection_mode: &'a mut PlugSelectionMode,
    pub donor: &'a WeaponDonor,
    pub socket_index: usize,
    pub is_added: bool,
    pub can_remove_added: bool,
    pub show_experimental_options: bool,
    pub show_technical_row: &'a mut bool,
    pub view: SocketView,
    pub perk_request: &'a mut Option<crate::app::custom_perks::workbench::Request>,
    pub log: &'a mut ActivityLog,
}

/// The choices a row shows, which are those of its view: its reusable plug items, or every choice
/// in Randomized, since a lane that rolls from its choices rolls its reusable ones too. Both
/// lists start at the column's first choice, so their indices are the column's.
struct RowChoices {
    socket_type_override: Option<u16>,
    max_authored_choices: usize,
    inherited: Vec<u32>,
    current_len: usize,
    /// How many leading choices are reusable plug items. The rest are only in the column's own
    /// randomized plug set, which holds every choice.
    reusable: usize,
    /// Whether the column can roll from its own choices: a weapon column that names no stock
    /// randomized set and fits one.
    can_roll: bool,
    stock_roll: Option<StockRoll>,
    is_overridden: bool,
    view: SocketView,
    /// How many choices the view shows.
    shown_len: usize,
    page_count: usize,
    page_start: usize,
    page_end: usize,
    current_page: Vec<u32>,
    can_add: bool,
}

/// The base's randomized plug set on a lane that does not roll from its own choices.
struct StockRoll {
    /// Its members that are not among the lane's choices, in native order.
    randomized: Vec<u32>,
    /// Whether the build keeps the set. A weapon build, or a gear build with edited sockets,
    /// replaces it with the lane's reusable plug items.
    kept: bool,
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
        let current = match current_override {
            Some(column) => column
                .choices
                .iter()
                .map(HexHash::parse_u32)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| {
                    format!("{} contains an invalid plug hash: {error}", socket.label)
                })?,
            None => inherited.clone(),
        };
        let current_len = current.len();
        let is_overridden = current_override.is_some();
        let lead = current_override.and_then(|column| column.reusable_choices);
        let reusable = lead.map_or(current_len, |lead| usize::from(lead).min(current_len));
        let stock_roll = base_roll(recipe, socket).map(|_| StockRoll {
            randomized: socket
                .ordered_randomized_choices
                .iter()
                .copied()
                .filter(|plug| !current.contains(plug))
                .collect(),
            kept: !is_overridden
                && !recipe.kind.is_weapon()
                && recipe.overrides.socket_columns.is_empty(),
        });
        let can_roll = recipe.kind.is_weapon()
            && current_override.is_none_or(|column| column.randomized_plug_set_index.is_none())
            && current_len <= crate::item::MAX_ROLLED_CHOICES;
        let view = context.view;
        let shown_len = match view {
            SocketView::Reusable => reusable,
            SocketView::Randomized => current_len,
        };
        let page_count = shown_len.max(1).div_ceil(SOCKET_CHOICE_PAGE_SIZE);
        *page = (*page).min(page_count - 1);
        let page_start = *page * SOCKET_CHOICE_PAGE_SIZE;
        let page_end = (page_start + SOCKET_CHOICE_PAGE_SIZE).min(shown_len);
        let current_page = current[page_start..page_end].to_vec();
        // A randomized choice joins a roll, which needs a reusable default and fits 64 choices.
        let rolls_after = lead.is_some() || view == SocketView::Randomized;
        let fits = current_len < max_authored_choices
            && (!rolls_after || current_len < crate::item::MAX_ROLLED_CHOICES);
        let can_add = fits
            && page_end == shown_len
            && (view == SocketView::Reusable || (can_roll && current_len > 0));
        Ok(Self {
            socket_type_override,
            max_authored_choices,
            inherited,
            current_len,
            reusable,
            can_roll,
            stock_roll,
            is_overridden,
            view,
            shown_len,
            page_count,
            page_start,
            page_end,
            current_page,
            can_add,
        })
    }

    /// Whether the column rolls from its own choices, which leaves some of them randomized alone.
    fn rolls(&self) -> bool {
        self.reusable < self.current_len
    }
}

enum RowCommand {
    ChangeRole(Option<u16>),
    Reset,
    Remove,
    Activate,
    EditChoice { index: usize, hash: Option<u32> },
    MakeDefault(usize),
    MoveChoice { from: usize, to: usize },
    SetReusable { index: usize, reusable: bool },
    RollFromBase,
    EditPerk(usize),
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
