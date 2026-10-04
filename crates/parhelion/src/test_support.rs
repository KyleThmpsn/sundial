//! Helpers the tests share.
use std::{
    hash::{Hash, Hasher},
    path::Path,
};

pub(crate) mod driver;

use sundial::investment::InvestmentCatalog;

/// Optional receipts from filesystem and worker workflows, alongside the UI captures.
pub(crate) fn artifact(name: &str, value: &impl serde::Serialize) {
    let Some(directory) = std::env::var_os("PARHELION_TEST_ARTIFACTS") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(name),
        serde_json::to_vec_pretty(value).unwrap(),
    )
    .unwrap();
}

/// Loads `install`'s catalog through a cache of the tests' own. A test never replaces the app's
/// shared cache, and each package set is scanned once rather than on every run. Sundial's tests
/// use the same folder.
pub(crate) fn catalog(install: &Path) -> Result<InvestmentCatalog, String> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    install.hash(&mut hasher);
    let cache = std::env::temp_dir()
        .join("sundial-test-catalogs")
        .join(format!("{:016x}.json", hasher.finish()));
    InvestmentCatalog::load_with_cache_path(install, &cache, false, |_| {})
}
