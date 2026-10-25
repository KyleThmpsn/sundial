//! Source-owned animation: the selected source controller compiled onto private native
//! first-person resources, and a standalone inspection export of the source closure.
//!
//! The weapon definition `80804A14` names the animation profile at `+314`, under its hold
//! class at `+310`, and the arm selection `808025AA` through the Tag64 at `+338`. The selection's
//! default arm entity at `+10` owns the arm skeleton, controls and `80803936` action lookup.
//! Runner alternatives at selection `+20` are not selected for a Guardian. Every action the
//! native state table can request is specialized through the source selector, and the clips it
//! selects are retargeted onto the native arm controls and a private weapon section.
use super::*;
use crate::tiger::animation::{clip as native_clip, control, pose::Pose};
use std::collections::{BTreeMap, BTreeSet};
mod clip;
mod compile;
mod controller;
mod cue;
mod donor;
mod native;
mod retarget;
mod source;
pub(super) use compile::compile;
pub(super) use donor::choose as choose_donor;
pub(super) use native::equipment_bones;

/// A source component: its tag, its owner payload, the owner's definition and that definition's
/// class.
type Component = (u32, std::sync::Arc<Payload>, usize, u32);

fn components(r: &mut Reader, entity: u32) -> Result<Vec<Component>> {
    let p = r.tag(entity, Some(0x8080BAAD))?;
    p.array(8, 12, Some(0x8080BAA2))?
        .into_iter()
        .map(|at| {
            let tag = p.u32(at)?;
            let owner = r.tag(tag, Some(0x8080BADB))?;
            let base = owner.pointer(24)?;
            let class = owner.u32(base.checked_sub(4).context("Source component definition")?)?;
            Ok((tag, owner, base, class))
        })
        .collect()
}

fn save(root: &Path, tag: u32, suffix: &str, payload: &Payload) -> Result<String> {
    let file = format!("source-animation/{tag:08X}-{suffix}.bin");
    fs::write(root.join(&file), &payload.0)?;
    Ok(file)
}

type Roles = BTreeMap<u32, BTreeSet<&'static str>>;

fn entities(source: &mut Reader, parts: &[Value], directory: &Path) -> Result<(Roles, Vec<Value>)> {
    let root = hash(
        parts.first().context("Empty source animation assembly")?,
        "entity",
    )?;
    fs::create_dir_all(directory.join("source-animation"))?;
    let mut selections = Vec::new();
    let mut entities = Roles::new();
    for part in parts {
        entities
            .entry(hash(part, "entity")?)
            .or_default()
            .insert("assembly");
    }
    for (owner, p, base, class) in components(source, root)? {
        if class == 0x80804A14 {
            let entity = source.ref64(&p, base + 0x320)?;
            if ![0, u32::MAX].contains(&entity) {
                entities.entry(entity).or_default().insert("attachment_320");
            }
            let selection = source.ref64(&p, base + 0x338)?;
            if ![0, u32::MAX].contains(&selection) {
                let data = source.tag(selection, Some(0x808025AA))?;
                let entity = source.ref64(&data, 0x10)?;
                if ![0, u32::MAX].contains(&entity) {
                    entities.entry(entity).or_default().insert("default_arms");
                }
                selections.push(json!({"owner":owner,"selection":selection,"default_entity":entity,
                    "file":save(directory,selection,"arm-selection",&data)?,
                    "limits":"Default arm selection only. Runner alternatives and native character retargeting are untranslated."}));
            }
        }
    }
    Ok((entities, selections))
}

fn export_clips(
    source: &mut Reader,
    bank: &Payload,
    directory: &Path,
    clips: &mut BTreeMap<u32, Value>,
) -> Result<Vec<u32>> {
    let mut members = Vec::new();
    for at in bank.array(8, 16, Some(0x8080AE00))? {
        let tag = source.ref64(bank, at)?;
        members.push(tag);
        if clips.contains_key(&tag) {
            continue;
        }
        let payload = source.tag(tag, Some(0x8080AE01))?;
        let original_file = save(directory, tag, "source", &payload)?;
        let converted = match clip::lower(&payload, None) {
            Ok((native, report)) => {
                json!({"source":tag,"status":"converted","source_file":original_file,
                "file":save(directory,tag,"native",&native)?,"report":report})
            }
            Err(error) => {
                json!({"source":tag,"status":"unconverted","source_file":original_file,"reason":format!("{error:#}")})
            }
        };
        clips.insert(tag, converted);
    }
    ensure!(
        !members.is_empty(),
        "Required source animation bank is empty"
    );
    Ok(members)
}

/// Export the selected source animation closure and checked native clip candidates. Every
/// controller remains explicitly unlinked until its source selectors and inputs are translated.
pub fn inspect(source: &mut Reader, parts: &[Value], directory: &Path) -> Result<Value> {
    let (entities, selections) = entities(source, parts, directory)?;
    let mut clips = BTreeMap::new();
    let mut controllers = Vec::new();
    for (entity, roles) in entities {
        let owners = components(source, entity)?;
        for &(tag, ref p, base, class) in &owners {
            let (bank_field, resources): (usize, &[(&str, usize, u32)]) = match class {
                0x8080397B => (
                    0x170,
                    &[
                        ("parameters", 0x178, 0x80803B54),
                        ("layers", 0x184, 0x808039DB),
                    ],
                ),
                0x80803936 => (
                    0xE8,
                    &[
                        ("dictionary", 0xEC, 0x8080ACBF),
                        ("states", 0xF4, 0x80803950),
                    ],
                ),
                _ => continue,
            };
            if [0, u32::MAX].contains(&p.u32(base + bank_field)?) {
                continue;
            }
            let bank_tag = p.u32(base + bank_field)?;
            let bank = source.tag(bank_tag, Some(0x80803BE9))?;
            let members = export_clips(source, &bank, directory, &mut clips)?;
            let mut files = json!({"owner":save(directory,tag,"owner",p)?,"bank":save(directory,bank_tag,"bank",&bank)?});
            for &(key, offset, class) in resources {
                let tag = p.u32(base + offset)?;
                if ![0, u32::MAX].contains(&tag) {
                    let data = source.tag(tag, Some(class))?;
                    files[key] = json!(save(directory, tag, key, &data)?);
                }
            }
            let mut rigs = Vec::new();
            for &(owner, ref data, base, class) in &owners {
                if ![0x80809FB7, 0x8080AD7E].contains(&class) {
                    continue;
                }
                rigs.push(json!({"owner":owner,"class":class,"definition":base,"file":save(directory,owner,"rig",data)?}));
            }
            controllers.push(json!({"entity":entity,"roles":roles,"owner":tag,"bank":bank_tag,"clips":members,"files":files,"rigs":rigs,
                "status":"unlinked","reason":"Source parameter inputs, layer activation and composition require controller translation"}));
        }
    }
    ensure!(
        !controllers.is_empty(),
        "Required source animation controller is absent"
    );
    Ok(
        json!({"revision":1,"controller_policy":"source_owned","controller_status":"unlinked","selections":selections,"controllers":controllers,
        "clips":clips.into_values().collect::<Vec<_>>(),"limits":"Native clip candidates only. No donor controller is accepted as a source controller. Source controller execution and gameplay remain unverified."}),
    )
}
