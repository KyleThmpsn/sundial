//! Package account proposals. Native package allocation and installation stay in Parhelion.
use super::{
    positions::compact,
    snapshot::{self, Snapshot, capture, open},
    validation::connection as validate,
};
use crate::account::{AuthoredAccountCleanup, AuthoredCollectionUnlock, AuthoredSocketChange};
use rusqlite::{Connection, params};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Duration,
};

pub(crate) fn replace(path: &Path, expected: &[u8], updated: &[u8]) -> Result<(), String> {
    snapshot::replace_with(path, expected, updated, replacement_tables)
}

fn replacement_tables(before: &Snapshot, after: &Snapshot) -> Result<Vec<String>, String> {
    const ORDER: [&str; 8] = [
        "sockets",
        "items",
        "profile_items",
        "dismantle_rewards",
        "character_stacks",
        "pending_rewards",
        "unlocks",
        "family5",
    ];
    for (name, table) in &after.tables {
        if before.tables[name] != *table
            && (!ORDER.contains(&name.as_str()) || before.tables[name].columns != table.columns)
        {
            return Err(format!("Unsupported account proposal table {name}"));
        }
    }
    // Sockets reference item identities. Rewrite them with any changed item collection so that
    // ON DELETE CASCADE cannot discard unchanged socket rows.
    let items_changed = before.tables["items"] != after.tables["items"];
    let changed = ORDER
        .into_iter()
        .filter(|name| {
            before.tables[*name] != after.tables[*name] || (*name == "sockets" && items_changed)
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    Ok(changed)
}

#[cfg(test)]
pub(crate) fn preview(
    path: &Path,
    hashes: &BTreeSet<u32>,
    unlocks: &[AuthoredCollectionUnlock],
    changes: &[AuthoredSocketChange],
) -> Result<AuthoredAccountCleanup, String> {
    preview_replacement(path, hashes, unlocks, changes, None)
}

mod placement;

pub(crate) fn preview_replacement(
    path: &Path,
    hashes: &BTreeSet<u32>,
    unlocks: &[AuthoredCollectionUnlock],
    changes: &[AuthoredSocketChange],
    slots: Option<&crate::account::AuthoredSlotReplacement>,
) -> Result<AuthoredAccountCleanup, String> {
    let mut source = open(path, false)?;
    let tx = source.transaction().map_err(err)?;
    validate(&tx)?;
    let original_bytes = capture(&tx)?;
    let mut staged = Connection::open_in_memory().map_err(err)?;
    rusqlite::backup::Backup::new(&tx, &mut staged)
        .map_err(err)?
        .run_to_completion(128, Duration::from_millis(1), None)
        .map_err(err)?;
    staged
        .execute_batch("PRAGMA foreign_keys=ON;")
        .map_err(err)?;
    let mut report = AuthoredAccountCleanup {
        settings_path: path.to_path_buf(),
        original_bytes,
        cleaned_bytes: Vec::new(),
        removed_items: BTreeMap::new(),
        cleared_plugs: 0,
        removed_reward_rules: 0,
        cleared_unlocks: 0,
        resized_items: BTreeMap::new(),
        slot_moves: vec![],
    };
    for hash in hashes {
        report.cleared_plugs += staged
            .execute("DELETE FROM sockets WHERE plug_hash=?", [i64::from(*hash)])
            .map_err(err)?;
        let mut removed = 0;
        for table in ["items", "profile_items", "character_stacks"] {
            removed += staged
                .execute(
                    &format!("DELETE FROM {table} WHERE definition_hash=?"),
                    [i64::from(*hash)],
                )
                .map_err(err)?;
        }
        if removed != 0 {
            report.removed_items.insert(*hash, removed);
        }
        for table in ["dismantle_rewards", "pending_rewards"] {
            report.removed_reward_rules += staged
                .execute(
                    &format!("DELETE FROM {table} WHERE definition_hash=?"),
                    [i64::from(*hash)],
                )
                .map_err(err)?;
        }
    }
    for unlock in unlocks {
        let removed_override = staged
            .execute(
                "DELETE FROM family5 WHERE kind=0 AND slot=?",
                [unlock.definition_index],
            )
            .map_err(err)?;
        let bank = match unlock.bank {
            1 => 0,
            2 => 1,
            3 => 4,
            6 => 2,
            _ => return Err("Unsupported authored unlock bank".into()),
        };
        let removed_flag = staged
            .execute(
                "DELETE FROM unlocks WHERE bank=? AND slot=? AND lane=0",
                params![bank, unlock.slot],
            )
            .map_err(err)?;
        report.cleared_unlocks += usize::from(removed_override != 0 || removed_flag != 0);
    }
    for table in ["profile_items", "dismantle_rewards"] {
        compact(&staged, table, "1=1")?;
    }
    compact(&staged, "family5", "kind=0")?;
    for slot in 0..3 {
        compact(
            &staged,
            "items",
            &format!("character_slot={slot} AND location=1"),
        )?;
        compact(
            &staged,
            "character_stacks",
            &format!("character_slot={slot}"),
        )?;
    }
    report.slot_moves = placement::relocate(&staged, hashes, slots)?;
    resize(&staged, hashes, changes, &mut report.resized_items)?;
    validate(&staged)?;
    report.cleaned_bytes = capture(&staged)?;
    Ok(report)
}
fn resize(
    db: &Connection,
    removed: &BTreeSet<u32>,
    changes: &[AuthoredSocketChange],
    report: &mut BTreeMap<u32, usize>,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for change in changes {
        if removed.contains(&change.definition_hash)
            || !seen.insert(change.definition_hash)
            || change.previous_socket_count > 12
            || change.default_plugs.len() > 12
            || change
                .default_plugs
                .iter()
                .flatten()
                .any(|h| *h == 0 || *h == u32::MAX)
        {
            return Err("Conflicting or unsupported replacement socket layouts".into());
        }
        let mut stmt=db.prepare("SELECT instance_soid,plug_count FROM items WHERE definition_hash=? AND socket_policy=1").map_err(err)?;
        let rows = stmt
            .query_map([i64::from(change.definition_hash)], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, usize>(1)?))
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        for (soid, count) in rows {
            let new = change.default_plugs.len();
            if count == new {
                continue;
            }
            if count != change.previous_socket_count {
                return Err(
                    "An authored item's socket count differs from the reviewed package".into(),
                );
            }
            db.execute(
                "DELETE FROM sockets WHERE instance_soid=? AND lane>=?",
                params![soid, new],
            )
            .map_err(err)?;
            for (lane, hash) in change.default_plugs.iter().enumerate().skip(count) {
                if let Some(hash) = hash {
                    db.execute(
                        "INSERT INTO sockets(instance_soid,lane,plug_hash) VALUES(?,?,?)",
                        params![soid, lane, hash],
                    )
                    .map_err(err)?;
                }
            }
            db.execute(
                "UPDATE items SET plug_count=? WHERE instance_soid=?",
                params![new, soid],
            )
            .map_err(err)?;
            *report.entry(change.definition_hash).or_default() += 1;
        }
    }
    Ok(())
}
fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
