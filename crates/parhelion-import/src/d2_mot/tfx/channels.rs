//! Find actual entity channel declarations across package versions.
use crate::d2_mot::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub fn procedures(r: &mut Reader, code: &[u8], limit: usize, class: u32) -> Result<Value> {
    ensure!(
        code.len() >= 4 && code.len() <= 4096 && (1..=16).contains(&limit),
        "invalid component procedure search"
    );
    ensure!(
        matches!(class, 0x80809C36 | 0x80809C0F),
        "unsupported procedure search class"
    );
    let mut tags = r.classes(class);
    tags.sort_unstable();
    let mut found = vec![];
    let mut examined = 0;
    for tag in tags {
        let Ok(bytes) = r.manager.read_tag(tiger_pkg::TagHash(tag)) else {
            continue;
        };
        examined += 1;
        if !bytes.windows(code.len()).any(|v| v == code) {
            continue;
        }
        let p = r.tag(tag, Some(class))?;
        if class == 0x80809C36 {
            let instance = p.pointer(16)?;
            found.push(json!({"tag":format!("{tag:08X}"),"instance":instance,"schema":p.pointer(24)?,"class":format!("{:08X}",p.u32(instance-4)?)}));
            r.tag(p.u32(0x44)?, None)?;
        } else {
            found.push(json!({"tag":format!("{tag:08X}"),"class":format!("{class:08X}")}));
        }
        if found.len() == limit {
            break;
        }
    }
    Ok(json!({"examined":examined,"matches":found}))
}

/// Native envelopes for ordinary float4 channel banks. Reusing an envelope of
/// exactly the required capacity retains its engine-specific allocation schema.
pub fn templates(r: &mut Reader, counts: &[usize]) -> Result<Value> {
    ensure!(
        !counts.is_empty() && counts.iter().all(|n| (1..=64).contains(n)),
        "invalid native channel capacities"
    );
    let wanted = counts.iter().copied().collect::<BTreeSet<_>>();
    let mut found = BTreeMap::new();
    let mut tags = r.classes(0x80809C36);
    tags.sort_unstable();
    let mut examined = 0;
    for tag in tags {
        let Ok(bytes) = r.manager.read_tag(tiger_pkg::TagHash(tag)) else {
            continue;
        };
        let p = Payload(bytes);
        examined += 1;
        let Ok((instance, schema, rows)) = plain_bank(&p) else {
            continue;
        };
        let n = rows.len();
        if !wanted.contains(&n) || found.contains_key(&n) {
            continue;
        }
        // The namespace-free range covers the entire vector storage.
        if p.u16(schema + 0x128)? != 0 || p.u16(schema + 0x12A)? as usize + 1 != n {
            continue;
        }
        let lookup = p.array(schema + 0x108, 12, Some(0x808097A0))?;
        if lookup.len() != n || lookup.iter().any(|a| p.u32(*a).ok() != Some(0x811C9DC5)) {
            continue;
        }
        r.tag(tag, Some(0x80809C36))?;
        let defaults = p.u32(0x44)?;
        r.tag(defaults, None)?;
        for i in 0..5 {
            r.tag(p.u32(schema + 0x50 + i * 24)?, None)?;
        }
        found.insert(n, json!({"tag":format!("{tag:08X}"),"instance":instance,"schema":schema,"defaults":format!("{defaults:08X}")}));
        if wanted.iter().all(|n| found.contains_key(n)) {
            break;
        }
    }
    Ok(
        json!({"examined":examined,"templates":found,"missing":wanted.into_iter().filter(|n| !found.contains_key(n)).collect::<Vec<_>>()}),
    )
}

