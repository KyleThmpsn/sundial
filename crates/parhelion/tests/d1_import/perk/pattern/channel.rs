//! Read a constant status publishing to a bank without inherited declarations.
use super::*;

fn lookup(p: &Payload, field: usize) -> CheckResult<Vec<[u32; 3]>> {
    p.array(field, 12, None)?
        .into_iter()
        .map(|row| Ok([p.u32(row)?, p.u32(row + 4)?, p.u32(row + 8)?]))
        .collect()
}

fn declarations(source: &Payload, target: &Payload) -> CheckResult<()> {
    let s = source.pointer(24)?;
    let n = definition(target)?;
    assert_eq!(source.u32(s - 4)?, 0x808008F6);
    assert_eq!(target.u32(n - 4)?, 0x80809790);
    assert_eq!(source.u64(s + 0x108)?, 0);
    assert_eq!(target.u64(n + 0x108)?, 0);
    for field in [0xF8, 0x108, 0x118] {
        assert_eq!(lookup(source, s + field)?, lookup(target, n + field)?);
    }
    let original = source.array(s + 0xD8, 112, Some(0x80800883))?;
    let emitted = target.array(n + 0xD8, 112, Some(0x808097A1))?;
    assert!(!original.is_empty());
    assert_eq!(original.len(), emitted.len());
    for (s, n) in original.into_iter().zip(emitted) {
        assert_eq!(source.u32(s)?, target.u32(n)?);
        for field in [8, 24, 56, 96] {
            assert_eq!(source.u64(s + field)?, 0);
            assert_eq!(target.u64(n + field)?, 0);
        }
        let words = |p: &Payload, at| -> CheckResult<Vec<u32>> {
            p.array(at + 80, 4, Some(0x8080000B))?
                .into_iter()
                .map(|row| p.u32(row).map_err(Into::into))
                .collect()
        };
        assert_eq!(words(source, s)?, words(target, n)?);
        assert_eq!(source.u16(s + 0x6A)?, target.u16(n + 0x6A)?);
    }
    Ok(())
}

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    reader: &mut Reader,
    graph: &Payload,
    native_tag: u32,
) -> CheckResult<()> {
    assert!(stock.tag(native_tag, None).is_err());
    let native = reader.tag(native_tag, Some(0x80809C0F))?;
    let originals = graph.array(32, 12, Some(0x80800715))?;
    let targets = native.array(16, 12, Some(0x80809C04))?;
    assert_eq!((originals.len(), targets.len()), (2, 2));
    for (ordinal, (s, n)) in originals.into_iter().zip(targets).enumerate() {
        let original = source.read(graph.u32(s)?)?;
        let tag = native.u32(n)?;
        assert!(stock.tag(tag, None).is_err());
        let target = reader.tag(tag, Some(0x80809C36))?;
        if ordinal == 0 {
            assert_eq!(original.u32(original.pointer(24)? - 4)?, 0x80802698);
            // Count, Duration, competition, HUD identity, enabled display and reciprocal
            // instances are the same observable status contract as the standalone form.
            status_fields(source, stock, reader, &original, &native, &target, 2)?;
        } else {
            declarations(&original, &target)?;
        }
    }
    // The bank's incoming name must resolve to the same actual status getter. This follows
    // the emitted endpoints and declaration ordinals independently of conversion metadata.
    let links = crate::readback::check_graph_dynamics(source, reader, graph, &native)?;
    assert!(
        links["connections"]
            .as_array()
            .is_some_and(|v| !v.is_empty())
    );
    fs::write(
        reader
            .output
            .join(format!("attached-channel-{native_tag:08X}.json")),
        serde_json::to_vec_pretty(&links)?,
    )?;
    Ok(())
}
