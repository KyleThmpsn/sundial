//! Shared native-content queries. Discovery is optional and inspection is always on demand.
use crate::{
    package_runtime::tft,
    sandbox_perk::{dependencies, ingredients, program::properties, projectile},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};
use tiger_pkg::PackageManager;
mod assets;
pub use assets::{AssetChoice, technical_name};
pub mod inspection;

pub struct Catalog {
    pub names: Arc<tft::Index>,
    pub effects: Arc<projectile::catalog::Catalog>,
    pub asset_choices: Vec<AssetChoice>,
    pub perks: Arc<dependencies::Index>,
    pub perk_assets: Vec<dependencies::content::PerkAssets>,
    /// Weapon items whose pattern entity names at least one asset.
    pub pattern_items: BTreeSet<u32>,
    pub abilities: Vec<ingredients::AbilitySource>,
    pub perk_search: BTreeMap<u16, String>,
}

/// Property keys can become usable even when the later discovery scan fails.
pub enum DiscoveryEvent {
    Progress(usize, usize),
    Keys(Result<Arc<properties::KeyIndex>, String>),
}

/// Composes the existing caches. Worker lifetime and cancellation remain with the caller.
pub fn discover(
    packages: &Path,
    mut report: impl FnMut(DiscoveryEvent),
) -> Result<Catalog, String> {
    let existing_keys = properties::cached_only(packages).ok().flatten();
    let keys_ready = existing_keys.is_some();
    if let Some(keys) = existing_keys {
        report(DiscoveryEvent::Keys(Ok(keys)));
    }
    let manager = open_packages(packages)?;
    if !keys_ready {
        report(DiscoveryEvent::Keys(properties::cached(packages, &manager)));
    }
    let names = tft::cached(packages, &manager, |current, total| {
        report(DiscoveryEvent::Progress(current, total))
    })?;
    let perks = dependencies::cached(packages, &manager, |_, _| {})?;
    let effects = projectile::catalog::cached(packages, &manager)?;
    let asset_choices = assets::asset_choices(&effects);
    let pattern_items = effects
        .entries
        .iter()
        .flat_map(|entry| entry.contexts.iter().filter_map(|context| context.item))
        .collect();
    let perk_assets = dependencies::content::map(&perks, &names);
    let abilities = ingredients::abilities(&manager)?;
    let perk_search = perk_assets
        .iter()
        .filter_map(|assets| {
            let index = u16::try_from(assets.perk_index).ok()?;
            Some((
                index,
                assets
                    .references()
                    .map(|index| names.references[index].path.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_ascii_lowercase(),
            ))
        })
        .collect();
    Ok(Catalog {
        names,
        effects,
        asset_choices,
        perks,
        perk_assets,
        pattern_items,
        abilities,
        perk_search,
    })
}

/// Opens a Shadowkeep package directory through Sundial's cross-platform runtime.
///
/// The directory may be the installed package set or an isolated authoring view whose parent is
/// laid out like a Shadowkeep installation.
pub fn open_packages(packages: &Path) -> Result<PackageManager, String> {
    let install = packages.parent().ok_or_else(|| {
        format!(
            "Packages directory has no install root: {}",
            packages.display()
        )
    })?;
    crate::package_runtime::open_shadowkeep_packages(install)
}

pub mod kinds;
