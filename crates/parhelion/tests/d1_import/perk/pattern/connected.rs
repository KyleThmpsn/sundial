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
    assert_eq!(native.u32(0x88)?, u32::MAX);
    let connections = native.array(32, 72, Some(0x80809BC9))?;
    assert_eq!(connections.len(), 1);
    let connection = connections[0];
    assert_eq!(native.u32(connection + 40)?, u32::MAX);
    assert_eq!(native.u64(connection + 48)?, 0);
    let mut expected_exports = std::collections::BTreeSet::new();
    for (ordinal, (&original, &converted)) in owners.iter().zip(&targets).enumerate() {
        let original = source.read(graph.u32(original)?)?;
        let tag = native.u32(converted)?;
        assert!(
            stock.tag(tag, None).is_err(),
            "Every connected owner must be private"
        );
        let converted = emitted.tag(tag, Some(0x80809C36))?;
        let (s, n) = (original.pointer(24)?, converted.pointer(24)?);
        let live = converted.pointer(16)?;
        // Complete owners retain their actual source extent and typed lifecycle. A
        // resized definition array alone cannot supply the required live instances.
        assert_eq!(converted.u64(0x48)?, (n - live) as u64);
        assert_eq!(converted.u64(0x50)?, (live - converted.pointer(8)?) as u64);
        assert!(converted.array(32, 8, Some(0x8080919F))?.is_empty());
        let lifecycle = converted.array(48, 16, Some(0x808091A4))?;
        if original.u32(s - 4)? == 0x808020DE {
            let rows = converted.array(n + 88, 88, Some(0x80803B06))?;
            let instances = converted.array(live + 80, 128, Some(0x80803B05))?;
            assert_eq!(rows.len(), instances.len());
            assert_eq!(lifecycle.len(), rows.len() + 1);
            // Follow the actual allocation resource. Its counted row allocation
            // must agree with the emitted instances, not merely the settings count.
            let allocation = emitted.tag(converted.u32(0x44)?, Some(0x80809BBB))?;
            assert_eq!(allocation.u32(8)?, 0x811C9DC5);
            assert_eq!(allocation.u64(16)?, 0);
            let members = allocation.array(32, 40, Some(0x80808852))?;
            assert_eq!(members.len(), 1);
            assert_eq!(allocation.u32(members[0])?, 0xFB05ED04);
            assert_eq!(allocation.u64(members[0] + 8)?, 0);
            let counted = allocation.array(members[0] + 24, 40, Some(0x80808852))?;
            assert_eq!(counted.len(), 1);
            assert_eq!(allocation.u32(counted[0])?, 0x587BC3F9);
            assert_eq!(allocation.u32(counted[0] + 16)?, 0x80803B05);
            assert_eq!(allocation.u32(counted[0] + 20)? as usize, rows.len());
            assert_eq!(allocation.u64(counted[0] + 8)?, 0);
            assert!(
                allocation
                    .array(counted[0] + 24, 40, Some(0x80808852))?
                    .is_empty()
            );
            for ((&definition, &instance), &call) in rows.iter().zip(&instances).zip(&lifecycle) {
                assert!(instance + 128 <= n);
                assert_eq!(converted.u64(definition + 8)?, instance as u64);
                assert_eq!(converted.pointer(call)?, instance);
                assert_eq!(converted.u32(call + 8)?, 0x808045BE);
                assert_eq!(converted.u16(call + 12)?, 0);
                assert_eq!(converted.u16(call + 14)?, 1);
                expected_exports.extend([
                    (
                        tag,
                        ordinal,
                        instance,
                        0x80803B05,
                        definition + 16,
                        0x80BFD9F1,
                    ),
                    (
                        tag,
                        ordinal,
                        instance,
                        0x80803B05,
                        definition + 48,
                        0x80BFD9EE,
                    ),
                ]);
            }
            let cleanup = *lifecycle.last().ok_or("Missing modifier cleanup")?;
            assert_eq!(converted.pointer(cleanup)?, live + 48);
            assert_eq!(converted.u32(cleanup + 8)?, 0x80803B00);
            assert_eq!(converted.u16(cleanup + 12)?, u16::MAX);
            assert_eq!(converted.u16(cleanup + 14)?, 1);
            for (offset, provider) in [(0x68, 0x80BFD9EB), (0x80, 0x80BFD9EC), (0xA0, 0x80BFD9ED)] {
                expected_exports.insert((
                    tag,
                    ordinal,
                    live + 48,
                    0x80803B00,
                    n + offset,
                    provider,
                ));
            }
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
        assert_eq!(lifecycle.len(), 1);
        assert_eq!(converted.pointer(lifecycle[0])?, live + 48);
        assert_eq!(converted.u32(lifecycle[0] + 8)?, 0x80803B32);
        assert_eq!(converted.u16(lifecycle[0] + 12)?, 0);
        assert_eq!(converted.u16(lifecycle[0] + 14)?, 2);
        for (offset, provider) in [
            (0x10, 0x80C23835),
            (0x30, 0x815B887A),
            (0x50, 0x80C23836),
            (0x68, 0x80C23837),
            (0x80, 0x815B887B),
        ] {
            expected_exports.insert((tag, ordinal, live + 48, 0x80803B32, nd + offset, provider));
        }
        for (offset, provider) in [
            (0x10, 0x80C2382F),
            (0x28, 0x80C23830),
            (0x40, 0x80C23831),
            (0x58, 0x80C23832),
        ] {
            expected_exports.insert((
                tag,
                ordinal,
                live + 96,
                0x80803C50,
                nd + 0xB0 + offset,
                provider,
            ));
        }
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
    // Follow every emitted export through its owner, source view and parameter.
    // This catches omitted callbacks, stale bank rows and wrong component ordinals.
    let maps = native.array(0x58, 40, Some(0x80809C22))?;
    let references = native.array(0x68, 24, Some(0x80809C20))?;
    assert_eq!(maps.len(), references.len());
    let mut actual_exports = std::collections::BTreeSet::new();
    for (&map, &reference) in maps.iter().zip(&references) {
        let owner = native.u32(reference)?;
        let ordinal = usize::try_from(native.u32(reference + 16)?)?;
        assert_eq!(
            native.u32(*targets.get(ordinal).ok_or("Export owner ordinal")?)?,
            owner
        );
        let p = emitted.tag(owner, Some(0x80809C36))?;
        let instance = usize::try_from(native.u64(reference + 8)?)?;
        let class = native.u32(reference + 4)?;
        let parameter = usize::try_from(native.u64(map + 24)?)?;
        let provider = native.u32(map + 32)?;
        assert_eq!(native.u32(map + 12)?, class);
        assert_eq!(native.u32(map + 16)?, owner);
        assert_eq!(native.u32(map + 20)?, 0x80809C50);
        // These unloaded callback fields control filtering and selection after
        // the original loader rebases their owner-relative source displacements.
        assert_eq!(native.u32(reference + 20)?, 0);
        assert_eq!(
            native.u32(map + 4)?,
            u32::try_from(instance - p.pointer(16)?)?
        );
        assert_eq!(native.u32(map + 36)?, 0);
        assert_eq!(
            native.u32(map)?,
            if provider == 0x80C23830 { 0x10000 } else { 0 }
        );
        assert_eq!(
            native.u32(map + 8)?,
            match provider {
                0x80BFD9EE => 1,
                0x80C2382F => (-100i32) as u32,
                _ => 0,
            }
        );
        assert_eq!(p.u32(instance)?, owner);
        assert_eq!(p.u32(parameter + 8)?, provider);
        assert!(actual_exports.insert((owner, ordinal, instance, class, parameter, provider)));
    }
    assert_eq!(actual_exports, expected_exports);
    let mut reached = std::collections::BTreeSet::new();
    let buckets = native.array(0x48, 8, Some(0x80809C25))?;
    assert!(buckets.len().is_power_of_two());
    for &bucket in &buckets {
        if native.u32(bucket)? == u32::MAX {
            continue;
        }
        let span = native.u32(bucket + 4)?;
        let first = (span & 0xFFFF) as usize;
        let count = ((span >> 16) & 0x7FFF) as usize;
        assert!(count > 0 && first + count <= references.len());
        for index in first..first + count {
            assert!(reached.insert(index));
        }
        // Independent original lookup contract. Retained keys must remain
        // findable through collision continuation, not merely exist in the array.
        let key = native.u32(bucket)?;
        let seed = native.u32(0x40)?;
        let mixed = (seed ^ key).wrapping_add(key.wrapping_shl(4));
        let mixed = (mixed ^ (mixed >> 10)).wrapping_mul(0x81);
        let mut slot = ((mixed ^ (mixed >> 13)) as usize) & (buckets.len() - 1);
        let mut found = false;
        for _ in 0..buckets.len() {
            let candidate = buckets[slot];
            if native.u32(candidate)? == key {
                assert_eq!(candidate, bucket);
                found = true;
                break;
            }
            assert_ne!(
                native.u32(candidate + 4)? & 0x80000000,
                0,
                "Native lookup loses a collided binding"
            );
            slot = (slot + 1) & (buckets.len() - 1);
        }
        assert!(found, "Native lookup never reaches a retained binding");
    }
    assert_eq!(reached.len(), references.len());
    let masks = native.array(0x78, 2, Some(0x80800006))?;
    assert!(!masks.is_empty());
    for at in masks {
        assert_eq!(native.u16(at)?, 0);
    }
    Ok(())
}
