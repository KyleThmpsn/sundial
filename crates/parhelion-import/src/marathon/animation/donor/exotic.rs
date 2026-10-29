//! An exotic donor's runtime without its own behavior.
//!
//! Animation fit chooses an exotic only when no ordinary donor serves: a type without one, or with
//! source animation the one shotgun whose first-person states request magazine reloads, since the
//! exotic's runtime keeps its own effects. The import keeps that weapon's runtime, so its reloads and first-person
//! animation, and takes the rest from the earliest ordinary candidate of its type: the plugs of its
//! intrinsic and trait sockets, its tier and, through the recipe's behavior donor, its content
//! block's behavior. An ordinary weapon in the exotic's own weapon-content owner lends that
//! block's firing graph, firing values, records, labels and type markers. One in another owner
//! lends its records and labels, and its shot travels as the recipe's fired graph and Barrel
//! edits: the graph it fires and its Barrel's pellets, pattern and random rotation. The exotic's
//! own firing graph, perks, state record and Exotic tier stay behind.
use super::*;
use sundial::package_authoring::entity::{
    WEAPON_BARREL_COMPONENT_KEY, WeaponComponentBinding, barrel, barrel_pellets, spread,
    weapon_component_bindings,
};

/// The native rarity byte that marks an exotic.
const EXOTIC: u8 = 5;
/// A runtime entity's weapon-content binding, whose owner holds every variant block.
const CONTENT: u32 = 0x5F0D_D954;
/// Socket types of a weapon's intrinsic frame and of its trait columns.
const INTRINSIC: u64 = 176;
const TRAIT: u64 = 92;
/// Recipe tier names by native rarity byte, from 1.
const TIERS: [&str; 5] = ["common", "uncommon", "rare", "legendary", "exotic"];

fn rarity(native: &mut Reader, hash: u32) -> Result<u8> {
    let item = crate::tiger::shadowkeep::item_tag(native, hash)?;
    native
        .tag(item, Some(0x80807BEA))?
        .u8(sundial::package_authoring::investment_schema::ITEM_RARITY_OFFSET)
}

/// Whether a native weapon is exotic. One whose rarity does not read counts as ordinary, and
/// animation fit rejects it on its own terms.
pub(super) fn exotic(native: &mut Reader, hash: u32) -> bool {
    rarity(native, hash).is_ok_and(|tier| tier == EXOTIC)
}

/// The one binding of a component in a runtime entity.
fn binding(entity: &Payload, key: u32) -> Result<WeaponComponentBinding> {
    let bindings = weapon_component_bindings(&entity.0, key).map_err(anyhow::Error::msg)?;
    let [binding] = bindings.as_slice() else {
        anyhow::bail!("The weapon does not have one component {key:08X}");
    };
    Ok(*binding)
}

/// The variant block a content group selects in a weapon-content owner, if it has one of its
/// own. The owner's definition embeds the default block at `+80` and counts its variant array at
/// `+240`, with a relative pointer at `+248` to a 16-byte header and `0x1C0`-byte rows. A block
/// names its content group at `+10`.
fn block(owner: &Payload, resource: usize, group: u32) -> Result<Option<usize>> {
    let definition = usize::try_from(owner.u64(resource + 8)?)?;
    let mut blocks = vec![definition + 0x80];
    let count = usize::try_from(owner.u64(definition + 0x240)?)?;
    if count != 0 {
        let header = owner.pointer(definition + 0x248)?;
        blocks.extend((0..count).map(|row| header + 16 + row * 0x1C0));
    }
    Ok(blocks
        .into_iter()
        .find(|&at| owner.u32(at + 0x10).ok() == Some(group)))
}

/// How a native weapon fires: its weapon-content owner, the graph its block names, its Barrel's
/// graph, and the Barrel's pellets, pattern and random rotation.
struct Firing {
    owner: u32,
    block_graph: Option<u32>,
    barrel_graph: Option<u32>,
    pellets: u16,
    pattern: Option<spread::Pattern>,
    rotation: bool,
}

