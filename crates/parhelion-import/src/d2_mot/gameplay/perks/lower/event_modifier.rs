//! Event damage adjustments preserve their symbolic filters and scalar program.
use super::{Node, append_array, event};
use crate::d2_mot::native::effects::resources::selectors;
use crate::d2_mot::{native::effects::lower_program, payload::Payload};
use anyhow::{Result, ensure};

fn types(source: &Payload, at: usize, bytes: &mut Vec<u8>, to: usize) -> Result<()> {
    let rows = source.array(at, 1, Some(0x8080655E))?;
    ensure!(rows.len() <= 32, "damage type list exceeds limit");
    if rows.is_empty() {
        ensure!(
            source.u64(at + 8)? == 0,
            "empty damage type list has a pointer"
        );
    }
    let values = rows
        .into_iter()
        .map(|row| source.u8(row))
        .collect::<Result<Vec<_>>>()?;
    ensure!(values.iter().all(|&v| v <= 6), "unknown source damage type");
    append_array(bytes, to, 0x80806B02, &values, 1)
}

pub(super) fn damage_filter(
    source: &Payload,
    at: usize,
    bytes: &mut Vec<u8>,
    to: usize,
    path: usize,
) -> Result<()> {
    types(source, at, bytes, to)?;
    types(source, at + 16, bytes, to + 16)?;
    event::labels(source, at + 32, bytes, to + 32, path)
}

fn scalar(source: &Payload, at: usize, bytes: &mut Vec<u8>) -> Result<usize> {
    ensure!(
        source.u32(at - 4)? == 0x8080B4E5
            && source.bytes::<8>(at)? == [0, 0, 0, 1, 0, 0, 0, 0]
            && source.u64(at + 8)? == 0,
        "source event scalar has an unsupported provider or operation"
    );
    ensure!(
        source.u64(at + 0x30)? == 1
            && source.u64(at + 0x38)? == 1
            && source.bytes::<16>(at + 0x40)? == [0; 16]
            && source.bytes::<8>(at + 0x50)? == [255, 0, 0, 0, 0xC5, 0x9D, 0x1C, 0x81],
        "source event scalar input contract differs"
    );
    let code = source
        .array(at + 0x10, 1, Some(0x80800009))?
        .into_iter()
        .map(|row| source.u8(row))
        .collect::<Result<Vec<_>>>()?;
    let constants = source
        .array(at + 0x20, 16, Some(0x80800090))?
        .into_iter()
        .map(|row| source.bytes::<16>(row))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    ensure!(
        !code.is_empty() && code.len() <= 4096 && constants.len() <= 4096,
        "source event scalar exceeds limits"
    );
    // An unselected input may not become a hidden dependency after lowering.
    ensure!(
        code == [0x42, 0, 0x4C, 0] && constants.len() == 16,
        "source event scalar is not a single literal"
    );
    let value = f32::from_le_bytes(constants[..4].try_into()?);
    ensure!(
        value.is_finite()
            && constants
                .chunks_exact(4)
                .all(|lane| lane == &constants[..4]),
        "source event scalar is not a finite scalar constant"
    );
    let code = lower_program(&code, 1, 1)?;
    let root = (bytes.len() + 11) & !7;
    bytes.resize(root + 64, 0);
    bytes[root - 4..root].copy_from_slice(&0x80802F18u32.to_le_bytes());
    bytes[root + 40..root + 44].copy_from_slice(&1u32.to_le_bytes());
    bytes[root + 48..root + 52].copy_from_slice(&1u32.to_le_bytes());
    bytes[root + 56] = 255;
    append_array(bytes, root + 8, 0x80800009, &code, 1)?;
    append_array(bytes, root + 24, 0x80800090, &constants, 16)?;
    Ok(root)
}

/// The native kind-40 consumer multiplies the selected event amount by one plus
/// this scalar. Native compilation rebuilds both label masks from their lists.
/// Assignment rows, target selectors and extra source operations remain explicit
/// translation gaps, rather than being silently removed.
pub fn event_modifier(source: &Payload, at: usize) -> Result<Node> {
    ensure!(
        source.u32(
            at.checked_sub(4)
                .ok_or_else(|| anyhow::anyhow!("event modifier prefix"))?
        )? == 0x8080B4E1,
        "unsupported source event modifier class"
    );
    ensure!(
        source.bytes::<8>(at)? == [35, 0, 0, 0, 0, 0, 0, 0]
            && source.u64(at + 8)? == 0x20
            && source.u64(at + 16)? == 0x100
            && source.bytes::<16>(at + 24)? == [0; 16],
        "source event modifier root selector requires translation"
    );
    ensure!(
        source.u64(at + 0xB8)? == 0
            && source.u64(at + 0xC0)? == 0
            && source.u64(at + 0xC8)? == 0x100
            && source.bytes::<16>(at + 0xD0)? == [0; 16]
            && source.bytes::<32>(at + 0x148)? == [0; 32]
            && source.bytes::<16>(at + 0x170)? == [0; 16],
        "source event modifier has unsupported flags or additional operations"
    );
    let mut bytes = vec![0; 336];
    bytes[0] = 40;
    bytes[8] = 0x20;
    let path = event::label_path(&mut bytes);
    damage_filter(source, at + 0x28, &mut bytes, 0x28, path)?;
    event::labels(source, at + 0xE0, &mut bytes, 0xC0, path)?;
    bytes[0x98..0xA0].copy_from_slice(&source.bytes::<8>(at + 0xA0)?);
    bytes[0xA0] = 255;
    bytes[0x110] = 255;
    let program = scalar(source, source.pointer(at + 0x168)?, &mut bytes)?;
    bytes[0x140..0x148].copy_from_slice(&(program as i64 - 0x140).to_le_bytes());
    Ok(Node {
        class: 0x80802F16,
        kind: 40,
        bytes,
    })
}

pub fn event_modifier_with_labels(
    source: &Payload,
    at: usize,
    resources: &selectors::Resources,
    groups: &selectors::GroupBindings,
) -> Result<Node> {
    let mut node = event_modifier(source, at)?;
    let mut payload = Payload(node.bytes);
    for (labels, predicate, target_labels, target_predicate) in
        [(0x48, 0xA8, 0x48, 0xA0), (0xE0, 0x138, 0xC0, 0x110)]
    {
        selectors::append_labels(
            source,
            (at + labels, at + predicate),
            &mut payload,
            (target_labels, target_predicate),
            resources,
            groups,
        )?;
    }
    node.bytes = payload.0;
    Ok(node)
}
