//! Package-only provenance for modern sandbox perks before native lowering.
//!
//! A trace preserves source dispatch kinds and component selectors. Neither is a native
//! enum, and a successful trace must never be presented as an installable private perk.
use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use super::{resource, table};
use crate::d2_mot::{payload::Payload, reader::Reader};

pub mod controller;
pub mod kinetic;
pub mod lower;
pub mod presentation;
pub mod sword;
pub mod translate;

const CONTROLLER: u32 = 0x8080B835;
const MODIFIER: u32 = 0x80802D32;
const SETTINGS: u32 = 0x80802D33;
// Shared sandbox assignment root in the supported modern package format. As in the native
// sandbox reader, validate the root and row classes. Other 978C maps include unrelated,
// encrypted content and are not candidates for this table.
const ASSIGNMENTS: u32 = 0x80C0C0E3;

pub struct Assignment {
    pub item_tag: u32,
    pub perk_hash: u32,
    pub runtime_key: u32,
    pub action_tag: u32,
    pub controller: std::sync::Arc<Payload>,
}

/// Resolve every controller through the source investment tables, without an item allowlist.
pub fn assigned(r: &mut Reader, plug_hash: u32) -> Result<Vec<Assignment>> {
    let (item_tag, item) = item(r, plug_hash)?;
    let block = resource(&item, 0x68, 0x80807381)?;
    let identities = table(r, 0x808076AA)?;
    let indices = identities.array(8, 12, Some(0x808076AE))?;
    let finished = table(r, 0x8080542D)?;
    let definitions = finished.array(8, 40, Some(0x80805433))?;
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for row in rows(&item, block + 16, 32, 0x80807387)? {
        let identity = *indices
            .get(usize::try_from(item.u32(row)?)?)
            .context("source perk index")?;
        let perk_hash = identities.u32(identity)?;
        let matching = definitions
            .iter()
            .copied()
            .filter(|at| finished.u32(*at).ok() == Some(perk_hash))
            .collect::<Vec<_>>();
        ensure!(
            matching.len() == 1,
            "source perk identity is missing or ambiguous"
        );
        let runtime_key = finished.u32(matching[0] + 4)?;
        let action_tag =
            action(r, runtime_key)?.context("source perk has no standalone controller")?;
        ensure!(seen.insert(action_tag), "source plug repeats a controller");
        result.push(Assignment {
            item_tag,
            perk_hash,
            runtime_key,
            action_tag,
            controller: r.tag(action_tag, Some(CONTROLLER))?,
        });
    }
    ensure!(!result.is_empty(), "source plug has no runtime controllers");
    Ok(result)
}

fn rows(p: &Payload, at: usize, stride: usize, class: u32) -> Result<Vec<usize>> {
    ensure!(p.u64(at)? <= 256, "perk list exceeds trace limit at {at:X}");
    p.array(at, stride, Some(class))
}

fn pointed_class(p: &Payload, at: usize) -> Result<u32> {
    ensure!(at >= 4, "perk record lacks a class word");
    let class = p.u32(at - 4)?;
    ensure!(class >> 16 == 0x8080, "invalid perk class at {at:X}");
    Ok(class)
}

fn path(p: &Payload, at: usize) -> Result<Option<String>> {
    if p.u64(at)? == 0 {
        return Ok(None);
    }
    let start = p.pointer(at)?;
    let tail = p.0.get(start..).context("perk path outside payload")?;
    let end = tail
        .iter()
        .position(|b| *b == 0)
        .context("unterminated perk path")?;
    Ok(Some(std::str::from_utf8(&tail[..end])?.to_owned()))
}

fn item(r: &mut Reader, hash: u32) -> Result<(u32, std::sync::Arc<Payload>)> {
    let mut found = BTreeSet::new();
    for tag in r.classes(0x80807997) {
        let index = r.tag(tag, Some(0x80807997))?;
        for row in index.array(8, 32, Some(0x8080799B))? {
            if index.u32(row)? == hash {
                found.insert(r.ref64(&index, row + 16)?);
            }
        }
    }
    ensure!(
        found.len() == 1,
        "missing or ambiguous source plug {hash:08X}"
    );
    let tag = *found.first().context("source plug")?;
    Ok((tag, r.tag(tag, Some(0x8080799D))?))
}

