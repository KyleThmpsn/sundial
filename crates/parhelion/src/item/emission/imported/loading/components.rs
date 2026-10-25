//! Grafted components load their native dispatch and layout resources with their
//! private parent. Their geometry donor's dependency index need not contain them.
use super::*;
use parhelion_import::d2_mot::payload::Payload;
use sundial::package_authoring::sandbox_perk::entity::residency::owner_requirements;

fn interface(
    manager: &sundial::package_authoring::PackageManager,
    nodes: &[Value],
    symbols: &BTreeMap<String, TagHash>,
    node: &Value,
    payload: &Payload,
    offset: usize,
) -> AuthoringResult<TagHash> {
    let patches = node["patches"]
        .as_array()
        .ok_or_else(|| invalid("Imported component fixups are missing"))?;
    let matching = patches
        .iter()
        .filter(|patch| patch["offset"].as_u64() == Some(offset as u64))
        .collect::<Vec<_>>();
    let (helper, template) = match matching.as_slice() {
        [] => {
            let tag = TagHash(
                payload
                    .u32(offset)
                    .map_err(|error| invalid(error.to_string()))?,
            );
            (tag, tag)
        }
        [patch] => {
            let symbol = patch["symbol"]
                .as_str()
                .ok_or_else(|| invalid("Imported interface symbol is missing"))?;
            let targets = nodes
                .iter()
                .filter(|target| target["symbol"] == symbol)
                .collect::<Vec<_>>();
            let [target] = targets.as_slice() else {
                return Err(invalid("Imported interface target is absent or ambiguous"));
            };
            if !target["reference"].is_null() {
                return Err(invalid(
                    "Imported interface must preserve its native metadata class",
                ));
            }
            let template = target["template"]
                .as_u64()
                .and_then(|tag| u32::try_from(tag).ok())
                .ok_or_else(|| invalid("Imported interface template is missing"))?;
            let helper = *symbols
                .get(symbol)
                .ok_or_else(|| invalid("Imported interface was not allocated"))?;
            (helper, TagHash(template))
        }
        _ => return Err(invalid("Imported interface has overlapping fixups")),
    };
    if manager
        .get_entry(template)
        .is_none_or(|entry| entry.file_type != 8 || entry.reference != 0x80809C54)
    {
        return Err(invalid(format!(
            "Imported interface requires unavailable native metadata {template}"
        )));
    }
    Ok(helper)
}

pub(super) fn include(
    manager: &sundial::package_authoring::PackageManager,
    folder: &Path,
    nodes: &[Value],
    symbols: &BTreeMap<String, TagHash>,
    groups: &mut BTreeMap<TagHash, Vec<TagHash>>,
) -> AuthoringResult<()> {
    let mut dependencies = BTreeMap::new();
    for node in nodes {
        let template = node["template"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| invalid("Imported component template is missing"))?;
        let entry = manager
            .get_entry(TagHash(template))
            .ok_or_else(|| invalid("Imported component template is unavailable"))?;
        if entry.file_type != 8 || !matches!(entry.reference, 0x80809C0F | 0x80809C36) {
            continue;
        }
        let name = node["symbol"]
            .as_str()
            .ok_or_else(|| invalid("Imported component symbol is missing"))?;
        let tag = *symbols
            .get(name)
            .ok_or_else(|| invalid("Imported component was not allocated"))?;
        let path = node["file"]
            .as_str()
            .ok_or_else(|| invalid("Imported component payload is missing"))?;
        let payload = fs::read(folder.join(path)).map_err(|error| invalid(error.to_string()))?;
        let mut required = BTreeSet::new();
        if entry.reference == 0x80809C36 {
            required.extend(
                owner_requirements(manager, tag.0, &payload)
                    .map_err(invalid)?
                    .into_iter()
                    .map(|requirement| TagHash(requirement.tag)),
            );
        } else {
            let payload = Payload(payload);
            for row in payload
                .array(0x58, 40, Some(0x80809C22))
                .map_err(|error| invalid(format!("Imported entity {name}: {error:#}")))?
            {
                required.insert(interface(
                    manager,
                    nodes,
                    symbols,
                    node,
                    &payload,
                    row + 32,
                )?);
            }
        }
        dependencies.insert(tag, required);
    }
    let mut enrolled = BTreeSet::new();
    for group in groups.values_mut() {
        let mut complete = group.iter().copied().collect::<BTreeSet<_>>();
        for tag in group.iter() {
            if let Some(required) = dependencies.get(tag) {
                complete.extend(required);
                enrolled.insert(*tag);
            }
        }
        *group = complete.into_iter().collect();
    }
    if dependencies.keys().any(|tag| !enrolled.contains(tag)) {
        return Err(invalid("Imported component has no private loading owner"));
    }
    Ok(())
}
