use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use tiger_pkg::{DestinyVersion, GameVersion, PackageManager, TagHash};
pub(crate) mod channels;
mod draws;
mod loading;
fn entity_map_arrays(p: &Payload) -> Result<()> {
    ensure!(p.u64(0)? == p.0.len() as u64, "entity map size mismatch");
    let rows = p.array(8, 8, Some(0x80809252))?;
    let auxiliary = p.array(24, 4, Some(0x8080000B))?;
    ensure!(
        auxiliary.len() >= rows.len().div_ceil(32),
        "entity map auxiliary bit capacity is too small"
    );
    let end = rows.last().context("empty entity map")? + 8;
    ensure!(
        p.pointer(32)? == end + 8,
        "entity map auxiliary overlaps rows"
    );
    ensure!(!auxiliary.is_empty(), "entity map auxiliary missing");
    for row in auxiliary {
        ensure!(
            p.u32(row)? == 0,
            "entity map auxiliary is not zero-initialized"
        );
    }
    Ok(())
}
fn require_loading_closure(parent: &BTreeSet<u32>, root: &BTreeSet<u32>) -> Result<()> {
    ensure!(
        parent.is_subset(root),
        "weapon-local loading index lacks retained native dependencies: {:?}",
        parent
            .difference(root)
            .map(|t| format!("{t:08X}"))
            .collect::<Vec<_>>()
    );
    Ok(())
}

fn mapped_bone_index(
    source_index: usize,
    target_index: usize,
    native_count: usize,
    required: &BTreeSet<usize>,
) -> Result<Option<usize>> {
    if target_index == usize::from(u16::MAX) {
        ensure!(
            !required.contains(&source_index),
            "required source bone {source_index} has no native mapping"
        );
        return Ok(None);
    }
    ensure!(target_index < native_count, "native bone outside skeleton");
    Ok(Some(target_index))
}
fn art_slots(p: &Payload, row: usize) -> Result<Vec<(u64, Vec<u32>)>> {
    let resources = p.array(24, 24, None)?;
    p.array(row + 16, 8, Some(0x80805DFE))?
        .into_iter()
        .map(|o| {
            let resource = p.pointer(o)?;
            ensure!(
                resources.binary_search(&resource).is_ok(),
                "art slot points outside registered resource table"
            );
            Ok((
                p.u64(resource)?,
                p.array(resource + 8, 4, Some(0x80805E01))?
                    .into_iter()
                    .map(|a| p.u32(a))
                    .collect::<Result<Vec<_>>>()?,
            ))
        })
        .collect()
}
pub(crate) fn dependencies(p: &Payload) -> Result<BTreeSet<u32>> {
    let mut out = BTreeSet::new();
    for row in p.array(16, 40, Some(0x80809EFB))? {
        let pkg = u16::try_from(p.u64(row)?)?;
        for (i, b) in p
            .array(row + 8, 4, Some(0x8080000B))?
            .into_iter()
            .enumerate()
        {
            let bits = p.u32(b)?;
            for bit in 0..32 {
                if bits & (1 << bit) != 0 {
                    out.insert(TagHash::new(pkg, u16::try_from(i * 32 + bit)?).0);
                }
            }
        }
        for o in p.array(row + 24, 2, Some(0x8080000A))? {
            out.insert(TagHash::new(pkg, p.u16(o)?).0);
        }
    }
    Ok(out)
}
pub fn audit(packages: &Path, stage: &Path, graph: &Path) -> Result<Value> {
    let version = GameVersion::Destiny(DestinyVersion::Destiny2Shadowkeep);
    let base = PackageManager::new(packages, version, None)?;
    let staged = PackageManager::new(stage, version, None)?;
    audit_graph(&base, &staged, graph, &BTreeSet::new())
}

