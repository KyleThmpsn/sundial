//! Fit explicit Reach first-person actions to a private native arm controller.
mod carrier;
mod motion;
mod source;
use crate::{
    halo_reach::{Scene, cache::Cache},
    presentation::append,
    tiger::{animation, payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub arms_model: String,
    /// Native named state to absolute source clip, including its mode prefix.
    pub actions: BTreeMap<String, String>,
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

pub(in crate::halo_reach) fn prepare(
    cache: &Cache,
    scene: &Scene,
    native: &mut Reader,
    donor: u32,
    options: &Options,
    root: &Path,
) -> Result<Value> {
    ensure!(
        scene.object.tag.group == "weap" && !options.actions.is_empty(),
        "First-person arm translation requires weapon actions"
    );
    let source_root = root.parent().context("Import root")?.join("source");
    let source = source::Source::read(cache, scene, options, &source_root)?;
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
        descriptors,
        consumers,
    } = carrier::Carrier::read(native, donor)?;
    let mut clip_rows = rows(&bank, 8, 4, 0x80808F48)?;
    let mut auxiliary = rows(&bank, 24, 2, 0x80800007)?;
    ensure!(
        clip_rows.len() / 4 == auxiliary.len() / 2,
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
    for (action, source_clip) in &options.actions {
        let name = hash(action);
        let mut mapped = BTreeMap::new();
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
            ensure!(
                usize::from(clip.u16(0x13E)?) == rig.bind.len(),
                "Native action uses a different skeleton"
            );
            let source_frames =
                source.clip(&source_root, source_clip, usize::from(clip.u16(0x13C)?))?;
            let motion::Translated { poses, hand_slots } =
                motion::translate(&source, &source_frames, &rig, &clip)?;
            if hands.is_empty() {
                hands = hand_slots;
            } else {
                ensure!(hands == hand_slots, "Native action hand maps disagree");
            }
            let converted = animation::clip::replace(&clip, &poses)?;
            let marker = hash(&format!("reach/arms/{action}/{descriptor}/{source_clip}"));
            ensure!(
                ![0, u32::MAX].contains(&marker) && markers.insert(marker),
                "Private arm clip marker collides with a bank word"
            );
            let file = files.add(&format!("clip-{name:08X}-{descriptor}"), &converted)?;
            extras.push(json!({"source":marker,"template":template,"file":file}));
            let index = u16::try_from(clip_rows.len() / 4)?;
            clip_rows.extend(marker.to_le_bytes());
            auxiliary.extend(u16::MAX.to_le_bytes());
            let mut row = bank.bytes::<32>(at)?;
            row[24..26].copy_from_slice(&index.to_le_bytes());
            mapped.insert(descriptor, descriptor_rows.len() / 32);
            descriptor_rows.extend(row);
            dispatch.extend(
                pose_layers.bytes::<2>(pose_layers.array(40, 2, Some(0x80800006))?[descriptor])?,
            );
        }
        actions.push(json!({"state":action,"source_clip":source_clip,"descriptors":mapped}));
        ensure!(
            routes.insert(name, mapped).is_none(),
            "Requested arm action names collide"
        );
    }
    let mut converted_bank = (*bank).clone();
    append(&mut converted_bank.0, 8, 0x80808F48, &clip_rows, 4)?;
    append(&mut converted_bank.0, 24, 0x80800007, &auxiliary, 2)?;
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
        &animation::state::clone_actions(&states, &routes)?,
    )?;
    let mut converted_layers = (*pose_layers).clone();
    append(&mut converted_layers.0, 40, 0x80800006, &dispatch, 2)?;
    let layer_template = files.add("pose-layers-template", &pose_layers)?;
    let layer_file = files.add("pose-layers", &converted_layers)?;
    let mut consumer_rows = Vec::new();
    for (tag, payload) in consumers {
        let key = format!("consumer-{tag:08X}");
        files.add(&key, &payload)?;
        consumer_rows.push(json!({"owner":tag,"file_key":key}));
    }
    Ok(
        json!({"runtime_entity":runtime,"first_person_status":"linked","first_person":{
            "attachment_owner":attachment,"entity":entity,"lookup_owner":lookup_tag,"bank":bank_tag,"files":files.names,"clips":[],"extra_clips":extras,"bank_consumers":consumer_rows,"rigs":[],
            "pose_layers":{"owner":pose_owner,"tag":pose_tag,"file":layer_file,"template_file":layer_template},
            "state_conversion":{"status":"source_states","parameters":parameter_tag,"states":state_tag},
            "arm_motion":{"actions":actions,"hand_slots":hands,"source_graph":source.graph["tag"],"arms_model":options.arms_model,"native_bones":rig.bind.len(),"native_controls":rig.controls.len()},
            "limits":["Source motion is fitted to native action duration and events. Native arm geometry, forearm IK and pose layers are retained. Source weapon subpart deformation is not translated. In-game playback remains unverified."]
        }}),
    )
}
