//! Recipe changes happen after controls have selected one command.
use super::super::{
    LogEntry, materialize_socket_column, recipe_socket_choices, reconcile_socket_plug_variants,
    set_recipe_socket_column, set_socket_role, shift_socket_choice_queries_after_removal,
};
use super::{RowChoices, RowCommand, RowContinuation, SocketRowContext};

pub(super) fn apply(
    context: &mut SocketRowContext<'_>,
    choices: &RowChoices,
    command: Option<RowCommand>,
) -> RowContinuation {
    match command {
        Some(RowCommand::ChangeRole(role)) => {
            set_socket_role(context.recipe, context.donor, context.socket_index, role);
            context.queries.clear();
            *context.page = 0;
            RowContinuation::Finished
        }
        Some(RowCommand::Reset) => reset(context, choices),
        Some(RowCommand::EditChoice { index, hash }) => edit_choice(context, choices, index, hash),
        Some(RowCommand::Activate) => {
            *context.show_technical_row = true;
            if !choices.is_overridden {
                materialize_socket_column(
                    context.recipe,
                    context.donor.sockets.len(),
                    context.socket_index,
                    &choices.inherited,
                    true,
                );
            }
            RowContinuation::TechnicalFields {
                scroll_to_header: true,
            }
        }
        None => RowContinuation::TechnicalFields {
            scroll_to_header: false,
        },
    }
}

fn reset(context: &mut SocketRowContext<'_>, choices: &RowChoices) -> RowContinuation {
    let recipe = &mut *context.recipe;
    let donor = context.donor;
    let socket = &donor.sockets[context.socket_index];
    let queries = &mut *context.queries;
    let inherited = &choices.inherited;
    if recipe.overrides.socket_columns.len() != donor.sockets.len() {
        recipe
            .overrides
            .socket_columns
            .resize_with(donor.sockets.len(), || None);
    }
    recipe.overrides.socket_columns[socket.index] = None;
    if recipe.overrides.socket_columns.iter().all(Option::is_none) {
        recipe.overrides.socket_columns.clear();
    }
    reconcile_socket_plug_variants(recipe, socket.index, inherited, None);
    queries.clear();
    RowContinuation::Finished
}

fn edit_choice(
    context: &mut SocketRowContext<'_>,
    choices: &RowChoices,
    choice_index: usize,
    hash: Option<u32>,
) -> RowContinuation {
    let recipe = &mut *context.recipe;
    let donor = context.donor;
    let socket = &donor.sockets[context.socket_index];
    let queries = &mut *context.queries;
    let inherited = &choices.inherited;
    let log = &mut *context.log;
    let catalog = context.catalog;
    let current = match recipe_socket_choices(recipe, socket.index, inherited) {
        Ok(current) => current,
        Err(error) => {
            log.push(LogEntry::error(format!(
                "{} contains an invalid plug hash: {error}",
                socket.label
            )));
            return RowContinuation::Finished;
        }
    };
    let Some(hash) = hash else {
        if choice_index == 0 {
            log.push(LogEntry::error(format!(
                "{} must retain a default plug",
                socket.label
            )));
        } else {
            let mut updated = current.clone();
            if choice_index < updated.len() {
                updated.remove(choice_index);
                set_recipe_socket_column(
                    recipe,
                    donor.sockets.len(),
                    socket.index,
                    inherited,
                    updated,
                    Some(choice_index),
                );
                shift_socket_choice_queries_after_removal(queries, choice_index);
            }
        }
        return RowContinuation::Finished;
    };
    let mut updated = current.clone();
    if updated
        .iter()
        .enumerate()
        .any(|(index, candidate)| index != choice_index && *candidate == hash)
    {
        log.push(LogEntry::error(format!(
            "{} already contains {}",
            socket.label,
            catalog.plug_label(hash, true)
        )));
        return RowContinuation::Finished;
    }
    if choice_index < updated.len() {
        updated[choice_index] = hash;
    } else {
        updated.push(hash);
    }
    set_recipe_socket_column(
        recipe,
        donor.sockets.len(),
        socket.index,
        inherited,
        updated,
        None,
    );
    RowContinuation::TechnicalFields {
        scroll_to_header: false,
    }
}
