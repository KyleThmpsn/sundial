//! A retained, indefinite Duration host and its complete constant modifier bank.
use super::*;

/// The native carriers keep both owners under one entity lifetime. Row counts select
/// allocation layouts only. Every numeric row and the complete duration policy are replaced.
pub(super) fn read(
    source: &Source,
    native: &mut Reader,
    graph: &Payload,
) -> Result<NativeAssetPatch> {
    let owners = graph.array(32, 12, Some(0x80800715))?;
    ensure!(
        owners.len() == 2 && graph.bytes::<32>(48)? == [0; 32],
        "Connected duration graph needs another owner or connection contract"
    );
    let host = graph.u32(owners[0])?;
    let bank = graph.u32(owners[1])?;
    ensure!(
        host != bank && source.reference(host)? == 0x80800861,
        "Connected duration owner differs"
    );
    let indices = graph.array(0x88, 2, Some(0x80800006))?;
    ensure!(
        indices.len() == 2 && indices.iter().all(|&at| graph.u16(at).ok() == Some(0)),
        "Connected duration indices differ"
    );
    let p = source.read(host)?;
    let policy = duration(graph, &p, host)?;
    let rows = modifier_owner(source, graph, bank)?;
    ensure!(
        graph.u64(0x68)? == (9 + 2 + rows.len() * 2) as u64,
        "Connected duration graph has additional exports"
    );
    let tag = match rows.len() {
        1 => 0x80BC581E,
        2 => 0x80C193CA,
        3 => 0x80BC5841,
        4 => 0x80BC57F9,
        5 => 0x80BC57F6,
        // Larger banks keep their complete standalone native owner. The private
        // graph composer replaces this carrier's bank without changing either
        // owner's live allocation or separating their shared entity lifetime.
        _ => 0x80BC581E,
    };
    let graph = native.tag(tag, Some(0x80809C0F))?;
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    ensure!(
        owners.len() == 2 && graph.bytes::<16>(48)? == [0; 16],
        "Native connected duration owners or inputs differ"
    );
    let host = graph.u32(owners[0])?;
    let bank = graph.u32(owners[1])?;
    let target = native.tag(host, Some(0x80809C36))?;
    let (s, live) = (target.pointer(24)?, target.pointer(16)?);
    let d = s + 0x48;
    let timer = d + 0xB0;
    let expression = d + 0x128;
    let state = s + 0x1D0;
    ensure!(
        target.u32(s - 4)? == 0x80803B30 && target.u32(live - 4)? == 0x80803B2F,
        "Native duration root differs"
    );
    for (i, def, ic, dc) in [
        (live, s, 0x80803B2F, 0x80803B30),
        (live + 0x30, d, 0x80803B32, 0x80803B33),
        (live + 0x60, timer, 0x80803C50, 0x80803C51),
        (live + 0x80, expression, 0x808089F5, 0x808089F6),
        (live + 0xB0, state, 0x808092F3, 0x808092F4),
    ] {
        ensure!(
            target.u32(def)? == host
                && target.u32(def + 4)? == ic
                && target.u64(def + 8)? == i as u64
                && target.u32(i)? == host
                && target.u32(i + 4)? == dc
                && target.u64(i + 8)? == def as u64,
            "Native duration pair differs"
        );
    }
    for (offset, value) in [
        (0x10, 0),
        (0x18, u64::MAX),
        (0x20, 0),
        (0x28, u64::MAX),
        (0x30, 0),
        (0x38, 0),
        (0x40, 0),
    ] {
        ensure!(
            target.u64(s + offset)? == value,
            "Native duration has additional host state"
        );
    }
    ensure!(
        target.u64(d + 0x28)? == 0x80803BD0
            && target.u64(d + 0x48)? == 0x80804C54
            && target.u64(d + 0xA8)? == 0
            && target.u64(timer + 0x70)? == 1,
        "Native duration has additional timer behavior"
    );
    for (container, offset, declaration, selector) in [
        (d, 0x10, 0x80C23835u32, 0u64),
        (d, 0x30, 0x815B887A, 0),
        (d, 0x50, 0x80C23836, 0),
        (d, 0x68, 0x80C23837, 0),
        (d, 0x80, 0x815B887B, 0),
        (timer, 0x10, 0x80C2382F, 0xFF9C),
        (timer, 0x28, 0x80C23830, 0),
        (timer, 0x40, 0x80C23831, 0),
        (timer, 0x58, 0x80C23832, 0),
    ] {
        let at = container + offset;
        ensure!(
            target.pointer(at)? == container
                && target.u64(at + 8)? == declaration as u64
                && target.u64(at + 16)? == selector,
            "Native duration parameter has another driver"
        );
    }
    ensure!(
        target.u64(state + 16)? == 2
            && target.u64(state + 24)? == 0x808092F7
            && target.u64(state + 32)? == 0
            && target.u64(state + 40)? == u32::MAX as u64,
        "Native duration has a state program"
    );
    let wires = graph.array(32, 72, Some(0x80809BC9))?;
    ensure!(wires.len() == 1, "Native duration has extra connections");
    let wire = wires[0];
    ensure!(
        graph.u64(wire)? == 0x811C9DC5
            && graph.u32(wire + 8)? == host
            && graph.u32(wire + 12)? == 0x808092F4
            && graph.u64(wire + 16)? == state as u64
            && graph.u64(wire + 24)? == 0
            && graph.u64(wire + 32)? == 0x811C9DC5
            && graph.u64(wire + 40)? == u64::MAX
            && graph.u64(wire + 48)? == 0
            && graph.u64(wire + 56)? == 65535
            && graph.u64(wire + 64)? == 0,
        "Native duration has a live connection"
    );
    indefinite(&target, expression, false)?;
    // The shared Duration interface identifies its embedded runtime resource, even
    // though the root component also exports timer and empty-state interfaces.
    let bindings = weapon_component_bindings(&graph.0, 0x09871285).map_err(anyhow::Error::msg)?;
    ensure!(
        bindings.len() == 1
            && bindings[0].owner_tag == host
            && bindings[0].concrete_class == 0x80803B32
            && bindings[0].resource_offset == (live + 0x30) as u64,
        "Native duration binding differs"
    );
    let binding = &bindings[0];
    let (mut patches, components, remove_owners) = if rows.len() <= 5 {
        (
            modifier_patches(native, &graph, bank, &rows)?,
            Vec::new(),
            Vec::new(),
        )
    } else {
        let (source_graph, patches) = modifier_patch(native, &rows)?;
        allocation(native, source_graph, rows.len())?;
        (
            Vec::new(),
            vec![NativeAssetComponent {
                source_graph,
                patches,
                replication: None,
            }],
            vec![bank],
        )
    };
    patches.push(NativeAssetResourcePatch {
        binding_hash: binding.binding_hash,
        resource_index: u16::try_from(binding.resource_index)?,
        offset: u32::try_from(d + 0x98 - usize::try_from(binding.resource_offset)?)?,
        expected: target.bytes::<20>(d + 0x98)?.to_vec(),
        bytes: policy.to_vec(),
        imported_particle: None,
    });
    Ok(NativeAssetPatch {
        action_index: 0,
        source_graph: tag,
        patches,
        appends: Vec::new(),
        remove_owners,
        components,
        programs: Vec::new(),
    })
}

