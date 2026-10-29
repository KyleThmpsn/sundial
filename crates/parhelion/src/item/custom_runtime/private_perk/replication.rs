//! Private patches and whole-owner replacement retain native rows and publish a private loading pair.
use super::*;
use crate::item::custom_runtime::assets::{self, Node};
use sundial::package_authoring::sandbox_perk::program::NativeAssetComponent;

pub(super) struct Plan {
    template: TagHash,
    payload: Vec<u8>,
    owners: Vec<u32>,
}

fn read(manager: &PackageManager, tag: u32, class: u32, file_type: u8) -> AuthoringResult<Vec<u8>> {
    if manager.get_entry(TagHash(tag)).is_none_or(|entry| {
        entry.reference != class || entry.file_type != file_type || entry.file_subtype != 0
    }) {
        return Err(invalid(
            "Replication resource has an incompatible native class",
        ));
    }
    let bytes = read_tag(manager, TagHash(tag), "replication resource")?;
    if read_u64(&bytes, 0)? != bytes.len() as u64 {
        return Err(invalid("Replication resource has an invalid extent"));
    }
    Ok(bytes)
}

fn owners(graph: &[u8]) -> AuthoringResult<Vec<u32>> {
    let (count, _, start, class) = crate::tag_payload::array_at(graph, 16)?;
    if count == 0 || count > 64 || class != 0x80809C04 {
        return Err(invalid(
            "Replicated graph needs one to 64 native component owners",
        ));
    }
    (0..count)
        .map(|index| read_u32(graph, start + index * 12))
        .collect()
}

/// Native rows carry an opaque trailing word. Use an intact row that is owned by
/// its actual graph, rather than deriving that word from class or allocation IDs.
fn record(manager: &PackageManager, tag: u32) -> AuthoringResult<(Vec<u8>, Vec<u32>)> {
    let payload = read(manager, tag, 0x80809BB6, 16)?;
    let graph = read(manager, read_u32(&payload, 8)?, 0x80809C0F, 8)?;
    let owners = owners(&graph)?;
    let (count, _, start, class) = crate::tag_payload::array_at(&payload, 48)?;
    if read_u32(&graph, 0x88)? != tag
        || payload.get(12..48) != Some(&[0; 36][..])
        || count != owners.len()
        || class != 0x80809BB8
    {
        return Err(invalid("Replication record does not match its native host"));
    }
    for (index, &owner) in owners.iter().enumerate() {
        let data = read(manager, owner, 0x80809C36, 8)?;
        let live = crate::tag_payload::bounded_relative_target(&data, 16, "Replication instance")?;
        let row = start + index * 32;
        if live < 4
            || read_u32(&payload, row)? != read_u32(&data, live - 4)?
            || read_u32(&payload, row + 4)? != read_u32(&data, 0x44)?
            || payload.get(row + 8..row + 24) != Some(&[0; 16][..])
            || read_u32(&payload, row + 28)? != 0
        {
            return Err(invalid("Replication row differs from its native component"));
        }
        read(manager, read_u32(&data, 0x44)?, 0x80809BBB, 8)?;
    }
    Ok((payload, owners))
}

fn shape(owner: &[u8]) -> AuthoringResult<(u32, u32, usize, usize, usize, u64)> {
    let live = crate::tag_payload::bounded_relative_target(owner, 16, "Replication instance")?;
    let definition =
        crate::tag_payload::bounded_relative_target(owner, 24, "Replication definition")?;
    let prefix = crate::tag_payload::bounded_relative_target(owner, 8, "Replication constructor")?;
    if live < 4
        || definition < live
        || prefix > live
        || read_u64(owner, 0x48)? != (definition - live) as u64
        || read_u64(owner, 0x50)? != (live - prefix) as u64
    {
        return Err(invalid(
            "Replication component has an inconsistent native allocation extent",
        ));
    }
    Ok((
        read_u32(owner, live - 4)?,
        read_u32(owner, 0x44)?,
        live,
        definition,
        owner.len(),
        read_u64(owner, 0x50)?,
    ))
}

