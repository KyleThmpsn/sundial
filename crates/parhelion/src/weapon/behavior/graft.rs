//! Reads the content owner a graft edits and fits the records, labels and graphs it copies.
use super::*;

/// Launch-speed boost applied when a graft gives a weapon projectiles it does not normally fire.
///
/// The private Micro-Missile captures behind `guided::profiles` established that this field is a
/// multiplier on the weapon's own launch speed rather than an absolute velocity, and that the
/// instance and definition copies must agree. Follow-up captures measured both halves of that
/// product. A Mountaintop launch supplied a launch input of 78.75 against a multiplier of one,
/// and a weapon that fires instantly supplied 1.26 against the same multiplier. Restoring a
/// launching frame's speed on a host that fires instantly therefore takes a multiplier near
/// 62.5, and a grafted round that does not get one crawls.
///
/// Kyle arrived at this figure from in-game testing on 2026-09-17, against sources whose own
/// multiplier reads one, so it is a working default rather than a measured constant, which is why
/// it stays editable per weapon. It is also the most a graft is raised to. A source already
/// reading more than this came from a frame that supplies no launch speed either, so it needs no
/// help, and scaling it as well is what drove every source above 149 into the sentinel.
pub const DEFAULT_PROJECTILE_SPEED_BOOST: f32 = 67.0;

/// The firing graph a weapon's own content block names, if it names one.
pub(super) fn host_graph(content: &Content, block: usize) -> Option<u32> {
    u32_at(&content.owner, block + GRAPH_OFFSET)
        .ok()
        .filter(|tag| *tag != 0 && *tag != u32::MAX)
}

/// Weapons that fire instantly carry this sentinel where a launch speed would be. Every stock
/// hitscan weapon's graph reads exactly this, and no weapon that launches anything does.
const HITSCAN_SPEED: f32 = 9999.0;

/// A graph's Projectile Movement binding: the projectile whose trajectory pool a host fills.
const PROJECTILE_MOVEMENT: u32 = 0x0437_756D;
/// The Barrel definition's spread pattern member, `m_spread`, a relative pointer to the pattern.
/// Every stock weapon that fires pellets sets it, Legend of Acrius and Tractor Cannon included, and
/// every other stock weapon leaves it zero, including Lord of Wolves, fusion rifles and bows.
const SPREAD_MEMBER: u32 = 0x3124_7F94;
/// The spread pattern's class, and where it keeps its total pellets per shot: 12 on every stock
/// pellet shotgun, the sum of its rings (1, 4 and 7), and 1 on Tractor Cannon.
const SPREAD_PATTERN: u32 = 0x8080_888D;
const SPREAD_PELLETS: usize = 0x60;

/// The trajectory pool a grafted graph's private clone needs on this host.
///
/// A weapon fires one trajectory per pellet, and the graph's projectile has a fixed pool of them.
/// The client looks each pellet's row up by index and reads through a missing one, so a pool
/// smaller than the host's pellets per shot froze the game on the second pellet: Thorn's graph,
/// with a pool of one like every graph that fires one round, on a shotgun firing twelve. The stock
/// graphs that fire pellets carry fifteen. A host that fires one round, or a graph with room
/// already, needs nothing.
pub(super) fn trajectory_need(
    manager: &PackageManager,
    host: &[u8],
    graph: u32,
) -> AuthoringResult<Option<u16>> {
    use sundial::package_authoring::entity::projectile_trajectory_capacity;
    let Some(pellets) = host_pellets(manager, host) else {
        return Ok(None);
    };
    let payload = manager
        .read_tag(TagHash(graph))
        .map_err(|error| invalid(error.to_string()))?;
    let Ok(projectiles) = weapon_component_bindings(&payload, PROJECTILE_MOVEMENT) else {
        return Ok(None);
    };
    let [projectile] = projectiles.as_slice() else {
        return Err(invalid(format!(
            "Graph 0x{graph:08X} has {} projectiles, so its trajectory pool cannot be fitted to a pellet weapon",
            projectiles.len()
        )));
    };
    let owner = manager
        .read_tag(TagHash(projectile.owner_tag))
        .map_err(|error| invalid(error.to_string()))?;
    let instance = usize::try_from(projectile.resource_offset)
        .map_err(|_| invalid("Projectile resource offset overflow"))?;
    let capacity = projectile_trajectory_capacity(&owner, projectile.owner_tag, instance)
        .map_err(|error| invalid(format!("Graph 0x{graph:08X}: {error}")))?;
    Ok((usize::from(pellets) > capacity).then_some(pellets))
}