/// The complete stock owner supplies live rows and their lifecycle, not only
/// a settings array. Its parameter providers remain inside the same owner.
pub(super) fn allocation(native: &mut Reader, tag: u32, count: usize) -> Result<()> {
    let graph = native.tag(tag, Some(0x80809C0F))?;
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    ensure!(
        owners.len() == 1
            && graph.bytes::<32>(32)? == [0; 32]
            && graph.u64(0x78)? == 0
            && graph.u32(0x88)? == u32::MAX,
        "Native modifier allocation has another graph contract"
    );
    let owner = graph.u32(owners[0])?;
    let p = native.tag(owner, Some(0x80809C36))?;
    let (live, definition) = (p.pointer(16)?, p.pointer(24)?);
    let extent = definition
        .checked_sub(live)
        .context("Native modifier source extent underflows")?;
    let prefix = live
        .checked_sub(p.pointer(8)?)
        .context("Native modifier prefix extent underflows")?;
    ensure!(
        p.u32(live - 4)? == 0x80803AFD
            && p.u32(definition - 4)? == 0x80803AFE
            && p.u64(0x48)? == extent as u64
            && p.u64(0x50)? == prefix as u64
            && p.array(32, 8, Some(0x8080919F))?.is_empty(),
        "Native modifier allocation has another source extent"
    );
    for (instance, settings, instance_class, settings_class) in [
        (live, definition, 0x80803AFD, 0x80803AFE),
        (live + 48, definition + 72, 0x80803B00, 0x80803B01),
    ] {
        ensure!(
            p.u32(instance)? == owner
                && p.u32(settings)? == owner
                && p.u32(instance + 4)? == settings_class
                && p.u32(settings + 4)? == instance_class
                && p.u64(instance + 8)? == settings as u64
                && p.u64(settings + 8)? == instance as u64,
            "Native modifier root pair differs"
        );
    }
    let rows = p.array(definition + 88, 88, Some(0x80803B06))?;
    let instances = p.array(live + 80, 128, Some(0x80803B05))?;
    let lifecycle = p.array(48, 16, Some(0x808091A4))?;
    ensure!(
        rows.len() == count && instances.len() == count && lifecycle.len() == count + 1,
        "Native modifier allocation or lifecycle count differs"
    );
    let allocation = native.tag(p.u32(0x44)?, Some(0x80809BBB))?;
    ensure!(
        allocation.u64(8)? == 0x811C9DC5
            && allocation.u64(16)? == 0
            && allocation.u64(24)? == u32::MAX as u64,
        "Native modifier allocation root differs"
    );
    let members = allocation.array(32, 40, Some(0x80808852))?;
    ensure!(
        members.len() == 1
            && allocation.u64(members[0])? == 0xFB05ED04
            && allocation.u64(members[0] + 8)? == 0
            && allocation.u64(members[0] + 16)? == u32::MAX as u64,
        "Native modifier allocation member differs"
    );
    let counted = allocation.array(members[0] + 24, 40, Some(0x80808852))?;
    ensure!(
        counted.len() == 1
            && allocation.u64(counted[0])? == 0x587BC3F9
            && allocation.u64(counted[0] + 8)? == 0
            && allocation.u32(counted[0] + 16)? == 0x80803B05
            && allocation.u32(counted[0] + 20)? as usize == count
            && allocation
                .array(counted[0] + 24, 40, Some(0x80808852))?
                .is_empty(),
        "Native modifier counted allocation differs"
    );
    for ((&row, &instance), &call) in rows.iter().zip(&instances).zip(&lifecycle) {
        ensure!(
            instance + 128 <= definition
                && p.u32(instance)? == owner
                && p.u32(instance + 4)? == 0x80803B06
                && p.u32(row)? == owner
                && p.u32(row + 4)? == 0x80803B05
                && p.u64(row + 8)? == instance as u64
                && p.u64(instance + 8)? == row as u64
                && p.pointer(call)? == instance
                && p.u32(call + 8)? == 0x808045BE
                && p.u16(call + 12)? == 0
                && p.u16(call + 14)? == 1,
            "Native modifier allocation needs another constructor or cleanup"
        );
    }
    let cleanup = lifecycle[count];
    ensure!(
        p.pointer(cleanup)? == live + 48
            && p.u32(cleanup + 8)? == 0x80803B00
            && p.u16(cleanup + 12)? == u16::MAX
            && p.u16(cleanup + 14)? == 1,
        "Native modifier bank cleanup differs"
    );
    let maps = graph.array(0x58, 40, Some(0x80809C22))?;
    let refs = graph.array(0x68, 24, Some(0x80809C20))?;
    ensure!(
        maps.len() == count * 2 + 3 && refs.len() == maps.len(),
        "Native modifier exports differ"
    );
    let mut expected = BTreeSet::from([
        (live + 48, definition + 0x68, 0x80BFD9EB),
        (live + 48, definition + 0x80, 0x80BFD9EC),
        (live + 48, definition + 0xA0, 0x80BFD9ED),
    ]);
    for (&row, &instance) in rows.iter().zip(&instances) {
        expected.extend([
            (instance, row + 16, 0x80BFD9F1),
            (instance, row + 48, 0x80BFD9EE),
        ]);
    }
    for (&map, &reference) in maps.iter().zip(&refs) {
        let instance = usize::try_from(graph.u64(reference + 8)?)?;
        let parameter = usize::try_from(graph.u64(map + 24)?)?;
        let provider = graph.u32(map + 32)?;
        ensure!(
            graph.u32(reference)? == owner
                && graph.u64(reference + 16)? == 0
                && graph.u32(map + 16)? == owner
                && graph.u32(map + 20)? == 0x80809C50
                && graph.u32(map)? == 0
                && graph.u32(map + 12)? == graph.u32(reference + 4)?
                && p.u64(parameter + 8)? == provider as u64
                && expected.remove(&(instance, parameter, provider)),
            "Native modifier provider partition differs"
        );
    }
    ensure!(
        expected.is_empty(),
        "Native modifier exports are incomplete"
    );
    for (key, size, bank) in [
        (0x7330E39F, count, false),
        (0x95A60F29, count + 1, true),
        (0xACD79FE6, 1, true),
        (0x245BEBBA, 1, true),
    ] {
        let bindings = weapon_component_bindings(&graph.0, key).map_err(anyhow::Error::msg)?;
        ensure!(
            bindings.len() == size
                && bindings.iter().all(|binding| {
                    binding.owner_tag == owner
                        && binding.component_index == 0
                        && ((binding.concrete_class == 0x80803B05
                            && instances.contains(&(binding.resource_offset as usize)))
                            || (bank
                                && binding.concrete_class == 0x80803B00
                                && binding.resource_offset == (live + 48) as u64))
                }),
            "Native modifier binding partition differs"
        );
    }
    Ok(())
}

