//! Record completion fields name flag definitions. Sunrise resolves them through the account bank.
use std::collections::BTreeMap;

use tiger_pkg::{PackageManager, TagHash};

use crate::{
    investment_schema::{
        ROOT_UNLOCK_FLAG_BANK_TABLE_SLOT, ROOT_UNLOCK_FLAG_DEFINITION_TABLE_SLOT,
        UNLOCK_FLAG_DEFINITION_ROW_CLASS, UNLOCK_FLAG_DEFINITION_ROW_SIZE,
        investment_root_table_tag,
    },
    package_authoring::{SHADOWKEEP_ACCOUNT_FLAG_BANK, SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY},
    package_payload::{array_at, u16_at, u32_at},
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Unlock {
    pub definition_index: u16,
    pub slot: u16,
}

pub(super) struct Mapping {
    slots: BTreeMap<u32, u16>,
    definitions: Vec<u8>,
    definition_count: usize,
    definition_rows: usize,
    bank: Vec<u8>,
    bank_rows: usize,
}

impl Mapping {
    pub fn load(manager: &PackageManager, root: &[u8]) -> Result<Self, String> {
        let read = |slot| {
            let tag = investment_root_table_tag(root, slot)?;
            manager
                .read_tag(TagHash(tag))
                .map_err(|error| format!("Could not read title unlocks: {error}"))
        };
        let bank = read(ROOT_UNLOCK_FLAG_BANK_TABLE_SLOT)?;
        let (count, bank_rows, class) = array_at(&bank, 8)?;
        if count > SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY
            || class != 0x8080_7D48
            || bank_rows
                .checked_add(count * 8)
                .is_none_or(|end| end > bank.len())
        {
            return Err("The installed account flag mapping is invalid".into());
        }
        let mut slots = BTreeMap::new();
        for slot in 0..count {
            // Sunrise keeps the first mapping when a definition occurs more than once.
            slots
                .entry(u32_at(&bank, bank_rows + slot * 8 + 4)?)
                .or_insert(slot as u16);
        }
        let definitions = read(ROOT_UNLOCK_FLAG_DEFINITION_TABLE_SLOT)?;
        let (definition_count, definition_rows, class) = array_at(&definitions, 8)?;
        if class != UNLOCK_FLAG_DEFINITION_ROW_CLASS
            || definition_count
                .checked_mul(UNLOCK_FLAG_DEFINITION_ROW_SIZE)
                .and_then(|length| definition_rows.checked_add(length))
                .is_none_or(|end| end > definitions.len())
        {
            return Err("The installed title unlock definitions are invalid".into());
        }
        Ok(Self {
            slots,
            definitions,
            definition_count,
            definition_rows,
            bank,
            bank_rows,
        })
    }

    pub fn resolve(&self, definition_index: u16) -> Result<Unlock, String> {
        // Sunrise reads this field as a signed 16-bit slot and excludes non-positive values.
        if definition_index == 0 || definition_index > i16::MAX as u16 {
            return Err("This title has no claim flag that Sunrise can use".into());
        }
        let slot = self
            .slots
            .get(&u32::from(definition_index))
            .copied()
            .ok_or("This title has no account claim flag that Sunrise can use")?;
        if usize::from(definition_index) >= self.definition_count {
            return Err("This title references a missing claim flag".into());
        }
        let row =
            self.definition_rows + usize::from(definition_index) * UNLOCK_FLAG_DEFINITION_ROW_SIZE;
        if u16_at(&self.definitions, row + 4)?.to_le_bytes()[0] != SHADOWKEEP_ACCOUNT_FLAG_BANK
            || u16_at(&self.definitions, row + 6)? != slot
            || u32_at(&self.definitions, row)?
                != u32_at(&self.bank, self.bank_rows + usize::from(slot) * 8)?
        {
            return Err("This title's claim flag does not match its account mapping".into());
        }
        Ok(Unlock {
            definition_index,
            slot,
        })
    }
}
