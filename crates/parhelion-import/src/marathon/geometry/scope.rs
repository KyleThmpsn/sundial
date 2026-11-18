//! A source optic's physical aperture mask locates its opening without material or item hashes.
use super::*;

#[derive(Clone)]
pub(crate) struct Lens {
    pub center: [f32; 3],
    pub aim: [f32; 3],
    pub axes: [[f32; 3]; 2],
    pub radii: [f32; 2],
    /// Unit normal of the mask's own plane.
    pub normal: [f32; 3],
    pub artwork: bool,
    pub outline: Vec<[f32; 3]>,
    /// The source eye lies within the bounds of the mesh that carries the aperture, as in the
    /// long-range sights whose eye sits inside their sight box. The source shows such a sight's
    /// aimed view through the aperture alone.
    pub enclosed: bool,
    /// The aperture outline in view tangents from the source eye, as in [`Framing::window`], when
    /// the sight has a rear-to-front axis. The source reticle's stencil test keeps its artwork
    /// inside this window. Empty without an axis.
    pub window: Vec<[f32; 3]>,
    /// The aimed view an enclosed sight with an angular override shows.
    pub framing: Option<Framing>,
}

/// An enclosed sight's aimed view as the source frames it. The source view at full aim spans the
/// sight's angular override horizontally, and the aperture seen from the source eye bounds the
/// scene in it, with the housing dark around it.
#[derive(Clone)]
pub(crate) struct Framing {
    /// Tangent of half the source horizontal view.
    pub tangent: f32,
    /// The aperture outline in view tangents from the source eye, with screen X to the right and
    /// Y up. A tangent point is inside the window when every `a * x + b * y + c` is positive.
    pub window: Vec<[f32; 3]>,
    /// Distance from the source eye to the aperture along the sight axis.
    pub eye: f32,
    /// The source eye, the unit sight axis and the aperture axes, in the sight model's space.
    pub origin: [f32; 3],
    pub forward: [f32; 3],
    pub axes: [[f32; 3]; 2],
}

impl Framing {
    /// Whether every point lies ahead of the source eye, so the aimed view can show it.
    pub(super) fn ahead(&self, points: &[[f32; 3]]) -> bool {
        points.iter().all(|p| {
            (0..3)
                .map(|a| (p[a] - self.origin[a]) * self.forward[a])
                .sum::<f32>()
                > 0.
        })
    }

    fn new(lens: &Lens, angle: f32, rear: [f32; 3], front: [f32; 3]) -> Option<Self> {
        if !lens.enclosed || !(angle > 0. && angle < std::f32::consts::PI) {
            return None;
        }
        let (eye, forward, window) = sighted(lens, rear, front)?;
        Some(Self {
            tangent: (angle * 0.5).tan(),
            window,
            eye,
            origin: rear,
            forward,
            axes: lens.axes,
        })
    }
}

/// The aperture as the source eye at `rear` sees it along the axis to `front`: the eye's distance
/// to the aperture along the axis, the unit axis, and the outline in view tangents with screen X
/// to the right and Y up.
fn sighted(lens: &Lens, rear: [f32; 3], front: [f32; 3]) -> Option<(f32, [f32; 3], Vec<[f32; 3]>)> {
    let axis: [f32; 3] = std::array::from_fn(|a| front[a] - rear[a]);
    let length = axis.iter().map(|v| v * v).sum::<f32>().sqrt();
    let eye = (0..3)
        .map(|a| (lens.center[a] - rear[a]) * axis[a])
        .sum::<f32>()
        / length;
    if !(eye.is_finite() && eye > 0.) {
        return None;
    }
    let offset: [f32; 3] = std::array::from_fn(|a| lens.aim[a] - lens.center[a]);
    let origin = lens
        .axes
        .map(|axis| (0..3).map(|a| axis[a] * offset[a]).sum::<f32>());
    let edges = if lens.outline.is_empty() {
        let [x, y] = lens.radii;
        vec![[-1., 0., x], [1., 0., x], [0., -1., y], [0., 1., y]]
    } else {
        lens.outline.clone()
    };
    // Aperture axes run along model +Y and +Z. With the source forward along +X and +Z up,
    // +Y points to the left of the view.
    let window = edges
        .into_iter()
        .map(|[a, b, c]| [-a * eye, b * eye, a * origin[0] + b * origin[1] + c])
        .collect();
    Some((eye, axis.map(|v| v / length), window))
}

