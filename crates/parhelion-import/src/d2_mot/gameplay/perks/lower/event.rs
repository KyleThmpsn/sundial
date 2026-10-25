//! Event filters retain symbolic labels and rebuild native masks during compilation.
use super::{Node, append_array};
use crate::d2_mot::native::effects::resources::selectors;
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};

const EMPTY: u32 = 0x811C9DC5;
const LABEL_PATH: &str = "content/common/native/sandbox/label_globals.label_globals.tft";

pub(super) fn label_path(bytes: &mut Vec<u8>) -> usize {
    let at = bytes.len();
    bytes.extend_from_slice(LABEL_PATH.as_bytes());
    bytes.push(0);
    at
}

fn empty(p: &Payload, at: usize) -> Result<()> {
    ensure!(
        p.bytes::<16>(at)? == [0; 16],
        "unsupported populated source filter at {at:X}"
    );
    Ok(())
}

fn label_reference(
    p: &Payload,
    at: usize,
    output: &mut [u8],
    to: usize,
    path: usize,
) -> Result<()> {
    ensure!(
        p.u32(at + 12)? == 1
            && p.u64(at + 16)? == 0
            && (0x80800001..=0x81FFFFFF).contains(&p.u32(at + 8)?),
        "unsupported source label reference at {at:X}"
    );
    let start = p.pointer(at)?;
    let tail = p.0.get(start..).context("source label path extent")?;
    let length = tail
        .iter()
        .position(|&byte| byte == 0)
        .context("unterminated source label path")?;
    ensure!(
        &tail[..length] == LABEL_PATH.as_bytes(),
        "source labels use an unsupported registry"
    );
    output[to..to + 8].copy_from_slice(&(path as i64 - to as i64).to_le_bytes());
    output[to + 8..to + 12].copy_from_slice(&0x80C70CA1u32.to_le_bytes());
    output[to + 12..to + 16].fill(0);
    Ok(())
}

fn label_list(p: &Payload, at: usize, output: &mut Vec<u8>, to: usize, path: usize) -> Result<()> {
    let count = usize::try_from(p.u64(at)?)?;
    ensure!(count <= 256, "source label list exceeds limit");
    if count == 0 {
        empty(p, at)?;
        output[to..to + 16].fill(0);
        return Ok(());
    }
    let header = p.pointer(at + 8)?;
    ensure!(
        header >= 4 && p.u32(header - 4)? == 0x80809FB8 && p.u64(header + 8)? == 0x80809787,
        "source label array envelope differs"
    );
    let rows = p.array(at, 32, Some(0x80809787))?;
    append_array(output, to, 0x808094B3, &vec![0; rows.len() * 24], 24)?;
    let target =
        to + 8 + usize::try_from(i64::from_le_bytes(output[to + 8..to + 16].try_into()?))? + 16;
    for (index, row) in rows.into_iter().enumerate() {
        ensure!(p.u32(row + 4)? == 0, "unsupported source label flags");
        let to = target + index * 24;
        output[to..to + 4].copy_from_slice(&p.bytes::<4>(row)?);
        label_reference(p, row + 8, output, to + 8, path)?;
    }
    Ok(())
}

pub(super) fn labels(
    p: &Payload,
    at: usize,
    output: &mut Vec<u8>,
    to: usize,
    path: usize,
) -> Result<()> {
    for operation in 0..4 {
        label_list(p, at + operation * 16, output, to + operation * 16, path)?;
    }
    label_reference(p, at + 64, output, to + 64, path)
}

/// Modern 8080B7B2 and native 80803DE7 separate the killed object's labels from
/// the damage-event filter. A single positive killed-target state is representable
/// by the native Duration key predicate. The native checker queries the killed
/// object's Duration interfaces and method five compares their definition keys.
/// Other modern state operations are refused.
/// Native label masks are intentionally rebuilt against the destination registry.
pub fn kill_condition(p: &Payload, at: usize) -> Result<Node> {
    ensure!(
        at >= 4 && p.u32(at - 4)? == 0x8080B7B2,
        "unsupported source kill condition class"
    );
    let root = p.bytes::<512>(at)?;
    let probability = p.f32(at)?;
    ensure!(
        probability.is_finite()
            && (0.0..=1.0).contains(&probability)
            && root[4] == 255
            && root[5] == 2
            && root[6] == 0,
        "unsupported source kill dispatch, probability or flags"
    );
    ensure!(
        p.u64(at + 0x70)? == 0,
        "source killed-target extension mask requires translation"
    );
    ensure!(
        p.bytes::<72>(at + 0x78)? == [0; 72]
            && p.bytes::<16>(at + 0xC0)? == [0; 16]
            && p.bytes::<16>(at + 0xD0)? == [0; 16],
        "source kill object references require translation"
    );
    // New source state filters have four operations for both attacker and victim.
    // Preserve only the independently representable singleton victim predicate.
    for field in [0x158, 0x168, 0x178, 0x188, 0x1A8, 0x1B8, 0x1C8] {
        empty(p, at + field)?;
    }
    let states = p.array(at + 0x198, 4, Some(0x80800070))?;
    ensure!(
        states.len() <= 1,
        "source kill has multiple required target states"
    );
    if states.is_empty() {
        empty(p, at + 0x198)?;
    }
    let state = states
        .first()
        .map(|&row| p.u32(row))
        .transpose()?
        .unwrap_or(EMPTY);
    ensure!(
        state != 0 && state != u32::MAX,
        "invalid killed-target state key"
    );
    ensure!(
        p.u64(at + 0x1D8)? == 0
            && root[0x1E0..0x1E2] == [0; 2]
            && root[0x1E2] <= 1
            && root[0x1E3..0x1E8] == [0; 5]
            && p.u32(at + 0x1E8)? == EMPTY
            && p.u32(at + 0x1EC)? == 0
            && p.u32(at + 0x1F0)? == 0
            && p.f32(at + 0x1F4)? == -1.0
            && p.u64(at + 0x1F8)? == 0,
        "source kill has unsupported value, range or ownership extensions"
    );
    ensure!(
        root[0x150] <= 1 && root[0x151] <= 1 && root[0x152..0x158] == [0; 6],
        "source ability filter flags differ"
    );
    let mut bytes = vec![0; 344];
    bytes[..8].copy_from_slice(&root[..8]);
    let path = label_path(&mut bytes);
    labels(p, at + 8, &mut bytes, 8, path)?;
    labels(p, at + 0xE0, &mut bytes, 0xD0, path)?;
    // Source compiled predicates address a different registry and cannot be copied.
    bytes[0x58] = 255;
    bytes[0x128] = 255;
    bytes[0x120..0x128].copy_from_slice(&root[0x138..0x140]);
    bytes[0x138..0x140].copy_from_slice(&root[0x150..0x158]);
    bytes[0x141] = root[0x1E2];
    bytes[0x144..0x148].copy_from_slice(&state.to_le_bytes());
    bytes[0x148..0x14C].copy_from_slice(&EMPTY.to_le_bytes());
    bytes[0x154..0x158].copy_from_slice(&(-1.0f32).to_le_bytes());
    Ok(Node {
        class: 0x80803DE7,
        kind: 2,
        bytes,
    })
}

pub fn kill_condition_with_labels(
    source: &Payload,
    at: usize,
    resources: &selectors::Resources,
    groups: &selectors::GroupBindings,
) -> Result<Node> {
    let mut node = kill_condition(source, at)?;
    let mut payload = Payload(node.bytes);
    for (labels, predicate, target_labels, target_predicate) in
        [(8, 0x60, 8, 0x58), (0xE0, 0x140, 0xD0, 0x128)]
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
