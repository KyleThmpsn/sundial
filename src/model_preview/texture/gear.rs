//! Explicit gear images take precedence over the component's implicit donor plates.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DyeMap {
    pub texture: usize,
    pub transform: [u32; 4],
}
impl DyeMap {
    pub fn uv(self, uv: [f32; 2]) -> [f32; 2] {
        let t = self.transform.map(f32::from_bits);
        [uv[0] * t[0] + t[2], uv[1] * t[1] + t[3]]
    }
    pub fn slot(value: [f32; 4]) -> u8 {
        let third = if value[1] - value[2] < 1.2 {
            value[1]
        } else {
            value[2]
        };
        let bank = if third >= 127.5 {
            2
        } else if value[1] >= 127.5 {
            1
        } else {
            0
        };
        bank * 2 + u8::from(value[0] >= 127.5)
    }
}
#[derive(Clone, Copy)]
pub(in crate::model_preview) struct Gear {
    pub albedo: usize,
    pub normal: usize,
    pub mask: usize,
    pub map: Option<DyeMap>,
    /// Compiled raw-vertex UV to the explicitly bound plate, when recovered.
    pub uv: Option<[f32; 4]>,
}
pub(in crate::model_preview) fn gear(
    manager: &PackageManager,
    material: u32,
    model_uv: [f32; 4],
    model: &mut Model,
) -> Result<Option<Gear>, String> {
    let bindings = material_bindings(manager, material).unwrap_or_default();
    let mut tags = [None; 3];
    for &(slot, tag) in &bindings {
        if slot < 3 && tags[slot as usize].replace(tag).is_some() {
            return Err("Gear material binds an image slot twice".into());
        }
    }
    let [Some(albedo), Some(normal), Some(mask)] = tags else {
        return Ok(None);
    };
    let load = |tag, model: &mut Model| -> Result<usize, String> {
        if let Some(index) = model.textures.iter().position(|t| t.tag == tag) {
            return Ok(index);
        }
        if model.textures.len() >= MAX_TEXTURES {
            return Err("The preview texture budget is full".into());
        }
        super::load_model(manager, tag, model)
    };
    let albedo = load(albedo, model)?;
    let normal = load(normal, model)?;
    let mask = load(mask, model)?;
    if model.textures[normal].size != model.textures[albedo].size
        || model.textures[mask].size != model.textures[albedo].size
    {
        return Err("Explicit gear images have different dimensions".into());
    }
    let mut uv = None;
    let bytes = checked(manager, material, 0x8080_71E8)?;
    let map = super::super::effects::native::map_transform(manager, &bytes, model_uv)
        .and_then(|placement| {
            let &(_, tag) = bindings.iter().find(|(slot, _)| *slot == placement.slot)?;
            uv = placement.uv;
            Some(load(tag, model).map(|texture| DyeMap {
                texture,
                transform: placement.transform.map(f32::to_bits),
            }))
        })
        .transpose()?;
    Ok(Some(Gear {
        albedo,
        normal,
        mask,
        map,
        uv,
    }))
}
