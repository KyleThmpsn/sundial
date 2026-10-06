use super::*;
use std::collections::BTreeMap;

/// Join the render owner's ordered inputs to ordinary channel-bank initial vectors.
pub(in crate::model_preview) fn inputs(
    owner: Option<&[u8]>,
    components: &[Vec<u8>],
) -> Result<Vec<[f32; 4]>, String> {
    let Some(owner) = owner else {
        return Ok(Vec::new());
    };
    let instance = pointer(owner, 0x10)?;
    let (count, rows) = vertex::table(owner, instance + 0x120, 0x8080_9788, 96, 256)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut defaults = BTreeMap::new();
    for bytes in components {
        let Ok(data) = pointer(bytes, 0x18) else {
            continue;
        };
        if data < 4 || u32_at(bytes, data - 4)? != 0x8080_9790 {
            continue;
        }
        let instance = pointer(bytes, 0x10)?;
        let (vectors, values) = vertex::table(bytes, instance + 0x50, 0x8080_0090, 16, 4096)?;
        let (count, rows) = vertex::table(bytes, data + 0xD8, 0x8080_97A1, 112, 4096)?;
        if count != vectors {
            return Err("The effect channel declarations and cached vectors differ".into());
        }
        for index in 0..count {
            let row = rows + index * 112;
            // Procedural defaults are not equivalent to a zero initialized allocation.
            if u64_at(bytes, row + 8)? != 0
                || bytes[row + 16..row + 72].iter().any(|v| *v != 0)
                || u64_at(bytes, row + 96)? != 0
            {
                continue;
            }
            // +0x48 selects separate numeric storage. Cached float4 values at instance
            // +0x50 follow declaration order, including channels with no numeric slot.
            let value = vector(bytes, values + index * 16)?;
            let name = u32_at(bytes, row)?;
            if defaults.insert(name, value).is_some_and(|old| old != value) {
                return Err("The effect's initial channel value is ambiguous".into());
            }
        }
    }
    (0..count)
        .map(|index| {
            let row = rows + index * 96;
            let link = usize::try_from(u64_at(owner, row + 8)?)
                .map_err(|_| "Invalid effect input link")?;
            if u32_at(owner, row + 4)? != 0x8080_9789
                || u32_at(owner, link + 4)? != 0x8080_9788
                || u64_at(owner, link + 8)? != row as u64
                || u64_at(owner, link + 24)? != 0x8080_97C1
            {
                return Err("The effect input has no supported vector property".into());
            }
            let name = u32_at(owner, link + 32)?;
            defaults
                .get(&name)
                .copied()
                .ok_or_else(|| format!("The initial effect channel {name:08X} is unavailable"))
        })
        .collect()
}