/// Model-space positions of a draw's vertices, in their local skeleton's rest pose when posed.
pub(super) fn points(
    model: &Payload,
    vertices: &Payload,
    posed: Option<&[[f32; 12]]>,
    faces: &[[u32; 3]],
) -> Result<Vec<[f32; 3]>> {
    faces
        .iter()
        .flatten()
        .map(|&index| position(model, vertices, posed, index))
        .collect()
}

/// Share of the aperture's smaller radius within which solid geometry lies in the aperture's
/// plane. The examined holographic housing's rim spans 1.0 to 1.8 mm behind a mask whose smaller
/// radius is 12.4 mm.
const PLANE: f32 = 0.2;

/// Whether a point lies inside or on a triangle of either winding.
fn inside(triangle: &[[f32; 2]; 3], point: [f32; 2]) -> bool {
    let edge = |a: [f32; 2], b: [f32; 2]| {
        (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0])
    };
    let sides = [
        edge(triangle[0], triangle[1]),
        edge(triangle[1], triangle[2]),
        edge(triangle[2], triangle[0]),
    ];
    sides.iter().all(|side| *side >= 0.) || sides.iter().all(|side| *side <= 0.)
}

impl Lens {
    /// Whether a point in aperture coordinates lies in the opening: inside every outline edge, or
    /// within the aperture radii when the outline is unsupported.
    fn within(&self, point: [f32; 2]) -> bool {
        if self.outline.is_empty() {
            (0..2).all(|k| point[k].abs() <= self.radii[k])
        } else {
            self.outline
                .iter()
                .all(|edge| edge[0] * point[0] + edge[1] * point[1] + edge[2] > 0.)
        }
    }

    /// Whether the sight axis passes through the opening.
    pub(super) fn aimed(&self) -> bool {
        let offset: [f32; 3] = std::array::from_fn(|a| self.aim[a] - self.center[a]);
        self.within(
            self.axes
                .map(|axis| (0..3).map(|a| axis[a] * offset[a]).sum::<f32>()),
        )
    }

    /// The share of the opening that the triangles cover, sampled on a grid across its radii.
    fn covered(&self, triangles: &[[[f32; 2]; 3]]) -> f32 {
        const STEPS: usize = 64;
        let bounds = triangles
            .iter()
            .map(|t| {
                let low: [f32; 2] = std::array::from_fn(|k| t[0][k].min(t[1][k]).min(t[2][k]));
                let high: [f32; 2] = std::array::from_fn(|k| t[0][k].max(t[1][k]).max(t[2][k]));
                (low, high)
            })
            .collect::<Vec<_>>();
        let (mut open, mut closed) = (0u32, 0u32);
        for row in 0..STEPS {
            for column in 0..STEPS {
                let point = [column, row].map(|step| (step as f32 + 0.5) / STEPS as f32 * 2. - 1.);
                let point = [point[0] * self.radii[0], point[1] * self.radii[1]];
                if !self.within(point) {
                    continue;
                }
                open += 1;
                if triangles
                    .iter()
                    .zip(&bounds)
                    .any(|(triangle, (low, high))| {
                        (0..2).all(|k| point[k] >= low[k] && point[k] <= high[k])
                            && inside(triangle, point)
                    })
                {
                    closed += 1;
                }
            }
        }
        if open == 0 {
            1.
        } else {
            closed as f32 / open as f32
        }
    }

