//! A connected status sound keeps its complete matched native scheduler and allocation.
use super::*;
use crate::d1_roi::sequence::sound::Sound;
mod allocation;
mod layout;
use layout::{connections, graph, native_pair, root};

pub(in super::super) const GRAPH: u32 = 0x80BC5833;

pub(in super::super) struct Companion {
    pub owner: u32,
    pub definition: usize,
    pub patch: NativeAssetResourcePatch,
}

pub(in super::super) fn read(
    source: &Source,
    native: &mut Reader,
    source_graph: &Payload,
) -> Result<Option<Companion>> {
    let rows = source_graph.array(32, 12, Some(0x80800715))?;
    if rows.len() != 3 {
        return Ok(None);
    }
    let owner = source_graph.u32(rows[1])?;
    let p = source.read(owner)?;
    if p.u32(p.pointer(24)? - 4)? != 0x80800610 {
        return Ok(None);
    }
    ensure!(
        source.reference(owner)? == 0x80800861,
        "Source sound sequence owner class differs"
    );
    let owners = graph(source_graph, false)?;
    let target_graph = native.tag(GRAPH, Some(0x80809C0F))?;
    let targets = graph(&target_graph, true)?;
    let target = native.tag(targets[1], Some(0x80809C36))?;
    let source_status = source.read(owners[0])?;
    let target_status = native.tag(targets[0], Some(0x80809C36))?;
    layout::status(&source_status, false)?;
    layout::status(&target_status, true)?;
    allocation::read(native, &target_graph, &targets)?;
    connections(
        source_graph,
        (owners[0], source_status.pointer(24)?),
        (owner, p.pointer(24)?),
        false,
    )?;
    connections(
        &target_graph,
        (targets[0], target_status.pointer(24)?),
        (targets[1], target.pointer(24)?),
        true,
    )?;
    root(&p, owner, false)?;
    root(&target, targets[1], true)?;
    providers(source, native, source_graph, &p, true, 1, 4)?;
    providers(source, native, &target_graph, &target, false, 1, 4)?;
    let tree = Topology::read(&p)?;
    ensure!(
        tree.control_count() == 1 && tree.events().len() == 1 && tree.input_count() == 0,
        "Status sound sequence needs one complete control and event"
    );
    let (s, n) = (p.pointer(24)?, target.pointer(24)?);
    let control = p.pointer(p.array(s + 0x160, 8, Some(0x80800629))?[0])?;
    let event = tree.events()[0].offset;
    let controls = target.array(n + 0x158, 24, Some(0x808093E6))?;
    let events = target.array(n + 0x168, 24, Some(0x808093E6))?;
    ensure!(
        controls.len() == 1 && events.len() == 1,
        "Native sound sequence node count differs"
    );
    let nc = target.pointer(controls[0] + 16)?;
    let ne = target.pointer(events[0] + 16)?;
    ensure!(
        p.u32(control - 4)? == 0x80800691
            && p.u32(control + 28)? == 1
            && p.bytes::<16>(control + 48)? == [0; 16]
            && target.u32(nc - 4)? == 0x808093D7
            && target.u32(nc + 48)? == 1
            && target.bytes::<16>(nc + 72)? == [0; 16]
            && target.u32(ne - 4)? == 0x80806B38
            && target.u32(ne + 48)? == 6,
        "Status sound control, event or predicates differ"
    );
    for (from, to, ic, dc) in [
        (control, nc, 0x808093D6, 0x808093D7),
        (event, ne, 0x80806B37, 0x80806B38),
    ] {
        ensure!(
            p.bytes::<28>(from)? == target.bytes::<28>(to + 16)?,
            "Status sound timing or parent differs"
        );
        native_pair(
            &target,
            targets[1],
            usize::try_from(target.u64(to + 8)?)?,
            to,
            ic,
            dc,
        )?;
    }
    let children = p.array(control + 32, 4, Some(0x8080052C))?;
    let native_children = target.array(nc + 56, 4, Some(0x808093FB))?;
    ensure!(
        children.len() == 1
            && native_children.len() == 1
            && p.u32(children[0])? == 2
            && target.u32(native_children[0])? == 1,
        "Status sound child selector differs"
    );
    let sound = Sound::read(source, &p, event)?;
    ensure!(
        sound.parameters().is_empty() && sound.locator() == 0,
        "Status sound needs parameter translation"
    );
    let mut translated = (*target).clone();
    let patch = sound.write(
        &mut translated,
        ne,
        &crate::d1_roi::audio::cue_symbol(sound.cue()),
        &BTreeMap::new(),
        &BTreeMap::from([(0, 0)]),
    )?;
    ensure!(
        translated.0.len() == target.0.len()
            && translated.0[..patch.offset] == target.0[..patch.offset]
            && translated.0[patch.offset + 4..] == target.0[patch.offset + 4..],
        "Status sound translation changes the matched native allocation or event body"
    );
    let bindings =
        weapon_component_bindings(&target_graph.0, 0x1C0F1EBB).map_err(anyhow::Error::msg)?;
    ensure!(
        bindings.len() == 1
            && bindings[0].owner_tag == targets[1]
            && bindings[0].concrete_class == 0x808084D7
            && bindings[0].resource_offset == target.pointer(16)? as u64,
        "Native sound sequence binding differs"
    );
    Ok(Some(Companion {
        owner,
        definition: p.pointer(24)?,
        patch: NativeAssetResourcePatch {
            binding_hash: bindings[0].binding_hash,
            resource_index: u16::try_from(bindings[0].resource_index)?,
            offset: u32::try_from(
                patch
                    .offset
                    .checked_sub(target.pointer(16)?)
                    .context("Sound resource precedes runtime")?,
            )?,
            expected: target.bytes::<4>(patch.offset)?.to_vec(),
            bytes: translated.bytes::<4>(patch.offset)?.to_vec(),
            imported_particle: Some(patch.symbol),
        },
    }))
}
