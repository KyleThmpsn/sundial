#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod catalog;
mod class_items;
mod dummy_items;
mod game_settings;
mod generated_file;
mod hash;
mod http;
mod orbit_map;
mod package_runtime;
mod paths;
mod persistence;
mod storage;
#[cfg(test)]
mod test_support;
mod unnamed_plugs;
mod updates;

fn main() -> eframe::Result<()> {
    app::run()
}
