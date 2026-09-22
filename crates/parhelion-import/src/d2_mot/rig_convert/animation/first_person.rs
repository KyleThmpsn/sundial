//! Substitute converted source first-person clips into private copies of the
//! native donor's first-person animation chain, matched by clip name.
//!
//! The native runtime entity reaches its first-person entity through the
//! attachment table owner. That entity's lookup owner names a clip bank whose
//! rows are clip tags. Every link is a plain tag word, so package authoring can
//! copy the chain and rewrite each link. Controllers and state tables stay
//! native and address clips by bank ordinal, so the bank keeps its row order
//! and only rows whose source clip converted are replaced. This requires the
//! two first-person skeletons to be identical in bone names, order and parents.
use super::clips;
use crate::d2_mot::reader::{Reader, write_json};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const NATIVE_LOOKUP: &str = "8080344B";
const SOURCE_LOOKUP: &str = "808025F8";
const NATIVE_ATTACHMENTS: &str = "80804221";

/// The first-person entity of a rig report with its skeleton and lookup owner.
struct FirstPerson {
    entity: u32,
    bones: Vec<Value>,
    lookup_owner: u32,
}

fn hex(v: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(v.as_str().context("tag text")?, 16)?)
}

fn first_person(rig: &Value, lookup_class: &str) -> Result<Option<FirstPerson>> {
    let runtime = rig["runtime_entity"].as_str().context("runtime entity")?;
    let components = rig["components"].as_array().context("rig components")?;
    let mut entities = components
        .iter()
        .filter_map(|c| c["entity"].as_str())
        .filter(|e| *e != runtime)
        .collect::<Vec<_>>();
    entities.sort_unstable();
    entities.dedup();
    let entity = match entities.as_slice() {
        [] => return Ok(None),
        [one] => *one,
        _ => anyhow::bail!("more than one attached animation entity"),
    };
    let owners = components
        .iter()
        .filter(|c| c["entity"] == entity)
        .collect::<Vec<_>>();
    let skeletons = rig["skeletons"]
        .as_array()
        .context("rig skeletons")?
        .iter()
        .filter(|s| owners.iter().any(|c| c["owner"] == s["owner"]))
        .collect::<Vec<_>>();
    ensure!(
        skeletons.len() == 1,
        "first-person entity skeleton missing or ambiguous"
    );
    let lookups = owners
        .iter()
        .filter(|c| c["class"] == lookup_class)
        .collect::<Vec<_>>();
    ensure!(
        lookups.len() == 1,
        "first-person animation lookup missing or ambiguous"
    );
    Ok(Some(FirstPerson {
        entity: u32::from_str_radix(entity, 16)?,
        bones: skeletons[0]["bones"]
            .as_array()
            .context("skeleton bones")?
            .clone(),
        lookup_owner: hex(&lookups[0]["owner"])?,
    }))
}

/// Native controllers drive bone ordinals directly, so substituted clips need
/// the same skeleton: equal length and, per index, equal name and parent.
fn identical(source: &[Value], native: &[Value]) -> bool {
    source.len() == native.len()
        && source.iter().zip(native).enumerate().all(|(i, (a, b))| {
            a["index"].as_u64() == Some(i as u64)
                && b["index"].as_u64() == Some(i as u64)
                && a["name_hash"] == b["name_hash"]
                && a["parent"] == b["parent"]
        })
}

/// Read a lookup owner's bank rows as clip tags with the clip names they carry.
fn bank(r: &mut Reader, owner: u32, modern: bool) -> Result<(u32, Vec<(u32, u32)>)> {
    let p = r.tag(owner, Some(if modern { 0x80809B06 } else { 0x80809C36 }))?;
    let bank_tag = p.u32(p.pointer(24)? + if modern { 0xA8 } else { 0x90 })?;
    let bank = r.tag(bank_tag, Some(if modern { 0x8080289F } else { 0x808036F6 }))?;
    let mut clips = Vec::new();
    for row in bank.array(
        8,
        if modern { 16 } else { 4 },
        Some(if modern { 0x80808BDF } else { 0x80808F48 }),
    )? {
        let tag = if modern {
            r.ref64(&bank, row)?
        } else {
            bank.u32(row)?
        };
        let clip = r.tag(tag, Some(if modern { 0x80808BE0 } else { 0x80808F49 }))?;
        ensure!(
            clip.0.len() >= 0x190 && clip.u64(0)? == clip.0.len() as u64,
            "animation clip payload size differs"
        );
        clips.push((tag, clip.u32(0x120)?));
    }
    Ok((bank_tag, clips))
}