fn action(r: &mut Reader, key: u32) -> Result<Option<u32>> {
    if [0, u32::MAX, 0x811C9DC5].contains(&key) {
        return Ok(None);
    }
    let map = r.tag(ASSIGNMENTS, Some(0x8080978C))?;
    let mut matches = Vec::new();
    for row in map.array(8, 24, Some(0x8080870F))? {
        if map.u32(row)? == key {
            matches.push(r.ref64(&map, row + 8)?);
        }
    }
    ensure!(
        matches.len() <= 1,
        "ambiguous source perk assignment {key:08X}"
    );
    Ok(matches.first().copied())
}

fn modifiers(
    r: &mut Reader,
    tag: u32,
    p: &Payload,
    instance: usize,
    settings: usize,
) -> Result<Value> {
    let runtime = rows(p, instance + 0x50, 128, MODIFIER)?;
    let definitions = rows(p, settings + 0x58, 112, SETTINGS)?;
    ensure!(
        runtime.len() == definitions.len(),
        "modifier pair count mismatch"
    );
    let mut result = Vec::new();
    for (live, definition) in runtime.into_iter().zip(definitions) {
        ensure!(
            p.u32(live)? == tag
                && p.u32(live + 4)? == SETTINGS
                && p.u64(live + 8)? == definition as u64
                && p.u32(definition)? == tag
                && p.u32(definition + 4)? == MODIFIER
                && p.u64(definition + 8)? == live as u64,
            "modifier pair does not address its own owner"
        );
        let metadata = [0x20, 0x40]
            .into_iter()
            .map(|offset| {
                let dependency = r.ref64(p, definition + offset)?;
                r.tag(dependency, None)?;
                Ok(format!("{dependency:08X}"))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut modifier = json!({
            "offset": definition, "instance_offset": live, "class": format!("{SETTINGS:08X}"),
            "amount": p.f32(definition + 0x10)?, "amount_bits": format!("{:08X}", p.u32(definition + 0x10)?),
            "operation": p.u8(definition + 0x14)?, "ability": p.i16(definition + 0x58)?,
            "input": p.u16(definition + 0x5A)?, "input_key": p.u32(definition + 0x5C)?,
            "component": p.u8(definition + 0x60)?, "metadata": metadata,
            "native_component": null, "native_input": null
        });
        match lower::modifier_input(p.u8(definition + 0x60)?, p.u16(definition + 0x5A)?) {
            Ok((component, input)) => {
                modifier["native_component"] = json!(component);
                modifier["native_input"] = json!(input);
            }
            Err(error) => modifier["lowering_error"] = json!(error.to_string()),
        }
        result.push(modifier);
    }
    for offset in [0x70, 0x90, 0xB0, 0xD8] {
        let dependency = r.ref64(p, settings + offset)?;
        r.tag(dependency, None)?;
    }
    Ok(json!(result))
}

fn entity(r: &mut Reader, tag: u32) -> Result<Value> {
    let p = r.tag(tag, Some(0x80809AD8))?;
    let mut components = Vec::new();
    for row in rows(&p, 8, 12, 0x80809ACD)? {
        let owner = p.u32(row)?;
        let data = r.tag(owner, Some(0x80809B06))?;
        let live = data.pointer(16)?;
        let definition = data.pointer(24)?;
        let live_class = pointed_class(&data, live)?;
        let definition_class = pointed_class(&data, definition)?;
        let mut record = json!({"owner": format!("{owner:08X}"), "instance_offset": live,
            "definition_offset": definition, "instance_class": format!("{live_class:08X}"),
            "definition_class": format!("{definition_class:08X}"), "decoded_modifiers": false});
        if live_class == 0x80802D2A && definition_class == 0x80802D2B {
            ensure!(
                data.u32(live)? == owner
                    && data.u32(live + 4)? == definition_class
                    && data.u64(live + 8)? == definition as u64
                    && data.u32(definition)? == owner
                    && data.u32(definition + 4)? == live_class
                    && data.u64(definition + 8)? == live as u64,
                "component root pair mismatch"
            );
            record["modifiers"] = modifiers(r, owner, &data, live, definition)?;
            record["decoded_modifiers"] = json!(true);
        }
        components.push(record);
    }
    Ok(json!({"tag": format!("{tag:08X}"), "components": components}))
}

fn expression(p: &Payload, at: usize) -> Result<Value> {
    let code = rows(p, at + 0x28, 1, 0x80800009)?
        .into_iter()
        .map(|row| p.u8(row))
        .collect::<Result<Vec<_>>>()?;
    let constants = rows(p, at + 0x38, 16, 0x80800090)?
        .into_iter()
        .map(|row| Ok(hex::encode(p.bytes::<16>(row)?)))
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"code": hex::encode(code), "constant_bits": constants,
        "header": hex::encode(p.bytes::<120>(at)?)}),
    )
}

