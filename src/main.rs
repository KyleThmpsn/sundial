#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod catalog;
mod catalyst_plugs;
mod class_items;
mod dummy_items;
mod game_settings;
mod hash;
mod storage;
#[cfg(test)]
mod test_support;
mod unnamed_plugs;
mod updates;

fn main() -> eframe::Result<()> {
    app::run()
}
