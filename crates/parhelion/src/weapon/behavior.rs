//! Grafts stock weapon behavior records and firing graphs onto authored weapons.
//!
//! A weapon's behavior lives in its content variant block (class `0x80803ACE`), selected inside a
//! shared per-family owner by the pattern row's content group hash. Each block holds one or more
//! triples of relative pointers: a label array, a state array, and a behavior array. Pointing a
//! target block's state and behavior slots at another block's records transplants the behavior.
//! Some weapons only carry a distinct state array, while firing graphs are absolute tag references
//! that can be transplanted independently. Full record grafts are confirmed in game across auto
//! rifles, scout rifles, pulse rifles and sniper rifles. State-only transfers still need gameplay
//! validation.
//!
//! What a record actually carries, from player reports on 2026-09-21:
//!
//! - **The record is a gesture, not the whole perk.** Symmetry's behavior on another weapon gave
//!   the special reload, enough to make Hard Light swap to its alternate fire, and no Dynamic
//!   Charge stacks from precision hits. Ten records serve fourteen exotics and several legendaries
//!   share them, which fits a shared gesture primitive rather than an exotic's own perk. Expect a
//!   record to move how a weapon is handled and to leave what the perk counts behind.
//! - **A weapon has one firing graph, so two projectiles cannot coexist.** Thorn's behavior on
//!   Lumina made it poison and cost it the orbs on kill and Noble Rounds. The graph slot at
//!   `+0xF0` holds one tag, so grafting a weapon that fires something of its own replaces whatever
//!   the host fired, including another exotic's rounds.
use std::collections::BTreeSet;

use crate::tag_payload::{read_u32 as u32_at, read_u64 as u64_at};
use crate::{AuthoringResult, error::invalid, item::WeaponRuntimeResourcePatch};
use sundial::package_authoring::PackageManager;
use sundial::package_authoring::entity::weapon_component_bindings;
use sundial::package_authoring::sandbox_perk::program::{
    NativeAssetResourceAppend, NativeAssetResourcePatch,
};
use tiger_pkg::TagHash;

mod catalog;
mod firing;
mod graft;
mod sockets;
#[cfg(test)]
mod tests;

use catalog::*;
pub use catalog::{
    CATALOG, ELEMENT_SWITCH, behavior, catalog_for_owner, catalog_for_type,
    element_switch_for_owner, paired_record_source, requested_graphs, source_switches_element,
    switches_element_for_type,
};
pub use firing::BehaviorFiring;
pub(crate) use firing::*;
pub use graft::DEFAULT_PROJECTILE_SPEED_BOOST;
pub(crate) use graft::*;
pub(crate) use sockets::*;

const BINDING: u32 = 0x5F0D_D954;
const ARRAY_HEADER_CLASS: u32 = 0x8080_9FBD;
const LABEL_ARRAY_CLASS: u32 = 0x8080_94B3;
const STATE_ARRAY_CLASS: u32 = 0x8080_94B0;
const BEHAVIOR_ARRAY_CLASS: u32 = 0x8080_3AD9;
/// The class of a weapon's content variant block.
const VARIANT_BLOCK_CLASS: u32 = 0x8080_3ACE;
/// A triple occupies three consecutive slots of sixteen bytes: label, state, behavior.
const SLOT_STRIDE: usize = 0x10;
/// One row of a block's label array: the label hash, then a fixed twenty bytes shared by every row.
const LABEL_ROW_SIZE: usize = 0x18;
/// The block field naming the weapon's firing and projectile graph.
const GRAPH_OFFSET: usize = 0xF0;
/// The block fields naming the weapon's firing values: two records (class 80803F26) that each
/// name a bank of channel programs (class 80804781) the stat translator reads.
const VALUE_OFFSETS: [usize; 2] = [0x150, 0x160];
const VALUE_RECORD_CLASS: u32 = 0x8080_3F26;
/// The block fields that, with the label array's first row, are the weapon's type markers.
/// +0x30 is the FNV-1 hash of the type's name (`scout_rifle`, `pulse_rifle`, `hand_cannon`).
/// +0x18 is a frame key from the same space as the first-person attachment's animation keys:
/// every pulse rifle names 7B4E9613, and hand cannons name their frame's. They stay the base
/// weapon's under any appearance. Thousand Vows, One Thousand Voices wearing Eriana's Vow, fired
/// single shots without charging, and Laser Lumina, Prometheus Lens wearing Lumina, drew no beam,
/// while their blocks named hand_cannon and their stat tables were their own.
const TYPE_MARKER_OFFSETS: [usize; 2] = [0x18, 0x30];
/// The type marker that names the weapon type.
pub(crate) const TYPE_NAME_OFFSET: usize = 0x30;

