//! Shadowkeep gear texture plates (0x808072D2): albedo at 0x24, normal map at 0x28,
//! gearstack at 0x2C. Sampled on 2026-09-21: 0x28 reads as a flat tangent normal
//! (128, 128, 255) and 0x2C as occlusion, smoothness, emission and the dye mask in alpha.
use super::*;

pub(in crate::model_preview) fn albedo(
    manager: &PackageManager,
    component: &[u8],
    model: &mut Model,
) -> Result<Option<usize>, String> {
    plate(manager, component, model, 0x24)
}

pub(in crate::model_preview) fn gearstack(
    manager: &PackageManager,
    component: &[u8],
    model: &mut Model,
) -> Result<Option<usize>, String> {
    plate(manager, component, model, 0x2C)
}

pub(in crate::model_preview) fn normal(
    manager: &PackageManager,
    component: &[u8],
    model: &mut Model,
) -> Result<Option<usize>, String> {
    plate(manager, component, model, 0x28)
}

fn plate(
    manager: &PackageManager,
    component: &[u8],
    model: &mut Model,
    offset: usize,
) -> Result<Option<usize>, String> {
    let data = pointer(component, 0x18)?;
    let tag = u32_at(component, data + 0x248)?;
    if matches!(tag, 0 | u32::MAX) {
        return Ok(None);
    }
    let header = checked(manager, tag, 0x8080_72D2)?;
    let tag = u32_at(&header, offset)?;
    if matches!(tag, 0 | u32::MAX) {
        return Ok(None);
    }
    if let Some(index) = model.textures.iter().position(|t| t.tag == tag) {
        return Ok(Some(index));
    }
    if model.textures.len() >= MAX_TEXTURES {
        return Err("The preview texture budget is full".into());
    }
    let plate = checked(manager, tag, 0x8080_9EBB)?;
    let rects = rectangles(&plate)?;
    let dimensions = [
        rects.iter().map(|r| r.x + r.width).max().unwrap_or(0),
        rects.iter().map(|r| r.y + r.height).max().unwrap_or(0),
    ];
    if dimensions.contains(&0) {
        return Ok(None);
    }
    // Native placement descriptors use a square canvas, including unoccupied space.
    // Explicit material images retain the dimensions from their image headers.
    let side = dimensions[0].max(dimensions[1]).next_power_of_two();
    let dimensions = [side; 2];
    let size = preview_size(dimensions);
    let single = rects.len() == 1;
    if size != dimensions {
        model.notices.push(format!(
            "Plate 0x{tag:08X} uses {} × {} instead of {} × {} to fit the preview memory budget.",
            size[0], size[1], dimensions[0], dimensions[1]
        ));
    }
    let mut texture = Texture {
        tag,
        size,
        rgba: vec![0; size[0] * size[1] * 4],
        linear: None,
        mips: None,
    };
    for rect in rects {
        let mut source = load(manager, rect.tag)?;
        composite(&mut texture, &source, &rect, dimensions, offset == 0x24);
        if single
            && [rect.x, rect.y] == [0, 0]
            && [rect.width, rect.height] == dimensions
            && source.size == size
        {
            texture.mips = source.mips.take();
        }
    }
    if texture.mips.is_none() {
        texture.generate_mips(offset == 0x24);
    }
    retain(model, texture).map(Some)
}

fn converted(pixel: &[u8], color: bool) -> [f32; 4] {
    std::array::from_fn(|i| {
        let value = f32::from(pixel[i]) / 255.0;
        if color && i < 3 {
            shader::linear(value)
        } else {
            value
        }
    })
}

fn composite(
    texture: &mut Texture,
    source: &Texture,
    rect: &Rect,
    dimensions: [usize; 2],
    color: bool,
) {
    if source.linear.is_some() && texture.linear.is_none() {
        texture.linear = Some(
            texture
                .rgba
                .chunks_exact(4)
                .map(|p| converted(p, color))
                .collect(),
        );
    }
    let size = texture.size;
    let x0 = rect.x * size[0] / dimensions[0];
    let y0 = rect.y * size[1] / dimensions[1];
    let x1 = (rect.x + rect.width) * size[0] / dimensions[0];
    let y1 = (rect.y + rect.height) * size[1] / dimensions[1];
    for y in y0..y1 {
        for x in x0..x1 {
            let sx = (x - x0) * source.size[0] / (x1 - x0);
            let sy = (y - y0) * source.size[1] / (y1 - y0);
            let from = (sy * source.size[0] + sx) * 4;
            let to = (y * size[0] + x) * 4;
            texture.rgba[to..to + 4].copy_from_slice(&source.rgba[from..from + 4]);
            if let Some(pixels) = &mut texture.linear {
                pixels[to / 4] = source.linear.as_ref().map_or_else(
                    || converted(&source.rgba[from..from + 4], color),
                    |pixels| pixels[from / 4],
                );
            }
        }
    }
}

struct Rect {
    tag: u32,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}
fn rectangles(bytes: &[u8]) -> Result<Vec<Rect>, String> {
    let (count, rows) = array(bytes, 0x10, 0x8080_9EBD, 20, 4096)?;
    (0..count)
        .map(|index| {
            let row = rows + index * 20;
            let rect = Rect {
                tag: u32_at(bytes, row)?,
                x: u32_at(bytes, row + 4)? as usize,
                y: u32_at(bytes, row + 8)? as usize,
                width: u32_at(bytes, row + 12)? as usize,
                height: u32_at(bytes, row + 16)? as usize,
            };
            if rect.width == 0
                || rect.height == 0
                || rect.x.checked_add(rect.width).is_none_or(|n| n > 8192)
                || rect.y.checked_add(rect.height).is_none_or(|n| n > 8192)
            {
                return Err("Invalid gear texture plate rectangle".into());
            }
            Ok(rect)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plate_rectangles_reject_invalid_dimensions_and_truncation() {
        let mut bytes = vec![0; 0x54];
        bytes[0x10..0x18].copy_from_slice(&1_u64.to_le_bytes());
        bytes[0x18..0x20].copy_from_slice(&0x18_u64.to_le_bytes());
        bytes[0x30..0x38].copy_from_slice(&1_u64.to_le_bytes());
        bytes[0x38..0x3C].copy_from_slice(&0x8080_9EBD_u32.to_le_bytes());
        bytes[0x4C..0x50].copy_from_slice(&1024_u32.to_le_bytes());
        bytes[0x50..0x54].copy_from_slice(&512_u32.to_le_bytes());
        assert_eq!(rectangles(&bytes).unwrap()[0].width, 1024);
        assert!(rectangles(&bytes[..0x53]).is_err());
        bytes[0x44..0x48].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(rectangles(&bytes).is_err());
    }
}