fn witness(manager: &PackageManager, tag: u32, owner: u32) -> AuthoringResult<[u8; 32]> {
    let expected = shape(&read(manager, owner, 0x80809C36, 8)?)?;
    let (payload, owners) = record(manager, tag)?;
    let (_, _, start, _) = crate::tag_payload::array_at(&payload, 48)?;
    let mut selected = None;
    for (index, original) in owners.into_iter().enumerate() {
        let row = start + index * 32;
        if (read_u32(&payload, row)?, read_u32(&payload, row + 4)?) != (expected.0, expected.1) {
            continue;
        }
        if shape(&read(manager, original, 0x80809C36, 8)?)? != expected {
            return Err(invalid(
                "Replication witness has another component allocation layout",
            ));
        }
        let bytes = crate::tag_payload::read_array::<32>(&payload, row)?;
        if selected.is_some_and(|old| old != bytes) {
            return Err(invalid("Replication witness has ambiguous serialized rows"));
        }
        selected = Some(bytes);
    }
    selected.ok_or_else(|| invalid("Replication witness has no matching component allocation"))
}

impl Plan {
    pub(super) fn prepare(
        manager: &PackageManager,
        source: TagHash,
        graph: &[u8],
        removals: &BTreeSet<u32>,
        components: &[NativeAssetComponent],
    ) -> AuthoringResult<Option<Self>> {
        let tag = read_u32(graph, 0x88)?;
        // Removal-only edits keep their existing nonreplacement route. An intact
        // replicated graph needs a private record even when only scalar fields or
        // separately compiled action references change in one of its owners.
        if tag == u32::MAX || (components.is_empty() && !removals.is_empty()) {
            if components
                .iter()
                .any(|component| component.replication.is_some())
            {
                return Err(invalid("Replication witnesses require a replicated host"));
            }
            return Ok(None);
        }
        let (mut payload, original) = record(manager, tag)?;
        if read_u32(&payload, 8)? != source.0 || original != owners(graph)? {
            return Err(invalid("Private replication names another native host"));
        }
        let retained = original
            .len()
            .checked_sub(components.len())
            .ok_or_else(|| invalid("Replication replacement has too many components"))?;
        if removals.len() != components.len()
            || original[retained..]
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                != *removals
        {
            return Err(invalid(
                "Replication replacement must preserve the retained owner ordinals",
            ));
        }
        let (_, _, start, _) = crate::tag_payload::array_at(&payload, 48)?;
        let mut owners = original[..retained].to_vec();
        for (index, component) in components.iter().enumerate() {
            let graph = read(manager, component.source_graph, 0x80809C0F, 8)?;
            let added = self::owners(&graph)?;
            let [owner] = added.as_slice() else {
                return Err(invalid(
                    "Replication replacement needs a complete single-owner graph",
                ));
            };
            let row = witness(
                manager,
                component
                    .replication
                    .ok_or_else(|| invalid("Replication replacement has no native row witness"))?,
                *owner,
            )?;
            let offset = start + (retained + index) * 32;
            payload[offset..offset + 32].copy_from_slice(&row);
            owners.push(*owner);
        }
        if owners.iter().copied().collect::<BTreeSet<_>>().len() != owners.len() {
            return Err(invalid("Replication replacement repeats an owner"));
        }
        Ok(Some(Self {
            template: TagHash(tag),
            payload,
            owners,
        }))
    }

