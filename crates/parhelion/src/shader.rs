//! Source shader materials used by the editor, preview and native emission.
use crate::{WeaponDyeReferenceRecipe, WeaponRecipe};
use serde_json::Value;
use std::{collections::BTreeMap, fs};
pub use sundial::package_authoring::{DyeSource, DyeTextureSource};
pub(crate) mod edits;

pub(crate) const SOURCE_DYE: u16 = 0xF000;
pub(crate) fn source_channel(index: u16) -> Option<i8> {
    index
        .checked_sub(SOURCE_DYE)
        .filter(|i| matches!(i, 0..=2 | 4..=15))
        .map(|i| i as i8)
}
pub(crate) fn texture_tag(channel: i8, index: usize) -> u32 {
    0x7F00_0000 + channel as u32 * 2 + index as u32
}
pub(crate) fn texture_symbol(tag: u32) -> Option<String> {
    let offset = tag.checked_sub(0x7F00_0000)?;
    (offset < 32 && matches!(offset / 2, 0..=2 | 4..=15))
        .then(|| format!("dye-{}-texture-{}", offset / 2, offset % 2))
}

pub(crate) fn rows(channels: impl IntoIterator<Item = i8>) -> [Vec<WeaponDyeReferenceRecipe>; 3] {
    let rows = channels
        .into_iter()
        .map(|channel_index| WeaponDyeReferenceRecipe {
            channel_index,
            dye_reference_index: SOURCE_DYE + channel_index as u16,
        })
        .collect::<Vec<_>>();
    [rows.clone(), rows, vec![]]
}

pub(crate) fn source_icon(recipe: &WeaponRecipe) -> Result<crate::icon_edit::ImportedIcon, String> {
    crate::imported::source_icon(recipe)
}

pub(crate) fn normalize_scope(graph: &Value, payload: &mut Vec<u8>) -> Result<(), String> {
    let revision = match graph.get("shader_scope_layout") {
        // Early static bundles retained native scopes without converting them.
        None if !graph["material_programs"].is_array() => return Ok(()),
        None => None,
        Some(value) => Some(
            value
                .as_u64()
                .ok_or("Invalid imported shader scope layout")?,
        ),
    };
    parhelion_import::d2_mot::dye_bundle::normalize_scope(payload, revision)
        .map_err(|e| e.to_string())
}

/// Read the pinned source graph without an installed base shader or package reader.
pub fn source_materials(recipe: &WeaponRecipe) -> Result<BTreeMap<i8, DyeSource>, String> {
    let reference = recipe
        .overrides
        .imported_graph
        .as_ref()
        .ok_or("No imported shader selected")?;
    reference.validate_shader().map_err(|e| e.to_string())?;
    let graph: Value = serde_json::from_slice(
        &fs::read(reference.directory.join("asset-graph.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if graph["kind"] != "shader" {
        return Err("Selected graph is not a shader".into());
    }
    let nodes = graph["nodes"]
        .as_array()
        .ok_or("Source shader nodes missing")?;
    let node = |symbol: &str| {
        nodes
            .iter()
            .find(|n| n["symbol"] == symbol)
            .ok_or_else(|| format!("Missing source asset {symbol}"))
    };
    let payload = |n: &Value| {
        fs::read(
            reference
                .directory
                .join(n["file"].as_str().ok_or("Source asset path missing")?),
        )
        .map_err(|e| e.to_string())
    };
    let mut materials = BTreeMap::new();
    for dye in graph["dyes"]
        .as_array()
        .ok_or("Source shader channels missing")?
    {
        let channel = i8::try_from(dye["channel"].as_i64().ok_or("Source channel missing")?)
            .map_err(|e| e.to_string())?;
        let scope_node = node(&format!("dye-{channel}-scope"))?;
        let mut scope = payload(scope_node)?;
        normalize_scope(&graph, &mut scope)?;
        let mut textures = Vec::new();
        for index in 0..2 {
            let symbol = format!("dye-{channel}-texture-{index}");
            let texture = match nodes.iter().find(|n| n["symbol"] == symbol) {
                None => None,
                Some(header) => Some(DyeTextureSource::Local {
                    tag: texture_tag(channel, index),
                    header: payload(header)?.into(),
                    data: payload(node(
                        header["reference"]
                            .as_str()
                            .ok_or("Source texture data missing")?,
                    )?)?
                    .into(),
                }),
            };
            textures.push(texture);
        }
        materials.insert(
            channel,
            DyeSource {
                scope: scope.into(),
                detail: textures.remove(0),
                normal: textures.remove(0),
            },
        );
    }
    reference.validate_shader().map_err(|e| e.to_string())?;
    Ok(materials)
}
