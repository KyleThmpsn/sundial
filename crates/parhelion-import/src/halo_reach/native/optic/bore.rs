//! Subtract a bounded optical tunnel from the configured scope's opaque surfaces.
use super::*;

#[derive(Clone, Copy)]
struct Vertex {
    position: [f32; 3],
    barycentric: [f32; 3],
}

fn distance(plane: [f32; 4], p: [f32; 3]) -> f32 {
    (0..3).map(|a| plane[a] * p[a]).sum::<f32>() + plane[3]
}

fn split(polygon: &[Vertex], plane: [f32; 4]) -> (Vec<Vertex>, Vec<Vertex>) {
    let mut inside = Vec::new();
    let mut outside = Vec::new();
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        let da = distance(plane, a.position);
        let db = distance(plane, b.position);
        if da >= 0. {
            inside.push(a);
        } else {
            outside.push(a);
        }
        if (da >= 0.) != (db >= 0.) {
            let t = da / (da - db);
            let intersection = Vertex {
                position: std::array::from_fn(|axis| {
                    a.position[axis] + t * (b.position[axis] - a.position[axis])
                }),
                barycentric: std::array::from_fn(|axis| {
                    a.barycentric[axis] + t * (b.barycentric[axis] - a.barycentric[axis])
                }),
            };
            inside.push(intersection);
            outside.push(intersection);
        }
    }
    (inside, outside)
}

fn area(a: Vertex, b: Vertex, c: Vertex) -> f32 {
    let u = std::array::from_fn::<_, 3, _>(|i| b.position[i] - a.position[i]);
    let v = std::array::from_fn::<_, 3, _>(|i| c.position[i] - a.position[i]);
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    cross.iter().map(|v| v * v).sum::<f32>().sqrt() * 0.5
}

fn vertex(mesh: &mut Mesh, face: [u32; 3], v: Vertex) -> Result<u32> {
    let index = u32::try_from(mesh.positions.len())?;
    let attributes = std::array::from_fn(|a| {
        (0..3)
            .map(|i| mesh.attributes[face[i] as usize][a] * v.barycentric[i])
            .sum()
    });
    if !mesh.weights.is_empty() {
        let mut weights = BTreeMap::<u8, f32>::new();
        for (i, source) in face.iter().enumerate() {
            let source = mesh.weights[*source as usize];
            for lane in 0..4 {
                let weight = f32::from(source[lane]) * v.barycentric[i];
                if weight > 0.0001 {
                    *weights.entry(source[lane + 4]).or_default() += weight;
                }
            }
        }
        ensure!(
            !weights.is_empty() && weights.len() <= 4,
            "Optic clipping requires at most four joint influences per intersection"
        );
        let sum = weights.values().sum::<f32>();
        let mut encoded = [0u8; 8];
        let mut largest = 0;
        for (i, (bone, weight)) in weights.into_iter().enumerate() {
            encoded[i] = (weight * 255. / sum).floor() as u8;
            encoded[i + 4] = bone;
            if encoded[i] > encoded[largest] {
                largest = i;
            }
        }
        encoded[largest] += (255 - encoded[..4].iter().map(|&w| u16::from(w)).sum::<u16>()) as u8;
        mesh.weights.push(encoded);
    }
    mesh.positions.push(v.position);
    mesh.attributes.push(attributes);
    Ok(index)
}

pub(super) fn open(
    mesh: &mut Mesh,
    lenses: &BTreeMap<u32, Lens>,
    rear: [f32; 3],
    front: [f32; 3],
    radii: [f32; 2],
    extent: [f32; 2],
) -> Result<Value> {
    const SIDES: usize = 24;
    // An inscribed opening leaves the source lens rim intact, including quantization error.
    let radii = radii.map(|r| r * 0.96);
    let length = front[0] - rear[0];
    ensure!(length > 0., "Optic tunnel has no forward extent");
    let mut planes = vec![
        [1., 0., 0., -(extent[0] - 0.002)],
        [-1., 0., 0., extent[1] + 0.002],
    ];
    for i in 0..SIDES {
        let angle = std::f32::consts::TAU * i as f32 / SIDES as f32;
        let [y, z] = [angle.cos(), angle.sin()];
        let radius_scale = (std::f32::consts::PI / SIDES as f32).cos();
        let slope = ((radii[1] - radii[0]) * radius_scale
            + y * (front[1] - rear[1])
            + z * (front[2] - rear[2]))
            / length;
        planes.push([
            slope,
            -y,
            -z,
            radii[0] * radius_scale + y * rear[1] + z * rear[2] - slope * rear[0],
        ]);
    }
    let mut removed = 0;
    let mut removed_area = 0.;
    for group in 0..mesh.groups.len() {
        if lenses.contains_key(&mesh.groups[group].0) {
            continue;
        }
        let mut output = Vec::new();
        for face in std::mem::take(&mut mesh.groups[group].1) {
            let mut inside = face
                .iter()
                .enumerate()
                .map(|(i, &index)| Vertex {
                    position: mesh.positions[index as usize],
                    barycentric: std::array::from_fn(|a| if i == a { 1. } else { 0. }),
                })
                .collect::<Vec<_>>();
            let mut fragments = Vec::new();
            for &plane in &planes {
                let (remaining, outside) = split(&inside, plane);
                inside = remaining;
                if outside.len() >= 3 {
                    fragments.push(outside);
                }
                if inside.len() < 3 {
                    break;
                }
            }
            let cut_area = (1..inside.len().saturating_sub(1))
                .map(|i| area(inside[0], inside[i], inside[i + 1]))
                .sum::<f32>();
            if cut_area <= 1e-12 {
                output.push(face);
                continue;
            }
            removed += 1;
            removed_area += cut_area;
            for polygon in fragments {
                for i in 1..polygon.len() - 1 {
                    let triangle = [polygon[0], polygon[i], polygon[i + 1]];
                    if area(triangle[0], triangle[1], triangle[2]) > 1e-12 {
                        output.push([
                            vertex(mesh, face, triangle[0])?,
                            vertex(mesh, face, triangle[1])?,
                            vertex(mesh, face, triangle[2])?,
                        ]);
                    }
                }
            }
        }
        mesh.groups[group].1 = output;
    }
    Ok(
        json!({"sides":SIDES,"radius_fraction":0.96,"changed_triangles":removed,"removed_area_square_metres":removed_area,"method":"Finite lens-aligned polygonal tunnel subtracted from non-lens triangles before surface and shadow emission"}),
    )
}
