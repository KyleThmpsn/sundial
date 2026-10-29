//! Read the staged sequence tree and resolve HUD keys through each build's real dictionary.
use super::*;

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    graph: &Payload,
    tag: u32,
) -> CheckResult<()> {
    assert!(stock.manager.get_entry(tiger_pkg::TagHash(tag)).is_none());
    let native = emitted.tag(tag, Some(0x80809C0F))?;
    let originals = graph.array(32, 12, Some(0x80800715))?;
    let targets = native.array(16, 12, Some(0x80809C04))?;
    assert_eq!((originals.len(), targets.len()), (1, 1));
    assert_eq!(graph.bytes::<32>(48)?, [0; 32]);
    assert_eq!(native.bytes::<32>(32)?, [0; 32]);
    for (graph, field) in [(graph, 0x88), (native.as_ref(), 0x78)] {
        let masks = graph.array(field, 2, Some(0x80800006))?;
        assert_eq!(masks.len(), 2);
        for at in masks {
            assert_eq!(graph.u16(at)?, 0);
        }
    }
    let original = source.read(graph.u32(originals[0])?)?;
    let owner_tag = native.u32(targets[0])?;
    assert!(
        stock
            .manager
            .get_entry(tiger_pkg::TagHash(owner_tag))
            .is_none()
    );
    let owner = emitted.tag(owner_tag, Some(0x80809C36))?;
    let (s, n) = (original.pointer(24)?, owner.pointer(24)?);
    assert_eq!(owner.u32(n - 4)?, 0x808084E9);
    assert_eq!(original.u32(s + 0x2C0)?, owner.u32(n + 0x2D0)?);
    assert_eq!(original.u64(s + 0x2C8)?, 0);
    assert_eq!(owner.u64(n + 0x2D8)?, 0);
    // Original construction and root-completion consumers read these separate source
    // Booleans. Native construction and completion read bits in a packed flag byte.
    let flags = owner.u8(n + 0x2E0)?;
    assert_eq!(original.u8(s + 0x2C4)? != 0, flags & 1 != 0);
    assert_eq!(original.u8(s + 0x2D0)? != 0, flags & 2 != 0);
    assert_eq!(original.u8(s + 0x2D3)? != 0, flags & 4 != 0);
    for (from, to) in [(0x190, 0x178), (0x1A0, 0x188), (0x1B0, 0x1B0)] {
        assert_eq!(original.u64(s + from)?, 0);
        assert_eq!(owner.u64(n + to)?, 0);
    }
    assert_eq!(original.u64(s + 0x170)?, 0);
    let controls = original.array(s + 0x160, 8, Some(0x80800629))?;
    let events = original.array(s + 0x180, 8, Some(0x80800629))?;
    let native_controls = owner.array(n + 0x158, 24, Some(0x808093E6))?;
    let native_events = owner.array(n + 0x168, 24, Some(0x808093E6))?;
    assert_eq!((controls.len(), events.len()), (1, 1));
    assert_eq!((native_controls.len(), native_events.len()), (1, 1));
    let control = original.pointer(controls[0])?;
    let event = original.pointer(events[0])?;
    let native_control = owner.pointer(native_controls[0] + 16)?;
    let native_event = owner.pointer(native_events[0] + 16)?;
    assert_eq!(original.u32(control - 4)?, 0x80800691);
    assert_eq!(owner.u32(native_control - 4)?, 0x808093D7);
    assert_eq!(original.u32(event - 4)?, 0x80802573);
    assert_eq!(owner.u32(native_event - 4)?, 0x80803F24);
    for (from, to) in [(control, native_control), (event, native_event)] {
        assert_eq!(original.bytes::<28>(from)?, owner.bytes::<28>(to + 16)?);
        let live = usize::try_from(owner.u64(to + 8)?)?;
        assert_eq!(owner.u32(live)?, owner_tag);
        assert_eq!(owner.u64(live + 8)?, to as u64);
        assert_eq!(owner.u32(live + 4)?, owner.u32(to - 4)?);
    }
    let children = original.array(control + 32, 4, Some(0x8080052C))?;
    let native_children = owner.array(native_control + 56, 4, Some(0x808093FB))?;
    assert_eq!((children.len(), native_children.len()), (1, 1));
    assert_eq!(original.u8(children[0])?, 2);
    assert_eq!(owner.u8(native_children[0])?, 1);
    assert_eq!(original.u16(children[0] + 2)?, 0);
    assert_eq!(owner.u16(native_children[0] + 2)?, 0);
    let original_resource = source.read(original.u32(event + 32)?)?;
    let resource_tag = owner.u32(native_event + 56)?;
    let resource = emitted.tag(resource_tag, Some(0x80803F26))?;
    assert_eq!(original_resource.bytes::<8>(24)?, resource.bytes::<8>(24)?);
    let source_configs = source.classes(0x8080287D);
    assert_eq!(source_configs.len(), 1);
    let source_config = source.read(source_configs[0])?;
    let configs = stock.classes(0x808047C1);
    assert_eq!(configs.len(), 1);
    let native_config = stock.tag(configs[0], Some(0x808047C1))?;
    let names = |config: &Payload| -> CheckResult<Vec<u32>> {
        let mut result = Vec::new();
        for group in config.array(8, 16, None)? {
            for row in config.array(group, 4, Some(0x80800070))? {
                result.push(config.u32(row)?);
            }
        }
        Ok(result)
    };
    let resolve =
        |resource: &Payload, names: &[u32]| -> CheckResult<std::collections::BTreeMap<u32, bool>> {
            let mut values = std::collections::BTreeMap::new();
            for row in resource.array(8, 4, None)? {
                let key = usize::from(resource.u16(row)?)
                    .checked_sub(128)
                    .ok_or("Builtin HUD key")?;
                let name = *names.get(key).ok_or("HUD key outside dictionary")?;
                assert!(resource.u8(row + 2)? <= 1);
                assert!(values.insert(name, resource.u8(row + 2)? != 0).is_none());
            }
            Ok(values)
        };
    assert_eq!(
        resolve(&original_resource, &names(&source_config)?)?,
        resolve(&resource, &names(&native_config)?)?
    );
    if let Ok(stock_resource) = stock.tag(resource_tag, Some(0x80803F26)) {
        assert_eq!(
            resource.0, stock_resource.0,
            "Stock HUD resource was modified"
        );
    }
    Ok(())
}
