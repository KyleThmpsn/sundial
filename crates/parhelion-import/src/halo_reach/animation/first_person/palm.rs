//! Anatomical wrist axes avoid carrying the donor's grip into every source action.
use crate::{halo_reach::rig::normalize, tiger::animation::pose::Pose};
use anyhow::Result;

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Rotation from an anatomical palm frame into the wrist's local coordinates.
pub(super) fn frame(wrist: Pose, index: Pose, middle: Pose, pinky: Pose) -> Result<Pose> {
    let local = wrist.inverse();
    let index = local.then(index).0;
    let middle = local.then(middle).0;
    let pinky = local.then(pinky).0;
    let x = normalize([middle[4], middle[5], middle[6]])?;
    let across = std::array::from_fn(|axis| index[axis + 4] - pinky[axis + 4]);
    let z = normalize(cross(x, across))?;
    let y = normalize(cross(z, x))?;
    axes(x, y, z)
}

pub(in crate::halo_reach::animation) fn axes(
    x: [f32; 3],
    y: [f32; 3],
    z: [f32; 3],
) -> Result<Pose> {
    // Matrix columns are the anatomical axes. Select the largest quaternion
    // component to avoid division near zero at a half turn.
    let candidates = [
        1. + x[0] - y[1] - z[2],
        1. - x[0] + y[1] - z[2],
        1. - x[0] - y[1] + z[2],
        1. + x[0] + y[1] + z[2],
    ];
    let component = (0..4)
        .max_by(|&a, &b| candidates[a].total_cmp(&candidates[b]))
        .unwrap();
    let largest = candidates[component].max(0.).sqrt() * 0.5;
    let divisor = 4. * largest;
    let q = match component {
        0 => [
            largest,
            (y[0] + x[1]) / divisor,
            (z[0] + x[2]) / divisor,
            (y[2] - z[1]) / divisor,
        ],
        1 => [
            (y[0] + x[1]) / divisor,
            largest,
            (z[1] + y[2]) / divisor,
            (z[0] - x[2]) / divisor,
        ],
        2 => [
            (z[0] + x[2]) / divisor,
            (z[1] + y[2]) / divisor,
            largest,
            (x[1] - y[0]) / divisor,
        ],
        _ => [
            (y[2] - z[1]) / divisor,
            (z[0] - x[2]) / divisor,
            (x[1] - y[0]) / divisor,
            largest,
        ],
    };
    Pose::checked([q[0], q[1], q[2], q[3], 0., 0., 0., 1.])
}