/// What a grafted record was observed to do in game.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BehaviorEffect {
    /// Hold Reload cycles the damage type, when a trait socket carries The Fundamentals.
    ElementSwitch,
    /// The record grafts cleanly but produced no observed effect on its own.
    NoObservedEffect,
    /// Not yet taken into a session.
    Untested,
}

/// Where a behavior lives, which decides how far it can be grafted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BehaviorSource {
    /// A record inside the content owner, reached by a relative pointer. Only weapons in the same
    /// owner can point at it.
    Record {
        owner_tag: u32,
        /// Locates the source weapon's block inside that owner.
        content_group: u32,
        /// Weapon types the owner covers, used to offer the record only where it can apply.
        family_types: &'static [&'static str],
    },
    /// A distinct state array with no behavior array. The record is copied into another content
    /// owner when needed, so it can be tried on any weapon family.
    State { owner_tag: u32, content_group: u32 },
    /// The weapon's firing and projectile graph, named by tag at block `+0xF0`. A tag reference is
    /// absolute, so any weapon can point at it.
    Graph { tag: u32 },
    /// A firing graph whose source weapon also carries a distinct state array. Both pieces are
    /// transferred by one Behavior choice.
    GraphState {
        tag: u32,
        owner_tag: u32,
        content_group: u32,
    },
}

impl BehaviorSource {
    /// Content-owner record carried by this source and whether its behavior array travels too.
    const fn record_source(self) -> Option<(u32, u32, bool)> {
        match self {
            Self::Record {
                owner_tag,
                content_group,
                ..
            } => Some((owner_tag, content_group, true)),
            Self::State {
                owner_tag,
                content_group,
            }
            | Self::GraphState {
                owner_tag,
                content_group,
                ..
            } => Some((owner_tag, content_group, false)),
            Self::Graph { .. } => None,
        }
    }
}

/// One graftable behavior, identified by the weapon whose block carries it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Behavior {
    pub id: &'static str,
    pub name: &'static str,
    pub source_name: &'static str,
    pub source_item_hash: u32,
    pub source: BehaviorSource,
    /// The source weapon's own intrinsic plug, which some behaviors need in a socket to work.
    pub intrinsic_plug: Option<u32>,
    /// The source weapon's exotic trait plug, which usually carries the perk half.
    pub trait_plug: Option<u32>,
    pub effect: BehaviorEffect,
    /// A known limit of this source, shown before the weapon is built.
    pub caution: Option<&'static str>,
    pub summary: &'static str,
}

impl Behavior {
    /// Whether this record drives The Fundamentals.
    #[must_use]
    pub const fn switches_element(&self) -> bool {
        matches!(self.effect, BehaviorEffect::ElementSwitch)
    }

    /// The content owner a copied record belongs to, if this behavior carries one.
    #[must_use]
    pub const fn owner_tag(&self) -> Option<u32> {
        match self.source {
            BehaviorSource::Record { owner_tag, .. }
            | BehaviorSource::State { owner_tag, .. }
            | BehaviorSource::GraphState { owner_tag, .. } => Some(owner_tag),
            BehaviorSource::Graph { .. } => None,
        }
    }

    /// The firing graph carried by this behavior, if any.
    #[must_use]
    pub const fn graph_tag(&self) -> Option<u32> {
        match self.source {
            BehaviorSource::Graph { tag } | BehaviorSource::GraphState { tag, .. } => Some(tag),
            BehaviorSource::Record { .. } | BehaviorSource::State { .. } => None,
        }
    }

    /// Whether this behavior includes a firing graph rather than only content-owner records.
    #[must_use]
    pub const fn has_graph(&self) -> bool {
        self.graph_tag().is_some()
    }

    /// Whether this source carries the content owner's behavior array rather than state alone.
    #[must_use]
    pub const fn carries_behavior_record(&self) -> bool {
        matches!(self.source, BehaviorSource::Record { .. })
    }

