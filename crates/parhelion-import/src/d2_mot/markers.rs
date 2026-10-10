//! Source attachment markers in native marker-set components.
//!
//! The runtime resolves named aim, muzzle and grip points through a part's marker
//! component and its resource registrations. Final authoring selects the source
//! set by art selector and position, writes its exact transforms, and relocates
//! the component's header, data, entity aliases and event connection together.
//! Missing components acquire validated native registrations and lookup metadata.
//!
//! The name-matching helpers remain available to intermediate retained-part
//! preparation. Final authoring replaces those provisional sets by placement.
use crate::d2_mot::payload::Payload;
use anyhow::{Result, ensure};
use std::collections::BTreeMap;
mod author;
pub(crate) mod bindings;
pub(crate) mod optics;
pub use author::author;
pub use author::replace;

/// Where a marker row keeps its transform and its name, and how far apart the rows are.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub stride: usize,
    /// The root binding and binding mode, as two native-width words.
    pub binding: usize,
    /// The rotation, a unit quaternion in x, y, z, w order. A marker is a transform, not a
    /// point: a weapon can carry two rows of one name at one place facing different ways.
    pub orientation: usize,
    pub position: usize,
    pub name: usize,
    pub row_class: u32,
}

/// Shadowkeep. The marker array descriptor sits at a fixed place in the component's data
/// struct, which is where this differs from the source era rather than in the rows.
pub const NATIVE: Layout = Layout {
    stride: 64,
    binding: 0,
    orientation: 0x10,
    position: 0x20,
    name: 0x30,
    row_class: 0x8080_8513,
};

/// The current game.
pub const SOURCE: Layout = Layout {
    stride: 48,
    binding: 0x20,
    orientation: 0x00,
    position: 0x10,
    name: 0x28,
    row_class: 0x8080_81A9,
};

/// The marker-set component, by the class of its instance struct.
pub const NATIVE_COMPONENT: u32 = 0x8080_8506;
/// The array descriptor, relative to the component's data struct.
const NATIVE_DESCRIPTOR: usize = 0xB0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marker {
    pub name: u32,
    pub binding: [u32; 2],
    pub position: [f32; 4],
    /// The way the marker faces. Carrying a position without this leaves an imported weapon
    /// emitting and aiming along the donor's axes from the source's point.
    pub orientation: [f32; 4],
    /// Where the row begins, so a rewrite can put a transform back where it came from.
    pub row: usize,
}

/// Every marker in one marker-set payload. `descriptor` is the array descriptor's offset.
pub fn read(payload: &Payload, descriptor: usize, layout: Layout) -> Result<Vec<Marker>> {
    let rows = payload.array(descriptor, layout.stride, Some(layout.row_class))?;
    rows.into_iter()
        .map(|row| {
            let mut position = [0.0; 4];
            for (index, value) in position.iter_mut().enumerate() {
                *value = payload.f32(row + layout.position + index * 4)?;
            }
            // Every marker row observed carries a point, never a direction.
            ensure!(
                (position[3] - 1.0).abs() < 1e-3,
                "marker row at {row:X} is not a position"
            );
            let mut orientation = [0.0; 4];
            for (index, value) in orientation.iter_mut().enumerate() {
                *value = payload.f32(row + layout.orientation + index * 4)?;
            }
            // A rotation that is not a unit quaternion means the offset is wrong, which is
            // worth failing on rather than writing a nonsense transform onto the carrier.
            let norm = orientation.iter().map(|value| value * value).sum::<f32>();
            ensure!(
                (norm - 1.0).abs() < 1e-2,
                "marker row at {row:X} has no rotation at {:#X}",
                layout.orientation
            );
            Ok(Marker {
                name: payload.u32(row + layout.name)?,
                binding: [
                    payload.u32(row + layout.binding)?,
                    payload.u32(row + layout.binding + 4)?,
                ],
                position,
                orientation,
                row,
            })
        })
        .collect()
}

/// The markers of a native marker-set component, found through its own data struct.
pub fn read_native(payload: &Payload) -> Result<Vec<Marker>> {
    let data = payload.pointer(0x18)?;
    ensure!(data >= 4, "marker component has no data struct");
    read(payload, data + NATIVE_DESCRIPTOR, NATIVE)
}

