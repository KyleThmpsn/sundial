//! Donor parts an import keeps beside its own geometry.
//!
//! The import replaces the donor's geometry, but some donor parts also carry marker sets the
//! engine reads by part: a sight's aim markers decide where aiming looks, and a modular
//! donor's barrel names the fire points. Emptying those slots leaves the engine without them,
//! which drops the view when aiming. Each marker-bearing donor part other than the host
//! therefore stays in its slot as a private copy that shows nothing, with its markers moved
//! to the matching part of the source weapon.
use crate::d2_mot::{bundle::add, markers, payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::path::Path;

/// The loading index template every private parent clones.
const COMPANION: u32 = 0x81A6_62DE;
/// The first private art key of kept parts. Each import owns sixteen from its ordinal.
pub const KEY_BASE: u32 = 0xE2C0_0000;
/// Kept parts one import may hold.
const MAX_PARTS: u32 = 16;

fn put(data: &mut [u8], at: usize, bytes: &[u8]) -> Result<()> {
    data.get_mut(at..at + bytes.len())
        .context("kept part write outside payload")?
        .copy_from_slice(bytes);
    Ok(())
}

fn patch(offset: usize, symbol: &str) -> Value {
    json!({"offset":offset,"symbol":symbol})
}

fn tag_text(value: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().context("native tag text")?,
        16,
    )?)
}

/// The model header's position scale. Quantized positions decode as the offset at 0x60 plus
/// this scale times the stored value, as the native mesh writer and model preview both read it.
const POSITION_SCALE: usize = 0x6C;

/// A copy of `model` that shows nothing. Its position scale is zero, so every vertex decodes to
/// the offset and no triangle has area. Draws, materials and buffers stay the donor's, because
/// no native model ships without meshes, draw records or indices.
fn collapsed(model: &Payload) -> Result<Vec<u8>> {
    ensure!(
        !model.array(16, 0x88, Some(0x8080_7378))?.is_empty(),
        "kept part model has no meshes"
    );
    ensure!(
        model.f32(POSITION_SCALE)? > 0.0,
        "kept part model has no position scale"
    );
    let mut out = model.0.clone();
    put(&mut out, POSITION_SCALE, &0f32.to_le_bytes())?;
    Ok(out)
}

/// The marker-bearing donor parts other than the one hosting the import.
fn kept(template: &Value, host: u32) -> Result<Vec<&Value>> {
    let parts = template["parents"]
        .as_array()
        .context("donor parents")?
        .iter()
        .filter(|parent| parent["marker_set"] == true)
        .map(|parent| Ok((tag_text(&parent["assignment"])?, parent)))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|(assignment, _)| *assignment != host)
        .map(|(_, parent)| parent)
        .collect::<Vec<_>>();
    ensure!(
        parts.len() <= MAX_PARTS as usize,
        "the donor keeps more marker-bearing parts than one import can hold"
    );
    Ok(parts)
}

