//! Account unlock synchronization. The app supplies JSON save and backup policy.
use crate::catalog::UnlockDefinition;
use std::path::{Path, PathBuf};
pub(crate) fn synchronize_authored_collection_unlocks_at(
    settings_path: &Path,
    unlocks: &[(usize, u8, u16)],
    save: impl FnOnce(&Path, &serde_json::Value, &serde_json::Value) -> Result<PathBuf, String>,
) -> Result<(PathBuf, Option<PathBuf>, usize), String> {
    let original = crate::persistence::json_document::load_workspace_json(settings_path)?;
    let database_path = crate::persistence::investment_path(settings_path);
    if crate::game_settings::requires_sqlite_account(&original) {
        {
            use crate::persistence::sqlite_account::{self, SqliteAccountDocumentLoad};
            let mut document =
                match sqlite_account::load_document(&database_path).map_err(|e| e.to_string())? {
                    SqliteAccountDocumentLoad::Loaded(document) => document,
                    _ => return Err(
                        "A compatible Sunrise investment database is required for authored unlocks"
                            .into(),
                    ),
                };
            let changed = apply_native_authored_unlocks(&mut document, unlocks)?;
            let backup = if changed == 0 {
                None
            } else {
                Some(
                    sqlite_account::save_document(&mut document)
                        .map_err(|e| e.to_string())?
                        .backup,
                )
            };
            return Ok((database_path, backup, changed));
        }
    }
    synchronize_loaded_authored_collection_unlocks(settings_path, original, unlocks, save)
}

fn apply_native_authored_unlocks(
    document: &mut crate::persistence::sqlite_account::SqliteAccountDocument,
    unlocks: &[(usize, u8, u16)],
) -> Result<usize, String> {
    let mut changed = 0;
    for unlock in unlocks {
        let count = if matches!(unlock.1, 3 | 6) {
            document.characters().characters().len()
        } else {
            1
        };
        let mut definition_changed = false;
        for index in 0..count {
            let (view, edits) = authored_unlock_changes(
                document.progression_view(index),
                std::slice::from_ref(unlock),
            )?;
            if edits != 0 {
                document
                    .apply_progression_view(index, &view)
                    .map_err(|error| error.to_string())?;
                definition_changed = true;
            }
        }
        changed += usize::from(definition_changed);
    }
    Ok(changed)
}

fn synchronize_loaded_authored_collection_unlocks(
    settings_path: &Path,
    original: serde_json::Value,
    unlocks: &[(usize, u8, u16)],
    save: impl FnOnce(&Path, &serde_json::Value, &serde_json::Value) -> Result<PathBuf, String>,
) -> Result<(PathBuf, Option<PathBuf>, usize), String> {
    let (document, changed) = authored_unlock_changes(original.clone(), unlocks)?;
    let backup = if changed == 0 {
        None
    } else {
        // Refuse to overwrite edits made by Sunrise, Sundial, or the user while the authored
        // package installation was finishing.
        crate::persistence::json_document::verify_unchanged(settings_path, &original)?;
        Some(save(settings_path, &document, &original)?)
    };
    Ok((settings_path.to_path_buf(), backup, changed))
}

fn authored_unlock_changes(
    original: serde_json::Value,
    unlocks: &[(usize, u8, u16)],
) -> Result<(serde_json::Value, usize), String> {
    let mut document = original;
    let mut changed = 0usize;
    for &(definition_index, bank, slot) in unlocks {
        if bank == crate::account_contract::SHADOWKEEP_ACCOUNT_FLAG_BANK
            && usize::from(slot) >= crate::account_contract::SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY
        {
            return Err(format!(
                "Authored unlock definition {definition_index} uses account slot {slot}, beyond the extended Shadowkeep account-flag region"
            ));
        }
        let definition = UnlockDefinition {
            hash: 0,
            code: u16::from(bank),
            compact_slot: Some(slot),
            name: None,
            description: None,
            runtime_writers: Vec::new(),
            tested_by: Vec::new(),
        };
        let state = crate::persistence::progression::collection_state_snapshot(&document)
            .ok_or("The active settings.json does not expose a supported unlock-state layout")?;
        match state.flag_value(definition_index, &definition) {
            Some(true) => continue,
            Some(false) => {}
            None => {
                return Err(format!(
                    "Authored unlock definition {definition_index} uses unsupported bank {bank}"
                ));
            }
        }
        if !crate::persistence::progression::mutations::set_collection_flag(
            &mut document,
            definition_index,
            &definition,
            true,
        ) {
            return Err(format!(
                "Could not set authored unlock definition {definition_index} at bank {bank}, slot {slot}"
            ));
        }
        changed += 1;
    }
    Ok((document, changed))
}

#[cfg(test)]
mod tests;