/// The host's pellets per shot, when its Barrel carries a spread pattern laid out as the stock
/// ones are. A host without one, or one that cannot be read, fires one round as far as a graft is
/// concerned, as every graft assumed before.
fn host_pellets(manager: &PackageManager, host: &[u8]) -> Option<u16> {
    use sundial::package_authoring::entity::WEAPON_BARREL_COMPONENT_KEY;
    use sundial::package_authoring::runtime::{
        WeaponRuntimeValue, load_weapon_runtime_graph_for_entity,
    };
    let graph = load_weapon_runtime_graph_for_entity(manager, 0, 0, 0, host).ok()?;
    let (owner_tag, field, spread) = graph
        .resources
        .iter()
        .filter(|resource| resource.binding_hash == WEAPON_BARREL_COMPONENT_KEY)
        .flat_map(|resource| {
            std::iter::once(&resource.instance)
                .chain(resource.definition.iter())
                .flat_map(move |root| {
                    root.fields
                        .iter()
                        .map(move |field| (resource.owner_tag, field))
                })
        })
        .find_map(|(owner_tag, field)| {
            let spread = match field.value {
                WeaponRuntimeValue::Unsigned(spread) if spread != 0 => spread,
                _ => return None,
            };
            field
                .locator
                .path
                .last()
                .is_some_and(|step| step.name_hash == SPREAD_MEMBER)
                .then_some((owner_tag, field.owner_offset, spread))
        })?;
    let owner = manager.read_tag(TagHash(owner_tag)).ok()?;
    let pattern = usize::try_from(field)
        .ok()?
        .checked_add_signed(isize::try_from(spread as i64).ok()?)?;
    if u32_at(&owner, pattern).ok()? != SPREAD_PATTERN {
        return None;
    }
    u16::try_from(u32_at(&owner, pattern + SPREAD_PELLETS).ok()?)
        .ok()
        .filter(|pellets| *pellets > 1)
}

/// Values to apply inside a grafted graph's private clone, if it needs any.
///
/// A behavior graph is itself a weapon entity, so both sides are read the same way. The speed a
/// grafted round leaves at is the graph's own multiplier times the launch input its frame
/// supplies, and a weapon that fires instantly supplies almost none, which is why the round
/// crawls. Raising the multiplier in the private clone fixes that without touching the source
/// weapon or the host.
///
/// How much to raise it depends on the graph. Each stock multiplier is paired with the frame it
/// shipped on, so one from a launching frame reads at or below one and one from a frame that
/// fires instantly reads in the hundreds or thousands. Only the first sort loses anything to a
/// graft. So the boost is capped at itself: a graph reading one lands on the boost, a slower
/// graph lands proportionally below it, and a graph already above it keeps the number its own
/// weapon uses.
pub(super) fn graph_values(
    manager: &PackageManager,
    host_graph: Option<u32>,
    graph: u32,
    boost: f32,
) -> AuthoringResult<Vec<sundial::package_authoring::runtime::WeaponRuntimeValueOverride>> {
    // One is the neutral multiplier, so anything at or below it asks for no change. A hand-edited
    // recipe carrying NaN stops here too, rather than reaching the clamp, where `f32::min` would
    // quietly turn it into the largest value this can write.
    if boost.is_nan() || boost <= 1.0 {
        return Ok(Vec::new());
    }
    // A weapon that already launches something of its own supplies a real speed, so leave it be.
    if host_graph.is_some_and(|tag| launches_its_own(manager, tag)) {
        return Ok(Vec::new());
    }
    let Ok(payload) = manager.read_tag(TagHash(graph)) else {
        return Ok(Vec::new());
    };
    // Clamp before anything is measured against it, so a hand-typed figure above the sentinel can
    // neither reach hitscan nor lower a graph that already reads more than the clamp allows.
    let boost = boost.min(HITSCAN_SPEED - 1.0);
    let mut draft = Vec::new();
    for parameter in projectile_speeds(manager, graph, &payload)? {
        let base = parameter.original();
        // Scale by the boost, stop at the boost, and never end below what the graph already has.
        // Stopping at the boost is what spares a graph that came from a frame supplying no launch
        // speed of its own, the sentinel included. Ending at the base keeps this sound on a base
        // large enough to overflow the product.
        let raised = (base * boost).min(boost).max(base);
        // Nothing to write when the speed does not move, and no private clone is appended for it.
        if raised <= base {
            continue;
        }
        parameter.set(&mut draft, raised).map_err(invalid)?;
    }
    Ok(draft)
}

