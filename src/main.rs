#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod catalog;
mod class_items;
mod dummy_items;
mod game_settings;
mod storage;
mod unnamed_plugs;
mod updates;

fn main() -> eframe::Result<()> {
    app::run()
}
