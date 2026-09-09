//! Package reads stay off the UI thread and reuse the same decoders as Parhelion.

use std::path::Path;

use crate::{
    investment_schema::{GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT, investment_globals_table_tag},
    package_runtime::{open_shadowkeep_packages, resolve_live_named_tag},
    sandbox_perk::{
        FinishedSandboxPerk, finished_sandbox_perk_at, load_sandbox_perk_runtime_action,
        validate_finished_sandbox_perk_catalog,
    },
    weapon_runtime::{
        WeaponRuntimeGraph, load_weapon_runtime_entity_at_pattern_index_with_manager,
        load_weapon_runtime_graph_for_entity,
    },
};
use tiger_pkg::{PackageManager, TagHash};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum RuntimeTarget {
    Weapon(u16),
    Perk(u16),
    Dye(u16),
}

#[derive(Debug)]
pub(super) enum LoadedDetails {
    Weapon(WeaponRuntimeGraph),
    Perk(PerkDetails),
    Dye(crate::weapon_dyes::WeaponDyeColors),
}

#[derive(Debug)]
pub(super) struct PerkDetails {
    pub row: FinishedSandboxPerk,
    pub action: Result<PerkAction, String>,
}

#[derive(Debug)]
pub(super) struct PerkAction {
    pub tag: u32,
    pub graphs: Vec<PerkGraph>,
}

#[derive(Debug)]
pub(super) struct PerkGraph {
    pub tag: u32,
    pub action_offsets: Vec<usize>,
    pub decoded: Result<WeaponRuntimeGraph, String>,
}

pub(super) fn load(install: &Path, target: RuntimeTarget) -> Result<LoadedDetails, String> {
    if let RuntimeTarget::Dye(index) = target {
        return crate::weapon_dyes::load_weapon_dye_colors(&install.join("packages"), &[index])?
            .remove(&index)
            .ok_or_else(|| "The dye reader returned no result".to_owned())?
            .map(LoadedDetails::Dye);
    }
    let manager = open_shadowkeep_packages(install)?;
    match target {
        RuntimeTarget::Weapon(pattern_index) => {
            // Translation row indices are authoritative; several items may share a pattern.
            let source =
                load_weapon_runtime_entity_at_pattern_index_with_manager(&manager, pattern_index)?;
            load_weapon_runtime_graph_for_entity(
                &manager,
                source.item_hash,
                source.pattern_global_id_hash,
                source.entity_tag,
                &source.payload,
            )
            .map(LoadedDetails::Weapon)
        }
        RuntimeTarget::Perk(index) => load_perk(&manager, index).map(LoadedDetails::Perk),
        RuntimeTarget::Dye(_) => {
            unreachable!("dye reads are handled before opening runtime packages")
        }
    }
}

fn load_perk(manager: &PackageManager, index: u16) -> Result<PerkDetails, String> {
    let globals_tag = resolve_live_named_tag(manager, "investment_globals", None)?;
    let globals = manager
        .read_tag(globals_tag)
        .map_err(|error| error.to_string())?;
    let catalog_tag = TagHash(investment_globals_table_tag(
        &globals,
        GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT,
    )?);
    let payload = manager
        .read_tag(catalog_tag)
        .map_err(|error| error.to_string())?;
    validate_finished_sandbox_perk_catalog(&payload)?;
    let row = finished_sandbox_perk_at(&payload, usize::from(index))?;
    // Preserve the finished row even if its action is absent or cannot be decoded.
    let action =
        load_sandbox_perk_runtime_action(manager, &globals, usize::from(index)).map(|action| {
            let graphs = action
                .graphs
                .into_iter()
                .map(|source| PerkGraph {
                    tag: source.tag.0,
                    action_offsets: source.action_offsets,
                    decoded: load_weapon_runtime_graph_for_entity(
                        manager,
                        0,
                        0,
                        source.tag.0,
                        &source.payload,
                    ),
                })
                .collect();
            PerkAction {
                tag: action.action_tag.0,
                graphs,
            }
        });
    Ok(PerkDetails { row, action })
}