fn duration(graph: &Payload, p: &Payload, owner: u32) -> Result<[u8; 20]> {
    let (s, live) = (p.pointer(24)?, p.pointer(16)?);
    let d = s + 0x60;
    let timer = d + 0xB8;
    ensure!(
        p.u32(s - 4)? == 0x808029C3 && p.u32(live - 4)? == 0x8080222E,
        "Source connected duration root differs"
    );
    for (i, def, ic, dc) in [
        (live, s, 0x8080222E, 0x808029C3),
        (live + 0x30, d, 0x8080255D, 0x8080223C),
        (live + 0x60, timer, 0x808027B5, 0x80802890),
        (live + 0x80, d + 0x138, 0x808008AC, 0x8080038D),
    ] {
        status::pair(p, owner, i, def, ic, dc)?;
    }
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
            p.u64(s + offset)? == value,
            "Source duration has additional host state"
        );
    }
    ensure!(
        p.u64(d + 0x30)? == 0x8080299F
            && p.u64(d + 0x50)? == 0x80802B20
            // Bit 2 admits equal-priority replacement. Bit 3 retains the same
            // bounded matching path but requires a strictly greater priority.
            && matches!(p.u32(d + 0xA0)?, 5 | 8)
            && p.u32(d + 0xA8)? > 0
            && p.u64(d + 0xB0)? == 0
            && p.u64(timer + 0x78)? == 1,
        "Source duration needs another matching or timer policy"
    );
    indefinite(p, d + 0x138, true)?;
    let declarations: [(usize, usize, usize, u32, u32, u64, u32); 9] = [
        (d + 0x18, d, live + 0x30, 0x8080255D, 0x80AAC286, 0u64, 0),
        (d + 0x38, d, live + 0x30, 0x8080255D, 0x80AAC284, 0, 0),
        (d + 0x58, d, live + 0x30, 0x8080255D, 0x80AAEDB3, 0, 0),
        (d + 0x70, d, live + 0x30, 0x8080255D, 0x80AAC288, 0, 0),
        (d + 0x88, d, live + 0x30, 0x8080255D, 0x80AAC444, 0, 0),
        (
            timer + 0x18,
            timer,
            live + 0x60,
            0x808027B5,
            0x80AAEDB4,
            0xFF9C,
            0,
        ),
        (
            timer + 0x30,
            timer,
            live + 0x60,
            0x808027B5,
            0x80AAC285,
            0,
            1,
        ),
        (
            timer + 0x48,
            timer,
            live + 0x60,
            0x808027B5,
            0x80AAC289,
            0,
            0,
        ),
        (
            timer + 0x60,
            timer,
            live + 0x60,
            0x808027B5,
            0x80AAC287,
            0,
            0,
        ),
    ];
    let exports = graph.array(0x68, 40, Some(0x80800370))?;
    let refs = graph.array(0x78, 24, Some(0x80800790))?;
    ensure!(
        exports.len() == refs.len(),
        "Connected duration reference count differs"
    );
    let mut seen = BTreeSet::new();
    for (&export, &reference) in exports.iter().zip(&refs) {
        if graph.u32(export + 16)? != owner {
            continue;
        }
        let at = usize::try_from(graph.u64(export + 24)?)?;
        let &(_, container, instance, class, declaration, selector, output) = declarations
            .iter()
            .find(|d| d.0 == at)
            .context("Unknown duration export")?;
        ensure!(
            p.pointer(at)? == container
                && p.u64(at + 8)? == declaration as u64
                && p.u64(at + 16)? == selector
                && graph.u32(export)? == output << 16
                && graph.u32(export + 4)? as usize == instance - live
                && graph.u32(export + 8)? == (selector as u16 as i16 as i32) as u32
                && graph.u32(export + 12)? == class
                && graph.u64(export + 16)? == owner as u64
                && graph.u64(export + 32)? == declaration as u64
                && graph.u64(reference)? == owner as u64
                && graph.u64(reference + 8)? == instance as u64
                && graph.u64(reference + 16)? == 0
                && seen.insert(at),
            "Source duration export has another driver or receiver"
        );
    }
    ensure!(
        seen.len() == declarations.len(),
        "Source duration exports are incomplete"
    );
    p.bytes(d + 0xA0)
}

fn indefinite(p: &Payload, at: usize, source: bool) -> Result<()> {
    let shift = if source { 8 } else { 0 };
    let code = p.array(at + 16 + shift, 1, Some(0x80800009))?;
    let values = p.array(at + 32 + shift, 16, Some(0x80800090))?;
    ensure!(
        code.len() == 4
            && code
                .iter()
                .zip([0x34, 0, 0x3E, 0])
                .all(|(&a, v)| p.u8(a).ok() == Some(v))
            && values.len() == 1
            && (0..4).all(|lane| p.u32(values[0] + lane * 4).ok() == Some((-1.0f32).to_bits()))
            && p.u64(at + 48 + shift)? == 1
            && p.u64(at + 56 + shift)? == 1
            && p.bytes::<24>(at + 64 + shift)? == [0; 24]
            && p.u64(at + 88 + shift)? == 0x811C9DC5,
        "Connected duration needs another lifetime expression"
    );
    Ok(())
}
