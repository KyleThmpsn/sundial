use std::{
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::investment::InvestmentCatalog;

static NEXT_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) fn artifact(name: &str, value: &serde_json::Value) {
    let Some(directory) = std::env::var_os("PARHELION_TEST_ARTIFACTS") else {
        return;
    };
    let directory = PathBuf::from(directory);
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join(name),
        serde_json::to_vec_pretty(value).unwrap(),
    )
    .unwrap();
}

pub(crate) struct TestDirectory(pub(crate) PathBuf);

impl TestDirectory {
    pub(crate) fn new(purpose: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sundial-{purpose}-test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock must be after the Unix epoch")
                .as_nanos(),
            NEXT_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("test directory should be created");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Loads `install`'s catalog through a cache of the tests' own. A test never replaces the app's
/// shared cache, and each package set is scanned once rather than on every run.
pub(crate) fn catalog(install: &Path) -> Result<InvestmentCatalog, String> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    install.hash(&mut hasher);
    let cache = std::env::temp_dir()
        .join("sundial-test-catalogs")
        .join(format!("{:016x}.json", hasher.finish()));
    InvestmentCatalog::load_with_cache_path(install, &cache, false, |_| {})
}
