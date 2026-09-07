//! Settings boundaries: discovery, installed defaults, validation, encoding, and persistence.
mod defaults;
mod encoding;
mod paths;
mod persistence;
mod validation;

pub(super) use super::preferences::store::{load_preferences, preferences_path};
#[cfg(test)]
pub(super) use crate::package_runtime::normalize_sunrise_version;
pub(super) use defaults::{detect_sunrise_version, load_installed_sunrise_defaults};
#[cfg(test)]
pub(super) use encoding::settings_size_limit_for_schema;
pub(super) use encoding::{encode_settings, prepare_settings};
pub(super) use paths::{
    backups_path, catalog_path, missing_settings_message, resolve_settings_path,
    settings_path_for_install,
};
pub(super) use persistence::{
    SaveJsonError, SaveJsonResult, create_adjacent_backup, load_workspace_json,
    require_game_closed, save_json, verify_workspace_source_unchanged,
};
#[cfg(test)]
pub(super) use persistence::{
    load_json, save_json_with_backup_root, save_test_json_checked, verify_source_unchanged,
};
pub(super) use validation::{
    character_ability_issue, character_ability_issue_for_values, repair_known_ability_pairs,
    validate_document, validate_workspace_document,
};

#[cfg(test)]
pub(super) use validation::validate_characters;