    /// Whether this source brings projectiles whose launch speed a graft can raise.
    ///
    /// Two things have to hold. Only a graph carries the firing side at all, and only a graph
    /// from a weapon that launches something reads a speed below the hitscan sentinel for the
    /// boost to raise. A graph from a weapon that fires instantly answers no, because raising a
    /// multiplier it does not have writes nothing.
    #[must_use]
    pub fn launches_projectiles(&self) -> bool {
        self.has_graph() && LAUNCHING_SOURCES.contains(&self.id)
    }

    /// Whether this source's own plugs change how many rounds a burst fires, which is what makes
    /// a firing pattern worth choosing. See [`BehaviorFiring`].
    #[must_use]
    pub fn changes_burst(&self) -> bool {
        BURST_SOURCES.contains(&self.id)
    }

    /// Whether a weapon of this type can graft this behavior. Copied states and graphs reach every
    /// weapon family.
    #[must_use]
    pub fn reaches_type(&self, type_name: &str) -> bool {
        match self.source {
            BehaviorSource::Record { family_types, .. } => family_types.contains(&type_name),
            BehaviorSource::State { .. }
            | BehaviorSource::Graph { .. }
            | BehaviorSource::GraphState { .. } => true,
        }
    }
}

/// The content group a weapon's pattern row selects, and the base weapon's own group when that row
/// comes from another weapon, as it does for an appearance whose rig moves across.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ContentGroups {
    pub(crate) selected: Option<u32>,
    pub(crate) own: Option<u32>,
    /// The content group whose block lends its type markers, when another weapon's are chosen
    /// over the base weapon's own.
    pub(crate) kind: Option<u32>,
    /// The first-person animations another weapon lends the selected group's row.
    pub(crate) animations: Option<crate::weapon::animations::Profile>,
    /// The hold of a pinned appearance's own rig, which the kept rig's attachment takes.
    pub(crate) hold: Option<crate::weapon::animations::Hold>,
}

/// What a set of behavior grafts compiles into.
#[derive(Debug, Default)]
pub(crate) struct Grafted {
    pub(crate) patches: Vec<WeaponRuntimeResourcePatch>,
    pub(crate) appends: Vec<crate::item::WeaponRuntimeResourceAppend>,
}

