//! Source selectors use eight mask bytes. Parameter identities, not bit ordinals, cross eras.
use super::*;

pub(in crate::d1_roi::native::animation) fn domains(
    p: &Payload,
    source: bool,
) -> Result<BTreeMap<u8, BTreeMap<u32, Vec<u8>>>> {
    let rows = p.array(8, 32, Some(if source { 0x80800663 } else { 0x80808EE5 }))?;
    let width = if source { 8 } else { 16 };
    let mut result = BTreeMap::new();
    for builtin in 0..if source { 15u8 } else { 24u8 } {
        let index = p.u16(24 + usize::from(builtin) * 2)?;
        if index == u16::MAX {
            continue;
        }
        let row = *rows
            .get(usize::from(index))
            .context("Parameter group index")?;
        let mut names = BTreeMap::new();
        for at in p.array(
            row + 16,
            width + 8,
            Some(if source { 0x80800567 } else { 0x80808EE8 }),
        )? {
            ensure!(
                names
                    .insert(p.u32(at + width)?, p.0[at..at + width + 4].to_vec())
                    .is_none(),
                "Duplicate parameter name"
            );
        }
        result.insert(builtin, names);
    }
    Ok(result)
}

fn chosen(
    p: &Payload,
    rows: &[usize],
    width: usize,
    index: usize,
    mask: &[u8],
    active: &mut BTreeSet<usize>,
) -> Result<Option<i16>> {
    ensure!(active.insert(index), "Cyclic source selector");
    let row = *rows.get(index).context("Selector record index")?;
    let result = if p.0[row + width..row + width + 4] == mask[width..width + 4] {
        Some(p.i16(row + width + 8)?)
    } else {
        let matches = p.u8(row + width - 1)? & mask[width - 1] & 128 != 0
            && p.0[row..row + width]
                .iter()
                .zip(mask)
                .all(|(&a, &b)| a & !b == 0);
        let child = p.i16(row + width + 4)?;
        let alternate = p.i16(row + width + 6)?;
        ensure!(child >= -1 && alternate >= -1, "Invalid selector child");
        let refined = if matches && child >= 0 {
            chosen(p, rows, width, child as usize, mask, active)?
        } else {
            None
        };
        if matches && refined.is_none() {
            Some(p.i16(row + width + 8)?)
        } else if alternate >= 0 {
            chosen(p, rows, width, alternate as usize, mask, active)?.or(refined)
        } else {
            refined
        }
    };
    active.remove(&index);
    Ok(result)
}

fn select(p: &Payload, source: bool, fixed: &BTreeMap<u8, Vec<u8>>) -> Result<usize> {
    select_at(p, 8, source, fixed)
}

pub(in crate::d1_roi::native::animation) fn select_at(
    p: &Payload,
    base: usize,
    source: bool,
    fixed: &BTreeMap<u8, Vec<u8>>,
) -> Result<usize> {
    let width = if source { 8 } else { 16 };
    let records = p.array(
        base + 16,
        width + 12,
        Some(if source { 0x80802B27 } else { 0x8080347D }),
    )?;
    let ops = p.array(
        base + 32,
        4,
        Some(if source { 0x808022C3 } else { 0x8080347C }),
    )?;
    let mut index = 0;
    let mut visited = BTreeSet::new();
    loop {
        ensure!(visited.insert(index), "Cyclic source selector operation");
        let op = p.u32(*ops.get(index).context("Selector operation")?)?;
        ensure!(op & 0xFF00 == 0, "Unknown selector operation flags");
        let root = (op >> 16) as usize;
        let mask = fixed
            .get(&(op as u8))
            .context("Untranslated selector parameter")?;
        let value = chosen(p, &records, width, root, mask, &mut BTreeSet::new())?
            .unwrap_or(p.i16(*records.get(root).context("Selector root")? + width + 8)?);
        if value < 0 {
            return Ok((!value) as usize);
        }
        ensure!(value > 0, "Zero selector operation");
        index = value as usize - 1;
    }
}

/// Survey every shared parameter context. Ambiguous row pairs remain native.
pub(super) fn pairs(
    sd: &Payload,
    nd: &Payload,
    ss: &Payload,
    ns: &Payload,
    source_profile: u32,
    native_profile: u32,
) -> Result<BTreeMap<usize, BTreeSet<usize>>> {
    let source = domains(sd, true)?;
    let native = domains(nd, false)?;
    let mut source_fixed = BTreeMap::from([(
        3,
        source[&3]
            .get(&source_profile)
            .context("Source weapon profile")?
            .clone(),
    )]);
    let mut native_fixed = BTreeMap::from([(
        3,
        native[&3]
            .get(&native_profile)
            .context("Native weapon profile")?
            .clone(),
    )]);
    let mapping = [
        (0, 0),
        (1, 1),
        (2, 2),
        (5, 10),
        (6, 11),
        (9, 14),
        (11, 20),
        (12, 21),
        (13, 22),
    ];
    let sp = ss
        .array(8, 1, Some(0x8080440D))?
        .into_iter()
        .map(|r| ss.u8(r))
        .collect::<Result<BTreeSet<_>>>()?;
    let np = ns
        .array(8, 1, Some(0x80808DA4))?
        .into_iter()
        .map(|r| ns.u8(r))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        sp.iter()
            .all(|&p| p == 3 || mapping.iter().any(|&(s, _)| s == p))
            && np
                .iter()
                .all(|&p| p == 3 || mapping.iter().any(|&(_, n)| n == p)),
        "Stance uses an untranslated parameter domain"
    );
    let mut contexts = Vec::new();
    let mut count = 1usize;
    for (s, n) in mapping {
        if !sp.contains(&s) && !np.contains(&n) {
            continue;
        }
        let values = source[&s]
            .keys()
            .filter(|name| native[&n].contains_key(name))
            .filter(|&&name| s != 1 || matches!(name, 0x4866DB53 | 0x9E464D07))
            .copied()
            .collect::<Vec<_>>();
        ensure!(!values.is_empty(), "No shared stance parameter values");
        count = count
            .checked_mul(values.len())
            .context("Stance context count overflow")?;
        ensure!(count <= 100_000, "Stance context domain is too large");
        contexts.push((s, n, values));
    }
    let mut output = BTreeMap::<usize, BTreeSet<usize>>::new();
    for mut index in 0..count {
        for (s, n, values) in &contexts {
            let name = values[index % values.len()];
            index /= values.len();
            source_fixed.insert(*s, source[s][&name].clone());
            native_fixed.insert(*n, native[n][&name].clone());
        }
        output
            .entry(select(ns, false, &native_fixed)?)
            .or_default()
            .insert(select(ss, true, &source_fixed)?);
    }
    Ok(output)
}
