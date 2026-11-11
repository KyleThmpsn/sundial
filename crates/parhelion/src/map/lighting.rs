//! Native monochrome object-light probes. The consumer selects a gradient by the sign of each
//! axis displacement and adds its three contributions to the visibility at the probe position.

#[derive(Clone, Copy)]
pub(super) struct Probe {
    pub position: [f32; 3],
    pub visibility: f32,
    pub gradient: [[f32; 2]; 3],
}

impl From<[f32; 4]> for Probe {
    fn from(source: [f32; 4]) -> Self {
        Self {
            position: [source[0], source[1], source[2]],
            visibility: source[3],
            gradient: [[0.; 2]; 3],
        }
    }
}

impl Probe {
    /// Shadowkeep's ordinary probe consumer adds 0.1 m to the input height. Move the native
    /// field origin by the same amount so its gradient remains anchored at the source sample.
    pub fn anchored(source: [f32; 4], gradient: [[f32; 2]; 3]) -> Self {
        let mut probe = Self::from(source);
        probe.position[2] += 0.1;
        probe.gradient = gradient;
        probe
    }

    pub fn valid(self) -> bool {
        self.position.iter().all(|v| v.is_finite())
            && (0. ..=1.).contains(&self.visibility)
            && self.gradient.iter().flatten().all(|v| v.is_finite())
    }

    /// Class 8080726D stores position.w before visibility, then six signed-axis coefficients.
    pub fn row(self) -> [f32; 12] {
        let mut row = [0.; 12];
        row[..3].copy_from_slice(&self.position);
        row[3] = 1.;
        row[4] = self.visibility;
        for (axis, coefficients) in self.gradient.into_iter().enumerate() {
            row[5 + axis * 2..7 + axis * 2].copy_from_slice(&coefficients);
        }
        row
    }
}

/// An area's complete probe population after the native default row and its cluster mapping.
pub(super) struct Plan {
    pub probes: Vec<Probe>,
    pub clusters: Vec<u16>,
}