/// Whether a weapon's own firing graph launches something rather than hitting instantly.
pub(super) fn launches_its_own(manager: &PackageManager, graph: u32) -> bool {
    let Ok(payload) = manager.read_tag(TagHash(graph)) else {
        return false;
    };
    projectile_speeds(manager, graph, &payload).is_ok_and(|speeds| {
        speeds
            .iter()
            .any(|parameter| parameter.original() < HITSCAN_SPEED)
    })
}

/// The projectile launch-speed parameters an entity's runtime graph exposes, if any.
fn projectile_speeds(
    manager: &PackageManager,
    entity_tag: u32,
    entity: &[u8],
) -> AuthoringResult<
    Vec<sundial::package_authoring::sandbox_perk::entity::projectile::parameters::Parameter>,
> {
    use sundial::package_authoring::sandbox_perk::entity::projectile::parameters::{
        Kind, discover,
    };
    let loaded = sundial::package_authoring::runtime::load_weapon_runtime_graph_for_entity(
        manager, 0, 0, entity_tag, entity,
    )
    .map_err(invalid)?;
    Ok(discover(&loaded)
        .into_iter()
        .filter(|parameter| parameter.kind == Kind::Speed)
        .collect())
}

/// The variant block a content group selects, found by scanning an owner payload directly.
///
/// Used for a source owner, where there is no entity to resolve a binding through.
fn block_in_owner(owner: &[u8], group: u32) -> Option<usize> {
    let mut at = 0;
    while at + 0x20 <= owner.len() {
        if u32_at(owner, at + 4).ok()? == VARIANT_BLOCK_CLASS
            && u32_at(owner, at + 0x10).ok()? == group
        {
            return Some(at);
        }
        at += 4;
    }
    None
}

/// The rows of a block's label array: the labels its kill and hit events carry.
///
/// Every block dumped so far names the weapon type in row zero and the weapon's own labels after
/// it. Those own labels are what an exotic's perk keys on: Cosmology detonates only on a kill
/// carrying "bucket 2", which Graviton Lance's block holds and a legendary pulse rifle's does
/// not, so a graft that moved the graph, the record and the trait still never detonated.
#[cfg(test)]
pub(super) fn label_rows(owner: &[u8], block: usize) -> AuthoringResult<Vec<Vec<u8>>> {
    optional_label_rows(owner, block)?
        .ok_or_else(|| invalid("Weapon variant block has no label array"))
}

pub(super) fn optional_label_rows(
    owner: &[u8],
    block: usize,
) -> AuthoringResult<Option<Vec<Vec<u8>>>> {
    let Some(triple) = find_triple(owner, block)? else {
        return Ok(None);
    };
    let Some(target) = slot_target(owner, triple) else {
        return Ok(None);
    };
    if u32_at(owner, target + 8)? != LABEL_ARRAY_CLASS {
        return Err(invalid("Weapon label array has an unexpected class"));
    }
    let count = usize::try_from(u64_at(owner, triple + 8)?)
        .map_err(|_| invalid("Weapon label array count overflow"))?;
    (0..count)
        .map(|index| {
            let start = target + 16 + index * LABEL_ROW_SIZE;
            owner
                .get(start..start + LABEL_ROW_SIZE)
                .map(<[u8]>::to_vec)
                .ok_or_else(|| invalid("Weapon label array is truncated"))
        })
        .collect::<AuthoringResult<Vec<_>>>()
        .map(Some)
}

