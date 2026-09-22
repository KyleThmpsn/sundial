//! Native carrier mapping, not shader bytecode translation. Stage identities 0..22
//! match Charm's TfxRenderStage schema; stage 23 is modern compute skinning.
use crate::d2_mot::{
    payload::Payload,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
const STAGES: [&str; 24] = [
    "GenerateGbuffer",
    "Decals",
    "InvestmentDecals",
    "ShadowGenerate",
    "LightingApply",
    "LightProbeApply",
    "DecalsAdditive",
    "Transparents",
    "Distortion",
    "LightShaftOcclusion",
    "SkinPrepass",
    "LensFlares",
    "DepthPrepass",
    "WaterReflection",
    "PostprocessTransparentStencil",
    "Impulse",
    "Reticle",
    "WaterRipples",
    "MaskSunLight",
    "Volumetrics",
    "Cubemaps",
    "PostprocessScreen",
    "WorldForces",
    "ComputeSkinning",
];
fn raw(root: &Path, tag: u32) -> Result<Payload> {
    Ok(Payload(fs::read(
        root.join("raw").join(format!("{tag:08X}.bin")),
    )?))
}
fn put(data: &mut [u8], offset: usize, bytes: &[u8]) -> Result<()> {
    data.get_mut(offset..offset + bytes.len())
        .context("native write exceeds payload")?
        .copy_from_slice(bytes);
    Ok(())
}
fn ranges(p: &Payload, offset: usize, n: usize, parts: usize) -> Result<Vec<usize>> {
    let values = (0..n)
        .map(|i| Ok(p.u16(offset + i * 2)? as usize))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        values[0] == 0 && values[n - 1] == parts && values.windows(2).all(|w| w[0] <= w[1]),
        "stage ranges must cover all parts monotonically"
    );
    Ok(values)
}
fn texture_slots(p: &Payload, modern: bool) -> Result<Vec<Value>> {
    let mut slots = vec![];
    for (name, base) in if modern {
        [("vertex", 0x70), ("pixel", 0x2B0)]
    } else {
        [("vertex", 0x48), ("pixel", 0x2C8)]
    } {
        for row in p.array(base + 8, if modern { 24 } else { 8 }, None)? {
            slots.push(json!({"stage":name,"slot":p.u32(row)?,"tag_reference_bytes":hex::encode(if modern{p.0.get(row+8..row+24)}else{p.0.get(row+4..row+8)}.context("texture row")?)}));
        }
    }
    Ok(slots)
}

pub fn map(
    source: &Path,
    native: &Path,
    output: &Path,
    modern: &Payload,
    mesh: usize,
    template: &Value,
) -> Result<Value> {
    map_stages(source, native, output, modern, mesh, template, (false, 20))
}

pub fn map_plated_stride(
    source: &Path,
    native: &Path,
    output: &Path,
    modern: &Payload,
    mesh: usize,
    template: &Value,
    stride: i16,
) -> Result<Value> {
    map_stages(
        source,
        native,
        output,
        modern,
        mesh,
        template,
        (true, stride),
    )
}

/// Does one native mesh carry every draw contract the source mesh retains?
/// Donor selection and the wider package search share this so both apply
/// exactly the same material rules to a candidate.
struct SourceMesh<'a> {
    modern: &'a Payload,
    parts: &'a [usize],
    ranges: &'a [usize],
}

