//! Tangent-space normals in the preview's unflipped native UV convention.
#[derive(Clone, Copy)]
pub(crate) struct Basis {
    pub normal: [f32; 3],
    tangent: [f32; 3],
    bitangent: [f32; 3],
}

pub(crate) fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let length = dot(v, v).sqrt();
    (length.is_finite() && length > 1e-8).then(|| v.map(|c| c / length))
}
pub(crate) fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

impl Basis {
    pub fn triangle(points: [[f32; 3]; 3], uv: [[f32; 2]; 3]) -> Option<Self> {
        let a: [f32; 3] = std::array::from_fn(|i| points[1][i] - points[0][i]);
        let b: [f32; 3] = std::array::from_fn(|i| points[2][i] - points[0][i]);
        let normal = normalize(cross(a, b))?;
        let du = [uv[1][0] - uv[0][0], uv[2][0] - uv[0][0]];
        let dv = [uv[1][1] - uv[0][1], uv[2][1] - uv[0][1]];
        let det = du[0] * dv[1] - du[1] * dv[0];
        if !det.is_finite() || det.abs() < 1e-8 {
            return None;
        }
        Some(Self {
            normal,
            tangent: normalize(std::array::from_fn(|i| (a[i] * dv[1] - b[i] * dv[0]) / det))?,
            bitangent: normalize(std::array::from_fn(|i| (b[i] * du[0] - a[i] * du[1]) / det))?,
        })
    }

    pub fn with_normal(self, normal: [f32; 3]) -> Self {
        let normal = normalize(normal).unwrap_or(self.normal);
        let normal = if dot(normal, self.normal) < 0.0 {
            normal.map(|v| -v)
        } else {
            normal
        };
        let amount = dot(self.tangent, normal);
        let tangent = normalize(std::array::from_fn(|i| {
            self.tangent[i] - normal[i] * amount
        }))
        .unwrap_or(self.tangent);
        let b = cross(normal, tangent);
        let bitangent = if dot(b, self.bitangent) < 0.0 {
            b.map(|v| -v)
        } else {
            b
        };
        Self {
            normal,
            tangent,
            bitangent,
        }
    }

    pub fn apply(self, packed: [f32; 2]) -> [f32; 3] {
        let [x, y] = packed.map(|v| (v * 2.0 - 1.0).clamp(-1.0, 1.0));
        let z = (1.0 - x * x - y * y).max(0.0).sqrt();
        normalize(std::array::from_fn(|i| {
            self.tangent[i] * x + self.bitangent[i] * y + self.normal[i] * z
        }))
        .unwrap_or(self.normal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mirrored_uvs_reverse_the_tangent_and_neutral_maps_preserve_the_surface() {
        let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let a = Basis::triangle(points, [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]).unwrap();
        let b = Basis::triangle(points, [[0.0, 0.0], [-1.0, 0.0], [0.0, 1.0]]).unwrap();
        assert_eq!(a.apply([0.5, 0.5]), [0.0, 0.0, 1.0]);
        assert!(a.apply([0.8, 0.5])[0] > 0.0);
        assert!(b.apply([0.8, 0.5])[0] < 0.0);
        assert!(Basis::triangle(points, [[0.0; 2]; 3]).is_none());
        assert!(
            dot(
                a.with_normal([0.0, 1.0, 1.0]).apply([0.5, 0.5]),
                normalize([0.0, 1.0, 1.0]).unwrap()
            ) > 0.999
        );
    }
}