fn label_of(row: &[u8]) -> u32 {
    u32::from_le_bytes(
        row[..4]
            .try_into()
            .expect("a label row starts with its hash"),
    )
}

/// The block of a behavior's own source weapon, read through its runtime entity.
///
/// A record can be shared between weapons, so the record's block is not always the source
/// weapon's block, and only the source weapon's own block holds the labels its perk keys on.
pub(super) fn source_block(
    manager: &PackageManager,
    entry: &Behavior,
) -> AuthoringResult<(Vec<u8>, usize)> {
    use sundial::package_authoring::runtime::load_weapon_runtime_entity_with_manager;
    let runtime = load_weapon_runtime_entity_with_manager(manager, entry.source_item_hash)
        .map_err(|error| invalid(format!("{}: {error}", entry.source_name)))?;
    let source = content(manager, &runtime.payload)?;
    let block = block_for_group(&source, runtime.weapon_content_group_hash)?;
    Ok((source.owner, block))
}

/// Carries the source weapons' own labels onto the host.
///
/// A label array is a self-contained run, so a new one is appended holding the host's rows and
/// the source rows the host lacks, and the host's label slot is pointed at it. Row zero of each
/// source is its weapon type and stays behind: the host keeps its own type for every perk that
/// filters on one. When the host block is an appearance's, `kind_block` names the base weapon's
/// own block and its type replaces the appearance's. Nothing is appended when no source brings a
/// label the host lacks and the type is unchanged.
pub(super) fn label_append(
    content: &Content,
    host_block: usize,
    kind_block: Option<usize>,
    sources: &[(Vec<u8>, usize)],
) -> AuthoringResult<Option<crate::item::WeaponRuntimeResourceAppend>> {
    // Labels are an addition to a graft, never a condition of one. A weapon whose block keeps
    // no label array, on either side, simply carries none, which is what every graft did before
    // labels travelled. Six catalogued sources are in that position, and a host can be too, so
    // reading either must not fail the build.
    let Some(mut host_rows) = optional_label_rows(&content.owner, host_block)? else {
        return Ok(None);
    };
    // A scout rifle wearing a pulse rifle's look is still a scout rifle to every perk.
    let mut retyped = false;
    if let Some(kind_block) = kind_block
        && let Some(kind) = optional_label_rows(&content.owner, kind_block)?
            .and_then(|rows| rows.into_iter().next())
        && let Some(first) = host_rows.first_mut()
        && *first != kind
    {
        *first = kind;
        retyped = true;
    }
    let mut known = host_rows
        .iter()
        .map(|row| label_of(row))
        .collect::<BTreeSet<_>>();
    let mut extra = Vec::new();
    for (source_owner, source_block) in sources {
        let Some(rows) = optional_label_rows(source_owner, *source_block)? else {
            continue;
        };
        for row in rows.into_iter().skip(1) {
            if known.insert(label_of(&row)) {
                extra.push(row);
            }
        }
    }
    if extra.is_empty() && !retyped {
        return Ok(None);
    }
    let rows = host_rows.len() + extra.len();
    let mut bytes = ARRAY_HEADER_CLASS.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(rows as u64).to_le_bytes());
    bytes.extend_from_slice(&LABEL_ARRAY_CLASS.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    for row in host_rows.iter().chain(&extra) {
        bytes.extend_from_slice(row);
    }
    let triple = first_triple(&content.owner, host_block)?;
    Ok(Some(crate::item::WeaponRuntimeResourceAppend {
        binding_hash: BINDING,
        resource_index: 0,
        bytes,
        slots: vec![(
            slot_offset(triple, content.resource)?,
            4,
            i64::try_from(rows).map_err(|_| invalid("Weapon label array count overflow"))?,
        )],
        arrays: Vec::new(),
    }))
}

