//! Explicit source motion on appended native vehicle controls.
use super::{codec::Motion, playback};
use crate::{
    halo_reach::{Scene, rig},
    presentation::{Graph, put},
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
pub struct Binding {
    pub native_clip: u32,
    pub source_clip: String,
    #[serde(default)]
    pub shared: usize,
}

struct Owner {
    tag: u32,
    original: Payload,
    converted: Payload,
    symbol: String,
    bank_field: Option<usize>,
    relocation: Option<(usize, usize)>,
}

pub(in crate::halo_reach) struct Bundle {
    pub skeleton: Value,
    pub section: Value,
    pub source_motion: Value,
    entity: Payload,
    owners: Vec<Owner>,
    bank: (u32, Payload),
    clips: Vec<(u32, Payload)>,
}

fn multiply(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [x, y, z, w] = a;
    let [i, j, k, r] = b;
    [
        w * i + x * r + y * k - z * j,
        w * j - x * k + y * r + z * i,
        w * k + x * j - y * i + z * r,
        w * r - x * i - y * j - z * k,
    ]
}

fn source_bones(scene: &Scene, first: usize) -> Result<Vec<animation::rig::Bone>> {
    let bones = &scene.models[0].model.bones;
    let objects = rig::world(bones)?;
    let mut rotations = Vec::new();
    let mut output = Vec::new();
    for (index, bone) in bones.iter().enumerate() {
        ensure!(
            bone.parent.is_none_or(|p| p < index),
            "Source rig is not in parent-first order"
        );
        let rotation = rig::quaternion(
            bone.parent
                .map_or(bone.rotation, |p| multiply(rotations[p], bone.rotation)),
        )?;
        rotations.push(rotation);
        let inverse = rig::inverse_rigid(&objects[index]);
        let transform = |q: [f32; 4], t: [f32; 3]| [q[0], q[1], q[2], q[3], t[0], t[1], t[2], 1.];
        let name = format!("source/{}", bone.name)
            .bytes()
            .fold(0x811c9dc5u32, |h, b| {
                h.wrapping_mul(0x01000193) ^ u32::from(b)
            });
        output.push(animation::rig::Bone {
            name,
            parent: bone.parent.map_or(0, |p| first + p),
            local: transform(bone.rotation, bone.translation),
            object: transform(rotation, objects[index][12..15].try_into()?),
            inverse: transform(
                [-rotation[0], -rotation[1], -rotation[2], rotation[3]],
                inverse[12..15].try_into()?,
            ),
        });
    }
    Ok(output)
}

fn poses(
    scene: &Scene,
    source: &Value,
    root: &Path,
    binding: &Binding,
    frames: usize,
) -> Result<Vec<Vec<[f32; 8]>>> {
    let model = &scene.models[0].model;
    let map = playback::mapping(source, model)?;
    let clips = source["clips"]
        .as_array()
        .context("Source animation clips")?
        .iter()
        .filter(|c| c["name"] == binding.source_clip)
        .collect::<Vec<_>>();
    ensure!(
        clips.len() == 1,
        "Requested source animation is missing or ambiguous"
    );
    let shared = clips[0]["shared"]
        .as_array()
        .context("Source clip variants")?
        .get(binding.shared)
        .context("Requested source clip variant is absent")?;
    let member = playback::member(source, shared)?;
    ensure!(
        shared["movement_type"] == 0
            && member["movement"] == 0
            && matches!(shared["animation_type"].as_u64(), Some(1 | 3)),
        "Native playback currently requires an absolute source clip without root movement"
    );
    let file = member["motion"]["file"]
        .as_str()
        .context("Source clip has no decoded motion")?;
    let motion: Motion = serde_json::from_slice(&fs::read(root.join(file))?)?;
    ensure!(
        motion.frames > 0
            && shared["frames"].as_u64() == Some(motion.frames as u64)
            && motion.tracks.len() == map.len(),
        "Source motion dimensions disagree"
    );
    let sample = |values: &[[f32; 8]], frame: usize| -> Result<[f32; 8]> {
        let time = if frames == 1 {
            0.
        } else {
            frame as f32 * (values.len() - 1) as f32 / (frames - 1) as f32
        };
        let lo = time.floor() as usize;
        let hi = (lo + 1).min(values.len() - 1);
        let t = time - lo as f32;
        let a = values[lo];
        let mut b = values[hi];
        if a[..4].iter().zip(&b[..4]).map(|(a, b)| a * b).sum::<f32>() < 0. {
            b[..4].iter_mut().for_each(|v| *v = -*v);
        }
        let mut out = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
        let q = rig::quaternion(out[..4].try_into()?).or_else(|_| {
            let norm = out[..4].iter().map(|v| v * v).sum::<f32>().sqrt();
            ensure!(
                norm.is_finite() && norm > 0.000001,
                "Degenerate interpolated source rotation"
            );
            Ok::<_, anyhow::Error>(std::array::from_fn(|i| out[i] / norm))
        })?;
        out[..4].copy_from_slice(&q);
        Ok(out)
    };
    let mut result = vec![Vec::new(); model.bones.len()];
    for (index, track) in motion.tracks.iter().enumerate() {
        let bone = &model.bones[map[index]];
        for length in [
            track.rotation.len(),
            track.translation.len(),
            track.scale.len(),
        ] {
            ensure!(
                length == 0 || length == 1 || length == motion.frames,
                "Incomplete source motion channel"
            );
        }
        let mut values = Vec::new();
        for frame in 0..motion.frames {
            let q = track
                .rotation
                .get(if track.rotation.len() == 1 { 0 } else { frame })
                .copied()
                .unwrap_or(bone.rotation);
            let p = track
                .translation
                .get(if track.translation.len() == 1 {
                    0
                } else {
                    frame
                })
                .copied()
                .unwrap_or(bone.translation);
            let s = track
                .scale
                .get(if track.scale.len() == 1 { 0 } else { frame })
                .copied()
                .unwrap_or(1.);
            values.push([q[0], q[1], q[2], q[3], p[0], p[1], p[2], s]);
        }
        result[map[index]] = (0..frames)
            .map(|frame| sample(&values, frame))
            .collect::<Result<_>>()?;
    }
    Ok(result)
}

fn owner(reader: &mut Reader, entity: &Payload, class: u32) -> Result<(u32, Payload)> {
    let mut found = Vec::new();
    for row in entity.array(16, 12, Some(0x80809C04))? {
        let tag = entity.u32(row)?;
        let value = reader.tag(tag, Some(0x80809C36))?;
        if value.u32(value.pointer(24)? - 4)? == class {
            found.push((tag, (*value).clone()));
        }
    }
    ensure!(
        found.len() == 1,
        "Native animation owner {class:08X} is missing or ambiguous"
    );
    Ok(found.pop().unwrap())
}

pub(in crate::halo_reach) fn prepare(
    reader: &mut Reader,
    scene: &Scene,
    entity_tag: u32,
    bindings: &[Binding],
    root: &Path,
) -> Result<Bundle> {
    ensure!(
        scene.models.len() == 1 && scene.models[0].parent.is_none(),
        "Native source motion currently requires one complete model rig"
    );
    let source_root = root
        .parent()
        .context("Source motion directory")?
        .join("source");
    let graphs: Vec<Value> =
        serde_json::from_slice(&fs::read(source_root.join("animations.json"))?)?;
    let source_tag = scene.models[0]
        .animation
        .as_ref()
        .context("Source model has no animation graph")?;
    let sources = graphs
        .iter()
        .filter(|g| g["tag"]["datum"] == source_tag.datum)
        .collect::<Vec<_>>();
    ensure!(
        sources.len() == 1,
        "Source animation graph is missing or ambiguous"
    );
    let source = sources[0];
    let entity = (*reader.tag(entity_tag, Some(0x80809C0F))?).clone();
    let (fk_tag, fk) = owner(reader, &entity, 0x80808546)?;
    let (control_tag, controls) = owner(reader, &entity, 0x80808F8F)?;
    let (lookup_tag, lookup) = owner(reader, &entity, 0x8080344B)?;
    let bank_tag = lookup.u32(lookup.pointer(24)? + 0x90)?;
    let bank = (*reader.tag(bank_tag, Some(0x808036F6))?).clone();
    let count = fk
        .array(fk.pointer(24)? + 0x80, 16, Some(0x80808A08))?
        .len();
    let bind = fk.array(fk.pointer(24)? + 0x90, 32, Some(0x80809F75))?;
    let identity = [0f32, 0., 0., 1., 0., 0., 0., 1.];
    ensure!(
        !bind.is_empty()
            && (0..8).all(|i| fk
                .f32(bind[0] + i * 4)
                .is_ok_and(|v| (v - identity[i]).abs() < 0.00001)),
        "Native root bind requires a separate source coordinate adapter"
    );
    let extended = animation::rig::extend(&fk, fk_tag, &controls, &source_bones(scene, count)?)?;
    let mut skeleton = crate::tiger::rig::decode(
        &extended.skeleton,
        extended.skeleton.pointer(24)?,
        0x80808546,
        fk_tag,
        false,
    )?;
    skeleton["source_bone_first"] = json!(extended.original_bones);
    let total = skeleton["bones"]
        .as_array()
        .context("Extended bones")?
        .len();
    let mut owners = vec![
        Owner {
            tag: fk_tag,
            original: fk,
            converted: extended.skeleton,
            symbol: "motion-skeleton".into(),
            bank_field: None,
            relocation: Some(extended.relocation),
        },
        Owner {
            tag: control_tag,
            original: controls.clone(),
            converted: extended.controls,
            symbol: "motion-controls".into(),
            bank_field: None,
            relocation: None,
        },
        Owner {
            tag: lookup_tag,
            original: lookup.clone(),
            converted: lookup.clone(),
            symbol: "motion-lookup".into(),
            bank_field: Some(lookup.pointer(24)? + 0x90),
            relocation: None,
        },
    ];
    for class in [0x80803640, 0x808036CF] {
        let (tag, payload) = owner(reader, &entity, class)?;
        let field = animation::bank_field(&payload)?;
        ensure!(
            payload.u32(field)? == bank_tag,
            "Native controller names another animation bank"
        );
        owners.push(Owner {
            tag,
            original: payload.clone(),
            converted: payload,
            symbol: format!("motion-consumer-{tag:08X}"),
            bank_field: Some(field),
            relocation: None,
        });
    }
    let mut selected = BTreeMap::new();
    for binding in bindings {
        ensure!(
            selected.insert(binding.native_clip, binding).is_none(),
            "Repeated native clip binding"
        );
    }
    let mut clips = Vec::new();
    let mut seen = BTreeSet::new();
    for row in bank.array(8, 4, Some(0x80808F48))? {
        let tag = bank.u32(row)?;
        if !seen.insert(tag) {
            continue;
        }
        let original = reader.tag(tag, Some(0x80808F49))?;
        let motion = selected
            .remove(&tag)
            .map(|binding| {
                poses(
                    scene,
                    source,
                    &source_root,
                    binding,
                    original.u16(0x13C)? as usize,
                )
            })
            .transpose()?;
        clips.push((
            tag,
            animation::clip::extend(&original, count, total, motion.as_deref())?,
        ));
    }
    ensure!(
        selected.is_empty(),
        "Requested native clip is outside the selected vehicle bank"
    );
    fs::create_dir_all(root.join("animation"))?;
    let save = |name: &str, payload: &Payload| -> Result<String> {
        let file = format!("animation/{name}.bin");
        fs::write(root.join(&file), &payload.0)?;
        Ok(file)
    };
    let mut files = json!({"entity":save("entity", &entity)?, "bank":save("bank", &bank)?, "converted_bank":save("bank-converted", &bank)?});
    let mut consumers = Vec::new();
    let mut rigs = Vec::new();
    for owner in &owners {
        let original = save(&format!("{}-original", owner.symbol), &owner.original)?;
        let file = save(&owner.symbol, &owner.converted)?;
        if owner.tag == lookup_tag {
            files["lookup_owner"] = json!(original);
        } else if owner.bank_field.is_some() {
            let key = format!("consumer_{}", consumers.len());
            files[&key] = json!(original);
            consumers.push(json!({"owner":owner.tag,"file_key":key}));
        } else {
            files[format!("rig_{}_original", rigs.len())] = json!(original);
            files[format!("rig_{}_converted", rigs.len())] = json!(file);
            rigs.push(json!({"owner":owner.tag,"original_file":original,"file":file,"relocation":owner.relocation}));
        }
    }
    let clip_files = clips
        .iter()
        .map(|(tag, p)| Ok(json!({"native":tag,"file":save(&format!("clip-{tag:08X}"),p)?})))
        .collect::<Result<Vec<_>>>()?;
    let section = json!({"status":"linked","runtime_entity":entity_tag,"lookup_owner":lookup_tag,"bank":bank_tag,
        "files":files,"bank_consumers":consumers,"clips":clip_files,"rigs":rigs,"preserve_timing":true});
    let source_motion = json!({"bindings":bindings,"bones":total,"source_graph":source_tag,"source_bones":scene.models[0].model.bones,
        "first_source_bone":count,"resampling":"Normalized source duration fitted to the unchanged native frame count",
        "limits":["Native bank selection, timing, events and vehicle behavior are retained. Source root movement, layering and events are not translated. In-game playback is unverified."]});
    Ok(Bundle {
        skeleton,
        section,
        source_motion,
        entity,
        owners,
        bank: (bank_tag, bank),
        clips,
    })
}

impl Bundle {
    pub(in crate::halo_reach) fn link(
        &self,
        graph: &mut Graph,
        entity: &mut [u8],
        patches: &mut Vec<Value>,
    ) -> Result<()> {
        let mut bank = self.bank.1.0.clone();
        let mut bank_patches = Vec::new();
        for (tag, clip) in &self.clips {
            let symbol = format!("motion-clip-{tag:08X}");
            graph.add(&symbol, *tag, &clip.0, None, vec![])?;
            for row in self.bank.1.array(8, 4, Some(0x80808F48))? {
                if self.bank.1.u32(row)? == *tag {
                    put(&mut bank, row, &u32::MAX.to_le_bytes())?;
                    bank_patches.push(json!({"offset":row,"symbol":symbol}));
                }
            }
        }
        graph.add("motion-bank", self.bank.0, &bank, None, bank_patches)?;
        for owner in &self.owners {
            if let Some((cut, delta)) = owner.relocation {
                animation::rig::rebase_entity(entity, &owner.original, owner.tag, cut, delta)?;
            }
            let mut payload = owner.converted.0.clone();
            let mut owner_patches = Vec::new();
            for at in (0..owner.converted.0.len().saturating_sub(3)).step_by(4) {
                if owner.converted.u32(at)? == owner.tag {
                    put(&mut payload, at, &u32::MAX.to_le_bytes())?;
                    owner_patches.push(json!({"offset":at,"symbol":owner.symbol}));
                }
            }
            if let Some(field) = owner.bank_field {
                put(&mut payload, field, &u32::MAX.to_le_bytes())?;
                owner_patches.push(json!({"offset":field,"symbol":"motion-bank"}));
            }
            graph.add(&owner.symbol, owner.tag, &payload, None, owner_patches)?;
            for at in crate::tiger::entity::owner_slots(&self.entity, &owner.original, owner.tag)? {
                put(entity, at, &u32::MAX.to_le_bytes())?;
                patches.push(json!({"offset":at,"symbol":owner.symbol}));
            }
        }
        Ok(())
    }
}
