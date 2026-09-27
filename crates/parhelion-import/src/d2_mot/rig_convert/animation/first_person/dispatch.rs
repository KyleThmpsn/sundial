//! Pose dispatch uses bank descriptor ordinals, including newly imported actions.
//!
//! Native 104CC70 indexes the signed-short table at +28 without a bounds check.
//! FFFF means no supplemental pose. Shared named actions calibrate layer-choice
//! correspondence. Unknown source layer operations remain explicit omissions.
use super::*;
use crate::d2_mot::{payload::Payload, rig_convert::write_array};

type Edge = (u16, u16);
type Route = [Vec<Edge>; 2];
type Role = (usize, u32, u16);

struct Table {
    layers: [Vec<(u32, usize)>; 2],
    routes: Vec<Route>,
    indices: Vec<u16>,
}

impl Table {
    fn read(p: &Payload, modern: bool) -> Result<Self> {
        let mut layers = [Vec::new(), Vec::new()];
        for (family, output) in layers.iter_mut().enumerate() {
            let (stride, class) = match (modern, family) {
                (true, 0) => (32, 0x808028F9),
                (true, _) => (24, 0x808028F6),
                (false, 0) => (24, 0x80803724),
                (false, _) => (24, 0x80803721),
            };
            let mut names = BTreeSet::new();
            for row in p.array(8 + family * 16, stride, Some(class))? {
                let name = p.u32(row + 16)?;
                ensure!(names.insert(name), "pose layer names repeat");
                output.push((name, usize::try_from(p.u64(row)?)?));
            }
        }
        let mut routes = Vec::new();
        for row in p.array(56, 32, Some(if modern { 0x808026A0 } else { 0x808034EF }))? {
            let mut route = [Vec::new(), Vec::new()];
            for family in 0..2 {
                for at in p.array(
                    row + family * 16,
                    4,
                    Some(if modern { 0x808026A2 } else { 0x808034F1 }),
                )? {
                    let choice = p.u16(at)?;
                    let layer = p.u16(at + 2)?;
                    let (_, count) = layers[family]
                        .get(usize::from(layer))
                        .context("pose route layer is outside its table")?;
                    ensure!(
                        usize::from(choice) < *count && choice <= i16::MAX as u16,
                        "pose route choice is outside its layer"
                    );
                    route[family].push((choice, layer));
                }
            }
            routes.push(route);
        }
        let indices = p
            .array(40, 2, Some(0x80800006))?
            .iter()
            .map(|&at| p.u16(at))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            indices.iter().all(|&index| index == u16::MAX
                || (index <= i16::MAX as u16 && usize::from(index) < routes.len())),
            "pose dispatch addresses an absent route"
        );
        Ok(Self {
            layers,
            routes,
            indices,
        })
    }

    fn route(&self, descriptor: usize) -> Result<Option<&Route>> {
        let index = *self
            .indices
            .get(descriptor)
            .context("pose descriptor exceeds dispatch table")?;
        Ok((index != u16::MAX).then(|| &self.routes[usize::from(index)]))
    }

    fn roles(&self, route: &Route) -> Vec<Role> {
        route
            .iter()
            .enumerate()
            .flat_map(|(family, edges)| {
                edges.iter().map(move |&(choice, layer)| {
                    (family, self.layers[family][usize::from(layer)].0, choice)
                })
            })
            .collect()
    }
}

fn names(bank: &Payload, modern: bool) -> Result<BTreeMap<u32, Vec<usize>>> {
    let mut names = BTreeMap::<u32, Vec<usize>>::new();
    for (index, at) in bank
        .array(
            if modern { 0x58 } else { 0x68 },
            if modern { 48 } else { 32 },
            Some(if modern { 0x80808BDE } else { 0x80809002 }),
        )?
        .into_iter()
        .enumerate()
    {
        names.entry(bank.u32(at + 16)?).or_default().push(index);
    }
    Ok(names)
}

/// Check the actual consumer domain before an authored controller can be linked.
pub fn validate(table: &Payload, bank: &Payload) -> Result<()> {
    let table = Table::read(table, false)?;
    ensure!(
        table.indices.len() == bank.array(0x68, 32, Some(0x80809002))?.len(),
        "pose dispatch does not cover the animation bank descriptors"
    );
    Ok(())
}

