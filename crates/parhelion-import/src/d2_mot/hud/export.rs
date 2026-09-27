use crate::d2_mot::{
    icon, localization, profile,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

// The ammunition HUD table, distinct from the 100x100 equipment icon table.
const TABLE: u32 = 0x80A9_E796;

/// Resolve the selected content variant before reading its HUD override. An unset
/// override uses the weapon family icon, as opposed to another variant's artwork.
pub fn export(reader: &mut Reader, report: &Value, rig: &Value) -> Result<Value> {
    let components = rig["components"].as_array().context("HUD rig components")?;
    let owners = components
        .iter()
        .filter(|c| c["entity"] == rig["runtime_entity"] && c["class"] == "80802CF4")
        .collect::<Vec<_>>();
    ensure!(
        owners.len() == 1,
        "source HUD content component is missing or ambiguous"
    );
    let owner_tag = profile::hash(owners[0], "owner")?;
    let owner = reader.tag(owner_tag, Some(0x80809B06))?;
    let resource = owner.pointer(24)?;
    ensure!(
        owner.u32(resource - 4)? == 0x80802CF4,
        "source HUD content layout differs"
    );
    // The empty-name hash is a valid default content selector, not a tag.
    let content = u32::from_str_radix(rig["content_key"].as_str().context("HUD content key")?, 16)?;
    let variants = owner.array(resource + 0x318, 0x2A8, Some(0x8080B773))?;
    let selected = variants
        .iter()
        .copied()
        .filter(|&at| owner.u32(at + 0x10).ok() == Some(content))
        .collect::<Vec<_>>();
    ensure!(
        selected.len() <= 1,
        "source HUD content variant is ambiguous"
    );
    let property = selected.first().copied().unwrap_or(resource + 0x70);
    let override_key = owner.u32(property + 0x150)?;
    let table = reader.tag(TABLE, Some(0x80803EBA))?;
    let rows = table.array(8, 112, Some(0x80803EBE))?;
    let find = |key: u32| -> Result<Option<usize>> {
        let matches = rows
            .iter()
            .copied()
            .filter(|&at| table.u32(at).ok() == Some(key))
            .collect::<Vec<_>>();
        ensure!(
            matches.len() <= 1,
            "source HUD table contains a duplicate key"
        );
        Ok(matches.first().copied())
    };
    let (key, row, selection) = if override_key != 0x811C9DC5 {
        (
            override_key,
            find(override_key)?.context("source HUD override is absent from its table")?,
            "variant_override",
        )
    } else {
        let family = owner.u32(property + 0x30)?;
        if let Some(row) = find(family)? {
            (family, row, "family")
        } else {
            // A runtime family can name its base HUD through a sibling content
            // variant instead of using the animation-family key directly. Only
            // variants of this same kind and family, without their own override,
            // can provide that base. Explicit exotic artwork never becomes the
            // generic silhouette for the rest of the family.
            let kind = owner.u32(property + 0x18)?;
            let mut inherited = std::collections::BTreeMap::new();
            for &variant in &variants {
                if owner.u32(variant + 0x18)? == kind
                    && owner.u32(variant + 0x30)? == family
                    && owner.u32(variant + 0x150)? == 0x811C9DC5
                {
                    let key = owner.u32(variant + 0x10)?;
                    if let Some(row) = find(key)? {
                        inherited.insert(key, row);
                    }
                }
            }
            ensure!(inherited.len() <= 1, "source HUD family base is ambiguous");
            if let Some((key, row)) = inherited.into_iter().next() {
                (key, row, "family_variant")
            } else {
                // Some animation families have no same-named UI entry, such as a
                // distinct fusion-rifle controller. Resolve the source item's type
                // only when the content family supplies no HUD binding.
                let item = u32::try_from(report["item_hash"].as_u64().context("HUD source item")?)?;
                let index =
                    usize::try_from(report["item_index"].as_u64().context("HUD source index")?)?;
                let strings = localization::item_strings(reader, item, index)?;
                let strings = reader.tag(strings, Some(0x8080549F))?;
                let name = localization::Resolver::default().label(reader, &strings, 0x8C)?;
                let name = name.to_ascii_lowercase().replace(' ', "_");
                let key = name.bytes().fold(0x811C9DC5u32, |h, b| {
                    h.wrapping_mul(16_777_619) ^ u32::from(b)
                });
                (
                    key,
                    find(key)?.with_context(|| {
                        format!("source HUD family {name} is absent from its table")
                    })?,
                    "item_type",
                )
            }
        }
    };
    let layer = table.u32(row + 4)?;
    reader.tag(layer, Some(0x80803ECF))?;
    let texture = icon::layer_texture(reader, layer)?.context("source HUD layer is empty")?;
    let image = icon::read_texture(reader, texture)?;
    ensure!(
        matches!(image.format, 28 | 29) && (image.width, image.height) == (137, 76),
        "unsupported source HUD texture layout"
    );
    super::save(&reader.output.join("hud-icon.png"), 137, 76, &image.data)?;
    let result = json!({"table":format!("{TABLE:08X}"),"key":format!("{key:08X}"),"layer":format!("{layer:08X}"),"texture":format!("{texture:08X}"),"content_key":rig["content_key"],"family_fallback":selection != "variant_override","selection":selection,"path":"hud-icon.png","size":[137,76]});
    write_json(&reader.output.join("hud.json"), &result)?;
    Ok(result)
}
