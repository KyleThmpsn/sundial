//! Fit explicit Reach first-person actions to a private native arm controller.
mod actions;
mod arm;
mod carrier;
mod constraint;
mod defaults;
mod framing;
mod layer;
mod melee;
mod motion;
mod offset;
pub(super) mod palm;
mod parts;
mod source;
pub use super::route::{Destination, Kind, Route};
use crate::{
    halo_reach::{Scene, cache::Cache},
    presentation::append,
    tiger::{animation, payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
pub(in crate::halo_reach) use defaults::select as defaults;
pub use framing::Framing;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Action {
    Clip(String),
    Sequence(Vec<String>),
    Route(Route),
}

impl Action {
    pub(super) fn names<'a>(&'a self, graph: &'a Value) -> Result<Vec<&'a str>> {
        match self {
            Self::Clip(name) => Ok(vec![name.as_str()]),
            Self::Sequence(names) => {
                ensure!(
                    !names.is_empty() && names.len() <= 16,
                    "Invalid source action sequence"
                );
                Ok(names.iter().map(String::as_str).collect())
            }
            Self::Route(route) => Ok(vec![route.clip(graph)?]),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    #[serde(default)]
    pub arms_model: String,
    /// Native named state to an absolute source clip or ordered clip sequence.
    #[serde(default)]
    pub actions: BTreeMap<String, Action>,
    /// Alternate source weapon strikes, selected by the private melee attack.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub melee: Vec<String>,
    /// Metres in the native grip parent's axes, applied only to absolute motion.
    #[serde(default)]
    pub held_offset: [f32; 3],
    /// Source view placement uses the native utility-parented weapon reference.
    #[serde(default)]
    pub framing: Framing,
    /// Bind the source handling recordings to the translated actions' clips. Off keeps the
    /// donor's cues on every clip, which isolates the route when an equip or action misbehaves.
    #[serde(default = "enabled")]
    pub handling_audio: bool,
}

fn enabled() -> bool {
    true
}

fn hash(name: &str) -> u32 {
    name.bytes()
        .fold(0x811C9DC5, |h, b| h.wrapping_mul(0x01000193) ^ u32::from(b))
}

fn hex(v: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        v.as_str().context("Native rig identity")?,
        16,
    )?)
}

fn rows(p: &Payload, field: usize, stride: usize, class: u32) -> Result<Vec<u8>> {
    Ok(p.array(field, stride, Some(class))?
        .into_iter()
        .flat_map(|at| p.0[at..at + stride].iter().copied())
        .collect())
}

struct Files<'a> {
    root: &'a Path,
    names: BTreeMap<String, String>,
}
impl Files<'_> {
    fn add(&mut self, key: &str, p: &Payload) -> Result<String> {
        let name = format!("animation/{key}.bin");
        ensure!(
            self.names.insert(key.to_owned(), name.clone()).is_none(),
            "Repeated arm asset name"
        );
        fs::write(self.root.join(&name), &p.0)?;
        Ok(name)
    }
}