pub(in crate::model_preview) fn load(
    manager: &PackageManager,
    tag: u32,
    objects: Result<&[[f32; 4]], &str>,
    surface: u8,
    model: &mut Model,
) -> Result<Material, String> {
    let bytes = checked(manager, tag, 0x8080_71E8)?;
    if *bytes.get(0x20).ok_or("Truncated effect state")? != 0x88 {
        return Err("The transparent blend state is not decoded".into());
    }
    let pixel = shader(manager, u32_at(&bytes, 0x2C8)?, 0)?;
    let kind = match pixel {
        [
            0x4d,
            0xf8,
            0x1e,
            0x31,
            0x82,
            0x3a,
            0xc6,
            0x8a,
            0x45,
            0xf8,
            0x8d,
            0x48,
            0x57,
            0x13,
            0x23,
            0xaf,
        ] => Kind::Gradient,
        [
            0x21,
            0x9b,
            0xd0,
            0x6a,
            0x73,
            0x37,
            0xb4,
            0x2d,
            0xdb,
            0x32,
            0xa6,
            0xc8,
            0x2a,
            0xaf,
            0xd6,
            0xee,
        ] => Kind::SoftGradient,
        [
            0x43,
            0xb9,
            0xa5,
            0xd8,
            0x59,
            0xde,
            0x7b,
            0x9b,
            0xbc,
            0x01,
            0xf9,
            0x00,
            0xb0,
            0xcd,
            0x2c,
            0x20,
        ] => Kind::GearGlow,
        [
            0x54,
            0x11,
            0x22,
            0xe1,
            0x28,
            0xdb,
            0x6f,
            0x03,
            0x68,
            0x7c,
            0xdb,
            0x15,
            0xcc,
            0x4b,
            0x74,
            0xbd,
        ] => Kind::GearFresnel,
        [
            0x9f,
            0x65,
            0x3a,
            0xc1,
            0x82,
            0xcd,
            0xcc,
            0x73,
            0x95,
            0x97,
            0x1f,
            0xba,
            0x7a,
            0xf8,
            0xf8,
            0x81,
        ] => Kind::ScrollingMasks,
        [
            0xd1,
            0xe0,
            0x0b,
            0x52,
            0x7c,
            0x5a,
            0x68,
            0xb1,
            0x74,
            0xd1,
            0xcf,
            0x17,
            0x53,
            0x4a,
            0xda,
            0xbc,
        ] => Kind::DistortedGlow,
        [
            0xef,
            0xf7,
            0x4e,
            0x0e,
            0x0e,
            0x86,
            0x55,
            0x80,
            0xed,
            0x44,
            0x75,
            0xe2,
            0x79,
            0x63,
            0x96,
            0x9f,
        ] => Kind::WaveGlow,
        _ => {
            return native::load(
                manager,
                tag,
                &bytes,
                objects.map_err(str::to_owned)?,
                surface,
                model,
            );
        }
    };
    let vertex = u32_at(&bytes, 0x48)?;
    if !matches!(vertex, 0 | u32::MAX)
        && !matches!(
            shader(manager, vertex, 1)?,
            [
                0xf9, 0x6a, 0xab, 0x44, 0xf3, 0x4c, 0xa8, 0x51, 0xa0, 0xfc, 0xa2, 0x1e, 0xc8, 0xef,
                0x6e, 0x4f
            ] | [
                0x08, 0x5e, 0xab, 0x2d, 0x86, 0xad, 0xdf, 0x61, 0x7f, 0xb9, 0x8a, 0x7f, 0x4c, 0xe1,
                0xd2, 0x20
            ] | [
                0x8b, 0xc2, 0x1b, 0xba, 0xba, 0xc2, 0x31, 0x84, 0x6c, 0xa0, 0x78, 0xad, 0x52, 0xbc,
                0x57, 0x28
            ] | [
                0xc0, 0xa0, 0x59, 0x7d, 0x4d, 0xc3, 0x8c, 0xb9, 0x86, 0xbc, 0xf7, 0x57, 0x6f, 0x22,
                0xec, 0x2c
            ]
        )
    {
        return Err("The effect vertex program is not decoded".into());
    }
    let constants = constants(manager, &bytes)?;
    let expected = match kind {
        Kind::Gradient => 9,
        Kind::SoftGradient => 10,
        Kind::GearGlow => 8,
        Kind::GearFresnel => 21,
        Kind::ScrollingMasks => 41,
        Kind::DistortedGlow => 13,
        Kind::WaveGlow => 32,
        _ => 0,
    };
    if constants.len() != expected {
        return Err("The effect constants do not match its program".into());
    }
    let globals = crate::dyes::material::global_channels(manager);
    let objects = objects.map_err(str::to_owned)?;
    let program = Program::material(&bytes, 0x2C8, &globals, objects, surface, constants.len())?;
    let mut material = Material {
        kind,
        constants,
        program,
        samplers: texture::material_samplers(manager, tag),
        ..Default::default()
    };
    let bindings = match kind {
        Kind::ScrollingMasks => Some((5, 3, 4)),
        Kind::DistortedGlow | Kind::WaveGlow => Some((3, 2, 1)),
        _ => None,
    };
    if let Some((first, required, samplers)) = bindings {
        let (count, rows) = vertex::table(&bytes, 0x2D0, 0x8080_7211, 8, 32)?;
        for row in (0..count).map(|i| rows + i * 8) {
            let slot = u32_at(&bytes, row)?;
            if !(first..first + required as u32).contains(&slot) {
                continue;
            }
            let tag = u32_at(&bytes, row + 4)?;
            let header = manager.read_tag(tag)?;
            let color = matches!(u32_at(&header, 4)?, 29 | 72 | 75 | 78 | 91 | 93 | 99);
            let index = if let Some(index) = model.textures.iter().position(|t| t.tag == tag) {
                index
            } else {
                if model.textures.len() >= MAX_TEXTURES {
                    return Err("The preview texture budget is full".into());
                }
                texture::load_model(manager, tag, model)?
            };
            material.textures[(slot - first) as usize] = Some(index);
            material.color[(slot - first) as usize] = color;
        }
        if material.textures[..required].iter().any(Option::is_none)
            || material.samplers.len() < samplers
        {
            return Err("The effect is missing a texture or sampler".into());
        }
    }
    material
        .frame(0.0)
        .ok_or("The effect expression cannot initialize")?;
    Ok(material)
}