    /// The triangles of a solid draw that frame the opening rather than block it, one flag per
    /// triangle of `points`. A source mask can reach past the glass onto the rim that holds it,
    /// and an opening cut along the mask's outline then shows a gap between the glass and that
    /// rim. Rim triangles lie in the aperture's plane and connect, through corners they share in
    /// that plane, to geometry outside the outline. They count only while they leave most of the
    /// opening open, so a display back that closes it is still cut.
    pub(super) fn frame(&self, points: &[[f32; 3]]) -> Vec<bool> {
        let mut flags = vec![false; points.len() / 3];
        if self.outline.is_empty() {
            return flags;
        }
        let reach = PLANE * self.radii[0].min(self.radii[1]);
        let local = |p: &[f32; 3]| {
            let offset: [f32; 3] = std::array::from_fn(|a| p[a] - self.center[a]);
            let along = |v: &[f32; 3]| (0..3).map(|a| v[a] * offset[a]).sum::<f32>();
            (
                [along(&self.axes[0]), along(&self.axes[1])],
                along(&self.normal),
            )
        };
        // Corners match by position, since a seam repeats a vertex.
        let corner = |p: &[f32; 3]| p.map(|v| (v * 100_000.).round() as i64);
        let mut planar = Vec::new();
        let mut shared = BTreeMap::<[i64; 3], Vec<usize>>::new();
        let mut pending = Vec::new();
        for (index, triangle) in points.chunks_exact(3).enumerate() {
            let corners = [
                local(&triangle[0]),
                local(&triangle[1]),
                local(&triangle[2]),
            ];
            if corners.iter().any(|(_, depth)| depth.abs() > reach) {
                continue;
            }
            let slot = planar.len();
            planar.push((index, corners.map(|(point, _)| point)));
            for p in triangle {
                shared.entry(corner(p)).or_default().push(slot);
            }
            if corners.iter().any(|(point, _)| !self.within(*point)) {
                pending.push(slot);
            }
        }
        let mut rim = vec![false; planar.len()];
        for &slot in &pending {
            rim[slot] = true;
        }
        while let Some(slot) = pending.pop() {
            let index = planar[slot].0;
            for p in &points[index * 3..index * 3 + 3] {
                for &next in &shared[&corner(p)] {
                    if !rim[next] {
                        rim[next] = true;
                        pending.push(next);
                    }
                }
            }
        }
        let triangles = planar
            .iter()
            .zip(&rim)
            .filter(|(_, rim)| **rim)
            .map(|((_, triangle), _)| *triangle)
            .collect::<Vec<_>>();
        if triangles.is_empty() || self.covered(&triangles) >= 0.5 {
            return flags;
        }
        for ((index, _), rim) in planar.iter().zip(&rim) {
            flags[*index] = *rim;
        }
        flags
    }

    pub(super) fn placed(&self, matrix: &[f32; 16]) -> Self {
        Self {
            center: transform(self.center, matrix, true),
            aim: transform(self.aim, matrix, true),
            axes: self.axes.map(|v| transform(v, matrix, false)),
            radii: self.radii,
            normal: transform(self.normal, matrix, false),
            artwork: self.artwork,
            outline: self.outline.clone(),
            enclosed: self.enclosed,
            window: self.window.clone(),
            framing: self.framing.clone(),
        }
    }
}

/// A vertex's model-space position, in its local skeleton's rest pose when that is posed.
fn position(
    model: &Payload,
    vertices: &Payload,
    posed: Option<&[[f32; 12]]>,
    index: u32,
) -> Result<[f32; 3]> {
    let mut result = [0.; 3];
    for (axis, value) in result.iter_mut().enumerate() {
        *value = (f32::from(vertices.i16(index as usize * 24 + axis * 2)?) / 32767.).max(-1.)
            * model.f32(0xA0 + axis * 4)?
            + model.f32(0xB0 + axis * 4)?;
    }
    Ok(match posed {
        Some(posed) => rig::apply(
            posed
                .get(index as usize)
                .context("Posed aperture vertex is absent")?,
            result,
            true,
        ),
        None => result,
    })
}

fn axis(owner: &Payload) -> Result<Option<[[f32; 3]; 2]>> {
    use crate::tiger::markers::{self, Layout};
    let data = owner.pointer(24)?;
    let mut points = Vec::new();
    for (offset, name) in [(0x2D0, 0x8D0DD3CD), (0x370, 0x0B7BA45D)] {
        let rows = markers::read(
            owner,
            data + offset,
            Layout {
                row_class: 0x8080BA9E,
                ..markers::SOURCE
            },
        )?;
        ensure!(rows.len() <= 1, "Ambiguous scope aim default");
        if let Some(row) = rows.first() {
            ensure!(
                row.binding == [0, 0] && row.name == name,
                "Unsupported scope aim binding"
            );
            points.push([row.position[0], row.position[1], row.position[2]]);
        }
    }
    if points.is_empty() {
        return Ok(None);
    }
    ensure!(
        points.len() == 2 && points[1][0] > points[0][0],
        "Incomplete scope aim axis"
    );
    Ok(Some(points.try_into().unwrap()))
}

type Axis = [[f32; 3]; 2];

