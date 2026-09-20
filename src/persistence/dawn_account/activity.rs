//! Dawn's vendor and mission records. Identity and native ordering are retained on edits.
mod writer;

use super::{DawnAccountDocument, error::DawnAccountError};
use rusqlite::Connection;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VendorProgress {
    pub owner: String,
    pub position: i32,
    pub vendor: u16,
    pub points: i32,
    pub rewards: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VendorUnlock {
    pub owner: String,
    pub kind: i32,
    pub position: i32,
    pub slot: u16,
    pub value: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Mission {
    pub owner: String,
    pub hash: u32,
    pub checkpoint: u32,
    pub slice: i32,
    pub activity: i32,
    pub progress: i32,
    pub completed: bool,
    pub updated: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ActivityState {
    pub vendors: Vec<VendorProgress>,
    pub unlocks: Vec<VendorUnlock>,
    pub missions: Vec<Mission>,
}

impl ActivityState {
    pub(super) fn load(db: &Connection) -> Result<Self, DawnAccountError> {
        let vendors = db.prepare("SELECT owner_soid,position,vendor,points,rewards FROM vendor_progress ORDER BY owner_soid,position")?
            .query_map([], |r| Ok(VendorProgress { owner:r.get(0)?, position:r.get(1)?, vendor:r.get(2)?, points:r.get(3)?, rewards:r.get(4)? }))?
            .collect::<Result<_,_>>()?;
        let unlocks = db.prepare("SELECT owner_soid,kind,position,slot,value FROM vendor_unlocks ORDER BY owner_soid,kind,position")?
            .query_map([], |r| Ok(VendorUnlock { owner:r.get(0)?, kind:r.get(1)?, position:r.get(2)?, slot:r.get(3)?, value:r.get(4)? }))?
            .collect::<Result<_,_>>()?;
        let missions = db.prepare("SELECT character_soid,mission_hash,checkpoint_hash,checkpoint_slice_set,activity_index,progress,completed,updated_utc FROM missions ORDER BY character_soid,mission_hash")?
            .query_map([], |r| Ok(Mission { owner:r.get(0)?, hash:r.get(1)?, checkpoint:r.get(2)?, slice:r.get(3)?, activity:r.get(4)?, progress:r.get(5)?, completed:r.get(6)?, updated:r.get(7)? }))?
            .collect::<Result<_,_>>()?;
        Ok(Self {
            vendors,
            unlocks,
            missions,
        })
    }

    pub(super) fn validate(&self, document: &DawnAccountDocument) -> Result<(), String> {
        let mut positions = std::collections::BTreeSet::new();
        let mut identities = std::collections::BTreeSet::new();
        for r in &self.vendors {
            if !document.valid_owner(&r.owner, true)
                || !(0..16).contains(&r.position)
                || r.vendor == u16::MAX
                || r.points < 0
                || r.rewards < 0
                || !positions.insert((r.owner.to_ascii_uppercase(), r.position))
                || !identities.insert((r.owner.to_ascii_uppercase(), r.vendor))
            {
                return Err(
                    "Vendor progress has an invalid owner, duplicate vendor or out-of-range value"
                        .into(),
                );
            }
        }
        let mut groups = std::collections::BTreeMap::<_, Vec<_>>::new();
        let mut slots = std::collections::BTreeSet::new();
        for r in &self.unlocks {
            if !document.valid_owner(&r.owner, true)
                || !(0..2).contains(&r.kind)
                || !slots.insert((r.owner.to_ascii_uppercase(), r.kind, r.slot))
            {
                return Err("Vendor unlocks have an invalid owner, kind or duplicate slot".into());
            }
            groups
                .entry((r.owner.to_ascii_uppercase(), r.kind))
                .or_default()
                .push(r.position);
        }
        for positions in groups.values_mut() {
            positions.sort_unstable();
            if positions.len() > 2048 || positions.iter().enumerate().any(|(i, p)| *p != i as i32) {
                return Err(
                    "Vendor unlock rows must remain contiguous with at most 2048 entries per bank"
                        .into(),
                );
            }
        }
        let mut missions = std::collections::BTreeSet::new();
        for r in &self.missions {
            if !document.valid_owner(&r.owner, false)
                || r.hash == 0
                || r.updated < 0
                || !missions.insert((r.owner.to_ascii_uppercase(), r.hash))
            {
                return Err("Mission records require a character and a unique mission hash".into());
            }
        }
        Ok(())
    }
}

impl DawnAccountDocument {
    pub(crate) fn activity_state(&self) -> &ActivityState {
        &self.activity
    }

    pub(crate) fn character_owner(&self, index: usize) -> Option<String> {
        self.characters()
            .characters()
            .get(index)?
            .soid
            .map(|s| super::contract::format_soid(s.get()))
    }

    fn valid_owner(&self, owner: &str, account: bool) -> bool {
        let Ok(soid) = u64::from_str_radix(owner, 16) else {
            return false;
        };
        owner.len() == 16
            && soid != 0
            && ((account && soid == self.primary_soid().get())
                || self
                    .characters()
                    .characters()
                    .iter()
                    .any(|c| c.soid.is_some_and(|s| s.get() == soid)))
    }

    pub(crate) fn set_activity_state(&mut self, mut state: ActivityState) -> Result<(), String> {
        state.validate(self)?;
        // Mission identities come from Dawn. Editing a checkpoint must not create a new mission
        // or queue a reward. Keep its native update clock in sync only after an actual edit.
        if state.missions.len() != self.activity.missions.len()
            || state.missions.iter().any(|r| {
                !self
                    .activity
                    .missions
                    .iter()
                    .any(|old| old.owner == r.owner && old.hash == r.hash)
            })
        {
            return Err("Only missions already recorded by Dawn can be edited".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs() as i64;
        for row in &mut state.missions {
            if self
                .activity
                .missions
                .iter()
                .any(|old| old.owner == row.owner && old.hash == row.hash && old != row)
            {
                row.updated = now;
            }
        }
        state
            .vendors
            .sort_by(|a, b| (&a.owner, a.position).cmp(&(&b.owner, b.position)));
        state
            .unlocks
            .sort_by(|a, b| (&a.owner, a.kind, a.position).cmp(&(&b.owner, b.kind, b.position)));
        state
            .missions
            .sort_by(|a, b| (&a.owner, a.hash).cmp(&(&b.owner, b.hash)));
        self.activity = state;
        Ok(())
    }

    pub(crate) fn vendor_campaigns(&self, index: usize) -> Option<u8> {
        let owner = self.character_owner(index)?;
        self.carried
            .characters
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(&owner))
            .map(|(_, r)| r.vendor_campaigns as u8)
    }

    pub(crate) fn set_vendor_campaigns(&mut self, index: usize, flags: u8) -> Result<(), String> {
        if flags > 7 {
            return Err("Only Dawn's three campaign selections are supported".into());
        }
        let owner = self.character_owner(index).ok_or("Choose a character")?;
        let row = self
            .carried
            .characters
            .iter_mut()
            .find(|(key, _)| key.eq_ignore_ascii_case(&owner))
            .map(|(_, r)| r)
            .ok_or("Character state is unavailable")?;
        row.vendor_campaigns = i64::from(flags);
        Ok(())
    }
}
