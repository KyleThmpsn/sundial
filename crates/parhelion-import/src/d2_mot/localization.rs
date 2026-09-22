//! Modern investment names: item strings -> bank index -> locale -> hash ordinal.
//! Layout references: Charm Tiger/Schema/{Investment,Strings}, checked against
//! the installed Edge of Fate packages. Source packages are only read.
use crate::d2_mot::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub struct Resolver {
    banks: BTreeMap<usize, StringBank>,
    values: BTreeMap<(usize, u32), String>,
}

struct StringBank {
    data: Arc<Payload>,
    combos: BTreeMap<u32, usize>,
    parts: Vec<usize>,
    characters: std::ops::Range<usize>,
}

impl Resolver {
    /// Resolve English labels with each bank indexed only once per scan.
    pub fn label(&mut self, r: &mut Reader, strings: &Payload, field: usize) -> Result<String> {
        let index = strings.u32(field)? as usize;
        let hash = strings.u32(field + 4)?;
        if index == 0xffff || hash == 0x811C9DC5 {
            return Ok(String::new());
        }
        if let Some(value) = self.values.get(&(index, hash)) {
            return Ok(value.clone());
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = self.banks.entry(index) {
            let (tag, _) = bank(r, index)?;
            let header = r.tag(tag, Some(0x808099EF))?;
            let hashes = header.array(8, 4, Some(0x80800070))?;
            let data = r.tag(header.u32(0x18)?, Some(0x808099F1))?;
            let rows = data.array(0x38, 16, Some(0x808099F5))?;
            ensure!(
                hashes.len() == rows.len(),
                "Source string table sizes differ"
            );
            let mut combos = BTreeMap::new();
            for (at, combo) in hashes.into_iter().zip(rows) {
                ensure!(
                    combos.insert(header.u32(at)?, combo).is_none(),
                    "Duplicate source string hash"
                );
            }
            entry.insert(StringBank {
                parts: data.array(8, 32, Some(0x808099F7))?,
                characters: data.array_range(0x28, 1, None)?,
                data,
                combos,
            });
        }
        let bank = &self.banks[&index];
        let combo = *bank.combos.get(&hash).context("Source string is missing")?;
        let value = decode_parts(&bank.data, combo, &bank.parts, &bank.characters)?;
        self.values.insert((index, hash), value.clone());
        Ok(value)
    }
}

fn table(r: &mut Reader, class: u32) -> Result<std::sync::Arc<Payload>> {
    let tags = r.classes(class);
    ensure!(tags.len() == 1, "ambiguous localization table {class:08X}");
    r.tag(tags[0], Some(class))
}

pub fn item_strings(r: &mut Reader, hash: u32, index: usize) -> Result<u32> {
    let p = table(r, 0x80805499)?;
    let rows = p.array(8, 32, Some(0x8080549D))?;
    let row = *rows.get(index).context("item-string index outside table")?;
    ensure!(p.u32(row)? == hash, "item-string identity mismatch");
    r.ref64(&p, row + 16)
}

fn bank(r: &mut Reader, index: usize) -> Result<(u32, Value)> {
    let p = table(r, 0x80805A09)?;
    let rows = p.array(8, 32, Some(0x80805A0E))?;
    let row = *rows
        .get(index)
        .context("localized bank index outside table")?;
    let key = p.u32(row)?;
    // A zero Tag64 is a deliberate indirection, not a missing hash lookup.
    let direct = r.ref64(&p, row + 8)?;
    if direct != 0 {
        return Ok((
            direct,
            json!({"index":index,"key":format!("{key:08X}"),"route":"direct"}),
        ));
    }
    let other = p.u16(row + 24)? as usize;
    let secondary = table(r, 0x8080BA26)?;
    let rows = secondary.array(8, 32, Some(0x8080BA2C))?;
    let row = *rows
        .get(other)
        .context("secondary localized bank index outside table")?;
    ensure!(
        secondary.u32(row)? == key,
        "secondary localized bank identity mismatch"
    );
    Ok((
        r.ref64(&secondary, row + 16)?,
        json!({"index":index,"key":format!("{key:08X}"),"route":"secondary","secondary_index":other}),
    ))
}

/// Outcome of a checked source string lookup.
pub enum Text {
    Found(String),
    /// A deliberate empty reference.
    Empty,
    /// The bank resolved but carries no such string. Source data drops the
    /// text of retired entries while leaving their display rows in place.
    Absent,
}

/// Resolves a checked source string reference, including lore display text.
pub fn text(r: &mut Reader, bank_index: usize, hash: u32, locale: usize) -> Result<Text> {
    ensure!(locale < 14, "Invalid source language index");
    if bank_index == 0xffff || hash == 0x811C9DC5 {
        return Ok(Text::Empty);
    }
    let (tag, _) = bank(r, bank_index)?;
    let header = r.tag(tag, Some(0x808099EF))?;
    let hashes = header.array(8, 4, Some(0x80800070))?;
    let found = hashes
        .iter()
        .enumerate()
        .filter_map(|(index, offset)| (header.u32(*offset).ok() == Some(hash)).then_some(index))
        .collect::<Vec<_>>();
    let [index] = found.as_slice() else {
        ensure!(
            found.is_empty(),
            "Source string {hash:08X} is ambiguous in bank {bank_index}"
        );
        return Ok(Text::Absent);
    };
    let data = r.tag(header.u32(0x18 + locale * 4)?, Some(0x808099F1))?;
    let combos = data.array(0x38, 16, Some(0x808099F5))?;
    ensure!(
        hashes.len() == combos.len(),
        "Source string table sizes differ"
    );
    Ok(Text::Found(decode(&data, combos[*index])?))
}

pub fn item_name(r: &mut Reader, hash: u32, index: usize, locale: usize) -> Result<Value> {
    item_label(r, hash, index, locale, 0x80)
}

pub fn item_label(
    r: &mut Reader,
    hash: u32,
    index: usize,
    locale: usize,
    field: usize,
) -> Result<Value> {
    ensure!(locale < 14, "locale index must be in 0..14 (English is 0)");
    let strings_tag = item_strings(r, hash, index)?;
    let strings = r.tag(strings_tag, Some(0x8080549F))?;
    let bank_index = strings.u32(field)? as usize;
    let name_hash = strings.u32(field + 4)?;
    if bank_index == 0xFFFF || name_hash == 0x811C9DC5 {
        return Ok(
            json!({"name":null,"status":"empty","item_hash":hash,"strings_tag":format!("{strings_tag:08X}"),"locale_index":locale}),
        );
    }
    let (header_tag, bank) = bank(r, bank_index)?;
    let header = r.tag(header_tag, Some(0x808099EF))?;
    let hashes = header.array(8, 4, Some(0x80800070))?;
    let matches = hashes
        .iter()
        .enumerate()
        .filter_map(|(i, &o)| (header.u32(o).ok() == Some(name_hash)).then_some(i))
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "name hash {name_hash:08X} missing or duplicated in bank {header_tag:08X}"
    );
    let locale_tag = header.u32(0x18 + locale * 4)?;
    let data = r.tag(locale_tag, Some(0x808099F1))?;
    let combos = data.array(0x38, 16, Some(0x808099F5))?;
    ensure!(
        hashes.len() == combos.len(),
        "localized hash/combo count mismatch"
    );
    let ordinal = matches[0];
    let name = decode(&data, combos[ordinal])?;
    Ok(
        json!({"name":name,"item_hash":hash,"strings_tag":format!("{strings_tag:08X}"),"name_hash":format!("{name_hash:08X}"),"bank":bank,"header_tag":format!("{header_tag:08X}"),"locale_tag":format!("{locale_tag:08X}"),"locale_index":locale,"ordinal":ordinal}),
    )
}

