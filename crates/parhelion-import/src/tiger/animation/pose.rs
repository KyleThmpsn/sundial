//! Checked uniform-scale transforms for native control-space translation.
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug)]
pub struct Pose(pub [f32; 8]);

impl Pose {
    pub const IDENTITY: Self = Self([0., 0., 0., 1., 0., 0., 0., 1.]);

    pub fn checked(mut v: [f32; 8]) -> Result<Self> {
        let norm = v[..4].iter().map(|v| v * v).sum::<f32>().sqrt();
        ensure!(
            v.iter().all(|v| v.is_finite()) && norm > 1e-6 && v[7] > 0.,
            "Invalid animation pose"
        );
        v[..4].iter_mut().for_each(|v| *v /= norm);
        Ok(Self(v))
    }

    fn rotate(self, p: [f32; 3]) -> [f32; 3] {
        let [x, y, z, w, ..] = self.0;
        let t = [
            2. * (y * p[2] - z * p[1]),
            2. * (z * p[0] - x * p[2]),
            2. * (x * p[1] - y * p[0]),
        ];
        [
            p[0] + w * t[0] + y * t[2] - z * t[1],
            p[1] + w * t[1] + z * t[0] - x * t[2],
            p[2] + w * t[2] + x * t[1] - y * t[0],
        ]
    }

    pub fn then(self, b: Self) -> Self {
        let [x, y, z, w, ..] = self.0;
        let [i, j, k, r, ..] = b.0;
        let p = self.rotate([b.0[4] * self.0[7], b.0[5] * self.0[7], b.0[6] * self.0[7]]);
        Self([
            w * i + x * r + y * k - z * j,
            w * j - x * k + y * r + z * i,
            w * k + x * j - y * i + z * r,
            w * r - x * i - y * j - z * k,
            p[0] + self.0[4],
            p[1] + self.0[5],
            p[2] + self.0[6],
            self.0[7] * b.0[7],
        ])
    }

    pub fn inverse(self) -> Self {
        let mut inverse = self;
        for v in &mut inverse.0[..3] {
            *v = -*v;
        }
        inverse.0[7] = 1. / self.0[7];
        let p = inverse.rotate([
            -self.0[4] * inverse.0[7],
            -self.0[5] * inverse.0[7],
            -self.0[6] * inverse.0[7],
        ]);
        inverse.0[4..7].copy_from_slice(&p);
        inverse
    }

    pub fn mix(self, mut b: Self, t: f32) -> Result<Self> {
        if self.0[..4]
            .iter()
            .zip(&b.0[..4])
            .map(|(a, b)| a * b)
            .sum::<f32>()
            < 0.
        {
            b.0[..4].iter_mut().for_each(|v| *v = -*v);
        }
        Self::checked(std::array::from_fn(|i| {
            self.0[i] + (b.0[i] - self.0[i]) * t
        }))
    }
}