fn calibration(
    source: &Table,
    native: &Table,
    source_bank: &Payload,
    native_bank: &Payload,
) -> Result<BTreeMap<Role, BTreeSet<u16>>> {
    let source_names = names(source_bank, true)?;
    let native_names = names(native_bank, false)?;
    let mut evidence = BTreeMap::<Role, BTreeSet<u16>>::new();
    // Observe named layer choices in paired, unique actions. An equal index
    // alone does not establish the same operation across versions.
    for (name, from) in &source_names {
        let Some(to) = native_names.get(name) else {
            continue;
        };
        let ([from], [to]) = (from.as_slice(), to.as_slice()) else {
            continue;
        };
        let (Some(from), Some(to)) = (source.route(*from)?, native.route(*to)?) else {
            continue;
        };
        let source_roles = source.roles(from);
        let native_roles = native.roles(to);
        for role in &source_roles {
            let matches = native_roles
                .iter()
                .filter(|r| r.0 == role.0 && r.1 == role.1)
                .collect::<Vec<_>>();
            if source_roles
                .iter()
                .filter(|r| r.0 == role.0 && r.1 == role.1)
                .count()
                == 1
                && let [matched] = matches.as_slice()
            {
                evidence.entry(*role).or_default().insert(matched.2);
            }
        }
    }
    Ok(evidence)
}

fn lower(
    source: &Table,
    native: &Table,
    route: &Route,
    evidence: &BTreeMap<Role, BTreeSet<u16>>,
) -> Result<(Route, Vec<Value>)> {
    let mut lowered = [Vec::new(), Vec::new()];
    let mut missing = Vec::new();
    for role in source.roles(route) {
        let mapped = evidence
            .get(&role)
            .filter(|values| values.len() == 1)
            .and_then(|values| values.first());
        let layer = native.layers[role.0]
            .iter()
            .position(|&(name, _)| name == role.1);
        if let (Some(&choice), Some(layer)) = (mapped, layer) {
            lowered[role.0].push((choice, u16::try_from(layer)?));
        } else {
            missing.push(json!({"family":role.0,"layer":role.1,"choice":role.2}));
        }
    }
    Ok((lowered, missing))
}