/// Pair native and source clips that share a name unique on both sides.
/// Returns `(native, source, name)` in native bank order.
fn matched(native: &[(u32, u32)], source: &[(u32, u32)]) -> Vec<(u32, u32, u32)> {
    let count = |clips: &[(u32, u32)]| {
        let mut by_name: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (tag, name) in clips {
            by_name.entry(*name).or_default().push(*tag);
        }
        by_name
    };
    let source_names = count(source);
    let native_names = count(native);
    native
        .iter()
        .filter_map(
            |(tag, name)| match (native_names.get(name), source_names.get(name)) {
                (Some(n), Some(s)) if n.len() == 1 && s.len() == 1 => Some((*tag, s[0], *name)),
                _ => None,
            },
        )
        .collect()
}

fn word_occurrences(payload: &[u8], tag: u32) -> usize {
    payload
        .chunks_exact(4)
        .filter(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == tag)
        .count()
}

/// Prepare the first-person clip substitution for one import. The result is
/// the graph's `animation` section: `first_person` is present only when the
/// chain was linked, otherwise `reason` says why the native animation stays.
pub fn prepare(
    modern_packages: &Path,
    native_packages: &Path,
    source_rig: &Value,
    native_rig: &Value,
    work: &Path,
    graph: &Path,
) -> Result<Value> {
    let unavailable = |reason: String| json!({"first_person_status":"native","reason":reason,"gameplay_verified":false});
    let (Some(source), Some(native)) = (
        first_person(source_rig, SOURCE_LOOKUP)?,
        first_person(native_rig, NATIVE_LOOKUP)?,
    ) else {
        return Ok(unavailable(
            "source or native rig has no first-person entity".into(),
        ));
    };
    if !identical(&source.bones, &native.bones) {
        return Ok(unavailable(format!(
            "first-person skeletons differ: source {} bones, native {} bones",
            source.bones.len(),
            native.bones.len()
        )));
    }
    let runtime_entity = hex(&native_rig["runtime_entity"])?;
    let attachment_owners = native_rig["components"]
        .as_array()
        .context("native components")?
        .iter()
        .filter(|c| c["class"] == NATIVE_ATTACHMENTS && c["entity"] == native_rig["runtime_entity"])
        .map(|c| hex(&c["owner"]))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        attachment_owners.len() == 1,
        "native first-person attachment owner missing or ambiguous"
    );
    let attachment_owner = attachment_owners[0];

    let mut sr = Reader::new(modern_packages, &work.join("source"), true)?;
    let (_, source_clips) = bank(&mut sr, source.lookup_owner, true)?;
    let mut nr = Reader::new(native_packages, &work.join("native"), false)?;
    let (bank_tag, native_clips) = bank(&mut nr, native.lookup_owner, false)?;
    let attachments = nr.tag(attachment_owner, Some(0x80809C36))?;
    let entity = nr.tag(native.entity, Some(0x80809C0F))?;
    let lookup = nr.tag(native.lookup_owner, Some(0x80809C36))?;
    let bank_payload = nr.tag(bank_tag, Some(0x808036F6))?;
    ensure!(
        word_occurrences(&attachments.0, native.entity) > 0
            && word_occurrences(&entity.0, native.lookup_owner) > 0
            && word_occurrences(&lookup.0, bank_tag) > 0,
        "native first-person chain links are not plain tag words"
    );

    let out = graph.join("animation");
    fs::create_dir_all(&out)?;
    let mut files = BTreeMap::new();
    for (key, name, payload) in [
        (
            "attachment_owner",
            "first-person-attachment-owner.bin",
            &attachments.0,
        ),
        ("entity", "first-person-entity.bin", &entity.0),
        ("lookup_owner", "first-person-lookup-owner.bin", &lookup.0),
        ("bank", "first-person-bank.bin", &bank_payload.0),
    ] {
        fs::write(out.join(name), payload)?;
        files.insert(key, format!("animation/{name}"));
    }
    let mut converted = Vec::new();
    let mut unconverted = Vec::new();
    let mut with_events = 0;
    for (native_tag, source_tag, name) in matched(&native_clips, &source_clips) {
        let source_clip = sr.tag(source_tag, Some(0x80808BE0))?;
        // Event-bearing clips ride on the native counterpart's event block when
        // both clips carry the same events. Otherwise the native clip stays.
        let result = clips::convert(&source_clip.0).or_else(|error| {
            if !format!("{error:#}").contains("event records") {
                return Err(error);
            }
            let counterpart = nr.tag(native_tag, Some(0x80808F49))?;
            clips::convert_with_events(&source_clip.0, &counterpart.0)
                .with_context(|| "event carry")
        });
        match result {
            Ok((payload, report)) => {
                let file = format!("animation/clip-{native_tag:08X}.bin");
                fs::write(graph.join(&file), &payload.0)?;
                if report["event_count"].as_u64().unwrap_or(0) > 0 {
                    with_events += 1;
                }
                converted.push(json!({"native":native_tag,"source":source_tag,"name":name,"file":file,"streams":report["streams"],"events":report["events"]}));
            }
            Err(error) => unconverted.push(
                json!({"native":native_tag,"source":source_tag,"name":name,"reason":format!("{error:#}")}),
            ),
        }
    }
    let unmatched = native_clips.len() - converted.len() - unconverted.len();
    sr.finish()?;
    nr.finish()?;
    let section = json!({
        "first_person_status": if converted.is_empty() { "native" } else { "linked" },
        "runtime_entity": runtime_entity,
        "first_person": {
            "attachment_owner": attachment_owner,
            "entity": native.entity,
            "lookup_owner": native.lookup_owner,
            "bank": bank_tag,
            "skeleton_bones": native.bones.len(),
            "native_clips": native_clips.len(),
            "source_clips": source_clips.len(),
            "unmatched_native_clips": unmatched,
            "clips_with_carried_events": with_events,
            "files": files,
            "clips": converted,
            "unconverted": unconverted,
        },
        "gameplay_verified": false,
    });
    write_json(&out.join("animation.json"), &section)?;
    Ok(section)
}