/// Whether the entity has a sight, and the rear-to-front axis and angular override of the sight
/// that has one.
fn source_axis(reader: &mut Reader, entry: &Value) -> Result<(bool, Option<(Axis, f32)>)> {
    let entity = reader.tag(hash(entry, "entity")?, None)?;
    let mut optic = false;
    let mut sight_axis = None;
    for row in entity.array(8, 12, None)? {
        let owner = reader.tag(entity.u32(row)?, None)?;
        let data = owner.pointer(24)?;
        if owner.u32(data - 4)? == 0x80803E3C {
            optic = true;
            if let Some(points) = axis(&owner)? {
                ensure!(sight_axis.is_none(), "Ambiguous scope aim axis");
                sight_axis = Some((points, owner.f32(data + 0xE0)?));
            }
        }
    }
    Ok((optic, sight_axis))
}

pub(super) fn read(
    reader: &mut Reader,
    entry: &Value,
    model: &Payload,
    mesh: usize,
    parts: &[usize],
    streams: (&Payload, &[u32], usize),
    posed: Option<&[[f32; 12]]>,
) -> Result<Option<Lens>> {
    let (vertices, indices, width) = streams;
    let start = model.u16(mesh + 0x30 + 15 * 2)? as usize;
    let end = model.u16(mesh + 0x32 + 15 * 2)? as usize;
    let masks = parts.get(start..end).context("Scope mask range")?;
    if masks.is_empty() {
        return Ok(None);
    }
    let (optic, sight) = source_axis(reader, entry)?;
    if !optic {
        return Ok(None);
    }
    let sight_axis = sight.map(|(points, _)| points);
    let mut points = Vec::new();
    for &part in masks {
        if model.u8(part + 33)? > 3 {
            continue;
        }
        for triangle in faces(model, part, indices, width, vertices.0.len() / 24)? {
            for index in triangle {
                points.push(position(model, vertices, posed, index)?);
            }
        }
    }
    ensure!(
        !points.is_empty(),
        "Required source scope aperture is empty"
    );
    let lo = std::array::from_fn::<_, 3, _>(|a| {
        points.iter().map(|p| p[a]).fold(f32::INFINITY, f32::min)
    });
    let hi = std::array::from_fn::<_, 3, _>(|a| {
        points
            .iter()
            .map(|p| p[a])
            .fold(f32::NEG_INFINITY, f32::max)
    });
    let radii = [(hi[1] - lo[1]) * 0.5, (hi[2] - lo[2]) * 0.5];
    ensure!(
        radii.iter().all(|v| v.is_finite() && *v > 0.00001),
        "Degenerate source scope aperture"
    );
    let center = std::array::from_fn(|a| (lo[a] + hi[a]) * 0.5);
    let aim = sight_axis.map_or(center, |[rear, front]| {
        let t = (center[0] - rear[0]) / (front[0] - rear[0]);
        std::array::from_fn(|a| rear[a] + (front[a] - rear[a]) * t)
    });
    let slope = sight_axis.map_or([0.; 2], |[rear, front]| {
        [
            (front[1] - rear[1]) / (front[0] - rear[0]),
            (front[2] - rear[2]) / (front[0] - rear[0]),
        ]
    });
    let axes = [[-slope[0], 1., 0.], [-slope[1], 0., 1.]];
    let outline = outline(&points, center, axes)?;
    let normal = plane(&points, [1., slope[0], slope[1]])?;
    let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
    for index in 0..vertices.0.len() / 24 {
        let point = position(model, vertices, posed, u32::try_from(index)?)?;
        for a in 0..3 {
            bounds[0][a] = bounds[0][a].min(point[a]);
            bounds[1][a] = bounds[1][a].max(point[a]);
        }
    }
    let enclosed = sight_axis
        .is_some_and(|[rear, _]| (0..3).all(|a| (bounds[0][a]..=bounds[1][a]).contains(&rear[a])));
    let start = usize::from(model.u16(mesh + 0x30 + 17 * 2)?);
    let end = usize::from(model.u16(mesh + 0x32 + 17 * 2)?);
    let mut artwork = false;
    for &part in parts.get(start..end).context("Reticle draw range")? {
        if model.u8(part + 33)? <= 3
            && !faces(model, part, indices, width, vertices.0.len() / 24)?.is_empty()
        {
            artwork = true;
        }
    }
    let mut lens = Lens {
        center,
        aim,
        axes,
        radii,
        normal,
        artwork,
        outline,
        enclosed,
        window: Vec::new(),
        framing: None,
    };
    lens.window = sight_axis
        .and_then(|[rear, front]| sighted(&lens, rear, front))
        .map(|(_, _, window)| window)
        .unwrap_or_default();
    lens.framing = sight.and_then(|([rear, front], angle)| Framing::new(&lens, angle, rear, front));
    Ok(Some(lens))
}

