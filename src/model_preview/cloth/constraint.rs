use super::{
    Point,
    collision::Collisions,
    frame,
    graph::{Graph, MAX_PARTICLES},
    math::*,
    pack::Pack,
};

pub(super) struct Simulation {
    pub current: usize,
    pub previous: usize,
    pub particles: Vec<Particle>,
    pub constraints: Vec<Constraint>,
    gravity: Vector,
    damping: f32,
    triangles: Vec<[usize; 3]>,
    flips: Vec<u8>,
    normals: bool,
    pub collisions: Collisions,
}

pub(super) struct Particle {
    pub mass: f32,
    pub inverse: f32,
    pub radius: f32,
    pub friction: f32,
}
pub(super) struct Link {
    a: usize,
    b: usize,
    rest: f32,
    stiffness: f32,
}
pub(super) struct Bend {
    vertices: [usize; 4],
    weights: [f32; 4],
    stiffness: f32,
    rest: Option<f32>,
}
pub(super) struct BonePlane {
    particle: usize,
    bone: usize,
    equation: [f32; 4],
    stiffness: f32,
}
pub(super) enum Constraint {
    Link(Vec<Link>),
    Stretch(Vec<Link>),
    Bend(Vec<Bend>),
    Plane {
        set: usize,
        planes: Vec<BonePlane>,
    },
    /// Steady simulation has no requested transition. Viewer seeks initialize both
    /// particle buffers again, matching the native inactive transition state.
    Transition,
}

