//! Source projectiles without a render model on a native carrier that draws no model either.
//!
//! A Reach bullet or plasma bolt has no render model. Its flight shows only through its attached
//! effects. The carrier is the stock Fallen arc bolt `80C1BF02`, chosen by structure: an object
//! type 18 entity with no render (`808072BD`) or marker (`80808507`) owner, one movement whose
//! instant-trace bit 20 at definition `+0x48` is clear with `FFFFFFFF` at `+0x6C`, so it flies,
//! no detonation graph at `+0x180`, contact responses whose damage actions (selector 9) name
//! `FFFFFFFF` and so deal the firing weapon's damage, an impact table at `+0x1E0`, and one effect
//! flow whose locators name `root` (kind 0) and `world_space` (kind 1). That flow draws the stock
//! bolt with its systems bound to `root`, the projectile itself. The entity has no replication
//! record at `+0x88`, and neither does its private copy, as stock unreplicated carriers such as
//! `80BBB0DB` are fired.
//!
//! Two carrier parts belong to the Fallen bolt rather than to a source projectile. The steering
//! pair, definitions `8080377D` and `80803784`, only consumes the movement and the channel bank,
//! and its steering has no angular-rate implementation at `+0x160` to take a source turn rate, so
//! every source leaves it out. A guided source takes the guided grenade's steering and target
//! acquisition in its place, bound to the bolt's movement and bank as a modeled guided carrier's
//! are. The flow that only plays a sound event (`80806B38`) is the bolt's own flight sound, which
//! no source declares.
//!
//! The bolt's movement `815B39AC` answers contacts in two response groups. Its default group
//! takes every material through the root fallback and deals damage and requests a stop. Its last
//! group, scanned first, bounces and steers (selector 17) off native material `80BFDC95`, an
//! energy record. That is the arc bolt's rule, not a source's, so the private movement drops it
//! and the source's default rule meets every surface. A source that bounces off its default
//! material, or off Reach metal or terrain, takes the group back with its own filter, and a source
//! with a second such rule adds a copy of the group, as the ricochet host describes.
use super::*;
use crate::tiger::entity::links;

/// The stock Fallen arc bolt.
pub(super) const CARRIER: u32 = 0x80c1_bf02;
const RENDER: u32 = 0x8080_72bd;
const MARKERS: u32 = 0x8080_8507;
const MOVEMENT: u32 = 0x8080_388f;
const FLOW: u32 = 0x8080_84e9;
const SOUND: u32 = 0x8080_6b38;
const STEERING: [u32; 2] = [0x8080_377d, 0x8080_3784];
/// Movement definition `+0x48` bit 20, the instant trace.
const TRACE: u32 = 0x0010_0000;
/// Response selectors that bounce: 2 and its steering form 17.
const BOUNCES: [u8; 2] = [2, 17];

/// A scene for the source projectile `request` names: its object and a point model with no
/// geometry, markers or bones. Effect conversion then finds the projectile origin and refuses a
/// source marker, since the carrier has none.
pub(super) fn scene(cache: &cache::Cache, request: &ExportRequest) -> Result<Scene> {
    let tag = cache.find(&request.group, &request.path)?;
    let object = Object::read(cache, tag)?;
    ensure!(
        object
            .world
            .as_ref()
            .and_then(|branch| branch.model.as_ref())
            .is_none(),
        "An effect-only source projectile has a render model"
    );
    let model = model::Model {
        tag: object.tag.clone(),
        bones: Vec::new(),
        markers: Vec::new(),
        regions: Vec::new(),
        selected: BTreeMap::new(),
        materials: Vec::new(),
        primitives: Vec::new(),
    };
    Ok(Scene {
        object,
        models: vec![PlacedModel {
            model,
            animation: None,
            parent: None,
        }],
        limits: vec![
            "The source projectile has no render model. Its flight shows only through its attached effects."
                .into(),
        ],
        scale: 1.,
        damage: None,
        mounts: Vec::new(),
    })
}

