//! Pointer-free modifier bodies require a matching existing property and reciprocal pairs.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedModifier {
    pub template_key: u32,
    pub class: u32,
    pub words: [u32; 8],
}

impl FixedModifier {
    pub fn byte_len(self) -> Result<usize, String> {
        match self.class {
            0x80804399 => Ok(32),
            0x8080442E => Ok(8),
            0x80804528 | 0x8080411F => Ok(4),
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
                || u64_at(payload, row + ROW_GATE)? != 0
                || u32_at(payload, row + ROW_FLAGS)? != 255
            {
                return Err(
                    "Fixed ability modifier template has another class, gate or owner".into(),
                );
            }
            payload
                .get(m + 16..m + 16 + self.byte_len()?)
                .ok_or("Truncated fixed ability modifier")?;
        }
        if found != 1 {
            return Err("Fixed ability modifier requires one matching stock property".into());
        }
        Ok(())
    }
}
