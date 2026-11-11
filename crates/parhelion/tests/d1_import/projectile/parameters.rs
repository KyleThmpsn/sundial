//! Decode child creation parameters from original and emitted movement definitions.
use super::*;

fn read(p: &Payload, old: bool) -> Result<BTreeMap<(u32, u32), Value>> {
    let d = p.pointer(24)?;
    let mut values = BTreeMap::new();
    for row in p.array(
        d + if old { 0x1A0 } else { 0x198 },
        if old { 136 } else { 120 },
        None,
    )? {
        let header = row + if old { 48 } else { 32 };
        let key = (
            p.u32(row + if old { 120 } else { 104 })?,
            p.u32(row + if old { 128 } else { 112 })?,
        );
        let code = p
            .array(header, 1, None)?
            .into_iter()
            .map(|at| p.u8(at))
            .collect::<Result<Vec<_>>>()?;
        let constants = p
            .array(header + 16, 16, None)?
            .into_iter()
            .map(|at| {
                p.0.get(at..at + 16)
                    .map(hex::encode)
                    .context("Child parameter constant extent")
            })
            .collect::<Result<Vec<_>>>()?;
        let names = p
            .array(header + 48, if old { 48 } else { 40 }, None)?
            .into_iter()
            .map(|at| p.u32(at + if old { 40 } else { 32 }))
            .collect::<Result<Vec<_>>>()?;
        ensure!(values.insert(key,json!({"code":code,"constants":constants,"inputs":names,"counts":[p.u64(header+32)?,p.u64(header+40)?]})).is_none(), "Ambiguous child parameter output");
    }
    Ok(values)
}

/// Every source child parameter reaches the staged movement unchanged, unless `unread` finds that
/// nothing the source movement creates mentions its name.
pub(super) fn check(
    source: &Payload,
    stock: &Payload,
    staged: &Payload,
    unread: impl Fn(u32) -> Result<bool>,
) -> Result<Value> {
    let original = read(source, true)?;
    let carrier = read(stock, false)?;
    let emitted = read(staged, false)?;
    let mut omitted = Vec::new();
    for (key, program) in &original {
        if !emitted.contains_key(key) && !carrier.contains_key(key) && unread(key.1)? {
            omitted.push(json!({"namespace":key.0,"name":key.1}));
            continue;
        }
        ensure!(
            emitted.get(key) == Some(program),
            "Staged child parameter lost its source output or program"
        );
    }
    ensure!(
        emitted == carrier,
        "Retained child creation parameters differ from the native carrier"
    );
    Ok(
        json!({"source_count":original.len(),"native_count":emitted.len(),"omitted_unread":omitted,"additional_native":emitted.keys().filter(|key| !original.contains_key(key)).map(|(namespace,name)|json!({"namespace":namespace,"name":name})).collect::<Vec<_>>()}),
    )
}
