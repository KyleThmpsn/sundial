//! Independently derive native prerequisites from emitted component records.
use super::*;

pub(super) fn references(
    base: &PackageManager,
    staged: &PackageManager,
    payload: &Payload,
    class: u32,
) -> Result<BTreeSet<u32>> {
    let mut typed = BTreeSet::new();
    if class == 0x80809C0F {
        for row in payload.array(0x58, 40, Some(0x80809C22))? {
            typed.insert((payload.u32(row + 32)?, 0x80809C54));
        }
    } else {
        ensure!(class == 0x80809C36, "unsupported component loading class");
        let layout = payload.u32(0x44)?;
        if ![0, u32::MAX, 0x811C9DC5].contains(&layout) {
            typed.insert((layout, 0x80809BBB));
        }
        for field in [16, 24] {
            if payload.u64(field)? == 0 {
                continue;
            }
            let data = payload.pointer(field)?;
            let schema = payload.u32(data.checked_sub(4).context("component class header")?)?;
            if schema & 0xFFF00000 != 0x80800000 {
                typed.insert((schema, 0x80800000));
            }
        }
        if payload.u64(24)? != 0 {
            let data = payload.pointer(24)?;
            if payload.u32(data.checked_sub(4).context("component data header")?)? == 0x8080393B {
                payload.bytes::<0x290>(data)?;
                for field in [0x50, 0x1B0, 0x1C8, 0x240, 0x258, 0x270] {
                    typed.insert((payload.u32(data + field)?, 0x80809C54));
                }
            }
        }
    }
    for &(tag, expected) in &typed {
        let entry = staged
            .get_entry(TagHash(tag))
            .or_else(|| base.get_entry(TagHash(tag)))
            .with_context(|| format!("component prerequisite {tag:08X} is unavailable"))?;
        ensure!(
            entry.file_type == 8 && entry.reference == expected,
            "component prerequisite {tag:08X} has an unexpected class"
        );
    }
    Ok(typed.into_iter().map(|(tag, _)| tag).collect())
}
