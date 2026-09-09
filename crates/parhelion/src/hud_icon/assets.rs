//! Private HUD textures and rows in the native ammunition silhouette bank.
use super::HudImage;
use crate::{
    AuthoringResult, NewTagReference, NewTagReferenceOverride, NewTagSpec, NewTagStorageMode,
    ReplacementSpec,
    appended_tags::AppendedTagAllocator,
    error::invalid,
    tag_payload::{read_u32, read_u64, relative_target, write_u32, write_u64},
};
use std::collections::BTreeMap;
use tiger_pkg::{PackageManager, TagHash};
pub(crate) const TABLE: TagHash = TagHash(0x80EFC07B);
const LAYER: TagHash = TagHash(0x80B47176);
const TEXTURE: TagHash = TagHash(0x80B464EB);
const ROW_SIZE: usize = 112;
fn add_rows(mut table: Vec<u8>, added: &[(u32, TagHash)]) -> AuthoringResult<Vec<u8>> {
    let count =
        usize::try_from(read_u64(&table, 8)?).map_err(|_| invalid("HUD row count overflow"))?;
    let header = relative_target(&table, 16)?;
    if count == 0
        || count > 4096
        || header != 32
        || read_u64(&table, header)? != count as u64
        || read_u32(&table, header - 4)? != 0x80809FBD
        || read_u32(&table, header + 8)? != 0x80804A59
        || table.len() != 48 + count * ROW_SIZE
    {
        return Err(invalid("Unsupported ammunition HUD icon table layout"));
    }
    let mut rows = BTreeMap::new();
    for row in table[48..].chunks_exact(ROW_SIZE) {
        if rows.insert(read_u32(row, 0)?, row.to_vec()).is_some() {
            return Err(invalid("Duplicate native HUD icon key"));
        }
    }
    let template = rows
        .get(&0x08491234)
        .ok_or_else(|| invalid("Missing audited HUD row template"))?
        .clone();
    for &(key, layer) in added {
        if [0, u32::MAX, 0x811C9DC5].contains(&key) || rows.contains_key(&key) {
            return Err(invalid(format!(
                "HUD icon key {key:08X} collides with an existing row"
            )));
        }
        let mut row = template.clone();
        write_u32(&mut row, 0, key)?;
        write_u32(&mut row, 4, layer.0)?;
        rows.insert(key, row);
    }
    table.truncate(48);
    write_u64(&mut table, 8, rows.len() as u64)?;
    write_u64(&mut table, 32, rows.len() as u64)?;
    for row in rows.into_values() {
        table.extend_from_slice(&row);
    }
    let len = table.len() as u64;
    write_u64(&mut table, 0, len)?;
    Ok(table)
}
pub(crate) fn build<'a>(
    manager: &PackageManager,
    images: impl Iterator<Item = (u32, &'a HudImage)>,
    nodes: &mut Vec<NewTagSpec>,
    references: &mut Vec<NewTagReferenceOverride>,
) -> AuthoringResult<Option<ReplacementSpec>> {
    let images: Vec<_> = images.collect();
    if images.is_empty() {
        return Ok(None);
    }
    if manager
        .get_entry(TABLE)
        .is_none_or(|e| e.reference != 0x80804A55)
        || manager
            .get_entry(LAYER)
            .is_none_or(|e| e.reference != 0x80804A69)
    {
        return Err(invalid("Native HUD table/layer schema changed"));
    }
    let table = manager
        .read_tag(TABLE)
        .map_err(|e| invalid(e.to_string()))?;
    let layer = manager
        .read_tag(LAYER)
        .map_err(|e| invalid(e.to_string()))?;
    let header = manager
        .read_tag(TEXTURE)
        .map_err(|e| invalid(e.to_string()))?;
    if layer.len() != 132
        || read_u32(&layer, 0x80)? != TEXTURE.0
        || read_u32(&layer, 0x1C)? != 0x80804A65
        || header.len() != 40
        || read_u32(&header, 4)? != 28
        || header[14..22] != [137, 0, 76, 0, 1, 0, 1, 0]
        || read_u32(&header, 36)? != u32::MAX
    {
        return Err(invalid("Audited HUD texture/layer layout changed"));
    }
    let data = TagHash(
        manager
            .get_entry(TEXTURE)
            .ok_or_else(|| invalid("HUD texture header missing"))?
            .reference,
    );
    if manager
        .get_entry(data)
        .is_none_or(|e| e.file_type != 40 || e.file_subtype != 1)
    {
        return Err(invalid("HUD texture buffer is invalid"));
    }
    let allocator =
        AppendedTagAllocator::new(crate::package_profile::PARHELION_ASSET_PACKAGE_ID, 0);
    let mut rows = vec![];
    for (key, image) in images {
        let ordinal = nodes.len();
        let header_tag = allocator.assigned_tag(ordinal + 1, "HUD header", "HUD icon")?;
        let layer_tag = allocator.assigned_tag(ordinal + 2, "HUD layer", "HUD icon")?;
        let mut private_layer = layer.clone();
        write_u32(&mut private_layer, 0x80, header_tag.0)?;
        nodes.extend([
            NewTagSpec {
                template_tag: data,
                payload: image.rgba().to_vec(),
                storage: NewTagStorageMode::InheritTemplate,
            },
            NewTagSpec {
                template_tag: TEXTURE,
                payload: header.clone(),
                storage: NewTagStorageMode::InheritTemplate,
            },
            NewTagSpec {
                template_tag: LAYER,
                payload: private_layer,
                storage: NewTagStorageMode::InheritTemplate,
            },
        ]);
        references.extend([
            NewTagReferenceOverride {
                new_tag_ordinal: ordinal,
                reference: NewTagReference::Appended(ordinal + 1),
            },
            NewTagReferenceOverride {
                new_tag_ordinal: ordinal + 1,
                reference: NewTagReference::Appended(ordinal),
            },
        ]);
        rows.push((key, layer_tag));
    }
    Ok(Some(ReplacementSpec {
        tag: TABLE,
        payload: add_rows(table, &rows)?,
    }))
}
