//! Native per-instance colliders. The studio has no landscape collision provider.
use super::{Point, constraint::Simulation, graph::Graph, math::*, pack::Pack};

pub(super) struct Collisions {
    definitions: Vec<Collider>,
    set: Option<usize>,
    bones: Vec<usize>,
    offsets: Vec<Matrix>,
    masks: Vec<u32>,
}
struct Collider {
    transform: Matrix,
    shape: Shape,
    linear: Vector,
    angular: Vector,
}
enum Shape {
    Capsule {
        a: Vector,
        b: Vector,
        radius: f32,
    },
    Tapered {
        a: Vector,
        b: Vector,
        small: f32,
        big: f32,
    },
    Plane([f32; 4]),
}
pub(super) struct Placed {
    matrix: Matrix,
    linear: Vector,
    angular: Vector,
}

impl Collisions {
    pub fn read(p: &Pack<'_>, at: usize, particles: usize, g: &Graph) -> Result<Self, String> {
        let raw_set = p.u32(at + 0x80)? as u32 as i32;
        let set = if raw_set < 0 {
            None
        } else {
            Some(raw_set as usize)
        };
        let bones = p
            .array(at + 0x88, 4, 31)?
            .into_iter()
            .map(|row| p.u32(row))
            .collect::<Result<Vec<_>, _>>()?;
        let offsets = p
            .array(at + 0x98, 64, 31)?
            .into_iter()
            .map(|row| p.vector(row))
            .collect::<Result<Vec<Matrix>, _>>()?;
        let mut definitions = Vec::new();
        for collider in p.objects(at + 0xa8, 31)? {
            p.expect(collider, "hclCollidable", 0x90)?;
            if p.u8(collider + 0x80)? != 0 {
                return Err("Cloth pinching colliders are not supported".into());
            }
            let shape = p
                .pointer(collider + 0x88)?
                .ok_or("Cloth collider has no shape")?;
            let shape = match p.class(shape)? {
                "hclCapsuleShape" => Shape::Capsule {
                    a: p.vector(shape + 0x20)?,
                    b: p.vector(shape + 0x30)?,
                    radius: p.f32(shape + 0x50)?,
                },
                "hclTaperedCapsuleShape" => Shape::Tapered {
                    a: p.vector(shape + 0x20)?,
                    b: p.vector(shape + 0x30)?,
                    small: p.f32(shape + 0x90)?,
                    big: p.f32(shape + 0x94)?,
                },
                "hclPlaneShape" => Shape::Plane(p.vector(shape + 0x20)?),
                class => return Err(format!("Cloth collision shape {class} is not supported")),
            };
            match &shape {
                Shape::Capsule { a, b, radius } if *radius < 0. || length(sub(*b, *a)) <= 0. => {
                    return Err("Invalid cloth capsule".into());
                }
                Shape::Tapered { a, b, small, big }
                    if *small < 0. || *big < *small || length(sub(*b, *a)) <= big - small =>
                {
                    return Err("Invalid tapered cloth capsule".into());
                }
                Shape::Plane(e) if (length([e[0], e[1], e[2]]) - 1.).abs() > 0.003 => {
                    return Err("Invalid cloth collision plane".into());
                }
                _ => {}
            }
            let transform = p.vector(collider + 0x20)?;
            inverse_rigid(&transform)?;
            definitions.push(Collider {
                transform,
                shape,
                linear: p.vector(collider + 0x60)?,
                angular: p.vector(collider + 0x70)?,
            });
        }
        if let Some(set) = set {
            let transforms = g
                .transforms
                .get(set)
                .ok_or("Missing cloth collider transform set")?;
            if bones.len() != definitions.len()
                || offsets.len() != definitions.len()
                || bones.iter().any(|i| *i >= transforms.len())
            {
                return Err("Cloth collider transform map is incomplete".into());
            }
            for offset in &offsets {
                inverse_rigid(offset)?;
            }
        } else if !bones.is_empty() || !offsets.is_empty() {
            return Err("Unbound cloth collider transforms".into());
        }
        let masks = p
            .array(at + 0xf8, 4, particles)?
            .into_iter()
            .map(|row| p.u32(row).map(|v| v as u32))
            .collect::<Result<Vec<_>, _>>()?;
        if !definitions.is_empty() && masks.len() != particles {
            return Err("Cloth collision masks do not cover every particle".into());
        }
        Ok(Self {
            definitions,
            set,
            bones,
            offsets,
            masks,
        })
    }
    pub fn initial(&self, transforms: &[Vec<Matrix>]) -> Vec<Matrix> {
        self.definitions
            .iter()
            .enumerate()
            .map(|(i, c)| {
                self.set.map_or(c.transform, |set| {
                    compose(&transforms[set][self.bones[i]], &self.offsets[i])
                })
            })
            .collect()
    }
    pub fn prepare(
        &self,
        last: &mut [Matrix],
        transforms: &[Vec<Matrix>],
        substeps: usize,
    ) -> Vec<Vec<Placed>> {
        let mut result = (0..substeps).map(|_| Vec::new()).collect::<Vec<_>>();
        let dt = (1. / 60.) / substeps as f32;
        for (i, c) in self.definitions.iter().enumerate() {
            let (linear, angular) = if let Some(set) = self.set {
                let desired = compose(&transforms[set][self.bones[i]], &self.offsets[i]);
                let old = last[i];
                let linear = mul(
                    [
                        desired[12] - old[12],
                        desired[13] - old[13],
                        desired[14] - old[14],
                    ],
                    60.,
                );
                let q = quaternion(&old);
                let q = qmul(quaternion(&desired), [-q[0], -q[1], -q[2], q[3]]);
                let angle = 2. * q[3].abs().clamp(0., 1.).acos();
                let axis = [q[0], q[1], q[2]];
                let angular = if dot(axis, axis) > f32::EPSILON {
                    mul(normal(axis), angle * 60. * if q[3] < 0. { -1. } else { 1. })
                } else {
                    [0.; 3]
                };
                (linear, angular)
            } else {
                (c.linear, c.angular)
            };
            for frame in &mut result {
                let old = last[i];
                let t = add([old[12], old[13], old[14]], mul(linear, dt));
                // The archived integrator uses this bounded quaternion increment.
                let v = mul(angular, dt * 0.5);
                let s = dot(v, v) * 0.405_284_7;
                let w = ((1. - s * 0.822_948) - s * s * 0.130_529) - s * s * s * 0.044_408;
                let q = qmul([v[0], v[1], v[2], w], quaternion(&old));
                let norm = q.iter().map(|v| v * v).sum::<f32>().sqrt();
                let matrix = rotation(q.map(|v| v / norm), t);
                last[i] = matrix;
                frame.push(Placed {
                    matrix,
                    linear,
                    angular,
                });
            }
        }
        result
    }
    pub fn apply(&self, sim: &Simulation, buffers: &mut [Vec<Point>], placed: &[Placed], dt: f32) {
        for (index, (definition, placed)) in self.definitions.iter().zip(placed).enumerate() {
            for (i, particle) in sim.particles.iter().enumerate() {
                if self.masks[i] & (1 << index) == 0 {
                    continue;
                }
                let p = buffers[sim.current][i].position;
                let (surface, n, distance) = definition.shape.contact(&placed.matrix, p);
                if distance >= particle.radius {
                    continue;
                }
                let position = add(p, mul(n, particle.radius - dot(sub(p, surface), n)));
                let previous = buffers[sim.previous][i].position;
                let origin = [placed.matrix[12], placed.matrix[13], placed.matrix[14]];
                let velocity = add(placed.linear, cross(placed.angular, sub(surface, origin)));
                let motion = sub(sub(position, previous), mul(velocity, dt));
                let tangent = sub(motion, mul(n, dot(motion, n)));
                buffers[sim.current][i].position = position;
                buffers[sim.previous][i].position = add(previous, mul(tangent, particle.friction));
            }
        }
    }
}

