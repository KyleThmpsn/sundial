//! Vector and quaternion arithmetic the importers share. A vector is `[x, y, z]` and a quaternion
//! is `[x, y, z, w]`.

pub(crate) fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|k| a[k] - b[k])
}

pub(crate) fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| a[k] * b[k]).sum()
}

pub(crate) fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The quaternion product `a * b`, the rotation `b` followed by `a`.
pub(crate) fn product(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [x, y, z, w] = a;
    let [i, j, k, r] = b;
    [
        w * i + x * r + y * k - z * j,
        w * j - x * k + y * r + z * i,
        w * k + x * j - y * i + z * r,
        w * r - x * i - y * j - z * k,
    ]
}

/// `v` turned by the unit quaternion `q`.
pub(crate) fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    let t = [
        2. * (y * v[2] - z * v[1]),
        2. * (z * v[0] - x * v[2]),
        2. * (x * v[1] - y * v[0]),
    ];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

/// The axis and the angle in radians of the rotation `q`, taken the short way round. A rotation
/// too small to have an axis gives a zero axis and angle.
pub(crate) fn axis_angle(q: [f32; 4]) -> ([f32; 3], f32) {
    let q = if q[3] < 0. { q.map(|v| -v) } else { q };
    let angle = 2. * q[3].clamp(-1., 1.).acos();
    let s = (1. - q[3] * q[3]).max(0.).sqrt();
    if s < 1e-6 {
        ([0.; 3], 0.)
    } else {
        ([q[0] / s, q[1] / s, q[2] / s], angle)
    }
}
