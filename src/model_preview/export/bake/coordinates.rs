//! Detail coordinates reconstructed in the primary texture's space for material baking.
use crate::model_preview::Model;

pub(super) struct Coordinates {
    matrix: [[f32; 3]; 2],
    primary: [[f32; 2]; 3],
    detail: [[f32; 2]; 3],
}

impl Coordinates {
    pub fn matrix(&self) -> [[f32; 3]; 2] {
        self.matrix
    }

    pub fn read(model: &Model, triangle: usize) -> Result<Option<Self>, String> {
        let indices = model.triangles[triangle];
        let mut primary = indices.map(|i| model.uvs[i as usize]);
        let detail = indices
            .map(|i| model.detail_uvs.get(i as usize).copied())
            .map(|v| v.ok_or("A native detail coordinate is missing"));
        let [a, b, c] = detail;
        let detail = [a?, b?, c?];
        if !primary
            .iter()
            .chain(&detail)
            .flatten()
            .all(|v| v.is_finite())
        {
            return Err("The model has a non-finite texture coordinate".into());
        }
        if detail[0] == detail[1] && detail[0] == detail[2] {
            return Ok(Some(Self {
                matrix: detail[0].map(|v| [0.0, 0.0, v]),
                primary,
                detail,
            }));
        }
        // An exported plate repeats in primary UV space. Each non-repeating detail map
        // must fit one primary tile, otherwise one plate texel would need several colors.
        for axis in 0..2 {
            let low = primary
                .iter()
                .map(|v| v[axis])
                .fold(f32::INFINITY, f32::min)
                .floor();
            let high = primary
                .iter()
                .map(|v| v[axis])
                .fold(f32::NEG_INFINITY, f32::max);
            if high > low + 1.0 + 0.000001 {
                return Ok(None);
            }
            for uv in &mut primary {
                uv[axis] -= low;
            }
        }
        let p = primary.map(|v| v.map(f64::from));
        let x = [p[1][0] - p[0][0], p[2][0] - p[0][0]];
        let y = [p[1][1] - p[0][1], p[2][1] - p[0][1]];
        let determinant = x[0] * y[1] - x[1] * y[0];
        if determinant.abs() < 1e-24 {
            return Ok(None);
        }
        let matrix = std::array::from_fn(|axis| {
            let d = detail.map(|v| f64::from(v[axis]));
            let u = ((d[1] - d[0]) * y[1] - (d[2] - d[0]) * y[0]) / determinant;
            let v = (x[0] * (d[2] - d[0]) - x[1] * (d[1] - d[0])) / determinant;
            [
                u as f32,
                v as f32,
                (d[0] - u * p[0][0] - v * p[0][1]) as f32,
            ]
        });
        if !matrix.iter().flatten().all(|v| v.is_finite()) {
            return Ok(None);
        }
        Ok(Some(Self {
            matrix,
            primary,
            detail,
        }))
    }

    pub fn sample(&self, uv: [f32; 2]) -> [f32; 2] {
        self.matrix.map(|m| m[0] * uv[0] + m[1] * uv[1] + m[2])
    }

    /// The representative map must agree at every added triangle's vertices. Float
    /// reconstruction noise may share a layer within one eighth of a detail texel.
    pub fn agrees(&self, other: &Self, tolerance: f32) -> bool {
        other
            .primary
            .iter()
            .zip(&other.detail)
            .all(|(&uv, target)| {
                self.sample(uv)
                    .iter()
                    .zip(target)
                    .all(|(a, b)| (a - b).abs() <= tolerance)
            })
    }
}
