//! Locating settings and shared application data without creating files.

#[cfg(test)]
mod tests;

pub(in crate::app) use crate::backups::root as backups_path;
pub(in crate::app) use crate::paths::shadowkeep_catalog_path as catalog_path;

pub(in crate::app) use crate::account::source::{
    missing_settings_message, resolve_settings_path, settings_path_for_install,
};