/// Discover candidates by the same source-authored localized item-type key.
/// This is a catalog filter, not a claim that every candidate can be converted.
pub fn same_type(r: &mut Reader, hash: u32) -> Result<Value> {
    let (index, _) = crate::d2_mot::assets::item::find(r, hash)?;
    let tag = item_strings(r, hash, index)?;
    let reference = r.tag(tag, Some(0x8080549F))?;
    let key = (reference.u32(0x8C)?, reference.u32(0x90)?);
    ensure!(
        key.0 != 0xFFFF && key.1 != 0x811C9DC5,
        "source has no item type"
    );
    let p = table(r, 0x80805499)?;
    let mut candidates = Vec::new();
    let mut unavailable = Vec::new();
    for (index, row) in p.array(8, 32, Some(0x8080549D))?.into_iter().enumerate() {
        let strings = match r
            .ref64(&p, row + 16)
            .and_then(|tag| r.tag(tag, Some(0x8080549F)))
        {
            Ok(strings) => strings,
            Err(error) => {
                unavailable
                    .push(json!({"index":index,"hash":p.u32(row)?,"error":format!("{error:#}")}));
                continue;
            }
        };
        if (strings.u32(0x8C)?, strings.u32(0x90)?) == key {
            let hash = p.u32(row)?;
            match item_name(r, hash, index, 0) {
                Ok(name) => candidates.push(json!({"hash":hash,"index":index,"name":name["name"]})),
                Err(error) => unavailable
                    .push(json!({"index":index,"hash":hash,"error":format!("name: {error:#}")})),
            }
        }
    }
    Ok(
        json!({"reference_item":hash,"type_key":key,"candidates":candidates,"unavailable":unavailable,"conversion_verified":false}),
    )
}