/// One weapon's state array and behavior record, copied out of its owner.
///
/// Every reference inside is a class word, an inline value or an absolute tag, so the run is
/// position independent and can be appended to another owner. The run ends at the next array
/// marker, which is how its length is known without a schema for the row type.
pub(crate) struct Record {
    pub(crate) bytes: Vec<u8>,
    pub(crate) state_offset: usize,
    pub(crate) behavior_offset: Option<usize>,
}

pub(super) fn extract_record(
    owner: &[u8],
    group: u32,
    include_behavior: bool,
) -> AuthoringResult<Record> {
    let block =
        block_in_owner(owner, group).ok_or_else(|| invalid("Behavior source block is missing"))?;
    let triple = first_triple(owner, block)?;
    let state = slot_target(owner, triple + SLOT_STRIDE)
        .ok_or_else(|| invalid("Behavior source has no state array"))?;
    let behavior = include_behavior
        .then(|| {
            slot_target(owner, triple + SLOT_STRIDE * 2)
                .ok_or_else(|| invalid("Behavior source has no behavior record"))
        })
        .transpose()?;
    if let Some(behavior) = behavior
        && u32_at(owner, behavior + 8)? != BEHAVIOR_ARRAY_CLASS
    {
        return Err(invalid("Behavior source record has an unexpected class"));
    }
    let start = state
        .checked_sub(4)
        .ok_or_else(|| invalid("Behavior source array marker is missing"))?;
    let last = behavior.unwrap_or(state);
    let rows = last
        .checked_add(16)
        .ok_or_else(|| invalid("Behavior source record overflows"))?;
    let end = (rows..owner.len().saturating_sub(4))
        .step_by(4)
        .find(|at| u32_at(owner, *at).is_ok_and(|word| word == ARRAY_HEADER_CLASS))
        .ok_or_else(|| invalid("Behavior source record has no end marker"))?;
    if end <= last || start >= state {
        return Err(invalid("Behavior source record has an unexpected layout"));
    }
    Ok(Record {
        bytes: owner[start..end].to_vec(),
        state_offset: state - start,
        behavior_offset: behavior.map(|behavior| behavior - start),
    })
}

/// The resolved content component of one weapon entity.
pub(crate) struct Content {
    pub(crate) owner_tag: u32,
    pub(crate) owner: Vec<u8>,
    /// Offset of the resource inside the owner payload; patch offsets are relative to it.
    pub(crate) resource: usize,
    /// Every variant block in the owner, in payload order.
    pub(crate) blocks: Vec<usize>,
}

/// Resolves the shared content owner and its variant blocks for one weapon entity.
pub(crate) fn content(manager: &PackageManager, entity: &[u8]) -> AuthoringResult<Content> {
    let bindings = weapon_component_bindings(entity, BINDING).map_err(invalid)?;
    let [binding] = bindings.as_slice() else {
        return Err(invalid(
            "Behavior grafting requires one weapon-content component",
        ));
    };
    let owner = manager
        .read_tag(TagHash(binding.owner_tag))
        .map_err(|error| invalid(error.to_string()))?;
    let resource = usize::try_from(binding.resource_offset)
        .map_err(|_| invalid("Weapon-content resource offset overflow"))?;
    let definition = usize::try_from(u64_at(&owner, resource + 8)?)
        .map_err(|_| invalid("Weapon-content definition offset overflow"))?;
    let blocks = crate::weapon::ammo::property_offsets(&owner, definition)?;
    Ok(Content {
        owner_tag: binding.owner_tag,
        owner,
        resource,
        blocks,
    })
}

/// The variant block a content group selects, falling back to the first block.
pub(crate) fn block_for_group(content: &Content, group: u32) -> AuthoringResult<usize> {
    let first = *content
        .blocks
        .first()
        .ok_or_else(|| invalid("Weapon content owner has no variant block"))?;
    for &block in &content.blocks {
        if u32_at(&content.owner, block + 0x10)? == group {
            return Ok(block);
        }
    }
    Ok(first)
}

/// Follows one slot's relative pointer and returns the class of the array it reaches.
fn slot_class(owner: &[u8], slot: usize) -> Option<u32> {
    let target = slot_target(owner, slot)?;
    if u32_at(owner, target.checked_sub(4)?).ok()? != ARRAY_HEADER_CLASS {
        return None;
    }
    u32_at(owner, target + 8).ok()
}