impl Shape {
    fn contact(&self, m: &Matrix, p: Vector) -> (Vector, Vector, f32) {
        match *self {
            Self::Plane(e) => {
                let n = direction(m, [e[0], e[1], e[2]]);
                let origin = [m[12], m[13], m[14]];
                let distance = dot(sub(p, origin), n) + e[3];
                (sub(p, mul(n, distance)), n, distance)
            }
            Self::Capsule { a, b, radius } => {
                let a = point(m, a);
                let b = point(m, b);
                let axis = sub(b, a);
                let t = (dot(sub(p, a), axis) / dot(axis, axis)).clamp(0., 1.);
                sphere(add(a, mul(axis, t)), radius, p)
            }
            Self::Tapered { a, b, small, big } => {
                let a = point(m, a);
                let b = point(m, b);
                let axis = sub(b, a);
                let l = length(axis);
                let axis = mul(axis, 1. / l);
                let h = dot(sub(p, a), axis);
                let radial = sub(sub(p, a), mul(axis, h));
                let sin = (big - small) / l;
                let cos = (1. - sin * sin).sqrt();
                let distance = length(radial) * cos - h * sin - small;
                let projected = h + distance * sin;
                if projected < -small * sin {
                    return sphere(a, small, p);
                }
                if projected > l - big * sin {
                    return sphere(b, big, p);
                }
                let n = sub(mul(normal(radial), cos), mul(axis, sin));
                (sub(p, mul(n, distance)), n, distance)
            }
        }
    }
}
fn sphere(center: Vector, radius: f32, p: Vector) -> (Vector, Vector, f32) {
    let delta = sub(p, center);
    let n = normal(delta);
    (add(center, mul(n, radius)), n, length(delta) - radius)
}
