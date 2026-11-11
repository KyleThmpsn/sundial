//! The base weapon's firing presentation, which a source presentation is written over.
//!
//! A weapon's content row names a presentation parent whose root holds up to six slot entities.
//! Each slot entity runs one effect flow (`808084D7` instance, `808084E9` definition) and reads
//! its scalar inputs from the slot's channel bank (`80809790`) through the entity's connection
//! rows. A source flow written over a slot's flow keeps that entity, its bank and its
//! allocation format. Its inputs bind by name to the bank's channels, and a name the bank does
//! not declare is added to a private copy of the bank when the source receives it the same way.
use crate::tiger::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Stock effect flows whose events supply the native delay (`808093CB`) envelope, as for the Reach
/// firing route's rebuilt flows.
pub(crate) const DELAY_ENVELOPES: [u32; 3] = [0x815b_901a, 0x80bc_308d, 0x80fc_7dfb];

/// The `80809BC9` connection rows of a native entity, and the class of an effect flow definition.
const CONNECTION: u32 = 0x8080_9bc9;
const FLOW: u32 = 0x8080_84e9;

fn hex(value: &Value, what: &str) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().with_context(|| format!("Native {what}"))?,
        16,
    )?)
}

pub(crate) fn unset(tag: u32) -> bool {
    matches!(tag, 0 | u32::MAX)
}

/// The base's flow owner in a slot entity of its own presentation.
fn native_flow(native: &mut Reader, entity: u32) -> Result<u32> {
    let e = native.tag(entity, Some(0x8080_9c0f))?;
    let mut flows = Vec::new();
    for at in e.array(16, 12, None)? {
        let owner = e.u32(at)?;
        if unset(owner) {
            continue;
        }
        let o = native.tag(owner, Some(0x8080_9c36))?;
        if o.u32(o.pointer(24)? - 4)? == FLOW {
            flows.push(owner);
        }
    }
    let [flow] = flows.as_slice() else {
        bail!("Native presentation slot entity {entity:08X} has no single effect flow");
    };
    Ok(*flow)
}

/// The finished flow's scalar input definition rows with their names, main inputs then auxiliary.
/// The writer seals the flow, and a sealed owner's `+18` pointer reaches the live copy of its
/// definition, whose input rows sit elsewhere than the offsets the writer recorded beforehand. A
/// connection must name the live row, as the Reach route's rebuilt flows do, or the input stays
/// unbound. Rows are 40 bytes at definition `+188` and `+2B8`, with the name at `+20`. `written`
/// is the input names the writer emitted, in order.
pub(crate) fn live_inputs(owner: &Payload, written: &[u32]) -> Result<Vec<(u32, usize)>> {
    let definition = owner.pointer(24)?;
    let mut rows = owner.array(definition + 0x188, 40, None)?;
    rows.extend(owner.array(definition + 0x2B8, 40, None)?);
    let live = rows
        .into_iter()
        .map(|row| Ok((owner.u32(row + 0x20)?, row)))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        live.iter().map(|(name, _)| name).eq(written.iter()),
        "Translated firing flow inputs differ from the writer's"
    );
    Ok(live)
}

