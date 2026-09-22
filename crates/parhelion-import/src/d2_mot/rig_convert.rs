//! Convert FK skeleton arrays and rigid or weighted geometry as an unlinked
//! prototype. Native animation/controller integration is a separate requirement.
use crate::d2_mot::{
    convert,
    payload::Payload,
    reader::{outside, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};
pub mod animation;
mod sections;

fn used_bones(root: &Path, item: &Value) -> Result<BTreeSet<usize>> {
    let report = load(&root.join("report.json"))?;
    ensure!(
        &report["item_tag"] == item,
        "rig geometry item identity differs"
    );
    let provenance = load(&root.join("source-manifest.json"))?;
    let mut used = BTreeSet::new();
    for entry in report["models"].as_array().context("rig source models")? {
        let model = raw(root, entry["model"].as_str().context("rig source model")?)?;
        for mesh in model.array(16, 128, Some(0x80806EC5))? {
            let tag = format!("{:08X}", model.u32(mesh)?);
            let header = raw(root, &tag)?;
            let reference = provenance["tags"][&tag]["reference"]
                .as_u64()
                .context("rig position buffer provenance")?;
            let data = raw(root, &format!("{reference:08X}"))?;
            ensure!(
                header.u16(4)? == 24
                    && header.u32(0)? as usize == data.0.len()
                    && data.0.len().is_multiple_of(24),
                "rig position buffer format differs"
            );
            let auxiliary = if data.0.chunks_exact(24).any(|v| {
                crate::d2_mot::skinning::selector(v).is_ok_and(crate::d2_mot::skinning::weighted)
            }) {
                let tag = format!("{:08X}", model.u32(mesh + 24)?);
                let reference = provenance["tags"][&tag]["reference"]
                    .as_u64()
                    .context("rig auxiliary buffer provenance")?;
                raw(root, &format!("{reference:08X}"))?.0
            } else {
                Vec::new()
            };
            used.extend(crate::d2_mot::skinning::used(&data.0, &auxiliary)?);
        }
    }
    ensure!(!used.is_empty(), "source rig has no draw vertices");
    Ok(used)
}

/// Reject source vertex encodings that no animation donor can translate.
pub(crate) fn check_source_skinning(root: &Path) -> Result<()> {
    let report = load(&root.join("report.json"))?;
    used_bones(root, &report["item_tag"])
        .map(|_| ())
        .map_err(crate::d2_mot::source_limit)
}
fn load(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn raw(root: &Path, tag: &str) -> Result<Payload> {
    Ok(Payload(fs::read(
        root.join("raw").join(format!("{tag}.bin")),
    )?))
}
fn match_bones(source: &[Value], target: &[Value]) -> Result<Vec<usize>> {
    source
        .iter()
        .map(|bone| {
            let name = bone["name_hash"].as_str().context("bone name")?;
            let matches = target
                .iter()
                .filter(|b| b["name_hash"] == name)
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1,
                "bone {name} missing or ambiguous in target rig"
            );
            Ok(usize::try_from(
                matches[0]["index"].as_u64().context("bone index")?,
            )?)
        })
        .collect()
}

/// Reuse native animation channels only when every source weapon bone has a
/// unique name and matching parent in the selected native weapon skeleton.
pub fn compatible_map(
    config: &Value,
    source_item_tag: &Value,
    native_item_tag: &Value,
) -> Result<Value> {
    let source =
        load(&Path::new(config["source"].as_str().context("source rig path")?).join("rig.json"))?;
    let native =
        load(&Path::new(config["native"].as_str().context("native rig path")?).join("rig.json"))?;
    ensure!(
        &source["item_tag"] == source_item_tag && &native["item_tag"] == native_item_tag,
        "rig item identity mismatch"
    );
    let bones = |rig: &Value, owner: &Value| -> Result<Vec<Value>> {
        let matches = rig["skeletons"]
            .as_array()
            .context("skeletons")?
            .iter()
            .filter(|s| &s["owner"] == owner)
            .collect::<Vec<_>>();
        ensure!(matches.len() == 1, "skeleton owner missing or ambiguous");
        Ok(matches[0]["bones"].as_array().context("bones")?.clone())
    };
    let from = bones(&source, &config["source_owner"])?;
    let to = bones(&native, &config["native_owner"])?;
    for set in [&from, &to] {
        ensure!(
            set.iter()
                .enumerate()
                .all(|(i, b)| b["index"].as_u64() == Some(i as u64)),
            "skeleton indices are not dense"
        );
    }
    let mut required = if let Some(geometry) = config["source_geometry"].as_str() {
        used_bones(Path::new(geometry), source_item_tag)?
    } else {
        (0..from.len()).collect()
    };
    // Unused bones may be absent from an older rig. Every used bone and all its
    // ancestors must still match by name and hierarchy. Unmapped slots remain
    // invalid, so later conversion cannot accidentally use a substitute bone.
    for index in required.clone() {
        let mut at = index;
        loop {
            let parent = from
                .get(at)
                .context("source vertex bone outside skeleton")?["parent"]
                .as_i64()
                .context("source bone parent")?;
            if parent == -1 {
                break;
            }
            let parent = usize::try_from(parent)?;
            ensure!(parent < at, "source bone hierarchy is not parent-first");
            required.insert(parent);
            at = parent;
        }
    }
    let mut mapping = Vec::new();
    for (index, bone) in from.iter().enumerate() {
        let matches = to
            .iter()
            .filter(|b| b["name_hash"] == bone["name_hash"])
            .collect::<Vec<_>>();
        if matches.is_empty() && !required.contains(&index) {
            mapping.push(u16::MAX as usize);
        } else {
            ensure!(
                matches.len() == 1,
                "required bone {} missing or ambiguous in native rig",
                bone["name_hash"]
            );
            mapping.push(usize::try_from(
                matches[0]["index"].as_u64().context("native bone index")?,
            )?);
        }
    }
    ensure!(
        !mapping.is_empty() && to.len() < 0x800,
        "unsupported native bone count"
    );
    for (i, bone) in from.iter().enumerate() {
        if !required.contains(&i) {
            continue;
        }
        let parent = bone["parent"].as_i64().context("source parent")?;
        let expected = if parent == -1 {
            -1
        } else {
            *mapping
                .get(usize::try_from(parent)?)
                .context("source parent index")? as i64
        };
        ensure!(
            to[mapping[i]]["parent"].as_i64() == Some(expected),
            "bone hierarchy mismatch"
        );
    }
    Ok(
        json!({"bone_map":mapping,"required_source_bones":required,"native_bone_count":to.len(),"source_bones":from,"native_bones":to,"source_owner":config["source_owner"],"native_owner":config["native_owner"],"native_animation_donor":native["item_tag"],"gameplay_verified":false}),
    )
}
fn write_array(
    target: &mut Vec<u8>,
    descriptor: usize,
    class: u32,
    count: usize,
    bytes: &[u8],
) -> Result<()> {
    let header = (target.len() + 19) & !15;
    target.resize(header - 4, 0);
    target.extend_from_slice(&0x80809FBDu32.to_le_bytes());
    target.extend_from_slice(&(count as u64).to_le_bytes());
    target.extend_from_slice(&(class as u64).to_le_bytes());
    target.extend_from_slice(bytes);
    target[descriptor..descriptor + 8].copy_from_slice(&(count as u64).to_le_bytes());
    target[descriptor + 8..descriptor + 16]
        .copy_from_slice(&(header as i64 - descriptor as i64 - 8).to_le_bytes());
    Ok(())
}
fn unlink_owner(payload: &mut Payload, owner: u32) -> Result<Vec<usize>> {
    let offsets = (0..payload.0.len().saturating_sub(3))
        .step_by(4)
        .filter(|&offset| payload.u32(offset).ok() == Some(owner))
        .collect::<Vec<_>>();
    ensure!(
        !offsets.is_empty(),
        "native skeleton self references missing"
    );
    // Owner references are typed triples: tag, class, absolute payload offset.
    // Validate every occurrence before replacing any of them.
    for &offset in &offsets {
        let class = payload.u32(offset + 4)?;
        let target = usize::try_from(u64::from_le_bytes(
            payload
                .0
                .get(offset + 8..offset + 16)
                .context("owner target")?
                .try_into()?,
        ))?;
        let scalar = [4, 8].into_iter().any(|back| {
            target.checked_sub(back).and_then(|at| payload.u32(at).ok()) == Some(class)
        });
        // Only the first element follows an array's type header. Later rig
        // sections must resolve to an exact row in the typed section array.
        let section = match class {
            0x80808544 => payload.array(payload.pointer(16)? + 0x30, 64, Some(class))?,
            0x80808549 => payload.array(payload.pointer(24)? + 0xD8, 24, Some(class))?,
            _ => Vec::new(),
        };
        ensure!(
            scalar || section.contains(&target),
            "unrecognized skeleton owner reference at {offset:X}"
        );
    }
    for &offset in &offsets {
        payload.0[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    }
    Ok(offsets)
}
/// Convert the complete source FK arrays into a native component envelope.
/// The result is unlinked. Animation/controller and entity connections must be
/// converted before a package may use it.
pub fn skeleton(source: &[u8], native: &[u8], native_owner: u32) -> Result<(Payload, Value)> {
    let modern = Payload(source.to_vec());
    let mut old = Payload(native.to_vec());
    let mr = modern.pointer(24)?;
    let nr = old.pointer(24)?;
    let owner_patches = unlink_owner(&mut old, native_owner)?;
    ensure!(
        modern.u32(mr - 4)? == 0x808081DE && old.u32(nr - 4)? == 0x80808546,
        "expected full FK skeletons"
    );
    let nodes = modern.array(mr + 0x90, 16, Some(0x80808642))?;
    let native_nodes = old.array(nr + 0x80, 16, Some(0x80808A08))?;
    ensure!(
        !nodes.is_empty() && nodes.len() < 0x800 && !native_nodes.is_empty(),
        "unsupported bone count"
    );
    ensure!(
        modern.u32(nodes[0])? == old.u32(native_nodes[0])?,
        "attachment root bone names differ"
    );
    for (i, &row) in nodes.iter().enumerate() {
        let parent = modern.u32(row + 4)? as i32;
        ensure!(
            parent == -1 || (parent >= 0 && (parent as usize) < i),
            "invalid parent order"
        );
        for o in [8, 12] {
            let link = modern.u32(row + o)? as i32;
            ensure!(
                link == -1 || (link >= 0 && (link as usize) < nodes.len()),
                "invalid hierarchy link"
            );
        }
    }
    let mut arrays = vec![];
    for (i, stride) in [16, 32, 32, 2, 2].into_iter().enumerate() {
        let from = mr + 0x90 + i * 16;
        let to = nr + 0x80 + i * 16;
        let source_class = [0x80808642, 0x80809F4F, 0x80809F4F, 0x80800006, 0x80800006][i];
        let native_class = [0x80808A08, 0x80809F75, 0x80809F75, 0x80800006, 0x80800006][i];
        let rows = modern.array(from, stride, Some(source_class))?;
        ensure!(
            rows.len() == nodes.len(),
            "skeleton parallel array mismatch"
        );
        let header = old.pointer(to + 8)?;
        let class = old.u32(header + 8)?;
        ensure!(
            class == native_class
                && old.array(to, stride, Some(native_class))?.len() == native_nodes.len(),
            "native skeleton parallel array contract differs"
        );
        let bytes = &modern.0[rows[0]..rows[0] + rows.len() * stride];
        if i == 1 || i == 2 {
            ensure!(
                bytes
                    .chunks_exact(4)
                    .all(|b| f32::from_le_bytes(b.try_into().unwrap()).is_finite()),
                "nonfinite transform"
            );
        }
        write_array(&mut old.0, to, class, rows.len(), bytes)?;
        arrays.push(json!({"descriptor":to,"count":rows.len(),"stride":stride,"class":format!("{class:08X}")}));
    }
    let sections = sections::convert(&modern, &mut old)?;
    let size = old.0.len();
    old.0[..8].copy_from_slice(&(size as u64).to_le_bytes());
    for (i, stride) in [16, 32, 32, 2, 2].into_iter().enumerate() {
        ensure!(
            old.array(nr + 0x80 + i * 16, stride, None)?.len() == nodes.len(),
            "converted skeleton count"
        );
    }
    Ok((
        old,
        json!({"bones":nodes.len(),"sections":sections,"arrays":arrays,"owner_patches":owner_patches}),
    ))
}

pub fn convert(
    source: &Path,
    native: &Path,
    model_source: &Path,
    out: &Path,
    source_owner: &str,
    native_owner: &str,
) -> Result<Value> {
    let out = outside(&outside(&outside(out, source)?, native)?, model_source)?;
    let modern = raw(source, source_owner)?;
    let old = raw(native, native_owner)?;
    let (old, skeleton) = skeleton(&modern.0, &old.0, u32::from_str_radix(native_owner, 16)?)?;
    let count = usize::try_from(skeleton["bones"].as_u64().context("converted bone count")?)?;
    let arrays = &skeleton["arrays"];
    let owner_patches = skeleton["owner_patches"]
        .as_array()
        .context("skeleton owner patches")?;
    let model_report = load(&model_source.join("report.json"))?;
    let models = model_report["models"].as_array().context("models")?;
    let provenance = load(&model_source.join("source-manifest.json"))?;
    let map = (0..count).map(|i| i as u16).collect::<Vec<_>>();
    // Validate every model and preserve the source's weighted selector and
    // auxiliary buffer. FK conversion no longer assumes a single rigid mesh.
    let mut geometry = Vec::new();
    for (model_index, entry) in models.iter().enumerate() {
        let model_tag = entry["model"].as_str().context("model tag")?;
        let model = raw(model_source, model_tag)?;
        for (mesh_index, mesh) in model
            .array(16, 128, Some(0x80806EC5))?
            .into_iter()
            .enumerate()
        {
            let buffer = |offset: usize| -> Result<Payload> {
                let tag = model.u32(mesh + offset)?;
                let reference = provenance["tags"][format!("{tag:08X}")]["reference"]
                    .as_u64()
                    .context("buffer reference")?;
                raw(model_source, &format!("{reference:08X}"))
            };
            let source_positions = buffer(0)?.0;
            let weighted = source_positions.chunks_exact(24).any(|v| {
                crate::d2_mot::skinning::selector(v).is_ok_and(crate::d2_mot::skinning::weighted)
            });
            let auxiliary = if weighted { buffer(24)?.0 } else { Vec::new() };
            let used = crate::d2_mot::skinning::used(&source_positions, &auxiliary)?;
            ensure!(
                used.iter().all(|i| *i < count),
                "geometry uses a bone outside the source skeleton"
            );
            let carrier =
                crate::d2_mot::skinning::carrier_positions(&source_positions, &auxiliary)?;
            let (positions, attributes) = convert::split_mapped(&carrier, &buffer(4)?.0, &map)?;
            let auxiliary = if weighted {
                crate::d2_mot::skinning::remap(&source_positions, &auxiliary, &map)?
            } else {
                Vec::new()
            };
            let selectors = source_positions
                .chunks_exact(24)
                .flat_map(|v| v[6..8].iter().copied())
                .collect::<Vec<_>>();
            geometry.push((
                model_index,
                mesh_index,
                model_tag.to_owned(),
                positions,
                attributes,
                auxiliary,
                selectors,
            ));
        }
    }
    fs::create_dir_all(&out)?;
    fs::write(out.join("skeleton.unlinked.bin"), &old.0)?;
    let mut meshes = Vec::new();
    for (model, mesh, tag, positions, attributes, auxiliary, selectors) in geometry {
        let prefix = format!("model-{model}-mesh-{mesh}");
        for (suffix, data) in [
            ("positions", &positions),
            ("attributes", &attributes),
            ("auxiliary", &auxiliary),
            ("source-selectors", &selectors),
        ] {
            fs::write(out.join(format!("{prefix}-{suffix}.bin")), data)?;
        }
        meshes.push(json!({"model":tag,"mesh":mesh,"prefix":prefix,"vertices":positions.len()/8,"weighted":!auxiliary.is_empty()}));
    }
    let rig = load(&source.join("rig.json"))?;
    let skeletons = rig["skeletons"].as_array().context("source skeletons")?;
    let weapon = skeletons
        .iter()
        .find(|s| s["owner"] == source_owner)
        .context("source skeleton owner")?;
    let weapon_bones = weapon["bones"].as_array().context("weapon bones")?;
    let mut first_person = vec![];
    for skeleton in skeletons {
        let bones = skeleton["bones"].as_array().context("rig bones")?;
        if bones.len() > weapon_bones.len() {
            if let Ok(map) = match_bones(weapon_bones, bones) {
                first_person.push(json!({"owner":skeleton["owner"],"bone_map":map}));
            }
        }
    }
    let patches = owner_patches
        .iter()
        .map(|offset| json!({"offset":offset,"symbol":"skeleton-owner"}))
        .collect::<Vec<_>>();
    let report = json!({"source_owner":source_owner,"native_template":native_owner,"bones":count,"bone_map":map,"meshes":meshes,"arrays":arrays,"patches":patches,"runtime_ready":false,"installed":false,"remaining":["enroll private skeleton owner into a compatible runtime entity","verify native animation tracks and bone palette loading","convert first-person rig bindings and animation clips"]});
    let mut report = report;
    report["source_first_person_bindings"] = json!(first_person);
    write_json(&out.join("rig-conversion.json"), &report)?;
    write_json(
        &out.join("source-animation-bindings.json"),
        &animation::inspect(source, true)?,
    )?;
    write_json(
        &out.join("native-animation-bindings.json"),
        &animation::inspect(native, false)?,
    )?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn skeleton_fixture(modern: bool, count: usize, owner: u32) -> Payload {
        let mut data = vec![0u8; 0x200];
        let resource = 0xA0usize;
        let class = if modern { 0x808081DEu32 } else { 0x80808546u32 };
        data[24..32].copy_from_slice(&((resource - 24) as u64).to_le_bytes());
        data[resource - 4..resource].copy_from_slice(&class.to_le_bytes());
        data[40..44].copy_from_slice(&owner.to_le_bytes());
        data[44..48].copy_from_slice(&class.to_le_bytes());
        data[48..56].copy_from_slice(&(resource as u64).to_le_bytes());
        let base = resource + if modern { 0x90 } else { 0x80 };
        for (array, stride) in [16, 32, 32, 2, 2].into_iter().enumerate() {
            let class = match array {
                0 => {
                    if modern {
                        0x80808642
                    } else {
                        0x80808A08
                    }
                }
                1 | 2 => {
                    if modern {
                        0x80809F4F
                    } else {
                        0x80809F75
                    }
                }
                _ => 0x80800006,
            };
            let mut rows = vec![0u8; count * stride];
            for i in 0..count {
                let at = i * stride;
                if array == 0 {
                    rows[at..at + 4].copy_from_slice(&(100u32 + i as u32).to_le_bytes());
                    rows[at + 4..at + 8].copy_from_slice(&(i as i32 - 1).to_le_bytes());
                    rows[at + 8..at + 16].fill(0xFF);
                } else if array <= 2 {
                    rows[at + 12..at + 16].copy_from_slice(&1f32.to_le_bytes());
                    rows[at + 16..at + 20].copy_from_slice(&(i as f32 * 0.25).to_le_bytes());
                }
            }
            write_array(&mut data, base + array * 16, class, count, &rows).unwrap();
        }
        Payload(data)
    }

    #[test]
    fn owner_references_resolve_later_section_rows_and_reject_interior_addresses() {
        let mut native = skeleton_fixture(false, 2, 22);
        let mut rows = vec![0; 48];
        rows[0..4].copy_from_slice(&22u32.to_le_bytes());
        rows[4..8].copy_from_slice(&0x80808546u32.to_le_bytes());
        rows[8..16].copy_from_slice(&0xA0u64.to_le_bytes());
        rows.copy_within(0..16, 24);
        write_array(&mut native.0, 0x178, 0x80808549, 2, &rows).unwrap();
        let second = native.array(0x178, 24, Some(0x80808549)).unwrap()[1];
        native.0[44..48].copy_from_slice(&0x80808549u32.to_le_bytes());
        native.0[48..56].copy_from_slice(&(second as u64).to_le_bytes());
        let mut valid = native.clone();
        let patches = unlink_owner(&mut valid, 22).unwrap();
        assert_eq!(patches.len(), 3);
        assert!(patches.iter().all(|at| valid.u32(*at).unwrap() == u32::MAX));
        native.0[48..56].copy_from_slice(&(second as u64 + 4).to_le_bytes());
        assert!(unlink_owner(&mut native, 22).is_err());
        assert_eq!(native.u32(40).unwrap(), 22);
    }

    #[test]
    fn source_fk_arrays_replace_native_counts_and_keep_bind_transforms() {
        let source = skeleton_fixture(true, 4, 11);
        let native = skeleton_fixture(false, 2, 22);
        let (converted, report) = skeleton(&source.0, &native.0, 22).unwrap();
        assert_eq!(report["bones"], 4);
        assert_eq!(report["owner_patches"], json!([40]));
        assert_eq!(converted.u32(40).unwrap(), u32::MAX);
        for (i, stride) in [16, 32, 32, 2, 2].into_iter().enumerate() {
            let from = source
                .array_range(0xA0 + 0x90 + i * 16, stride, None)
                .unwrap();
            let to = converted
                .array_range(0xA0 + 0x80 + i * 16, stride, None)
                .unwrap();
            assert_eq!(source.0[from], converted.0[to]);
        }
        assert_eq!(native.u32(40).unwrap(), 22);
        let mut invalid = source.clone();
        let rows = invalid.array(0x130, 16, None).unwrap();
        invalid.0[rows[1] + 4..rows[1] + 8].copy_from_slice(&(-2i32).to_le_bytes());
        assert!(skeleton(&invalid.0, &native.0, 22).is_err());
        invalid = source.clone();
        let rows = invalid.array(0x140, 32, None).unwrap();
        invalid.0[rows[0]..rows[0] + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(skeleton(&invalid.0, &native.0, 22).is_err());
    }
    #[test]
    fn compatible_animation_map_requires_matching_items_and_hierarchy() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let native = temp.path().join("native");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&native).unwrap();
        let bone =
            |index, name: &str, parent| json!({"index":index,"name_hash":name,"parent":parent});
        let from = json!({"item_tag":"SOURCE","skeletons":[{"owner":"A","bones":[bone(0,"ROOT",-1),bone(1,"MAG",0),bone(2,"BOLT",0)]}]});
        let mut to = json!({"item_tag":"NATIVE","skeletons":[{"owner":"B","bones":[bone(0,"ROOT",-1),bone(1,"BOLT",0),bone(2,"MAG",0)]}]});
        write_json(&source.join("rig.json"), &from).unwrap();
        write_json(&native.join("rig.json"), &to).unwrap();
        let config = json!({"source":source,"native":native,"source_owner":"A","native_owner":"B"});
        let mapping = compatible_map(&config, &json!("SOURCE"), &json!("NATIVE")).unwrap();
        assert_eq!(mapping["bone_map"], json!([0, 2, 1]));
        assert!(compatible_map(&config, &json!("OTHER"), &json!("NATIVE")).is_err());
        to["skeletons"][0]["bones"][2]["parent"] = json!(1);
        write_json(&native.join("rig.json"), &to).unwrap();
        assert!(compatible_map(&config, &json!("SOURCE"), &json!("NATIVE")).is_err());
    }
    #[test]
    fn missing_unused_bones_remain_invalid_and_used_ancestors_are_required() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let native = dir.path().join("native");
        fs::create_dir_all(source.join("raw")).unwrap();
        fs::create_dir_all(&native).unwrap();
        let bone = |index, name, parent| json!({"index":index,"name_hash":name,"parent":parent});
        let mut from = json!({"item_tag":"SOURCE","skeletons":[{"owner":"A","bones":[bone(0,"ROOT",-1),bone(1,"UNUSED",0),bone(2,"CHAMBER",0)]}]});
        let to = json!({"item_tag":"NATIVE","skeletons":[{"owner":"B","bones":[bone(0,"ROOT",-1),bone(1,"CHAMBER",0)]}]});
        write_json(&source.join("rig.json"), &from).unwrap();
        write_json(&native.join("rig.json"), &to).unwrap();
        write_json(
            &source.join("report.json"),
            &json!({"item_tag":"SOURCE","models":[{"model":"00000010"}]}),
        )
        .unwrap();
        write_json(
            &source.join("source-manifest.json"),
            &json!({"tags":{"00000011":{"reference":18}}}),
        )
        .unwrap();
        let mut model = vec![0; 0xA0];
        let mut mesh = vec![0; 128];
        mesh[..4].copy_from_slice(&17u32.to_le_bytes());
        write_array(&mut model, 16, 0x80806EC5, 1, &mesh).unwrap();
        fs::write(source.join("raw/00000010.bin"), model).unwrap();
        let mut header = vec![0; 12];
        header[..4].copy_from_slice(&48u32.to_le_bytes());
        header[4..6].copy_from_slice(&24u16.to_le_bytes());
        fs::write(source.join("raw/00000011.bin"), header).unwrap();
        let mut positions = vec![0; 48];
        positions[30..32].copy_from_slice(&2u16.to_le_bytes());
        fs::write(source.join("raw/00000012.bin"), &positions).unwrap();
        let config = json!({"source":source,"source_geometry":source,"native":native,"source_owner":"A","native_owner":"B"});
        let result = compatible_map(&config, &json!("SOURCE"), &json!("NATIVE")).unwrap();
        assert_eq!(result["bone_map"], json!([0, 65535, 1]));
        assert_eq!(result["required_source_bones"], json!([0, 2]));
        assert!(convert::split_mapped(&positions, &[0; 8], &[0, u16::MAX, 1]).is_ok());
        positions[30..32].copy_from_slice(&1u16.to_le_bytes());
        assert!(convert::split_mapped(&positions, &[0; 8], &[0, u16::MAX, 1]).is_err());
        fs::write(source.join("raw/00000012.bin"), &positions).unwrap();
        assert!(compatible_map(&config, &json!("SOURCE"), &json!("NATIVE")).is_err());
        positions[30..32].copy_from_slice(&2u16.to_le_bytes());
        fs::write(source.join("raw/00000012.bin"), &positions).unwrap();
        from["skeletons"][0]["bones"][2]["parent"] = json!(1);
        write_json(&source.join("rig.json"), &from).unwrap();
        assert!(compatible_map(&config, &json!("SOURCE"), &json!("NATIVE")).is_err());
    }

    #[test]
    fn owner_references_are_all_validated_before_unlinking() {
        let mut data = Payload(vec![0; 96]);
        for offset in [0, 16] {
            data.0[offset..offset + 4].copy_from_slice(&0x80BBC314u32.to_le_bytes());
            data.0[offset + 4..offset + 8].copy_from_slice(&0x80808546u32.to_le_bytes());
            data.0[offset + 8..offset + 16].copy_from_slice(&64u64.to_le_bytes());
        }
        data.0[60..64].copy_from_slice(&0x80808546u32.to_le_bytes());
        let original = data.0.clone();
        data.0[24..32].copy_from_slice(&96u64.to_le_bytes());
        assert!(unlink_owner(&mut data, 0x80BBC314).is_err());
        assert_eq!(data.u32(0).unwrap(), 0x80BBC314);
        data.0 = original;
        assert_eq!(unlink_owner(&mut data, 0x80BBC314).unwrap(), vec![0, 16]);
        assert_eq!(data.u32(0).unwrap(), u32::MAX);
        assert_eq!(data.u32(16).unwrap(), u32::MAX);
    }
    #[test]
    fn mapping_uses_names_not_positions_and_rejects_ambiguity() {
        let source = vec![json!({"name_hash":"A"}), json!({"name_hash":"B"})];
        let target = vec![
            json!({"name_hash":"B","index":9}),
            json!({"name_hash":"A","index":4}),
        ];
        assert_eq!(match_bones(&source, &target).unwrap(), vec![4, 9]);
        assert!(match_bones(&source, &target[..1]).is_err());
        let mut duplicate = target.clone();
        duplicate.push(target[0].clone());
        assert!(match_bones(&source, &duplicate).is_err());
    }
}