/// Whether flow `p` plays only sound events.
fn sound_only(p: &Payload) -> Result<bool> {
    let definition = p.pointer(24)?;
    let classes = p
        .array(definition + 0x168, 24, Some(0x8080_93e6))?
        .into_iter()
        .map(|row| p.u32(p.pointer(row + 16)? - 4))
        .collect::<Result<Vec<_>>>()?;
    Ok(!classes.is_empty() && classes.iter().all(|&class| class == SOUND))
}

/// The carrier's components for an effect-only source, with the carrier report in the assembly's
/// report. A guided source steers with grafted guidance. Where the graft fails it is refused
/// unless `unguided` lets it fly without its steering, and the report's `steering` says why.
pub(super) fn prepare(
    reader: &mut Reader,
    carrier: u32,
    behavior: &Value,
    unguided: bool,
) -> Result<assembly::Assembly> {
    ensure!(
        shard::Shards::read(behavior)?.is_none(),
        "An effect-only projectile with attached shards needs a modeled carrier's assembly"
    );
    let mut assembly = assembly::Assembly::read(reader, carrier)?;
    ensure!(
        assembly.entity.u16(0x96)? == 18,
        "The effect-only carrier is not a projectile entity"
    );
    let mut steering = BTreeSet::new();
    let mut sounds = BTreeSet::new();
    for (&tag, p) in &assembly.owners {
        let class = assembly::Assembly::class(p)?;
        ensure!(
            !matches!(class, RENDER | MARKERS),
            "The effect-only carrier draws a model through component {tag:08X}"
        );
        if STEERING.contains(&class) {
            steering.insert(tag);
        }
        if class == FLOW && sound_only(p)? {
            sounds.insert(tag);
        }
    }
    let movement = assembly.one(MOVEMENT)?;
    let p = &assembly.owners[&movement];
    let definition = p.pointer(24)?;
    let flags = p.u32(definition + 0x48)?;
    ensure!(
        flags & TRACE == 0 && p.u32(definition + 0x6c)? == u32::MAX,
        "The effect-only carrier's movement is an instant trace"
    );
    let omitted = steering.union(&sounds).copied().collect::<BTreeSet<_>>();
    let steered = guide(reader, &mut assembly, &omitted, behavior, unguided)?;
    let unbounced = unfiltered(
        assembly
            .owners
            .get_mut(&movement)
            .context("Effect-only carrier movement")?,
    )?;
    let hex = |tags: &BTreeSet<u32>| {
        tags.iter()
            .map(|tag| format!("{tag:08X}"))
            .collect::<Vec<_>>()
    };
    assembly.report = json!({"carrier":format!("{carrier:08X}"),
        "movement_flags":format!("{flags:08X}"),
        "replicated":assembly.entity.u32(0x88)? != u32::MAX,
        "omitted_steering":hex(&steering),"omitted_sound_flows":hex(&sounds),
        "disabled_material_bounces":unbounced,"steering":steered,
        "basis":"A stock projectile with no render or marker owner, a moving movement, no detonation graph, contact damage from the firing weapon and an effect flow bound to the projectile root"});
    Ok(assembly)
}

/// Leaves `omitted` out of `assembly` and, for a guided source, grafts the guided grenade's
/// steering and target acquisition in place of the carrier's own pair. Returns the steering
/// receipt, null for an unguided source.
fn guide(
    reader: &mut Reader,
    assembly: &mut assembly::Assembly,
    omitted: &BTreeSet<u32>,
    behavior: &Value,
    unguided: bool,
) -> Result<Value> {
    if !tracking::enabled(behavior)? {
        if !omitted.is_empty() {
            omit(reader, assembly, omitted)?;
        }
        return Ok(Value::Null);
    }
    let error = match assembly::steered(reader, assembly, omitted, behavior) {
        Ok(steered) => {
            let report = steered.report.clone();
            *assembly = steered;
            return Ok(json!({"status":"translated","graft":report,
                "basis":"The guided grenade's steering and acquisition read the carrier's movement through the same provider, and its turn rate binds to the movement's speed and distance through the carrier's channel bank"}));
        }
        Err(error) => error,
    };
    ensure!(
        unguided,
        "The effect-only carrier cannot take the source's guidance: {error:#}"
    );
    if !omitted.is_empty() {
        omit(reader, assembly, omitted)?;
    }
    Ok(
        json!({"status":"native","reason":format!("The guidance did not graft, so the projectile flies unguided: {error:#}")}),
    )
}