/// Builds the runtime patches that graft each behavior onto the weapon's own variant block.
///
/// When the pattern row selects another weapon's block, the base weapon's own type markers,
/// firing graph, firing values, behavior record and labels go back into it first, so an
/// appearance changes how the weapon looks and not how it fires. A requested behavior still
/// replaces the half it brings, and chosen type markers replace the base's.
pub(crate) fn patches(
    manager: &PackageManager,
    entity: &[u8],
    groups: ContentGroups,
    requested: &[String],
    speed_boost: f32,
) -> AuthoringResult<Grafted> {
    let restores_own = groups.own.is_some() && groups.own != groups.selected;
    // Putting the base's own behavior back is a best effort that never fails a build asking for
    // nothing else. A behavior or type markers the recipe asks for are not.
    let asked = !requested.is_empty() || groups.kind.is_some();
    if !asked && !restores_own {
        return Ok(Grafted::default());
    }
    let content = match content(manager, entity) {
        Ok(content) => content,
        Err(_) if !asked => return Ok(Grafted::default()),
        Err(error) => return Err(error),
    };
    let behaviors = requested
        .iter()
        .map(|id| resolve_request(id, content.owner_tag))
        .collect::<AuthoringResult<Vec<_>>>()?;
    let mut appends = Vec::new();
    let block = match groups.selected {
        Some(group) => block_for_group(&content, group),
        None => content
            .blocks
            .first()
            .copied()
            .ok_or_else(|| invalid("Weapon content owner has no variant block")),
    };
    let block = match block {
        Ok(block) => block,
        Err(_) if !asked => return Ok(Grafted::default()),
        Err(error) => return Err(error),
    };
    let kind = groups
        .kind
        .map(|group| {
            exact_block(&content, group).ok_or_else(|| {
                invalid(
                    "The weapon chosen for Type Markers keeps its runtime in another content owner. Choose another weapon.",
                )
            })
        })
        .transpose()?;
    let own = groups
        .own
        .filter(|_| restores_own)
        .and_then(|group| exact_block(&content, group))
        .filter(|&own| own != block);
    let mut graph_requested = false;
    let mut patches = Vec::with_capacity(behaviors.len() * 3);
    // One block holds one behavior record, so the same record must not be written twice. Two
    // requests resolve to it whenever a weapon's graph is chosen and element switching is on,
    // because the graph pairs with the very record the element-switch request finds. Writing it
    // twice put two edits over the same bytes and the overlap guard failed the whole build.
    let mut applied_records = Vec::new();
    let mut label_sources = Vec::new();
    for entry in behaviors {
        // Every source weapon's own labels come along, whichever half of it is borrowed, because
        // its perk may key on them wherever the behavior itself lives. A source whose own block
        // cannot be reached, because it has no sandbox-pattern runtime row of its own, carries no
        // labels rather than failing the graft: that is what these sources did before labels
        // travelled at all, and Arbalest, Legend of Acrius, Traveler's Chosen and Warden's Law
        // are all in that position.
        if let Some(entry) = entry
            && let Ok(block) = source_block(manager, entry)
        {
            label_sources.push(block);
        }
        let record_source = entry
            .and_then(|entry| entry.source.record_source())
            // A graph choice carries its weapon's behavior record too, so one pick brings both
            // halves. Without this the firing side transferred and the behavior never ran.
            .or_else(|| {
                entry
                    .and_then(paired_record)
                    .and_then(|paired| paired.source.record_source())
            })
            .or_else(|| {
                entry
                    .is_none()
                    .then(element_switch_source)
                    .flatten()
                    .map(|(owner_tag, content_group)| (owner_tag, content_group, true))
            });
        if let Some((owner_tag, group, include_behavior)) = record_source {
            if applied_records.contains(&(owner_tag, group)) {
                continue;
            }
            if !applied_records.is_empty() {
                // One block holds one behavior record, so a second distinct one has nowhere to
                // go. Without this the two writes landed on the same bytes and the build failed
                // with an internal overlap message that named neither behavior.
                return Err(invalid(
                    "A weapon can carry one borrowed behavior record. Choose a single behavior that brings one, or pick a firing graph instead.",
                ));
            }
            applied_records.push((owner_tag, group));
            let triple = first_triple(&content.owner, block)?;
            let source_owner = (owner_tag != content.owner_tag)
                .then(|| {
                    manager
                        .read_tag(TagHash(owner_tag))
                        .map_err(|error| invalid(error.to_string()))
                })
                .transpose()?;
            if let Some(source) = &source_owner {
                let record = extract_record(source, group, include_behavior)?;
                let mut slots = vec![(
                    slot_offset(triple + SLOT_STRIDE, content.resource)?,
                    record.state_offset,
                    1,
                )];
                if let Some(behavior_offset) = record.behavior_offset {
                    slots.push((
                        slot_offset(triple + SLOT_STRIDE * 2, content.resource)?,
                        behavior_offset,
                        0,
                    ));
                }
                appends.push(crate::item::WeaponRuntimeResourceAppend {
                    binding_hash: BINDING,
                    resource_index: 0,
                    bytes: record.bytes,
                    slots,
                    arrays: Vec::new(),
                });
            } else {
                let graft = resolve(&content, group, include_behavior)?;
                let mut targets = vec![(triple + SLOT_STRIDE, graft.state_target, 1)];
                if let Some(behavior_target) = graft.behavior_target {
                    targets.push((triple + SLOT_STRIDE * 2, behavior_target, 0));
                }
                for (slot, target, count) in targets {
                    patches.push(WeaponRuntimeResourcePatch {
                        binding_hash: BINDING,
                        resource_index: 0,
                        offset: slot_offset(slot, content.resource)?,
                        bytes: slot_bytes(target, slot, count)?,
                        graph_values: Vec::new(),
                        graph_removals: Vec::new(),
                        graph_trajectories: None,
                    });
                }
            }
        } else if entry.is_none() {
            return Err(invalid("No element-switch record is available to graft."));
        }

        if let Some(tag) = entry.and_then(Behavior::graph_tag) {
            graph_requested = true;
            patches.push(WeaponRuntimeResourcePatch {
                binding_hash: BINDING,
                resource_index: 0,
                offset: slot_offset(block + GRAPH_OFFSET, content.resource)?,
                bytes: tag.to_le_bytes().to_vec(),
                graph_values: graph_values(manager, host_graph(&content, block), tag, speed_boost)?,
                graph_removals: Vec::new(),
                graph_trajectories: trajectory_need(manager, entity, tag)?,
            });
        }
    }
    // Putting the base's own behavior back is a best effort, so a block it cannot read keeps
    // what the requested behaviors bring, as it did before.
    let mut own_labels = None;
    if let Some(own) = own
        && let Ok(restored) = own_patches(
            manager,
            &content,
            block,
            own,
            applied_records.is_empty(),
            !graph_requested,
        )
    {
        patches.extend(restored);
        // Its perks key on its labels wherever its behavior runs, as a borrowed source's do.
        label_sources.push((content.owner.clone(), own));
        own_labels = Some(own);
    }
    // Chosen type markers win over the base's own, which the block otherwise gets whatever the
    // appearance. Firing stays the base's through its graph, values and records above.
    let own_markers = own_labels;
    match (kind, own_markers) {
        (Some(source), _) => patches.extend(type_marker_patches(&content, block, source)?),
        (None, Some(own)) => {
            patches.extend(type_marker_patches(&content, block, own).unwrap_or_default());
        }
        (None, None) => {}
    }
    let kind_block = kind.or(own_markers);
    // The base's labels still go on, so its perks key on them, whichever row 0 the block keeps.
    if !requested.is_empty() || kind_block.is_some() || own_labels.is_some() {
        let labels =
            label_append(&content, block, kind_block, &label_sources).or_else(|error| {
                if own_labels.is_some() && kind.is_none() {
                    label_sources.pop();
                    label_append(&content, block, None, &label_sources)
                } else {
                    Err(error)
                }
            })?;
        if let Some(append) = labels {
            appends.push(append);
        }
    }
    Ok(Grafted { patches, appends })
}

