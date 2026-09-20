//! Dawn's eight unfiltered dismantle material policies.
use super::error::DawnAccountError;
use rusqlite::{Connection, params};
use sundial_account::ProfileState;
#[cfg(test)]
mod tests;

pub(super) fn rows(profile: &ProfileState) -> Vec<(u32, i32)> {
    profile
        .dismantle_rewards()
        .iter()
        .map(|reward| (reward.definition_hash.get(), reward.quantity))
        .collect()
}

pub(super) fn save(
    db: &Connection,
    loaded: &[(u32, i32)],
    profile: &ProfileState,
) -> Result<(), DawnAccountError> {
    let stored: Vec<(i64, u32, i32)> = db
        .prepare(
            "SELECT position,definition_hash,quantity FROM dismantle_rewards ORDER BY position",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    if stored.len() != loaded.len()
        || stored
            .iter()
            .enumerate()
            .any(|(index, &(position, hash, quantity))| {
                position != index as i64 || loaded[index] != (hash, quantity)
            })
    {
        return Err(DawnAccountError::Unwritable(
            "Dawn dismantle rewards changed while they were open. Reload before saving.".into(),
        ));
    }
    let current = rows(profile);
    // Validate again at the persistence boundary. Dawn has no rarity or gear filters.
    ProfileState::try_new(
        super::DawnAccountDocument::profile_capabilities(),
        Vec::new(),
        profile.dismantle_rewards().to_vec(),
    )?;
    for (position, &(hash, quantity)) in current.iter().enumerate() {
        if loaded.get(position) == Some(&(hash, quantity)) {
            continue;
        }
        if position < stored.len() {
            db.execute(
                "UPDATE dismantle_rewards SET definition_hash=?2,quantity=?3 WHERE position=?1",
                params![position, hash, quantity],
            )?;
        } else {
            db.execute(
                "INSERT INTO dismantle_rewards(position,definition_hash,quantity) VALUES(?1,?2,?3)",
                params![position, hash, quantity],
            )?;
        }
    }
    db.execute(
        "DELETE FROM dismantle_rewards WHERE position>=?1",
        [current.len()],
    )?;
    Ok(())
}
