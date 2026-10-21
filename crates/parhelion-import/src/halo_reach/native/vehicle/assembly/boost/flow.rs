//! Source-sized trail controls, inputs and locator storage on a standalone flow envelope.
use super::*;
use crate::tiger::{
    allocation::Row,
    sequence::{self, Kind, Node, Timing},
};

pub(super) const OWNER: u32 = 0x80fc7ee1;
pub(super) const SYSTEM: u32 = 0x80f2f261;
const ENTITY: u32 = 0x80c0dc18;

pub(super) struct Flow {
    pub payload: Payload,
    pub allocation: Payload,
    pub allocation_tag: u32,
    pub registration: Payload,
    pub input: usize,
    pub events: Vec<usize>,
    pub gates: Vec<usize>,
}

fn array(
    payload: &mut Payload,
    field: usize,
    class: u32,
    stride: usize,
    bytes: &[u8],
) -> Result<Vec<usize>> {
    append(&mut payload.0, field, u64::from(class), bytes, stride)?;
    payload.array(field, stride, Some(class))
}

fn input(payload: &mut Payload, name: u32) -> Result<usize> {
    let root = payload.pointer(16)?;
    let definition = payload.pointer(24)?;
    let i = array(payload, root + 0xd0, 0x80809788, 96, &[0; 96])?[0];
    let d = array(payload, definition + 0x188, 0x80809789, 40, &[0; 40])?[0];
    for (at, class, twin) in [(i, 0x80809789u32, d), (d, 0x80809788u32, i)] {
        put(&mut payload.0, at, &OWNER.to_le_bytes())?;
        put(&mut payload.0, at + 4, &class.to_le_bytes())?;
        put(&mut payload.0, at + 8, &(twin as u64).to_le_bytes())?;
    }
    sequence::relative(payload, i + 16, root)?;
    // Unbound scalar receiver state, shared with authored flight inputs.
    for offset in [0x20, 0x24, 0x30, 0x38, 0x3c] {
        put(&mut payload.0, i + offset, &u32::MAX.to_le_bytes())?;
    }
    put(&mut payload.0, d + 24, &0x808097c1u64.to_le_bytes())?;
    put(&mut payload.0, d + 32, &name.to_le_bytes())?;
    Ok(d)
}

fn locators(payload: &mut Payload, groups: &[(u32, usize, f32)]) -> Result<Row> {
    let root = payload.pointer(16)?;
    let d = payload.pointer(24)?;
    let points = groups.iter().map(|(_, count, _)| count).sum::<usize>();
    ensure!(
        !groups.is_empty() && groups.len() <= 255 && points <= 255,
        "Source trail locators exceed native byte selectors"
    );
    let mut declarations = Vec::new();
    let mut spans = Vec::new();
    let mut names = Vec::new();
    let mut first = 0u8;
    for (index, &(name, count, _)) in groups.iter().enumerate() {
        let count = u8::try_from(count)?;
        ensure!(count > 0, "Source trail locator has no markers");
        declarations.extend([2, first, count, 0, 0x3c, 0, 0, 0]);
        declarations.extend(name.to_le_bytes());
        declarations.extend([0xff, 0, 0, 0]);
        spans.extend([first, count, u8::try_from(index)?, 1, 0x3c, 0]);
        names.extend(name.to_le_bytes());
        first = first
            .checked_add(count)
            .context("Trail locator point overflow")?;
    }
    array(payload, d + 0x1b0, 0x80808617, 6, &spans)?;
    array(payload, d + 0x1c0, 0x80808615, 16, &declarations)?;
    array(payload, d + 0x1d0, 0x80808614, 4, &names)?;
    ensure!(
        payload.u64(root + 0x170)? == 0,
        "Trail host has an absolute transform pool"
    );
    let transforms = [0f32, 0., 0., 1., 0., 0., 0., 1.]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    array(
        payload,
        root + 0x180,
        0x80809f75,
        32,
        &transforms.repeat(points),
    )?;
    array(
        payload,
        root + 0x190,
        0x80808618,
        2,
        &[0x3c, 0].repeat(points),
    )?;
    let mut slot = [0u8; 24];
    slot[..8].fill(0xff);
    array(
        payload,
        root + 0x1a0,
        0x80809bff,
        24,
        &slot.repeat(groups.len()),
    )?;
    for (index, count) in [0, points, points, groups.len()].into_iter().enumerate() {
        put(
            &mut payload.0,
            d + 0x1e0 + index * 4,
            &u32::try_from(count)?.to_le_bytes(),
        )?;
    }
    Ok(Row::named(
        0x846fde3f,
        u32::MAX,
        0,
        vec![
            Row::named(0x4c2e87a1, 0x80809f75, points, Vec::new()),
            Row::named(0x6e385590, 0x80808618, points, Vec::new()),
            Row::named(0xa6e3dd20, 0x80809bff, groups.len(), Vec::new()),
        ],
    ))
}