/// The markers of a source marker-set payload. Unlike the native component, the descriptor's
/// place is not fixed, so it is found by its row class. More than one candidate is refused
/// rather than guessed between.
pub fn read_source(payload: &Payload) -> Result<Vec<Marker>> {
    let mut found: Option<Vec<Marker>> = None;
    for descriptor in (0..payload.0.len().saturating_sub(16)).step_by(8) {
        crate::cancellation::check()?;
        let Ok(markers) = read(payload, descriptor, SOURCE) else {
            continue;
        };
        if markers.is_empty() {
            continue;
        }
        ensure!(
            found.replace(markers).is_none(),
            "this payload holds more than one marker array"
        );
    }
    found.ok_or_else(|| anyhow::anyhow!("this payload holds no marker array"))
}

/// What moving the source's markers onto the carrier would change.
/// One marker the carrier keeps and the source also names, with the transform it takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Carried {
    pub name: u32,
    pub binding: [u32; 2],
    pub position: [f32; 4],
    pub orientation: [f32; 4],
}

#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    /// Markers present in both, with the transform the native row takes.
    pub matched: Vec<Carried>,
    /// Named in the source and absent from the carrier, appended as new rows. A modular donor
    /// keeps markers such as `grip` and `iron_sight` on parts the import leaves out.
    pub added: Vec<Carried>,
    /// Named on the carrier and absent from the source. Left as the carrier wrote them.
    pub carrier_only: Vec<u32>,
}

/// Match by name. Nothing is matched by position, order or index: two weapons agree about
/// what `iron_sight` means and about nothing else.
pub fn plan(carrier: &[Marker], source: &[Marker]) -> Plan {
    let source_by_name = source
        .iter()
        .map(|marker| (marker.name, *marker))
        .collect::<BTreeMap<_, _>>();
    let carrier_names = carrier
        .iter()
        .map(|marker| marker.name)
        .collect::<std::collections::BTreeSet<_>>();
    let mut matched = Vec::new();
    let mut carrier_only = Vec::new();
    for marker in carrier {
        match source_by_name.get(&marker.name) {
            Some(source) => matched.push(Carried {
                name: marker.name,
                binding: source.binding,
                position: source.position,
                orientation: source.orientation,
            }),
            None => carrier_only.push(marker.name),
        }
    }
    matched.dedup_by_key(|carried| carried.name);
    carrier_only.dedup();
    let added = source
        .iter()
        .filter(|marker| !carrier_names.contains(&marker.name))
        .map(|marker| Carried {
            name: marker.name,
            binding: marker.binding,
            position: marker.position,
            orientation: marker.orientation,
        })
        .collect();
    Plan {
        matched,
        added,
        carrier_only,
    }
}

