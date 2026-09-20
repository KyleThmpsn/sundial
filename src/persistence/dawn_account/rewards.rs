//! Dawn delivery ledger. Mission/run identities belong to Dawn, not Sunrise's queue.
use super::{DawnAccountDocument, contract, error::DawnAccountError};
use rusqlite::{Connection, params};
mod edits;
mod writer;
pub(super) use writer::save;

// A namespaced editor origin, not a completed game mission. Dawn's delivery consumer
// validates that mission_hash is nonzero but never resolves it or writes mission state.
pub(crate) const EDITOR_MISSION: u32 = 0xFFFF_FFFE;
const EDITOR_SESSION: u64 = 0x5355_4E44_4941_4C00;

pub(crate) fn supports_currency(metadata: &crate::catalog::InventoryMetadata) -> bool {
    metadata.is_profile_items_candidate()
        && metadata.bucket_capacity == Some(1)
        && !matches!(metadata.native_bucket_id, 13 | 14)
        && metadata
            .max_stack_size
            .is_some_and(|maximum| maximum <= i32::MAX as u32)
}

pub(super) fn sequence(db: &Connection) -> Result<i64, DawnAccountError> {
    Ok(db.query_row("SELECT max(COALESCE((SELECT seq FROM sqlite_sequence WHERE name='reward_debts'),0),COALESCE((SELECT max(debt_id) FROM reward_debts),0))", [], |row| row.get(0))?)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RewardDebt {
    pub id: i64,
    pub account_soid: u64,
    pub character_soid: u64,
    pub runtime_epoch: u64,
    pub session_id: u64,
    pub run_id: u64,
    pub mission_hash: u32,
    pub definition_hash: u32,
    pub quantity: i32,
    pub credited: i32,
    pub delivered: bool,
}

pub(super) fn load(db: &Connection) -> Result<Vec<RewardDebt>, DawnAccountError> {
    let mut query = db.prepare("SELECT debt_id,account_soid,character_soid,runtime_epoch,session_id,run_id,mission_hash,definition_hash,quantity,credited,delivered FROM reward_debts ORDER BY debt_id")?;
    let rows = query.query_map([], |r| {
        let soid = |index| -> rusqlite::Result<u64> {
            let text: String = r.get(index)?;
            contract::parse_soid(&text).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    index,
                    rusqlite::types::Type::Text,
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Dawn reward_debts contains an invalid fixed-width identity",
                    )
                    .into(),
                )
            })
        };
        Ok((
            RewardDebt {
                id: r.get(0)?,
                account_soid: soid(1)?,
                character_soid: soid(2)?,
                runtime_epoch: soid(3)?,
                session_id: soid(4)?,
                run_id: soid(5)?,
                mission_hash: r.get(6)?,
                definition_hash: r.get(7)?,
                quantity: r.get(8)?,
                credited: r.get(9)?,
                delivered: false,
            },
            r.get::<_, i32>(10)?,
        ))
    })?;
    let mut debts = Vec::new();
    for row in rows {
        let (mut debt, delivered) = row?;
        if debt.id <= 0
            || debt.mission_hash == 0
            || debt.definition_hash == 0
            || debt.quantity <= 0
            || !(0..=debt.quantity).contains(&debt.credited)
            || !matches!(delivered, 0 | 1)
        {
            return Err(DawnAccountError::Unwritable(
                "Dawn reward_debts contains an invalid delivery record".into(),
            ));
        }
        debt.delivered = delivered == 1;
        debts.push(debt);
    }
    Ok(debts)
}

impl DawnAccountDocument {
    pub(crate) fn reward_debts(&self) -> &[RewardDebt] {
        &self.reward_debts
    }
}

#[cfg(test)]
mod tests;