/// Writes `source`'s type marker keys into the selected block. The label array's first row, the
/// third marker, travels with the labels.
fn type_marker_patches(
    content: &Content,
    block: usize,
    source: usize,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let mut patches = Vec::new();
    for offset in TYPE_MARKER_OFFSETS {
        let key = u32_at(&content.owner, source + offset)?;
        if u32_at(&content.owner, block + offset)? == key {
            continue;
        }
        patches.push(WeaponRuntimeResourcePatch {
            binding_hash: BINDING,
            resource_index: 0,
            offset: slot_offset(block + offset, content.resource)?,
            bytes: key.to_le_bytes().to_vec(),
            graph_values: Vec::new(),
            graph_removals: Vec::new(),
            graph_trajectories: None,
        });
    }
    Ok(patches)
}

/// A weapon's own type markers' name key, read from its block in its content owner.
pub(crate) fn type_name_key(content: &Content, group: u32) -> Option<u32> {
    exact_block(content, group)
        .and_then(|block| u32_at(&content.owner, block + TYPE_NAME_OFFSET).ok())
}

/// The type markers `group`'s own block carries in `entity`'s content owner: its frame key and
/// type name, in `TYPE_MARKER_OFFSETS` order.
#[cfg(test)]
pub(crate) fn markers(
    manager: &PackageManager,
    entity: &[u8],
    group: u32,
) -> AuthoringResult<[u32; 2]> {
    let content = content(manager, entity)?;
    let block = exact_block(&content, group)
        .ok_or_else(|| invalid("The appearance's own block is not in its content owner"))?;
    Ok([
        u32_at(&content.owner, block + TYPE_MARKER_OFFSETS[0])?,
        u32_at(&content.owner, block + TYPE_MARKER_OFFSETS[1])?,
    ])
}

/// The type markers of the block `group` resolves to in `entity`'s content owner, falling back
/// to the first block as the game does.
#[cfg(test)]
pub(crate) fn resolved_markers(
    manager: &PackageManager,
    entity: &[u8],
    group: u32,
) -> AuthoringResult<[u32; 2]> {
    let content = content(manager, entity)?;
    let block = block_for_group(&content, group)?;
    Ok([
        u32_at(&content.owner, block + TYPE_MARKER_OFFSETS[0])?,
        u32_at(&content.owner, block + TYPE_MARKER_OFFSETS[1])?,
    ])
}

/// The frame key `group`'s own block carries beside its type name, the first type marker.
pub(crate) fn frame_key(content: &Content, group: u32) -> Option<u32> {
    exact_block(content, group)
        .and_then(|block| u32_at(&content.owner, block + TYPE_MARKER_OFFSETS[0]).ok())
}

/// A projectile the weapon fires as its own: a stock projectile graph cloned privately with
/// checked owner patches and definition appends. Every variant block names the clone as its
/// firing graph, so the weapon keeps its own pattern instead of a perk overriding it.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FiredGraph {
    pub source_graph: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patches: Vec<NativeAssetResourcePatch>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub appends: Vec<NativeAssetResourceAppend>,
    /// Particle events whose stock systems become the weapon's imported particle systems.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub particles: Vec<FiredParticle>,
}

