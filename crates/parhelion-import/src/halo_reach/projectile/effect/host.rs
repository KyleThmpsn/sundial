//! An independent projectile trail flow with native activation and cleanup registrations.
use super::*;
use crate::tiger::{
    entity::{compiled, links},
    sequence::{self, Kind, Node, Timing},
};

const ENTITY: u32 = 0x80c0dc18;
const OWNER: u32 = 0x80fc7ee1;

pub(super) fn prepare(
    native: &mut Reader,
    owners: &mut BTreeMap<u32, Payload>,
) -> Result<(Target, u32, Payload)> {
    ensure!(
        !owners.contains_key(&OWNER),
        "Projectile trail host is already present"
    );
    let mut payload = (*native.tag(OWNER, Some(0x80809c36))?).clone();
    let definition = payload.pointer(24)?;
    for field in [0x178, 0x188, 0x200, 0x210, 0x220, 0x230, 0x2b8] {
        ensure!(
            payload.u64(definition + field)? == 0,
            "Trail host has additional dependencies"
        );
    }
    let locators = payload.array(definition + 0x1c0, 16, Some(0x80808615))?;
    let spans = payload.array(definition + 0x1b0, 6, Some(0x80808617))?;
    let names = payload.array(definition + 0x1d0, 4, Some(0x80808614))?;
    ensure!(
        locators.len() == 1 && spans.len() == 1 && names.len() == 1,
        "Trail host needs one declared object locator"
    );
    ensure!(
        payload.bytes::<16>(locators[0])?
            == [
                9, 0, 1, 0, 0x3f, 0, 0, 0, 0xc5, 0x9d, 0x1c, 0x81, 0xff, 0, 0, 0
            ]
            && payload.bytes::<6>(spans[0])? == [0, 1, 0, 1, 0x3f, 0],
        "Trail host locator contract differs"
    );
    let name = crate::tiger::audio::bank::hash("reach/projectile/trail");
    put(&mut payload.0, locators[0], &[2, 0, 1, 0, 0x3c, 0, 0, 0])?;
    put(&mut payload.0, locators[0] + 8, &name.to_le_bytes())?;
    put(&mut payload.0, spans[0] + 4, &0x3cu16.to_le_bytes())?;
    put(&mut payload.0, names[0], &name.to_le_bytes())?;
    let markers = owners
        .iter()
        .filter_map(|(&tag, p)| (p.u32(p.pointer(24).ok()? - 4).ok()? == 0x80808507).then_some(tag))
        .collect::<Vec<_>>();
    ensure!(
        markers.len() == 1,
        "Trail host requires one marker component"
    );
    let marker = markers::Marker {
        name,
        binding: [0, 0],
        position: [0., 0., 0., 1.],
        orientation: [0., 0., 0., 1.],
        row: 0,
    };
    let (updated, _) = markers::rewrite(&owners[&markers[0]], &[marker])?;
    owners.insert(markers[0], updated);
    let allocation_tag = payload.u32(0x44)?;
    ensure!(
        owners
            .values()
            .all(|owner| owner.u32(0x44).ok() != Some(allocation_tag)),
        "Authored trail allocation is shared with another component"
    );
    let mut allocation = (*native.tag(allocation_tag, Some(0x80809bbb))?).clone();
    let root = Node {
        name: crate::tiger::audio::bank::hash("reach/projectile/trail/root"),
        timing: Timing::default(),
        kind: Kind::Parallel(vec![Node {
            name,
            timing: Timing {
                duration: [1.; 2],
                flags: 8,
                ..Timing::default()
            },
            kind: Kind::Event(0x80806cc6),
        }]),
    };
    let envelope = (*native.tag(0x80fc7dfb, Some(0x80809c36))?).clone();
    let written = sequence::write(&mut payload, &mut allocation, &root, &[envelope])?;
    owners.insert(OWNER, payload);
    Ok((
        Target {
            owner: OWNER,
            event: written.events[&name],
            locator: 0,
            name,
            duration: 1.,
        },
        allocation_tag,
        allocation,
    ))
}

pub(super) fn attach(
    native: &mut Reader,
    entity: &mut Payload,
    owners: &BTreeMap<u32, Payload>,
) -> Result<()> {
    let host = native.tag(ENTITY, Some(0x80809c0f))?;
    let host_links = links::Graph::read(&host, false)?;
    ensure!(
        host_links.components.contains(&OWNER)
            && host_links.connections.is_empty()
            && host_links.named_connections.is_empty(),
        "Trail host requires connected providers"
    );
    let template = native.tag(OWNER, Some(0x80809c36))?;
    let components = entity
        .array(16, 12, Some(0x80809c04))?
        .into_iter()
        .map(|row| entity.u32(row))
        .collect::<Result<Vec<_>>>()?;
    let mut components = components
        .iter()
        .map(|tag| compiled::Component {
            owner: *tag,
            payload: &owners[tag],
            template: &owners[tag],
            entity,
        })
        .collect::<Vec<_>>();
    components.push(compiled::Component {
        owner: OWNER,
        payload: &owners[&OWNER],
        template: &template,
        entity: &host,
    });
    let rows = |field| {
        entity
            .array(field, 72, Some(0x80809bc9))?
            .into_iter()
            .map(|row| Ok(entity.bytes::<72>(row)?.to_vec()))
            .collect::<Result<Vec<_>>>()
    };
    let links = links::NativeRows {
        connections: rows(32)?,
        named_connections: rows(48)?,
    };
    *entity = compiled::emit(entity, &components, &links)?;
    Ok(())
}
