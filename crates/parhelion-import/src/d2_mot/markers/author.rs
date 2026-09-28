//! Place source marker sets on their own art parts and relocate native resource pointers.
use super::*;
use crate::d2_mot::{bundle::add, reader::Reader};
use anyhow::Context;
use serde_json::{Value, json};
use std::{fs, path::Path};

fn tag(value: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().context("marker tag text")?,
        16,
    )?)
}

fn source_parts(source: &Path) -> Result<BTreeMap<(u64, u64), Vec<Marker>>> {
    let report: Value = serde_json::from_slice(&fs::read(source.join("report.json"))?)?;
    let mut result = BTreeMap::new();
    let Some(parts) = report["art_parts"].as_array() else {
        return Ok(result);
    };
    for part in parts {
        if part["missing"] == true {
            // A declared empty source region must not keep donor grip or aim
            // markers simply because its invisible carrier part still exists.
            for placement in part["placements"]
                .as_array()
                .context("empty source placements")?
            {
                let key = (
                    placement["selector"]
                        .as_u64()
                        .context("empty source selector")?,
                    placement["position"]
                        .as_u64()
                        .context("empty source position")?,
                );
                if let Some(previous) = result.insert(key, Vec::new()) {
                    ensure!(
                        previous.is_empty(),
                        "empty source marker placement conflicts with a part"
                    );
                }
            }
            continue;
        }
        let entity = tag(&part["entity"])?;
        if matches!(entity, 0 | u32::MAX | 0x811C9DC5) {
            continue;
        }
        let payload = Payload(fs::read(source.join(format!("raw/{entity:08X}.bin")))?);
        let mut found = None;
        for row in payload.array(8, 12, None)? {
            let owner = payload.u32(row)?;
            let path = source.join(format!("raw/{owner:08X}.bin"));
            if !path.exists() {
                continue;
            }
            if let Ok(markers) = read_source(&Payload(fs::read(path)?)) {
                ensure!(
                    found.replace(markers).is_none(),
                    "source part has ambiguous marker sets"
                );
            }
        }
        let Some(markers) = found else { continue };
        for placement in part["placements"]
            .as_array()
            .context("source marker placements")?
        {
            let (Some(selector), Some(position)) = (
                placement["selector"].as_u64(),
                placement["position"].as_u64(),
            ) else {
                continue;
            };
            if let Some(previous) = result.insert((selector, position), markers.clone()) {
                ensure!(previous == markers, "source marker placement is ambiguous");
            }
        }
    }
    Ok(result)
}

fn template(reader: &mut Reader, native: &Value) -> Result<Option<(u32, Payload, Payload)>> {
    for part in native["parents"]
        .as_array()
        .context("native marker parents")?
    {
        if part["marker_set"] != true {
            continue;
        }
        let parent = hex::decode(
            part["parent_bytes"]
                .as_str()
                .context("native marker parent")?,
        )?;
        let entity = u32::from_le_bytes(
            parent
                .get(16..20)
                .context("native marker entity")?
                .try_into()?,
        );
        let entity = reader.tag(entity, Some(0x80809C0F))?;
        for row in entity.array(16, 12, Some(0x80809C04))? {
            let owner = entity.u32(row)?;
            let payload = reader.tag(owner, Some(0x80809C36))?;
            let header = payload.pointer(16)?;
            if header >= 4 && payload.u32(header - 4)? == NATIVE_COMPONENT {
                read_native(&payload)?;
                return Ok(Some((owner, (*payload).clone(), (*entity).clone())));
            }
        }
    }
    Ok(None)
}