/// A presentation for a base whose content row binds none. Shadowkeep's unnamed audio component
/// (`80803AC9` resource, `0x1C0`-byte `80803ACF` variant rows at `+240` keyed at `+10`, default row
/// at `+80`) names a row's presentation parent at `+C0` or its entity at `+B0`. The rocket
/// launchers' shared owner `8152A37A` keeps a presentation in each of its twelve keyed rows, one per
/// stock rocket content key, while its default row, which any other key selects, names neither.
/// The translated slots then take the structure of the first keyed row whose root holds a slot
/// entity in every source slot. Its parent, root and slot entities become private copies bound into
/// the empty default row, and the root's other slots are cleared, so that no stock weapon's firing
/// presentation plays beside the translated one.
fn adopted(
    native: &mut Reader,
    components: &[Value],
    content: u32,
    slots: &BTreeSet<usize>,
) -> Result<Value> {
    let mut found = Vec::new();
    for component in components {
        if component["class"] != "80803AC9" {
            continue;
        }
        let owner_tag = hex(&component["owner"], "audio component owner")?;
        let owner = (*native.tag(owner_tag, Some(0x8080_9c36))?).clone();
        let resource = owner.pointer(24)?;
        ensure!(
            owner.u32(resource - 4)? == 0x8080_3ac9,
            "Native unnamed audio component differs"
        );
        let rows = owner.array(resource + 0x240, 0x1C0, Some(0x8080_3acf))?;
        let mut keys = BTreeSet::new();
        for &row in &rows {
            ensure!(
                keys.insert(owner.u32(row + 0x10)?),
                "Native audio content rows repeat a key"
            );
        }
        ensure!(
            !keys.contains(&content),
            "The base's content row {content:08X} binds no firing presentation"
        );
        let default = resource + 0x80;
        ensure!(
            unset(owner.u32(default + 0xC0)?) && unset(owner.u32(default + 0xB0)?),
            "The base's default audio row binds a presentation without slot entities"
        );
        for &row in &rows {
            let parent = owner.u32(row + 0xC0)?;
            if unset(parent) {
                continue;
            }
            let root = native.tag(parent, Some(0x8080_744a))?.u32(16)?;
            if unset(root) {
                continue;
            }
            let root_payload = (*native.tag(root, Some(0x8080_3adc))?).clone();
            let mut entities = BTreeMap::new();
            for slot in 0..6 {
                let entity = root_payload.u32(0x18 + slot * 0x10)?;
                if !unset(entity) && native.reference(entity).ok() == Some(0x8080_9c0f) {
                    entities.insert(slot, entity);
                }
            }
            if !slots.iter().all(|slot| entities.contains_key(slot)) {
                continue;
            }
            let bound = entities
                .iter()
                .filter(|(slot, _)| slots.contains(slot))
                .map(|(&slot, &entity)| json!({"slot":slot,"entity":format!("{entity:08X}")}))
                .collect::<Vec<_>>();
            let cleared = entities
                .keys()
                .filter(|slot| !slots.contains(slot))
                .copied()
                .collect::<Vec<_>>();
            found.push(json!({"owner":format!("{owner_tag:08X}"),
                "content_key":format!("{content:08X}"),"row":default,"parent_offset":default + 0xC0,
                "parent":format!("{parent:08X}"),"root":format!("{root:08X}"),"slots":bound,
                "bound_parent":"FFFFFFFF","cleared_slots":cleared,
                "adopted":{"key":format!("{:08X}",owner.u32(row + 0x10)?),"row":row}}));
            break;
        }
    }
    let [presentation] = found.as_slice() else {
        bail!(
            "The base's content row {content:08X} binds no firing presentation, and {} audio \
             components offer one to adopt",
            found.len()
        );
    };
    Ok(presentation.clone())
}

/// The bank's declaration names in order. Declarations are 112-byte rows at definition `+D8`, the
/// name first.
fn declared(bank: &Payload) -> Result<Vec<u32>> {
    let definition = bank.pointer(24)?;
    bank.array(definition + 0xD8, 112, Some(0x8080_97a1))?
        .into_iter()
        .map(|row| bank.u32(row))
        .collect()
}

/// The base slot entity's channel bank (resource `80809790`): its owner tag and payload.
fn slot_bank(native: &mut Reader, entity: &Payload) -> Result<(u32, Payload)> {
    let mut banks = Vec::new();
    for at in entity.array(16, 12, None)? {
        let owner = entity.u32(at)?;
        if unset(owner) {
            continue;
        }
        let o = native.tag(owner, Some(0x8080_9c36))?;
        if o.u32(o.pointer(24)? - 4)? == 0x8080_9790 {
            banks.push((owner, (*o).clone()));
        }
    }
    let [bank] = banks.as_slice() else {
        bail!("The base slot entity has no single channel bank");
    };
    Ok(bank.clone())
}

