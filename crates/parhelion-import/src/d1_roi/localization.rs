use super::*;

pub(super) struct Labels {
    banks: Vec<u32>,
    values: BTreeMap<usize, BTreeMap<u32, String>>,
}

impl Labels {
    pub fn new(source: &Source) -> Result<Self> {
        let tags = source.classes(0x808034CB);
        ensure!(tags.len() == 1, "Missing or ambiguous source string banks");
        let table = source.read(tags[0])?;
        let banks = table
            .array(8, 24, Some(0x80803473))?
            .into_iter()
            .map(|row| table.u32(row + 16))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            banks,
            values: BTreeMap::new(),
        })
    }

    pub fn text(&mut self, source: &Source, strings: &Payload, offset: usize) -> Result<String> {
        self.optional_text(source, strings, offset)?
            .with_context(|| {
                format!(
                    "Source label {:08X} is missing from bank {}",
                    strings.u32(offset + 4).unwrap_or_default(),
                    strings.u32(offset).unwrap_or_default()
                )
            })
    }

    pub fn optional_text(
        &mut self,
        source: &Source,
        strings: &Payload,
        offset: usize,
    ) -> Result<Option<String>> {
        let bank = strings.u32(offset)? as usize;
        let hash = strings.u32(offset + 4)?;
        if matches!(bank, 0xFFFF | 0xFFFFFFFF) || hash == 0x811C9DC5 {
            return Ok(Some(String::new()));
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = self.values.entry(bank) {
            let tag = *self
                .banks
                .get(bank)
                .context("String bank ordinal is outside table")?;
            ensure!(
                source.reference(tag)? == 0x8080035A,
                "Unexpected localized string container"
            );
            let header = source.read(tag)?;
            let hashes = header.array(8, 4, Some(0x80800070))?;
            let data_tag = header.u32(0x18)?;
            ensure!(
                source.reference(data_tag)? == 0x808008BE,
                "Unexpected localized string data"
            );
            let data = source.read(data_tag)?;
            let combos = data.array(0x48, 16, Some(0x8080033E))?;
            let parts = data.array(8, 32, Some(0x8080058C))?;
            let chars = data.array_range(0x38, 1, Some(0x80800005))?;
            ensure!(
                hashes.len() == combos.len(),
                "Localized string table lengths disagree"
            );
            let mut values = BTreeMap::new();
            for (hash, combo) in hashes.into_iter().zip(combos) {
                let text = decode(&data, &parts, &chars, combo)?;
                ensure!(
                    values.insert(header.u32(hash)?, text).is_none(),
                    "Duplicate source string identity"
                );
            }
            entry.insert(values);
        }
        Ok(self.values[&bank].get(&hash).cloned())
    }
}

fn decode(
    data: &Payload,
    parts: &[usize],
    chars: &std::ops::Range<usize>,
    combo: usize,
) -> Result<String> {
    let mut text = String::new();
    let count = usize::try_from(data.u64(combo + 8)?)?;
    if count > 0 {
        let first = parts
            .binary_search(&data.pointer(combo)?)
            .map_err(|_| anyhow::anyhow!("String points outside part table"))?;
        let end = first
            .checked_add(count)
            .context("String part count overflow")?;
        for &part in parts
            .get(first..end)
            .context("String part span exceeds table")?
        {
            if data.u16(part + 0x14)? == 0 {
                continue;
            }
            let start = data.pointer(part + 8)?;
            let end = start
                .checked_add(usize::from(data.u16(part + 0x14)?))
                .context("String character count overflow")?;
            ensure!(
                start >= chars.start && end <= chars.end,
                "String characters exceed their array"
            );
            let shift = u32::from(data.u16(part + 0x18)?);
            for character in std::str::from_utf8(&data.0[start..end])?.chars() {
                text.push(
                    char::from_u32(character as u32 + shift)
                        .context("Invalid shifted Unicode scalar")?,
                );
            }
        }
    }
    Ok(text)
}
