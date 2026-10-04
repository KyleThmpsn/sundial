//! Shared installed-resource discovery. Inspection remains on demand.
use crate::package_runtime::reader::PackageManager;
use crate::{
    package_runtime::tft,
    sandbox_perk::{dependencies, entity, ingredients, program::properties},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};
mod assets;
pub use assets::{AssetChoice, technical_name};
pub mod behaviors;
pub mod conditions;
pub mod inspection;
pub mod scripts;
pub use crate::package_runtime::labels;

pub struct Catalog {
    pub names: Arc<tft::Index>,
    pub effects: Arc<entity::catalog::Catalog>,
    pub asset_choices: Vec<AssetChoice>,
    pub perks: Arc<dependencies::Index>,
    pub perk_assets: Vec<dependencies::content::PerkAssets>,
    /// Weapon items whose pattern entity names at least one asset.
    pub pattern_items: BTreeSet<u32>,
    pub abilities: Vec<ingredients::AbilitySource>,
    pub perk_search: BTreeMap<u16, String>,
    pub scripts: Arc<Vec<scripts::Choice>>,
}

/// Property keys can become usable even when the later discovery scan fails.
pub enum DiscoveryEvent {
    Phase(Phase),
    Progress(usize, usize),
    Keys(Result<Arc<properties::KeyIndex>, String>),
    Labels(Result<Arc<labels::Registry>, String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Packages,
    Labels,
    PropertyKeys,
    NativePaths,
    Perks,
    Objects,
    Abilities,
    Assembly,
}
impl Phase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Packages => "Opening Packages",
            Self::Labels => "Reading Labels",
            Self::PropertyKeys => "Reading Property Keys",
            Self::NativePaths => "Reading Native Paths",
            Self::Perks => "Reading Perk Programs",
            Self::Objects => "Reading Objects and Components",
            Self::Abilities => "Reading Abilities",
            Self::Assembly => "Assembling the Catalog",
        }
    }
}

/// Composes the existing caches. Worker lifetime and cancellation remain with the caller.
pub fn discover(packages: &Path, report: impl FnMut(DiscoveryEvent)) -> Result<Catalog, String> {
    discover_cancellable(packages, &AtomicBool::new(false), report)
}

pub fn discover_cancellable(
    packages: &Path,
    cancel: &AtomicBool,
    mut report: impl FnMut(DiscoveryEvent),
) -> Result<Catalog, String> {
    use crate::package_runtime::check_cancelled;
    check_cancelled(cancel)?;
    report(DiscoveryEvent::Phase(Phase::Packages));
    let existing_keys = properties::cached_only(packages).ok().flatten();
    let keys_ready = existing_keys.is_some();
    if let Some(keys) = existing_keys {
        report(DiscoveryEvent::Keys(Ok(keys)));
    }
    let manager = open_packages(packages)?;
    check_cancelled(cancel)?;
    report(DiscoveryEvent::Phase(Phase::Labels));
    report(DiscoveryEvent::Labels(
        labels::Registry::load(&manager).map(Arc::new),
    ));
    if !keys_ready {
        report(DiscoveryEvent::Phase(Phase::PropertyKeys));
        check_cancelled(cancel)?;
        let keys = properties::cached_cancellable(packages, &manager, cancel);
        check_cancelled(cancel)?;
        report(DiscoveryEvent::Keys(keys));
    }
    report(DiscoveryEvent::Phase(Phase::NativePaths));
    check_cancelled(cancel)?;
    let names = tft::cached_cancellable(packages, &manager, cancel, |current, total| {
        report(DiscoveryEvent::Progress(current, total))
    })?;
    report(DiscoveryEvent::Phase(Phase::Perks));
    check_cancelled(cancel)?;
    let perks = dependencies::cached_cancellable(packages, &manager, cancel, |current, total| {
        report(DiscoveryEvent::Progress(current, total))
    })?;
    report(DiscoveryEvent::Phase(Phase::Objects));
    check_cancelled(cancel)?;
    let effects = entity::catalog::cached_cancellable(packages, &manager, cancel)?;
    report(DiscoveryEvent::Phase(Phase::Assembly));
    check_cancelled(cancel)?;
    let asset_choices = assets::asset_choices(&effects);
    let pattern_items = effects
        .entries
        .iter()
        .flat_map(|entry| entry.contexts.iter().filter_map(|context| context.item))
        .collect();
    let perk_assets = dependencies::content::map(&perks, &names);
    report(DiscoveryEvent::Phase(Phase::Abilities));
    check_cancelled(cancel)?;
    let abilities = ingredients::abilities(&manager)?;
    report(DiscoveryEvent::Phase(Phase::Assembly));
    check_cancelled(cancel)?;
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
    let scripts = Arc::new(scripts::choices(&names));
    check_cancelled(cancel)?;
    Ok(Catalog {
        scripts,
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
