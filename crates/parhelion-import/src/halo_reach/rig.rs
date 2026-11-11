//! Source-space rig math. Lengths are in metres, matrices are column-major.
use anyhow::{Result, ensure};
use serde::Serialize;

pub const METRES_PER_UNIT: f32 = 3.048;

/// Native recipient-push strength for a source instantaneous acceleration in world units per
/// second, at the import's length scale. A velocity-faithful strength of acceleration times length
/// scale barely moves combatants, since the native physical listener divides the push by the
/// recipient's mass. The scale is a gameplay calibration: the Concussion Rifle's 2.5 at the
/// default length scale gives 20, the strength of Chosen of the Warmind's stronger stock push
/// profile `81A69860`, which throws combatants the way the source blast does.
pub fn push_strength(acceleration: f32, metres: f32) -> f32 {
    const CALIBRATION: f32 = 20. / (2.5 * METRES_PER_UNIT);
    acceleration * metres * CALIBRATION
}

pub const IDENTITY: [f32; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

#[derive(Clone, Debug, Serialize)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub inverse_bind: [f32; 16],
}

pub fn multiply(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    std::array::from_fn(|i| (0..4).map(|k| a[k * 4 + i % 4] * b[i / 4 * 4 + k]).sum())
}

pub fn point(matrix: &[f32; 16], p: [f32; 3], translate: bool) -> [f32; 3] {
    std::array::from_fn(|a| {
        matrix[a] * p[0]
            + matrix[4 + a] * p[1]
            + matrix[8 + a] * p[2]
            + if translate { matrix[12 + a] } else { 0. }
    })
}

pub fn normalize(v: [f32; 3]) -> Result<[f32; 3]> {
    let length = v.iter().map(|v| v * v).sum::<f32>().sqrt();
    ensure!(
        length.is_finite() && length > 0.000001,
        "Zero or nonfinite direction"
    );
    Ok(v.map(|v| v / length))
}

pub fn quaternion(mut q: [f32; 4]) -> Result<[f32; 4]> {
    let length = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    ensure!(
        length.is_finite() && (length - 1.).abs() < 0.02,
        "Invalid source quaternion"
    );
    for v in &mut q {
        *v /= length;
    }
    Ok(q)
}

pub fn transform(translation: [f32; 3], q: [f32; 4]) -> [f32; 16] {
    let [x, y, z, w] = q;
    [
        1. - 2. * (y * y + z * z),
        2. * (x * y + w * z),
        2. * (x * z - w * y),
        0.,
        2. * (x * y - w * z),
        1. - 2. * (x * x + z * z),
        2. * (y * z + w * x),
        0.,
        2. * (x * z + w * y),
        2. * (y * z - w * x),
        1. - 2. * (x * x + y * y),
        0.,
        translation[0],
        translation[1],
        translation[2],
        1.,
    ]
}

pub fn world(bones: &[Bone]) -> Result<Vec<[f32; 16]>> {
    let mut output = Vec::with_capacity(bones.len());
    for (index, bone) in bones.iter().enumerate() {
        let mut result = transform(bone.translation, bone.rotation);
        let mut parent = bone.parent;
        let mut seen = vec![index];
        while let Some(p) = parent {
            ensure!(
                p < bones.len() && !seen.contains(&p),
                "Invalid or cyclic source hierarchy"
            );
            seen.push(p);
            result = multiply(&transform(bones[p].translation, bones[p].rotation), &result);
            parent = bones[p].parent;
        }
        output.push(result);
    }
    Ok(output)
}

pub fn inverse_rigid(matrix: &[f32; 16]) -> [f32; 16] {
    let mut result = IDENTITY;
    for a in 0..3 {
        for b in 0..3 {
            result[a * 4 + b] = matrix[b * 4 + a];
        }
    }
    let p = point(&result, [-matrix[12], -matrix[13], -matrix[14]], false);
    result[12..15].copy_from_slice(&p);
    result
}