fn record_lowering(value: &mut Value, result: Result<lower::Node>) {
    match result {
        Ok(native) => {
            value["native_kind"] = json!(native.kind);
            value["native_class"] = json!(format!("{:08X}", native.class));
            value["native_node"] = json!({
                "kind": native.kind,
                "bytes": format!("0x{}", hex::encode(native.bytes))
            });
        }
        Err(error) => value["lowering_error"] = json!(error.to_string()),
    }
}

fn node(r: &mut Reader, p: &Payload, row: usize, condition: bool) -> Result<Value> {
    ensure!(p.u64(row)? != 0, "null perk node");
    let at = p.pointer(row)?;
    let class = pointed_class(p, at)?;
    let kind = p.u8(at + if condition { 5 } else { 0 })?;
    // A record can end its payload in fewer than eight bytes, as a four-byte effect does.
    let header =
        p.0.get(at..(at + 8).min(p.0.len()))
            .context("perk node outside payload")?;
    let mut value = json!({"offset": at, "class": format!("{class:08X}"), "source_kind": kind,
        "header": hex::encode(header), "native_kind": null});
    if condition {
        if class == 0x80803060 {
            ensure!(kind == 1, "unexpected source timer dispatch");
            value["seconds"] = json!(p.f32(at + 8)?);
            record_lowering(&mut value, lower::timer_condition(p, at));
        } else if class == 0x80803086 {
            record_lowering(&mut value, lower::weapon_swap_condition(p, at));
        } else if class == 0x8080BDCF {
            ensure!(kind == 49, "unexpected source condition 49 dispatch");
            // Forty bytes fit every checked record. Longer windows can spill into the
            // next typed object, so preserve only the validated common prefix.
            value["source_record"] = json!(hex::encode(p.bytes::<40>(at)?));
            value["source_record_complete"] = json!(false);
            record_lowering(&mut value, lower::object_slot_condition(p, at));
        } else if class == 0x808030BE {
            record_lowering(&mut value, lower::ability_condition(p, at));
        } else if class == 0x80803061 {
            record_lowering(&mut value, lower::state_value_condition(p, at));
        }
    } else if class == 0x808022E5 {
        record_lowering(&mut value, lower::component_value(p, at));
    } else if class == 0x808030F3 {
        record_lowering(&mut value, lower::host_record(p, at));
    } else if [0x80803130, 0x8080B7C6].contains(&class) {
        ensure!(
            kind == if class == 0x80803130 { 2 } else { 1 },
            "unexpected source attachment dispatch"
        );
        value["path"] = json!(path(p, at + 8)?);
        let tag = r.ref64(p, at + 16)?;
        value["entity"] = entity(r, tag)?;
        if class == 0x80803130 {
            value["expression"] = expression(p, at)?;
        }
    }
    Ok(value)
}

