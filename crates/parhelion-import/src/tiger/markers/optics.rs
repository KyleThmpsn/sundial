//! Optics carry their aim transforms in embedded marker providers, separately
//! from the art part's marker set. Preserve the native component and its wiring.
use super::*;
use crate::{graph::add, tiger::reader::Reader};
use anyhow::Context;
use serde_json::{Value, json};
use std::{fs, path::Path};

/// Source model input endpoints keyed by the source component tag and input hash,
/// holding the composed model's symbol, its native input hash and its component index.
pub(crate) type ModelInputs = BTreeMap<(u32, u64), (String, u64, u32)>;

/// Align a native optic to object-space points while retaining its runtime interfaces.
pub(crate) fn aligned(native: &Payload, rear: [f32; 3], front: [f32; 3]) -> Result<Payload> {
    ensure!(
        rear.iter().chain(&front).all(|v| v.is_finite()) && front[0] > rear[0],
        "Invalid native sight axis"
    );
    let data = native.pointer(24)?;
    ensure!(
        native.u32(data - 4)? == 0x8080393B,
        "Expected native optic component"
    );
    let mut result = native.clone();
    for (offset, name, point) in [(0x198, 0x8D0DD3CD, rear), (0x228, 0x0B7BA45D, front)] {
        ensure!(
            native.u32(data + offset - 40)? == 0x80809BE2
                && native.u32(data + offset - 12)? == name,
            "Native aim provider layout differs"
        );
        let rows = read(
            native,
            data + offset,
            Layout {
                row_class: 0x80809C00,
                ..NATIVE
            },
        )?;
        ensure!(
            rows.len() == 1 && rows[0].name == name,
            "Native optic needs one default transform"
        );
        let at = rows[0].row;
        result.0[at..at + 16].fill(0);
        for (start, values) in [
            (16, [0., 0., 0., 1.]),
            (32, [point[0], point[1], point[2], 1.]),
        ] {
            for (i, value) in values.into_iter().enumerate() {
                result.0[at + start + i * 4..at + start + i * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        result.0[data + offset - 32..data + offset - 28].copy_from_slice(&1u32.to_le_bytes());
    }
    Ok(result)
}

fn source_parts(source: &Path) -> Result<BTreeMap<(u64, u64), Payload>> {
    let report: Value = serde_json::from_slice(&fs::read(source.join("report.json"))?)?;
    let mut result = BTreeMap::new();
    for part in report["art_parts"].as_array().into_iter().flatten() {
        if part["missing"] == true {
            continue;
        }
        let Some(tag) = part["entity"].as_str() else {
            continue;
        };
        let path = source.join(format!("raw/{tag}.bin"));
        if !path.exists() {
            continue;
        }
        let entity = Payload(fs::read(path)?);
        for row in entity.array(8, 12, Some(0x80809ACD))? {
            let path = source.join(format!("raw/{:08X}.bin", entity.u32(row)?));
            if !path.exists() {
                continue;
            }
            let owner = Payload(fs::read(path)?);
            let data = owner.pointer(24)?;
            if data < 4 || owner.u32(data - 4)? != 0x80802B93 {
                continue;
            }
            // Bows use procedural aiming without embedded default poses. Their
            // already converted ADS visibility controls are independent of this pass.
            if owner.u64(data + 0x1B0)? == 0 && owner.u64(data + 0x250)? == 0 {
                continue;
            }
            for placement in part["placements"].as_array().context("optic placements")? {
                let key = (
                    placement["selector"].as_u64().context("optic selector")?,
                    placement["position"].as_u64().context("optic position")?,
                );
                if let Some(previous) = result.insert(key, owner.clone()) {
                    ensure!(previous.0 == owner.0, "source optic placement is ambiguous");
                }
            }
        }
    }
    Ok(result)
}

fn convert(source: &Payload, native: &Payload) -> Result<Payload> {
    let s = source.pointer(24)?;
    let n = native.pointer(24)?;
    ensure!(
        source.u32(s - 4)? == 0x80802B93
            && native.u32(n - 4)? == 0x8080393B
            && source.u32(s + 0x154)? == 0x80802BA4
            && native.u32(n + 0x13C)? == 0x8080394B,
        "optic embedded aim provider layout differs"
    );
    let mut result = native.clone();
    // This scalar occupies the same fixed optic prefix in both eras. Native
    // sights use positive angular overrides or the negative one-degree sentinel.
    // Keeping the shell's override changes aiming even with the source markers.
    for (p, base) in [(source, s), (native, n)] {
        let angle = p.f32(base + 0x80)?;
        ensure!(
            angle.is_finite()
                && (-0.018..=std::f32::consts::PI).contains(&angle)
                && p.bytes::<12>(base + 0x84)? == [0; 12],
            "optic angular override layout differs"
        );
    }
    result.0[n + 0x80..n + 0x84].copy_from_slice(&source.bytes::<4>(s + 0x80)?);
    for (sd, nd, name) in [(0x1B0, 0x198, 0x8D0DD3CD), (0x250, 0x228, 0x0B7BA45D)] {
        ensure!(
            source.u32(s + sd - 12)? == name && native.u32(n + nd - 12)? == name,
            "optic aim provider name differs"
        );
        for (p, base, offset, class) in [(source, s, sd, 0x80809AA8), (native, n, nd, 0x80809BE2)] {
            ensure!(
                p.u32(base + offset - 40)? == class
                    && p.u32(base + offset - 36)? == 0
                    && p.u32(base + offset - 32)? <= 2
                    && p.u32(base + offset - 28)? == u32::MAX
                    && p.bytes::<8>(base + offset - 24)? == [0; 8],
                "optic aim provider mode layout differs"
            );
        }
        result.0[n + nd - 32..n + nd - 28].copy_from_slice(&source.bytes::<4>(s + sd - 32)?);
        let modern = read(
            source,
            s + sd,
            Layout {
                row_class: 0x80809AC9,
                ..SOURCE
            },
        )?;
        let old = read(
            native,
            n + nd,
            Layout {
                row_class: 0x80809C00,
                ..NATIVE
            },
        )?;
        ensure!(
            modern.len() <= 1 && old.len() <= 1,
            "optic has multiple default aim transforms"
        );
        let mut rows = Vec::new();
        for from in &modern {
            ensure!(
                from.name == name,
                "source optic aim marker identity differs"
            );
            // Source rows put the root binding and its mode after the transform.
            // Native rows keep the same pair in a 16-byte prefix. Both mode 0
            // and mode 1 occur in native defaults. Other bone bindings need an
            // explicit skeleton mapping before they can be carried.
            ensure!(
                source.u32(from.row + 32)? == 0 && source.u32(from.row + 36)? <= 1,
                "source optic default has an unsupported root binding"
            );
            let mut row = if let Some(to) = old.first() {
                ensure!(to.name == name, "native optic aim marker identity differs");
                native.bytes::<64>(to.row)?.to_vec()
            } else {
                let mut row = vec![0; 64];
                row[48..52].copy_from_slice(&name.to_le_bytes());
                row
            };
            ensure!(
                row[8..16] == [0; 8],
                "native optic binding extension differs"
            );
            row[..8].copy_from_slice(&source.bytes::<8>(from.row + 32)?);
            for (at, values) in [(16, from.orientation), (32, from.position)] {
                for (i, value) in values.iter().enumerate() {
                    row[at + i * 4..at + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }
            }
            rows.extend(row);
        }
        // Native marker rows are 64 bytes. The following 16 bytes belong to
        // the next serialized array's alignment and class prefix.
        bindings::append(
            &mut result,
            &mut Vec::new(),
            n + nd,
            64,
            0x80809C00,
            &rows,
            &[],
        )?;
    }
    Ok(result)
}

fn template(
    reader: &mut Reader,
    native: &Value,
    placement: &Value,
) -> Result<Option<(u32, Payload, Payload)>> {
    let mut candidates = native["parents"]
        .as_array()
        .context("native optic parents")?
        .iter()
        .filter(|part| part["placement"]["selector"] == placement["selector"])
        .collect::<Vec<_>>();
    candidates.sort_by_key(|part| {
        (
            part["placement"] != *placement,
            part["placement"]["position"].as_u64(),
        )
    });
    let mut rejected = None;
    for part in candidates {
        let mut found = None;
        let bytes = hex::decode(
            part["parent_bytes"]
                .as_str()
                .context("optic parent bytes")?,
        )?;
        let tag = u32::from_le_bytes(
            bytes
                .get(16..20)
                .context("optic parent entity")?
                .try_into()?,
        );
        let entity = reader.tag(tag, Some(0x80809C0F))?;
        for row in entity.array(16, 12, Some(0x80809C04))? {
            let tag = entity.u32(row)?;
            let owner = reader.tag(tag, Some(0x80809C36))?;
            if owner.u32(owner.pointer(24)? - 4)? == 0x8080393B {
                ensure!(found.is_none(), "native optic placement ambiguous");
                found = Some((tag, (*owner).clone(), (*entity).clone()));
            }
        }
        if let Some((tag, owner, entity)) = found {
            // Only the component format and event interfaces are carried. The
            // actual aim poses come from this source alternative. Some native
            // sights also require unsupported display components, so try an
            // equivalent supported format within the same art region.
            match closure(reader, &entity, tag, "optic-template") {
                Ok(_) => return Ok(Some((tag, owner, entity))),
                Err(error) => rejected = Some(error),
            }
        }
    }
    if let Some(error) = rejected {
        return Err(error.context("no compatible optic component format in this art region"));
    }
    Ok(None)
}

fn closure(
    reader: &mut Reader,
    entity: &Payload,
    optic: u32,
    symbol: &str,
) -> Result<BTreeMap<u32, (String, Payload)>> {
    let mut tags = std::collections::BTreeSet::from([optic]);
    let events = entity.array(0x20, 72, Some(0x80809BC9))?;
    loop {
        let before = tags.len();
        for &row in &events {
            let ends = [entity.u32(row + 8)?, entity.u32(row + 40)?];
            if ends.iter().any(|tag| tags.contains(tag)) {
                tags.extend(ends.into_iter().filter(|tag| *tag != u32::MAX));
            }
        }
        if tags.len() == before {
            break;
        }
        ensure!(
            tags.len() <= 3,
            "optic event closure exceeds the supported controller and model inputs"
        );
    }
    let mut result = BTreeMap::new();
    for tag in tags {
        let payload = reader.tag(tag, Some(0x80809C36))?;
        let class = payload.u32(payload.pointer(24)? - 4)?;
        ensure!(
            if tag == optic {
                class == 0x8080393B
            } else {
                matches!(class, 0x80809790 | 0x808072BD)
            },
            "optic event closure contains unsupported component {class:08X}"
        );
        let name = if tag == optic {
            symbol.to_owned()
        } else if class == 0x80809790 {
            format!("{symbol}-controller")
        } else {
            format!("{symbol}-model-inputs")
        };
        result.insert(tag, (name, (*payload).clone()));
    }
    Ok(result)
}

fn model_inputs(
    source_tag: u32,
    source: &Payload,
    target: &Payload,
    symbol: &str,
    entity: &Payload,
    node: &Value,
) -> Result<ModelInputs> {
    let source_names = crate::tiger::channel::object_channel_map(&source.0)?;
    let target_names = crate::tiger::channel::object_channel_map(&target.0)?;
    let components = entity.array(16, 12, Some(0x80809C04))?;
    let indices = components
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            node["patches"].as_array().is_some_and(|patches| {
                patches
                    .iter()
                    .any(|p| p["offset"].as_u64() == Some(**row as u64) && p["symbol"] == symbol)
            })
        })
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    ensure!(
        indices.len() == 1,
        "composed optic has no unique imported model binding"
    );
    let old = source.array(source.pointer(16)? + 0x120, 96, Some(0x80809788))?;
    let new = target.array(target.pointer(16)? + 0x120, 96, Some(0x80809788))?;
    source_names
        .iter()
        .map(|(name, index)| {
            let target_index = *target_names
                .get(name)
                .with_context(|| format!("optic model input {name} is absent"))?;
            Ok((
                (source_tag, source.u64(old[usize::from(*index)] + 8)?),
                (
                    symbol.to_owned(),
                    target.u64(new[usize::from(target_index)] + 8)?,
                    u32::try_from(indices[0])?,
                ),
            ))
        })
        .collect()
}

pub(super) fn author(
    reader: &mut Reader,
    source: &Path,
    native: &Value,
    directory: &Path,
    graph: &mut Value,
    parts: &[(String, Value)],
) -> Result<()> {
    let sources = source_parts(source)?;
    let mut report = Vec::new();
    for (entity_symbol, placement) in parts {
        let (Some(selector), Some(position)) = (
            placement["selector"].as_u64(),
            placement["position"].as_u64(),
        ) else {
            continue;
        };
        let Some(source) = sources.get(&(selector, position)) else {
            continue;
        };
        let nodes = graph["nodes"].as_array_mut().context("optic nodes")?;
        let index = nodes
            .iter()
            .position(|n| n["symbol"] == *entity_symbol)
            .context("optic entity")?;
        let mut node = nodes[index].clone();
        let path = directory.join(node["file"].as_str().context("optic entity file")?);
        let mut entity = Payload(fs::read(&path)?);
        let mut found = None;
        for row in entity.array(16, 12, Some(0x80809C04))? {
            let patch = node["patches"]
                .as_array()
                .context("optic patches")?
                .iter()
                .find(|p| p["offset"].as_u64() == Some(row as u64));
            let tag = if let Some(patch) = patch {
                let linked = nodes
                    .iter()
                    .find(|n| n["symbol"] == patch["symbol"])
                    .context("optic target")?;
                u32::try_from(linked["template"].as_u64().context("optic template")?)?
            } else {
                entity.u32(row)?
            };
            let owner = reader.tag(tag, Some(0x80809C36))?;
            let data = owner.pointer(24)?;
            if data >= 4 && owner.u32(data - 4)? == 0x8080393B {
                ensure!(found.is_none(), "native optic component ambiguous");
                found = Some((row, tag, (*owner).clone()));
            }
        }
        let symbol = format!(
            "{}optics",
            entity_symbol
                .strip_suffix("entity")
                .context("optic symbol")?
        );
        let added_component = found.is_none();
        let fallback = if added_component {
            template(reader, native, placement)?
        } else {
            None
        };
        let Some((tag, original)) = found
            .as_ref()
            .map(|(_, tag, payload)| (*tag, payload))
            .or_else(|| fallback.as_ref().map(|(tag, payload, _)| (*tag, payload)))
        else {
            report.push(json!({"entity":entity_symbol,"status":"native_component_absent","selector":selector,"position":position}));
            continue;
        };
        let mut payload = convert(source, original)?;
        let patches = author::relocate(&mut payload, tag, &symbol)?;
        let mut added = Vec::new();
        add(
            directory, &mut added, &symbol, tag, &payload.0, None, patches,
        )?;
        if let Some(i) = nodes.iter().position(|n| n["symbol"] == symbol) {
            nodes[i] = added.remove(0);
        } else {
            nodes.extend(added);
        }
        let mut companions = Vec::new();
        if let Some((row, _, _)) = &found {
            bindings::retarget(
                &mut entity,
                node["patches"].as_array_mut().context("optic patches")?,
                *row,
                tag,
                original,
                &symbol,
            )?;
        } else {
            let (_, _, template) = fallback.as_ref().context("optic component template")?;
            let mut components = closure(reader, template, tag, &symbol)?;
            let geometry = components
                .iter()
                .filter_map(|(&tag, (_, p))| {
                    (p.u32(p.pointer(24).ok()?.checked_sub(4)?).ok()? == 0x808072BD).then_some(tag)
                })
                .collect::<Vec<_>>();
            ensure!(
                geometry.len() == 1,
                "optic closure lacks a unique model input provider"
            );
            let old_model = geometry[0];
            let (_, old_payload) = components
                .remove(&old_model)
                .context("optic model input provider")?;
            let model_symbol = format!(
                "{}owner",
                entity_symbol
                    .strip_suffix("entity")
                    .context("optic entity symbol")?
            );
            let model_node = nodes
                .iter()
                .find(|n| n["symbol"] == model_symbol)
                .context("imported optic geometry")?;
            let model = Payload(fs::read(
                directory.join(model_node["file"].as_str().context("optic model file")?),
            )?);
            let inputs = model_inputs(
                old_model,
                &old_payload,
                &model,
                &model_symbol,
                &entity,
                &node,
            )?;
            let owners = components
                .iter()
                .map(|(tag, (name, _))| (*tag, name.clone()))
                .collect::<BTreeMap<_, _>>();
            for (&owner, (name, original)) in &components {
                if owner == tag {
                    continue;
                }
                let mut payload = original.clone();
                let patches = author::relocate(&mut payload, owner, name)?;
                // Cross-component behavior travels through the audited entity
                // event table. Reject hidden payload links outside that table.
                for &other in owners.keys().filter(|&&other| other != owner) {
                    ensure!(
                        !payload.0.chunks_exact(4).any(|w| w == other.to_le_bytes()),
                        "optic controller contains an unconverted cross-owner link"
                    );
                }
                let mut added = Vec::new();
                add(
                    directory, &mut added, name, owner, &payload.0, None, patches,
                )?;
                nodes.extend(added);
                companions.push(name.clone());
            }
            bindings::graft::insert(
                reader,
                &mut entity,
                node["patches"].as_array_mut().context("optic patches")?,
                template,
                &owners,
                &inputs,
            )?;
        }
        fs::write(path, entity.0)?;
        nodes[index] = node;
        report.push(json!({"entity":entity_symbol,"optics":symbol,"status":"source_aim_transforms","source_provider_modes":true,"source_angular_override":true,"selector":selector,"position":position,"added_component":added_component,"companions":companions,"gameplay_verified":false}));
    }
    graph["source_optics"] = json!(report);
    Ok(())
}
