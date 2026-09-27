//! Adds the authored items an account has no other way to obtain after a package install.
use std::path::Path;

use crate::account::{
    AuthoredGrantOutcome, AuthoredGrantReport, AuthoredGrantTarget, AuthoredItemGrant,
};
use crate::app::account_workspace::{self as account, WorkspaceDocument};
use crate::persistence::json_account::inventory::{InventoryItemLocation, NewInventoryItem};
use sundial_account::{CharacterAbilities, CharacterMetadataUpdate};

/// The ability entries a subclass starts with. Every stock list shares one layout, so the same
/// entries select class ability, movement, grenade, super and melee on any of them.
const DEFAULT_ABILITIES: CharacterAbilities = CharacterAbilities {
    movement: 4,
    grenade: 7,
    super_ability: 10,
    melee: 11,
    class_ability: 2,
};

type BucketOf<'a> = dyn FnMut(u32) -> Result<Option<u8>, String> + 'a;

/// Adds each grant the account lacks and has room for, then saves once with a verified backup.
/// Copies the account already holds are skipped, so a repeat install adds nothing new.
pub(crate) fn grant_authored_items(
    install: &Path,
    grants: &[AuthoredItemGrant],
    bucket_of: &mut BucketOf<'_>,
) -> Result<AuthoredGrantReport, String> {
    let preferences = crate::app::settings::load_preferences().preferences;
    let runtime = crate::package_runtime::installed_runtime(install)?;
    let settings_path = super::authored_runtime_settings_path(install, &preferences, &runtime)?;
    // The runtime owns its account while it is up, and its journal would be written over.
    crate::app::settings::require_game_closed(crate::app::platform::destiny_is_running())?;
    let dawn = runtime.brand() == crate::package_runtime::RuntimeBrand::Dawn;
    let original = crate::persistence::json_document::load_workspace_json(&settings_path)?;
    let mut document = WorkspaceDocument::load(original.clone(), &settings_path, dawn);
    if let Some(reason) = document.account_editing_blocked() {
        return Err(reason.to_owned());
    }
    let mut report = AuthoredGrantReport {
        account_path: if document.uses_json_account() {
            settings_path.clone()
        } else {
            document.source_info().database_path
        },
        ..AuthoredGrantReport::default()
    };
    for grant in grants {
        match grant.target {
            AuthoredGrantTarget::Class { class_type, equip } => {
                grant_characters(
                    &mut document,
                    grant,
                    (class_type, equip),
                    bucket_of,
                    &mut report,
                )?;
            }
            AuthoredGrantTarget::Profile(quantity) => {
                grant_profile(&mut document, grant, quantity, bucket_of, &mut report)?;
            }
        }
    }
    if report.added.is_empty() && report.equipped.is_empty() {
        return Ok(report);
    }
    crate::package_runtime::verify_installed_runtime(install, &runtime)?;
    report.backup_path = Some(if document.uses_json_account() {
        crate::app::settings::save_json(&settings_path, document.json(), &original, false)
            .map_err(String::from)?
            .backup
    } else {
        document.save_account()?.backup().to_path_buf()
    });
    Ok(report)
}

/// Adds a copy to each character of the class that holds none and has room in its bucket, and
/// equips it when asked.
fn grant_characters(
    document: &mut WorkspaceDocument,
    grant: &AuthoredItemGrant,
    (class_type, equip): (u8, bool),
    bucket_of: &mut BucketOf<'_>,
    report: &mut AuthoredGrantReport,
) -> Result<(), String> {
    for character in 0..account::character_count(document) {
        if account::character_metadata(document, character)?.class_type != class_type {
            continue;
        }
        let snapshots = account::equipped_item_snapshots(document, character)?;
        let worn = snapshots.iter().any(|item| {
            item.slot == "subclass" && item.definition_hash == Some(u64::from(grant.item_hash))
        });
        let equipped = snapshots
            .into_iter()
            .filter_map(|item| {
                item.definition_hash
                    .and_then(|hash| u32::try_from(hash).ok())
            })
            .collect::<Vec<_>>();
        let stored = account::character_inventory_storage(document, character)
            .map_err(|error| error.to_string())?;
        // Dawn's Postmaster rows are held, but they sit outside the item's own bucket.
        let visible = account::character_inventory(document, character)
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        let outcome = AuthoredGrantOutcome {
            item_hash: grant.item_hash,
            character_index: Some(character),
        };
        if equipped
            .iter()
            .chain(stored.iter().map(|item| &item.definition_hash))
            .any(|hash| *hash == grant.item_hash)
        {
            let held = visible
                .iter()
                .find(|item| item.definition_hash == grant.item_hash)
                .map(|item| item.location);
            if let Some(location) = held.filter(|_| equip && !worn) {
                equip_subclass(document, location)?;
                report.equipped.push(outcome);
            }
            continue;
        }
        let mut used = 0;
        for hash in equipped
            .iter()
            .copied()
            .chain(visible.iter().map(|item| item.definition_hash))
        {
            used += usize::from(bucket_of(hash)? == Some(grant.bucket));
        }
        if used >= grant.capacity || stored.len() >= account::character_inventory_capacity(document)
        {
            report.full.push(outcome);
            continue;
        }
        let location = account::add_inventory_item(
            document,
            character,
            NewInventoryItem::single(grant.item_hash, 0),
        )
        .map_err(|error| error.to_string())?;
        report.added.push(outcome);
        if equip {
            equip_subclass(document, location)?;
            report.equipped.push(outcome);
        }
    }
    Ok(())
}

/// Moves a held subclass into the subclass slot, and its predecessor into the inventory. Where
/// the account keeps ability choices on the character, they start over at the defaults, as they
/// do when the app swaps a subclass.
fn equip_subclass(
    document: &mut WorkspaceDocument,
    location: InventoryItemLocation,
) -> Result<(), String> {
    account::swap_inventory_item_with_equipment(document, location, "subclass")
        .map_err(|error| error.to_string())?;
    if !document.uses_subclass_plug_abilities() {
        account::apply_character_updates(
            document,
            location.character_index,
            vec![CharacterMetadataUpdate::SetAbilities(DEFAULT_ABILITIES)],
        )?;
    }
    Ok(())
}

/// Adds one stack unless the profile already holds the item or its bucket is full.
fn grant_profile(
    document: &mut WorkspaceDocument,
    grant: &AuthoredItemGrant,
    quantity: i32,
    bucket_of: &mut BucketOf<'_>,
    report: &mut AuthoredGrantReport,
) -> Result<(), String> {
    let rows = account::profile_items(document)
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    if rows
        .iter()
        .any(|row| row.definition_hash == grant.item_hash)
    {
        return Ok(());
    }
    let mut used = 0;
    for row in &rows {
        used += usize::from(bucket_of(row.definition_hash)? == Some(grant.bucket));
    }
    let outcome = AuthoredGrantOutcome {
        item_hash: grant.item_hash,
        character_index: None,
    };
    if used >= grant.capacity
        || account::profile_item_capacity(document).is_some_and(|capacity| rows.len() >= capacity)
    {
        report.full.push(outcome);
        return Ok(());
    }
    account::add_profile_item(document, grant.item_hash, quantity)
        .map_err(|error| error.to_string())?;
    report.added.push(outcome);
    Ok(())
}
