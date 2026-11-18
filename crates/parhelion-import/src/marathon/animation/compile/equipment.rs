//! Native weapon actions drive a separate local equipment pose stream.
//!
//! The magazine Reload implementation requests `reload_1` with its duration. It cannot select
//! different source mechanisms for full and empty arm requests, so a shared native action is
//! admitted only when every corresponding arm route uses the same source mechanism. The firing
//! path requests `fire_1` whether the weapon is aimed, so it takes a source firing mechanism
//! only when hip and aimed fire name the same one. Native request names and states remain
//! intact. Arm clip events do not request equipment animations.
//!
//! Some carriers, machine guns among them, also keep a pose controller on the weapon entity
//! whose supplemental layers loop a clip of the same bank over the donor's own parts. It
//! follows the private bank as a consumer. Its layers stay only where they cannot reach the
//! source weapon, see [`Layers`].
use super::*;
use crate::tiger::animation::layer as native_layer;
use std::sync::Arc;

struct Controller {
    entity: Arc<Payload>,
    lookup: (u32, Arc<Payload>),
    consumer: (u32, Arc<Payload>),
    /// The pose controller that plays supplemental layers from the same bank.
    pose: Option<(u32, Arc<Payload>)>,
    bank: (u32, Arc<Payload>),
    states: Arc<Payload>,
}

impl Controller {
    fn read(native: &mut Reader, runtime: u32) -> Result<Option<Self>> {
        let entity = native.tag(runtime, Some(0x80809C0F))?;
        let mut lookups = Vec::new();
        let mut consumers = Vec::new();
        let mut poses = Vec::new();
        for row in entity.array(16, 12, Some(0x80809C04))? {
            let tag = entity.u32(row)?;
            let owner = native.tag(tag, Some(0x80809C36))?;
            match owner.u32(owner.pointer(24)? - 4)? {
                0x8080344B => lookups.push((tag, owner)),
                0x808036CF => consumers.push((tag, owner)),
                0x80803640 => poses.push((tag, owner)),
                _ => {}
            }
        }
        if lookups.is_empty() && consumers.is_empty() {
            return Ok(None);
        }
        ensure!(
            lookups.len() == 1 && consumers.len() == 1 && poses.len() <= 1,
            "Equipment controller is ambiguous"
        );
        let lookup = lookups.remove(0);
        let consumer = consumers.remove(0);
        let pose = poses.pop();
        let base = lookup.1.pointer(24)?;
        let bank = lookup.1.u32(base + 0x90)?;
        for (_, owner) in std::iter::once(&consumer).chain(&pose) {
            ensure!(
                owner.u32(crate::tiger::animation::bank_field(owner)?)? == bank,
                "Equipment output uses another bank"
            );
        }
        let states = native.tag(lookup.1.u32(base + 0x9C)?, Some(0x80803465))?;
        Ok(Some(Self {
            entity,
            lookup,
            consumer,
            pose,
            bank: (bank, native.tag(bank, Some(0x808036F6))?),
            states,
        }))
    }
}

/// The carrier's supplemental equipment layers and why any of them cannot stay.
///
/// A layer was authored for the donor's parts. It reaches the source weapon when one of its
/// clips drives a native bone the source rig is bound to, which is a bone the source rig shares
/// by name or an ancestor of one, or when a source mechanism replaces one of its clips. Then the
/// private table sends every descriptor to no route. Layers that only move donor parts stay on
/// the stock table.
struct Layers {
    tag: u32,
    table: Arc<Payload>,
    names: Vec<u32>,
    conflicts: Vec<Value>,
}

