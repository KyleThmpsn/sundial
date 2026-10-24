//! Follow ordered source artwork alternatives through the actual emitted item.
use super::*;

fn slots(reader: &mut Reader, item: u32, modern: bool) -> Result<BTreeMap<u64, Vec<u32>>> {
    let class = if modern { 0x808055CE } else { 0x80805DF5 };
    let tags = reader.classes(class);
    ensure!(tags.len() == 1, "Art metadata is missing or ambiguous");
    let metadata = reader.tag(tags[0], None)?;
    let rows = metadata.array(8, 32, None)?;
    let item = reader.tag(item, None)?;
    let mut result = BTreeMap::new();
    for entry in item.array(item.pointer(if modern { 0x70 } else { 0x88 })?, 4, None)? {
        let row = *rows
            .get(usize::from(item.u16(entry + 2)?))
            .context("Art index")?;
        for slot in metadata.array(row + 16, 8, None)? {
            let at = metadata.pointer(slot)?;
            let keys = metadata
                .array(at + 8, 4, None)?
                .into_iter()
                .map(|r| metadata.u32(r))
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                result.insert(metadata.u64(at)?, keys).is_none(),
                "Ambiguous art selector"
            );
        }
    }
    Ok(result)
}

pub(super) fn check(source: &mut Reader, staged: &mut Reader, from: u32, to: u32) -> Result<Value> {
    let original = slots(source, from, true)?;
    let emitted = slots(staged, to, false)?;
    let mut bindings = BTreeMap::new();
    let mut evidence = Vec::new();
    for (selector, keys) in original.into_iter().filter(|(_, keys)| keys.len() > 1) {
        let native = match selector {
            0..=5 | 9..=12 => selector,
            17..=22 => selector + 4,
            _ => anyhow::bail!("Source selector has no independent mapping witness"),
        };
        let actual = emitted.get(&native).context("Source art region vanished")?;
        ensure!(
            actual.len() == keys.len(),
            "Source art region width changed"
        );
        for (&key, &private) in keys.iter().zip(actual) {
            if matches!(key, 0 | u32::MAX | 0x811C9DC5) {
                ensure!(key == private, "Empty source art position changed");
            } else {
                ensure!(
                    !matches!(private, 0 | u32::MAX | 0x811C9DC5),
                    "Source alternative vanished"
                );
                if let Some(previous) = bindings.insert(key, private) {
                    ensure!(
                        previous == private,
                        "Repeated source assignment no longer shares its part"
                    );
                }
            }
        }
        evidence.push(json!({"source_selector":selector,"native_selector":native,
            "source_assignments":keys,"emitted_assignments":actual}));
    }
    ensure!(!evidence.is_empty(), "Required art variant corpus is empty");
    ensure!(
        bindings.values().collect::<BTreeSet<_>>().len() == bindings.len(),
        "Different source parts collapsed to the same assignment"
    );
    Ok(json!(evidence))
}