/// Native resource references are typed (owner, class, offset) triples. Updating
/// only an entity's component row leaves the component reading the old owner's data.
pub(super) fn relocate(payload: &mut Payload, owner: u32, symbol: &str) -> Result<Vec<Value>> {
    let mut patches = Vec::new();
    for at in (0..payload.0.len().saturating_sub(3)).step_by(4) {
        if payload.u32(at)? != owner {
            continue;
        }
        ensure!(
            payload.u32(at + 4)? & 0xFFFF0000 == 0x80800000
                && payload.u64(at + 8)? < payload.0.len() as u64,
            "marker owner has an untyped or out-of-bounds self-reference"
        );
        payload.0[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        patches.push(json!({"offset":at,"symbol":symbol}));
    }
    ensure!(
        !patches.is_empty(),
        "marker component lacks native resource pointers"
    );
    Ok(patches)
}

/// Replace a native component's complete marker set while preserving its registrations.
pub fn replace(template: &Payload, markers: &[Marker]) -> Result<Payload> {
    read_native(template)?;
    let mut payload = template.clone();
    let descriptor = payload
        .pointer(24)?
        .checked_add(NATIVE_DESCRIPTOR)
        .context("marker descriptor overflow")?;
    let end = descriptor
        .checked_add(16)
        .context("marker descriptor extent overflow")?;
    payload
        .0
        .get_mut(descriptor..end)
        .context("marker descriptor outside payload")?
        .fill(0);
    let rows = markers
        .iter()
        .map(|m| Carried {
            name: m.name,
            binding: m.binding,
            position: m.position,
            orientation: m.orientation,
        })
        .collect::<Vec<_>>();
    append(&mut payload, &rows)?;
    ensure!(
        read_native(&payload)?.len() == markers.len(),
        "source marker count differs"
    );
    Ok(payload)
}

/// Finalize all source part markers after separate and retained parts have been
/// authored. Source art placement, not marker-name overlap, selects the set.
pub fn author(
    reader: &mut Reader,
    source: &Path,
    native: &Value,
    directory: &Path,
    graph: &mut Value,
) -> Result<()> {
    let source_path = source;
    let source = source_parts(source_path)?;
    let fallback = template(reader, native)?;
    let host = graph["native_assignment"]
        .as_u64()
        .context("marker host assignment")?;
    let mut parts = Vec::new();
    for parent in native["parents"]
        .as_array()
        .context("native marker parents")?
    {
        if u64::from(tag(&parent["assignment"])?) == host {
            parts.push(("entity".to_owned(), parent["placement"].clone()));
        }
    }
    for part in graph["kept_parts"].as_array().into_iter().flatten() {
        let parent = part["parent"].as_str().context("kept marker parent")?;
        parts.push((
            format!(
                "{}entity",
                parent
                    .strip_suffix("parent")
                    .context("kept parent symbol")?
            ),
            part["placement"].clone(),
        ));
    }
    for part in graph["source_parts"].as_array().into_iter().flatten() {
        parts.push((
            part["entity"]
                .as_str()
                .context("source marker entity")?
                .to_owned(),
            json!({"selector":part["selector"],"position":part["position"]}),
        ));
    }
    let mut report = Vec::new();
    for (entity_symbol, placement) in &parts {
        let (Some(selector), Some(position)) = (
            placement["selector"].as_u64(),
            placement["position"].as_u64(),
        ) else {
            continue;
        };
        let Some(markers) = source.get(&(selector, position)) else {
            continue;
        };
        let nodes = graph["nodes"]
            .as_array_mut()
            .context("marker graph nodes")?;
        let index = nodes
            .iter()
            .position(|node| node["symbol"].as_str() == Some(entity_symbol.as_str()))
            .context("marker entity absent")?;
        let mut node = nodes[index].clone();
        let path = directory.join(node["file"].as_str().context("marker entity file")?);
        let mut entity = Payload(fs::read(&path)?);
        let mut found = None;
        for row in entity.array(16, 12, Some(0x80809C04))? {
            let patch = node["patches"]
                .as_array()
                .context("marker entity patches")?
                .iter()
                .find(|p| p["offset"].as_u64() == Some(row as u64));
            let original = if let Some(patch) = patch {
                let linked = nodes
                    .iter()
                    .find(|n| n["symbol"] == patch["symbol"])
                    .context("marker binding target absent")?;
                u32::try_from(
                    linked["template"]
                        .as_u64()
                        .context("marker owner template")?,
                )?
            } else {
                entity.u32(row)?
            };
            let payload = reader.tag(original, Some(0x80809C36))?;
            let header = payload.pointer(16)?;
            if header >= 4 && payload.u32(header - 4)? == NATIVE_COMPONENT {
                ensure!(found.is_none(), "entity has multiple marker components");
                found = Some((row, original, (*payload).clone()));
            }
        }
        let symbol = format!(
            "{}markers",
            entity_symbol
                .strip_suffix("entity")
                .context("marker entity symbol")?
        );
        if markers.is_empty() && found.is_none() {
            continue;
        }
        let (owner, original) = found
            .as_ref()
            .map(|(_, tag, p)| (*tag, p))
            .or_else(|| fallback.as_ref().map(|(tag, payload, _)| (*tag, payload)))
            .context("source markers require a native marker component layout")?;
        let mut payload = replace(original, markers)?;
        let marker_patches = relocate(&mut payload, owner, &symbol)?;
        let mut added = Vec::new();
        add(
            directory,
            &mut added,
            &symbol,
            owner,
            &payload.0,
            None,
            marker_patches,
        )?;
        if let Some(index) = nodes.iter().position(|n| n["symbol"] == symbol) {
            nodes[index] = added.remove(0);
        } else {
            nodes.extend(added);
        }
        let patches = node["patches"]
            .as_array_mut()
            .context("entity marker patches")?;
        if let Some((row, _, _)) = &found {
            bindings::retarget(&mut entity, patches, *row, owner, original, &symbol)?;
        } else {
            let (_, _, template) = fallback
                .as_ref()
                .context("native marker binding template")?;
            bindings::insert(&mut entity, patches, template, owner, &symbol)?;
        }
        fs::write(path, entity.0)?;
        nodes[index] = node;
        report.push(json!({"entity":entity_symbol,"markers":symbol,"selector":selector,"position":position,"count":markers.len(),"added_component":found.is_none(),"gameplay_verified":false}));
    }
    graph["source_markers"] = json!(report);
    optics::author(reader, source_path, native, directory, graph, &parts)?;
    Ok(())
}
