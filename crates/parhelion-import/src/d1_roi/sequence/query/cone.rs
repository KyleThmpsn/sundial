//! Cone query geometry. Rise of Iron shape `808022E0` holds a near distance, a far distance and a
//! half angle in degrees at `+A8`, `+AC` and `+B0`. Native shape `808092AB` holds the same three
//! values at `+04`, `+08` and `+0C` behind kind 4, and at `+28` names a Havok packfile whose one
//! `hkpConvexVerticesShape` is the cone's solid, baked for those values.
//!
//! The solid is the part of a ball of the far distance, centred on the apex, that lies inside
//! the half angle and beyond the near distance along the axis. Its frame starts at the near
//! plane with the axis along `+x`. Stock hulls with a near distance above zero and a half angle
//! from 5 to 38 degrees hold 25 vertices: three rings of eight at 45 degree steps and the far
//! point of the axis. The near ring sits on the near plane at the half angle, the far ring on
//! the ball at the half angle, and the middle ring on the ball at half of it. Their 25 planes
//! are the eight side facets, the eight facets from the far ring to the middle ring, the eight
//! from the middle ring to the axis point, and the near plane. Thirty-five stock hulls agree
//! with this construction within 0.0025 in every vertex and 0.0013 in every plane. Stock hulls
//! outside that range have other vertex or plane counts and are not built here.
use crate::tiger::physics::Hull;
use anyhow::{Result, ensure};

/// The stock hull of this form that supplies the packfile's class names, the shape header and
/// the package entry kind: Phoenix Uppercut's cone of near 1, far 3.5 and 17 degrees.
pub const TEMPLATE: u32 = 0x80BC8408;

/// The class a patch naming a cone hull declares. Hulls are untyped package entries.
pub const CLASS: u32 = 26;

#[derive(Clone, Copy)]
pub struct Cone {
    pub near: f32,
    pub far: f32,
    pub angle: f32,
}

impl Cone {
    pub fn read(near: f32, far: f32, angle: f32) -> Result<Self> {
        ensure!(
            near.is_finite()
                && far.is_finite()
                && near > 0.0
                && (5.0..=38.0).contains(&angle)
                && f64::from(far) * f64::from(angle).to_radians().cos() > f64::from(near),
            "Source query cone is outside the qualified native hull form"
        );
        Ok(Self { near, far, angle })
    }

    /// The three values' bits, which identify the baked hull.
    pub fn key(&self) -> [u32; 3] {
        [self.near, self.far, self.angle].map(f32::to_bits)
    }

    pub fn symbol(key: [u32; 3], prefix: &str) -> String {
        format!("{prefix}-cone-{:08X}{:08X}{:08X}", key[0], key[1], key[2])
    }

    pub fn from_key(key: [u32; 3]) -> Result<Self> {
        let [near, far, angle] = key.map(f32::from_bits);
        Self::read(near, far, angle)
    }

    pub(crate) fn hull(&self) -> Hull {
        let (near, far) = (f64::from(self.near), f64::from(self.far));
        let angle = f64::from(self.angle).to_radians();
        let ring = |x: f64, radius: f64| -> [[f64; 3]; 8] {
            std::array::from_fn(|k| {
                let turn = k as f64 * std::f64::consts::FRAC_PI_4;
                [x, radius * turn.cos(), radius * turn.sin()]
            })
        };
        let rings = [
            ring(0.0, near * angle.tan()),
            ring(far * angle.cos() - near, far * angle.sin()),
            ring(far * (angle / 2.0).cos() - near, far * (angle / 2.0).sin()),
        ];
        let apex = [far - near, 0.0, 0.0];
        let inside = [(far - near) / 2.0, 0.0, 0.0];
        // The plane through three points, facing away from the solid's interior.
        let plane = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| -> [f32; 4] {
            let u: [f64; 3] = std::array::from_fn(|i| b[i] - a[i]);
            let v: [f64; 3] = std::array::from_fn(|i| c[i] - a[i]);
            let mut normal = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let length = normal.iter().map(|x| x * x).sum::<f64>().sqrt();
            let dot = |n: &[f64; 3], p: &[f64; 3]| n.iter().zip(p).map(|(x, y)| x * y).sum::<f64>();
            let mut distance = -dot(&normal, &a) / length;
            normal = normal.map(|x| x / length);
            if dot(&normal, &inside) + distance > 0.0 {
                normal = normal.map(|x| -x);
                distance = -distance;
            }
            [
                normal[0] as f32,
                normal[1] as f32,
                normal[2] as f32,
                distance as f32,
            ]
        };
        let mut planes = Vec::with_capacity(25);
        for (from, to) in [(0, Some(1)), (1, Some(2)), (2, None)] {
            for k in 0..8 {
                let next = (k + 1) % 8;
                let third = to.map_or(apex, |ring| rings[ring][k]);
                planes.push(plane(rings[from][k], rings[from][next], third));
            }
        }
        planes.push([-1.0, 0.0, 0.0, 0.0]);
        let vertices = rings
            .iter()
            .flatten()
            .chain([&apex])
            .map(|vertex| vertex.map(|x| x as f32))
            .collect();
        Hull {
            vertices,
            planes,
            radius: 0.0,
        }
    }
}