impl Layers {
    fn read(
        native: &mut Reader,
        controller: &Controller,
        sources: &plan::Sources,
        replaced: &BTreeSet<u32>,
    ) -> Result<Option<Self>> {
        let Some((_, pose)) = &controller.pose else {
            return Ok(None);
        };
        let tag = pose.u32(pose.pointer(24)? + 0x11C)?;
        if tag == u32::MAX {
            return Ok(None);
        }
        let table = native.tag(tag, Some(0x808034EB))?;
        let rig = control::Rig::read(
            &sources.carrier.equipment_skeleton.1,
            &sources.carrier.equipment_controls.1,
        )?;
        let source = sources
            .weapon_rig
            .names
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let mut bound = BTreeSet::new();
        for (bone, name) in rig.names.iter().enumerate() {
            let mut at = source.contains(name).then_some(bone);
            while let Some(bone) = at.filter(|&bone| bound.insert(bone)) {
                at = rig.parents[bone];
            }
        }
        let tags = controller.bank.1.array(8, 4, Some(0x80808F48))?;
        let mut names = Vec::new();
        let mut conflicts = Vec::new();
        for (name, ordinals) in native_layer::layer_clips(&table)? {
            names.push(name);
            let mut bones = BTreeSet::new();
            let mut clips = BTreeSet::new();
            for ordinal in ordinals {
                let clip = controller
                    .bank
                    .1
                    .u32(*tags.get(ordinal).context("Equipment layer clip")?)?;
                if replaced.contains(&clip) {
                    clips.insert(clip);
                    continue;
                }
                for slot in native_layer::driven(&*native.tag(clip, Some(0x80808F49))?)? {
                    let bone = rig
                        .controls
                        .get(slot)
                        .context("Equipment layer clip control")?
                        .bone;
                    if bound.contains(&bone) {
                        bones.insert(rig.names[bone]);
                    }
                }
            }
            if !bones.is_empty() || !clips.is_empty() {
                conflicts.push(json!({"layer":format!("{name:08X}"),
                    "bound_bones":bones.iter().map(|b| format!("{b:08X}")).collect::<Vec<_>>(),
                    "replaced_clips":clips}));
            }
        }
        Ok(Some(Self {
            tag,
            table,
            names,
            conflicts,
        }))
    }

    /// The report of what was decided and, where the layers cannot stay, the private table.
    fn write(&self, owner: u32, graph: &Path) -> Result<(Value, Option<Value>)> {
        let report = json!({"table":self.tag,
            "layers":self.names.iter().map(|n| format!("{n:08X}")).collect::<Vec<_>>(),
            "kept":self.conflicts.is_empty(),"conflicts":self.conflicts});
        if self.conflicts.is_empty() {
            return Ok((report, None));
        }
        let template_file = "animation/equipment-layers-template.bin";
        let file = "animation/equipment-layers.bin";
        fs::write(graph.join(template_file), &self.table.0)?;
        fs::write(
            graph.join(file),
            &native_layer::undispatched(&self.table)?.0,
        )?;
        Ok((
            report,
            Some(json!({"owner":owner,"tag":self.tag,"template_file":template_file,"file":file})),
        ))
    }
}

/// Every admitted arm branch must resolve to one weapon-local mechanism. A native request with
/// no ammunition input cannot silently choose one of two distinct full/empty source mechanisms.
/// A branch that only fell back to the weapon's idle pose names no source mechanism, so it
/// cannot replace the native action either.
fn mechanism_for(
    routes: &plan::Routes,
    local: &pairing::WeaponLocal,
    actions: &[&str],
) -> Result<u32> {
    let actions = actions
        .iter()
        .map(|a| source::fnv(a))
        .collect::<BTreeSet<_>>();
    let mut selected = BTreeSet::new();
    let mut found = BTreeSet::new();
    for (&request, choices) in &routes.routes {
        let action = routes.requests[&request].action;
        if !actions.contains(&action) {
            continue;
        }
        found.insert(action);
        for &(descriptor, _) in choices {
            ensure!(
                local.chosen.get(&(request, descriptor)) != Some(&"idle"),
                "An arm branch has only the weapon's idle pose, which names no source mechanism"
            );
            selected.insert(
                *local
                    .overlays
                    .get(&(request, descriptor))
                    .context("An arm branch has no source equipment mechanism")?,
            );
        }
    }
    ensure!(
        found == actions && selected.len() == 1,
        "The native equipment request cannot distinguish the source action branches"
    );
    Ok(*selected.first().context("No source equipment motion")?)
}

