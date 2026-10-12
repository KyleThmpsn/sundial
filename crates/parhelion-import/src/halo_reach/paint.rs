//! Explicit source paint regions consume the equipped native Sparrow dye channels.
use super::material::Material;
use crate::{
    presentation::{Graph, put},
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaintMask {
    #[default]
    Surface,
    ChangeColor,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Paint {
    /// Native local dye channel, 0 through 2 for body, trim and detail.
    pub channel: u8,
    #[serde(default)]
    pub mask: PaintMask,
}

pub(super) struct Surface {
    pub mask: PaintMask,
    pub slot: u32,
    pub scopes: u32,
    pub channel: u8,
}

#[derive(Default)]
pub(super) struct Prepared {
    pub surfaces: BTreeMap<u32, Surface>,
    pub dyes: Vec<Value>,
    pub rows: Vec<Value>,
}

pub(super) fn prepare(
    native: &mut Reader,
    graph: &mut Graph,
    materials: &BTreeMap<u32, Material>,
    choices: &BTreeMap<String, Paint>,
    item_tag: u32,
    item: u32,
) -> Result<Prepared> {
    if choices.is_empty() {
        return Ok(Prepared::default());
    }
    let context = crate::d2_mot::tfx::context(native, false)?;
    let templates = crate::d2_mot::dyes::inspect(native, item_tag, false)?;
    let mut result = Prepared::default();
    let mut scopes = BTreeMap::new();
    for (name, choice) in choices {
        ensure!(
            choice.channel < 3,
            "Source paint channel exceeds the native gear bank"
        );
        let (&key, material) = materials
            .iter()
            .find(|(_, material)| material.tag.as_ref().is_some_and(|tag| &tag.path == name))
            .with_context(|| format!("Paint material {name} is absent"))?;
        ensure!(
            material.alpha_mode() == "OPAQUE",
            "Paint requires an opaque source surface"
        );
        if matches!(choice.mask, PaintMask::ChangeColor) {
            ensure!(
                material.mapping(&["change_color_map"]).is_some(),
                "Paint mask has no source change-color texture"
            );
        }
        let scope = context["scopes"]
            .as_array()
            .context("Native render scopes")?
            .iter()
            .find(|scope| scope["name"] == format!("gear_dye_{}", choice.channel))
            .context("Native gear dye scope")?;
        let index = scope["index"]
            .as_u64()
            .filter(|index| *index < 32)
            .context("Native dye scope index")?;
        let tag = u32::from_str_radix(scope["tag"].as_str().context("Native dye scope tag")?, 16)?;
        let payload = native.tag(tag, Some(0x808071f3))?;
        ensure!(
            payload.array(0x88, 16, Some(0x80800090))?.len() == 27,
            "Native dye vectors differ"
        );
        let slot = payload.u32(0xb8)?;
        ensure!(
            (1..=9).contains(&slot),
            "Native dye buffer conflicts with model buffers"
        );
        result.surfaces.insert(
            key,
            Surface {
                mask: choice.mask,
                slot,
                scopes: 1 << index,
                channel: choice.channel,
            },
        );
        scopes.insert(choice.channel, (index as u32, slot));
    }
    for (channel, (index, slot)) in scopes {
        let row = templates
            .as_array()
            .context("Native dye templates")?
            .iter()
            .find(|row| row["channel"] == u64::from(channel) + 10)
            .context("Sparrow donor has no matching dye template")?;
        let found = row["found"]
            .as_array()
            .filter(|found| found.len() == 1)
            .context("Native dye template is ambiguous")?;
        let template = &found[0];
        let tag = |name: &str| -> Result<u32> {
            Ok(u32::from_str_radix(
                template[name].as_str().context("Native dye resource")?,
                16,
            )?)
        };
        let symbol = |name: &str| format!("paint-{channel}-{name}");
        let scope_tag = tag("scope")?;
        let mut scope = native.tag(scope_tag, Some(0x808071f3))?.0.clone();
        let source = Payload(scope.clone());
        let vectors = source.array(0x88, 16, Some(0x80800090))?;
        ensure!(vectors.len() == 27, "Native default dye vectors differ");
        let mut constants = vec![0u8; 432];
        for (ordinal, at) in vectors.iter().copied().enumerate() {
            constants[ordinal * 16..ordinal * 16 + 16].copy_from_slice(&source.0[at..at + 16]);
        }
        // Neutral color leaves the source artwork intact until a shader replaces it.
        for row in [9, 13, 17, 21] {
            for lane in 0..4 {
                put(&mut constants, row * 16 + lane * 4, &1f32.to_le_bytes())?;
            }
        }
        for (ordinal, at) in vectors.into_iter().enumerate() {
            put(&mut scope, at, &constants[ordinal * 16..ordinal * 16 + 16])?;
        }
        for descriptor in [0x40, 0x58, 0x68, 0x78] {
            put(&mut scope, descriptor, &[0; 16])?;
        }
        put(&mut scope, 0x10, &index.to_le_bytes())?;
        put(&mut scope, 0xac, &0u32.to_le_bytes())?;
        put(&mut scope, 0xb8, &slot.to_le_bytes())?;
        put(&mut scope, 0xbc, &u32::MAX.to_le_bytes())?;
        let header = tag("buffer_header")?;
        let raw = native.reference(header)?;
        ensure!(
            native.tag(raw, None)?.0.len() == constants.len(),
            "Default dye buffer length differs"
        );
        graph.add(
            &symbol("constants"),
            raw,
            &constants,
            Some(&symbol("buffer")),
            vec![],
        )?;
        graph.add(
            &symbol("buffer"),
            header,
            &native.tag(header, None)?.0,
            Some(&symbol("constants")),
            vec![],
        )?;
        graph.add(
            &symbol("scope"),
            scope_tag,
            &scope,
            None,
            vec![json!({"offset":0xbc,"symbol":symbol("buffer")})],
        )?;
        let definition_tag = tag("dye")?;
        let mut definition = native.tag(definition_tag, None)?.0.clone();
        put(&mut definition, 12, &u32::MAX.to_le_bytes())?;
        graph.add(
            &symbol("definition"),
            definition_tag,
            &definition,
            None,
            vec![json!({"offset":12,"symbol":symbol("scope")})],
        )?;
        let parent = tag("relation")?;
        let mut bytes = native.tag(parent, None)?.0.clone();
        put(&mut bytes, 16, &u32::MAX.to_le_bytes())?;
        graph.add(
            &symbol("parent"),
            parent,
            &bytes,
            None,
            vec![json!({"offset":16,"symbol":symbol("definition")})],
        )?;
        graph.add(
            &symbol("companion"),
            0x81a662de,
            &native.tag(0x81a662de, None)?.0,
            None,
            vec![],
        )?;
        let companion = graph.nodes.last_mut().context("Dye companion")?;
        companion["shared_owner"] = json!(symbol("parent"));
        companion["source_parent"] = json!(parent);
        let manifest = format!("reach-{item:08X}-paint-{channel}")
            .bytes()
            .fold(0x811c9dc5u32, |h, b| {
                h.wrapping_mul(0x01000193) ^ u32::from(b)
            });
        let ordinal = result.dyes.len();
        result
            .dyes
            .push(json!({"channel":channel+10,"manifest":manifest,"parent":symbol("parent")}));
        result
            .rows
            .push(json!({"channel":channel+10,"dye":ordinal}));
    }
    Ok(result)
}
