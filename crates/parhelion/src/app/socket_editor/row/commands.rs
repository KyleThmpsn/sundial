//! Recipe changes happen after controls have selected one command.
use super::super::{
    LogEntry, materialize_socket_column, recipe_socket_choices, reconcile_socket_plug_variants,
    remove_base_socket, remove_last_added_socket, set_recipe_socket_column, set_socket_role,
    shift_socket_choice_queries_after_removal,
};
use super::{RowChoices, RowCommand, RowContinuation, SocketRowContext};
#[cfg(test)]
mod tests;

pub(super) fn apply(
    context: &mut SocketRowContext<'_>,
    choices: &RowChoices,
    command: Option<RowCommand>,
) -> RowContinuation {
    match command {
        Some(RowCommand::ChangeRole(role)) => {
            if context.is_added && role.is_none() {
                return RowContinuation::Finished;
            }
            set_socket_role(context.recipe, context.donor, context.socket_index, role);
            context.queries.clear();
            *context.page = 0;
            RowContinuation::Finished
        }
        Some(RowCommand::Reset) => reset(context, choices),
        Some(RowCommand::Remove) => {
            let removed = if context.is_added {
                context.can_remove_added
                    && remove_last_added_socket(context.recipe, context.socket_index)
            } else {
                remove_base_socket(
                    context.recipe,
                    context.donor.sockets.len(),
                    context.socket_index,
                );
                true
            };
            if removed {
                if *context.private_perk_socket == Some(context.socket_index) {
                    *context.private_perk_socket = None;
                }
                context.queries.clear();
                *context.page = 0;
            }
            RowContinuation::Finished
        }
        Some(RowCommand::EditChoice { index, hash }) => edit_choice(context, choices, index, hash),
        Some(RowCommand::MakeDefault(index)) => {
            match super::super::make_choice_default(
                context.recipe,
                context.donor.sockets.len(),
                context.socket_index,
                &choices.inherited,
                index,
            ) {
                Ok(()) => {
                    context.queries.clear();
                    *context.page = 0;
                }
                Err(error) => context.log.push(LogEntry::error(error)),
            }
            RowContinuation::Finished
        }
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
    if context.is_added {
        return RowContinuation::Finished;
    }
    let recipe = &mut *context.recipe;
    let donor = context.donor;
    let socket = &donor.sockets[context.socket_index];
    let queries = &mut *context.queries;
    let inherited = &choices.inherited;
    if recipe.overrides.socket_columns.len() < donor.sockets.len() {
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
    let private = match super::super::super::custom_perks::resolve_picked_perk(
        context.recipe_library,
        Some(recipe),
        catalog,
        hash,
    ) {
        Ok(private) => private,
        Err(error) => {
            log.push(LogEntry::error(error));
            return RowContinuation::Finished;
        }
    };
    let hash = match private
        .as_ref()
        .map(|variant| variant.source_plug_hash.parse_u32())
        .transpose()
    {
        Ok(source) => source.unwrap_or(hash),
        Err(error) => {
            log.push(LogEntry::error(error.to_string()));
            return RowContinuation::Finished;
        }
    };
    let mut updated = current.clone();
    if super::super::super::custom_perks::choice_conflicts(
        recipe,
        socket.index,
        choice_index,
        hash,
        private.as_ref(),
        &updated,
    ) {
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
    recipe.overrides.socket_plug_variants.retain(|variant| {
        usize::from(variant.socket_index) != socket.index
            || usize::from(variant.choice_index) != choice_index
    });
    set_recipe_socket_column(
        recipe,
        donor.sockets.len(),
        socket.index,
        inherited,
        updated,
        None,
    );
    if let Some(private) = private {
        super::super::super::custom_perks::attach_picked_perk(
            recipe,
            socket.index,
            choice_index,
            private,
        );
    }
    RowContinuation::TechnicalFields {
        scroll_to_header: false,
    }
}