fn mesh_carries(
    source: &SourceMesh,
    model: &Payload,
    m: usize,
    options: (bool, i16),
    source_raw: &mut dyn FnMut(u32) -> Result<Payload>,
    native_raw: &mut dyn FnMut(u32) -> Result<Payload>,
) -> Result<bool> {
    let (plated, stride) = options;
    let (modern, modern_parts, mr) = (source.modern, source.parts, source.ranges);
    let p = native_raw(model.u32(m)?)?;
    let a = native_raw(model.u32(m + 4)?)?;
    if (p.i16(4)?, p.i16(6)?, a.i16(4)?, a.i16(6)?) != (8, 0, stride, 0) {
        return Ok(false);
    }
    // A weapon can list sights and effect attachments before its body.
    // Matching vertex strides alone can select an attachment with no
    // G-buffer draws. Require every stage this conversion retains.
    let parts = model.array(m + 24, 32, Some(0x8080737E))?;
    let stages = ranges(model, m + 40, 24, parts.len())?;
    if (0..23).any(|stage| {
        (!plated || stage == 0 || stage == 3)
            && mr[stage] < mr[stage + 1]
            && stages[stage] == stages[stage + 1]
    }) {
        return Ok(false);
    }
    for stage in 0..23 {
        if plated && stage != 0 && stage != 3 {
            continue;
        }
        if mr[stage] < mr[stage + 1]
            && model.i16(m + 88 + stage * 2)? != if stride == 24 { 139 } else { 137 }
        {
            return Ok(false);
        }
        for &part in &modern_parts[mr[stage]..mr[stage + 1]] {
            let source_mat = source_raw(modern.u32(part)?)?;
            let alpha = modern.u32(part + 24)? & 8;
            let mut matches = BTreeSet::new();
            for &np in &parts[stages[stage]..stages[stage + 1]] {
                let tag = model.u32(np)?;
                let mat = native_raw(tag)?;
                if mat.u32(8)? == source_mat.u32(8)?
                    && (stage == 7 || (model.u16(np + 24)? as u32 & 8) == alpha)
                    && mat.u8(32)? == source_mat.u8(48)?
                {
                    matches.insert(tag);
                }
            }
            if matches.is_empty() || (!plated && matches.len() != 1) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn choose_carrier(
    source: &Path,
    native: &Path,
    modern: &Payload,
    mesh: usize,
    template: &Value,
    options: (bool, i16),
) -> Result<(u32, Payload, usize)> {
    let (_, stride) = options;
    let modern_parts = modern.array(mesh + 32, 36, Some(0x80806ECB))?;
    let mr = ranges(modern, mesh + 48, 25, modern_parts.len())?;
    let mut chosen = None;
    // Carrier models found elsewhere in the packages are rendering templates
    // only. The gameplay and animation donor is unchanged.
    for owner in &carrier_candidates(template) {
        let tag = u32::from_str_radix(owner["model"].as_str().context("native model tag")?, 16)?;
        let model = raw(native, tag)?;
        for m in model.array(16, 0x88, Some(0x80807378))? {
            if mesh_carries(
                &SourceMesh {
                    modern,
                    parts: &modern_parts,
                    ranges: &mr,
                },
                &model,
                m,
                options,
                &mut |t| raw(source, t),
                &mut |t| raw(native, t),
            )? {
                chosen = Some((tag, model, m));
                break;
            }
        }
        if chosen.is_some() {
            break;
        }
    }
    chosen.with_context(|| {
        format!("no native 8/{stride} carrier matches all retained material contracts")
    })
}

/// Find a native model anywhere in the packages whose mesh carries every draw
/// contract this source mesh retains. The donor's own models are searched
/// first; this is the fallback when none of them qualify, and it supplies a
/// rendering template only, leaving the gameplay and animation donor alone.
pub(crate) fn search_carrier(
    reader: &mut Reader,
    source: &Path,
    modern: &Payload,
    mesh: usize,
    options: (bool, i16),
) -> Result<u32> {
    let modern_parts = modern.array(mesh + 32, 36, Some(0x80806ECB))?;
    let mr = ranges(modern, mesh + 48, 25, modern_parts.len())?;
    let view = SourceMesh {
        modern,
        parts: &modern_parts,
        ranges: &mr,
    };
    let mut models = reader.classes(0x808073A5);
    models.sort_unstable();
    let mut cache: BTreeMap<u32, Option<Payload>> = BTreeMap::new();
    for tag in models {
        let Ok(bytes) = reader.manager.read_tag(tiger_pkg::TagHash(tag)) else {
            continue;
        };
        let model = Payload(bytes);
        // Unrelated assets use layouts this converter does not read.
        let Ok(meshes) = model.array(16, 0x88, Some(0x80807378)) else {
            continue;
        };
        for m in meshes {
            let mut native_raw = |t: u32| -> Result<Payload> {
                if let Some(entry) = cache.get(&t) {
                    return entry
                        .clone()
                        .context("native carrier payload is unreadable");
                }
                let read = reader
                    .manager
                    .read_tag(tiger_pkg::TagHash(t))
                    .ok()
                    .map(Payload);
                cache.insert(t, read.clone());
                read.context("native carrier payload is unreadable")
            };
            if mesh_carries(
                &view,
                &model,
                m,
                options,
                &mut |t| raw(source, t),
                &mut native_raw,
            )
            .unwrap_or(false)
            {
                return Ok(tag);
            }
        }
    }
    let (_, stride) = options;
    anyhow::bail!(
        "no native 8/{stride} carrier in the configured packages matches this source model"
    )
}

/// The material contracts a source mesh needs, as a stable key. The answer to
/// a carrier search depends on these and on the native packages, not on which
/// weapon asked, so two weapons with the same requirements share one result.
pub(crate) fn contract_signature(
    source: &Path,
    modern: &Payload,
    mesh: usize,
    options: (bool, i16),
) -> Result<String> {
    let (plated, stride) = options;
    let parts = modern.array(mesh + 32, 36, Some(0x80806ECB))?;
    let mr = ranges(modern, mesh + 48, 25, parts.len())?;
    let mut wanted = BTreeSet::new();
    for stage in 0..23 {
        if plated && stage != 0 && stage != 3 {
            continue;
        }
        for &part in &parts[mr[stage]..mr[stage + 1]] {
            let material = raw(source, modern.u32(part)?)?;
            wanted.insert((
                stage,
                material.u32(8)?,
                material.u8(48)?,
                modern.u32(part + 24)? & 8,
            ));
        }
    }
    // Version 2 also requires each retained stage's actual input layout.
    // Earlier cached carriers were selected from buffer strides alone.
    let mut key = format!("v2/{stride}/{}", u8::from(plated));
    for (stage, class, state, alpha) in wanted {
        key.push_str(&format!(":{stage},{class:X},{state:X},{alpha}"));
    }
    Ok(key)
}

/// The donor model displayed in the native body slot, with its owner and
/// entity. The assembled import is shown in that slot, so its runtime host and
/// preferred material carrier come from the same place rather than from an
/// attachment that happened to be listed first. Templates extracted without
/// placement data give `None`.
pub(crate) fn body_host(template: &Value) -> Option<Value> {
    if template["carrier_order"] == "table" {
        return None;
    }
    let parents = template["parents"].as_array()?;
    let placed = |p: &&Value, selector: Option<u64>| {
        let placement = &p["placement"];
        match selector {
            Some(s) => placement["selector"].as_u64() == Some(s) && placement["position"] == 0,
            None => placement["single"] == 0,
        }
    };
    let body = parents
        .iter()
        .find(|p| placed(p, Some(0)))
        .or_else(|| parents.iter().find(|p| placed(p, None)))?;
    let bytes = hex::decode(body["parent_bytes"].as_str()?).ok()?;
    let entity = u32::from_le_bytes(bytes.get(16..20)?.try_into().ok()?);
    if entity == u32::MAX {
        return None;
    }
    template["models"].as_array()?.iter().find_map(|m| {
        let tag = u32::from_str_radix(m["entity"].as_str()?, 16).ok()?;
        (tag == entity && m["owner"].is_string()).then(|| m.clone())
    })
}

/// The donor model whose owner and entity host searched rendering templates:
/// the body slot's model when it is preferred, otherwise the first donor model
/// with a runtime graph in assets-table order.
pub(crate) fn host_model(template: &Value) -> Option<Value> {
    body_host(template).or_else(|| {
        template["models"].as_array().and_then(|models| {
            models
                .iter()
                .find(|m| m["owner"].is_string() && m["entity"].is_string())
                .cloned()
        })
    })
}

/// Stop preferring the body slot for this template and rehost any searched
/// rendering templates on the table-order donor model. Used when the body
/// owner's channel bank cannot be adapted.
pub(crate) fn use_table_order(template: &mut Value, reason: &str) -> Result<()> {
    template["carrier_order"] = json!("table");
    template["body_host_unavailable"] = json!(reason);
    let host = host_model(template).context("native donor model with a runtime graph")?;
    if let Some(carriers) = template["carrier_models"].as_array_mut() {
        for carrier in carriers {
            carrier["owner"] = host["owner"].clone();
            carrier["entity"] = host["entity"].clone();
        }
    }
    Ok(())
}

/// The host donor model's payload when `carrier` is a searched rendering
/// template rather than one of the donor's own models.
fn host_header(native: &Path, template: &Value, carrier: u32) -> Result<Option<Payload>> {
    let own = template["models"]
        .as_array()
        .context("native models")?
        .iter()
        .any(|m| tag_of(m).ok() == Some(carrier));
    if own {
        return Ok(None);
    }
    let host = host_model(template).context("native donor model with a runtime graph")?;
    let host_tag = tag_of(&host)?;
    ensure!(host_tag != carrier, "rendering template hosts itself");
    Ok(Some(raw(native, host_tag)?))
}

fn tag_of(model: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        model["model"].as_str().context("native model tag")?,
        16,
    )?)
}

/// Native models a carrier may come from: the donor's own models plus any
/// rendering templates searched out of the packages. The body slot's model is
/// tried first because the import replaces the body. Consumers that need the
/// donor's owner or entity must keep using `template["models"]` alone.
pub(crate) fn carrier_candidates(template: &Value) -> Vec<Value> {
    let mut models = template["models"].as_array().cloned().unwrap_or_default();
    if let Some(body) = body_host(template)
        && let Some(at) = models
            .iter()
            .position(|m| m["model"] == body["model"] && m["entity"] == body["entity"])
    {
        let body = models.remove(at);
        models.insert(0, body);
    }
    models.extend(
        template["carrier_models"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    models
}

/// Source models the current template cannot carry, with the mesh each needs.
pub(crate) fn uncarried(
    source: &Path,
    native: &Path,
    report: &Value,
    template: &Value,
    stride: i16,
) -> Result<Vec<(u32, usize)>> {
    let mut missing = vec![];
    for entry in report["models"].as_array().context("source models")? {
        let tag = u32::from_str_radix(entry["model"].as_str().context("source model tag")?, 16)?;
        let model = raw(source, tag)?;
        let meshes = model.array(16, 128, Some(0x80806EC5))?;
        ensure!(meshes.len() == 1, "expected single mesh");
        if choose_carrier(source, native, &model, meshes[0], template, (true, stride)).is_err() {
            missing.push((tag, meshes[0]));
        }
    }
    Ok(missing)
}

/// Read one extracted source model payload by tag.
pub(crate) fn source_model(source: &Path, tag: u32) -> Result<Payload> {
    raw(source, tag)
}

/// Return the same primary render owner that bundle emission will retain.
/// The rig component's owner is a separate tag and cannot supply shader inputs.
pub(crate) fn primary_carrier_owner(
    source: &Path,
    native: &Path,
    report: &Value,
    template: &Value,
    stride: i16,
) -> Result<u32> {
    let entry = report["models"]
        .as_array()
        .context("source models")?
        .first()
        .context("primary source model")?;
    let model = raw(
        source,
        u32::from_str_radix(entry["model"].as_str().context("source model")?, 16)?,
    )?;
    let meshes = model.array(16, 128, Some(0x80806EC5))?;
    ensure!(meshes.len() == 1, "expected single mesh");
    let (tag, _, _) = choose_carrier(source, native, &model, meshes[0], template, (true, stride))?;
    let carriers = carrier_candidates(template);
    let selected = carriers
        .iter()
        .find(|m| {
            m["model"]
                .as_str()
                .and_then(|s| u32::from_str_radix(s, 16).ok())
                == Some(tag)
        })
        .context("selected native carrier")?;
    Ok(u32::from_str_radix(
        selected["owner"].as_str().context("native render owner")?,
        16,
    )?)
}

/// Check all pieces before creating atlases or compiling shaders for this donor.
pub(crate) fn check_carriers(
    source: &Path,
    native: &Path,
    report: &Value,
    template: &Value,
    stride: i16,
) -> Result<()> {
    for model in report["models"].as_array().context("source models")? {
        let tag = model["model"].as_str().context("source model tag")?;
        let model = raw(source, u32::from_str_radix(tag, 16)?)?;
        let meshes = model.array(16, 128, Some(0x80806EC5))?;
        ensure!(meshes.len() == 1, "expected single mesh");
        choose_carrier(source, native, &model, meshes[0], template, (true, stride))
            .with_context(|| format!("source model {tag}"))?;
    }
    Ok(())
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
fn map_stages(
    source: &Path,
    native: &Path,
    output: &Path,
    modern: &Payload,
    mesh: usize,
    template: &Value,
    options: (bool, i16),
) -> Result<Value> {
    let (plated, stride) = options;
    let modern_parts = modern.array(mesh + 32, 36, Some(0x80806ECB))?;
    let mr = ranges(modern, mesh + 48, 25, modern_parts.len())?;
    let (native_tag, native_model, native_mesh) =
        choose_carrier(source, native, modern, mesh, template, options)?;
    let native_parts = native_model.array(native_mesh + 24, 32, Some(0x8080737E))?;
    let nr = ranges(&native_model, native_mesh + 40, 24, native_parts.len())?;
    let mut removed = vec![];
    for &part in &modern_parts[mr[23]..mr[24]] {
        let material_tag = modern.u32(part)?;
        if material_tag != u32::MAX {
            let material = raw(source, material_tag)?;
            ensure!(
                material.u32(8)? == 6,
                "compute-stage entry is not a compute material"
            );
        }
        // Compute removal requires equivalent draw geometry in a native-supported stage.
        let range = (
            modern.u32(part + 8)?,
            modern.u32(part + 12)?,
            modern.u8(part + 29)?,
        );
        ensure!(
            modern_parts[..mr[23]].iter().any(|&p| (
                modern.u32(p + 8).ok(),
                modern.u32(p + 12).ok(),
                modern.u8(p + 29).ok()
            ) == (
                Some(range.0),
                Some(range.1),
                Some(range.2)
            )),
            "compute-only geometry cannot be removed"
        );
        removed.push(json!({"part_offset":part,"material":format!("{:08X}",modern.u32(part)?),"reason":"rigid bone-0 geometry already present in draw stages"}));
    }
    let mut records = vec![];
    let mut stage_reports = vec![];
    let mut assignments = vec![];
    let mut materials = BTreeMap::new();
    let mut relocations = vec![];
    let mut native_ranges = vec![0u16];
    let mut layouts = [-1i16; 23];
    for stage in 0..23 {
        // The plated converter retains only G-buffer and shadow draws. Do not
        // require a material carrier for an effect pass it explicitly omits.
        if plated && stage != 0 && stage != 3 {
            native_ranges.push(u16::try_from(records.len())?);
            if mr[stage] < mr[stage + 1] {
                stage_reports.push(json!({"stage":stage,"name":STAGES[stage],"source_range":[mr[stage],mr[stage+1]],"native_range":[records.len(),records.len()],"source_layout":modern.u8(mesh+98+stage)?,"native_layout":-1}));
            }
            continue;
        }
        if mr[stage] < mr[stage + 1] {
            ensure!(
                nr[stage] < nr[stage + 1],
                "native carrier lacks stage {}",
                STAGES[stage]
            );
            let layout = native_model.i16(native_mesh + 88 + stage * 2)?;
            ensure!(
                layout == if stride == 24 { 139 } else { 137 },
                "unverified native input layout {layout}"
            );
            layouts[stage] = layout;
        }
        for &part in &modern_parts[mr[stage]..mr[stage + 1]] {
            let source_tag = modern.u32(part)?;
            let source_mat = raw(source, source_tag)?;
            let bind = source_mat.u32(8)?;
            let alpha = modern.u32(part + 24)? & 8;
            let mut candidates = vec![];
            let mut seen = BTreeSet::new();
            for &np in &native_parts[nr[stage]..nr[stage + 1]] {
                let tag = native_model.u32(np)?;
                if !seen.insert(tag) {
                    continue;
                }
                let mat = raw(native, tag)?;
                if mat.u32(8)? == bind
                    && (stage == 7 || (native_model.u16(np + 24)? as u32 & 8) == alpha)
                    && mat.u8(32)? == source_mat.u8(48)?
                {
                    candidates.push((tag, np, mat));
                }
            }
            // Plated conversion replaces these placeholders with the audited
            // plated bank after splitting source dye channels into draw groups.
            ensure!(
                !candidates.is_empty() && (plated || candidates.len() == 1),
                "stage {} material {source_tag:08X}: {} compatible carriers",
                STAGES[stage],
                candidates.len()
            );
            let (donor, np, mat) = candidates.remove(0);
            let symbol = format!("material-{source_tag:08X}-stage-{stage}");
            if !materials.contains_key(&symbol) {
                fs::write(output.join(format!("{symbol}.bin")), &mat.0)?;
                let source_slots = texture_slots(&source_mat, true)?;
                let donor_slots = texture_slots(&mat, false)?;
                materials.insert(symbol.clone(),json!({"source_material":format!("{source_tag:08X}"),"native_donor":format!("{donor:08X}"),"payload":format!("{symbol}.bin"),"class":"808071E8","stage":STAGES[stage],"bind_mode":bind,"source_texture_slots":source_slots,"native_texture_slots":donor_slots,"shader_translation":false,"appearance":"native donor approximation","texture_binding_status":"source plates and fixed textures must be bound before installation","native_dependency_policy":"retain donor references in private clone; never mutate stock material"}));
            }
            let mut record = native_model.0[np..np + 32].to_vec();
            // Material and buffers stay null until the private allocator resolves symbols.
            put(&mut record, 0, &u32::MAX.to_le_bytes())?;
            put(&mut record, 4, &modern.bytes::<20>(part + 4)?)?;
            // Use the native donor's flag semantics; dye/LOD fields moved by two bytes.
            put(&mut record, 26, &modern.bytes::<4>(part + 28)?)?;
            let index = records.len();
            records.push(record);
            relocations.push(json!({"offset":0,"part_index":index,"symbol":symbol}));
            assignments.push(json!({"source_part":(part-modern_parts[0])/36,"native_part":index,"stage":STAGES[stage],"source_material":format!("{source_tag:08X}"),"native_donor":format!("{donor:08X}"),"symbol":symbol,"source_flags":modern.u32(part+24)?,"native_flags":native_model.u16(np+24)?,"lod":modern.u8(part+29)?}));
        }
        native_ranges.push(u16::try_from(records.len())?);
        if mr[stage] < mr[stage + 1] {
            stage_reports.push(json!({"stage":stage,"name":STAGES[stage],"source_range":[mr[stage],mr[stage+1]],"native_range":[native_ranges[stage],native_ranges[stage+1]],"source_layout":modern.u8(mesh+98+stage)?,"native_layout":layouts[stage]}));
        }
    }
    // Re-number the per-part draw order after removal of compute records.
    let mut order = assignments
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let p = modern_parts[a["source_part"].as_u64().unwrap() as usize];
            Ok((modern.u16(p + 22)?, i))
        })
        .collect::<Result<Vec<_>>>()?;
    order.sort_unstable();
    for (rank, (_, i)) in order.into_iter().enumerate() {
        put(&mut records[i], 22, &u16::try_from(rank)?.to_le_bytes())?;
    }
    // Native model header, one mesh, then native parts. Relative pointers rebuilt.
    let mesh_offset = 0xB0;
    let part_header = 0x140;
    let part_offset = 0x150;
    let mut payload = vec![0u8; part_offset + records.len() * 32];
    put(&mut payload, part_header - 4, &0x80809FBDu32.to_le_bytes())?;
    put(&mut payload, 0, &native_model.bytes::<160>(0)?)?;
    // A searched rendering template supplies draw records and material shells
    // only. The header block after the bounds describes the model within its
    // owner's skeleton, so it must come from the donor model that hosts the
    // import, not from an unrelated model elsewhere in the packages.
    if let Some(host) = host_header(native, template, native_tag)? {
        put(&mut payload, 0x30, &host.bytes::<32>(0x30)?)?;
    }
    put(
        &mut payload,
        0,
        &(payload_len(records.len()) as u64).to_le_bytes(),
    )?;
    put(&mut payload, 16, &1u64.to_le_bytes())?;
    put(&mut payload, 24, &(0xA0i64 - 24).to_le_bytes())?;
    // Bounds and transforms follow source geometry; preserve native header flags/contracts.
    put(&mut payload, 0x20, &modern.bytes::<16>(0x20)?)?;
    put(&mut payload, 0x50, &modern.bytes::<64>(0x50)?)?;
    put(&mut payload, 0x9C, &0x80809FBDu32.to_le_bytes())?;
    put(&mut payload, 0xA0, &1u64.to_le_bytes())?;
    put(&mut payload, 0xA8, &0x80807378u32.to_le_bytes())?;
    put(
        &mut payload,
        mesh_offset,
        &native_model.bytes::<136>(native_mesh)?,
    )?;
    for (offset, symbol) in [
        (0, "positions-header"),
        (4, "attributes-header"),
        (16, "indices-header"),
    ] {
        put(&mut payload, mesh_offset + offset, &u32::MAX.to_le_bytes())?;
        relocations.push(json!({"offset":mesh_offset+offset,"symbol":symbol}));
    }
    put(&mut payload, mesh_offset + 8, &u32::MAX.to_le_bytes())?;
    put(
        &mut payload,
        mesh_offset + 24,
        &(records.len() as u64).to_le_bytes(),
    )?;
    put(
        &mut payload,
        mesh_offset + 32,
        &((part_header as i64) - (mesh_offset as i64 + 32)).to_le_bytes(),
    )?;
    for (i, &v) in native_ranges.iter().enumerate() {
        put(&mut payload, mesh_offset + 40 + i * 2, &v.to_le_bytes())?
    }
    for (i, &v) in layouts.iter().enumerate() {
        put(&mut payload, mesh_offset + 88 + i * 2, &v.to_le_bytes())?
    }
    put(
        &mut payload,
        part_header,
        &(records.len() as u64).to_le_bytes(),
    )?;
    put(&mut payload, part_header + 8, &0x8080737Eu32.to_le_bytes())?;
    for (i, record) in records.iter().enumerate() {
        put(&mut payload, part_offset + i * 32, record)?;
    }
    for relocation in &mut relocations {
        if let Some(i) = relocation["part_index"].as_u64() {
            relocation["offset"] = json!(part_offset + i as usize * 32);
        }
    }
    let check = Payload(payload.clone());
    let meshes = check.array(16, 136, Some(0x80807378))?;
    ensure!(meshes == [mesh_offset], "native mesh self-check");
    ensure!(
        check.array(mesh_offset + 24, 32, Some(0x8080737E))?.len() == records.len(),
        "native part self-check"
    );
    fs::write(output.join("model.unlinked.bin"), payload)?;
    let report = json!({"model_class":"808073A5","native_carrier":format!("{native_tag:08X}"),"native_mesh":native_mesh,"native_parts":records.len(),"removed_compute_parts":removed,"stages":stage_reports,"parts":assignments,"materials":materials,"relocations":relocations,"installable":false,"mapping_kind":"native carrier substitution; source geometry preserved; shaders not translated","remaining":["source texture and dye bindings","model component and plate conversion","private relocation allocation","in-game appearance verification"]});
    write_json(&output.join("mapping.json"), &report)?;
    Ok(report)
}
fn payload_len(parts: usize) -> usize {
    0x150 + parts * 32
}
#[cfg(test)]
mod tests;