/// Where a prepared graph keeps its animation payloads.
pub fn directory(graph: &Path) -> PathBuf {
    graph.join("animation")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bone(index: u64, name: &str, parent: i64) -> Value {
        json!({"index":index,"name_hash":name,"parent":parent})
    }

    #[test]
    fn skeletons_must_agree_on_names_order_and_parents() {
        let a = vec![
            bone(0, "root", -1),
            bone(1, "hand", 0),
            bone(2, "finger", 1),
        ];
        assert!(identical(&a, &a));
        let mut renamed = a.clone();
        renamed[2] = bone(2, "thumb", 1);
        assert!(!identical(&a, &renamed));
        let mut reparented = a.clone();
        reparented[2] = bone(2, "finger", 0);
        assert!(!identical(&a, &reparented));
        let swapped = vec![
            bone(0, "root", -1),
            bone(1, "finger", 0),
            bone(2, "hand", 1),
        ];
        assert!(!identical(&a, &swapped));
        assert!(!identical(&a, &a[..2]));
        let mut sparse = a.clone();
        sparse[1] = bone(5, "hand", 0);
        assert!(!identical(&sparse, &sparse));
    }

    #[test]
    fn clips_pair_only_through_names_unique_on_both_sides() {
        let native = [
            (0x80C0_0001, 10),
            (0x80C0_0002, 20),
            (0x80C0_0003, 30),
            (0x80C0_0004, 30),
        ];
        let source = [
            (0x80A0_0009, 20),
            (0x80A0_0008, 10),
            (0x80A0_0007, 40),
            (0x80A0_0006, 20),
        ];
        // Name 20 repeats in the source and name 30 repeats in the native bank.
        assert_eq!(
            matched(&native, &source),
            vec![(0x80C0_0001, 0x80A0_0008, 10)]
        );
        assert!(matched(&native, &[]).is_empty());
    }

    #[test]
    fn first_person_entity_is_the_attached_entity_with_one_skeleton_and_lookup() {
        let rig = json!({
            "runtime_entity": "80C69D8C",
            "components": [
                {"entity":"80C69D8C","owner":"80C239CC","class":"80808546"},
                {"entity":"80C69D8C","owner":"80C69D2D","class":"8080344B"},
                {"entity":"80C69D88","owner":"80C69D33","class":"80808546"},
                {"entity":"80C69D88","owner":"81525320","class":"8080344B"}
            ],
            "skeletons": [
                {"owner":"80C239CC","bones":[bone(0,"root",-1)]},
                {"owner":"80C69D33","bones":[bone(0,"root",-1),bone(1,"hand",0)]}
            ]
        });
        let fp = first_person(&rig, NATIVE_LOOKUP).unwrap().unwrap();
        assert_eq!(
            (fp.entity, fp.lookup_owner, fp.bones.len()),
            (0x80C69D88, 0x81525320, 2)
        );
        let mut weapon_only = rig.clone();
        weapon_only["components"] =
            json!([{"entity":"80C69D8C","owner":"80C239CC","class":"80808546"}]);
        assert!(first_person(&weapon_only, NATIVE_LOOKUP).unwrap().is_none());
        let mut no_lookup = rig.clone();
        no_lookup["components"][3]["class"] = json!("80803640");
        assert!(first_person(&no_lookup, NATIVE_LOOKUP).is_err());
    }

    #[test]
    fn tag_words_are_counted_on_aligned_boundaries_only() {
        let mut bytes = vec![0u8; 16];
        bytes[4..8].copy_from_slice(&0x8152_5320u32.to_le_bytes());
        bytes[9..13].copy_from_slice(&0x8152_5320u32.to_le_bytes());
        assert_eq!(word_occurrences(&bytes, 0x8152_5320), 1);
    }
}
