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
    let dimension = rects
        .iter()
        .map(|r| (r.x + r.width).max(r.y + r.height))
        .max()
        .unwrap_or(0);
    if dimension == 0 {
        return Ok(None);
    }
    let dimension = dimension.next_power_of_two();
    let size = dimension.min(2048);
    let mut rgba = vec![0; size * size * 4];
    for rect in rects {
        let source = load(manager, rect.tag)?;
        let x0 = rect.x * size / dimension;
        let y0 = rect.y * size / dimension;
        let x1 = (rect.x + rect.width) * size / dimension;
        let y1 = (rect.y + rect.height) * size / dimension;
        for y in y0..y1 {
            for x in x0..x1 {
                let sx = (x - x0) * source.size[0] / (x1 - x0);
                let sy = (y - y0) * source.size[1] / (y1 - y0);
                let from = (sy * source.size[0] + sx) * 4;
                let to = (y * size + x) * 4;
                rgba[to..to + 4].copy_from_slice(&source.rgba[from..from + 4]);
            }
        }
    }
    model.textures.push(Texture {
        tag,
        size: [size, size],
        rgba,
    });
    Ok(Some(model.textures.len() - 1))
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
