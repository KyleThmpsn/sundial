//! Checked hk_2012.2.0-r1 section relocation. Offsets address serialized storage,
//! never host pointers. Class names come from virtual fixups, not byte scanning.
use crate::package_payload::{bytes_at, u16_at, u32_at, u64_at};
use std::collections::BTreeMap;

pub(super) struct Pack<'a> {
    bytes: &'a [u8],
    sections: Vec<Section>,
    pointers: BTreeMap<usize, usize>,
    classes: BTreeMap<usize, String>,
    pub root: usize,
}

struct Section {
    start: usize,
    offsets: [usize; 6],
}

impl<'a> Pack<'a> {
    pub fn read(bytes: &'a [u8]) -> Result<Self, String> {
        if bytes.len() > 32 * 1024 * 1024
            || bytes.get(..8) != Some(&[0x57, 0xE0, 0xE0, 0x57, 0x10, 0xC0, 0xC0, 0x10])
            || u32_at(bytes, 12)? != 9
            || bytes.get(16..20) != Some(&[8, 1, 0, 1])
            || bytes.get(40..55) != Some(b"hk_2012.2.0-r1\0")
        {
            return Err("The cloth solver requires a 64-bit Havok 2012.2 packfile".into());
        }
        let count = u32_at(bytes, 20)? as usize;
        if !(1..=8).contains(&count) {
            return Err("Invalid cloth section count".into());
        }
        let mut result = Self {
            bytes,
            sections: Vec::new(),
            pointers: BTreeMap::new(),
            classes: BTreeMap::new(),
            root: 0,
        };
        for i in 0..count {
            let at = 64 + i * 48 + 20;
            let start = u32_at(bytes, at)? as usize;
            let mut offsets = [0; 6];
            for (j, value) in offsets.iter_mut().enumerate() {
                *value = u32_at(bytes, at + 4 + j * 4)? as usize;
            }
            if start < 64 + count * 48
                || !offsets.windows(2).all(|w| w[0] <= w[1])
                || start
                    .checked_add(offsets[5])
                    .is_none_or(|end| end > bytes.len())
                || offsets[3] != offsets[4]
                || offsets[4] != offsets[5]
            {
                return Err("Invalid cloth section extent or external relocation".into());
            }
            if result
                .sections
                .iter()
                .any(|s| start < s.start + s.offsets[5] && s.start < start + offsets[5])
            {
                return Err("Overlapping cloth sections".into());
            }
            result.sections.push(Section { start, offsets });
        }
        result.root =
            result.address(u32_at(bytes, 24)? as usize, u32_at(bytes, 28)? as usize, 16)?;
        let root_class =
            result.address(u32_at(bytes, 32)? as usize, u32_at(bytes, 36)? as usize, 1)?;
        if result.string(root_class)? != "hclClothContainer" {
            return Err("The solver root is not a cloth container".into());
        }
        for i in 0..count {
            let section = &result.sections[i];
            let start = section.start;
            let bounds = section.offsets;
            for (table, stride) in [(0, 8), (1, 12), (2, 12)] {
                let range = start + bounds[table]..start + bounds[table + 1];
                let mut rows = bytes[range].chunks_exact(stride);
                for row in &mut rows {
                    let source = u32_at(row, 0)? as usize;
                    if source == u32::MAX as usize {
                        if row.iter().any(|v| *v != 255) {
                            return Err("Invalid cloth relocation padding".into());
                        }
                        continue;
                    }
                    let source = result.address(i, source, 8)?;
                    let (target_section, target_offset) = if table == 0 {
                        (i, u32_at(row, 4)? as usize)
                    } else {
                        (u32_at(row, 4)? as usize, u32_at(row, 8)? as usize)
                    };
                    let target = result.address(target_section, target_offset, 1)?;
                    if table == 2 {
                        let name = result.string(target)?.to_owned();
                        if result.classes.insert(source, name).is_some() {
                            return Err("Duplicate cloth virtual relocation".into());
                        }
                    } else if result.pointers.insert(source, target).is_some() {
                        return Err("Duplicate cloth pointer relocation".into());
                    }
                }
                if rows.remainder().iter().any(|v| *v != 255) {
                    return Err("Invalid cloth relocation tail".into());
                }
            }
        }
        result.expect(result.root, "hclClothContainer", 0x30)?;
        Ok(result)
    }

