//! Incoming events preserve their scalar threshold and checked damage/ability labels.
use super::*;

pub(super) fn read(
    source: &Source,
    p: &Payload,
    at: usize,
    native: Option<&mut Reader>,
) -> Result<NativeNode> {
    let threshold = p.f32(at + 0x16C)?;
    let distance = p.f32(at + 0x170)?;
    ensure!(
        (0.0..=1.0).contains(&p.f32(at)?)
            && p.u8(at + 4)? == 255
            && p.u8(at + 6)? == 0
            && p.bytes::<4>(at + 0x94)? == [0; 4]
            && threshold.is_finite()
            && threshold >= 0.0
            && distance.is_finite()
            && distance < 0.0
            && p.bytes::<4>(at + 0x174)? == [0; 4]
            && p.bytes::<8>(at + 0x188)? == [0; 8],
        "Source incoming event has unqualified state, distance or category restrictions"
    );
    // Source 76CFC0 checks this supplemental filter against the event's labels.
    // Native 1080900 has no matching field. Only its canonical empty form is neutral.
    ensure!(
        p.bytes::<64>(at + 0x98)? == [0; 64]
            && p.bytes::<128>(at + 0xE8)? == [0; 128]
            && p.bytes::<4>(at + 0x168)? == [1; 4],
        "Source incoming event has a supplemental label restriction"
    );
    let mut bytes = vec![0; 0xC8];
    bytes[..8].copy_from_slice(&p.bytes::<8>(at)?);
    // Source 6D19C0 and native F00330 publish the original packet plus the keyed
    // scalar from 5D2A80/EC0D90. Preserve the threshold without changing its unit.
    bytes[0x98..0x9C].copy_from_slice(&threshold.to_le_bytes());
    bytes[0xA4..0xA8].copy_from_slice(&distance.to_le_bytes());
    // Zero +9C retains rejection of the owner's own source identity. Other native
    // gates remain disabled. Null +C0 makes numeric callback 107E9C0 return zero.
    bytes[0xA0..0xA4].copy_from_slice(&0x811C9DC5u32.to_le_bytes());
    let mut graph = Graph {
        blocks: vec![Block {
            class: 0x80803DDC,
            count: None,
            bytes,
            links: BTreeMap::new(),
        }],
    };
    filter::damage(source, p, at + 8, at + 0xD8, &mut graph, 8, native)?;
    player::ability_ranges(p, at + 0x178, &mut graph, 0xA8)?;
    graph
        .validate_node(NodeKind::Condition(5))
        .map_err(anyhow::Error::msg)?;
    Ok(NativeNode {
        kind: 5,
        bytes: graph.emit().map_err(anyhow::Error::msg)?,
    })
}
