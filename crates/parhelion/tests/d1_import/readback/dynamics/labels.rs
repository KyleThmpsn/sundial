//! Decode both registries and compare effective query predicates and their compiled masks.
use super::*;

pub(super) struct Dictionary {
    bits: BTreeMap<u32, Vec<u8>>,
    atoms: BTreeSet<u32>,
}

pub(super) fn dictionary(p: &Payload, native: bool) -> Result<Dictionary> {
    let width = if native { 40 } else { 32 };
    let mut bits = BTreeMap::new();
    let mut atoms = BTreeSet::new();
    for (index, row) in p.array(8, 4, Some(0x80800070))?.into_iter().enumerate() {
        ensure!(index < width * 8, "Label registry exceeds its mask");
        let key = p.u32(row)?;
        let mut mask = vec![0; width];
        mask[index / 8] = 1 << (index % 8);
        ensure!(bits.insert(key, mask).is_none(), "Repeated atom identity");
        atoms.insert(key);
    }
    for row in p.array(
        24,
        width + 4,
        Some(if native { 0x808094BE } else { 0x80800513 }),
    )? {
        ensure!(
            bits.insert(p.u32(row)?, p.0[row + 4..row + 4 + width].to_vec())
                .is_none(),
            "Repeated group identity"
        );
    }
    Ok(Dictionary { bits, atoms })
}

pub(super) fn check(
    p: &Payload,
    at: usize,
    native: bool,
    source: &Source,
    registry: &Dictionary,
) -> Result<Option<[Vec<u32>; 4]>> {
    let source_dictionary;
    let original = if native {
        ensure!(
            p.u32(at + 0x48)? == 0x80C70CA1,
            "Query names another native registry"
        );
        registry
    } else {
        source_dictionary = dictionary(&source.read(p.u32(at + 0x48)?)?, false)?;
        &source_dictionary
    };
    let mut result: [Vec<u32>; 4] = Default::default();
    let mut possible = true;
    for (operation, names) in result.iter_mut().enumerate() {
        let mut mask = vec![0; if native { 40 } else { 32 }];
        let mut missing = false;
        let mut mapped_rank = false;
        for row in p.array(
            at + operation * 16,
            24,
            Some(if native { 0x808094B3 } else { 0x80800365 }),
        )? {
            let name = p.u32(row)?;
            let Some(bits) = original.bits.get(&name) else {
                ensure!(name == 0x811C9DC5, "Unregistered query label");
                continue;
            };
            for (out, value) in mask.iter_mut().zip(bits) {
                *out |= value;
            }
            if bits.iter().all(|&byte| byte == 0) {
                continue;
            }
            // This is the documented product mapping from D1 majors to native elite.
            // The names are from each registry, not an inferred numerical identity.
            let name = if !native && name == 0xA4FB017C && !registry.bits.contains_key(&name) {
                mapped_rank = true;
                0x574459CE
            } else {
                name
            };
            if registry.bits.contains_key(&name) {
                names.push(name);
            } else {
                ensure!(
                    !native && original.atoms.contains(&name),
                    "Unmapped query label group"
                );
                missing = true;
            }
        }
        if !native {
            ensure!(
                p.0[at + 0x50 + operation * 32..at + 0x50 + (operation + 1) * 32] == mask,
                "Source query mask differs from its names"
            );
            ensure!(
                p.u8(at + 0xD0 + operation)? == u8::from(mask.iter().all(|&byte| byte == 0)),
                "Source query empty flag differs"
            );
        }
        if mapped_rank {
            let mut seen_rank = false;
            names.retain(|&name| {
                if name != 0x574459CE {
                    return true;
                }
                let first = !seen_rank;
                seen_rank = true;
                first
            });
        }
        if missing {
            match operation {
                0 => possible &= !names.is_empty(),
                1 => possible = false,
                3 => names.clear(),
                _ => {}
            }
        }
    }
    if native {
        compiled(p, at, &result, registry)?;
    }
    Ok(possible.then_some(result))
}

fn compiled(p: &Payload, at: usize, names: &[Vec<u32>; 4], registry: &Dictionary) -> Result<()> {
    let mode = p.u8(at + 0x50)?;
    let operations: &[usize] = match mode {
        0 => &[0, 2],
        1 => &[0, 1, 2, 3],
        255 => &[],
        _ => anyhow::bail!("Unchecked native query label mode {mode}"),
    };
    let predicate = if operations.is_empty() {
        0
    } else {
        p.pointer(at + 0x58)?
    };
    for (operation, list) in names.iter().enumerate() {
        let Some(index) = operations.iter().position(|&v| v == operation) else {
            ensure!(list.is_empty(), "Query label operation has no native mask");
            continue;
        };
        let mut expected = [0; 40];
        for name in list {
            let mask = registry
                .bits
                .get(name)
                .context("Missing native query label")?;
            for (out, value) in expected.iter_mut().zip(mask) {
                *out |= value;
            }
        }
        ensure!(
            p.bytes::<40>(predicate + index * 40)? == expected,
            "Native query mask differs from its names"
        );
        ensure!(
            p.u8(predicate + operations.len() * 40 + index)? == u8::from(list.is_empty()),
            "Native query empty flag differs"
        );
    }
    Ok(())
}