pub(super) fn build(
    native: &mut Reader,
    channel: u32,
    groups: &[(u32, usize, f32)],
) -> Result<Flow> {
    let registration = (*native.tag(ENTITY, Some(0x80809c0f))?).clone();
    let links = links::Graph::read(&registration, false)?;
    ensure!(
        links.components.contains(&OWNER)
            && links.connections.is_empty()
            && links.named_connections.is_empty(),
        "Standalone trail host has external connections"
    );
    let mut payload = (*native.tag(OWNER, Some(0x80809c36))?).clone();
    let d = payload.pointer(24)?;
    for field in [0x148, 0x178, 0x188, 0x200, 0x210, 0x220, 0x230, 0x2b8] {
        ensure!(
            payload.u64(d + field)? == 0,
            "Standalone trail host has additional dependencies"
        );
    }
    let allocation_tag = payload.u32(0x44)?;
    let mut allocation = (*native.tag(allocation_tag, Some(0x80809bbb))?).clone();
    let input = input(&mut payload, channel)?;
    let locator_allocation = locators(&mut payload, groups)?;
    Row::write(
        &[
            Row::named(0x86374c3c, 0x80809788, 1, Vec::new()),
            locator_allocation,
        ],
        &mut allocation,
        32,
    )?;
    let root = Node {
        name: super::super::super::damage::hash("source/boost/root"),
        timing: Timing::default(),
        kind: Kind::Parallel(
            groups
                .iter()
                .enumerate()
                .map(|(index, &(name, _, threshold))| Node {
                    name: super::super::super::damage::hash(&format!("source/boost/gate/{index}")),
                    timing: Timing {
                        flags: 0x10,
                        ..Timing::default()
                    },
                    kind: Kind::Range {
                        input: 0,
                        bounds: [threshold, f32::MAX],
                        children: vec![Node {
                            name,
                            timing: Timing {
                                duration: [1.; 2],
                                flags: 8,
                                ..Timing::default()
                            },
                            kind: Kind::Event(0x80806cc6),
                        }],
                    },
                })
                .collect(),
        ),
    };
    let envelope = (*native.tag(0x80fc7dfb, Some(0x80809c36))?).clone();
    let written = sequence::write(&mut payload, &mut allocation, &root, &[envelope])?;
    let controls = payload.array(d + 0x158, 24, Some(0x808093e6))?;
    let parallel = payload.pointer(controls[0] + 16)?;
    payload.0[parallel + 0x31] = 1;
    let gates = controls[1..]
        .iter()
        .map(|row| payload.pointer(row + 16))
        .collect::<Result<Vec<_>>>()?;
    let events = groups
        .iter()
        .map(|(name, _, _)| written.events[name])
        .collect();
    Ok(Flow {
        payload,
        allocation,
        allocation_tag,
        registration,
        input,
        events,
        gates,
    })
}
