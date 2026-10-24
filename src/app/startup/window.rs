//! The window size and place eframe saves, kept from the last save while the window showed.
//!
//! eframe saves the root window's settings every 30 seconds and on exit, whether or not the window
//! is minimized. Windows reports a minimized window's client area as 0 by 0, so a save while
//! minimized replaced the last size with nothing, and the next launch opened at the minimum size.
//! A save that finds such a size puts back the settings from the last save while the window showed.
use eframe::egui;
use serde::Deserialize;

/// eframe's storage key for the root window's settings.
const KEY: &str = "window";
/// The smallest size eframe restores. A saved size below it is a minimized window's.
const SMALLEST: f32 = 64.0;

/// The one field of eframe's saved window settings that tells a minimized window apart.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Saved {
    inner_size_points: Option<egui::Vec2>,
}

/// The window settings from the last save while the window showed.
#[derive(Default)]
pub(super) struct Kept(Option<String>);

impl Kept {
    /// The settings the last session saved, when its window was showing then.
    pub(super) fn loaded(storage: Option<&dyn eframe::Storage>) -> Self {
        let mut kept = Self::default();
        if let Some(storage) = storage {
            kept.take(storage);
        }
        kept
    }

    /// Keeps the settings eframe just saved, or puts the kept ones back over a minimized window's.
    /// Without kept settings, the next launch opens at the default size.
    pub(super) fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.take(storage) {
            return;
        }
        match &self.0 {
            Some(settings) => storage.set_string(KEY, settings.clone()),
            None => storage.remove_string(KEY),
        }
    }

    /// Keeps the saved settings unless they hold a minimized window's size, and returns whether
    /// they were kept. Settings this build cannot read are kept as eframe wrote them.
    fn take(&mut self, storage: &dyn eframe::Storage) -> bool {
        let Some(settings) = storage.get_string(KEY) else {
            return true;
        };
        let minimized = eframe::get_value::<Saved>(storage, KEY)
            .and_then(|saved| saved.inner_size_points)
            .is_some_and(|size| size.min_elem() < SMALLEST);
        if !minimized {
            self.0 = Some(settings);
        }
        !minimized
    }
}