impl Firing {
    fn read(native: &mut Reader, hash: u32) -> Result<Self> {
        let item = crate::tiger::shadowkeep::item_tag(native, hash)?;
        let report = crate::tiger::rig::inspect(native, item, false)?;
        let group = native::hex(&report["content_key"])?;
        let entity = native.tag(native::hex(&report["runtime_entity"])?, Some(0x80809C0F))?;
        let content = binding(&entity, CONTENT)?;
        let owner = native.tag(content.owner_tag, None)?;
        let block_graph = block(&owner, usize::try_from(content.resource_offset)?, group)?
            .map(|at| owner.u32(at + 0xF0))
            .transpose()?
            .filter(|graph| ![0, u32::MAX].contains(graph));
        let barrel_binding = binding(&entity, WEAPON_BARREL_COMPONENT_KEY)?;
        let barrel_owner = native.tag(barrel_binding.owner_tag, None)?;
        let slots = barrel::projectile_slots(&barrel_owner.0, barrel_binding)
            .map_err(anyhow::Error::msg)?;
        Ok(Self {
            owner: content.owner_tag,
            block_graph,
            barrel_graph: slots.first().and_then(|slot| slot.1),
            pellets: barrel_pellets(&barrel_owner.0, barrel_binding).map_err(anyhow::Error::msg)?,
            pattern: spread::read_pattern(&barrel_owner.0, barrel_binding)
                .map_err(anyhow::Error::msg)?,
            rotation: spread::read_random_rotation(&barrel_owner.0, barrel_binding)
                .map_err(anyhow::Error::msg)?,
        })
    }

    /// The graph the weapon fires: its block's, else its Barrel's.
    fn graph(&self) -> Option<u32> {
        self.block_graph.or(self.barrel_graph)
    }

    /// The recipe's fired graph and Barrel edits that give a weapon firing as `self` fires the
    /// way `plain` does, where they differ.
    fn shot(&self, plain: &Self) -> Value {
        let mut shot = json!({});
        if let Some(graph) = plain.graph().filter(|graph| Some(*graph) != self.graph()) {
            shot["fired_graph"] = json!({"source_graph":graph});
        }
        if plain.pellets != self.pellets || plain.pattern != self.pattern {
            shot["barrel"] = match &plain.pattern {
                Some(pattern) => json!({"pellets":plain.pellets,"rings":pattern.rings,
                    "random_rotation":plain.rotation}),
                None => json!({"pellets":plain.pellets}),
            };
        }
        shot
    }
}

/// Each socket of type `kind`, as its lane and its plugs with the default first.
fn lanes(sockets: &Value, kind: u64) -> Result<Vec<(usize, Vec<String>)>> {
    let mut lanes = Vec::new();
    for socket in sockets["sockets"].as_array().context("Native sockets")? {
        if socket["socket_type"].as_u64() != Some(kind) {
            continue;
        }
        let mut plugs = socket["choices"]
            .as_array()
            .context("Native socket choices")?
            .iter()
            .collect::<Vec<_>>();
        plugs.sort_by_key(|plug| plug["default"] != true);
        let plugs = plugs
            .iter()
            .map(|plug| {
                plug["hash"]
                    .as_u64()
                    .map(|hash| format!("0x{hash:08X}"))
                    .context("Native plug hash")
            })
            .collect::<Result<Vec<_>>>()?;
        let lane = socket["index"].as_u64().context("Native socket index")?;
        lanes.push((usize::try_from(lane)?, plugs));
    }
    Ok(lanes)
}