/// A slot entity's connection rows before and after its flow is replaced.
pub(crate) type Rows = (Vec<u8>, Vec<u8>);

/// Each slot's entity and the flow owner in it.
pub(crate) type Targets = BTreeMap<usize, (u32, u32)>;

/// A slot bank that inherits the translated inputs' names the base bank does not declare: the
/// base bank's tag it replaces, the private bank, its allocation tag and allocation, and the
/// inherited names.
pub(crate) struct PrivateBank {
    pub tag: u32,
    pub payload: Payload,
    pub allocation_tag: u32,
    pub allocation: Payload,
    pub inherited: Vec<u32>,
}

/// The slot entity's connection rows before and after its flow is replaced, or `None` when they
/// stay as they are, and the private bank they read, if any. Rows are 72 bytes: consumer owner
/// `+08` and input definition `+10`, provider owner `+28`, class `+2C` and object `+30`, and
/// channel index `+40`. Every row feeding the replaced flow leaves with it. Each translated input
/// then reads the slot bank's channel of its own name, through a copy of a stock row from the
/// bank's vector provider (`808097C2`, definition `+60`), as the base flow's own inputs do. Rise of
/// Iron and Shadowkeep firing banks share these names, such as `EE87AE6F` and `D9C46BC4` in the
/// auto rifle's slot 0.
///
/// A name the base bank does not declare is added to a private copy of the bank as inherited from
/// the parent channel of its own name in the default namespace, but only where the source bank
/// inherits or receives it under that name the same way (`inherit`). Stock firing banks resolve such names, among them
/// `9F9E5A67` and `6379397C`, in the default namespace. The grown bank is sealed with the
/// `object-channels` layout, since the client copies a bank instance only up to its definition,
/// as the retained sequence route found. Sealing keeps the original definition offsets, so the
/// copied provider object stays valid, and the build retargets the rows' bank tag to the private
/// bank. A named connection reading the flow cannot be moved this way.
pub(crate) fn connections(
    native: &mut Reader,
    entity: u32,
    flow: u32,
    inputs: &[(u32, usize)],
    inherit: &BTreeSet<u32>,
) -> Result<(Option<Rows>, Option<PrivateBank>)> {
    let e = (*native.tag(entity, Some(0x8080_9c0f))?).clone();
    let feeds = |row: usize| -> Result<bool> { Ok(e.u32(row + 8)? == flow) };
    for row in e.array(0x30, 72, Some(CONNECTION))? {
        ensure!(
            !feeds(row)?,
            "A named connection reads the base's firing flow"
        );
    }
    // A flow without inputs reads no channel, so its slot needs no bank of its own.
    let (bank, names, provider, private) = if inputs.is_empty() {
        (u32::MAX, Vec::new(), 0, None)
    } else {
        let (bank, stock) = slot_bank(native, &e)?;
        let provider = stock.pointer(24)? + 0x60;
        let names = declared(&stock)?;
        let missing = inputs
            .iter()
            .map(|&(name, _)| name)
            .filter(|name| !names.contains(name))
            .collect::<BTreeSet<_>>();
        if let Some(name) = missing.iter().find(|name| !inherit.contains(name)) {
            bail!(
                "The base slot bank declares no channel {name:08X}, which the source bank does not receive by name"
            );
        }
        if missing.is_empty() {
            (bank, names, provider, None)
        } else {
            let missing = missing.into_iter().collect::<Vec<_>>();
            let allocation_tag = stock.u32(0x44)?;
            let allocation = (*native.tag(allocation_tag, Some(0x8080_9bbb))?).clone();
            let (mut payload, allocation) =
                crate::tiger::channel::inherited::extend(&stock, &allocation, &missing)?;
            let layout = crate::tiger::instance::LAYOUTS
                .iter()
                .find(|layout| layout.symbol == "object-channels")
                .context("Channel bank instance layout")?;
            crate::tiger::instance::seal(&mut payload, &mut Vec::new(), layout)?;
            let names = declared(&payload)?;
            (
                bank,
                names,
                provider,
                Some(PrivateBank {
                    tag: bank,
                    payload,
                    allocation_tag,
                    allocation,
                    inherited: missing,
                }),
            )
        }
    };
    let rows = e.array(0x20, 72, Some(CONNECTION))?;
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut dropped = false;
    let mut template = None;
    for row in rows {
        let bytes = e.bytes::<72>(row)?;
        before.extend(bytes);
        if feeds(row)? {
            dropped = true;
            if e.u32(row + 40)? == bank
                && e.u32(row + 44)? == 0x8080_97c2
                && e.u64(row + 48)? == u64::try_from(provider)?
            {
                template.get_or_insert(bytes);
            }
        } else {
            after.extend(bytes);
        }
    }
    // A base flow without inputs leaves no row to copy. Stock rows from a bank's vector provider
    // all store the default namespace, the consumer flow and input-definition class `80809789`,
    // endpoint kind 1, the namespace again, the bank and provider class, and endpoint kind 2.
    let template = template.unwrap_or_else(|| {
        let mut row = [0; 72];
        for (at, word) in [
            (0, 0x811C_9DC5),
            (8, flow),
            (12, 0x8080_9789),
            (24, 1),
            (32, 0x811C_9DC5),
            (40, bank),
            (44, 0x8080_97C2),
            (56, 2),
        ] {
            row[at..at + 4].copy_from_slice(&u32::to_le_bytes(word));
        }
        row[48..56].copy_from_slice(&(provider as u64).to_le_bytes());
        row
    });
    for &(name, definition) in inputs {
        let channel = names
            .iter()
            .position(|&declared| declared == name)
            .with_context(|| format!("The slot bank declares no channel {name:08X}"))?;
        let mut row = template;
        row[16..24].copy_from_slice(&u64::try_from(definition)?.to_le_bytes());
        row[64..72].copy_from_slice(&u64::try_from(channel)?.to_le_bytes());
        after.extend(row);
    }
    if !dropped && inputs.is_empty() {
        return Ok((None, private));
    }
    Ok((Some((before, after)), private))
}

