//! Read both owners of a retained duration and modifier graph after portable reload and staging.
use super::*;

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    graph: &Payload,
    tag: u32,
) -> CheckResult<()> {
    assert!(
        stock.tag(tag, None).is_err(),
        "Duration graph must be private"
    );
    let native = emitted.tag(tag, Some(0x80809C0F))?;
    let owners = graph.array(32, 12, Some(0x80800715))?;
    let targets = native.array(16, 12, Some(0x80809C04))?;
    assert_eq!((owners.len(), targets.len()), (2, 2));
    assert_eq!(graph.u64(48)?, 0);
    assert_eq!(graph.u64(64)?, 0);
    assert_eq!(native.u64(48)?, 0);
    let connections = native.array(32, 72, Some(0x80809BC9))?;
    assert_eq!(connections.len(), 1);
    let connection = connections[0];
    assert_eq!(native.u32(connection + 40)?, u32::MAX);
    assert_eq!(native.u64(connection + 48)?, 0);
    for (&original, &converted) in owners.iter().zip(&targets) {
        let original = source.read(graph.u32(original)?)?;
        let tag = native.u32(converted)?;
        assert!(
            stock.tag(tag, None).is_err(),
            "Every connected owner must be private"
        );
        let converted = emitted.tag(tag, Some(0x80809C36))?;
        let (s, n) = (original.pointer(24)?, converted.pointer(24)?);
        if original.u32(s - 4)? == 0x808020DE {
            modifier_rows(
                source,
                stock,
                &original,
                &converted,
                tag,
                original.array(s + 96, 96, Some(0x8080205F))?,
            )?;
            continue;
        }
        assert_eq!(original.u32(s - 4)?, 0x808029C3);
        assert_eq!(converted.u32(n - 4)?, 0x80803B30);
        let (d, nd) = (s + 0x60, n + 0x48);
        assert_eq!(
            original.bytes::<20>(d + 0xA0)?,
            converted.bytes::<20>(nd + 0x98)?
        );
        assert_eq!(
            original.bytes::<8>(d + 0x130)?,
            converted.bytes::<8>(nd + 0x120)?
        );
        // Follow the actual duration expression, including the source's indefinite -1 lifetime.
        let (e, ne) = (d + 0x138, nd + 0x128);
        let code = original.array(e + 24, 1, Some(0x80800009))?;
        let native_code = converted.array(ne + 16, 1, Some(0x80800009))?;
        assert_eq!(code.len(), native_code.len());
        for (s, n) in code.into_iter().zip(native_code) {
            assert_eq!(original.u8(s)?, converted.u8(n)?);
        }
        let values = original.array(e + 40, 16, Some(0x80800090))?;
        let native_values = converted.array(ne + 32, 16, Some(0x80800090))?;
        assert_eq!(values.len(), native_values.len());
        for (s, n) in values.into_iter().zip(native_values) {
            assert_eq!(original.bytes::<16>(s)?, converted.bytes::<16>(n)?);
        }
        // The extra native endpoint is an empty state program, never a discarded source wire.
        assert_eq!(native.u32(connection + 8)?, tag);
        let endpoint = usize::try_from(native.u64(connection + 16)?)?;
        assert_eq!(converted.u32(endpoint)?, tag);
        assert_eq!(converted.u64(endpoint + 16)?, 2);
        assert_eq!(converted.u64(endpoint + 24)?, 0x808092F7);
        assert_eq!(converted.u64(endpoint + 32)?, 0);
        assert_eq!(converted.u64(endpoint + 40)?, u32::MAX as u64);
        for def in [n, nd, nd + 0xB0, ne, endpoint] {
            assert_eq!(converted.u32(def)?, tag);
            let live = usize::try_from(converted.u64(def + 8)?)?;
            assert_eq!(converted.u32(live)?, tag);
            assert_eq!(converted.u64(live + 8)?, def as u64);
        }
    }
    Ok(())
}
