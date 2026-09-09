//! Package-backed stand-ins used by the Panoptes presentation.
//!
//! These hashes point at definitions in the installed Shadowkeep packages.
//! Sundial never embeds or redistributes their textures; `Catalog::icon_texture`
//! resolves and composites them lazily through tiger-pkg at runtime.

use eframe::egui;

use crate::catalog::Catalog;

/// The game's own empty mod-socket plate, used only for unoccupied mod sockets.
const EMPTY_MOD_SOCKET: u32 = 0x1CB5_C883;
fn empty_mod(catalog: &Catalog, context: &egui::Context) -> Option<egui::TextureHandle> {
    catalog.icon_texture(context, u64::from(EMPTY_MOD_SOCKET))
}

pub(super) fn socket(
    catalog: &Catalog,
    context: &egui::Context,
    hash: Option<u64>,
    empty_is_mod: bool,
) -> Option<egui::TextureHandle> {
    match hash {
        None if empty_is_mod => empty_mod(catalog, context),
        None => None,
        Some(hash) => catalog.icon_texture(context, hash),
    }
}

pub(super) fn gear(
    catalog: &Catalog,
    context: &egui::Context,
    hash: Option<u64>,
) -> Option<egui::TextureHandle> {
    hash.and_then(|hash| catalog.icon_texture(context, hash))
}
