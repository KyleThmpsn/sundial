//! The source sphere spring on the native equal-share spring consumer.
use anyhow::{Result, ensure};

/// Native base gravity in metres per second squared. `00EB60C0` normalizes compression stiffness
/// and both damping coefficients against it.
const BASE_GRAVITY: f32 = 12.713999;

/// One contact's native ray length, loaded rest distance and compression stiffness per metre.
pub(super) struct Spring {
    pub reach: f32,
    pub rest: f32,
    pub stiffness: f32,
}

/// Reach rests each friction point on a sphere of the point's radius about its marker. MCC U13
/// 12E910 queries that sphere and 233944 takes the penetration as the radius less the distance
/// from the sphere's centre to the surface. The point then pushes with
/// `body mass * gravity * penetration / ground depth`, so each point alone carries the whole
/// weight at a penetration of the ground depth and nothing once its sphere leaves the ground.
///
/// The native consumer `00EB60C0` scales an equal share of the weight, one part in `count`. Below
/// the rest distance the multiple is `1 + stiffness * (rest - distance)`, and above it the multiple
/// falls linearly from one to zero at the ray length. A ray as long as the radius, a rest distance
/// `depth / count` short of it and a stiffness of `count / depth` make both branches the one source
/// line, `count * (radius - distance) / depth` shares. Every contact takes the same spring, so the
/// body settles on its own load distribution as it does in Reach.
pub(super) fn spring(radius: f32, depth: f32, count: usize) -> Result<Spring> {
    let count = count as f32;
    let rest = radius - depth / count;
    ensure!(
        radius.is_finite() && depth.is_finite() && depth > 0. && count > 0. && rest > 0.,
        "Source wheel sphere is smaller than its share of the ground depth (radius {radius}, depth {depth})"
    );
    Ok(Spring {
        reach: radius,
        rest,
        stiffness: count / depth,
    })
}

/// Source 233944 subtracts `ground damping * normal velocity` from the spring term before the
/// body mass multiplies both, so each point damps with the whole mass. A native coefficient
/// multiplies an equal share of the mass and base gravity, whatever gravity scale is selected.
pub(super) fn damping(source: f32, count: usize) -> Result<f32> {
    let coefficient = source * count as f32 / BASE_GRAVITY;
    ensure!(
        coefficient.is_finite() && coefficient >= 0.,
        "Invalid source ground damping"
    );
    Ok(coefficient)
}

/// The damping ratio of the whole body's vertical motion on `count` equal source springs, for
/// the report. The springs sum to `count * gravity / depth` per unit mass and the dampers to
/// `count * source` per unit mass.
pub(super) fn heave_ratio(source: f32, depth: f32, count: usize, gravity: f32) -> f32 {
    let count = count as f32;
    count * source / (2. * (count * BASE_GRAVITY * gravity / depth).sqrt())
}

/// A tracked contact's rest distance and stiffness, which hold its balanced share of the weight at
/// the visible tread height inside the source ground depth. A tracked source hangs its treads from
/// several bodies and holds contacts on its centre line that the carrier's two tracks do not take,
/// so the source sphere spring's mass and contact count are not established for it. Its contacts
/// keep this support, which the carrier's tread drive was checked against.
pub(super) fn tracked(height: f32, travel: f32, load: f32, maximum: f32) -> Result<(f32, f32)> {
    ensure!(
        load > 0. && load < maximum && height > 0. && travel > 0.,
        "Source tread load exceeds its static suspension range"
    );
    // EB60C0 uses separate compression and extension slopes. At the visible
    // tread height, request this contact's balanced share rather than equal load.
    let (rest, stiffness) = if load >= 1. {
        let bounded = (1. / travel).min((maximum - load) / height);
        let stiffness = bounded.max((load - 1.) / (travel * 0.999));
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
        "Source tread travel cannot support its balanced load (height {height}, travel {travel}, load {load}, maximum {maximum}, rest {rest}, stiffness {stiffness})"
    );
    Ok((rest, stiffness))
}

/// A tracked contact's damper, for an isolated supported corner at `ratio` of critical about its
/// visible contact height. EB60C0 multiplies damping by base gravity, while the corner's weight
/// follows the selected gravity scale. Its extension slope differs from compression.
pub(super) fn tracked_damping(
    height: f32,
    reach: f32,
    rest: f32,
    stiffness: f32,
    load: f32,
    ratio: f32,
    gravity: f32,
) -> Result<f32> {
    let slope = if height <= rest {
        stiffness
    } else {
        1. / (reach - rest)
    };
    let coefficient = 2. * ratio * (slope * load * gravity / BASE_GRAVITY).sqrt();
    ensure!(
        reach > height && rest < reach && ratio > 0. && coefficient.is_finite() && coefficient > 0.,
        "Invalid native tread damping calibration"
    );
    Ok(coefficient)
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
            // Equal springs settle on a plane, so each contact's share of the weight is linear
            // in its position. EA17D0 passes mass / active contact count to every spring, and
            // these multiples of that share balance total weight and both horizontal moments.
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
