//! Application service for storage-neutral character-field commands.

use serde_json::Value;
use sundial_account::{CharacterCommand, CharacterMetadataUpdate};

use crate::persistence::json_account::JsonCharacterAdapter;

#[cfg(test)]
mod legacy;
#[cfg(test)]
mod tests;

pub(super) fn apply_updates(
    document: &mut Value,
    character_index: usize,
    updates: Vec<CharacterMetadataUpdate>,
) -> Result<bool, String> {
    if updates.is_empty() {
        return Ok(false);
    }
    let adapter = JsonCharacterAdapter::load_character_metadata(document, character_index)
        .map_err(|error| error.to_string())?;
    let character_id = adapter
        .character_id_at_index(character_index)
        .ok_or_else(|| format!("Character {} does not exist", character_index + 1))?;
    let mut commands = updates
        .into_iter()
        .map(|update| CharacterCommand::UpdateMetadata {
            character_id,
            update,
        });
    let first = commands
        .next()
        .expect("non-empty character metadata updates have a first command");
    let command = match commands.next() {
        None => first,
        Some(second) => CharacterCommand::Batch(
            std::iter::once(first)
                .chain(std::iter::once(second))
                .chain(commands)
                .collect(),
        ),
    };
    let (_, candidate, _) = adapter
        .apply(document, command)
        .map_err(|error| error.to_string())?;
    let changed = candidate != *document;
    if changed {
        *document = candidate;
    }
    Ok(changed)
}