/// Refresh companion dispatch for an already converted graph without re-encoding clips.
/// The caller commits the returned graph only after refreshing its reference manifest.
pub fn refresh(
    source: &mut Reader,
    native: &mut Reader,
    source_rig: &Value,
    native_rig: &Value,
    directory: &Path,
    graph: &mut Value,
) -> Result<bool> {
    let Some(fp) = graph
        .get_mut("animation")
        .and_then(|a| a.get_mut("first_person"))
    else {
        return Ok(false);
    };
    let files = &fp["files"];
    let Some(converted) = files["converted_bank"].as_str() else {
        return Ok(false);
    };
    let bank = Payload(fs::read(directory.join(converted))?);
    let original = Payload(fs::read(
        directory.join(files["bank"].as_str().context("native bank file")?),
    )?);
    let lookup = first_person(source_rig, SOURCE_LOOKUP)?.context("source first-person rig")?;
    let (tag, _) = super::bank(source, lookup.lookup_owner, true)?;
    let source_bank = source.tag(tag, Some(0x8080289F))?;
    let mut clips = BTreeMap::new();
    for clip in fp["clips"].as_array().context("converted clips")? {
        clips.insert(
            u32::try_from(clip["source"].as_u64().context("source clip")?)?,
            u32::try_from(clip["native"].as_u64().context("native clip")?)?,
        );
    }
    for clip in fp["extra_clips"].as_array().into_iter().flatten() {
        let tag = u32::try_from(clip["source"].as_u64().context("extra source clip")?)?;
        clips.insert(tag, tag);
    }
    let slots = bank.array(8, 4, Some(0x80808F48))?;
    let mut named = BTreeMap::new();
    for (index, row) in bank
        .array(0x68, 32, Some(0x80809002))?
        .into_iter()
        .enumerate()
    {
        let slot = *slots
            .get(usize::from(bank.u16(row + 24)?))
            .context("descriptor clip slot")?;
        named
            .entry((bank.u32(row + 16)?, bank.u32(slot)?))
            .or_insert(u32::try_from(index)?);
    }
    let mut mapping = BTreeMap::new();
    for (index, row) in source_bank
        .array(0x58, 48, Some(0x80808BDE))?
        .into_iter()
        .enumerate()
    {
        if source_bank.bytes::<16>(row)? != [0; 16] || source_bank.f32(row + 20)? != 1.0 {
            continue;
        }
        let tag = source.ref64(&source_bank, row + 24)?;
        if let Some(&clip) = clips.get(&tag)
            && let Some(&to) = named.get(&(source_bank.u32(row + 16)?, clip))
        {
            mapping.insert(u32::try_from(index)?, to);
        }
    }
    let section = prepare(
        source,
        native,
        source_rig,
        native_rig,
        &source_bank,
        &original,
        &bank,
        &mapping,
        directory,
        fp["pose_layers"].clone(),
    )?;
    fp["pose_layers"] = section;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    sr: &mut Reader,
    nr: &mut Reader,
    source_rig: &Value,
    native_rig: &Value,
    source_bank: &Payload,
    native_bank: &Payload,
    bank: &Payload,
    descriptors: &BTreeMap<u32, u32>,
    graph: &Path,
    mut section: Value,
) -> Result<Value> {
    let (_, _, source) = poses::controller(sr, source_rig, true)?;
    let (owner, tag, native) = poses::controller(nr, native_rig, false)?;
    let st = Table::read(&source, true)?;
    let nt = Table::read(&native, false)?;
    validate(&native, native_bank)?;
    ensure!(
        st.indices.len() == source_bank.array(0x58, 48, Some(0x80808BDE))?.len(),
        "source pose dispatch and bank disagree"
    );
    let evidence = calibration(&st, &nt, source_bank, native_bank)?;
    let count = bank.array(0x68, 32, Some(0x80809002))?.len();
    ensure!(
        count >= nt.indices.len(),
        "imported bank removed native descriptors"
    );
    let mut indices = nt.indices.clone();
    indices.resize(count, u16::MAX);
    let mut routes = nt.routes.clone();
    let mut assigned = BTreeMap::<usize, Option<Route>>::new();
    let mut absent = Vec::new();
    let mut translated = Vec::new();
    let mut unsupported = Vec::new();
    for (&from, &to) in descriptors {
        let to = usize::try_from(to)?;
        ensure!(to < count, "converted pose descriptor exceeds its bank");
        if to < nt.indices.len() {
            continue;
        }
        let Some(route) = st.route(usize::try_from(from)?)? else {
            absent.push(json!({"source":from,"native":to}));
            if let Some(previous) = assigned.insert(to, None) {
                ensure!(
                    previous.is_none(),
                    "source descriptor aliases disagree on pose dispatch"
                );
            }
            continue;
        };
        let (lowered, missing) = lower(&st, &nt, route, &evidence)?;
        if !missing.is_empty() {
            // No unchecked native index is emitted. The primary source clip is
            // still available, but unsupported supplemental operations are not.
            unsupported.push(json!({"source":from,"native":to,"operations":missing}));
            if let Some(previous) = assigned.insert(to, None) {
                ensure!(
                    previous.is_none(),
                    "source descriptor aliases disagree on pose dispatch"
                );
            }
            continue;
        }
        if let Some(previous) = assigned.insert(to, Some(lowered.clone())) {
            ensure!(
                previous.as_ref() == Some(&lowered),
                "source descriptor aliases disagree on pose dispatch"
            );
        }
        let index = if let Some(index) = routes.iter().position(|r| *r == lowered) {
            index
        } else {
            routes.push(lowered);
            routes.len() - 1
        };
        ensure!(
            index <= i16::MAX as usize,
            "pose dispatch exceeds native signed indexes"
        );
        indices[to] = index as u16;
        translated.push(json!({"source":from,"native":to,"route":index}));
    }
    ensure!(
        (nt.indices.len()..count).all(|i| assigned.contains_key(&i)),
        "new descriptor lacks source pose provenance"
    );
    let mut output = if section.is_null() {
        native.clone()
    } else {
        ensure!(
            section["owner"] == owner && section["tag"] == tag,
            "pose owner changed during preparation"
        );
        let bytes = fs::read(graph.join(section["file"].as_str().context("pose file")?))?;
        ensure!(
            bytes.len() >= native.0.len(),
            "prepared pose table lost its native prefix"
        );
        let mut original = bytes[..native.0.len()].to_vec();
        // Previous refreshes appended dispatch arrays. Restore their native
        // descriptors before rebuilding, preserving the prepared pose drivers.
        original[40..72].copy_from_slice(&native.0[40..72]);
        Payload(original)
    };
    write_array(
        &mut output.0,
        56,
        0x808034EF,
        routes.len(),
        &vec![0; routes.len() * 32],
    )?;
    let rows = output.array(56, 32, Some(0x808034EF))?;
    for (row, route) in rows.into_iter().zip(&routes) {
        for (family, edges) in route.iter().enumerate() {
            let bytes = edges
                .iter()
                .flat_map(|(choice, layer)| {
                    choice.to_le_bytes().into_iter().chain(layer.to_le_bytes())
                })
                .collect::<Vec<_>>();
            write_array(
                &mut output.0,
                row + family * 16,
                0x808034F1,
                edges.len(),
                &bytes,
            )?;
        }
    }
    write_array(
        &mut output.0,
        40,
        0x80800006,
        indices.len(),
        &indices
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect::<Vec<_>>(),
    )?;
    let size = output.0.len() as u64;
    output.0[..8].copy_from_slice(&size.to_le_bytes());
    validate(&output, bank)?;
    let file = "animation/first-person-pose-layers.bin";
    let template = "animation/first-person-pose-layers-template.bin";
    fs::write(graph.join(file), &output.0)?;
    fs::write(graph.join(template), &native.0)?;
    if section.is_null() {
        section =
            json!({"owner":owner,"tag":tag,"file":file,"template_file":template,"controls":[]});
    }
    section["dispatch"] = json!({"native_descriptors":nt.indices.len(),"descriptors":count,"source_absent":absent,"translated":translated,"unsupported_supplemental_routes":unsupported});
    Ok(section)
}
