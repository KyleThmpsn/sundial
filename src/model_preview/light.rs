//! Native light volume outlines. Layout and placement checked against the installed packages
//! and Alkahest's Pre-Beyond Light map light reader.
use super::*;

pub(super) const LIGHT_COLLECTION: u32 = 0x8080_713A;
pub(super) const SHADOWING_LIGHT: u32 = 0x8080_7140;
const LIGHT_ROW: u32 = 0x8080_713E;
const PLACEMENT_ROW: u32 = 0x8080_9F75;
type Matrix = [[f32; 4]; 4];

pub(crate) struct Info {
    pub tag: u32,
    pub volume_offset: [f32; 3],
    pub radius: f32,
    pub half_fov: f32,
    matrix: Matrix,
}

pub(super) fn info(manager: &PackageManager, tag: u32) -> Result<Info, String> {
    let entry = manager.get_entry(tag).ok_or("Light is missing")?;
    if entry.reference != SHADOWING_LIGHT || entry.file_type != 8 || entry.file_size != 0xD0 {
        return Err("Light has an unsupported native record".into());
    }
    let bytes = manager.read_tag(tag)?;
    parse_shadowing(tag, &bytes)
}

fn float(bytes: &[u8], at: usize) -> Result<f32, String> {
    let value = f32::from_bits(u32_at(bytes, at)?);
    value
        .is_finite()
        .then_some(value)
        .ok_or("Light has a non-finite transform".into())
}

fn matrix(bytes: &[u8], at: usize) -> Result<Matrix, String> {
    let mut result = [[0.0; 4]; 4];
    for (column, values) in result.iter_mut().enumerate() {
        for (lane, value) in values.iter_mut().enumerate() {
            *value = float(bytes, at + column * 16 + lane * 4)?;
        }
    }
    Ok(result)
}

fn parse_shadowing(tag: u32, bytes: &[u8]) -> Result<Info, String> {
    if bytes.len() != 0xD0 {
        return Err("Light has an invalid native size".into());
    }
    let matrix = matrix(bytes, 0x20)?;
    let radius = float(bytes, 0x80)?;
    let half_fov = float(bytes, 0x84)?;
    if !(0.0..=100_000.0).contains(&radius) || radius == 0.0 || !(0.0..=3.2).contains(&half_fov) {
        return Err("Light has an unsupported range or field of view".into());
    }
    Ok(Info {
        tag,
        volume_offset: [matrix[3][0], matrix[3][1], matrix[3][2]],
        radius,
        half_fov,
        matrix,
    })
}

pub(super) fn load(manager: &PackageManager, tag: u32) -> Result<Model, String> {
    let entry = manager.get_entry(tag).ok_or("Light is missing")?;
    let mut model = Model {
        assets: assets::generic(manager, tag)?,
        light_geometry: true,
        ..Default::default()
    };
    match entry.reference {
        SHADOWING_LIGHT => {
            let light = info(manager, tag)?;
            outline_cube(&mut model, light.matrix, [0.0, 0.0, 0.0, 1.0], [0.0; 3])?;
            model.assets.lights.push(light);
        }
        LIGHT_COLLECTION => load_collection(manager, tag, &mut model)?,
        _ => return Err("Unsupported light type".into()),
    }
    model.tags.push(tag);
    if model.triangles.is_empty() {
        return Err("The light contains no visible volume".into());
    }
    Ok(model)
}

pub(super) fn append(manager: &PackageManager, tags: &BTreeSet<u32>, model: &mut Model) {
    for &tag in tags {
        if model.triangles.len() + 48 > MAX_TRIANGLES || model.vertices.len() + 96 > MAX_VERTICES {
            model
                .notices
                .push("Additional light volumes exceed the preview budget".into());
            break;
        }
        let result = info(manager, tag)
            .and_then(|light| outline_cube(model, light.matrix, [0.0, 0.0, 0.0, 1.0], [0.0; 3]));
        match result {
            Ok(()) => {
                model.tags.push(tag);
                model.light_geometry = true;
            }
            Err(error) => model.notices.push(format!("Light 0x{tag:08X}: {error}")),
        }
    }
}

