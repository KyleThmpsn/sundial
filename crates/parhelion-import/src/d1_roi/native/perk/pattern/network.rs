//! Unbound network companions retained with their graph order and provider exclusions.
use super::*;

pub(super) const GRAPH: u32 = 0x80FF0737;
pub(super) const BANK_GRAPH: u32 = 0x80F9F7CF;

// Provider offset, source interface pair, native pair, and compiled method.
// These are declarations on the network implementation, not perk identities.
const PROVIDERS: [(usize, u32, u32, u32, u32, u32); 7] = [
    (0x60, 0x80800813, 0x808009A1, 0x8080974B, 0x8080974A, 7),
    (0x80, 0x80800813, 0x808009A1, 0x8080974B, 0x8080974A, 8),
    (0xA0, 0x80800581, 0x808008FE, 0x80809ADE, 0x80809ADD, 2),
    (0xB8, 0x80800935, 0x808005D5, 0x80809ADA, 0x80809AD9, 4),
    (0xD0, 0x80800727, 0x80800631, 0x80809BF0, 0x80809BEF, 3),
    (0xE8, 0x808006E1, 0x80800353, 0x80809494, 0x80809493, 5),
    (0x100, 0x8080063F, 0x80800753, 0x80809497, 0x80809496, 6),
];

fn schema(p: &Payload, base: u32, size: u32, name: &[u8]) -> Result<()> {
    ensure!(
        p.u64(0)? == p.0.len() as u64
            && p.u32(16)? == base
            && p.u32(20)? == size
            && p.0.windows(name.len()).any(|v| v == name),
        "Network schema differs"
    );
    Ok(())
}

fn native_schema(native: &mut Reader, class: u32, base: u32, size: u32, name: &[u8]) -> Result<()> {
    // Built-in classes name executable declarations. Generated classes instead carry
    // a package schema describing their base and extent.
    if class == base {
        Ok(())
    } else {
        schema(&*native.tag(class, Some(0x80800000))?, base, size, name)
    }
}

/// Validate all seven exported providers and their reciprocal runtime bindings.
fn exports(
    graph: &Payload,
    p: &Payload,
    owner: u32,
    native: bool,
    ordinal: usize,
    exclusion: u32,
) -> Result<()> {
    let (live, d) = (p.pointer(16)?, p.pointer(24)?);
    let class = p.u32(live - 4)?;
    let exports = graph.array(
        if native { 0x58 } else { 0x68 },
        40,
        Some(if native { 0x80809C22 } else { 0x80800370 }),
    )?;
    let refs = graph.array(
        if native { 0x68 } else { 0x78 },
        24,
        Some(if native { 0x80809C20 } else { 0x80800790 }),
    )?;
    ensure!(
        exports.len() == refs.len(),
        "Network export references differ"
    );
    let mut seen = BTreeSet::new();
    for (at, reference) in exports.into_iter().zip(refs) {
        if graph.u32(at + 16)? != owner {
            continue;
        }
        let offset = usize::try_from(graph.u64(at + 24)?)?
            .checked_sub(d)
            .context("Network export precedes definition")?;
        let &(source_offset, _, _, _, _, method) = PROVIDERS
            .iter()
            .find(|row| row.0 - if native { 24 } else { 0 } == offset)
            .context("Unknown network export")?;
        ensure!(
            graph.u32(at)? == if method == 3 { exclusion << 16 } else { 0 }
                && graph.u64(at + 4)? == 0
                && graph.u32(at + 12)? == class
                && graph.u32(at + 20)? == if native { 0x80809C50 } else { 0 }
                && graph.u64(at + 32)? == p.u64(d + offset + 8)?
                && graph.u32(reference)? == owner
                && graph.u32(reference + 4)? == if native { class } else { 0 }
                && graph.u64(reference + 8)? == live as u64
                && graph.u64(reference + 16)? == ordinal as u64
                && seen.insert(source_offset),
            "Network export or owner binding differs"
        );
    }
    ensure!(seen.len() == PROVIDERS.len(), "Incomplete network exports");
    Ok(())
}

fn lifecycle(p: &Payload, native: bool) -> Result<()> {
    let rows = p.array(0x30, 16, Some(if native { 0x808091A4 } else { 0x80800930 }))?;
    ensure!(
        rows.len() == 1
            && p.pointer(rows[0])? == p.pointer(16)?
            && p.u32(rows[0] + 8)? == if native { 0x808090E6 } else { 0x808006F9 }
            && p.u32(rows[0] + 12)? == 0x00010000,
        "Network lifecycle differs"
    );
    let constructor = p.pointer(8)?;
    ensure!(
        p.u32(constructor - 4)? == if native { 0x808090E7 } else { 0x8080096A }
            && p.bytes::<16>(constructor)? == [0; 16]
            && p.bytes::<16>(0x20)? == [0; 16]
            && p.u64(0)? == p.0.len() as u64
            && p.u64(0x48)?
                == p.pointer(24)?
                    .checked_sub(p.pointer(16)?)
                    .context("Network definition precedes instance")? as u64
            && p.0.len() == p.pointer(24)? + if native { 0x188 } else { 0x198 },
        "Network constructor or dependencies differ"
    );
    Ok(())
}