/// Disables the movement's bounces in groups filtered to named native materials and returns how
/// many nodes it disabled. Probability zero at node `+C0` makes `CEB970` reject a node before its
/// actions, and `CEB5D0` then passes the contact to the earlier groups.
fn unfiltered(p: &mut Payload) -> Result<usize> {
    let definition = p.pointer(24)?;
    let views = BTreeSet::from([definition, usize::try_from(p.u64(p.pointer(16)? + 8)?)?]);
    let mut nodes = BTreeSet::new();
    for &view in &views {
        for group in p.array(view + 0x218, 88, Some(0x8080_37cd))? {
            if p.u64(group + 0x18)? == 0 {
                continue;
            }
            for node in p.array(group + 0x48, 392, Some(0x8080_37d0))? {
                for action in p.array(node + 0x178, 16, Some(0x8080_37f7))? {
                    if BOUNCES.contains(&p.u8(action + 8)?) {
                        nodes.insert(node);
                    }
                }
            }
        }
    }
    for &node in &nodes {
        program::constant(p, node + 0xc0, 0.)?;
    }
    Ok(nodes.len())
}

/// Leaves components `omitted` out of `assembly`, with every connection row that ends at one.
/// Fails when an omitted component feeds a kept one.
/// Leaves the `omitted` components out of `assembly`, with every connection that ends at one.
/// Fails when one feeds a kept component. Endpoints outside the graph, such as the impact
/// grenade's `80C70C06`, keep their rows.
pub(super) fn omit(
    reader: &mut Reader,
    assembly: &mut assembly::Assembly,
    omitted: &BTreeSet<u32>,
) -> Result<()> {
    let mut graph = links::Graph::read(&assembly.entity, false)?;
    ensure!(
        !graph
            .connections
            .iter()
            .chain(&graph.named_connections)
            .any(|row| row
                .provider
                .object
                .is_some_and(|object| omitted.contains(&object.owner))),
        "An omitted carrier component feeds another component"
    );
    let ends = |row: &links::Connection| {
        [row.consumer.object, row.provider.object]
            .into_iter()
            .flatten()
            .any(|object| omitted.contains(&object.owner))
    };
    graph.components.retain(|tag| !omitted.contains(tag));
    graph.connections.retain(|row| !ends(row));
    graph.named_connections.retain(|row| !ends(row));
    for tag in omitted {
        assembly
            .owners
            .remove(tag)
            .context("Omitted carrier component is absent")?;
    }
    let mut endpoints = assembly.owners.clone();
    for object in graph.objects() {
        if let std::collections::btree_map::Entry::Vacant(entry) = endpoints.entry(object.owner) {
            entry.insert((*reader.tag(object.owner, Some(0x8080_9c36))?).clone());
        }
    }
    let rows = graph.native_rows(
        &graph.components,
        &endpoints,
        Ok,
        |_, channel| Ok(channel),
        |_, selector| Ok(selector),
    )?;
    let entity = {
        let components = graph
            .components
            .iter()
            .map(|tag| compiled::Component {
                owner: *tag,
                payload: &assembly.owners[tag],
                template: &assembly.owners[tag],
                entity: &assembly.entity,
            })
            .collect::<Vec<_>>();
        compiled::emit(&assembly.entity, &components, &rows)?
    };
    assembly.entity = entity;
    Ok(())
}