fn load_collection(manager: &PackageManager, tag: u32, model: &mut Model) -> Result<(), String> {
    let entry = manager
        .get_entry(tag)
        .ok_or("Light collection is missing")?;
    if entry.reference != LIGHT_COLLECTION
        || entry.file_type != 8
        || entry.file_size > 2 * 1024 * 1024
    {
        return Err("Light collection has an unsupported native record".into());
    }
    let bytes = manager.read_tag(tag)?;
    if bytes.len() != entry.file_size as usize || u64_at(&bytes, 0)? != bytes.len() as u64 {
        return Err("Light collection has an invalid native size".into());
    }
    let (count, _, light_rows, class) = native_array_at(&bytes, 0x30)?;
    let (placements, _, placement_rows, placement_class) = native_array_at(&bytes, 0x40)?;
    if class != LIGHT_ROW || placement_class != PLACEMENT_ROW || count != placements || count > 512
    {
        return Err("Light collection has an unsupported placement table".into());
    }
    let light_end = light_rows
        .checked_add(count.checked_mul(160).ok_or("Too many lights")?)
        .ok_or("Light row overflow")?;
    let placement_end = placement_rows
        .checked_add(count.checked_mul(32).ok_or("Too many placements")?)
        .ok_or("Light placement overflow")?;
    if light_end > bytes.len() || placement_end > bytes.len() {
        return Err("Light collection rows exceed the record".into());
    }
    for index in 0..count {
        let light = matrix(&bytes, light_rows + index * 160 + 0x20)?;
        let at = placement_rows + index * 32;
        let rotation = [
            float(&bytes, at)?,
            float(&bytes, at + 4)?,
            float(&bytes, at + 8)?,
            float(&bytes, at + 12)?,
        ];
        let translation = [
            float(&bytes, at + 16)?,
            float(&bytes, at + 20)?,
            float(&bytes, at + 24)?,
        ];
        outline_cube(
            model,
            light,
            rotation,
            [translation[0], translation[1], translation[2]],
        )?;
    }
    model.notices.push(format!("{count} native light volumes"));
    Ok(())
}

fn outline_cube(
    model: &mut Model,
    matrix: Matrix,
    rotation: [f32; 4],
    translation: [f32; 3],
) -> Result<(), String> {
    let mut corners = [[0.0; 3]; 8];
    for (index, output) in corners.iter_mut().enumerate() {
        let corner = [
            if index & 1 == 0 { -1.0 } else { 1.0 },
            if index & 2 == 0 { -1.0 } else { 1.0 },
            if index & 4 == 0 { -1.0 } else { 1.0 },
        ];
        let point = project(matrix, corner)?;
        let point = rotate(rotation, point)?;
        *output = std::array::from_fn(|axis| point[axis] + translation[axis]);
    }
    let mut width = 0.0_f32;
    for a in 0..8 {
        for b in a + 1..8 {
            width = width.max(distance(corners[a], corners[b]));
        }
    }
    let width = (width * 0.003).clamp(0.001, 100.0);
    for axis in 0..3 {
        for index in 0..8 {
            if index & (1 << axis) == 0 {
                line(model, corners[index], corners[index | (1 << axis)], width);
            }
        }
    }
    Ok(())
}

fn project(matrix: Matrix, point: [f32; 3]) -> Result<[f32; 3], String> {
    let value: [f32; 4] = std::array::from_fn(|lane| {
        matrix[0][lane] * point[0]
            + matrix[1][lane] * point[1]
            + matrix[2][lane] * point[2]
            + matrix[3][lane]
    });
    if value[3].abs() < 1.0e-5 {
        return Err("Light volume has a singular projection".into());
    }
    let point = std::array::from_fn(|axis| value[axis] / value[3]);
    if point
        .iter()
        .any(|value: &f32| !value.is_finite() || value.abs() > 1.0e7)
    {
        return Err("Light volume exceeds the preview range".into());
    }
    Ok(point)
}

