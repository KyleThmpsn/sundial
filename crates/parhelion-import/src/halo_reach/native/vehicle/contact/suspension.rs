//! Static load balance for the native equal-mass spring consumer.
use anyhow::{Result, ensure};

#[cfg(test)]
mod tests;

/// Calibrate an isolated supported corner about its visible contact height.
/// EB60C0 multiplies damping by base gravity, while the corner's weight follows
/// the selected gravity scale. Its extension slope differs from compression.
pub(super) fn damping(
    height: f32,
    reach: f32,
    rest: f32,
    stiffness: f32,
    load: f32,
    ratio: f32,
    gravity: f32,
) -> Result<(f32, f32)> {
    let slope = if height <= rest {
        stiffness
    } else {
        1. / (reach - rest)
    };
    let coefficient = 2. * ratio * (slope * load * gravity / 12.713999).sqrt();
    ensure!(
        height > 0.
            && reach > height
            && rest > 0.
            && rest < reach
            && stiffness.is_finite()
            && stiffness > 0.
            && load.is_finite()
            && load > 0.
            && ratio.is_finite()
            && ratio > 0.
            && gravity.is_finite()
            && gravity > 0.
            && slope.is_finite()
            && slope > 0.
            && coefficient.is_finite()
            && coefficient > 0.,
        "Invalid native corner damping calibration"
    );
    Ok((slope, coefficient))
}

pub(super) fn loads(positions: &[[f32; 3]], center: [f32; 3]) -> Result<Vec<f32>> {
    let count = positions.len() as f64;
    let mean: [f64; 2] = std::array::from_fn(|axis| {
        positions.iter().map(|p| f64::from(p[axis])).sum::<f64>() / count
    });
    let mut xx = 0.;
    let mut xy = 0.;
    let mut yy = 0.;
    for position in positions {
        let x = f64::from(position[0]) - mean[0];
        let y = f64::from(position[1]) - mean[1];
        xx += x * x;
        xy += x * y;
        yy += y * y;
    }
    let determinant = xx * yy - xy * xy;
    ensure!(
        determinant.is_finite() && determinant > f64::EPSILON * xx * yy,
        "Source wheels do not define a static support area"
    );
    let dx = f64::from(center[0]) - mean[0];
    let dy = f64::from(center[1]) - mean[1];
    let a = (dx * yy - dy * xy) / determinant;
    let b = (dy * xx - dx * xy) / determinant;
    positions
        .iter()
        .map(|position| {
            // EA17D0 passes mass / active wheel count to every spring. These
            // multiples balance total weight and both horizontal moments.
            let load = (1.
                + count
                    * (a * (f64::from(position[0]) - mean[0])
                        + b * (f64::from(position[1]) - mean[1]))) as f32;
            ensure!(
                load.is_finite() && load > 0.,
                "Source center of mass needs an unsupported wheel load distribution"
            );
            Ok(load)
        })
        .collect()
}

pub(super) fn parameters(height: f32, travel: f32, load: f32, maximum: f32) -> Result<(f32, f32)> {
    ensure!(
        load > 0. && load < maximum && height > 0. && travel > 0.,
        "Source wheel load exceeds its static suspension range"
    );
    // EB60C0 uses separate compression and extension slopes. At the visible
    // tire height, request this wheel's balanced share rather than equal load.
    let (rest, stiffness) = if load >= 1. {
        let stiffness = (1. / travel).min((maximum - load) / height);
        (height + (load - 1.) / stiffness, stiffness)
    } else {
        let rest = height + travel - travel / load;
        (rest, (1. / travel).min((maximum - 1.) / rest))
    };
    ensure!(
        rest.is_finite()
            && rest > 0.
            && rest < height + travel
            && stiffness.is_finite()
            && stiffness > 0.,
        "Source wheel travel cannot support its balanced load"
    );
    Ok((rest, stiffness))
}
