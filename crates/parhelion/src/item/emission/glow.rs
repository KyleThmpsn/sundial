//! Private native material and pixel resources for an authored weapon's glow option.
use super::*;
use crate::tag_payload::array_at;
use crate::weapon::glow::Change;
use linking::Node;

pub(super) fn model(
    manager: &sundial::package_authoring::PackageManager,
    prefix: &str,
    model: &mut Node,
    nodes: &mut Vec<Node>,
) -> AuthoringResult<()> {
    let (meshes, _, start, class) = array_at(&model.payload, 0x10)?;
    if class != 0x80807378 {
        return Err(invalid("Shader Glow model mesh layout differs"));
    }
    let mut parts = Vec::new();
    for mesh in 0..meshes {
        let (count, _, rows, class) = array_at(&model.payload, start + mesh * 136 + 24)?;
        if count != 0 && class != 0x8080737E {
            return Err(invalid("Shader Glow model part layout differs"));
        }
        // Stage zero owns opaque deferred gear shading. Decals, transparent effects,
        // depth and shadow passes retain their original programs and states.
        let first = usize::from(crate::tag_payload::read_u16(
            &model.payload,
            start + mesh * 136 + 40,
        )?);
        let end = usize::from(crate::tag_payload::read_u16(
            &model.payload,
            start + mesh * 136 + 42,
        )?);
        if first > end || end > count {
            return Err(invalid("Shader Glow model stage range differs"));
        }
        for i in first..end {
            parts.push(rows + i * 32);
        }
    }
    let mut materials = BTreeMap::<u32, Option<String>>::new();
    for offset in parts {
        let tag = read_u32(&model.payload, offset)?;
        if [0, u32::MAX].contains(&tag) {
            continue;
        }
        let symbol = if let Some(symbol) = materials.get(&tag) {
            symbol.clone()
        } else {
            let symbol = material(manager, &format!("{prefix}-glow-{tag:08X}"), tag, nodes)?;
            materials.insert(tag, symbol.clone());
            symbol
        };
        if let Some(symbol) = symbol {
            model.patch(offset, symbol)?;
        }
    }
    Ok(())
}

fn material(
    manager: &sundial::package_authoring::PackageManager,
    prefix: &str,
    tag: u32,
    nodes: &mut Vec<Node>,
) -> AuthoringResult<Option<String>> {
    let read = |tag: u32| {
        manager
            .read_tag(TagHash(tag))
            .map_err(|e| invalid(e.to_string()))
    };
    let entry = manager
        .get_entry(TagHash(tag))
        .ok_or_else(|| invalid("Shader Glow material is missing"))?;
    if entry.reference != 0x808071E8 {
        return Err(invalid("Shader Glow material layout differs"));
    }
    let material = read(tag)?;
    // Separate effects and non-dye passes keep their own rendering behavior.
    if read_u32(&material, 24)? & 0x1C00_0000 == 0 {
        return Ok(None);
    }
    let pixel = read_u32(&material, 0x2C8)?;
    if [0, u32::MAX].contains(&pixel) {
        return Ok(None);
    }
    let header_entry = manager
        .get_entry(TagHash(pixel))
        .filter(|e| e.file_type == 33 && e.file_subtype == 0)
        .ok_or_else(|| invalid("Shader Glow pixel resource type differs"))?;
    let data = header_entry.reference;
    manager
        .get_entry(TagHash(data))
        .filter(|e| e.file_type == 41 && e.file_subtype == 0 && e.reference == pixel)
        .ok_or_else(|| invalid("Shader Glow pixel resources are not reciprocal"))?;
    let header = read(pixel)?;
    let code = read(data)?;
    if header.len() != 40
        || crate::tag_payload::read_u64(&header, 0)? != 40
        || read_u32(&header, 8)? as usize != code.len()
        || read_u32(&header, 12)? != u32::MAX
        || header[16..].iter().any(|byte| *byte != 0)
    {
        return Err(invalid("Shader Glow pixel allocation header differs"));
    }
    let code = match crate::weapon::glow::patch(&code)
        .map_err(|e| e.context(format!("Material 0x{tag:08X}, pixel 0x{pixel:08X}")))?
    {
        Change::NotDyeMaterial | Change::AlreadySupported => return Ok(None),
        Change::Patched(code) => code,
    };
    let material_symbol = format!("{prefix}-material");
    let pixel_symbol = format!("{prefix}-pixel");
    let data_symbol = format!("{prefix}-bytecode");
    let mut material_node = Node::new(&material_symbol, tag, material);
    material_node.patch(0x2C8, &pixel_symbol)?;
    let mut header_node = Node::new(&pixel_symbol, pixel, header);
    write_u32(
        &mut header_node.payload,
        8,
        u32::try_from(code.len()).map_err(|_| invalid("Glow pixel program is too large"))?,
    )?;
    header_node.reference = Some(data_symbol.clone());
    let mut data_node = Node::new(&data_symbol, data, code);
    data_node.reference = Some(pixel_symbol);
    nodes.extend([material_node, header_node, data_node]);
    Ok(Some(material_symbol))
}
