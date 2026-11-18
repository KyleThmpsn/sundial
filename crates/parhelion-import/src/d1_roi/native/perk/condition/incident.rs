//! A source kill incident Shadowkeep does not register becomes a native kill condition.
//!
//! The source registry `81656529` derives each kill incident from the engine kill event
//! `790C2A95` through `808003C2` bodies, each with a parent name and a `808021A3` filter. The
//! filter's block at `+40` has the layout of the kill condition's damage filter, and its label
//! filter at `+2C8` the layout of the kill condition's victim labels. Shadowkeep's event table
//! derives the same `9B60AB3F` from the same kill event, but holds no child with a damage type
//! beside the precision label, and none with the precision label and a `taken` victim. A talent
//! listening for such an incident with context `killing_player` instead takes the kill
//! condition that armor kill talents already use, carrying the incident's own lists.
//!
//! This replaces the listener, so it is an adaptation. The meaning of the `9B60AB3F` scalar
//! prefix is not decoded. An incident that adds no victim label takes the victim list of the
//! kill talents that share its grids and its description of the target.
use super::*;

const REGISTRY: u32 = 0x8165_6529;
const KILL: u32 = 0x790C_2A95;
/// The player-kills-combatant incident every adapted incident derives from.
const BASE: u32 = 0x9B60_AB3F;
const KILLING_PLAYER: u32 = 0x4FE1_9919;
/// `hive`, `cabal`, `fallen`, `taken` and `vex`: the match-any victim list of the artifact kill
/// talents, such as action `80A7E088`, whose descriptions name the same targets.
const DARKNESS: [u32; 5] = [
    0xC2A4_EF43,
    0x208C_606E,
    0x6187_D46F,
    0x4F33_FBC2,
    0x2A68_2C12,
];
/// List descriptors outside the damage block and the victim label filter.
const OTHER: [usize; 17] = [
    0xD0, 0xE0, 0xF0, 0x100, 0x1A8, 0x1B8, 0x1C8, 0x1D8, 0x280, 0x290, 0x2A0, 0x2B0, 0x3A0, 0x3B0,
    0x3C0, 0x3D0, 0x3E8,
];
const DAMAGE: usize = 0x40;
const VICTIM: usize = 0x2C8;

/// The incident's filter and its parent name.
fn filter(registry: &Payload, rows: &[usize], name: u32) -> Result<Option<(usize, u32)>> {
    let Some(&row) = rows
        .iter()
        .find(|&&row| registry.u32(row).ok() == Some(name))
    else {
        return Ok(None);
    };
    let body = registry.pointer(row + 8)?;
    if registry.u32(body - 4)? != 0x8080_03C2 || registry.u64(body + 8)? == 0 {
        return Ok(None);
    }
    let filter = registry.pointer(body + 8)?;
    ensure!(
        registry.u32(filter - 4)? == 0x8080_21A3,
        "Source incident filter has another class"
    );
    Ok(Some((filter, registry.u32(body)?)))
}

fn empty(registry: &Payload, filter: usize, descriptors: &[usize]) -> Result<bool> {
    for &at in descriptors {
        if registry.u64(filter + at)? != 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The native kill condition for the game event condition at `at`, or `None` when its event
/// is not a qualified child of the kill incident.
pub(super) fn kill(
    source: &Source,
    p: &Payload,
    at: usize,
    mut native: Option<&mut Reader>,
) -> Result<Option<NativeNode>> {
    if p.u32(at + 12)? != KILLING_PLAYER {
        return Ok(None);
    }
    let registry = source.read(REGISTRY)?;
    let rows = registry.array(0x20, 32, Some(0x8080_03D3))?;
    let Some((own, parent)) = filter(&registry, &rows, p.u32(at + 8)?)? else {
        return Ok(None);
    };
    let Some((base, root)) = filter(&registry, &rows, parent)? else {
        return Ok(None);
    };
    let lists: Vec<usize> = (0..4).map(|list| list * 16).collect();
    let damage: Vec<usize> = lists.iter().map(|list| DAMAGE + list).collect();
    let victim: Vec<usize> = lists.iter().map(|list| VICTIM + list).collect();
    // The base adds nothing decoded, and the incident differs from it only in the two blocks.
    if parent != BASE
        || root != KILL
        || !empty(&registry, base, &OTHER)?
        || !empty(&registry, base, &damage)?
        || !empty(&registry, base, &victim)?
        || !empty(&registry, own, &OTHER)?
        || registry.bytes::<0x40>(own)? != registry.bytes::<0x40>(base)?
        || empty(&registry, own, &damage)?
    {
        return Ok(None);
    }
    ensure!(
        (0.0..=1.0).contains(&p.f32(at)?) && p.u8(at + 4)? == 0xFF && p.u8(at + 6)? == 0,
        "Source kill incident has unqualified probability or state"
    );
    let mut bytes = vec![0; 0x158];
    bytes[..8].copy_from_slice(&p.bytes::<8>(at)?);
    bytes[5] = 2;
    bytes[7] = 0;
    for offset in [0x144, 0x148] {
        put(&mut bytes, offset, &0x811C_9DC5u32.to_le_bytes())?;
    }
    // The two floats every artifact kill talent stores at source `+1B8`.
    put(&mut bytes, 0x150, &0.001f32.to_le_bytes())?;
    put(&mut bytes, 0x154, &(-1.0f32).to_le_bytes())?;
    bytes[0x58] = 0xFF;
    let mut graph = Graph {
        blocks: vec![Block {
            class: 0x8080_3DE7,
            count: None,
            bytes,
            links: BTreeMap::new(),
        }],
    };
    if empty(&registry, own, &victim)? {
        label::any(&DARKNESS, &mut graph, 8, native.as_deref_mut())?;
    } else {
        label::translate(
            source,
            &registry,
            own + VICTIM,
            &mut graph,
            8,
            native.as_deref_mut(),
        )?;
    }
    filter::damage(
        source,
        &registry,
        own + DAMAGE,
        own + 0x110,
        &mut graph,
        0xB0,
        native,
    )?;
    graph
        .validate_node(NodeKind::Condition(2))
        .map_err(anyhow::Error::msg)?;
    Ok(Some(NativeNode {
        kind: 2,
        bytes: graph.emit().map_err(anyhow::Error::msg)?,
    }))
}
