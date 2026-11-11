//! Compose native head regions and equipped armor through the shared model decoder and materials.
use super::*;
use crate::character::{Appearance as Identity, Field};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Character {
    pub appearance: Identity,
    pub armor: Vec<appearance::Appearance>,
    pub portrait: bool,
}

pub(crate) fn load_reported(
    packages: &Path,
    character: &Character,
    cancel: &Load,
) -> Result<Model, String> {
    let stored = cancel.stored();
    let cancel = &stored;
    cancel.say("Opening character packages", 0, 0);
    let manager = crate::investment::discovery::open_packages(packages)?;
    let globals = manager.read_tag(crate::package_runtime::resolve_live_named_tag(
        &manager,
        "investment_globals",
        None,
    )?)?;
    let table = checked(&manager, u32_at(&globals, 0x430)?, 0x8080_5DF5)?;
    let assets = manager.read_tag(crate::package_runtime::resolve_live_named_tag(
        &manager,
        "investment_assets",
        None,
    )?)?;
    let mapping = checked(&manager, u32_at(&assets, 0x20)?, 0x8080_56EA)?;
    let (n, rows) = array(&mapping, 8, 0x8080_56EC, 8, 100_000)?;
    let assignments = (0..n)
        .map(|i| {
            Ok((
                u32_at(&mapping, rows + i * 8)?,
                u32_at(&mapping, rows + i * 8 + 4)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let mut result = Model::default();
    let identity = &character.appearance;
    let face = identity.value(Field::Face)[0];
    let regions = checked(&manager, u32_at(&globals, 16 + 64 * 16)?, 0x8080_57A0)?;
    let (count, rows) = array(&regions, 8, 0x8080_56DC, 4, 256)?;
    let region = |name: &str| -> Result<u32, String> {
        let hash = crate::hash::fnv1_name_hash(name);
        (0..count)
            .find(|&i| u32_at(&regions, rows + i * 4) == Ok(hash))
            .map(|i| i as u32)
            .ok_or_else(|| format!("The installed appearance region {name} is missing"))
    };
    let head = region("head_cinematic")?;
    // Cinematic regions provide the stored geometry and materials used for a close portrait.
    // Gameplay heads can retain jaw geometry that requires the live pose.
    // Resolve the whole matching family rather than mixing its facial layers with gameplay art.
    for (region, selection, colors) in [
        (
            head,
            0,
            [
                identity.value(Field::Skin)[0],
                identity.value(Field::Lips)[0],
                identity.value(Field::Eye)[0],
            ],
        ),
        (
            region("hair_cinematic")?,
            usize::from(identity.value(Field::Hair)[0]),
            identity.value(Field::HairColor),
        ),
        (
            region("facial_hair_cinematic")?,
            usize::from(identity.value(Field::Feature)[0]),
            identity.value(Field::AccentColor),
        ),
        (
            region("facial_decal_cinematic")?,
            usize::from(identity.value(Field::Marking)[0]),
            [identity.value(Field::MarkColor)[0]; 3],
        ),
        (25, 0, [identity.value(Field::Eye)[0]; 3]),
    ] {
        cancel.check()?;
        let hashes = appearance::selected_hashes(
            &table,
            face,
            |r, count| (r == region && count > 0).then_some(selection),
            false,
        )?;
        if region == head && hashes.is_empty() {
            return Err("The selected face has no installed head model".into());
        }
        for hash in hashes {
            let mut part = load_assignment(&manager, &assignments, hash, cancel)?;
            paint(&manager, &colors, &mut part)?;
            appearance::append(&mut result, part, "Head")?;
        }
    }
    if !character.portrait {
        for (index, gear) in character.armor.iter().enumerate() {
            cancel.check()?;
            cancel.say(
                format!("Reading armor {} of {}", index + 1, character.armor.len()),
                index,
                character.armor.len(),
            );
            let hashes = appearance::selected_hashes(
                &table,
                gear.arrangement,
                |region, count| {
                    if count == 0 {
                        None
                    } else if region == u32::MAX {
                        Some(usize::from(identity.gender))
                    } else if matches!(region, 13..=20 | 25 | 26) {
                        None
                    } else {
                        Some(0)
                    }
                },
                false,
            )?;
            let mut part = Model::default();
            for hash in hashes {
                appearance::append(
                    &mut part,
                    load_assignment(&manager, &assignments, hash, cancel)?,
                    "Armor",
                )?;
            }
            if part.triangles.is_empty() {
                return Err("An equipped armor piece has no supported preview geometry".into());
            }
            appearance::apply_colors(&manager, &globals, gear, &mut part)?;
            freeze(&mut part);
            appearance::append(&mut result, part, "Armor")?;
        }
    }
    if result.triangles.is_empty() {
        return Err("The selected character has no supported geometry".into());
    }
    result.iridescence = texture::iridescence(&manager);
    // Some cloth definitions have no skinned display state. Their native solver must
    // initialize the geometry, but the composed portrait/body remains at that stored instant.
    result.animation = None;
    result.rigs.clear();
    result.cloth.clear();
    result.notices.sort();
    result.notices.dedup();
    Ok(result)
}

fn load_assignment(
    manager: &PackageManager,
    assignments: &BTreeMap<u32, u32>,
    hash: u32,
    cancel: &Load,
) -> Result<Model, String> {
    let tag = *assignments
        .get(&hash)
        .ok_or_else(|| format!("Character model assignment {hash:08X} is missing"))?;
    let relation = checked(manager, tag, 0x8080_744A)?;
    let model = load_with_manager(manager, u32_at(&relation, 0x10)?, cancel, None)?;
    if model.triangles.is_empty() {
        return Err(format!(
            "Character model {hash:08X} has no supported geometry"
        ));
    }
    Ok(model)
}

struct Paint {
    material: crate::dyes::material::Material,
    buffer: usize,
    rows: usize,
    textures: BTreeMap<usize, u32>,
}

fn paint(manager: &PackageManager, indices: &[u16; 3], model: &mut Model) -> Result<(), String> {
    let requested = indices
        .iter()
        .copied()
        .filter(|&i| i != u16::MAX)
        .collect::<Vec<_>>();
    let materials = crate::dyes::read_character_dyes(manager, &requested, |constants, scope| {
        let buffer = u32_at(scope, 0xB8)? as usize;
        if !(4..=7).contains(&buffer) || buffer == 4 && constants.len() != 12 * 16 {
            return Err("Unsupported native character material scope".into());
        }
        let mut material = if constants.len() == 12 * 16 {
            // Short head decals bind twelve native vectors. Keep those vectors verbatim in
            // the native program's buffer, with neutral fallback surfaces for basic rendering.
            let mut padded = vec![0; 27 * 16];
            padded[..constants.len()].copy_from_slice(constants);
            let mut value = crate::dyes::material::decode(&padded)?;
            value.surfaces[1] = value.surfaces[0];
            value
        } else if constants.len() == 27 * 16 {
            crate::dyes::material::decode(constants)?
        } else {
            return Err("Unsupported native character dye buffer layout".into());
        };
        (material.detail, material.normal) = crate::dyes::material::detail_textures(scope);
        let mut textures = BTreeMap::new();
        for slot in 0..16 {
            if let Some(tag) = crate::dyes::material::detail_texture(scope, slot)? {
                textures.insert(slot as usize, tag);
            }
        }
        Ok(Paint {
            material,
            buffer,
            rows: constants.len() / 16,
            textures,
        })
    })?;
    let bank = dye_bank(manager, indices, &materials, model)?;
    let mut constants = BTreeMap::new();
    let mut textures = BTreeMap::new();
    for index in indices.iter().filter(|&&i| i != u16::MAX) {
        let paint = materials
            .get(index)
            .ok_or("A character dye was not read")?
            .as_ref()
            .map_err(Clone::clone)?;
        constants.insert(paint.buffer, paint.material.vectors[..paint.rows].to_vec());
        for (&slot, &tag) in &paint.textures {
            let texture = appearance::load_detail(manager, tag, model)
                .ok_or("A required character scope texture could not be loaded")?;
            let header = manager.read_tag(tag)?;
            let color = matches!(u32_at(&header, 4)?, 29 | 72 | 75 | 78 | 91 | 93 | 99);
            textures.insert((paint.buffer, slot), (texture, color));
        }
    }
    for effect in &mut model.effects {
        if let Some(native) = effect.native.as_mut() {
            native.bind_scopes(&constants, &textures);
        }
    }
    // Native surface numbers are material inputs, not palette selectors. The owning art region
    // supplies its scopes. Keep local primary/secondary parity for the basic surface fallback.
    model.triangle_dye_banks = model
        .triangle_dyes
        .iter_mut()
        .map(|surface| {
            if *surface >= 12 {
                return None;
            }
            *surface %= 6;
            Some(0)
        })
        .collect();
    model.dye_banks = vec![bank];
    model.dyes = [None; 6];
    model.dye_animations.clear();
    Ok(())
}

fn dye_bank(
    manager: &PackageManager,
    indices: &[u16; 3],
    materials: &BTreeMap<u16, Result<Paint, String>>,
    model: &mut Model,
) -> Result<[Option<shader::Dye>; 6], String> {
    let mut dyes = [None; 6];
    for (channel, index) in indices.iter().enumerate() {
        if *index == u16::MAX {
            continue;
        }
        let paint = materials
            .get(index)
            .ok_or("A character dye was not read")?
            .as_ref()
            .map_err(Clone::clone)?;
        let material = &paint.material;
        let mut detail = |value: &Result<Option<u32>, String>| -> Result<Option<usize>, String> {
            value
                .as_ref()
                .map_err(Clone::clone)?
                .map(|tag| {
                    appearance::load_detail(manager, tag, model)
                        .ok_or("A required character dye texture could not be loaded".into())
                })
                .transpose()
        };
        let diffuse = detail(&material.detail)?;
        let normal = detail(&material.normal)?;
        for side in 0..2 {
            dyes[channel * 2 + side] = Some(shader::Dye {
                surface: material.surfaces[side],
                detail: diffuse,
                normal,
                transform: material.detail_transform,
                normal_transform: material.normal_transform,
                vectors: material.vectors,
            });
        }
    }
    Ok(dyes)
}

fn freeze(model: &mut Model) {
    let dyes = shader::dyes(model, 0.0);
    let bank = model.dye_banks.len();
    model.dye_banks.push(dyes);
    model.triangle_dye_banks = vec![Some(bank); model.triangles.len()];
    model.dyes = [None; 6];
    model.dye_animations.clear();
}
