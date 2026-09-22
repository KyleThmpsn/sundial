//! Keep imported models behind native owner loading boundaries.
use super::*;
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
};

fn companion(manager: &PackageManager, owner: u32) -> Result<(u32, usize)> {
    let path = &manager
        .package_paths
        .get(&TagHash(owner).pkg_id())
        .context("owner package")?
        .path;
    let mut file = fs::File::open(path)?;
    let mut header = vec![0; 0x118];
    file.read_exact(&mut header)?;
    let header = Payload(header);
    let size = header.u32(0x114)? as usize;
    ensure!(
        (64..=32 * 1024 * 1024).contains(&size),
        "invalid package table size"
    );
    file.seek(SeekFrom::Start(u64::from(header.u32(0x110)?)))?;
    let mut tables = vec![0; size];
    file.read_exact(&mut tables)?;
    let tables = Payload(tables);
    let rows = tables.array(0x30, 8, Some(0x80809A13))?;
    let found = rows
        .into_iter()
        .filter(|&row| tables.u32(row).ok() == Some(owner))
        .collect::<Vec<_>>();
    // Stock manifests can repeat the same enrollment for a shared dye owner.
    // Preserve that native representation, but reject conflicting companions.
    let targets = found
        .iter()
        .map(|row| tables.u32(row + 4))
        .collect::<Result<BTreeSet<_>>>()?;
    let tag = unique_companion(&targets, owner)?;
    let entry = manager.get_entry(TagHash(tag)).context("companion entry")?;
    ensure!(
        entry.file_type == 8 && entry.reference == 0x80809EF9,
        "invalid companion class"
    );
    Ok((tag, found.len()))
}

fn unique_companion(targets: &BTreeSet<u32>, owner: u32) -> Result<u32> {
    ensure!(
        targets.len() == 1,
        "owner {owner:08X} has no unique package enrollment"
    );
    Ok(*targets.first().context("companion target")?)
}

fn boundary(
    private: &BTreeSet<u32>,
    global: &BTreeSet<u32>,
    local: &BTreeSet<u32>,
    expected: &BTreeSet<u32>,
) -> Result<()> {
    ensure!(
        private.is_disjoint(global),
        "imported model resources are eagerly enrolled in the global runtime root"
    );
    ensure!(
        local == expected,
        "weapon-local loading closure differs from its private resources and retained native dependencies"
    );
    Ok(())
}

fn material_references(p: &Payload) -> Result<BTreeSet<u32>> {
    let mut refs = BTreeSet::new();
    for stage in [0x48, 0xe8, 0x188, 0x228, 0x2c8, 0x368] {
        refs.insert(p.u32(stage)?);
        refs.insert(p.u32(stage + 0x84)?);
        for row in p.array(stage + 8, 8, None)? {
            refs.insert(p.u32(row + 4)?);
        }
        for row in p.array(stage + 0x40, 16, None)? {
            refs.insert(p.u32(row)?);
        }
    }
    for empty in [0, u32::MAX, 0x811c9dc5] {
        refs.remove(&empty);
    }
    Ok(refs)
}

fn require_material_resources(
    material: u32,
    refs: &BTreeSet<u32>,
    available: &BTreeSet<u32>,
) -> Result<()> {
    ensure!(
        refs.is_subset(available),
        "material {material:08X} references resources outside its loading boundary: {:?}",
        refs.difference(available)
            .map(|t| format!("{t:08X}"))
            .collect::<Vec<_>>()
    );
    Ok(())
}

fn include_backing_resources(
    base: &PackageManager,
    staged: &PackageManager,
    required: &mut BTreeSet<u32>,
    read: &impl Fn(u32) -> Result<Payload>,
) -> Result<()> {
    for dependency in required.clone() {
        let entry = staged
            .get_entry(TagHash(dependency))
            .or_else(|| base.get_entry(TagHash(dependency)))
            .context("material resource missing")?;
        if staged
            .get_entry(TagHash(entry.reference))
            .or_else(|| base.get_entry(TagHash(entry.reference)))
            .is_some()
        {
            required.insert(entry.reference);
        }
        if entry.file_type == 32 && entry.file_subtype == 2 {
            let large = read(dependency)?.u32(36)?;
            if ![0, u32::MAX, 0x811C9DC5].contains(&large) {
                ensure!(
                    staged
                        .get_entry(TagHash(large))
                        .or_else(|| base.get_entry(TagHash(large)))
                        .is_some(),
                    "streamed texture payload {large:08X} is missing"
                );
                required.insert(large);
            }
        }
    }
    Ok(())
}