impl Simulation {
    pub fn read(p: &Pack<'_>, at: usize, index: usize, g: &Graph) -> Result<Self, String> {
        p.expect(at, "hclSimClothData", 0x180)?;
        if p.u8(at + 0x28)? != 0
            || p.u8(at + 0x2a)? != 0
            || !p.objects(at + 0xc8, 128)?.is_empty()
            || !p.objects(at + 0xe8, 128)?.is_empty()
        {
            return Err(
                "Cloth pinch detection, motion transfer and actions need an unsupported scene host"
                    .into(),
            );
        }
        let particles = p
            .array(at + 0x48, 16, MAX_PARTICLES)?
            .into_iter()
            .map(|row| {
                let v = p.vector::<4>(row)?;
                if v.iter().any(|v| *v < 0.) || v[3] > 1. {
                    return Err("Invalid cloth particle mass, radius or friction".into());
                }
                Ok(Particle {
                    mass: v[0],
                    inverse: v[1],
                    radius: v[2],
                    friction: v[3],
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if particles.is_empty() {
            return Err("Empty cloth particle set".into());
        }
        let find = |kind| -> Result<usize, String> {
            let matches = g
                .buffers
                .iter()
                .enumerate()
                .filter(|(_, b)| b.kind == kind && b.subtype == index)
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            if matches.len() != 1 || g.buffers[matches[0]].count != particles.len() {
                return Err("Cloth particle buffers are missing or ambiguous".into());
            }
            Ok(matches[0])
        };
        let current = find(1)?;
        let previous = find(2)?;
        let damping = p.f32(at + 0x20)?;
        if !(0.0..=1.0).contains(&damping) {
            return Err("Invalid cloth damping".into());
        }
        let ids = p
            .array(at + 0x58, 2, MAX_PARTICLES * 24)?
            .into_iter()
            .map(|row| p.u16(row))
            .collect::<Result<Vec<_>, _>>()?;
        if ids.len() % 3 != 0 || ids.iter().any(|i| *i >= particles.len()) {
            return Err("Invalid cloth simulation topology".into());
        }
        let triangles = ids
            .chunks_exact(3)
            .map(|t| [t[0], t[1], t[2]])
            .collect::<Vec<_>>();
        let flips = frame::flip_bytes(p, at + 0x68, triangles.len())?;
        let constraints = p
            .objects(at + 0xb8, 128)?
            .into_iter()
            .map(|c| Constraint::read(p, c, particles.len(), g))
            .collect::<Result<_, _>>()?;
        let collisions = Collisions::read(p, at, particles.len(), g)?;
        Ok(Self {
            current,
            previous,
            particles,
            constraints,
            gravity: p.vector(at + 0x10)?,
            damping,
            triangles,
            flips,
            normals: p.u8(at + 0x14c)? != 0,
            collisions,
        })
    }

    pub fn step(
        &self,
        buffers: &mut [Vec<Point>],
        transforms: &[Vec<Matrix>],
        colliders: &mut [Matrix],
        substeps: usize,
        iterations: usize,
        order: &[i32],
    ) {
        let dt = (1. / 60.) / substeps as f32;
        let damping = (1. - self.damping).powf(dt);
        let placed = self.collisions.prepare(colliders, transforms, substeps);
        for (substep, colliders) in placed.iter().enumerate() {
            for (i, particle) in self.particles.iter().enumerate() {
                let p = buffers[self.current][i].position;
                let prev = buffers[self.previous][i].position;
                let gravity = mul(mul(self.gravity, particle.mass), particle.inverse * dt * dt);
                buffers[self.previous][i].position = p;
                buffers[self.current][i].position =
                    add(add(p, mul(sub(p, prev), damping)), gravity);
            }
            for _ in 0..iterations {
                if order.is_empty() {
                    for c in &self.constraints {
                        c.apply(
                            &self.particles,
                            &mut buffers[self.current],
                            transforms,
                            substep,
                            substeps,
                        );
                    }
                    self.collisions.apply(self, buffers, colliders, dt);
                } else {
                    for &index in order {
                        if index == -1 {
                            self.collisions.apply(self, buffers, colliders, dt);
                        } else {
                            self.constraints[index as usize].apply(
                                &self.particles,
                                &mut buffers[self.current],
                                transforms,
                                substep,
                                substeps,
                            );
                        }
                    }
                }
            }
        }
        if self.normals {
            let points = &mut buffers[self.current];
            let mut normals = vec![[0.; 3]; points.len()];
            for (i, &t) in self.triangles.iter().enumerate() {
                let n = frame::face(points, t, &self.flips, i);
                for v in t {
                    normals[v] = add(normals[v], n);
                }
            }
            for (i, n) in normals.into_iter().enumerate() {
                let n = normal(n);
                buffers[self.current][i].normal = n;
                buffers[self.previous][i].normal = n;
            }
        }
    }
}

impl Constraint {
    fn read(p: &Pack<'_>, at: usize, particles: usize, g: &Graph) -> Result<Self, String> {
        let particle = |at| -> Result<usize, String> {
            let i = p.u16(at)?;
            if i >= particles {
                Err("Cloth constraint references a missing particle".into())
            } else {
                Ok(i)
            }
        };
        Ok(match p.class(at)? {
            "hclStandardLinkConstraintSet" | "hclStretchLinkConstraintSet" => {
                let links = p
                    .array(at + 0x20, 12, MAX_PARTICLES * 16)?
                    .into_iter()
                    .map(|row| {
                        let rest = p.f32(row + 4)?;
                        let stiffness = p.f32(row + 8)?;
                        if rest < 0. || stiffness < 0. {
                            return Err("Invalid cloth link length or stiffness".into());
                        }
                        Ok(Link {
                            a: particle(row)?,
                            b: particle(row + 2)?,
                            rest,
                            stiffness,
                        })
                    })
                    .collect::<Result<_, String>>()?;
                if p.class(at)? == "hclStandardLinkConstraintSet" {
                    Self::Link(links)
                } else {
                    Self::Stretch(links)
                }
            }
            "hclBendStiffnessConstraintSet" => {
                if !p.array(at + 0x30, 16, MAX_PARTICLES * 16)?.is_empty() {
                    return Err("Quantized cloth bend weights are not supported".into());
                }
                let rest = p.u8(at + 0x40)? != 0;
                let rows = p
                    .array(at + 0x20, 32, MAX_PARTICLES * 16)?
                    .into_iter()
                    .map(|row| {
                        Ok(Bend {
                            vertices: [
                                particle(row + 0x18)?,
                                particle(row + 0x1a)?,
                                particle(row + 0x1c)?,
                                particle(row + 0x1e)?,
                            ],
                            weights: p.vector(row)?,
                            stiffness: p.f32(row + 0x10)?,
                            rest: if rest { Some(p.f32(row + 0x14)?) } else { None },
                        })
                    })
                    .collect::<Result<_, String>>()?;
                Self::Bend(rows)
            }
            "hclBonePlanesConstraintSet" => {
                if !p.array(at + 0x30, 16, MAX_PARTICLES * 4)?.is_empty() {
                    return Err("Compressed cloth bone planes are not supported".into());
                }
                let set = p.u32(at + 0x40)?;
                let transforms = g
                    .transforms
                    .get(set)
                    .ok_or("Cloth bone plane transform set is missing")?;
                let planes = p
                    .array(at + 0x20, 32, MAX_PARTICLES * 4)?
                    .into_iter()
                    .map(|row| {
                        let bone = p.u16(row + 0x12)?;
                        if bone >= transforms.len() {
                            return Err("Cloth bone plane transform is missing".into());
                        }
                        Ok(BonePlane {
                            particle: particle(row + 0x10)?,
                            bone,
                            equation: p.vector(row)?,
                            stiffness: p.f32(row + 0x14)?,
                        })
                    })
                    .collect::<Result<_, String>>()?;
                Self::Plane { set, planes }
            }
            "hclTransitionConstraintSet" => {
                let reference = g.buffer(p.u32(at + 0x50)?)?;
                let rows = p.array(at + 0x20, 4, particles)?;
                for row in &rows {
                    particle(*row)?;
                    if p.u16(row + 2)? >= reference {
                        return Err("Missing cloth transition reference vertex".into());
                    }
                }
                let params = p.array(at + 0x30, 12, particles)?;
                if params.len() != 1 && params.len() != rows.len() {
                    return Err("Cloth transition parameters do not match their particles".into());
                }
                for row in params {
                    if p.vector::<3>(row)?.iter().any(|v| *v < 0.) {
                        return Err("Invalid cloth transition parameter".into());
                    }
                }
                Self::Transition
            }
            class => {
                return Err(format!(
                    "Cloth constraint {class} is not supported by playback"
                ));
            }
        })
    }

    fn apply(
        &self,
        particles: &[Particle],
        points: &mut [Point],
        transforms: &[Vec<Matrix>],
        _substep: usize,
        _substeps: usize,
    ) {
        match self {
            Self::Link(links) => {
                for link in links {
                    let delta = sub(points[link.b].position, points[link.a].position);
                    let distance = length(delta);
                    if distance <= 0. {
                        continue;
                    }
                    let correction =
                        mul(delta, ((distance - link.rest) * link.stiffness) / distance);
                    points[link.a].position = add(
                        points[link.a].position,
                        mul(correction, particles[link.a].inverse),
                    );
                    points[link.b].position = sub(
                        points[link.b].position,
                        mul(correction, particles[link.b].inverse),
                    );
                }
            }
            Self::Stretch(links) => {
                for link in links {
                    let delta = sub(points[link.b].position, points[link.a].position);
                    let distance = length(delta);
                    if distance <= 0. {
                        continue;
                    }
                    points[link.b].position = add(
                        points[link.b].position,
                        mul(
                            delta,
                            ((link.rest - distance).min(0.) * link.stiffness) / distance,
                        ),
                    );
                }
            }
            Self::Bend(links) => {
                for link in links {
                    let positions = link.vertices.map(|v| points[v].position);
                    let mut delta = [0.; 3];
                    for (position, weight) in positions.into_iter().zip(link.weights) {
                        delta = add(delta, mul(position, weight));
                    }
                    if let Some(rest) = link.rest {
                        let [a, b, c, d] = positions;
                        let edge = sub(d, c);
                        let n1 = cross(edge, sub(a, c));
                        let n2 = cross(sub(b, c), edge);
                        let edge2 = dot(edge, edge);
                        let factor = if edge2 > 0. {
                            length(n1) * length(n2) / edge2
                        } else {
                            0.
                        };
                        let n = normal(add(normal(n1), normal(n2)));
                        delta = add(delta, mul(n, factor * rest));
                    }
                    for (vertex, weight) in link.vertices.into_iter().zip(link.weights) {
                        points[vertex].position = add(
                            points[vertex].position,
                            mul(delta, (weight * link.stiffness) * particles[vertex].inverse),
                        );
                    }
                }
            }
            Self::Plane { set, planes } => {
                for plane in planes {
                    let transform = &transforms[*set][plane.bone];
                    let n = direction(
                        transform,
                        [plane.equation[0], plane.equation[1], plane.equation[2]],
                    );
                    let origin = [transform[12], transform[13], transform[14]];
                    let distance =
                        dot(sub(points[plane.particle].position, origin), n) + plane.equation[3];
                    if distance < 0. {
                        points[plane.particle].position = sub(
                            points[plane.particle].position,
                            mul(n, distance * plane.stiffness),
                        );
                    }
                }
            }
            Self::Transition => {}
        }
    }
}
