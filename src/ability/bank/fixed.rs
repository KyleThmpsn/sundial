//! Literal modifier bodies require a qualified bank-local witness and reciprocal pairs.
use super::*;
mod duration;
mod protection;
pub(super) mod rate;
pub use protection::constant_protection_definition;

/// Selection metadata for the private row. A zero required key means no inner gate.
/// Protection and active rates preserve source names under their qualified native storage routes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedSelection {
    pub name: u32,
    pub priority: u8,
    pub required_key: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedModifier {
    pub template_key: u32,
    pub class: u32,
    pub words: [u32; 8],
    pub selection: Option<FixedSelection>,
}

impl FixedModifier {
    pub fn byte_len(self) -> Result<usize, String> {
        match self.class {
            0x80804399 => Ok(32),
            0x808044D6 => Ok(16),
            duration::CLASS => Ok(12),
            0x8080442E | 0x808044D4 | protection::CLASS => Ok(8),
            0x80804528
            | 0x8080411F
            | 0x808042A5
            | 0x8080411B
            | 0x80804127
            | 0x808044D2
            | rate::CLASS => Ok(4),
            _ => Err("Unqualified fixed ability modifier class".into()),
        }
    }

    pub fn validate(self) -> Result<(), String> {
        let count = self.byte_len()? / 4;
        if [0, u32::MAX, 0x811C9DC5].contains(&self.template_key)
            || self.words[count..].iter().any(|&v| v != 0)
            || self.words[..count]
                .iter()
                .any(|&v| !f32::from_bits(v).is_finite())
            || (self.class == 0x8080442E && self.words[0] > 2)
            || (self.class == 0x8080411F && self.words[0] > 2)
            || (self.class == 0x80804127 && f32::from_bits(self.words[0]) <= 0.0)
            || (self.class == 0x808044D2 && self.words[0] > 1)
            || (matches!(self.class, 0x808044D4 | 0x808044D6)
                && self.words[..count].iter().any(|&v| f32::from_bits(v) < 0.0))
            || (matches!(self.class, 0x808044D2 | 0x808044D4 | 0x808044D6)
                && self.selection
                    != Some(FixedSelection {
                        name: 0x811C9DC5,
                        priority: 255,
                        required_key: 0,
                    }))
            || (self.class == protection::CLASS
                && (self.words[0] != (-1.0f32).to_bits()
                    || !(0.0..=1.0).contains(&f32::from_bits(self.words[1]))
                    || self
                        .selection
                        .is_some_and(|s| s.priority != 255 || s.required_key != 0)))
            || (self.class == rate::CLASS
                && (f32::from_bits(self.words[0]) < 0.0
                    || self.selection.is_none()
                    || self
                        .selection
                        .is_some_and(|s| s.priority != 255 || s.required_key != 0)))
            || (self.class == 0x808042A5
                && (self.selection.is_none() || !(1..=16).contains(&self.words[0])))
            || (self.class == duration::CLASS
                && (self.selection.is_none()
                    || self.words[0] != 1
                    || self.words[1..3].iter().any(|&v| f32::from_bits(v) <= 0.0)))
            || self.selection.is_some_and(|s| {
                !matches!(
                    self.class,
                    0x808042A5
                        | 0x808044D2
                        | 0x808044D4
                        | 0x808044D6
                        | duration::CLASS
                        | protection::CLASS
                        | rate::CLASS
                ) || (!matches!(s.priority, 1 | 2 | 255)
                    && !(self.class == 0x808042A5 && s.priority == 0))
                    || [u32::MAX, 0x811C9DC5].contains(&s.required_key)
            })
            || (self.class == 0x80804399
                && (self.words[1] > 1 || self.words[2..4] != [0; 2] || self.words[5..] != [0; 3]))
        {
            return Err("Fixed ability modifier has unqualified fields".into());
        }
        Ok(())
    }