fn rotate(rotation: [f32; 4], point: [f32; 3]) -> Result<[f32; 3], String> {
    let length = rotation.iter().map(|v| v * v).sum::<f32>().sqrt();
    if length < 1.0e-5 {
        return Err("Light placement has an invalid rotation".into());
    }
    let [x, y, z, w] = rotation.map(|v| v / length);
    let [px, py, pz] = point;
    let cross = [y * pz - z * py, z * px - x * pz, x * py - y * px];
    let doubled = cross.map(|v| v * 2.0);
    Ok([
        px + w * doubled[0] + y * doubled[2] - z * doubled[1],
        py + w * doubled[1] + z * doubled[0] - x * doubled[2],
        pz + w * doubled[2] + x * doubled[1] - y * doubled[0],
    ])
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3)
        .map(|axis| (a[axis] - b[axis]).powi(2))
        .sum::<f32>()
        .sqrt()
}

fn line(model: &mut Model, a: [f32; 3], b: [f32; 3], width: f32) {
    let delta = std::array::from_fn::<_, 3, _>(|axis| b[axis] - a[axis]);
    let length = distance(a, b);
    if length < 1.0e-5 {
        return;
    }
    let direction = delta.map(|value| value / length);
    let helper = if direction[2].abs() < 0.8 {
        [0.0, 0.0, 1.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let first = cross(direction, helper);
    let second = cross(direction, first);
    for side in [first, second] {
        let base = model.vertices.len() as u32;
        // Every lane beside the vertices and triangles stays as long as them. A weapon merges
        // its parts lane by lane, so a short lane on a part with a light would shift every
        // later part's dyes, flags and normals.
        model.normals.resize(model.vertices.len(), [0.0; 3]);
        model.vertices.extend([
            std::array::from_fn(|axis| a[axis] - side[axis] * width),
            std::array::from_fn(|axis| a[axis] + side[axis] * width),
            std::array::from_fn(|axis| b[axis] - side[axis] * width),
            std::array::from_fn(|axis| b[axis] + side[axis] * width),
        ]);
        model.uvs.extend([[0.0; 2]; 4]);
        model.weights.extend([None, None, None, None]);
        model.normals.extend([side; 4]);
        for triangle in [[base, base + 1, base + 2], [base + 2, base + 1, base + 3]] {
            let count = model.triangles.len();
            model.triangle_light.resize(count, false);
            model.triangle_emitter.resize(count, false);
            model.triangle_textures.resize(count, None);
            model.triangle_constant.resize(count, None);
            model.triangle_dyes.resize(count, 0);
            model.triangle_clip.resize(count, false);
            model.triangle_gearstacks.resize(count, None);
            model.triangle_normals.resize(count, None);
            model.triangles.push(triangle);
            model.triangle_light.push(true);
            model.triangle_emitter.push(false);
            model.triangle_textures.push(None);
            model.triangle_constant.push(Some([1.0, 0.58, 0.13]));
            model.triangle_dyes.push(0);
            model.triangle_clip.push(false);
            model.triangle_gearstacks.push(None);
            model.triangle_normals.push(None);
        }
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::{Model, outline_cube, parse_shadowing};

    #[test]
    fn shadowing_light_reads_its_native_volume_and_range() {
        let mut bytes = vec![0_u8; 0xD0];
        bytes[0x50..0x54].copy_from_slice(&2.0_f32.to_le_bytes());
        bytes[0x54..0x58].copy_from_slice(&(-3.0_f32).to_le_bytes());
        bytes[0x58..0x5C].copy_from_slice(&4.0_f32.to_le_bytes());
        bytes[0x5C..0x60].copy_from_slice(&1.0_f32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(&7.0_f32.to_le_bytes());
        let light = parse_shadowing(1, &bytes).unwrap();
        assert_eq!(light.volume_offset, [2.0, -3.0, 4.0]);
        assert_eq!(light.radius, 7.0);
        bytes[0x80..0x84].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(parse_shadowing(1, &bytes).is_err());
    }

    #[test]
    fn light_vertices_keep_animation_weights_aligned() {
        let mut model = Model::default();
        let matrix = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        outline_cube(&mut model, matrix, [0.0, 0.0, 0.0, 1.0], [0.0; 3]).unwrap();
        assert_eq!(model.vertices.len(), model.weights.len());
        assert_eq!(model.vertices.len(), model.uvs.len());
        assert_eq!(model.triangles.len(), 48);
    }
}
