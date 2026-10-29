//! Follow persisted symbolic cue obligations into the actual staged resource closure.
use super::*;

fn file(root: &Path, node: &Value) -> CheckResult<PathBuf> {
    let relative = Path::new(node["file"].as_str().ok_or("Prepared audio file")?);
    assert!(
        !relative.is_absolute(),
        "Prepared audio uses an absolute path"
    );
    let path = root.join(relative).canonicalize()?;
    assert!(
        path.starts_with(root),
        "Prepared audio leaves its graph directory"
    );
    Ok(path)
}

pub(super) fn check(
    source: &Source,
    emitted: &mut Reader,
    recipe: &WeaponRecipe,
    record: &Value,
) -> CheckResult<Value> {
    let reference = recipe
        .overrides
        .imported_graph
        .as_ref()
        .ok_or("Reloaded imported graph")?;
    let root = reference.directory.canonicalize()?;
    let graph_bytes = fs::read(root.join("asset-graph.json"))?;
    let graph: Value = serde_json::from_slice(&graph_bytes)?;
    assert_eq!(graph["audio_assets"]["installable"], true);
    let mut nodes = BTreeMap::new();
    for node in graph["audio_assets"]["nodes"]
        .as_array()
        .ok_or("Persisted audio nodes")?
    {
        let symbol = node["symbol"].as_str().ok_or("Audio symbol")?;
        assert!(
            nodes.insert(symbol.to_owned(), node).is_none(),
            "Duplicate persisted audio symbol"
        );
        assert!(!fs::read(file(&root, node)?)?.is_empty());
    }
    let source_cue = number(&record["source_cue"])?;
    let cue_tag = number(&record["cue"])?;
    let matches = nodes
        .iter()
        .filter(|(_, node)| node["source_cue_tag"].as_u64() == Some(u64::from(source_cue)))
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "The persisted group lost or duplicated its source cue"
    );
    let mut pending = vec![(matches[0].0.clone(), cue_tag)];
    let mut visited = BTreeMap::new();
    let mut files = Vec::new();
    let mut complete_objects = BTreeMap::new();
    while let Some((symbol, tag)) = pending.pop() {
        if let Some(before) = visited.insert(symbol.clone(), tag) {
            assert_eq!(
                before, tag,
                "One prepared resource resolved to different tags"
            );
            continue;
        }
        let node = *nodes.get(&symbol).ok_or("Unpublished audio symbol")?;
        let before = Payload(fs::read(file(&root, node)?)?);
        let actual = emitted.tag(tag, None)?;
        assert_eq!(
            before.0.len(),
            actual.0.len(),
            "Staging changed audio extent"
        );
        let mut expected = before.0.clone();
        let mut occupied = BTreeSet::new();
        for patch in node["patches"]
            .as_array()
            .ok_or("Persisted audio relocations")?
        {
            let offset =
                usize::try_from(patch["offset"].as_u64().ok_or("Audio relocation offset")?)?;
            assert!(occupied.insert(offset));
            assert_eq!(before.u32(offset)?, u32::MAX);
            let target = actual.u32(offset)?;
            assert!(!matches!(target, 0 | u32::MAX));
            let symbol = patch["symbol"].as_str().ok_or("Audio relocation symbol")?;
            assert!(nodes.contains_key(symbol));
            expected[offset..offset + 4].copy_from_slice(&target.to_le_bytes());
            pending.push((symbol.to_owned(), target));
        }
        if let Some(metadata) = node.get("audio_bank") {
            assert_eq!(node["storage"], "audio_bank");
            assert_eq!(
                serde_json::from_value::<Vec<u8>>(metadata["bytes"].clone())?,
                before.0
            );
            let original_tag = number(&node["source_bank_tag"])?;
            let original = source.read(original_tag)?;
            let from = bank::hierarchy(&original, 89)?;
            let prepared = bank::hierarchy(&before, 113)?;
            let staged = bank::hierarchy(&actual, 113)?;
            assert_eq!(from.len(), prepared.len());
            assert_eq!(from.len(), staged.len());
            assert_eq!(
                from.keys().collect::<BTreeSet<_>>(),
                prepared.keys().collect()
            );
            let declared: BTreeSet<u32> = serde_json::from_value(metadata["objects"].clone())?;
            assert_eq!(declared, from.keys().copied().collect());
            let mut identities = BTreeMap::new();
            let mut media = BTreeMap::new();
            for field in metadata["refs"].as_array().ok_or("Typed bank fields")? {
                let offset = usize::try_from(field["offset"].as_u64().ok_or("Bank field offset")?)?;
                assert!(occupied.insert(offset));
                let value = number(&field["value"])?;
                assert_eq!(before.u32(offset)?, value);
                let actual = actual.u32(offset)?;
                expected[offset..offset + 4].copy_from_slice(&actual.to_le_bytes());
                match field["kind"].as_str().ok_or("Typed bank field kind")? {
                    "Object" if declared.contains(&value) => {
                        assert_ne!(actual, value, "Local Wwise object was not private");
                        if let Some(previous) = identities.insert(value, actual) {
                            assert_eq!(
                                previous, actual,
                                "Wwise reference split one source identity"
                            );
                        }
                    }
                    "Object" => assert_eq!(actual, value, "Global Wwise object was rebound"),
                    "Bank" => assert_eq!(actual, emitted.tag(tag, None)?.u32(12)?),
                    "Media" => {
                        if let Some(previous) = media.insert(value, actual) {
                            assert_eq!(previous, actual);
                        }
                    }
                    "MediaSize" => {}
                    _ => return Err("Unknown persisted audio reference kind".into()),
                }
            }
            assert_eq!(
                identities.len(),
                from.len(),
                "A source HIRC definition has no private identity"
            );
            assert_eq!(
                identities.values().copied().collect::<BTreeSet<_>>().len(),
                from.len()
            );
            for (old, &(kind, _)) in &from {
                assert_eq!(prepared.get(old).map(|row| row.0), Some(kind));
                let new = *identities.get(old).ok_or("Absent object identity")?;
                assert_eq!(staged.get(&new).map(|row| row.0), Some(kind));
            }
            let mut declared_media = BTreeSet::new();
            for medium in node["audio_media"]
                .as_array()
                .ok_or("Persisted bank media")?
            {
                let old = number(&medium["id"])?;
                assert!(declared_media.insert(old));
                let tag = *media.get(&old).ok_or("Unreferenced prepared medium")?;
                let symbol = medium["symbol"].as_str().ok_or("Medium symbol")?;
                assert_eq!(
                    nodes.get(symbol).ok_or("Missing medium node")?["storage"],
                    "audio_media"
                );
                pending.push((symbol.to_owned(), tag));
            }
            assert_eq!(declared_media, media.keys().copied().collect());
            complete_objects.insert(tag, identities);
        }
        for field in node["audio_events"].as_array().into_iter().flatten() {
            let offset = usize::try_from(field["offset"].as_u64().ok_or("Cue event offset")?)?;
            assert!(occupied.insert(offset));
            assert_eq!(before.u32(offset)?, number(&field["id"])?);
            let event = actual.u32(offset)?;
            assert_ne!(event, before.u32(offset)?);
            expected[offset..offset + 4].copy_from_slice(&event.to_le_bytes());
        }
        assert_eq!(
            expected, actual.0,
            "Staging changed an undeclared audio field"
        );
        files.push(
            json!({"symbol":symbol,"tag":tag,"file":node["file"],"storage":node["storage"],
            "prepared_sha256":hex::encode(Sha256::digest(&before.0)),
            "staged_sha256":hex::encode(Sha256::digest(&actual.0))}),
        );
    }
    let wanted = record["required_resources"]
        .as_array()
        .ok_or("Reached resources")?
        .iter()
        .map(number)
        .collect::<Result<BTreeSet<_>>>()?;
    for tag in visited.values() {
        assert!(
            wanted.contains(tag),
            "Persisted cue closure escaped the validated loading resources"
        );
    }
    let bank = number(&record["audio"]["bank"])?;
    let identities = complete_objects
        .get(&bank)
        .ok_or("Cue bank was not persisted")?;
    for (old, actual) in record["audio"]["reachable_object_identities"]
        .as_object()
        .ok_or("Reached object identities")?
    {
        let old = old.parse::<u32>()?;
        assert_eq!(
            identities.get(&old),
            Some(&number(actual)?),
            "Cue resolved an event from another bank"
        );
    }
    Ok(
        json!({"graph_sha256":hex::encode(Sha256::digest(graph_bytes)),"nodes":files,
        "complete_object_identities":complete_objects,"portable_recipe_sha256":hex::encode(Sha256::digest(recipe.to_json_pretty()?.as_bytes()))}),
    )
}