pub(super) fn read(source: &Source, native: &mut Reader, graph: &Payload) -> Result<Option<u32>> {
    let bank = graph.u64(32)? == 3;
    companion(
        source,
        native,
        graph,
        Layout {
            graph: if bank { BANK_GRAPH } else { GRAPH },
            owners: if bank { 3 } else { 2 },
            ordinal: 1,
            exclusion: 4,
            built_in: bank,
        },
    )
}

struct Layout {
    graph: u32,
    owners: usize,
    ordinal: usize,
    exclusion: u32,
    built_in: bool,
}

pub(super) fn sound(source: &Source, native: &mut Reader, graph: &Payload) -> Result<u32> {
    companion(
        source,
        native,
        graph,
        Layout {
            graph: sequence::audio::GRAPH,
            owners: 3,
            ordinal: 2,
            exclusion: 8,
            built_in: false,
        },
    )?
    .context("Source sound companion is not a qualified generated network")
}

fn companion(
    source: &Source,
    native: &mut Reader,
    graph: &Payload,
    layout: Layout,
) -> Result<Option<u32>> {
    let Layout {
        graph: native_tag,
        owners: owner_count,
        ordinal,
        exclusion,
        built_in,
    } = layout;
    let owners = graph.array(32, 12, Some(0x80800715))?;
    if owners.len() != owner_count {
        return Ok(None);
    }
    let owner = graph.u32(owners[ordinal])?;
    let p = source.read(owner)?;
    let (live, d) = (p.pointer(16)?, p.pointer(24)?);
    let (ic, dc) = (p.u32(live - 4)?, p.u32(d - 4)?);
    // Schema kind is part of each complete topology, independent of owner count.
    if (dc == 0x80800362) != built_in {
        return Ok(None);
    }
    ensure!(
        source.reference(owner)? == 0x80800861,
        "Source network owner class differs"
    );
    if dc == 0x80800362 {
        // Built-in declarations live in the executable. The absent header value
        // is checked as observed storage, without assigning it a new meaning.
        ensure!(
            ic == 0x808006F9 && p.u32(0x40)? == u32::MAX,
            "Source built-in network header differs"
        );
    } else {
        if source.reference(dc).ok() != Some(0x80800000) {
            return Ok(None);
        }
        let definition = source.read(dc)?;
        if definition.u32(16)? != 0x80800362 {
            return Ok(None);
        }
        ensure!(
            source.reference(ic)? == 0x80800000,
            "Source network instance schema differs"
        );
        schema(
            &source.read(ic)?,
            0x808006F9,
            96,
            b"c_network_component_tag_instance\0",
        )?;
        schema(
            &definition,
            0x80800362,
            368,
            b"c_network_component_tag_definition\0",
        )?;
    }
    status::pair(&p, owner, live, d, ic, dc)?;
    for (offset, value) in [
        (0x18, 0),
        (0x20, u32::MAX as u64),
        (0x28, 0),
        (0x30, u32::MAX as u64),
        (0x38, u32::MAX as u64),
        (0x40, 0),
        (0x48, u32::MAX as u64),
        (0x50, 0),
        (0x58, 0),
    ] {
        ensure!(
            p.u64(d + offset)? == value,
            "Source network has additional host state"
        );
    }
    let native_graph = native.tag(native_tag, Some(0x80809C0F))?;
    let native_owners = native_graph.array(16, 12, Some(0x80809C04))?;
    ensure!(
        native_owners.len() == owner_count,
        "Native network graph owner count differs"
    );
    let tag = native_graph.u32(native_owners[ordinal])?;
    let target = native.tag(tag, Some(0x80809C36))?;
    let (ni, nd) = (target.pointer(16)?, target.pointer(24)?);
    let (nic, ndc) = (target.u32(ni - 4)?, target.u32(nd - 4)?);
    native_schema(
        native,
        nic,
        0x808090E6,
        96,
        b"c_network_component_tag_instance\0",
    )?;
    native_schema(
        native,
        ndc,
        0x808090E8,
        344,
        b"c_network_component_tag_definition\0",
    )?;
    ensure!(
        target.u32(ni)? == tag
            && target.u32(ni + 4)? == ndc
            && target.u64(ni + 8)? == nd as u64
            && target.u32(nd)? == tag
            && target.u32(nd + 4)? == nic
            && target.u64(nd + 8)? == ni as u64,
        "Native network pair differs"
    );
    let mut state = [0; 72];
    for at in [12, 20, 24] {
        state[at..at + 4].fill(255);
    }
    state[28..32].copy_from_slice(&0x811C9DC5u32.to_le_bytes());
    state[32..36].copy_from_slice(&[255, 0, 255, 255]);
    ensure!(
        p.bytes::<72>(live + 24)? == state
            && target.bytes::<72>(ni + 24)? == state
            && target.u64(ni + 16)? == 0,
        "Network instance has active state"
    );
    for (offset, value) in [
        (16, 0),
        (24, u64::MAX),
        (32, 0),
        (40, u64::MAX),
        (48, 0),
        (56, 0),
        (64, 0),
    ] {
        ensure!(
            target.u64(nd + offset)? == value,
            "Native network has additional host state"
        );
    }
    for &(offset, si, sc, ni, nc, method) in &PROVIDERS {
        for (payload, base, at, metadata, interface, instance, implementation, rows_class) in [
            (
                &p,
                d,
                d + offset,
                source.read(p.u32(d + offset + 8)?)?,
                si,
                sc,
                0x808006F9,
                0x808005C0,
            ),
            (
                &target,
                nd,
                nd + offset - 24,
                (*native.tag(target.u32(nd + offset - 16)?, Some(0x80809C54))?).clone(),
                ni,
                nc,
                0x808090E6,
                0x80809C56,
            ),
        ] {
            let methods = metadata.array(16, 24, Some(rows_class))?;
            ensure!(
                payload.pointer(at)? == base
                    && payload.u32(at + 12)? == 0
                    && payload.u64(at + 16)? == 0
                    && metadata.u64(0)? == metadata.0.len() as u64
                    && metadata.u32(8)? == interface
                    && metadata.u32(12)? == instance
                    && methods.len() == 1
                    && metadata.u32(methods[0])? == implementation
                    && metadata.u32(methods[0] + 4)? == method
                    && metadata.bytes::<16>(methods[0] + 8)? == [0; 16],
                "Network provider dispatch differs"
            );
        }
    }
    ensure!(
        p.u64(d + 0x78)? == 0x8080083E
            && p.u64(d + 0x98)? == 0x80800F3A
            && target.u64(nd + 0x60)? == 0x808090EB
            && target.u64(nd + 0x80)? == 0x808090EA
            && p.bytes::<68>(d + 0x118)? == target.bytes::<68>(nd + 0x100)?
            && p.u32(d + 0x15C)? == 0
            && p.u64(d + 0x160)? == 0
            && p.u64(d + 0x168)? == u32::MAX as u64
            && target.u32(nd + 0x144)? == 0
            && target.u64(nd + 0x148)? == 0
            && target.u64(nd + 0x150)? == u32::MAX as u64,
        "Network tuning or attachment requires translation"
    );
    lifecycle(&p, false)?;
    lifecycle(&target, true)?;
    exports(graph, &p, owner, false, ordinal, exclusion)?;
    exports(&native_graph, &target, tag, true, ordinal, exclusion)?;
    let allocation = native.tag(target.u32(0x44)?, None)?;
    ensure!(
        allocation.0.len() == 48
            && allocation.u64(0)? == 48
            && allocation.u64(8)? == 0x811C9DC5
            && allocation.u64(16)? == 0
            && allocation.u64(24)? == u32::MAX as u64
            && allocation.bytes::<16>(32)? == [0; 16],
        "Native network allocation is not empty"
    );
    Ok(Some(owner))
}

