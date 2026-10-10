//! Checked local dye payloads shared by source authoring and the normal model preview.
//! Missing vectors, malformed scopes and incomplete textures fail rather than borrowing stock dyes.
use super::*;
use crate::model_preview::{Model, shader, texture};
use crate::package_payload::native_array_at;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DyeTextureSource {
    Native(u32),
    Local {
        tag: u32,
        header: std::sync::Arc<[u8]>,
        data: std::sync::Arc<[u8]>,
    },
}

impl DyeTextureSource {
    pub fn tag(&self) -> u32 {
        match self {
            Self::Native(tag) | Self::Local { tag, .. } => *tag,
        }
    }
    fn load(&self, manager: Option<&PackageManager>) -> Result<texture::Texture, String> {
        match self {
            Self::Native(tag) => {
                texture::load(manager.ok_or("Native texture requires packages")?, *tag)
            }
            Self::Local { tag, header, data } => texture::from_payload(*tag, header, data),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DyeSource {
    pub scope: std::sync::Arc<[u8]>,
    pub detail: Option<DyeTextureSource>,
    pub normal: Option<DyeTextureSource>,
}

/// Resolve a source dye through the same native parent and scope records as an installed dye.
pub(crate) fn from_parent(manager: &PackageManager, parent: u32) -> Result<DyeSource, String> {
    let relation = typed(manager, parent, 0x8080_744A)?;
    let dye = typed(manager, word(&relation, 0x10)?, 0x8080_71CD)?;
    let scope = typed(manager, word(&dye, 0x0C)?, 0x8080_71F3)?;
    let (detail, normal) = material::detail_textures(&scope);
    Ok(DyeSource {
        scope: scope.into(),
        detail: detail?.map(DyeTextureSource::Native),
        normal: normal?.map(DyeTextureSource::Native),
    })
}

fn runtime(source: &DyeSource, channels: &[[f32; 4]]) -> Result<material::Material, String> {
    let (count, _, rows, class) = native_array_at(&source.scope, 0x88)?;
    if count != 27 || class != 0x80800090 {
        return Err("Source shader does not contain 27 native material vectors".into());
    }
    let vectors = source
        .scope
        .get(rows..rows + 27 * 16)
        .ok_or("Source material vectors are truncated")?;
    let mut result = material::decode(vectors)?;
    result.animation = material::source_animation(&source.scope, channels, result.vectors);
    Ok(result)
}

/// Static material values and texture thumbnails for the ordinary shader editor.
pub fn decode_source_material(source: &DyeSource) -> Result<DyeMaterial, String> {
    let material = runtime(source, &[])?;
    let pixels = |source: &Option<DyeTextureSource>| -> Result<Option<DyeTexture>, String> {
        source
            .as_ref()
            .map(|t| t.load(None).map(|t| shrink(t, DYE_TEXTURE_EDGE)))
            .transpose()
    };
    Ok(DyeMaterial {
        surfaces: material.surfaces.map(surface_material),
        vectors: material.vectors,
        detail_transform: material.detail_transform,
        normal_transform: material.normal_transform,
        detail_tag: source.detail.as_ref().map(DyeTextureSource::tag),
        normal_tag: source.normal.as_ref().map(DyeTextureSource::tag),
        detail: pixels(&source.detail)?,
        normal: pixels(&source.normal)?,
    })
}

/// Replace the preview's dye slots using local source scopes and their live animation.
pub(crate) fn apply(
    packages: &Path,
    sources: &BTreeMap<usize, DyeSource>,
    model: &mut Model,
    cancel: &crate::model_preview::Load,
) -> Result<(), String> {
    if cancel.stopped() {
        return Err(crate::model_preview::CANCELLED.into());
    }
    let manager = open_shadowkeep_package_manager(packages)?;
    apply_with_manager(&manager, sources, model, cancel)
}

pub(crate) fn apply_with_manager(
    manager: &PackageManager,
    sources: &BTreeMap<usize, DyeSource>,
    model: &mut Model,
    cancel: &crate::model_preview::Load,
) -> Result<(), String> {
    let channels = material::global_channels(manager);
    if !sources.is_empty() && model.iridescence.is_none() {
        model.iridescence = texture::iridescence(manager);
    }
    model
        .dye_animations
        .retain(|(slot, _)| !sources.contains_key(slot));
    for (&slot, source) in sources {
        if cancel.stopped() {
            return Err(crate::model_preview::CANCELLED.into());
        }
        if slot >= 3 {
            return Err("Source dye channel exceeds preview slots".into());
        }
        let material = runtime(source, &channels)?;
        let mut load = |source: &Option<DyeTextureSource>| -> Result<Option<usize>, String> {
            if cancel.stopped() {
                return Err(crate::model_preview::CANCELLED.into());
            }
            let Some(source) = source else {
                return Ok(None);
            };
            if let Some(index) = model.textures.iter().position(|t| t.tag == source.tag()) {
                return Ok(Some(index));
            }
            let texture = source.load(Some(manager))?;
            if model.textures.len() >= crate::model_preview::MAX_TEXTURES {
                return Err("Source textures exceed the preview budget".into());
            }
            model.textures.push(texture);
            Ok(Some(model.textures.len() - 1))
        };
        let detail = load(&source.detail)?;
        let normal = load(&source.normal)?;
        match material.animation {
            Ok(Some(animation)) => model.dye_animations.push((slot, animation)),
            Ok(None) => {}
            Err(error) => model.notices.push(format!("Source animation: {error}")),
        }
        for index in 0..2 {
            model.dyes[slot * 2 + index] = Some(shader::Dye {
                surface: material.surfaces[index],
                detail,
                normal,
                normal_transform: material.normal_transform,
                transform: material.detail_transform,
                vectors: material.vectors,
            });
        }
    }
    Ok(())
}