/// Absolute offset a slot's self-relative pointer reaches.
pub(super) fn slot_target(owner: &[u8], slot: usize) -> Option<usize> {
    let relative = i64::from_le_bytes(owner.get(slot..slot + 8)?.try_into().ok()?);
    if relative == 0 {
        return None;
    }
    slot.checked_add_signed(isize::try_from(relative).ok()?)
}

/// Offset of the label slot of the first label, state and behavior triple in one block.
///
/// Block layout is not fixed. A block is a run of sub-blocks and larger weapons repeat the triple,
/// so the slots are found by following pointers rather than by a hard-coded offset.
pub(crate) fn first_triple(owner: &[u8], block: usize) -> AuthoringResult<usize> {
    find_triple(owner, block)?
        .ok_or_else(|| invalid("Weapon variant block has no behavior slots to graft onto"))
}

pub(super) fn find_triple(owner: &[u8], block: usize) -> AuthoringResult<Option<usize>> {
    let size = usize::try_from(u32_at(owner, block + 8)?)
        .map_err(|_| invalid("Weapon variant block size overflow"))?;
    let end = block
        .checked_add(size)
        .filter(|end| *end <= owner.len())
        .ok_or_else(|| invalid("Weapon variant block is truncated"))?;
    let mut slot = block + 0x20;
    while slot + SLOT_STRIDE * 3 <= end {
        if slot_class(owner, slot) == Some(LABEL_ARRAY_CLASS)
            && slot_class(owner, slot + SLOT_STRIDE) == Some(STATE_ARRAY_CLASS)
        {
            return Ok(Some(slot));
        }
        slot += 8;
    }
    Ok(None)
}

/// The source records a behavior graft copies.
pub(crate) struct Graft {
    pub(crate) state_target: usize,
    pub(crate) behavior_target: Option<usize>,
}

/// Locates the state and behavior records the source weapon's block points at.
pub(crate) fn resolve(
    content: &Content,
    source_group: u32,
    include_behavior: bool,
) -> AuthoringResult<Graft> {
    let block = block_for_group(content, source_group)?;
    if u32_at(&content.owner, block + 0x10)? != source_group {
        return Err(invalid(
            "The behavior source is not present in this weapon's content owner",
        ));
    }
    let triple = first_triple(&content.owner, block)?;
    let state_target = slot_target(&content.owner, triple + SLOT_STRIDE)
        .ok_or_else(|| invalid("Behavior source has no state array"))?;
    let behavior_target = include_behavior
        .then(|| {
            slot_target(&content.owner, triple + SLOT_STRIDE * 2)
                .ok_or_else(|| invalid("Behavior source has no behavior record"))
        })
        .transpose()?;
    if let Some(behavior_target) = behavior_target
        && u32_at(&content.owner, behavior_target + 8)? != BEHAVIOR_ARRAY_CLASS
    {
        return Err(invalid("Behavior source record has an unexpected class"));
    }
    Ok(Graft {
        state_target,
        behavior_target,
    })
}

/// Turns an owner offset into the resource-relative offset a runtime patch carries.
pub(super) fn slot_offset(slot: usize, resource: usize) -> AuthoringResult<u32> {
    slot.checked_sub(resource)
        .and_then(|offset| u32::try_from(offset).ok())
        .ok_or_else(|| invalid("Behavior graft offset overflow"))
}

/// Eight bytes of self-relative pointer followed by eight bytes of count.
pub(super) fn slot_bytes(target: usize, slot: usize, count: i64) -> AuthoringResult<Vec<u8>> {
    let target = i64::try_from(target).map_err(|_| invalid("Behavior graft pointer overflow"))?;
    let slot = i64::try_from(slot).map_err(|_| invalid("Behavior graft pointer overflow"))?;
    let mut bytes = (target - slot).to_le_bytes().to_vec();
    bytes.extend_from_slice(&count.to_le_bytes());
    Ok(bytes)
}
