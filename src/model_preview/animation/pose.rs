use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct Transform {
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
    scale: f32,
}

impl Transform {
    pub fn identity() -> Self {
        Self {
            rotation: [0.0, 0.0, 0.0, 1.0],
            translation: [0.0; 3],
            scale: 1.0,
        }
    }
    pub fn read(bytes: &[u8], offset: usize) -> Result<Self, String> {
        let mut result = Self::identity();
        for i in 0..4 {
            result.rotation[i] = float(bytes, offset + i * 4)?;
        }
        for i in 0..3 {
            result.translation[i] = float(bytes, offset + 16 + i * 4)?;
        }
        result.scale = float(bytes, offset + 28)?;
        if (result.scale - 1.0).abs() > 0.0001 {
            return Err("Scaled skeletons are not supported yet.".into());
        }
        result.normalize()?;
        Ok(result)
    }
    pub fn normalize(&mut self) -> Result<(), String> {
        let norm = self.rotation.iter().map(|v| v * v).sum::<f32>().sqrt();
        if !norm.is_finite() || !(0.95..=1.05).contains(&norm) {
            return Err("Animation contains an invalid joint rotation.".into());
        }
        self.rotation = self.rotation.map(|v| v / norm);
        Ok(())
    }
    pub fn point(&self, point: [f32; 3]) -> [f32; 3] {
        let q = self.rotation;
        let p = point.map(|v| v * self.scale);
        let cross = |a: [f32; 3], b: [f32; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let v = [q[0], q[1], q[2]];
        let t = cross(v, p).map(|v| v * 2.0);
        let c = cross(v, t);
        std::array::from_fn(|i| p[i] + q[3] * t[i] + c[i] + self.translation[i])
    }
    pub fn compose(&self, child: Self) -> Self {
        let [x, y, z, w] = self.rotation;
        let [a, b, c, d] = child.rotation;
        Self {
            rotation: [
                w * a + x * d + y * c - z * b,
                w * b - x * c + y * d + z * a,
                w * c + x * b - y * a + z * d,
                w * d - x * a - y * b - z * c,
            ],
            translation: self.point(child.translation),
            scale: self.scale * child.scale,
        }
    }
    pub fn lerp(self, mut other: Self, t: f32) -> Self {
        if self
            .rotation
            .iter()
            .zip(other.rotation)
            .map(|(a, b)| a * b)
            .sum::<f32>()
            < 0.0
        {
            other.rotation = other.rotation.map(|v| -v);
        }
        let mut rotation =
            std::array::from_fn(|i| self.rotation[i] * (1.0 - t) + other.rotation[i] * t);
        let norm = rotation.iter().map(|v| v * v).sum::<f32>().sqrt();
        rotation = rotation.map(|v| v / norm);
        Self {
            rotation,
            translation: std::array::from_fn(|i| {
                self.translation[i] * (1.0 - t) + other.translation[i] * t
            }),
            scale: self.scale * (1.0 - t) + other.scale * t,
        }
    }
}
