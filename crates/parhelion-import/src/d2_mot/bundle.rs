//! Build an unallocated native asset graph for the isolated staging adapter.
use crate::d2_mot::{
    payload::Payload,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};
fn put(p: &mut [u8], o: usize, v: &[u8]) -> Result<()> {
    p.get_mut(o..o + v.len())
        .context("bundle write outside payload")?
        .copy_from_slice(v);
    Ok(())
}
pub(crate) fn add(
    out: &Path,
    nodes: &mut Vec<Value>,
    symbol: &str,
    template: u32,
    data: &[u8],
    reference: Option<&str>,
    patches: Vec<Value>,
) -> Result<()> {
    let file = format!("{symbol}.bin");
    fs::write(out.join(&file), data)?;
    nodes.push(json!({"symbol":symbol,"template":template,"file":file,"reference":reference,"patches":patches}));
    Ok(())
}
fn patch(offset: usize, symbol: &str) -> Value {
    json!({"offset":offset,"symbol":symbol})
}
pub fn build(r: &mut Reader, source: &Path, mapped: &Path) -> Result<Value> {
    build_configured(r, source, mapped, None)
}
pub fn build_configured(
    r: &mut Reader,
    source: &Path,
    mapped: &Path,
    config: Option<&Value>,
) -> Result<Value> {
    let out = r.output.clone();
    let mut nodes = vec![];
    let report: Value = serde_json::from_slice(&fs::read(source.join("report.json"))?)?;
    let provenance: Value =
        serde_json::from_slice(&fs::read(source.join("source-manifest.json"))?)?;
    let mapping: Value = serde_json::from_slice(&fs::read(mapped.join("mapping.json"))?)?;
    let index = config
        .and_then(|c| c["source_model_index"].as_u64())
        .unwrap_or(0) as usize;
    let variant = report["models"]
        .as_array()
        .context("source models")?
        .get(index)
        .context("source model index")?;
    let mut model_tag = 0x80EC2722;
    let mut owner_tag = 0x80EC2727;
    let mut entity_tag = 0x80EC2729;
    let mut parent_tag = 0x80EC272A;
    let mut native_item = 0x02222CBF;
    let mut assignment = 0xCFBE7264;
    if let Some(c) = config {
        let native: Value = serde_json::from_slice(&fs::read(
            Path::new(c["native_template"].as_str().context("native template")?)
                .join("template-report.json"),
        )?)?;
        let candidates = crate::d2_mot::mapping::carrier_candidates(&native);
        let selected = candidates
            .iter()
            .find(|m| m["model"] == mapping["native_carrier"])
            .context("selected native carrier")?;
        let parse = |v: &Value, k: &str| -> Result<u32> {
            Ok(u32::from_str_radix(
                v[k].as_str().context("native tag")?,
                16,
            )?)
        };
        model_tag = parse(selected, "model")?;
        owner_tag = parse(selected, "owner")?;
        entity_tag = parse(selected, "entity")?;
        let parent = native["parents"]
            .as_array()
            .context("parents")?
            .iter()
            .find(|p| {
                hex::decode(p["parent_bytes"].as_str().unwrap_or(""))
                    .ok()
                    .and_then(|b| {
                        b.get(16..20)
                            .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
                    })
                    == Some(entity_tag)
            })
            .context("native parent")?;
        parent_tag = parse(parent, "parent")?;
        assignment = parse(parent, "assignment")?;
        native_item = c["native_item"].as_u64().context("native item")? as u32;
    }
    let native_model = r.tag(model_tag, Some(0x808073A5))?;
    let carrier = *native_model
        .array(16, 136, Some(0x80807378))?
        .iter()
        .find(|&&m| {
            if let Some(selected) = mapping["native_mesh"].as_u64() {
                return selected as usize == m;
            }
            native_model
                .u32(m)
                .ok()
                .and_then(|t| r.tag(t, None).ok())
                .is_some_and(|h| h.i16(4).ok() == Some(8))
        })
        .context("native rigid carrier")?;
    let ph = native_model.u32(carrier)?;
    let ah = native_model.u32(carrier + 4)?;
    let ih = native_model.u32(carrier + 16)?;
    let original_owner = r.tag(owner_tag, Some(0x80809C36))?;
    let resource = original_owner.pointer(24)?;
    ensure!(
        original_owner.u32(resource - 4)? == 0x808072BD,
        "unsupported native model owner"
    );
    let model_slot = resource + 0x1DC;
    let plates_slot = resource + 0x248;
    let plates_tag = original_owner.u32(plates_slot)?;
    let native_plates = r.tag(plates_tag, None)?;
    for (symbol, file, template, reference) in [
        (
            "positions-data",
            "positions.bin",
            r.reference(ph)?,
            Some("positions-header"),
        ),
        (
            "positions-header",
            "positions.header.bin",
            ph,
            Some("positions-data"),
        ),
        (
            "attributes-data",
            "attributes.bin",
            r.reference(ah)?,
            Some("attributes-header"),
        ),
        (
            "attributes-header",
            "attributes.header.bin",
            ah,
            Some("attributes-data"),
        ),
        (
            "indices-data",
            "indices.bin",
            r.reference(ih)?,
            Some("indices-header"),
        ),
        (
            "indices-header",
            "indices.header.bin",
            ih,
            Some("indices-data"),
        ),
    ] {
        add(
            &out,
            &mut nodes,
            symbol,
            template,
            &fs::read(mapped.join(file))?,
            reference,
            vec![],
        )?;
    }
    let mut texture_symbols = vec![];
    for (i, name) in ["albedo", "normal", "gstack"].iter().enumerate() {
        let entries = variant["texture_plates"][name]
            .as_array()
            .context("plate entries")?;
        let composed = crate::d2_mot::plates::source_plate(source, &provenance, entries)?;
        let mut native = r.tag(0x80BA7101, None)?.0.clone();
        let w = composed.side;
        let height = composed.side;
        let data = composed.data;
        put(&mut native, 0, &u32::try_from(data.len())?.to_le_bytes())?;
        put(&mut native, 4, &composed.format.to_le_bytes())?;
        put(&mut native, 14, &u16::try_from(w)?.to_le_bytes())?;
        put(&mut native, 16, &u16::try_from(height)?.to_le_bytes())?;
        put(&mut native, 18, &1u16.to_le_bytes())?;
        put(&mut native, 20, &1u16.to_le_bytes())?;
        let tag = entries[0]["texture"].as_str().context("texture")?;
        let source_header = Payload(fs::read(source.join("raw").join(format!("{tag}.bin")))?);
        native[22] = source_header.u8(44)?;
        native[23] = u8::try_from(composed.mips)?;
        put(&mut native, 36, &u32::MAX.to_le_bytes())?;
        crate::d2_mot::texture::resident(&mut native, data.len())?;
        let hs = format!("texture-{name}-header");
        let ds = format!("texture-{name}-data");
        add(&out, &mut nodes, &ds, 0x80BA7100, &data, Some(&hs), vec![])?;
        add(
            &out,
            &mut nodes,
            &hs,
            0x80BA7101,
            &native,
            Some(&ds),
            vec![],
        )?;
        let original_plate = native_plates.u32(0x24 + i * 4)?;
        let mut plate = r.tag(original_plate, None)?.0.clone();
        put(&mut plate, 0x40, &u32::MAX.to_le_bytes())?;
        put(&mut plate, 0x4C, &(w as u32).to_le_bytes())?;
        put(&mut plate, 0x50, &(height as u32).to_le_bytes())?;
        let ps = format!("plate-{name}");
        add(
            &out,
            &mut nodes,
            &ps,
            original_plate,
            &plate,
            None,
            vec![patch(0x40, &hs)],
        )?;
        texture_symbols.push(ps);
    }
    let mut plates = native_plates.0.clone();
    let mut plate_patches = vec![];
    for (i, s) in texture_symbols.iter().enumerate() {
        put(&mut plates, 0x24 + i * 4, &u32::MAX.to_le_bytes())?;
        plate_patches.push(patch(0x24 + i * 4, s));
    }
    add(
        &out,
        &mut nodes,
        "plates",
        plates_tag,
        &plates,
        None,
        plate_patches,
    )?;
    for (symbol, material) in mapping["materials"]
        .as_object()
        .context("mapped materials")?
    {
        let tag = u32::from_str_radix(
            material["native_donor"]
                .as_str()
                .context("material donor")?,
            16,
        )?;
        let mut payload = Payload(fs::read(
            mapped.join(material["payload"].as_str().context("material payload")?),
        )?);
        let mut fixups = vec![];
        // Audited Hook alpha-clipped G-buffer shader: t3 albedo, t4 normal,
        // t5 gstack. Modern plated materials have no fixed texture rows.
        // Retain the alpha-capable shader, but point it at this weapon's plates.
        if tag == 0x80EC2704
            && material["stage"] == "GenerateGbuffer"
            && material["source_texture_slots"]
                .as_array()
                .context("source texture slots")?
                .is_empty()
        {
            let rows = payload.array(0x2D0, 8, None)?;
            for (slot, symbol) in [
                (3, "texture-albedo-header"),
                (4, "texture-normal-header"),
                (5, "texture-gstack-header"),
            ] {
                let matches = rows
                    .iter()
                    .copied()
                    .filter(|&o| payload.u32(o).ok() == Some(slot))
                    .collect::<Vec<_>>();
                ensure!(
                    matches.len() == 1,
                    "native plated shader slot {slot} missing or ambiguous"
                );
                let offset = matches[0] + 4;
                put(&mut payload.0, offset, &u32::MAX.to_le_bytes())?;
                fixups.push(patch(offset, symbol));
            }
        }
        add(&out, &mut nodes, symbol, tag, &payload.0, None, fixups)?;
    }
    let mut model = fs::read(mapped.join("model.unlinked.bin"))?;
    // Native array serialization requires the marker immediately preceding each header.
    put(&mut model, 0x9C, &0x80809FBDu32.to_le_bytes())?;
    put(&mut model, 0x13C, &0x80809FBDu32.to_le_bytes())?;
    add(
        &out,
        &mut nodes,
        "model",
        model_tag,
        &model,
        None,
        mapping["relocations"]
            .as_array()
            .context("model fixups")?
            .clone(),
    )?;
    let mut owner = original_owner.0.clone();
    let mut owner_patches = vec![patch(model_slot, "model"), patch(plates_slot, "plates")];
    for o in (0..owner.len() - 3).step_by(4) {
        if u32::from_le_bytes(owner[o..o + 4].try_into()?) == owner_tag {
            owner_patches.push(patch(o, "owner"));
        }
    }
    for p in &owner_patches {
        put(
            &mut owner,
            p["offset"].as_u64().context("owner offset")? as usize,
            &u32::MAX.to_le_bytes(),
        )?;
    }
    add(
        &out,
        &mut nodes,
        "owner",
        owner_tag,
        &owner,
        None,
        owner_patches,
    )?;
    let original = r.tag(entity_tag, Some(0x80809C0F))?;
    let slots = crate::d2_mot::entity::owner_slots(&original, &original_owner, owner_tag)?;
    let mut entity = original.0.clone();
    let mut ep = vec![];
    for slot in slots {
        put(&mut entity, slot, &u32::MAX.to_le_bytes())?;
        ep.push(patch(slot, "owner"));
    }
    crate::d2_mot::entity::reject_stale_owner(&Payload(entity.clone()), owner_tag)?;
    add(&out, &mut nodes, "entity", entity_tag, &entity, None, ep)?;
    let mut parent = r.tag(parent_tag, None)?.0.clone();
    put(&mut parent, 16, &u32::MAX.to_le_bytes())?;
    add(
        &out,
        &mut nodes,
        "parent",
        parent_tag,
        &parent,
        None,
        vec![patch(16, "entity")],
    )?;
    let companion = r.tag(0x81A662DE, None)?;
    add(
        &out,
        &mut nodes,
        "parent-companion",
        0x81A662DE,
        &companion.0,
        None,
        vec![],
    )?;
    let c = nodes.last_mut().unwrap();
    c["shared_owner"] = json!("parent");
    c["source_parent"] = json!(parent_tag);
    let result = json!({"item_hash":config.and_then(|c|c["item_hash"].as_u64()).unwrap_or(0x50EE7278),"art_key":config.and_then(|c|c["art_key"].as_u64()).unwrap_or(0xE2507278),"nodes":nodes,"parent":"parent","companion":"parent-companion","source_model":variant["model"],"source_owner":owner_tag,"source_entity":entity_tag,"native_model":model_tag,"native_item":native_item,"native_assignment":assignment,"model_slot":model_slot,"plates_slot":plates_slot,"native_draw_parts":mapping["native_parts"],"appearance":"Source geometry and plated textures; native material carriers","installable":false});
    write_json(&out.join("asset-graph.json"), &result)?;
    Ok(result)
}
