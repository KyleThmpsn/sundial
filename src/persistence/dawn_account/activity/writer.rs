use super::{ActivityState, DawnAccountError};
use rusqlite::{Transaction, params};

impl ActivityState {
    pub(in crate::persistence::dawn_account) fn save(
        &self,
        db: &Transaction<'_>,
        before: &Self,
    ) -> Result<(), DawnAccountError> {
        if self == before {
            return Ok(());
        }
        if Self::load(db)? != *before {
            return Err(DawnAccountError::Unwritable(
                "Dawn's vendor or mission records changed. Reload before saving".into(),
            ));
        }
        if self.vendors != before.vendors {
            replace_rows(
                db,
                "vendor_progress",
                &["owner_soid", "position"],
                self.vendors
                    .iter()
                    .map(|r| {
                        vec![
                            ("owner_soid", r.owner.clone().into()),
                            ("position", r.position.into()),
                            ("vendor", i64::from(r.vendor).into()),
                            ("points", r.points.into()),
                            ("rewards", r.rewards.into()),
                        ]
                    })
                    .collect(),
            )?;
        }
        if self.unlocks != before.unlocks {
            replace_rows(
                db,
                "vendor_unlocks",
                &["owner_soid", "kind", "slot"],
                self.unlocks
                    .iter()
                    .map(|r| {
                        vec![
                            ("owner_soid", r.owner.clone().into()),
                            ("kind", r.kind.into()),
                            ("position", r.position.into()),
                            ("slot", i64::from(r.slot).into()),
                            ("value", r.value.into()),
                        ]
                    })
                    .collect(),
            )?;
        }
        for row in &self.missions {
            if !before.missions.contains(row) {
                db.execute("UPDATE missions SET checkpoint_hash=?3,checkpoint_slice_set=?4,activity_index=?5,progress=?6,completed=?7,updated_utc=?8 WHERE character_soid=?1 AND mission_hash=?2", params![row.owner,row.hash,row.checkpoint,row.slice,row.activity,row.progress,row.completed,row.updated])?;
            }
        }
        Ok(())
    }
}

/// Plan complete final rows before deleting any rows. This avoids transient collisions
/// between the positional primary key and the second unique identity, while retaining
/// extension columns by the table's stable editor identity.
fn replace_rows(
    db: &Transaction<'_>,
    table: &str,
    identity: &[&str],
    edits: Vec<Vec<(&str, rusqlite::types::Value)>>,
) -> Result<(), DawnAccountError> {
    use crate::persistence::native_account::snapshot::quote;
    use rusqlite::types::Value;
    use std::collections::BTreeMap;
    let mut query = db.prepare(&format!("SELECT * FROM {}", quote(table)))?;
    let columns = query
        .column_names()
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let stored = query
        .query_map([], |row| {
            columns
                .iter()
                .enumerate()
                .map(|(i, key)| Ok((key.clone(), row.get::<_, Value>(i)?)))
                .collect::<rusqlite::Result<BTreeMap<_, _>>>()
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut rows = Vec::new();
    for edit in edits {
        let edit = edit
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect::<BTreeMap<_, _>>();
        let mut row = stored
            .iter()
            .find(|row| identity.iter().all(|key| row.get(*key) == edit.get(*key)))
            .cloned()
            .unwrap_or_default();
        row.extend(edit);
        rows.push(row);
    }
    drop(query);
    db.execute(&format!("DELETE FROM {}", quote(table)), [])?;
    for row in rows {
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            quote(table),
            row.keys().map(|s| quote(s)).collect::<Vec<_>>().join(","),
            vec!["?"; row.len()].join(",")
        );
        db.execute(&sql, rusqlite::params_from_iter(row.values()))?;
    }
    Ok(())
}
