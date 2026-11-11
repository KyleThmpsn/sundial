//! The private driver seat camera follows the source driver seat's camera track.
//!
//! Both engines keep the third-person seat camera on a track of points, each an offset from a
//! pivot paired with a view rotation, and look the track up by the view's pitch. The camera
//! distance is the length of the interpolated offset in both. A source `trak` holds control
//! points in world units from the seat camera marker, or from the vehicle origin when that marker
//! is empty or absent. The native rail `808066FB` holds `808066FD` points, a padded offset from
//! the camera event's locator at `+0x0` and a rotation at `+0x10`. Every native point keeps its
//! rotation and takes the source offset at that rotation's pitch, in native metres.
//!
//! The source lookup follows HREK `reach_tag_test.exe`. `515BE0` interpolates a four-point window
//! from one point before `floor((n - 1) t)`, clamped to the track, through the Newton cubic of
//! `517010` and `516EC0` at uniform knots `1 / (n - 1)`. Directions are each rotation applied to
//! the forward axis. Its caller `5168B4` bisects `t` over the track until the interpolated
//! direction has the requested pitch, then evaluates the offset there.
use super::{Scene, assembly::Assembly, secondary::Bundle};
use crate::{
    halo_reach::{native::seat, rig},
    presentation::{Graph, put},
    tiger::{markers, payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// The source seat flags of the driver and of a third-person seat camera.
const DRIVER: u64 = 1 << 2;
const THIRD_PERSON: u64 = 1 << 4;

/// One source control point, an offset in world units and the view's forward direction.
struct Point {
    offset: [f64; 3],
    forward: [f64; 3],
}

/// The view direction of a rotation, its turn of the forward axis.
fn forward([x, y, z, w]: [f32; 4]) -> [f64; 3] {
    let [x, y, z, w] = [x, y, z, w].map(f64::from);
    [
        1. - 2. * (y * y + z * z),
        2. * (x * y + w * z),
        2. * (x * z - w * y),
    ]
}

/// How far below the horizon a direction looks, in radians.
fn pitch_down(v: [f64; 3]) -> f64 {
    (-v[2]).atan2(v[0].hypot(v[1]))
}

/// The source cubic through four values at knots `start`, `start + step` and so on, at `t`.
fn cubic(values: [[f64; 3]; 4], start: f64, step: f64, t: f64) -> [f64; 3] {
    std::array::from_fn(|axis| {
        let [a, b, c, d] = values.map(|value| value[axis]);
        let (first, second, third) = (b - a, c - b, d - c);
        let (rise, next) = (second - first, third - second);
        let turn = next - rise;
        ((turn * (t - start - 2. * step) / (3. * step) + rise) * (t - start - step) / (2. * step)
            + first)
            * (t - start)
            / step
            + a
    })
}

/// A track lane at `t` from zero to one, through the window the source selects.
fn evaluate(track: &[Point], lane: fn(&Point) -> [f64; 3], t: f64) -> [f64; 3] {
    let last = track.len() - 1;
    let step = 1. / last as f64;
    let floor = (last as f64 * t) as usize;
    let start = floor.saturating_sub(1).min(track.len() - 4);
    let window = std::array::from_fn(|index| lane(&track[start + index]));
    cubic(window, start as f64 * step, step, t)
}

/// The source offset whose interpolated view looks `pitch` radians below the horizon.
fn offset(track: &[Point], pitch: f64) -> [f64; 3] {
    let (mut low, mut high, mut t) = (0., 1., 0.5);
    while high - low > 0.0001 {
        t = 0.5 * (low + high);
        if pitch_down(evaluate(track, |point| point.forward, t)) > pitch {
            low = t;
        } else {
            high = t;
        }
    }
    evaluate(track, |point| point.offset, t)
}

/// The driver seat's source camera: its pivot marker and its first track's control points, when
/// the seat has a third-person camera with a track the source can interpolate, or a marker the
/// camera sits on.
fn source(scene: &Scene) -> Result<Option<(String, Vec<Point>)>> {
    let seats = scene.object.gameplay["seats"]
        .as_array()
        .context("Source vehicle seats")?;
    let drivers = seats
        .iter()
        .filter(|seat| {
            seat["flags"]
                .as_u64()
                .is_some_and(|flags| flags & DRIVER != 0)
        })
        .collect::<Vec<_>>();
    let [driver] = drivers[..] else {
        return Ok(None);
    };
    let flags = driver["flags"]
        .as_u64()
        .context("Source driver seat flags")?;
    let mut camera = &driver["camera"];
    if flags & THIRD_PERSON == 0 || camera.is_null() {
        return Ok(None);
    }
    let seat_marker = camera["marker"].as_str().unwrap_or_default().to_owned();
    // A seat camera without tracks uses the vehicle's own unit camera (`vehi +0x198`), as the
    // Sabre's and Seraph's driver seats do. Kept on the carrier's camera, both sat far too close
    // to their much larger hulls (Dawn 1.7.2, 2026-11-06).
    if camera["tracks"].as_array().is_some_and(Vec::is_empty)
        && scene.object.gameplay["camera"]["tracks"]
            .as_array()
            .is_some_and(|tracks| !tracks.is_empty())
    {
        camera = &scene.object.gameplay["camera"];
    }
    // A camera with no track at all sits on its marker and looks along the unit's aim (Assembly
    // Reach `vehi.xml`, unit camera flag 3). The Sabre's and Seraph's driver cameras are such, on
    // `sabre_d_camera` and `driver_camera`, with no unit camera track either. Their rail keeps a
    // zero offset at every pitch, so each native point sits on the marker.
    if camera["tracks"].as_array().is_some_and(Vec::is_empty) && !seat_marker.is_empty() {
        let points = [-90f64, -30., 30., 90.]
            .map(|pitch| {
                let pitch = pitch.to_radians();
                Point {
                    offset: [0.; 3],
                    forward: [pitch.cos(), 0., pitch.sin()],
                }
            })
            .into_iter()
            .collect();
        return Ok(Some((seat_marker, points)));
    }
    let Some(points) = camera["tracks"]
        .as_array()
        .context("Source seat camera tracks")?
        .first()
        .map(|track| &track["control_points"])
    else {
        return Ok(None);
    };
    let points = points
        .as_array()
        .context("Source camera track points")?
        .iter()
        .map(|point| {
            let lane = |key: &str| -> Result<Vec<f32>> {
                point[key]
                    .as_array()
                    .context("Source camera track point")?
                    .iter()
                    .map(|value| {
                        value
                            .as_f64()
                            .map(|value| value as f32)
                            .context("Source camera track number")
                    })
                    .collect()
            };
            let offset: [f32; 3] = lane("position")?
                .try_into()
                .ok()
                .context("Source camera track position")?;
            let rotation = rig::quaternion(
                lane("orientation")?
                    .try_into()
                    .ok()
                    .context("Source camera track rotation")?,
            )?;
            Ok(Point {
                offset: offset.map(f64::from),
                forward: forward(rotation),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if points.len() < 4 {
        return Ok(None);
    }
    let marker = camera["marker"]
        .as_str()
        .context("Source seat camera marker")?;
    // The unit camera pivots on its own marker, or on the seat's when it names none.
    let marker = if marker.is_empty() {
        seat_marker
    } else {
        marker.to_owned()
    };
    Ok(Some((marker, points)))
}

/// The source pivot in native object space and metres: the named marker on the vehicle or on an
/// object attached to it, carried through each attachment, or the vehicle origin.
fn pivot(scene: &Scene, name: &str) -> Result<([f32; 3], Value)> {
    if !name.is_empty() {
        for (index, placed) in scene.models.iter().enumerate() {
            let found = seat::markers_in(&placed.model, name, &placed.model.selected)?;
            let [marker] = found[..] else {
                ensure!(
                    found.is_empty(),
                    "Source seat camera marker {name} is ambiguous"
                );
                continue;
            };
            let mut point = seat::marker_pose(&placed.model, marker)?.0;
            let mut at = index;
            while let Some(parent) = &scene.models[at].parent {
                point = rig::point(&parent.transform, point, true);
                if let Some(bone) = parent.bone {
                    let world = rig::world(&scene.models[parent.model].model.bones)?;
                    point = rig::point(
                        world.get(bone).context("Source attachment bone")?,
                        point,
                        true,
                    );
                }
                at = parent.model;
            }
            ensure!(
                point.iter().all(|value| value.is_finite()),
                "Source seat camera pivot is invalid"
            );
            return Ok((point, json!({"marker":name,"model":index})));
        }
    }
    // The source falls back to the vehicle origin for an empty or absent marker.
    Ok(([0.; 3], json!({"marker":name,"origin":true})))
}

/// The marker name a camera event's locator resolves to, or none for the owning entity's
/// transform (`FFFF`). The event's UInt16 at `+0x270` names one of its sequence's `80808617`
/// locators at definition `+0x1B0`, whose single `80808615` point row at `+0x1C0` names the marker
/// at `+0x8`.
fn locator(payload: &Payload, definition: usize, event: usize) -> Result<Option<u32>> {
    let ordinal = payload.u16(event + 0x270)?;
    if ordinal == u16::MAX {
        return Ok(None);
    }
    let locators = payload.array(definition + 0x1b0, 6, Some(0x80808617))?;
    let row = *locators
        .get(usize::from(ordinal))
        .context("Seat camera locator is outside its sequence")?;
    let points = payload.array(definition + 0x1c0, 16, Some(0x80808615))?;
    ensure!(
        payload.u8(row + 3)? == 1,
        "Seat camera locator has several point rows"
    );
    let point = *points
        .get(usize::from(payload.u8(row + 2)?))
        .context("Seat camera locator point is outside its sequence")?;
    Ok(Some(payload.u32(point + 8)?))
}

/// The carrier components whose instance struct has `class`.
fn owners(assembly: &Assembly, class: u32) -> Vec<u32> {
    assembly
        .payloads
        .iter()
        .filter_map(|(&owner, payload)| {
            payload
                .pointer(16)
                .ok()
                .and_then(|at| payload.u32(at - 4).ok())
                .filter(|&found| found == class)
                .map(|_| owner)
        })
        .collect()
}

/// Move the private carrier marker `name` onto the source pivot, keeping its bone and rotation,
/// so the rail turns about the source pivot.
fn place(assembly: &mut Assembly, bones: &Value, name: u32, pivot: [f32; 3]) -> Result<Value> {
    let sets = owners(assembly, markers::NATIVE_COMPONENT);
    let [owner] = sets[..] else {
        bail!("Vehicle camera marker set is absent or ambiguous");
    };
    let payload = assembly.payloads.get_mut(&owner).unwrap();
    let rows = markers::read_native(payload)?
        .into_iter()
        .filter(|row| row.name == name)
        .collect::<Vec<_>>();
    let [row] = rows[..] else {
        bail!("Seat camera locator marker {name:08X} is absent or ambiguous");
    };
    ensure!(
        payload.bytes::<8>(row.row + 8)? == [0; 8],
        "Seat camera marker binding extension differs"
    );
    let bone = usize::try_from(row.binding[1])?;
    ensure!(
        bone < bones["bones"]
            .as_array()
            .context("Seat camera marker skeleton")?
            .len(),
        "Seat camera marker bone is outside the native skeleton"
    );
    // A static carrier skeleton has no bind transforms, and its single root is the object frame.
    let local = if bones["bones"][bone]["inverse_object_transform"].is_null() {
        ensure!(
            bone == 0 && bones["static"] == true,
            "Seat camera marker bone has no bind transform"
        );
        pivot
    } else {
        seat::frame(bones, bone, (pivot, [0., 0., 0., 1.]))?.0
    };
    for (axis, value) in local.into_iter().enumerate() {
        put(
            &mut payload.0,
            row.row + markers::NATIVE.position + axis * 4,
            &value.to_le_bytes(),
        )?;
    }
    Ok(
        json!({"owner":format!("{owner:08X}"),"name":format!("{name:08X}"),"bone":bone,
        "previous_position":&row.position[..3],"position":local}),
    )
}

pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    assembly: &mut Assembly,
    scene: &Scene,
    bones: &Value,
) -> Result<Value> {
    let Some((marker, track)) = source(scene)? else {
        return Ok(json!({"retained":true,
            "limits":["The source driver seat has no third-person camera track, so the carrier camera is kept."]}));
    };
    if owners(assembly, 0x808042ad).is_empty() {
        return Ok(json!({"retained":true,
            "limits":["The carrier has no driver seat camera to follow the source track."]}));
    }
    let metres = f64::from(scene.metres());
    let (pivot, pivot_report) = pivot(scene, &marker)?;
    let driver = assembly.owner(0x808042ad)?;
    let payload = &assembly.payloads[&driver];
    let definition = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
    ensure!(
        payload.u32(definition - 4)? == 0x80803d7d,
        "Driver camera owner differs"
    );
    let secondary = payload.u32(definition + 0x248)?;
    let mut bundle = Bundle::read(native, secondary)?;
    let mut rails = BTreeMap::<u32, Option<u32>>::new();
    let mut moved = BTreeSet::new();
    let mut selected = Vec::new();
    for (&owner, payload) in &bundle.owners {
        let sequence = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
        if payload.u32(sequence - 4)? != 0x808084e9 {
            continue;
        }
        for row in payload.array(sequence + 0x168, 24, Some(0x808093e6))? {
            let node = payload.pointer(row + 16)?;
            if payload.u32(node - 4)? != 0x808066a3 {
                continue;
            }
            let camera = node + 0x38;
            let instance = usize::try_from(payload.u64(node + 8)?)?;
            ensure!(
                payload.u32(node + 4)? == 0x808066a2
                    && payload.u32(instance + 4)? == 0x808066a3
                    && payload.u64(instance + 8)? == node as u64
                    && node + 0x278 <= payload.0.len(),
                "Seat camera event pair differs"
            );
            let origin = locator(payload, sequence, node)?;
            if let Some(name) = origin {
                moved.insert(name);
            }
            // A named locator moves onto the source pivot, and the entity transform cannot, so its
            // rail carries the pivot itself.
            let base = if origin.is_some() { [0.; 3] } else { pivot };
            let references = payload.array(camera + 0xd8, 16, Some(0x808066fa))?;
            ensure!(!references.is_empty(), "Seat camera has no native rail");
            for reference in references {
                let tag = payload.u32(reference + 8)?;
                let symbol = format!("vehicle-camera-rail-{tag:08X}");
                if let Some(&previous) = rails.get(&tag) {
                    ensure!(
                        previous == origin,
                        "Seat camera rail {tag:08X} is shared by different locators"
                    );
                } else {
                    let mut rail = (*native.tag(tag, Some(0x808066fb))?).clone();
                    let points = rail.array(8, 32, Some(0x808066fd))?;
                    ensure!(points.len() >= 4, "Seat camera rail has too few points");
                    let mut written = Vec::new();
                    for &point in &points {
                        let rotation = (0..4)
                            .map(|axis| rail.f32(point + 16 + axis * 4))
                            .collect::<Result<Vec<_>>>()?;
                        ensure!(
                            (rotation.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 0.001,
                            "Seat camera rail orientation is invalid"
                        );
                        let pitch = pitch_down(forward([
                            rotation[0],
                            rotation[1],
                            rotation[2],
                            rotation[3],
                        ]));
                        let source = offset(&track, pitch);
                        let position: [f32; 3] =
                            std::array::from_fn(|axis| base[axis] + (source[axis] * metres) as f32);
                        ensure!(
                            position.iter().all(|value| value.is_finite()),
                            "Seat camera rail position is invalid"
                        );
                        for (axis, value) in position.iter().enumerate() {
                            put(&mut rail.0, point + axis * 4, &value.to_le_bytes())?;
                        }
                        written.push(json!({"pitch_degrees":(-pitch).to_degrees(),
                            "position":position,"distance":(source[0].hypot(source[1]).hypot(source[2]) * metres)}));
                    }
                    graph.add(&symbol, tag, &rail.0, None, Vec::new())?;
                    rails.insert(tag, origin);
                    selected.push(json!({"rail":format!("{tag:08X}"),"points":written}));
                }
                bundle
                    .resources
                    .entry(owner)
                    .or_default()
                    .insert(reference + 8, symbol);
                selected.push(json!({"owner":format!("{owner:08X}"),"event":node,
                    "camera":camera,"rail":format!("{tag:08X}"),
                    "locator":origin.map(|name| format!("{name:08X}"))}));
            }
        }
    }
    ensure!(!selected.is_empty(), "Driver has no reachable seat camera");
    let mut placed = Vec::new();
    for name in moved {
        placed.push(place(assembly, bones, name, pivot)?);
    }
    let symbol = bundle.emit(graph, "vehicle-driver")?;
    assembly.resource(driver, definition + 0x248, symbol)?;
    Ok(
        json!({"metres_per_source_unit":metres,"pivot":pivot,"source_pivot":pivot_report,
        "selections":selected,"locator_markers":placed,"gameplay_verified":false,
        "limits":["Rail offsets follow the source track at each native point's pitch. A rail on the entity transform turns about the vehicle origin, so a source pivot off the vertical axis through it is carried without its own turn.",
            "The moved locator marker also moves for any other consumer of that marker."]}),
    )
}