/// One particle event's system in a fired graph's owner, named by the stock tag it holds and
/// replaced with an imported particle system.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FiredParticle {
    pub binding_hash: u32,
    pub resource_index: u16,
    pub offset: u32,
    pub expected: u32,
    /// The imported particle system's node symbol.
    pub system: String,
}

/// Names `graph` as the firing graph of every variant block in the weapon's content owner,
/// the ones a perk selects included, as the ammunition edits do. The graph carries its own
/// launch speed, so no boost applies.
pub(crate) fn fired_graph_patches(
    manager: &PackageManager,
    entity: &[u8],
    graph: u32,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let content = content(manager, entity)?;
    content
        .blocks
        .iter()
        .map(|&block| {
            Ok(WeaponRuntimeResourcePatch {
                binding_hash: BINDING,
                resource_index: 0,
                offset: slot_offset(block + GRAPH_OFFSET, content.resource)?,
                bytes: graph.to_le_bytes().to_vec(),
                graph_values: Vec::new(),
                graph_removals: Vec::new(),
                graph_trajectories: None,
            })
        })
        .collect()
}

/// The block naming exactly this content group, with no fallback to the first block.
fn exact_block(content: &Content, group: u32) -> Option<usize> {
    content
        .blocks
        .iter()
        .copied()
        .find(|&block| u32_at(&content.owner, block + 0x10).is_ok_and(|found| found == group))
}

/// Points the selected block at the base weapon's own state array and behavior record, and names
/// its own firing graph, whichever of those no requested behavior already brings, and its own
/// firing values in every case. Its type markers go back separately, unless others are chosen.
///
/// Both blocks sit in one owner, so each slot keeps the base's target and count as they are. The
/// base's graph needs no speed change, since it is the one its own frame fires, and it loads
/// with the owner that already references it. A half the base's block lacks is left alone.
fn own_patches(
    manager: &PackageManager,
    content: &Content,
    block: usize,
    own: usize,
    records: bool,
    graph: bool,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let mut patches = Vec::new();
    if records
        && let (Some(target), Some(source)) = (
            find_triple(&content.owner, block)?,
            find_triple(&content.owner, own)?,
        )
    {
        for slot in [SLOT_STRIDE, SLOT_STRIDE * 2] {
            let Some(record) = slot_target(&content.owner, source + slot) else {
                continue;
            };
            let count = i64::try_from(u64_at(&content.owner, source + slot + 8)?)
                .map_err(|_| invalid("Behavior record count overflow"))?;
            let bytes = slot_bytes(record, target + slot, count)?;
            if content.owner.get(target + slot..target + slot + 16) == Some(bytes.as_slice()) {
                continue;
            }
            patches.push(WeaponRuntimeResourcePatch {
                binding_hash: BINDING,
                resource_index: 0,
                offset: slot_offset(target + slot, content.resource)?,
                bytes,
                graph_values: Vec::new(),
                graph_removals: Vec::new(),
                graph_trajectories: None,
            });
        }
    }
    if graph
        && let Some(tag) = host_graph(content, own)
        && host_graph(content, block) != Some(tag)
    {
        patches.push(WeaponRuntimeResourcePatch {
            binding_hash: BINDING,
            resource_index: 0,
            offset: slot_offset(block + GRAPH_OFFSET, content.resource)?,
            bytes: tag.to_le_bytes().to_vec(),
            graph_values: Vec::new(),
            graph_removals: Vec::new(),
            graph_trajectories: None,
        });
    }
    // The values decide how the weapon fires whatever graph it fires, so they go back even when
    // a requested behavior brings its own graph.
    for offset in VALUE_OFFSETS {
        let tag = u32_at(&content.owner, own + offset)?;
        if u32_at(&content.owner, block + offset)? == tag
            || manager
                .get_entry(TagHash(tag))
                .is_none_or(|entry| entry.reference != VALUE_RECORD_CLASS)
        {
            continue;
        }
        patches.push(WeaponRuntimeResourcePatch {
            binding_hash: BINDING,
            resource_index: 0,
            offset: slot_offset(block + offset, content.resource)?,
            bytes: tag.to_le_bytes().to_vec(),
            graph_values: Vec::new(),
            graph_removals: Vec::new(),
            graph_trajectories: None,
        });
    }
    Ok(patches)
}