fn convert(
    template: &Payload,
    mechanism: &Mechanism,
    sources: &plan::Sources,
    local: &pairing::WeaponLocal,
    rig: &control::Rig,
) -> Result<Payload> {
    ensure!(
        template.u8(0x181)? == 0
            && (0x20..=0x68)
                .step_by(8)
                .all(|p| template.u64(p).ok() == Some(0)),
        "Equipment clip has unsupported procedural streams"
    );
    let frames = u16::try_from(mechanism.frames.len())?;
    ensure!(frames > 0, "Source equipment clip is empty");
    let mut tracks = vec![Vec::with_capacity(usize::from(frames)); rig.controls.len()];
    let mapping = local
        .bones
        .iter()
        .map(|&(_, bone)| {
            let name = sources.weapon_rig.names[bone];
            let native_bone = rig
                .names
                .iter()
                .position(|n| *n == name)
                .context("Equipment skeleton lacks a source mechanism bone")?;
            let rest = sources.weapon_rig.bind[0]
                .inverse()
                .then(sources.weapon_rig.bind[bone]);
            let native_rest = rig.bind[0].inverse().then(rig.bind[native_bone]);
            Ok((native_bone, rest.inverse().then(native_rest)))
        })
        .collect::<Result<Vec<_>>>()?;
    let mapped = mapping
        .iter()
        .enumerate()
        .map(|(i, (bone, correction))| (*bone, (i, *correction)))
        .collect::<BTreeMap<_, _>>();
    for frame in &mechanism.frames {
        ensure!(
            frame.len() == mapping.len(),
            "Source mechanism palette differs"
        );
        let mut objects = rig.bind.clone();
        for (slot, control) in rig.controls.iter().enumerate() {
            let value = match mapped.get(&control.bone) {
                Some(&(bone, correction)) => rig.local(
                    slot,
                    &objects,
                    objects[0].then(frame[bone].then(correction)),
                )?,
                None => Pose(rig.defaults[slot]),
            };
            objects[control.bone] = rig.compose(slot, &objects, value);
            tracks[slot].push(value.0);
        }
    }
    let mut envelope = template.clone();
    let old_frames = template.u16(0x13C)?;
    ensure!(old_frames > 0, "Native equipment clip is empty");
    // Keep every native event and its payload. Only its frame changes with the clip's span.
    for row in template.array(0x160, 8, Some(0x8080902B))? {
        let event = template.pointer(row)?;
        let kind = template.u32(event)?;
        let old = kind & 0xFFFF;
        ensure!(
            old < u32::from(old_frames),
            "Native equipment event lies outside its clip"
        );
        let frame = if old_frames == 1 {
            0
        } else {
            (old as f32 * f32::from(frames - 1) / f32::from(old_frames - 1)).round() as u32
        };
        put(
            &mut envelope.0,
            event,
            &((kind & !0xFFFF) | frame).to_le_bytes(),
        )?;
    }
    put(&mut envelope.0, 0x13C, &frames.to_le_bytes())?;
    native_clip::replace_for(
        &envelope,
        &tracks,
        rig.names.len(),
        &sources.extension.equipment_controls,
    )
}

