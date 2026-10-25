//! Super-specific numeric declarations share the bank's flattened input storage.
use super::*;
use serde::{Deserialize, Serialize};

pub(super) const CLASS: u32 = 0x8080447C;
pub(super) const TWIN: u32 = 0x8080447D;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupEntry {
    pub class: u32,
    pub input: u16,
    pub value: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GroupModifier {
    pub template_key: u32,
    pub count: u8,
    pub entries: [GroupEntry; 8],
}

impl GroupModifier {
    pub fn validate(self) -> Result<(), String> {
        let count = usize::from(self.count);
        if !(1..=self.entries.len()).contains(&count)
            || [0, u32::MAX, 0x811C9DC5].contains(&self.template_key)
            || self.entries[count..]
                .iter()
                .any(|v| *v != GroupEntry::default())
        {
            return Err("Invalid grouped ability property".into());
        }
        let mut seen = BTreeSet::new();
        for entry in &self.entries[..count] {
            let valid = match (entry.class, entry.input) {
                (0x80804483, 0..=2) | (0x80804484, 3) => true,
                (0x80804481, 32) => f32::from_bits(entry.value) > 0.0,
                _ => false,
            };
            if !valid
                || !f32::from_bits(entry.value).is_finite()
                || !seen.insert((entry.class, entry.input))
            {
                return Err("Unqualified grouped ability input".into());
            }
        }
        Ok(())
    }

    /// Select one ungated same-key declaration. A gated alternative does not
    /// qualify the unconditional property and must remain a separate stock row.
    pub fn check_bank(self, payload: &[u8]) -> Result<(), String> {
        self.validate()?;
        let layout = layout(payload)?;
        let mut found = 0;
        for i in 0..layout.definition_rows.count {
            let row = layout.definition_rows.first + i * DEFINITION_ROW_SIZE;
            if u32_at(payload, row + ROW_KEY)? != self.template_key
                || u64_at(payload, row + ROW_GATE)? != 0
            {
                continue;
            }
            let m = pointer(payload, row + ROW_MODIFIER)?;
            if u32_at(payload, m + BLOCK_CLASS)? != CLASS {
                continue;
            }
            found += 1;
            let twin = usize::try_from(u64_at(payload, m + BLOCK_TWIN)?)
                .map_err(|_| "Grouped modifier twin overflow")?;
            if u32_at(payload, m.checked_sub(4).ok_or("Group modifier marker")?)? != TWIN
                || u32_at(payload, twin + BLOCK_CLASS)? != TWIN
                || u64_at(payload, twin + BLOCK_TWIN)? != m as u64
                || u32_at(payload, m)? != layout.owner
                || u32_at(payload, twin)? != layout.owner
                || u32_at(payload, row + ROW_FLAGS)? != 255
                || u32_at(payload, row + ROW_MODE)? != 255
            {
                return Err("Grouped modifier template has another pair, owner or mode".into());
            }
            let children = rows(payload, m + 16, 0x80804485, "grouped declaration")?;
            let inputs = rows(payload, layout.instance + 0xA8, 0x808045B7, "grouped input")?;
            let first = u32_at(payload, m + 32)? as usize;
            if children.count == 0
                || children.count > 32
                || u32_at(payload, m + 36)? as usize != children.count
                || first
                    .checked_add(children.count)
                    .is_none_or(|end| end > inputs.count)
            {
                return Err("Grouped declaration input range differs".into());
            }
            let mut available = BTreeSet::new();
            for index in 0..children.count {
                let child = pointer(payload, children.first + index * 8)?;
                let class = u32_at(payload, child.checked_sub(4).ok_or("Group child marker")?)?;
                let selector = u32_at(payload, child)?;
                let value = u32_at(payload, child + 4)?;
                let flat = inputs.first + (first + index) * 48;
                if u32_at(payload, flat + 40)? != value
                    || u32_at(payload, flat + 44)? != ((selector >> 8) & 255)
                {
                    return Err("Stock grouped declaration and flattened value disagree".into());
                }
                if !available.insert((class, selector, value)) {
                    return Err("Ambiguous stock grouped declaration".into());
                }
            }
            for entry in &self.entries[..usize::from(self.count)] {
                // This route qualifies literal same-key child values. It deliberately
                // does not infer arbitrary new selectors from a shared parent class.
                if !available.contains(&(entry.class, u32::from(entry.input), entry.value)) {
                    return Err("Grouped ability input has no exact native witness".into());
                }
            }
        }
        if found != 1 {
            return Err("Grouped ability property has no unique ungated witness".into());
        }
        Ok(())
    }
}
