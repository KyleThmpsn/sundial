//! Bank names and parameter listings from the ability tables, once a caller registers them.
use super::*;

/// The game's names for the stock banks, registered from the ability tables by a caller that
/// has them: the ability whose entity's script reads the bank, or the class and slot when
/// several abilities share it.
static BANK_NAMES: std::sync::RwLock<BTreeMap<u32, String>> =
    std::sync::RwLock::new(BTreeMap::new());
/// The abilities whose banks list a parameter, by slot and parameter, from the same tables.
static PARAMETER_ABILITIES: std::sync::RwLock<BTreeMap<(AbilityTarget, u32), Vec<String>>> =
    std::sync::RwLock::new(BTreeMap::new());

/// Replaces the registered bank names and parameter listings.
pub fn register_bank_names(
    names: BTreeMap<u32, String>,
    listings: BTreeMap<(AbilityTarget, u32), Vec<String>>,
) {
    if let Ok(mut current) = BANK_NAMES.write() {
        *current = names;
    }
    if let Ok(mut current) = PARAMETER_ABILITIES.write() {
        *current = listings;
    }
}

/// The game's name for a bank, once registered.
pub fn bank_name(bank: u32) -> Option<String> {
    BANK_NAMES.read().ok()?.get(&bank).cloned()
}

/// Every registered bank name, by tag.
pub fn bank_names() -> BTreeMap<u32, String> {
    BANK_NAMES
        .read()
        .map(|names| names.clone())
        .unwrap_or_default()
}

/// The abilities whose banks list `parameter` on `slot`, once registered.
pub fn parameter_abilities(slot: AbilityTarget, parameter: u32) -> Vec<String> {
    PARAMETER_ABILITIES
        .read()
        .ok()
        .and_then(|listings| listings.get(&(slot, parameter)).cloned())
        .unwrap_or_default()
}