/// Write the matched transforms into a copy of the carrier's payload and append the markers
/// only the source names. The rows of markers the source does not name are left alone.
pub fn rewrite(payload: &Payload, source: &[Marker]) -> Result<(Payload, Plan)> {
    let carrier = read_native(payload)?;
    let plan = plan(&carrier, source);
    // A name can mark several points, such as three muzzle ports. Each carrier row takes the
    // source row of the same name and occurrence, and a carrier with more rows repeats the last.
    let mut by_name = BTreeMap::<u32, Vec<&Marker>>::new();
    for marker in source {
        by_name.entry(marker.name).or_default().push(marker);
    }
    let mut seen = BTreeMap::<u32, usize>::new();
    let mut out = payload.clone();
    for marker in &carrier {
        let Some(rows) = by_name.get(&marker.name) else {
            continue;
        };
        let occurrence = seen.entry(marker.name).or_default();
        let entry = rows[(*occurrence).min(rows.len() - 1)];
        *occurrence += 1;
        write_binding(
            &mut out.0[marker.row..marker.row + NATIVE.stride],
            entry.binding,
        )?;
        for (at, values) in [
            (NATIVE.position, entry.position),
            (NATIVE.orientation, entry.orientation),
        ] {
            for (index, value) in values.iter().enumerate() {
                let at = marker.row + at + index * 4;
                out.0[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    if !plan.added.is_empty() {
        append(&mut out, &plan.added)?;
    }
    // The carrier's rows keep their names and order, followed by the added markers.
    let after = read_native(&out)?;
    ensure!(
        after.len() == carrier.len() + plan.added.len()
            && after.iter().zip(&carrier).all(|(a, b)| a.name == b.name)
            && after[carrier.len()..]
                .iter()
                .zip(&plan.added)
                .all(|(a, b)| {
                    a.name == b.name
                        && a.binding == b.binding
                        && a.position == b.position
                        && a.orientation == b.orientation
                }),
        "marker rewrite changed the component's shape"
    );
    Ok((out, plan))
}

fn write_binding(row: &mut [u8], binding: [u32; 2]) -> Result<()> {
    ensure!(
        binding[0] == 0 && binding[1] <= 1,
        "source marker has an unsupported root binding"
    );
    ensure!(
        row[8..16] == [0; 8],
        "native marker binding extension differs"
    );
    row[..4].copy_from_slice(&binding[0].to_le_bytes());
    row[4..8].copy_from_slice(&binding[1].to_le_bytes());
    Ok(())
}

/// Grow the marker array by moving it to the end of the payload with the added rows.
///
/// The rows are followed by the sight-and-fire references, which point backwards into the
/// component, so the array is not grown in place. The descriptor is repointed, the old rows
/// stay as unreferenced bytes and every other offset in the component keeps its meaning.
fn append(payload: &mut Payload, added: &[Carried]) -> Result<()> {
    let data = payload.pointer(0x18)?;
    let descriptor = data + NATIVE_DESCRIPTOR;
    let mut rows = Vec::new();
    for row in payload.array(descriptor, NATIVE.stride, Some(NATIVE.row_class))? {
        rows.extend_from_slice(&payload.0[row..row + NATIVE.stride]);
    }
    for marker in added {
        let mut row = [0u8; NATIVE.stride];
        write_binding(&mut row, marker.binding)?;
        for (at, values) in [
            (NATIVE.orientation, marker.orientation),
            (NATIVE.position, marker.position),
        ] {
            for (index, value) in values.iter().enumerate() {
                row[at + index * 4..at + index * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        row[NATIVE.name..NATIVE.name + 4].copy_from_slice(&marker.name.to_le_bytes());
        rows.extend_from_slice(&row);
    }
    let count = (rows.len() / NATIVE.stride) as u64;
    let bytes = &mut payload.0;
    // Array headers are 16-byte aligned with their marker word just before them.
    let header = (bytes.len() + 19) & !15;
    bytes.resize(header - 4, 0);
    bytes.extend(0x8080_9FBDu32.to_le_bytes());
    bytes.extend(count.to_le_bytes());
    bytes.extend(u64::from(NATIVE.row_class).to_le_bytes());
    bytes.extend(rows);
    let relative = i64::try_from(header)? - i64::try_from(descriptor + 8)?;
    bytes[descriptor..descriptor + 8].copy_from_slice(&count.to_le_bytes());
    bytes[descriptor + 8..descriptor + 16].copy_from_slice(&relative.to_le_bytes());
    let size = bytes.len() as u64;
    bytes[0..8].copy_from_slice(&size.to_le_bytes());
    Ok(())
}

/// The marker sets of a source export's `raw` directory, one per resource that holds one. A
/// modern weapon keeps a set per part, and one name can mark a different point on each part:
/// the receiver's `primary_trigger` is not the barrel's.
pub fn read_export(raw: &std::path::Path) -> Result<Vec<Vec<Marker>>> {
    let mut entries = std::fs::read_dir(raw)?
        .map(|entry| Ok(entry?.path()))
        .collect::<Result<Vec<_>>>()?;
    entries.sort();
    let mut sets = Vec::new();
    for path in entries {
        crate::cancellation::check()?;
        if path.extension().is_none_or(|extension| extension != "bin") {
            continue;
        }
        let payload = Payload(std::fs::read(&path)?);
        if let Ok(found) = read_source(&payload) {
            sets.push(found);
        }
    }
    Ok(sets)
}

/// A gear view owns one art entity. Its raw directory also contains shared
/// runtime and character dependencies, whose markers must not be transplanted
/// onto this model because they happen to share an attachment name.
fn source_sets(source: &std::path::Path) -> Result<Vec<Vec<Marker>>> {
    use anyhow::Context;
    let report_path = source.join("report.json");
    if !report_path.exists() {
        return read_export(&source.join("raw"));
    }
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(&report_path)?)?;
    let Some(entity) = report.get("independent_art_entity") else {
        return read_export(&source.join("raw"));
    };
    let entity = u32::from_str_radix(entity.as_str().context("Source art entity")?, 16)?;
    let entity = Payload(std::fs::read(source.join(format!("raw/{entity:08X}.bin")))?);
    let mut sets = Vec::new();
    for row in entity.array(8, 12, Some(0x80809ACD))? {
        crate::cancellation::check()?;
        let owner = entity.u32(row)?;
        let payload = Payload(std::fs::read(source.join(format!("raw/{owner:08X}.bin")))?);
        if let Ok(markers) = read_source(&payload) {
            sets.push(markers);
        }
    }
    ensure!(
        sets.len() <= 1,
        "Source art entity has ambiguous marker sets"
    );
    Ok(sets)
}

/// The source part whose marker set names the most of the carrier's markers, so every carried
/// point comes from one part. Nothing shared means no part corresponds to the carrier.
pub fn corresponding<'a>(carrier: &[Marker], sets: &'a [Vec<Marker>]) -> Option<&'a [Marker]> {
    let names = carrier
        .iter()
        .map(|marker| marker.name)
        .collect::<std::collections::BTreeSet<_>>();
    sets.iter()
        .map(|set| {
            let shared = set
                .iter()
                .map(|marker| marker.name)
                .filter(|name| names.contains(name))
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            (shared, set)
        })
        .filter(|(shared, _)| *shared > 0)
        .max_by_key(|(shared, set)| (*shared, std::cmp::Reverse(set.len())))
        .map(|(_, set)| set.as_slice())
}

/// Move the source weapon's markers onto the carrier's marker set, in place.
///
/// Returns the native tag the entity names, the offset in the entity that names it, the
/// rewritten component and what changed. `None` means there is nothing to carry: either the
/// export has no markers, the entity has no marker set, or the two weapons share no name.
pub fn carry(
    reader: &mut crate::d2_mot::reader::Reader,
    source: &std::path::Path,
    entity: &Payload,
) -> Result<Option<(u32, usize, Payload, Plan)>> {
    let source_sets = source_sets(source)?;
    if source_sets.is_empty() {
        return Ok(None);
    }
    let mut found = None;
    for row in entity.array(0x10, 12, None)? {
        crate::cancellation::check()?;
        let tag = entity.u32(row)?;
        let Ok(payload) = reader.tag(tag, Some(0x8080_9C36)) else {
            continue;
        };
        let Ok(header) = payload.pointer(0x10) else {
            continue;
        };
        if header < 4 || payload.u32(header - 4)? != NATIVE_COMPONENT {
            continue;
        }
        ensure!(
            found.is_none(),
            "this entity carries more than one marker set"
        );
        found = Some((tag, row, Payload(payload.0.clone())));
    }
    let Some((tag, row, payload)) = found else {
        return Ok(None);
    };
    let Some(source_markers) = corresponding(&read_native(&payload)?, &source_sets) else {
        return Ok(None);
    };
    let (rewritten, plan) = rewrite(&payload, source_markers)?;
    if plan.matched.is_empty() && plan.added.is_empty() {
        return Ok(None);
    }
    Ok(Some((tag, row, rewritten, plan)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One marker-set payload: a descriptor, the array header it points at, and the rows.
    /// `data_struct` is where a native component's data begins, which is what fixes the
    /// descriptor's place; the source era is read from an explicit descriptor instead.
    /// An unturned marker. Rows need a real rotation now, so a fixture that leaves it zero
    /// would be rejected as a wrong offset rather than read as "facing forward".
    const FORWARD: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
    /// A quarter turn about the vertical, the shape a real turned row has.
    const TURNED: [f32; 4] = [
        0.0,
        std::f32::consts::FRAC_1_SQRT_2,
        0.0,
        std::f32::consts::FRAC_1_SQRT_2,
    ];

    fn component(
        markers: &[(u32, [f32; 3], [f32; 4])],
        layout: Layout,
        data_struct: Option<usize>,
    ) -> Payload {
        let descriptor = match data_struct {
            Some(base) => base + NATIVE_DESCRIPTOR,
            None => 0x40,
        };
        let header = descriptor + 0x30;
        let rows = header + 16;
        let mut data = vec![0u8; rows + layout.stride * markers.len()];
        let count = markers.len() as u64;
        data[descriptor..descriptor + 8].copy_from_slice(&count.to_le_bytes());
        let delta = header as i64 - (descriptor + 8) as i64;
        data[descriptor + 8..descriptor + 16].copy_from_slice(&delta.to_le_bytes());
        data[header..header + 8].copy_from_slice(&count.to_le_bytes());
        data[header + 8..header + 12].copy_from_slice(&layout.row_class.to_le_bytes());
        for (index, (name, position, orientation)) in markers.iter().enumerate() {
            let row = rows + index * layout.stride;
            for (axis, value) in position.iter().chain(std::iter::once(&1.0)).enumerate() {
                let at = row + layout.position + axis * 4;
                data[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            for (axis, value) in orientation.iter().enumerate() {
                let at = row + layout.orientation + axis * 4;
                data[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            let at = row + layout.name;
            data[at..at + 4].copy_from_slice(&name.to_le_bytes());
        }
        if let Some(base) = data_struct {
            let delta = base as i64 - 0x18;
            data[0x18..0x20].copy_from_slice(&delta.to_le_bytes());
        }
        Payload(data)
    }

    fn native(markers: &[(u32, [f32; 3], [f32; 4])]) -> Payload {
        component(markers, NATIVE, Some(0x20))
    }

    fn source(markers: &[(u32, [f32; 3], [f32; 4])]) -> Payload {
        component(markers, SOURCE, None)
    }

    const IRON_SIGHT: u32 = 0x8D0D_D3CD;
    const PRIMARY_FIRE: u32 = 0xF2A0_71CC;
    const GRIP: u32 = 0x2E08_5C75;

    /// The point of the pass: a marker both weapons name takes the source's transform, one the
    /// carrier alone names is left alone, and one only the source names is appended.
    #[test]
    fn source_markers_are_carried_and_added_by_name() {
        let carrier = native(&[
            (GRIP, [-0.1, -0.04, 0.03], FORWARD),
            (PRIMARY_FIRE, [0.636, -0.00137, 0.124], FORWARD),
        ]);
        let source = source(&[
            (PRIMARY_FIRE, [0.613, -0.00137, 0.099], TURNED),
            (IRON_SIGHT, [-0.215, 0.0, 0.192], FORWARD),
        ]);
        let source_markers = read(&source, 0x40, SOURCE).unwrap();
        assert_eq!(source_markers.len(), 2);
        let (rewritten, plan) = rewrite(&carrier, &source_markers).unwrap();
        assert_eq!(plan.matched.len(), 1);
        assert_eq!(plan.matched[0].name, PRIMARY_FIRE);
        assert_eq!(plan.carrier_only, vec![GRIP]);
        assert_eq!(plan.added.len(), 1);
        assert_eq!(plan.added[0].name, IRON_SIGHT);

        let after = read_native(&rewritten).unwrap();
        let fire = after.iter().find(|m| m.name == PRIMARY_FIRE).unwrap();
        assert!(
            (fire.position[0] - 0.613).abs() < 1e-6,
            "{:?}",
            fire.position
        );
        assert!((fire.position[2] - 0.099).abs() < 1e-6);
        // A marker is a transform. Carrying the point but not the facing would leave the
        // weapon emitting along the donor's axes from the source's position.
        assert_eq!(fire.orientation, TURNED);
        // The carrier's own marker is untouched.
        let grip = after.iter().find(|m| m.name == GRIP).unwrap();
        assert!((grip.position[0] + 0.1).abs() < 1e-6);
        assert_eq!(grip.orientation, FORWARD);
        // The source's sight is now a marker the runtime can find, at the source's point.
        let sight = after.iter().find(|m| m.name == IRON_SIGHT).unwrap();
        assert!((sight.position[0] + 0.215).abs() < 1e-6);
        assert!((sight.position[2] - 0.192).abs() < 1e-6);
        // Everything the original array was followed by is still where it was.
        let tail = read_native(&carrier).unwrap().last().unwrap().row + NATIVE.stride;
        assert_eq!(rewritten.0[tail..carrier.0.len()], carrier.0[tail..]);
    }

    /// The synthetic rows prove the arithmetic. This proves the layout: a real carrier and a
    /// real source, read from the packages and an export, agreeing about a marker by name.
    /// `PARHELION_MARKER_NATIVE_PACKAGES` is a Shadowkeep package directory and
    /// `PARHELION_MARKER_SOURCE_RAW` a source export's `raw` directory.
    #[test]
    #[ignore = "requires PARHELION_MARKER_NATIVE_PACKAGES and PARHELION_MARKER_SOURCE_RAW"]
    fn a_real_carrier_and_source_agree_about_a_marker_by_name() {
        let packages =
            std::path::PathBuf::from(std::env::var_os("PARHELION_MARKER_NATIVE_PACKAGES").unwrap());
        let raw =
            std::path::PathBuf::from(std::env::var_os("PARHELION_MARKER_SOURCE_RAW").unwrap());
        let scratch = tempfile::tempdir().unwrap();
        let mut reader =
            crate::d2_mot::reader::Reader::new(&packages, scratch.path(), false).unwrap();
        // Chroma Rush's carrier keeps this marker set; the import leaves it native today.
        let carrier = reader.tag(0x8161_ECC9, Some(0x8080_9C36)).unwrap();
        let carrier = Payload(carrier.0.clone());
        let carrier_markers = read_native(&carrier).unwrap();
        assert!(!carrier_markers.is_empty());

        let mut source_markers = Vec::new();
        for entry in std::fs::read_dir(&raw).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|ext| ext != "bin") {
                continue;
            }
            let payload = Payload(std::fs::read(&path).unwrap());
            if let Ok(markers) = read_source(&payload) {
                source_markers.extend(markers);
            }
        }
        assert!(!source_markers.is_empty(), "no source markers were found");

        let (rewritten, plan) = rewrite(&carrier, &source_markers).unwrap();
        assert!(
            !plan.matched.is_empty(),
            "carrier {:08X?} source {:08X?}",
            carrier_markers.iter().map(|m| m.name).collect::<Vec<_>>(),
            source_markers.iter().map(|m| m.name).collect::<Vec<_>>()
        );
        // Every matched row now holds the source's position, and nothing else moved.
        let after = read_native(&rewritten).unwrap();
        for carried in &plan.matched {
            let expected = source_markers
                .iter()
                .find(|marker| marker.name == carried.name)
                .unwrap();
            let row = after.iter().find(|m| m.name == carried.name).unwrap();
            assert_eq!(row.position, expected.position);
            assert_eq!(row.orientation, expected.orientation);
            assert_eq!(row.binding, expected.binding);
        }
        for marker in &carrier_markers {
            if !source_markers
                .iter()
                .any(|source| source.name == marker.name)
            {
                let row = after.iter().find(|m| m.name == marker.name).unwrap();
                assert_eq!(row.position, marker.position);
            }
        }
        println!(
            "matched {:08X?}, carrier only {:08X?}, added {:08X?}",
            plan.matched
                .iter()
                .map(|carried| carried.name)
                .collect::<Vec<_>>(),
            plan.carrier_only,
            plan.added
                .iter()
                .map(|added| added.name)
                .collect::<Vec<_>>()
        );
    }

    /// A row whose fourth component is not one is a direction or something else again, and
    /// carrying a position onto it would be meaningless.
    #[test]
    fn a_row_that_is_not_a_position_is_refused() {
        let mut payload = native(&[(GRIP, [0.0, 0.0, 0.0], FORWARD)]);
        let row = read_native(&payload).unwrap()[0].row;
        let at = row + NATIVE.position + 12;
        payload.0[at..at + 4].copy_from_slice(&0.0f32.to_le_bytes());
        assert!(read_native(&payload).is_err());
    }

    /// A row whose rotation is not a unit quaternion means the orientation offset is wrong.
    /// Writing that onto a carrier would point the weapon nowhere, so it is refused.
    #[test]
    fn a_row_without_a_real_rotation_is_refused() {
        let mut payload = native(&[(GRIP, [0.0, 0.0, 0.0], FORWARD)]);
        let row = read_native(&payload).unwrap()[0].row;
        let at = row + NATIVE.orientation + 12;
        payload.0[at..at + 4].copy_from_slice(&0.0f32.to_le_bytes());
        assert!(read_native(&payload).is_err());
    }
}