#[allow(clippy::too_many_arguments)]
pub(in crate::halo_reach) fn prepare(
    cache: &Cache,
    scene: &Scene,
    native: &mut Reader,
    donor: u32,
    options: &Options,
    aliases: &BTreeMap<String, String>,
    root: &Path,
    source_audio: bool,
) -> Result<Value> {
    ensure!(
        options
            .held_offset
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 0.2),
        "Held offset exceeds 20 centimetres"
    );
    ensure!(
        scene.object.tag.group == "weap"
            && (!options.actions.is_empty()
                || !options.melee.is_empty()
                || options.held_offset != [0.; 3]),
        "First-person arm translation requires weapon actions or a held offset"
    );
    let source_root = root.parent().context("Import root")?.join("source");
    let source = (!options.actions.is_empty() || !options.melee.is_empty())
        .then(|| source::Source::read(cache, scene, options, &source_root))
        .transpose()?;
    ensure!(
        source.is_some() || options.framing == Framing::Native,
        "Source framing requires translated source actions"
    );
    let mut carrier = carrier::Carrier::read(native, donor)?;
    let reference_defaults = carrier.rig.defaults.clone();
    let reference_controls = carrier.controls.clone();
    let extension = source
        .as_ref()
        .map(|source| parts::prepare(source, scene, &carrier, aliases))
        .transpose()?
        .flatten();
    let mut rig_owners = Vec::new();
    let mut equipment_skeleton = None;
    if let Some(extension) = extension {
        carrier.rig = extension.rig;
        carrier.controls = extension.controls;
        equipment_skeleton = Some(extension.skeleton);
        rig_owners = extension.owners;
    }
    let carrier::Carrier {
        runtime,
        attachment,
        entity,
        lookup_tag,
        lookup,
        bank_tag,
        bank,
        parameter_tag,
        parameters,
        state_tag,
        states,
        pose_owner,
        pose_tag,
        pose_layers,
        rig,
        controls,
        descriptors,
        consumers,
        ..
    } = carrier;
    let mut clip_rows = rows(&bank, 8, 4, 0x80808F48)?;
    // One signed 32-bit word per clip, class 0x80800007. A clip without one holds -1.
    let mut auxiliary = rows(&bank, 24, 4, 0x80800007)?;
    ensure!(
        clip_rows.len() / 4 == auxiliary.len() / 4,
        "Native clip auxiliary map differs"
    );
    let mut descriptor_rows = rows(&bank, 0x68, 32, 0x80809002)?;
    ensure!(
        descriptors
            .iter()
            .all(|&at| bank.bytes::<16>(at).ok() == Some([0; 16])),
        "Native animation descriptors require inline track relocation"
    );
    let mut dispatch = rows(&pose_layers, 40, 2, 0x80800006)?;
    fs::create_dir_all(root.join("animation"))?;
    let mut files = Files {
        root,
        names: BTreeMap::new(),
    };
    let mut rigs = Vec::new();
    for owner in rig_owners {
        let template_file =
            files.add(&format!("rig-{:08X}-template", owner.tag), &owner.original)?;
        let file = files.add(&format!("rig-{:08X}", owner.tag), &owner.converted)?;
        rigs.push(json!({"native":owner.tag,"first_person":owner.first_person,"kind":owner.kind,
            "template_file":template_file,"file":file,"relocation":owner.relocation.map(|(cut,delta)|json!({"cut":cut,"delta":delta}))}));
    }
    for (key, p) in [
        (
            "attachment_owner",
            native.tag(attachment, Some(0x80809C36))?,
        ),
        ("entity", native.tag(entity, Some(0x80809C0F))?),
        ("lookup_owner", lookup),
        ("bank", bank.clone()),
        ("parameters_template", parameters.clone()),
        ("parameters", parameters),
        ("states_template", states.clone()),
    ] {
        files.add(key, &p)?;
    }
    let mut markers = bank
        .0
        .chunks_exact(4)
        .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
        .collect::<BTreeSet<_>>();
    let mut routes = BTreeMap::new();
    let mut extras = Vec::new();
    let mut actions = Vec::new();
    let mut hands = Vec::new();
    let mut held_offset = options.held_offset;
    let mut framing = json!({"mode":"native"});
    let calibration = if let Some(source) = &source {
        let idle = *animation::state::descriptors(&states, hash("idle"))?
            .first()
            .context("Native idle descriptor")?;
        let descriptor = *descriptors
            .get(idle)
            .context("Native idle descriptor outside bank")?;
        ensure!(
            bank.u16(descriptor + 26)? == 1,
            "Native idle requires absolute playback"
        );
        let slot = usize::from(bank.u16(descriptor + 24)?);
        let rows = bank.array(8, 4, Some(0x80808F48))?;
        let reference = native.tag(
            bank.u32(*rows.get(slot).context("Native idle clip")?)?,
            Some(0x80808F49),
        )?;
        let source_reference = if let Some(idle) = options.actions.get("idle") {
            source.action(&source_root, idle, 1)?
        } else {
            source.clip(&source_root, "first_person:idle", 1)?
        };
        let mut defaults = animation::clip::sample_common(
            &reference,
            &reference_defaults,
            &reference_controls,
            0,
        )?;
        defaults.extend_from_slice(&rig.defaults[defaults.len()..]);
        (held_offset, framing) = framing::resolve(
            cache,
            scene,
            source,
            &rig,
            &defaults,
            &source_reference[0],
            options,
        )?;
        Some(motion::Calibration::new(
            source,
            &rig,
            &controls,
            &defaults,
            &source_reference[0],
            aliases,
            held_offset,
        )?)
    } else {
        None
    };
    let grip = offset::grip(&rig, held_offset)?;
    let selected_actions = actions::complete(&options.actions, &states, &bank)?;
    for (action, source_clip) in &selected_actions {
        let source = source.as_ref().context("Source arm actions")?;
        let name = hash(action);
        let mut mapped = BTreeMap::new();
        let mut emitted = Vec::new();
        let mut reach_adjustment = 0f32;
        for descriptor in animation::state::descriptors(&states, name)? {
            let at = *descriptors
                .get(descriptor)
                .context("Native action descriptor is outside bank")?;
            ensure!(
                bank.u16(at + 26)? == 1,
                "Native arm action needs absolute playback"
            );
            let slot = usize::from(bank.u16(at + 24)?);
            let template = u32::from_le_bytes(
                clip_rows
                    .get(slot * 4..slot * 4 + 4)
                    .context("Native descriptor clip missing")?
                    .try_into()?,
            );
            let clip = native.tag(template, Some(0x80808F49))?;
            let source_frames =
                source.action(&source_root, source_clip, usize::from(clip.u16(0x13C)?))?;
            let motion::Translated {
                poses,
                hand_slots,
                shoulder_reach_adjustment,
            } = calibration
                .as_ref()
                .context("Arm calibration")?
                .translate(&source_frames, &rig)?;
            reach_adjustment = reach_adjustment.max(shoulder_reach_adjustment);
            if hands.is_empty() {
                hands = hand_slots;
            } else {
                ensure!(hands == hand_slots, "Native action hand maps disagree");
            }
            let mut converted =
                animation::clip::replace_for(&clip, &poses, rig.bind.len(), &controls)?;
            let released_constraints = constraint::release(&mut converted, &rig)
                .with_context(|| format!("Source action {action}, native clip {template:08X}"))?;
            let marker = hash(&format!(
                "reach/arms/{action}/{descriptor}/{}",
                serde_json::to_string(source_clip)?
            ));
            ensure!(
                ![0, u32::MAX].contains(&marker) && markers.insert(marker),
                "Private arm clip marker collides with a bank word"
            );
            let file = files.add(&format!("clip-{name:08X}-{descriptor}"), &converted)?;
            emitted.push(
                json!({"descriptor":descriptor,"marker":format!("{marker:08X}"),
                "template":template,"file":&file,"released_hand_constraints":released_constraints}),
            );
            extras.push(json!({"source":marker,"template":template,"file":file}));
            let index = u16::try_from(clip_rows.len() / 4)?;
            clip_rows.extend(marker.to_le_bytes());
            auxiliary.extend(u32::MAX.to_le_bytes());
            let mut row = bank.bytes::<32>(at)?;
            row[24..26].copy_from_slice(&index.to_le_bytes());
            mapped.insert(descriptor, descriptor_rows.len() / 32);
            descriptor_rows.extend(row);
            dispatch.extend(
                pose_layers.bytes::<2>(pose_layers.array(40, 2, Some(0x80800006))?[descriptor])?,
            );
        }
        actions.push(
            json!({"state":action,"source_clip":source_clip.names(&source.graph)?,"source_selection":source_clip,"descriptors":mapped,"clips":emitted,
            "maximum_shoulder_reach_adjustment_metres":reach_adjustment}),
        );
        ensure!(
            routes
                .insert(
                    name,
                    mapped
                        .into_iter()
                        .map(|(from, to)| (from, vec![to]))
                        .collect()
                )
                .is_none(),
            "Requested arm action names collide"
        );
    }
    let mut strikes = Vec::new();
    if !options.melee.is_empty() {
        for strike in melee::prepare(
            source.as_ref().context("Source melee actions")?,
            &source_root,
            native,
            &bank,
            &states,
            &rig,
            &controls,
            calibration.as_ref().context("Melee arm calibration")?,
            &options.melee,
            source_audio,
        )? {
            ensure!(
                ![0, u32::MAX].contains(&strike.marker) && markers.insert(strike.marker),
                "Private melee clip marker collides with a bank word"
            );
            if hands.is_empty() {
                hands = strike.hands;
            } else {
                ensure!(hands == strike.hands, "Native melee hand maps disagree");
            }
            let file = files.add(&format!("melee-{:08X}", strike.marker), &strike.payload)?;
            extras.push(json!({"source":strike.marker,"template":strike.template,"file":file}));
            let index = u16::try_from(clip_rows.len() / 4)?;
            clip_rows.extend(strike.marker.to_le_bytes());
            auxiliary.extend(u32::MAX.to_le_bytes());
            let mut row = bank.bytes::<32>(descriptors[strike.descriptor])?;
            row[16..20].copy_from_slice(&strike.marker.to_le_bytes());
            // Descriptor duration is a native single-precision value.
            row[20..24].copy_from_slice(
                &(((f64::from(strike.payload.u16(0x13C)?) - 1.) / 30.0) as f32).to_le_bytes(),
            );
            row[24..26].copy_from_slice(&index.to_le_bytes());
            let mapped = routes.entry(strike.state).or_insert_with(BTreeMap::new);
            mapped
                .entry(strike.descriptor)
                .or_insert_with(Vec::new)
                .push(descriptor_rows.len() / 32);
            descriptor_rows.extend(row);
            dispatch.extend(
                pose_layers
                    .bytes::<2>(pose_layers.array(40, 2, Some(0x80800006))?[strike.descriptor])?,
            );
            strikes.push(strike.report);
        }
    }
    let clips = offset::clips(native, &bank, &rig, grip, held_offset, &mut files)?;
    let mut converted_bank = (*bank).clone();
    append(&mut converted_bank.0, 8, 0x80808F48, &clip_rows, 4)?;
    append(&mut converted_bank.0, 24, 0x80800007, &auxiliary, 4)?;
    append(
        &mut converted_bank.0,
        0x68,
        0x80809002,
        &descriptor_rows,
        32,
    )?;
    files.add("converted_bank", &converted_bank)?;
    files.add(
        "states",
        &animation::state::clone_variants(&states, &routes)?,
    )?;
    let mut converted_layers = (*pose_layers).clone();
    append(&mut converted_layers.0, 40, 0x80800006, &dispatch, 2)?;
    let (converted_layers, layer_report) =
        layer::source_actions(native, &converted_layers, &bank, descriptors.len())?;
    let layer_template = files.add("pose-layers-template", &pose_layers)?;
    let layer_file = files.add("pose-layers", &converted_layers)?;
    let mut consumer_rows = Vec::new();
    for (tag, payload) in consumers {
        let key = format!("consumer-{tag:08X}");
        files.add(&key, &payload)?;
        consumer_rows.push(json!({"owner":tag,"file_key":key}));
    }
    Ok(
        json!({"runtime_entity":runtime,"first_person_status":"linked","equipment_skeleton":equipment_skeleton,"first_person":{
            "attachment_owner":attachment,"entity":entity,"lookup_owner":lookup_tag,"bank":bank_tag,"files":files.names,"clips":clips,"extra_clips":extras,"bank_consumers":consumer_rows,"rig_extension":!rigs.is_empty(),"rigs":rigs,
            "pose_layers":{"owner":pose_owner,"tag":pose_tag,"file":layer_file,"template_file":layer_template,"source_actions":layer_report},
            "state_conversion":{"status":"source_states","parameters":parameter_tag,"states":state_tag},
            "melee":{"strikes":strikes,"selection":"Equal source alternatives inside each native state choice",
                "limits":["The source primary keyframe and full motion duration are translated. The native attack controller, hit acceptance and post-melee ready transition still require gameplay acceptance."]},
            "arm_motion":{"actions":actions,"hand_slots":hands,"weapon_parts":calibration.as_ref().map(|c| &c.parts),"source_graph":source.as_ref().map(|s| &s.graph["tag"]),"arms_model":options.arms_model,"native_bones":rig.bind.len(),"native_controls":rig.controls.len(),"held_offset":options.held_offset,"applied_offset":held_offset,"framing":framing,"grip_control":grip,"alignment":"Native gun orientation with the selected idle framing, source wrists, palm axes, shoulder movement, elbow planes and bind-relative weapon parts. Translated actions release donor hand targets while retaining native arm solving. Shoulders move only as far as required to retain wrist contact within native segment lengths."},
            "retained_native_behavior":["Arm geometry and segment lengths", "Two-bone arm solver", "Non-melee action duration and event timing", "Untranslated pose layers and state transitions"],
            "limits":["Non-melee source motion is fitted to native action duration and events. Weapon strikes retain their source duration and contact keyframe. Source elbow and shoulder controls drive the native arm solver. Weapon parts without a corresponding native control remain unmapped and are listed separately. In-game playback remains unverified.", "Source framing maps camera-relative source positions into the native utility-parented weapon reference. This does not establish the final native camera origin, view projection or ADS blend."]
        }}),
    )
}