/// Reuses package indexes while checking each weapon's complete asset graph.
/// A previously verified generation can supply existing weapon dependencies.
/// Imported model assets must still stay outside the global loading root.
pub fn audit_many(
    packages: &Path,
    stage: &Path,
    graphs: &[PathBuf],
    baseline: Option<&Path>,
) -> Result<Vec<Value>> {
    let version = GameVersion::Destiny(DestinyVersion::Destiny2Shadowkeep);
    let base = PackageManager::new(packages, version, None)?;
    let staged = PackageManager::new(stage, version, None)?;
    let baseline_root = baseline
        .map(|path| -> Result<BTreeSet<u32>> {
            let manager = PackageManager::new(path, version, None)?;
            dependencies(&Payload(manager.read_tag(TagHash(0x80EE8CBD))?))
        })
        .transpose()?
        .unwrap_or_default();
    graphs
        .iter()
        .map(|graph| {
            let result = audit_graph(&base, &staged, graph, &baseline_root)
                .with_context(|| format!("Auditing {}", graph.display()))?;
            eprintln!("Verified {}", graph.display());
            Ok(result)
        })
        .collect()
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
fn audit_graph(
    base: &PackageManager,
    staged: &PackageManager,
    graph: &Path,
    baseline_root: &BTreeSet<u32>,
) -> Result<Value> {
    let read = |tag: u32| -> Result<Payload> {
        let h = TagHash(tag);
        if staged.package_paths.contains_key(&h.pkg_id()) {
            Ok(Payload(staged.read_tag(h).or_else(|_| base.read_tag(h))?))
        } else {
            Ok(Payload(base.read_tag(h)?))
        }
    };
    let symbols: Value = serde_json::from_slice(&fs::read(graph.join("allocated.json"))?)?;
    let tag = |s: &str| -> Result<u32> {
        Ok(u32::try_from(
            symbols[s]
                .as_u64()
                .with_context(|| format!("missing {s}"))?,
        )?)
    };
    let nodes: Value = serde_json::from_slice(&fs::read(graph.join("asset-graph.json"))?)?;
    let runtime_map = read(0x80EC3F60)?;
    entity_map_arrays(&runtime_map).context("runtime entity lookup auxiliary array")?;
    let item_hash = nodes["item_hash"].as_u64().context("item hash")? as u32;
    let art_key = nodes["art_key"].as_u64().context("art key")? as u32;
    let source_owner = nodes["source_owner"].as_u64().unwrap_or(0x80EC2727) as u32;
    let donor_item = nodes["native_item"].as_u64().unwrap_or(0x02222CBF) as u32;
    let donor_key = nodes["native_assignment"].as_u64().unwrap_or(0xCFBE7264) as u32;
    for n in nodes["nodes"].as_array().context("nodes")? {
        let name = n["symbol"].as_str().context("symbol")?;
        let t = tag(name)?;
        let actual = read(t)?;
        ensure!(
            actual.0 == fs::read(graph.join("linked").join(format!("{name}.bin")))?,
            "staged payload differs: {name}"
        );
        for patch in n["patches"].as_array().context("patches")? {
            ensure!(
                actual.u32(patch["offset"].as_u64().context("offset")? as usize)?
                    == tag(patch["symbol"].as_str().context("target")?)?,
                "unresolved fixup"
            )
        }
        if let Some(target) = n["reference"].as_str() {
            ensure!(
                staged.get_entry(TagHash(t)).context("entry")?.reference == tag(target)?,
                "buffer reference mismatch"
            )
        }
    }
    let companion = read(tag("parent-companion")?)?;
    if symbols.get("object-channels").is_some() {
        channels::interpolation(
            &read(tag("object-channels")?)?,
            &read(tag("object-channel-allocation")?)?,
        )?;
    }
    ensure!(
        companion.u32(8)? == tag("parent-companion")? && companion.u32(12)? == tag("parent")?,
        "private parent loading identity mismatch"
    );
    let root_deps = dependencies(&read(0x80EE8CBD)?)?;
    let loading = loading::audit(base, staged, &nodes, &symbols, &read, baseline_root)?;
    for (_, v) in symbols.as_object().context("symbols")? {
        let t = v.as_u64().context("tag")? as u32;
        ensure!(
            !root_deps.contains(&t),
            "private tag {t:08X} missing loading enrollment"
        )
    }
    let parent = read(tag("parent")?)?;
    ensure!(parent.u32(16)? == tag("entity")?, "parent not linked");
    let entity = read(tag("entity")?)?;
    crate::d2_mot::entity::reject_stale_owner(&entity, source_owner)?;
    ensure!(
        entity
            .array(16, 12, Some(0x80809C04))?
            .iter()
            .any(|&o| entity.u32(o).ok() == Some(tag("owner").unwrap())),
        "entity owner not linked"
    );
    let owner = read(tag("owner")?)?;
    ensure!(
        owner.u32(nodes["model_slot"].as_u64().unwrap_or(0x83C) as usize)? == tag("model")?
            && owner.u32(nodes["plates_slot"].as_u64().unwrap_or(0x8A8) as usize)?
                == tag("plates")?,
        "model/plate owner not linked"
    );
    let global_tag = base
        .lookup
        .named_tags
        .iter()
        .find(|e| e.name == "investment_globals")
        .context("globals")?
        .hash
        .0;
    let globals = read(global_tag)?;
    let root = read(globals.u32(16)?)?;
    let items = read(root.u32(8 + 48 * 16)?)?;
    if let Some(ornament) = crate::d2_mot::ornament_audit::check(&nodes, &globals, &items, &read)? {
        eprintln!("Ornament socket audit: {ornament}");
    }
    let rows = items.array(8, 24, None)?;
    let row = *rows
        .iter()
        .find(|&&o| items.u32(o).ok() == Some(item_hash))
        .context("Ergo Same missing")?;
    let item = read(items.u32(row + 16)?)?;
    crate::d2_mot::arrays::validate(&item.0).map_err(anyhow::Error::msg)?;
    if let Some(dyes) = nodes["dyes"].as_array() {
        let translation = item.pointer(0x88)?;
        ensure!(
            item.u64(translation + 0x28)? == 0 && item.u64(translation + 0x38)? == 0,
            "donor dyes remain active"
        );
        let locked = item.array(translation + 0x48, 4, None)?;
        ensure!(locked.len() == dyes.len(), "locked dye count mismatch");
        let dye_table = read(0x81613D24)?;
        let dye_rows = dye_table.array(8, 8, None)?;
        let assignment = read(0x80EC3F60)?;
        let assignment_rows = assignment.array(8, 8, None)?;
        for dye in dyes {
            let channel = dye["channel"].as_u64().context("dye channel")? as u8;
            let key = dye["manifest"].as_u64().context("dye key")? as u32;
            let row = *locked
                .iter()
                .find(|&&o| item.u8(o).ok() == Some(channel))
                .context("private dye channel missing")?;
            let entry = *dye_rows
                .get(item.u16(row + 2)? as usize)
                .context("dye table index")?;
            ensure!(
                dye_table.u32(entry + 4)? == key,
                "private dye index resolves incorrectly"
            );
            let relation = *assignment_rows
                .iter()
                .find(|&&o| assignment.u32(o).ok() == Some(key))
                .context("dye assignment missing")?;
            ensure!(
                assignment.u32(relation + 4)?
                    == tag(dye["parent"].as_str().context("dye parent")?)?,
                "dye parent mismatch"
            );
        }
        for node in nodes["nodes"].as_array().context("nodes")? {
            if let Some(owner) = node["shared_owner"].as_str() {
                let c = read(tag(node["symbol"].as_str().context("companion")?)?)?;
                ensure!(c.u32(12)? == tag(owner)?, "dye companion owner mismatch");
                ensure!(
                    dependencies(&c)?.contains(&tag(owner)?),
                    "dye owner not enrolled"
                );
            }
        }
    }
    let art = item.array(item.pointer(0x88)?, 4, None)?;
    ensure!(
        art.len() == 1 && item.u8(art[0])? == 255,
        "class restriction returned"
    );
    let metadata = read(globals.u32(16 + 66 * 16)?)?;
    let metadata_rows = metadata.array(8, 32, None)?;
    let m = *metadata_rows
        .get(item.u16(art[0] + 2)? as usize)
        .context("art index")?;
    ensure!(
        metadata.u32(m)? == item_hash,
        "private item artwork not selected"
    );
    let donor = if let Some(&row) = metadata_rows
        .iter()
        .find(|&&o| metadata.u32(o).ok() == Some(donor_item))
    {
        row
    } else {
        let native_row = *rows
            .iter()
            .find(|&&row| items.u32(row).ok() == Some(donor_item))
            .context("native artwork item missing")?;
        let native_definition = Payload(base.read_tag(TagHash(items.u32(native_row + 16)?))?);
        let mut candidates = BTreeSet::new();
        for selector in
            native_definition.array(native_definition.pointer(0x88)?, 4, Some(0x808077B5))?
        {
            let row = *metadata_rows
                .get(native_definition.u16(selector + 2)? as usize)
                .context("native artwork alias index")?;
            let mut keys = vec![metadata.u32(row + 8)?, metadata.u32(row + 12)?];
            for (_, slot_keys) in art_slots(&metadata, row)? {
                keys.extend(slot_keys);
            }
            if keys.contains(&donor_key) {
                candidates.insert(row);
            }
        }
        ensure!(
            candidates.len() == 1,
            "native artwork alias missing or ambiguous"
        );
        *candidates.first().context("native artwork alias")?
    };
    let mut expected_slots = art_slots(&metadata, donor)?;

    // The imported graph already contains the assembled body and attachments.
    // Check placement independently of the staging adapter's policy helper.
    let singles = if expected_slots.is_empty() {
        [art_key, 0x811C9DC5]
    } else {
        ensure!(
            expected_slots
                .iter()
                .filter(|(selector, _)| *selector == 0)
                .count()
                == 1,
            "private assembled artwork requires one body selector"
        );
        for (selector, keys) in &mut expected_slots {
            keys.fill(0x811C9DC5);
            if *selector == 0 {
                *keys.first_mut().context("empty native body selector")? = art_key;
            }
        }
        [0x811C9DC5; 2]
    };
    ensure!(
        [metadata.u32(m + 8)?, metadata.u32(m + 12)?] == singles
            && art_slots(&metadata, m)? == expected_slots,
        "private assembled artwork retains donor geometry or uses the wrong slot"
    );
    // Every original item's selectors and assignment keys must survive insertion.
    let original_metadata = Payload(base.read_tag(TagHash(globals.u32(16 + 66 * 16)?))?);
    for old in original_metadata.array(8, 32, None)? {
        if original_metadata.u32(old)? == item_hash {
            continue;
        }
        let new = *metadata_rows
            .iter()
            .find(|&&o| metadata.u32(o).ok() == original_metadata.u32(old).ok())
            .context("metadata row removed")?;
        ensure!(
            art_slots(&metadata, new)? == art_slots(&original_metadata, old)?,
            "unrelated art slots changed"
        );
    }
    let assignments = read(0x80EC3F61)?;
    let ar = assignments.array(8, 8, None)?;
    let mut assignment_map = std::collections::BTreeMap::new();
    for &row in &ar {
        ensure!(
            assignment_map
                .insert(assignments.u32(row)?, assignments.u32(row + 4)?)
                .is_none(),
            "duplicate art assignment key"
        );
    }
    let stock_assignments = Payload(base.read_tag(TagHash(0x80EC3F61))?);
    for row in stock_assignments.array(8, 8, None)? {
        ensure!(
            assignment_map.get(&stock_assignments.u32(row)?)
                == Some(&stock_assignments.u32(row + 4)?),
            "stock art assignment changed or disappeared"
        );
    }
    let a = *ar
        .iter()
        .find(|&&o| assignments.u32(o).ok() == Some(art_key))
        .context("private art assignment missing")?;
    ensure!(
        assignments.u32(a + 4)? == tag("parent")?,
        "private artwork parent mismatch"
    );
    let model = read(tag("model")?)?;
    if nodes.get("material_adapter").is_some() {
        let native_scale = model.f32(0x6C)?;
        ensure!(
            native_scale > 0.0
                && [0x50, 0x54, 0x58]
                    .into_iter()
                    .all(|offset| model.f32(offset).ok() == Some(native_scale)),
            "merged model disagrees with the native shader's uniform position scale"
        );
    }
    let mesh = model.array(16, 136, Some(0x80807378))?[0];
    ensure!(
        model.u32(0x9C)? == 0x80809FBD && model.u32(0x13C)? == 0x80809FBD,
        "native array markers missing"
    );
    let parts = model.array(mesh + 24, 32, Some(0x8080737E))?;
    draws::validate(&model, mesh, &parts)?;
    ensure!(
        parts.len() == nodes["native_draw_parts"].as_u64().unwrap_or(24) as usize,
        "draw parts mismatch"
    );
    if let Some(rig) = nodes.get("rig_mapping") {
        let owner_tag = u32::from_str_radix(
            rig["native_owner"]
                .as_str()
                .context("native skeleton owner")?,
            16,
        )?;
        let owner = read(owner_tag)?;
        let resource = owner.pointer(24)?;
        ensure!(
            owner.u32(resource - 4)? == 0x80808546,
            "native FK skeleton class changed"
        );
        let bones = owner.array(resource + 0x80, 16, Some(0x80808A08))?;
        ensure!(
            Some(bones.len() as u64) == rig["native_bone_count"].as_u64(),
            "native bone count changed"
        );
        let mapping = rig["bone_map"].as_array().context("bone map")?;
        let source_bones = rig["source_bones"].as_array().context("source bones")?;
        ensure!(
            mapping.len() == source_bones.len(),
            "bone map length mismatch"
        );
        let required: BTreeSet<usize> = match rig.get("required_source_bones") {
            Some(value) => serde_json::from_value(value.clone())?,
            None => (0..mapping.len()).collect(),
        };
        ensure!(
            required.iter().all(|&index| index < mapping.len()),
            "required source bone outside source skeleton"
        );
        let mut allowed = BTreeSet::new();
        for (source_index, (target, source)) in mapping.iter().zip(source_bones).enumerate() {
            let index = target.as_u64().context("target bone index")? as usize;
            let Some(index) = mapped_bone_index(source_index, index, bones.len(), &required)?
            else {
                continue;
            };
            let row = bones[index];
            let name = u32::from_str_radix(
                source["name_hash"].as_str().context("source bone name")?,
                16,
            )?;
            ensure!(owner.u32(row)? == name, "mapped native bone name changed");
            allowed.insert(u16::try_from(index)?);
        }
        let positions = read(tag("positions-data")?)?;
        ensure!(positions.0.len() % 8 == 0, "position stream is truncated");
        for row in (0..positions.0.len()).step_by(8) {
            ensure!(
                u32::from(positions.u16(row + 6)?) < model.u32(0x40)?,
                "model palette does not cover vertex bone selector"
            );
            ensure!(
                allowed.contains(&positions.u16(row + 6)?),
                "vertex references an unmapped bone"
            );
        }
    }
    for original in nodes["nodes"]
        .as_array()
        .context("nodes")?
        .iter()
        .map(|n| n["template"].as_u64().unwrap() as u32)
    {
        ensure!(
            read(original)?.0 == base.read_tag(TagHash(original))?,
            "stock donor changed"
        )
    }
    Ok(
        json!({"passed":true,"loading":loading,"private_tags":symbols.as_object().unwrap().len(),"native_draw_parts":parts.len(),"private_art_index":item.u16(art[0]+2)?,"native_materials":"linked payloads and references verified","gameplay_verified":false,"runtime_map_rows":runtime_map.u64(8)?,"runtime_map_auxiliary_words":runtime_map.u64(24)?}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unmapped_sentinel_is_allowed_only_for_unused_source_bones() {
        let required = BTreeSet::from([0, 2]);
        assert_eq!(mapped_bone_index(1, 65535, 4, &required).unwrap(), None);
        assert!(mapped_bone_index(2, 65535, 4, &required).is_err());
        assert!(mapped_bone_index(1, 4, 4, &required).is_err());
        assert_eq!(mapped_bone_index(2, 3, 4, &required).unwrap(), Some(3));
    }

    #[test]
    fn entity_map_rejects_truncated_or_overlapping_runtime_array() {
        let mut p = Payload(vec![0; 100]);
        p.0[..8].copy_from_slice(&100u64.to_le_bytes());
        for (descriptor, header, class) in [(8usize, 48usize, 0x80809252u32), (24, 80, 0x8080000B)]
        {
            p.0[descriptor..descriptor + 8].copy_from_slice(&1u64.to_le_bytes());
            p.0[descriptor + 8..descriptor + 16]
                .copy_from_slice(&((header - descriptor - 8) as u64).to_le_bytes());
            p.0[header..header + 8].copy_from_slice(&1u64.to_le_bytes());
            p.0[header + 8..header + 12].copy_from_slice(&class.to_le_bytes());
        }
        entity_map_arrays(&p).unwrap();
        let valid = p.clone();
        p.0.truncate(72);
        p.0[..8].copy_from_slice(&72u64.to_le_bytes());
        assert!(entity_map_arrays(&p).is_err());
        p = valid;
        p.0[96] = 1;
        assert!(entity_map_arrays(&p).is_err());
    }
    #[test]
    fn private_assets_alone_do_not_cover_retained_native_components() {
        let parent = BTreeSet::from([0x81D4001E, 0x81D4001F, 0x80EC2728]);
        let mut root = BTreeSet::from([0x81D4001E, 0x81D4001F]);
        let error = require_loading_closure(&parent, &root).unwrap_err();
        assert!(error.to_string().contains("80EC2728"));
        root.insert(0x80EC2728);
        require_loading_closure(&parent, &root).unwrap();
    }
}