pub fn plain_bank(p: &Payload) -> Result<(usize, usize, Vec<usize>)> {
    let instance = p.pointer(16)?;
    let schema = p.pointer(24)?;
    ensure!(
        instance >= 4
            && schema >= 4
            && p.u32(instance - 4)? == 0x8080979F
            && p.u32(schema - 4)? == 0x80809790,
        "native channel bank types differ"
    );
    let rows = p.array(schema + 0xD8, 112, Some(0x808097A1))?;
    ensure!(
        !rows.is_empty() && rows.len() <= 64,
        "native channel bank capacity"
    );
    let vectors = p.array(instance + 0x50, 16, Some(0x80800090))?;
    let states = p.array(instance + 0x90, 16, Some(0x8080979C))?;
    ensure!(
        vectors.len() == rows.len() && states.len() == rows.len(),
        "native channel bank arrays differ"
    );
    let indices = rows
        .iter()
        .map(|row| p.u32(row + 72))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        indices == (0..rows.len() as u32).collect(),
        "native channel vector allocation differs"
    );
    for (i, row) in rows.iter().copied().enumerate() {
        ensure!(
            p.u32(row + 8)? == 0 && p.0[row + 16..row + 72].iter().all(|v| *v == 0),
            "native channel bank contains special defaults"
        );
        let dependencies = p.array(row + 80, 8, Some(0x8080000B))?;
        ensure!(
            dependencies.len() == 1 && p.u64(dependencies[0])? == 1u64 << i,
            "native channel has dependencies"
        );
    }
    ensure!(
        vectors
            .iter()
            .all(|row| p.0[*row..*row + 16].iter().all(|v| *v == 0)),
        "native bank has initialized state"
    );
    ensure!(
        p.u64(instance + 0x40)? == 1 && p.u64(instance + 0xA0)? == 1,
        "native bank bitset width differs"
    );
    ensure!(
        p.u32(schema + 0x134)? as usize == rows.len()
            && p.u32(schema + 0x140)? as usize == rows.len(),
        "native bank field counts differ"
    );
    let defaults = p.pointer(8)?;
    ensure!(
        defaults >= 4 && p.u32(defaults - 4)? == 0x8080978F,
        "native defaults type differs"
    );
    p.0.get(defaults..instance - 8)
        .context("native initial-state extent")?;
    Ok((instance, schema, rows))
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
pub fn search(
    r: &mut Reader,
    modern: bool,
    hashes: &[u32],
    limit: usize,
    numeric: bool,
) -> Result<Value> {
    ensure!(
        !hashes.is_empty() && (1..=16).contains(&limit),
        "invalid channel search"
    );
    let class = if modern { 0x80809B06 } else { 0x80809C36 };
    let row_class = if modern { 0x808095A9u64 } else { 0x808097A1u64 };
    let signature = row_class.to_le_bytes();
    let mut tags = vec![];
    for (&package, entries) in &r.manager.lookup.tag32_entries_by_pkg {
        for (index, entry) in entries.iter().enumerate() {
            if entry.reference == class {
                tags.push(tiger_pkg::TagHash::new(package, u16::try_from(index)?).0);
            }
        }
    }
    tags.sort_unstable();
    let mut found: BTreeMap<u32, Vec<Value>> = hashes.iter().map(|h| (*h, vec![])).collect();
    let mut examined = 0;
    let mut unreadable = 0;
    for tag in tags {
        let p = match r.manager.read_tag(tiger_pkg::TagHash(tag)) {
            Ok(bytes) => Payload(bytes),
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        examined += 1;
        let mut retain = false;
        for at in (8..p.0.len().saturating_sub(7)).step_by(4) {
            if p.0[at..at + 8] != signature {
                continue;
            }
            let count = p.u64(at - 8)?;
            if count == 0 || count > 255 || at + 8 + count as usize * 112 > p.0.len() {
                continue;
            }
            for row in (at + 8..at + 8 + count as usize * 112).step_by(112) {
                let hash = p.u32(row)?;
                // Resource/string channels share the declaration table. Pixel
                // expressions require an actual float4 slot, not those handles.
                if numeric && (p.u32(row + 8)? != 0 || p.u32(row + 72)? > 255) {
                    continue;
                }
                if let Some(matches) = found.get_mut(&hash).filter(|v| v.len() < limit) {
                    matches.push(json!({"component":format!("{tag:08X}"),"offset":row,"index":p.u32(row+72)?,"declaration":hex::encode(&p.0[row..row+112])}));
                    retain = true;
                }
            }
        }
        if retain {
            r.tag(tag, Some(class))?;
        }
        if found.values().all(|v| v.len() >= limit) {
            break;
        }
    }
    Ok(
        json!({"examined":examined,"unreadable":unreadable,"channels":found.into_iter().map(|(k,v)|(format!("{k:08X}"),v)).collect::<BTreeMap<_,_>>()}),
    )
}