/// A solid draw's triangles as the sets that become surfaces: its rim under an aperture cutout,
/// flagged, and the rest. A draw without a cutout or a rim is one set.
pub(super) fn split(
    cutout: Option<&Lens>,
    model: &Payload,
    vertices: &Payload,
    posed: Option<&[[f32; 12]]>,
    faces: Vec<[u32; 3]>,
) -> Result<Vec<(Vec<[u32; 3]>, bool)>> {
    let rim = match cutout {
        Some(lens) => lens.frame(&points(model, vertices, posed, &faces)?),
        None => Vec::new(),
    };
    let (frame, rest): (Vec<_>, Vec<_>) = faces
        .into_iter()
        .enumerate()
        .partition(|(index, _)| rim.get(*index).copied().unwrap_or(false));
    Ok([(frame, true), (rest, false)]
        .into_iter()
        .filter(|(faces, _)| !faces.is_empty())
        .map(|(faces, frame)| (faces.into_iter().map(|(_, face)| face).collect(), frame))
        .collect())
}

/// The unit normal of the mask's plane, from its own triangles with each oriented along the sight
/// direction `ahead`.
fn plane(points: &[[f32; 3]], ahead: [f32; 3]) -> Result<[f32; 3]> {
    let mut normal = [0.; 3];
    for triangle in points.chunks_exact(3) {
        let edges: [[f32; 3]; 2] =
            std::array::from_fn(|k| std::array::from_fn(|a| triangle[k + 1][a] - triangle[0][a]));
        let face: [f32; 3] = std::array::from_fn(|a| {
            edges[0][(a + 1) % 3] * edges[1][(a + 2) % 3]
                - edges[0][(a + 2) % 3] * edges[1][(a + 1) % 3]
        });
        let sign = if (0..3).map(|a| face[a] * ahead[a]).sum::<f32>() < 0. {
            -1.
        } else {
            1.
        };
        for a in 0..3 {
            normal[a] += face[a] * sign;
        }
    }
    let length = normal.iter().map(|v| v * v).sum::<f32>().sqrt();
    ensure!(
        length.is_finite() && length > 0.,
        "Source scope aperture has no plane"
    );
    Ok(normal.map(|v| v / length))
}

/// A convex, filled source mask supplies the opening, including its rounded corners. Other
/// profiles retain their original passes rather than cutting a bounding box through metal.
fn outline(points: &[[f32; 3]], center: [f32; 3], axes: [[f32; 3]; 2]) -> Result<Vec<[f32; 3]>> {
    let project =
        |p: &[f32; 3]| axes.map(|axis| (0..3).map(|i| axis[i] * (p[i] - center[i])).sum::<f32>());
    let cross = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let area = points
        .chunks_exact(3)
        .map(|t| cross(project(&t[0]), project(&t[1]), project(&t[2])).abs())
        .sum::<f32>();
    let mut vertices = points.iter().map(project).collect::<Vec<_>>();
    vertices.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    vertices.dedup();
    ensure!(
        vertices.len() >= 3,
        "Source aperture has no projected polygon"
    );
    let mut hull = Vec::new();
    let half = |input: Vec<[f32; 2]>| {
        let mut side = Vec::new();
        for p in input {
            while side.len() >= 2 && cross(side[side.len() - 2], side[side.len() - 1], p) <= 0. {
                side.pop();
            }
            side.push(p);
        }
        side.pop();
        side
    };
    hull.extend(half(vertices.clone()));
    vertices.reverse();
    hull.extend(half(vertices));
    if !(3..=64).contains(&hull.len()) {
        return Ok(Vec::new());
    }
    let hull_area = (1..hull.len() - 1)
        .map(|i| cross(hull[0], hull[i], hull[i + 1]))
        .sum::<f32>();
    if !area.is_finite() || area <= 0. || (area - hull_area).abs() > area * 0.001 {
        return Ok(Vec::new());
    }
    Ok((0..hull.len())
        .map(|i| {
            let a = hull[i];
            let b = hull[(i + 1) % hull.len()];
            let length = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let x = -(b[1] - a[1]) / length;
            let y = (b[0] - a[0]) / length;
            [x, y, -x * a[0] - y * a[1]]
        })
        .collect())
}