/// Socket columns that give `base`'s intrinsic socket and its trait sockets, in order, the plugs
/// of `plain`'s, with every other socket inherited and given its native default where it needs
/// one. Returns the columns and the base sockets `plain` has no counterpart for, which keep their
/// own plugs.
fn columns(native: &mut Reader, base: u32, plain: u32) -> Result<(Value, Vec<usize>)> {
    let base_sockets = crate::tiger::shadowkeep::sockets(native, base)?;
    let plain_sockets = crate::tiger::shadowkeep::sockets(native, plain)?;
    let count = base_sockets["sockets"]
        .as_array()
        .context("Native sockets")?
        .len();
    let mut columns = vec![Value::Null; count];
    let mut kept = Vec::new();
    for kind in [INTRINSIC, TRAIT] {
        let theirs = lanes(&plain_sockets, kind)?;
        for (at, (lane, _)) in lanes(&base_sockets, kind)?.into_iter().enumerate() {
            match theirs.get(at) {
                Some((_, plugs)) if !plugs.is_empty() => {
                    *columns.get_mut(lane).context("Native socket lane")? =
                        json!({"choices":plugs});
                }
                _ => kept.push(lane),
            }
        }
    }
    let mut columns = json!(columns);
    crate::tiger::shadowkeep::fill_native_defaults(&base_sockets, &mut columns, &mut Vec::new())?;
    Ok((columns, kept))
}

/// The recipe overrides that keep the chosen donor's runtime and leave its own behavior when it
/// is an exotic, recorded in `report`. The ordinary weapon is the earliest candidate in the
/// exotic's weapon-content owner, else the earliest ordinary candidate, whose shot then travels as
/// a fired graph and Barrel edits. Empty for an ordinary donor or when no ordinary candidate
/// reads.
pub(in crate::marathon) fn plain(
    native: &mut Reader,
    donors: &[(u32, String)],
    chosen: usize,
    report: &mut Value,
) -> Result<Value> {
    let (base, name) = (donors[chosen].0, donors[chosen].1.as_str());
    if rarity(native, base)? != EXOTIC {
        return Ok(json!({}));
    }
    let exotic = Firing::read(native, base)?;
    let mut skipped = Vec::new();
    let mut found: Option<(u32, &str, Firing)> = None;
    for (index, (hash, candidate)) in donors.iter().enumerate() {
        if index == chosen {
            continue;
        }
        let ordinary = rarity(native, *hash).and_then(|tier| {
            (tier < EXOTIC)
                .then(|| Firing::read(native, *hash))
                .transpose()
        });
        match ordinary {
            Ok(Some(firing)) => {
                let shared = firing.owner == exotic.owner;
                if shared || found.is_none() {
                    found = Some((*hash, candidate.as_str(), firing));
                }
                if shared {
                    break;
                }
            }
            Ok(None) => {}
            Err(error) => skipped.push(
                json!({"donor":format!("{hash:08X}"),"name":candidate,"reason":format!("{error:#}")}),
            ),
        }
    }
    let Some((plain, plain_name, firing)) = found else {
        report["exotic_behavior"] = json!({"kept":true,"skipped":skipped,
            "reason":format!("No ordinary weapon of this type reads, so {name}'s own behavior stays.")});
        return Ok(json!({}));
    };
    let tier = *TIERS
        .get(usize::from(rarity(native, plain)?).wrapping_sub(1))
        .context("Native rarity")?;
    let (columns, kept) = columns(native, base, plain)?;
    let shared = firing.owner == exotic.owner;
    let mut overrides = json!({"behavior_donor":{"item_hash":format!("0x{plain:08X}"),"expected_name":plain_name},
        "rarity":tier,"socket_columns":columns});
    // Another owner's block cannot lend its graph or values, so the shot travels on its own.
    let shot = if shared {
        json!({})
    } else {
        exotic.shot(&firing)
    };
    for (key, value) in shot.as_object().into_iter().flatten() {
        overrides[key] = value.clone();
    }
    report["exotic_behavior"] = json!({"kept":false,"behavior_donor":format!("{plain:08X}"),
        "name":plain_name,"rarity":tier,"shared_content_owner":shared,"shot":shot,
        "kept_sockets":kept,"skipped":skipped,
        "reason":format!("{name} is exotic, so the import keeps its runtime and takes {plain_name}'s firing, perks and tier.")});
    Ok(overrides)
}