    fn address(&self, section: usize, offset: usize, length: usize) -> Result<usize, String> {
        let section = self.sections.get(section).ok_or("Missing cloth section")?;
        if offset
            .checked_add(length)
            .is_none_or(|end| end > section.offsets[0])
        {
            return Err("Cloth relocation exceeds its data section".into());
        }
        Ok(section.start + offset)
    }

    pub fn span(&self, at: usize, length: usize) -> Result<&'a [u8], String> {
        if !self.sections.iter().any(|s| {
            at >= s.start
                && at
                    .checked_add(length)
                    .is_some_and(|end| end <= s.start + s.offsets[0])
        }) {
            return Err("Cloth field exceeds its data section".into());
        }
        self.bytes
            .get(at..at + length)
            .ok_or_else(|| "Truncated cloth field".into())
    }

    pub fn string(&self, at: usize) -> Result<&'a str, String> {
        let section = self
            .sections
            .iter()
            .find(|s| at >= s.start && at < s.start + s.offsets[0])
            .ok_or("Cloth string is outside its data section")?;
        let bytes = &self.bytes[at..(section.start + section.offsets[0]).min(at + 512)];
        let end = bytes
            .iter()
            .position(|c| *c == 0)
            .ok_or("Unterminated cloth string")?;
        std::str::from_utf8(&bytes[..end]).map_err(|_| "Invalid cloth string".into())
    }

    pub fn pointer(&self, at: usize) -> Result<Option<usize>, String> {
        let raw = u64_at(self.span(at, 8)?, 0)?;
        match self.pointers.get(&at) {
            Some(target) => Ok(Some(*target)),
            None if raw == 0 => Ok(None),
            None => Err("Unresolved cloth pointer".into()),
        }
    }

    pub fn class(&self, at: usize) -> Result<&str, String> {
        self.classes
            .get(&at)
            .map(String::as_str)
            .ok_or_else(|| "Missing cloth virtual type".into())
    }

    pub fn expect(&self, at: usize, class: &str, size: usize) -> Result<(), String> {
        if self.class(at)? != class {
            return Err(format!("Expected {class}, found {}", self.class(at)?));
        }
        self.span(at, size).map(|_| ())
    }

    pub fn array(&self, at: usize, stride: usize, limit: usize) -> Result<Vec<usize>, String> {
        let count = self.u32(at + 8)?;
        if count > limit || count.checked_mul(stride).is_none() {
            return Err("Cloth array exceeds the preview budget".into());
        }
        let target = self.pointer(at)?;
        if count == 0 {
            return Ok(Vec::new());
        }
        let target = target.ok_or("Cloth array has no storage")?;
        self.span(target, count * stride)?;
        Ok((0..count).map(|i| target + i * stride).collect())
    }

    pub fn objects(&self, at: usize, limit: usize) -> Result<Vec<usize>, String> {
        self.array(at, 8, limit)?
            .into_iter()
            .map(|row| self.pointer(row)?.ok_or_else(|| "Null cloth object".into()))
            .collect()
    }
    pub fn u32(&self, at: usize) -> Result<usize, String> {
        Ok(u32_at(self.span(at, 4)?, 0)? as usize)
    }
    pub fn u16(&self, at: usize) -> Result<usize, String> {
        Ok(u16_at(self.span(at, 2)?, 0)? as usize)
    }
    pub fn u8(&self, at: usize) -> Result<usize, String> {
        Ok(self.span(at, 1)?[0] as usize)
    }
    pub fn f32(&self, at: usize) -> Result<f32, String> {
        let value = f32::from_le_bytes(bytes_at(self.span(at, 4)?, 0)?);
        if value.is_finite() {
            Ok(value)
        } else {
            Err("Non-finite cloth value".into())
        }
    }
    pub fn vector<const N: usize>(&self, at: usize) -> Result<[f32; N], String> {
        let mut result = [0.; N];
        for (i, value) in result.iter_mut().enumerate() {
            *value = self.f32(at + i * 4)?;
        }
        Ok(result)
    }
}