/// The base's firing presentation and, for each of `slots`, its slot entity and that entity's
/// flow owner. `rig` is the base's runtime rig, with its content key and components. A base whose
/// content row binds no presentation adopts one, as [`adopted`] describes.
pub(crate) fn slot_targets(
    native: &mut Reader,
    rig: &Value,
    slots: &BTreeSet<usize>,
) -> Result<(Value, Targets)> {
    let content = hex(&rig["content_key"], "rig content key")?;
    let components = rig["components"]
        .as_array()
        .context("Native rig components")?
        .clone();
    let inspected = crate::tiger::audio::inspect(native, &components, content, false)?;
    let groups = inspected["unnamed_groups"]
        .as_array()
        .context("Native firing presentation groups")?
        .iter()
        .filter(|group| group["slots"].as_array().is_some_and(|s| !s.is_empty()))
        .cloned()
        .collect::<Vec<_>>();
    let presentation = match groups.as_slice() {
        [presentation] => presentation.clone(),
        [] => adopted(native, &components, content, slots)?,
        _ => bail!(
            "The base's content row {content:08X} selects {} firing presentations with slot \
             entities, not one",
            groups.len()
        ),
    };
    ensure!(
        presentation["direct_entity"] != true,
        "The base's firing presentation names its entity directly"
    );
    let native_slot = |slot: usize| -> Result<u32> {
        let entry = presentation["slots"]
            .as_array()
            .context("Native presentation slots")?
            .iter()
            .find(|entry| entry["slot"].as_u64() == Some(slot as u64))
            .with_context(|| format!("The base's presentation has no slot {slot} entity"))?;
        hex(&entry["entity"], "presentation slot entity")
    };
    let mut targets = BTreeMap::new();
    for &slot in slots {
        let entity = native_slot(slot)?;
        let flow = native_flow(native, entity)?;
        targets.insert(slot, (entity, flow));
    }
    Ok((presentation, targets))
}