pub fn lookup_hashes(r: &mut Reader, wanted: &[u32]) -> Result<Value> {
    let mut result = serde_json::Map::new();
    let mut unreadable = Vec::new();
    for tag in r.classes(0x808099EF) {
        let header = match r.tag(tag, None) {
            Ok(header) => header,
            Err(error) => {
                unreadable.push(json!({"tag":format!("{tag:08X}"),"error":error.to_string()}));
                continue;
            }
        };
        let hashes = header.array(8, 4, None)?;
        let matching = hashes
            .iter()
            .enumerate()
            .filter_map(|(i, &o)| {
                let h = header.u32(o).ok()?;
                wanted.contains(&h).then_some((i, h))
            })
            .collect::<Vec<_>>();
        if matching.is_empty() {
            continue;
        }
        let data = r.tag(header.u32(0x18)?, None)?;
        let combos = data.array(0x38, 16, None)?;
        for (i, h) in matching {
            result.insert(format!("{h:08X}"), json!({"text":decode(&data, *combos.get(i).context("string ordinal")?)?,"bank":format!("{tag:08X}")}));
        }
    }
    Ok(json!({"strings":result,"unreadable_banks":unreadable}))
}

fn decode(p: &Payload, combo: usize) -> Result<String> {
    let parts = p.array(8, 32, Some(0x808099F7))?;
    let characters = p.array_range(0x28, 1, None)?;
    decode_parts(p, combo, &parts, &characters)
}

fn decode_parts(
    p: &Payload,
    combo: usize,
    parts: &[usize],
    characters: &std::ops::Range<usize>,
) -> Result<String> {
    let count = usize::try_from(p.u64(combo + 8)?)?;
    if count == 0 {
        return Ok(String::new());
    }
    let first = p.pointer(combo)?;
    let start = parts
        .binary_search(&first)
        .map_err(|_| anyhow::anyhow!("combo points outside or between string parts"))?;
    let end = start.checked_add(count).context("combo extent overflow")?;
    let selected = parts
        .get(start..end)
        .context("combo exceeds string parts")?;
    let mut result = String::new();
    for &part in selected {
        let start = p.pointer(part + 8)?;
        let length = p.u16(part + 0x14)? as usize;
        if length == 0 {
            continue;
        }
        let end = start
            .checked_add(length)
            .context("character extent overflow")?;
        ensure!(
            !characters.is_empty() && start >= characters.start && end <= characters.end,
            "string points outside character array"
        );
        let bytes = p.0.get(start..end).context("truncated characters")?;
        let text = std::str::from_utf8(bytes).context("invalid localized UTF-8")?;
        let shift = u32::from(p.u16(part + 0x18)?);
        for c in text.chars() {
            result
                .push(char::from_u32(c as u32 + shift).context("invalid shifted Unicode scalar")?);
        }
    }
    Ok(result)
}
