pub(super) type Vector = [f32; 3];
pub(super) type Matrix = [f32; 16];
pub(super) fn add(a: Vector, b: Vector) -> Vector {
    std::array::from_fn(|i| a[i] + b[i])
}
pub(super) fn sub(a: Vector, b: Vector) -> Vector {
    std::array::from_fn(|i| a[i] - b[i])
}
pub(super) fn mul(a: Vector, s: f32) -> Vector {
    a.map(|v| v * s)
}
pub(super) fn dot(a: Vector, b: Vector) -> f32 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}
pub(super) fn cross(a: Vector, b: Vector) -> Vector {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(super) fn length(a: Vector) -> f32 {
    dot(a, a).sqrt()
}
pub(super) fn normal(a: Vector) -> Vector {
    let l = length(a);
    if l > 0. { mul(a, 1. / l) } else { [0.; 3] }
}
pub(super) fn identity() -> Matrix {
    std::array::from_fn(|i| if i % 5 == 0 { 1. } else { 0. })
}
pub(super) fn direction(m: &Matrix, v: Vector) -> Vector {
    std::array::from_fn(|i| (m[i] * v[0] + m[4 + i] * v[1]) + m[8 + i] * v[2])
}
pub(super) fn point(m: &Matrix, v: Vector) -> Vector {
    std::array::from_fn(|i| ((m[i] * v[0] + m[12 + i]) + m[4 + i] * v[1]) + m[8 + i] * v[2])
}
pub(super) fn compose(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = identity();
    for column in 0..4 {
        let v = [b[column * 4], b[column * 4 + 1], b[column * 4 + 2]];
        let v = if column == 3 {
            point(a, v)
        } else {
            direction(a, v)
        };
        out[column * 4..column * 4 + 3].copy_from_slice(&v);
    }
    out
}
pub(super) fn inverse_rigid(m: &Matrix) -> Result<Matrix, String> {
    let mut out = identity();
    for i in 0..3 {
        for j in 0..3 {
            out[i * 4 + j] = m[j * 4 + i];
        }
    }
    let t = direction(&out, [m[12], m[13], m[14]]);
    out[12..15].copy_from_slice(&t.map(|v| -v));
    let check = compose(m, &out);
    if check
        .iter()
        .zip(identity())
        .any(|(a, b)| (a - b).abs() > 0.003)
    {
        return Err("Cloth bind transforms must be rigid".into());
    }
    Ok(out)
}

pub(super) fn quaternion(m: &Matrix) -> [f32; 4] {
    let mut q = [0.; 4];
    let trace = m[0] + m[5] + m[10];
    if trace > 0. {
        let s = (trace + 1.).sqrt() * 2.;
        q = [
            (m[6] - m[9]) / s,
            (m[8] - m[2]) / s,
            (m[1] - m[4]) / s,
            s * 0.25,
        ];
    } else {
        let i = if m[0] > m[5] && m[0] > m[10] {
            0
        } else if m[5] > m[10] {
            1
        } else {
            2
        };
        let j = (i + 1) % 3;
        let k = (i + 2) % 3;
        let s = (1. + m[i * 5] - m[j * 5] - m[k * 5]).max(0.).sqrt() * 2.;
        if s > 0. {
            q[i] = s * 0.25;
            q[j] = (m[j * 4 + i] + m[i * 4 + j]) / s;
            q[k] = (m[k * 4 + i] + m[i * 4 + k]) / s;
            q[3] = (m[j * 4 + k] - m[k * 4 + j]) / s;
        } else {
            q[3] = 1.;
        }
    }
    let norm = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    q.map(|v| v / norm)
}
pub(super) fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [x, y, z, w] = a;
    let [i, j, k, r] = b;
    [
        w * i + x * r + y * k - z * j,
        w * j - x * k + y * r + z * i,
        w * k + x * j - y * i + z * r,
        w * r - x * i - y * j - z * k,
    ]
}
pub(super) fn rotation(q: [f32; 4], t: Vector) -> Matrix {
    let [x, y, z, w] = q;
    [
        1. - 2. * (y * y + z * z),
        2. * (x * y + z * w),
        2. * (x * z - y * w),
        0.,
        2. * (x * y - z * w),
        1. - 2. * (x * x + z * z),
        2. * (y * z + x * w),
        0.,
        2. * (x * z + y * w),
        2. * (y * z - x * w),
        1. - 2. * (x * x + y * y),
        0.,
        t[0],
        t[1],
        t[2],
        1.,
    ]
}