pub(super) fn modifier(
    source: &Source,
    native: &mut Reader,
    graph: &Payload,
) -> Result<(u32, Vec<NativeAssetResourcePatch>)> {
    // This complete stock graph has a network owner followed by one constant row.
    // Keeping its owner count and allocation avoids constructing a new replication layout.
    const ENVELOPE: u32 = 0x80C19433;
    let owners = graph.array(0x20, 12, Some(0x80800715))?;
    ensure!(
        owners.len() == 2,
        "Source network modifier owner count differs"
    );
    companion(
        source,
        native,
        graph,
        Layout {
            graph: ENVELOPE,
            owners: 2,
            ordinal: 0,
            exclusion: 1,
            built_in: false,
        },
    )?
    .context("Source modifier companion is not the qualified network component")?;
    let rows = modifier_owner(source, graph, graph.u32(owners[1])?)?;
    ensure!(
        rows.len() == 1,
        "Network modifier bank has no equal-sized native envelope"
    );
    let target = native.tag(ENVELOPE, Some(0x80809C0F))?;
    let native_owners = target.array(0x10, 12, Some(0x80809C04))?;
    ensure!(
        native_owners.len() == 2,
        "Native network modifier owner count differs"
    );
    for (payload, shift, count) in [(graph, 0x10, 11), (&*target, 0, 12)] {
        let masks = payload.array(0x78 + shift, 2, None)?;
        ensure!(
            payload.bytes::<32>(0x20 + shift)? == [0; 32]
                && masks.len() == 2
                && masks.iter().all(|&at| payload.u16(at).ok() == Some(0))
                && payload.array(0x58 + shift, 40, None)?.len() == count,
            "Network modifier graph has connections, masks or extra exports"
        );
    }
    let patches = modifier_patches(native, &target, target.u32(native_owners[1])?, &rows)?;
    Ok((ENVELOPE, patches))
}
