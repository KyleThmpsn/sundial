//! A source optic's physical aperture mask locates its opening without material or item hashes.
use super::*;

#[derive(Clone)]
pub(crate) struct Lens {
    pub center: [f32; 3],
    pub aim: [f32; 3],
    pub axes: [[f32; 3]; 2],
    pub radii: [f32; 2],
    pub artwork: bool,
    pub outline: Vec<[f32; 3]>,
    /// The source eye lies within the bounds of the mesh that carries the aperture, as in the
    /// long-range sights whose eye sits inside their sight box. The source shows such a sight's
    /// aimed view through the aperture alone.
    pub enclosed: bool,
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
        Some(Self {
            tangent: (angle * 0.5).tan(),
            window,
            eye,
            origin: rear,
            forward: axis.map(|v| v / length),
            axes: lens.axes,
        })
    }
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

impl Lens {
    /// Whether the sight axis passes through the opening: inside every outline edge, or within
    /// the aperture radii when the outline is unsupported.
    pub(super) fn aimed(&self) -> bool {
        let offset: [f32; 3] = std::array::from_fn(|a| self.aim[a] - self.center[a]);
        let point = self
            .axes
            .map(|axis| (0..3).map(|a| axis[a] * offset[a]).sum::<f32>());
        if self.outline.is_empty() {
            (0..2).all(|k| point[k].abs() <= self.radii[k])
        } else {
            self.outline
                .iter()
                .all(|edge| edge[0] * point[0] + edge[1] * point[1] + edge[2] > 0.)
        }
    }

    pub(super) fn placed(&self, matrix: &[f32; 16]) -> Self {
        Self {
            center: transform(self.center, matrix, true),
            aim: transform(self.aim, matrix, true),
            axes: self.axes.map(|v| transform(v, matrix, false)),
            radii: self.radii,
            artwork: self.artwork,
            outline: self.outline.clone(),
            enclosed: self.enclosed,
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
        artwork,
        outline,
        enclosed,
        framing: None,
    };
    lens.framing = sight.and_then(|([rear, front], angle)| Framing::new(&lens, angle, rear, front));
    Ok(Some(lens))
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