pub(super) fn audit(
    base: &PackageManager,
    staged: &PackageManager,
    graph: &Value,
    symbols: &Value,
    read: &impl Fn(u32) -> Result<Payload>,
    baseline_root: &BTreeSet<u32>,
) -> Result<Value> {
    let root = dependencies(&read(0x80EE8CBD)?)?;
    let stock_root = dependencies(&Payload(base.read_tag(TagHash(0x80EE8CBD))?))?;
    ensure!(
        stock_root.is_subset(&root),
        "stock runtime root dependencies were removed"
    );
    for &added in root.difference(&stock_root) {
        ensure!(
            base.get_entry(TagHash(added)).is_none() || baseline_root.contains(&added),
            "global root gained native model dependency {added:08X}"
        );
    }
    let tag = |name: &str| -> Result<u32> {
        Ok(u32::try_from(
            symbols[name].as_u64().context("loading symbol")?,
        )?)
    };
    let private = symbols
        .as_object()
        .context("symbols")?
        .values()
        .map(|value| {
            Ok(u32::try_from(
                value.as_u64().context("private loading tag")?,
            )?)
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let mut native = BTreeSet::new();
    let mut owners = BTreeMap::new();
    let mut boundaries = BTreeSet::new();
    let mut references = BTreeMap::new();
    let mut materials = BTreeMap::new();
    for node in graph["nodes"].as_array().context("nodes")? {
        let name = node["symbol"].as_str().context("resource symbol")?;
        let mut edges = node["patches"]
            .as_array()
            .context("resource patches")?
            .iter()
            .map(|p| tag(p["symbol"].as_str().context("reference symbol")?))
            .collect::<Result<BTreeSet<_>>>()?;
        if let Some(target) = node["reference"].as_str() {
            edges.insert(tag(target)?);
        }
        references.insert(tag(name)?, edges);
        let template = TagHash(node["template"].as_u64().context("node template")? as u32);
        if base
            .get_entry(template)
            .is_some_and(|e| e.reference == 0x808071e8)
        {
            let mut required = material_references(&read(tag(name)?)?)?;
            include_backing_resources(base, staged, &mut required, read)?;
            materials.insert(tag(name)?, required);
        }
        if let Some(owner_name) = node["shared_owner"]
            .as_str()
            .or_else(|| (node["symbol"] == "parent-companion").then_some("parent"))
        {
            let source_owner = node["source_parent"].as_u64().unwrap_or(0x80EC272A) as u32;
            let (source_companion, _) = companion(base, source_owner)?;
            let payload = Payload(base.read_tag(TagHash(source_companion))?);
            ensure!(
                payload.u32(8)? == source_companion && payload.u32(12)? == source_owner,
                "native loading identity mismatch"
            );
            let inherited = dependencies(&payload)?;
            native.extend(inherited.iter().copied());
            let owner = tag(owner_name)?;
            let selected = tag(node["symbol"].as_str().context("companion symbol")?)?;
            let (enrolled, row_count) = companion(staged, owner)?;
            ensure!(
                enrolled == selected && row_count == 1,
                "private package enrollment mismatch"
            );
            boundaries.extend([owner, selected]);
            ensure!(
                owners.insert(owner, (selected, inherited)).is_none(),
                "duplicate owner"
            );
        }
    }
    ensure!(!owners.is_empty(), "no model loading owners");
    let mut covered = BTreeSet::new();
    let mut closures = Vec::new();
    for (&owner, (companion, inherited)) in &owners {
        let companion = *companion;
        let payload = read(companion)?;
        ensure!(
            payload.u32(8)? == companion && payload.u32(12)? == owner,
            "private loading identity mismatch"
        );
        let local = dependencies(&payload)?;
        require_loading_closure(inherited, &local)?;
        independent_owners(&local, owner, companion, &boundaries)?;
        let owned = local
            .intersection(&private)
            .copied()
            .collect::<BTreeSet<_>>();
        let mut expected: BTreeSet<_> = inherited.union(&owned).copied().collect();
        for (&material, required) in &materials {
            if owned.contains(&material) {
                expected.extend(
                    required
                        .iter()
                        .filter(|&&tag| base.get_entry(TagHash(tag)).is_some())
                        .copied(),
                );
            }
        }
        boundary(&private, &root, &local, &expected)?;
        let available = local.union(&root).copied().collect();
        for (&material, required) in &materials {
            if owned.contains(&material) {
                require_material_resources(material, required, &available)?;
            }
        }
        for dependency in &owned {
            ensure!(
                references
                    .get(dependency)
                    .context("local resource missing from graph")?
                    .is_subset(&local),
                "loading owner {owner:08X} omits a referenced private resource"
            );
        }
        covered.extend(owned.iter().copied());
        closures.push(json!({"owner":format!("{owner:08X}"),"private_resources":owned.len(),"native_dependencies":inherited.len()}));
    }
    ensure!(
        covered == private,
        "private resources have no loading owner"
    );
    Ok(
        json!({"private_resources":private.len(),"owner_count":owners.len(),"retained_native_dependencies":native.len(),"global_private_resources":0,"global_added_dependencies":root.len()-stock_root.len(),"cross_owner_dependencies":0,"closures":closures}),
    )
}

fn independent_owners(
    local: &BTreeSet<u32>,
    owner: u32,
    companion: u32,
    boundaries: &BTreeSet<u32>,
) -> Result<()> {
    ensure!(
        local.contains(&owner) && local.contains(&companion),
        "owner loading identity is not enrolled"
    );
    for &dependency in local.intersection(boundaries) {
        ensure!(
            dependency == owner || dependency == companion,
            "loading owner {owner:08X} includes another owner or companion {dependency:08X}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn material_loading_requires_borrowed_shader_headers_and_bytecode() {
        let material = 0x81d4005d;
        let required = BTreeSet::from([0x80efad5c, 0x815b9299, 0x815b9298]);
        assert!(
            require_material_resources(material, &required, &BTreeSet::from([material])).is_err()
        );
        assert!(
            require_material_resources(
                material,
                &required,
                &BTreeSet::from([material, 0x80efad5c, 0x815b9299])
            )
            .is_err()
        );
        require_material_resources(material, &required, &required).unwrap();
    }
    #[test]
    fn rejects_mutually_enrolled_owners_but_allows_shared_leaf_resources() {
        let boundaries = BTreeSet::from([10, 11, 20, 21]);
        independent_owners(&BTreeSet::from([10, 11, 30]), 10, 11, &boundaries).unwrap();
        independent_owners(&BTreeSet::from([20, 21, 30]), 20, 21, &boundaries).unwrap();
        assert!(
            independent_owners(&BTreeSet::from([10, 11, 20, 21, 30]), 10, 11, &boundaries).is_err()
        );
        assert!(independent_owners(&BTreeSet::from([10, 30]), 10, 11, &boundaries).is_err());
    }
    #[test]
    fn stock_enrollment_requires_one_distinct_companion() {
        assert_eq!(
            unique_companion(&[20, 20].into_iter().collect(), 10).unwrap(),
            20
        );
        assert!(unique_companion(&BTreeSet::new(), 10).is_err());
        assert!(unique_companion(&BTreeSet::from([20, 21]), 10).is_err());
    }
    #[test]
    fn rejects_global_preloading_and_missing_or_cross_weapon_dependencies() {
        let private = BTreeSet::from([10, 11]);
        let global = BTreeSet::from([1, 2]);
        let expected = BTreeSet::from([10, 11, 20]);
        boundary(&private, &global, &expected, &expected).unwrap();
        assert!(boundary(&private, &BTreeSet::from([1, 10]), &expected, &expected).is_err());
        assert!(boundary(&private, &global, &private, &expected).is_err());
        assert!(
            boundary(
                &private,
                &global,
                &BTreeSet::from([10, 11, 20, 99]),
                &expected
            )
            .is_err()
        );
    }
}