/// Add the private nodes of every kept donor part to a finished graph in `out` and describe
/// them in `graph["kept_parts"]`. `ordinal` is the import's art ordinal.
pub fn build(
    reader: &mut Reader,
    source: &Path,
    template: &Value,
    out: &Path,
    graph: &mut Value,
    ordinal: u32,
) -> Result<()> {
    let host = u32::try_from(
        graph["native_assignment"]
            .as_u64()
            .context("host assignment")?,
    )?;
    let parts = kept(template, host)?;
    let mut nodes = Vec::new();
    let mut described = Vec::new();
    for (index, parent) in parts.into_iter().enumerate() {
        let prefix = format!("kept-{index}");
        let symbol = |role: &str| format!("{prefix}-{role}");
        let parent_tag = tag_text(&parent["parent"])?;
        let bytes = hex::decode(parent["parent_bytes"].as_str().context("parent bytes")?)?;
        let entity_tag =
            u32::from_le_bytes(bytes.get(16..20).context("parent entity")?.try_into()?);
        let entity = reader.tag(entity_tag, Some(0x8080_9C0F))?;
        let mut entity_bytes = entity.0.clone();
        let mut entity_patches = Vec::new();
        // The part's own model stays in place but shows nothing.
        if let Some(model) = template["models"]
            .as_array()
            .context("donor models")?
            .iter()
            .find(|model| model["entity"].as_str() == Some(&format!("{entity_tag:08X}")))
        {
            let (model_tag, owner_tag) = (tag_text(&model["model"])?, tag_text(&model["owner"])?);
            let owner = reader.tag(owner_tag, Some(0x8080_9C36))?;
            let resource = owner.pointer(24)?;
            ensure!(
                owner.u32(resource - 4)? == 0x8080_72BD,
                "unsupported kept part model owner"
            );
            let model_slot = resource + 0x1DC;
            ensure!(
                owner.u32(model_slot)? == model_tag,
                "kept part owner names another model"
            );
            let model = reader.tag(model_tag, Some(0x8080_73A5))?;
            add(
                out,
                &mut nodes,
                &symbol("model"),
                model_tag,
                &collapsed(&model)?,
                None,
                vec![],
            )?;
            let mut owner_bytes = owner.0.clone();
            let mut owner_patches = vec![patch(model_slot, &symbol("model"))];
            for at in (0..owner_bytes.len() - 3).step_by(4) {
                if u32::from_le_bytes(owner_bytes[at..at + 4].try_into()?) == owner_tag {
                    owner_patches.push(patch(at, &symbol("owner")));
                }
            }
            for p in &owner_patches {
                put(
                    &mut owner_bytes,
                    p["offset"].as_u64().context("offset")? as usize,
                    &u32::MAX.to_le_bytes(),
                )?;
            }
            add(
                out,
                &mut nodes,
                &symbol("owner"),
                owner_tag,
                &owner_bytes,
                None,
                owner_patches,
            )?;
            for slot in crate::d2_mot::entity::owner_slots(&entity, &owner, owner_tag)? {
                put(&mut entity_bytes, slot, &u32::MAX.to_le_bytes())?;
                entity_patches.push(patch(slot, &symbol("owner")));
            }
        }
        let mut marker_report = Value::Null;
        if let Some((marker_tag, row, payload, plan)) = markers::carry(reader, source, &entity)? {
            put(&mut entity_bytes, row, &u32::MAX.to_le_bytes())?;
            entity_patches.push(patch(row, &symbol("markers")));
            add(
                out,
                &mut nodes,
                &symbol("markers"),
                marker_tag,
                &payload.0,
                None,
                vec![],
            )?;
            marker_report = json!({
                "source": format!("{marker_tag:08X}"),
                "carried": plan.matched.iter().map(|carried| format!("{:08X}", carried.name)).collect::<Vec<_>>(),
                "added": plan.added.iter().map(|added| format!("{:08X}", added.name)).collect::<Vec<_>>(),
            });
        }
        add(
            out,
            &mut nodes,
            &symbol("entity"),
            entity_tag,
            &entity_bytes,
            None,
            entity_patches,
        )?;
        let mut parent_bytes = reader.tag(parent_tag, None)?.0.clone();
        put(&mut parent_bytes, 16, &u32::MAX.to_le_bytes())?;
        add(
            out,
            &mut nodes,
            &symbol("parent"),
            parent_tag,
            &parent_bytes,
            None,
            vec![patch(16, &symbol("entity"))],
        )?;
        let companion = reader.tag(COMPANION, None)?;
        add(
            out,
            &mut nodes,
            &symbol("parent-companion"),
            COMPANION,
            &companion.0,
            None,
            vec![],
        )?;
        let node = nodes.last_mut().context("kept companion")?;
        node["shared_owner"] = json!(symbol("parent"));
        node["source_parent"] = json!(parent_tag);
        let key = KEY_BASE
            .checked_add(
                ordinal
                    .checked_mul(MAX_PARTS)
                    .context("kept key overflow")?,
            )
            .and_then(|base| base.checked_add(u32::try_from(index).ok()?))
            .context("kept key overflow")?;
        described.push(json!({
            "assignment": tag_text(&parent["assignment"])?,
            "key": key,
            "parent": symbol("parent"),
            "placement": parent["placement"],
            "markers": marker_report,
        }));
    }
    graph["nodes"]
        .as_array_mut()
        .context("graph nodes")?
        .extend(nodes);
    graph["kept_parts"] = json!(described);
    Ok(())
}
