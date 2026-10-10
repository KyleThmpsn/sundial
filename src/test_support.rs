/// The clean Shadowkeep `packages` directory the opt-in tests read, from `SUNDIAL_STOCK_PACKAGES`.
pub(crate) fn stock_packages() -> PathBuf {
    PathBuf::from(
        std::env::var_os("SUNDIAL_STOCK_PACKAGES")
            .expect("SUNDIAL_STOCK_PACKAGES must point to clean Shadowkeep packages"),
    )
}

/// A Destiny 2 install root whose `packages` may hold authored content, from `SUNDIAL_INSTALL`.
pub(crate) fn install() -> PathBuf {
    PathBuf::from(
        std::env::var_os("SUNDIAL_INSTALL").expect("SUNDIAL_INSTALL names a Destiny 2 install"),
    )
}

/// This test's own folder under `SUNDIAL_TEST_ARTIFACTS`, or `None` when no artifact root is set.
pub(crate) fn artifacts(name: &str) -> Option<PathBuf> {
    std::env::var_os("SUNDIAL_TEST_ARTIFACTS").map(|root| PathBuf::from(root).join(name))
}

/// This test's own folder under `SUNDIAL_TEST_ARTIFACTS`, for tests that cannot run without one.
pub(crate) fn artifact_dir(name: &str) -> PathBuf {
    artifacts(name).expect("SUNDIAL_TEST_ARTIFACTS names the artifact root")
}

/// The live install's `packages` the model preview suites read, from `SUNDIAL_PREVIEW_PACKAGES`.
pub(crate) fn preview_packages() -> PathBuf {
    PathBuf::from(
        std::env::var_os("SUNDIAL_PREVIEW_PACKAGES")
            .expect("SUNDIAL_PREVIEW_PACKAGES must point to a packages directory"),
    )
}

use std::{
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use eframe::egui;

use crate::investment::InvestmentCatalog;

static NEXT_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

/// Headless frame captures the layout tests write as meshes and textures.
pub mod capture;

/// Native verification runs on Rust's test thread, with the desktop backend selected normally.
#[cfg(all(test, any(windows, target_os = "linux")))]
pub(crate) fn native_event_loop<T: 'static>(builder: &mut winit::event_loop::EventLoopBuilder<T>) {
    #[cfg(windows)]
    use winit::platform::windows::EventLoopBuilderExtWindows;
    // Both Linux backends use this flag. The X11 extension does not force a backend.
    #[cfg(target_os = "linux")]
    use winit::platform::x11::EventLoopBuilderExtX11;
    builder.with_any_thread(true);
}

/// The pointer moving to `position`, then the primary button pressed or released there. A press
/// and its release go in separate frames, as a real click does.
pub(crate) fn primary_press(position: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

pub(crate) fn artifact(name: &str, value: &serde_json::Value) {
    let Some(directory) = std::env::var_os("SUNDIAL_TEST_ARTIFACTS") else {
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