    /// Clone untouched retained owners too, then reuse the prepared-asset validators
    /// and loading writer with the final private graph and owner identities.
    pub(super) fn finish(
        self,
        manager: &PackageManager,
        source: TagHash,
        mut graph: Vec<u8>,
        resources: &[TagHash],
        allocator: AppendedTagAllocator,
        tags: &mut Vec<NewTagSpec>,
    ) -> AuthoringResult<TagHash> {
        let current = owners(&graph)?;
        if current.len() != self.owners.len() {
            return Err(invalid("Composed replication owner count changed"));
        }
        let mut nodes = Vec::new();
        let mut symbols = BTreeMap::new();
        for (index, (&tag, &template)) in current.iter().zip(&self.owners).enumerate() {
            let (private, payload) = if tag == template {
                let private =
                    allocator.assigned_tag(tags.len(), "Private replicated owner", "component")?;
                let mut payload = read(manager, template, 0x80809C36, 8)?;
                retarget_weapon_component_owner_payload(&mut payload, &graph, template, private.0)
                    .map_err(invalid)?;
                retarget_weapon_component_owner(&mut graph, template, private.0)
                    .map_err(invalid)?;
                tags.push(NewTagSpec {
                    template_tag: TagHash(template),
                    payload: payload.clone(),
                    storage: crate::NewTagStorageMode::InheritTemplate,
                });
                (private, payload)
            } else {
                let ordinal = allocator.ordinal(TagHash(tag))?;
                let node = tags
                    .get(ordinal)
                    .filter(|node| {
                        node.template_tag.0 == template
                            && node.storage == crate::NewTagStorageMode::InheritTemplate
                    })
                    .ok_or_else(|| {
                        invalid("Replicated owner is outside this private allocation")
                    })?;
                (TagHash(tag), node.payload.clone())
            };
            // A preserved native row only describes the original allocation.
            // Nested programs may replace a declared reference without growing
            // this owner, but appends or altered allocation extents need a new
            // independently qualified layout and cannot retain the stock row.
            if shape(&payload)? != shape(&read(manager, template, 0x80809C36, 8)?)? {
                return Err(invalid(
                    "Private replicated component changed its native allocation layout",
                ));
            }
            let symbol = format!("owner-{index}");
            symbols.insert(symbol.clone(), private);
            nodes.push(Node {
                symbol,
                template,
                payload,
                reference: None,
                storage: crate::NewTagStorageMode::InheritTemplate,
                patches: Vec::new(),
                metadata: (),
            });
        }
        let start = tags.len();
        allocator.reserve(start, 3)?;
        let root = allocator.assigned_tag(start, "Private replicated graph", "graph")?;
        let replication =
            allocator.assigned_tag(start + 1, "Private replication", "replication")?;
        let companion =
            allocator.assigned_tag(start + 2, "Private replication loading", "companion")?;
        symbols.insert("root".into(), root);
        symbols.insert("replication".into(), replication);
        let mut symbolic = graph.clone();
        let (_, _, rows, _) = crate::tag_payload::array_at(&symbolic, 16)?;
        let mut patches = vec![(0x88, "replication".into())];
        write_u32(&mut symbolic, 0x88, u32::MAX)?;
        for index in 0..self.owners.len() {
            write_u32(&mut symbolic, rows + index * 12, u32::MAX)?;
            patches.push((rows + index * 12, format!("owner-{index}")));
        }
        nodes.push(Node {
            symbol: "root".into(),
            template: source.0,
            payload: symbolic,
            reference: None,
            storage: crate::NewTagStorageMode::InheritTemplate,
            patches,
            metadata: (),
        });
        let mut replica = self.payload;
        write_u32(&mut replica, 8, u32::MAX)?;
        nodes.push(Node {
            symbol: "replication".into(),
            template: self.template.0,
            payload: replica,
            reference: None,
            storage: crate::NewTagStorageMode::InheritTemplate,
            patches: vec![(8, "root".into())],
            metadata: (),
        });
        let mut required = assets::replication::requirements(&nodes, manager)?;
        required.extend_from_slice(resources);
        required.extend(symbols.values().copied());
        required.push(companion);
        let root_node = nodes
            .iter()
            .find(|node| node.symbol == "root")
            .ok_or_else(|| invalid("Private replicated graph is missing"))?;
        let loading = assets::replication::Loading::read(&nodes, root_node, manager, &required)?;
        let loading = loading.materialize(&nodes, &symbols, companion)?;
        let mut replica = nodes
            .last()
            .ok_or_else(|| invalid("Private replication is missing"))?
            .payload
            .clone();
        write_u32(&mut replica, 8, root.0)?;
        write_u32(&mut graph, 0x88, replication.0)?;
        tags.extend([
            NewTagSpec {
                template_tag: source,
                payload: graph,
                storage: crate::NewTagStorageMode::InheritTemplate,
            },
            NewTagSpec {
                template_tag: self.template,
                payload: replica,
                storage: crate::NewTagStorageMode::InheritTemplate,
            },
            loading,
        ]);
        Ok(root)
    }
}