    pub fn check_bank(self, payload: &[u8]) -> Result<(), String> {
        self.validate()?;
        let layout = layout(payload)?;
        if self.class == rate::CLASS {
            return rate::check(payload, &layout, self);
        }
        if matches!(self.class, 0x808044D2 | 0x808044D4 | 0x808044D6)
            && (u32_at(payload, layout.definition - 4)? != 0x8080429E
                || u32_at(payload, layout.instance - 4)? != 0x8080429F)
        {
            return Err("Movement literal requires the qualified Jump bank".into());
        }
        let mut found = 0;
        for i in 0..layout.definition_rows.count {
            let row = layout.definition_rows.first + i * DEFINITION_ROW_SIZE;
            if u32_at(payload, row + ROW_KEY)? != self.template_key {
                continue;
            }
            let m = pointer(payload, row + ROW_MODIFIER)?;
            if u32_at(payload, m + BLOCK_CLASS)? != self.class {
                continue;
            }
            if let Some(selection) = self.selection {
                if (self.class != protection::CLASS
                    && u32_at(payload, row + ROW_NAME)? != selection.name)
                    || u32_at(payload, row + ROW_FLAGS)? != u32::from(selection.priority)
                {
                    continue;
                }
                let gate = if u64_at(payload, row + ROW_GATE)? == 0 {
                    0
                } else {
                    let gate = pointer(payload, row + ROW_GATE)?;
                    if gate < 4
                        || u32_at(payload, gate - 4)? != GATE_CLASS
                        || u32_at(payload, gate + 4)? != 0
                    {
                        return Err("Fixed ability modifier has an unqualified gate".into());
                    }
                    u32_at(payload, gate)?
                };
                if gate != selection.required_key {
                    continue;
                }
            }
            found += 1;
            let twin = usize::try_from(u64_at(payload, m + BLOCK_TWIN)?)
                .map_err(|_| "Modifier twin overflow")?;
            let marker = m.checked_sub(4).ok_or("Modifier has no class marker")?;
            if u32_at(payload, m + BLOCK_CLASS)? != self.class
                || u32_at(payload, marker)? != self.class + 1
                || u32_at(payload, twin + BLOCK_CLASS)? != self.class + 1
                || u64_at(payload, twin + BLOCK_TWIN)? != m as u64
                || u32_at(payload, m)? != layout.owner
                || u32_at(payload, twin)? != layout.owner
                || (self.selection.is_none()
                    && (u64_at(payload, row + ROW_GATE)? != 0
                        || u32_at(payload, row + ROW_FLAGS)? != 255))
            {
                return Err(
                    "Fixed ability modifier template has another class, gate or owner".into(),
                );
            }
            payload
                .get(m + 16..m + 16 + self.byte_len()?)
                .ok_or("Truncated fixed ability modifier")?;
            if self.class == duration::CLASS {
                duration::check(payload, layout.owner, m, twin)?;
            } else if self.class == protection::CLASS {
                protection::check(payload, m, twin)?;
            }
        }
        if found != 1 {
            return Err("Fixed ability modifier requires one matching stock property".into());
        }
        Ok(())
    }

    pub(super) fn definition_size(self) -> Result<usize, String> {
        Ok(match self.class {
            duration::CLASS => 72,
            protection::CLASS => protection::SIZE,
            _ => (16 + self.byte_len()?).next_multiple_of(8),
        })
    }

    pub(super) fn extra_blocks(self) -> usize {
        if self.class == duration::CLASS { 2 } else { 0 }
    }

    pub(super) fn write_nested(
        self,
        out: &mut [u8],
        owner: u32,
        definition: usize,
        instance: usize,
    ) -> Result<(), String> {
        if self.class == duration::CLASS {
            duration::write(out, owner, definition, instance)?;
        } else if self.class == protection::CLASS {
            protection::write(out, definition, self.words[1])?;
        }
        Ok(())
    }
}

pub(super) fn instance_size(class: u32) -> usize {
    if class == duration::CLASS { 80 } else { 40 }
}