pub(super) fn prepare(
    native: &mut Reader,
    sources: &plan::Sources,
    routes: &plan::Routes,
    local: &pairing::WeaponLocal,
    graph: &Path,
) -> Result<Value> {
    let Some(controller) = Controller::read(native, sources.carrier.runtime)? else {
        return Ok(
            json!({"status":"unavailable","reason":"The native carrier has no equipment action controller"}),
        );
    };
    let rig = control::Rig::read(
        &sources.extension.equipment_skeleton,
        &sources.extension.equipment_controls,
    )?;
    let names = controller
        .states
        .array(8, 8, Some(0x8080342E))?
        .into_iter()
        .map(|r| controller.states.u32(r))
        .collect::<Result<BTreeSet<_>>>()?;
    let descriptors = controller.bank.1.array(0x68, 32, Some(0x80809002))?;
    let tags = controller.bank.1.array(8, 4, Some(0x80808F48))?;
    // Each replaced native clip with its source descriptor and converted payload.
    let mut selected = BTreeMap::<u32, (u32, Payload)>::new();
    let mut actions = Vec::new();
    let mut excluded = Vec::new();
    // The reload is admitted first. A later action keeps its native request, and is reported,
    // when an admitted action already replaces one of its clips with another mechanism or when
    // one of its native clips cannot carry the source poses.
    for (action, arms) in [
        ("reload_1", &["reload_full", "reload_empty"][..]),
        ("fire_1", &["fire", "iron_sight_fire"][..]),
    ] {
        let name = source::fnv(action);
        if !names.contains(&name) {
            continue;
        }
        let attempt = (|| -> Result<(u32, Vec<(u32, Payload)>)> {
            let mechanism = mechanism_for(routes, local, arms)?;
            let mut clips = BTreeSet::new();
            for index in crate::tiger::animation::state::descriptors(&controller.states, name)? {
                let row = *descriptors
                    .get(index)
                    .context("Equipment action descriptor")?;
                let ordinal = usize::from(controller.bank.1.u16(row + 24)?);
                let tag = controller
                    .bank
                    .1
                    .u32(*tags.get(ordinal).context("Equipment action clip")?)?;
                clips.insert(tag);
            }
            ensure!(
                !clips
                    .iter()
                    .any(|tag| selected.get(tag).is_some_and(|(m, _)| *m != mechanism)),
                "Native action aliases share a clip but require different source mechanisms"
            );
            let motion = &local.mechanisms[&mechanism];
            let mut payloads = Vec::new();
            for tag in clips {
                let template = native.tag(tag, Some(0x80808F49))?;
                payloads.push((tag, convert(&template, motion, sources, local, &rig)?));
            }
            Ok((mechanism, payloads))
        })();
        match attempt {
            Ok((mechanism, payloads)) => {
                actions.push(
                    json!({"native_action":name,"arm_actions":arms,"source_descriptor":mechanism,
                    "native_clips":payloads.iter().map(|(tag, _)| *tag).collect::<Vec<_>>()}),
                );
                for (tag, payload) in payloads {
                    selected.insert(tag, (mechanism, payload));
                }
            }
            Err(e) if crate::cancellation::is_cancelled(&e) => return Err(e),
            Err(e) => excluded.push(json!({"native_action":name,"reason":format!("{e:#}")})),
        }
    }
    let replaced = selected.keys().copied().collect::<BTreeSet<_>>();
    let layers = Layers::read(native, &controller, sources, &replaced)?;
    let (layer_report, pose_layers) = match (&layers, &controller.pose) {
        (Some(layers), Some((owner, _))) => layers.write(*owner, graph)?,
        _ => (Value::Null, None),
    };
    let mut clips = Vec::new();
    let mut bank = controller.bank.1.as_ref().clone();
    for (tag, (descriptor, payload)) in selected {
        let motion = &local.mechanisms[&descriptor];
        let file = format!("animation/equipment-{tag:08X}.bin");
        fs::write(graph.join(&file), &payload.0)?;
        for &row in &descriptors {
            let ordinal = usize::from(bank.u16(row + 24)?);
            let Some(&at) = tags.get(ordinal) else {
                ensure!(
                    ordinal == usize::from(u16::MAX),
                    "Equipment descriptor has an invalid clip"
                );
                continue;
            };
            if bank.u32(at)? == tag {
                put(
                    &mut bank.0,
                    row + 20,
                    &(f32::from(payload.u16(0x13C)? - 1) / 30.).to_le_bytes(),
                )?;
            }
        }
        clips.push(json!({"native":tag,"file":file,"source_descriptor":descriptor,"source_name":motion.name}));
    }
    // Without a replaced action the section is still linked when the donor's layers must come
    // off the source weapon, since only a private controller can drop them.
    if clips.is_empty() && pose_layers.is_none() {
        let mut section = json!({"status":"unavailable","reason":"No native equipment request has one compatible source mechanism","excluded":excluded});
        if !layer_report.is_null() {
            section["native_layers"] = layer_report;
        }
        return Ok(section);
    }
    let mut files = BTreeMap::new();
    let mut consumers = vec![json!({"owner":controller.consumer.0,"file_key":"consumer"})];
    let mut owners = vec![
        ("entity", controller.entity.as_ref()),
        ("lookup_owner", controller.lookup.1.as_ref()),
        ("bank", controller.bank.1.as_ref()),
        ("converted_bank", &bank),
        ("consumer", controller.consumer.1.as_ref()),
    ];
    if let Some((owner, payload)) = &controller.pose {
        owners.push(("pose", payload.as_ref()));
        consumers.push(json!({"owner":owner,"file_key":"pose"}));
    }
    for (name, data) in owners {
        let file = format!("animation/equipment-{name}.bin");
        fs::write(graph.join(&file), &data.0)?;
        files.insert(name, file);
    }
    for (key, field) in [
        ("pose_layers_file", "file"),
        ("pose_layers_template_file", "template_file"),
    ] {
        if let Some(file) = pose_layers
            .as_ref()
            .and_then(|layers| layers[field].as_str())
        {
            files.insert(key, file.to_owned());
        }
    }
    let mut section = json!({"status":"linked","runtime_entity":sources.carrier.runtime,
        "lookup_owner":controller.lookup.0,"bank":controller.bank.0,"files":files,"clips":clips,
        "bank_consumers":consumers,
        "actions":actions,"excluded":excluded,"gameplay_verified":false});
    // A carrier without a pose controller keeps the section it always had.
    if !layer_report.is_null() {
        section["native_layers"] = layer_report;
    }
    if let Some(layers) = pose_layers {
        section["pose_layers"] = layers;
    }
    Ok(section)
}