fn controller(r: &mut Reader, tag: u32) -> Result<Value> {
    let class = r.reference(tag)?;
    let p = r.tag(tag, Some(class))?;
    let mut result = json!({"tag": format!("{tag:08X}"), "class": format!("{class:08X}"),
        "requires_native_lowering": true, "states": [], "auxiliary": null,
        "decoded_controller": false});
    if class != CONTROLLER {
        return Ok(result);
    }
    ensure!(
        p.u64(0)? == p.0.len() as u64,
        "controller declared size mismatch"
    );
    let decoded = controller::read(&p).with_context(|| format!("perk controller {tag:08X}"))?;
    let auxiliary = decoded
        .auxiliary
        .iter()
        .map(|record| {
            json!({
                "offset": record.offset,
                "class": format!("{:08X}", record.class),
                "state_reference_class": (record.class == controller::STATE_AUXILIARY).then_some("8080B848"),
                "key": record.key.map(|key| format!("{key:08X}")),
                "unknown_integer": record.unknown_integer,
                "state_offsets": &record.state_offsets,
                "state_keys": record.state_keys.iter().map(|key| format!("{key:08X}")).collect::<Vec<_>>(),
                "source_record": &record.source_record,
            })
        })
        .collect::<Vec<_>>();
    let mut states = Vec::new();
    for source in decoded.states {
        let state = source.offset;
        let effects = rows(&p, state + 8, 24, 0x808037AB)?
            .into_iter()
            .map(|row| node(r, &p, row, false))
            .collect::<Result<Vec<_>>>()
            .with_context(|| format!("perk controller {tag:08X} state effects at {state:X}"))?;
        let mut groups = Vec::new();
        for transition in source.transitions {
            let group = transition.offset;
            let destination = transition.destination;
            let conditions = rows(&p, group + 16, 8, 0x808037B9)?
                .into_iter()
                .map(|row| node(r, &p, row, true))
                .collect::<Result<Vec<_>>>()
                .with_context(|| format!("perk controller {tag:08X} conditions at {group:X}"))?;
            groups.push(
                json!({"offset": group, "key": p.u32(group)?, "conditions": conditions,
                "destination_offset": destination, "destination_key": p.u32(destination)?,
                "condition_tree": transition.conditions,
                "source_record": hex::encode(p.bytes::<48>(group)?)}),
            );
        }
        states.push(
            json!({"offset": state, "key": p.u32(state)?, "effects": effects,
            "condition_groups": groups, "event_mask": format!("{:016X}", source.event_mask),
            "source_record": hex::encode(p.bytes::<136>(state)?)}),
        );
    }
    result["states"] = json!(states);
    result["auxiliary"] = json!(auxiliary);
    result["decoded_controller"] = json!(true);
    result["source_header"] = json!(hex::encode(p.bytes::<80>(0)?));
    Ok(result)
}

/// One finished sandbox perk's runtime controller, found through its runtime key.
fn finished_perk(r: &mut Reader, hash: u32) -> Result<Value> {
    let finished = table(r, 0x8080542D)?;
    let matches = finished
        .array(8, 40, Some(0x80805433))?
        .into_iter()
        .filter(|at| finished.u32(*at).ok() == Some(hash))
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "missing or ambiguous finished perk {hash:08X}"
    );
    let key = finished.u32(matches[0] + 4)?;
    let mut record = match action(r, key)? {
        Some(tag) => controller(r, tag)?,
        None => json!({"states": [], "unresolved": "No standalone runtime assignment"}),
    };
    record["perk_hash"] = json!(hash);
    record["runtime_key"] = json!(key);
    Ok(record)
}

/// Trace an item's own sandbox perk, one that no plug carries, such as an exotic's item perk.
pub fn trace_perk(r: &mut Reader, perk_hash: u32) -> Result<Value> {
    Ok(
        json!({"format": 1, "perk_hash": perk_hash, "installable": false,
        "perks": [finished_perk(r, perk_hash)?]}),
    )
}

/// Trace a plug through its source investment identities, runtime controller and attachments.
/// Raw reads and their source manifest are saved by `Reader`. This does not emit native assets.
pub fn trace(r: &mut Reader, plug_hash: u32) -> Result<Value> {
    let (item_tag, item) = item(r, plug_hash)?;
    let block = resource(&item, 0x68, 0x80807381)?;
    let identities = table(r, 0x808076AA)?;
    let indices = identities.array(8, 12, Some(0x808076AE))?;
    let mut perks = Vec::new();
    for source in rows(&item, block + 16, 32, 0x80807387)? {
        let index = item.u32(source)? as usize;
        let identity = *indices
            .get(index)
            .context("perk index outside source catalog")?;
        let mut record = finished_perk(r, identities.u32(identity)?)?;
        record["source_index"] = json!(index);
        perks.push(record);
    }
    Ok(
        json!({"format": 1, "plug_hash": plug_hash, "item_tag": format!("{item_tag:08X}"),
        "installable": false, "perks": perks,
        "remaining_contracts": ["Controller state transitions and native event routing",
            "Condition and effect class mappings", "Component and property selector mappings",
            "Expressions, label dictionaries and private dependency allocation"]}),
    )
}
