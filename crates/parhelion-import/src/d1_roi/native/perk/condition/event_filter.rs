//! Event slot, ADS and dual-label predicates with matched original consumers.
use super::*;

pub(super) fn read(
    source: &Source,
    p: &Payload,
    at: usize,
    native: Option<&mut Reader>,
) -> Result<Option<NativeNode>> {
    let kind = p.u8(at + 5)?;
    if !matches!(
        (p.u32(at - 4)?, kind),
        (0x808030F1, 3) | (0x808030A1, 9) | (0x80802192, 23)
    ) {
        return Ok(None);
    }
    ensure!(
        (0.0..=1.0).contains(&p.f32(at)?) && p.u8(at + 4)? == 0xFF && p.u8(at + 6)? == 0,
        "Source event predicate has unsupported probability or retained state"
    );
    if kind == 3 {
        return value(source, p, at, native).map(Some);
    }
    if kind == 9 {
        ensure!(
            p.u8(at + 8)? & !7 == 0 && p.bytes::<3>(at + 9)? == [0; 3],
            "Source ability-slot event requires an unsupported slot or reserved field"
        );
        // Source 76AC00 and native 107C490 select a byte mask bit with event +4.
        // The admitted low three bits retain the Grenade, Super and Melee slots.
    } else {
        ensure!(
            p.u8(at + 8)? <= 1 && p.u8(at + 9)? <= 1 && p.u8(at + 10)? <= 1 && p.u8(at + 11)? == 0,
            "Source aim event has unsupported selectors or reserved fields"
        );
        // Source 76D360 and native 1080CC0 compare event +4 with +8 and apply
        // owning-weapon +A. Optional +9 requires the related state component and
        // excludes its signed values 1 and 2. Both actual state getters execute in
        // the bounded callback comparison, with owner/context services supplied.
    }
    Ok(Some(NativeNode {
        kind,
        bytes: p.bytes::<12>(at)?.to_vec(),
    }))
}

fn value(
    source: &Source,
    p: &Payload,
    at: usize,
    mut native: Option<&mut Reader>,
) -> Result<NativeNode> {
    ensure!(
        p.bytes::<4>(at + 0xDC)? == [0; 4]
            && p.bytes::<0x48>(at + 0xE0)? == [0; 0x48]
            && p.u8(at + 0x204)? == 0
            && p.u8(at + 0x205)? <= 1
            && p.bytes::<2>(at + 0x206)? == [0; 2],
        "Source value event has supplemental categories, ownership or reserved fields"
    );
    let lower = p.f32(at + 0x1FC)?;
    let upper = p.f32(at + 0x200)?;
    ensure!(
        lower.is_finite() && upper.is_finite() && lower <= upper,
        "Source value event has an invalid numeric range"
    );
    // Source 76C980 and native 107DD90 compare two independent event-label sets
    // before the inclusive scalar range. Empty supplemental categories with an
    // unset ownership byte pass through both original helpers without lookup.
    let mut bytes = vec![0; 0x120];
    bytes[..8].copy_from_slice(&p.bytes::<8>(at)?);
    bytes[0x58] = 0xFF;
    bytes[0x100] = 0xFF;
    put(&mut bytes, 0x110, &p.bytes::<8>(at + 0x1FC)?)?;
    // Source 76CA30 and native 107D380 swap the same two event identities between
    // target and position context. Preserve that selector as well as admission.
    bytes[0x119] = p.u8(at + 0x205)?;
    put(&mut bytes, 0x11C, &0x811C9DC5u32.to_le_bytes())?;
    let mut graph = Graph {
        blocks: vec![Block {
            class: 0x80803DE5,
            count: None,
            bytes,
            links: BTreeMap::new(),
        }],
    };
    label::translate(source, p, at + 8, &mut graph, 8, native.as_deref_mut())?;
    label::translate(source, p, at + 0x128, &mut graph, 0xB0, native)?;
    graph
        .validate_node(NodeKind::Condition(3))
        .map_err(anyhow::Error::msg)?;
    Ok(NativeNode {
        kind: 3,
        bytes: graph.emit().map_err(anyhow::Error::msg)?,
    })
}