fn constants(manager: &PackageManager, bytes: &[u8]) -> Result<Vec<[f32; 4]>, String> {
    let values = stage_constants(manager, bytes, 0x2C8)?;
    if values
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || v.abs() > 1e6)
    {
        return Err("Invalid effect constant".into());
    }
    Ok(values)
}

pub(super) fn stage_constants(
    manager: &PackageManager,
    bytes: &[u8],
    stage: usize,
) -> Result<Vec<[f32; 4]>, String> {
    let external = u32_at(bytes, stage + 0x84)?;
    if !matches!(external, 0 | u32::MAX | 0x811C_9DC5) {
        let entry = manager
            .get_entry(external)
            .ok_or("The effect constant buffer is missing")?;
        let data = manager.read_tag(entry.reference)?;
        if data.len() % 16 != 0 || data.len() > 128 * 16 {
            return Err("Invalid effect constant buffer length".into());
        }
        return (0..data.len() / 16)
            .map(|index| shader_vector(&data, index * 16))
            .collect();
    }
    let (count, rows) = vertex::table(bytes, stage + 0x50, 0x8080_0090, 16, 128)?;
    (0..count)
        .map(|index| shader_vector(bytes, rows + index * 16))
        .collect()
}

fn shader_vector(bytes: &[u8], at: usize) -> Result<[f32; 4], String> {
    let mut value = [0.0; 4];
    for (lane, v) in value.iter_mut().enumerate() {
        *v = f32::from_bits(u32_at(bytes, at + lane * 4)?);
    }
    Ok(value)
}

fn vector(bytes: &[u8], at: usize) -> Result<[f32; 4], String> {
    let mut value = [0.0; 4];
    for (lane, v) in value.iter_mut().enumerate() {
        *v = f32::from_bits(u32_at(bytes, at + lane * 4)?);
        if !v.is_finite() || v.abs() > 1e6 {
            return Err("Invalid effect constant".into());
        }
    }
    Ok(value)
}

fn shader(manager: &PackageManager, tag: u32, stage: u32) -> Result<[u8; 16], String> {
    bytes_at(&shader_bytes(manager, tag, stage)?, 4)
}

pub(super) fn shader_bytes(
    manager: &PackageManager,
    tag: u32,
    stage: u32,
) -> Result<Vec<u8>, String> {
    let header = manager
        .get_entry(tag)
        .ok_or("The effect shader is missing")?;
    let data = manager
        .get_entry(header.reference)
        .ok_or("The effect shader payload is missing")?;
    if header.file_type != 33
        || data.file_type != 41
        || data.reference != tag
        || data.file_size > 1024 * 1024
    {
        return Err("Invalid effect shader resources".into());
    }
    let bytes = manager.read_tag(header.reference)?;
    if bytes.get(..4) != Some(b"DXBC") || u32_at(&bytes, 24)? as usize != bytes.len() {
        return Err("Invalid effect shader container".into());
    }
    let count = u32_at(&bytes, 28)? as usize;
    if count > 32 {
        return Err("Too many effect shader chunks".into());
    }
    for index in 0..count {
        let at = u32_at(&bytes, 32 + index * 4)? as usize;
        if matches!(bytes.get(at..at + 4), Some(b"SHEX" | b"SHDR"))
            && u32_at(&bytes, at + 8)? >> 16 == stage
        {
            return Ok(bytes);
        }
    }
    Err("The effect shader stage differs".into())
}
