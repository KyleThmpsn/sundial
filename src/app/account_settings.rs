//! Application service for storage-neutral account-setting commands.

use serde_json::Value;
use sundial_account::AccountSettingsCommand;

use crate::persistence::json_account::JsonAccountSettingsAdapter;

pub(super) fn apply_commands(
    document: &mut Value,
    commands: Vec<AccountSettingsCommand>,
) -> Result<bool, String> {
    if commands.is_empty() {
        return Ok(false);
    }
    let adapter = JsonAccountSettingsAdapter::load_for_commands(document, &commands)
        .map_err(|error| error.to_string())?;
    let (_, candidate, changed) = adapter
        .apply(document, commands)
        .map_err(|error| error.to_string())?;
    if changed {
        *document = candidate;
    }
    Ok(changed)
}

#[cfg(test)]
mod legacy;

#[cfg(test)]
mod tests;
