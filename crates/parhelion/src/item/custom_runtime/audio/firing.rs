//! Private presentation closure for source firing sounds.
use super::*;

/// Firing events keyed by owner, parent, root and direct-entity layout.
type PresentationGroups<'a> = BTreeMap<(u32, u32, u32, bool), Vec<&'a Value>>;

/// The firing events grouped by the presentation chain they patch.
fn presentation_groups(audio: &ImportedAudio) -> AuthoringResult<PresentationGroups<'_>> {
    let mut groups = PresentationGroups::new();
    for event in &audio.firing {
        let presentation = &event["native_presentation"];
        groups
            .entry((
                hex(&presentation["owner"], "presentation owner")?,
                hex(&presentation["parent"], "presentation parent")?,
                hex(&presentation["root"], "presentation root")?,
                presentation["direct_entity"].as_bool().unwrap_or(false),
            ))
            .or_default()
            .push(event);
    }
    Ok(groups)
}

/// The private clone of `pattern_owner` an earlier edit appended and the entity binds, if any.
fn live_owner(
    tags: &[NewTagSpec],
    allocator: AppendedTagAllocator,
    pattern_owner: u32,
    bound: &BTreeSet<u32>,
) -> AuthoringResult<Option<usize>> {
    let mut existing = None;
    for (ordinal, entry) in tags.iter().enumerate() {
        if entry.template_tag.0 == pattern_owner
            && bound.contains(
                &allocator
                    .assigned_tag(ordinal, "Imported audio", "live presentation owner")?
                    .0,
            )
            && existing.replace(ordinal).is_some()
        {
            return Err(invalid(
                "Presentation component has multiple live private owners",
            ));
        }
    }
    Ok(existing)
}

fn root(
    manager: &PackageManager,
    parent: u32,
    entity: u32,
    direct: bool,
) -> AuthoringResult<Vec<u8>> {
    if direct && parent != entity {
        return Err(invalid(
            "Direct firing presentation has different entity links",
        ));
    }
    stock(
        manager,
        entity,
        if direct { 0x80809C0F } else { 0x80803ADC },
        "presentation root",
    )
}

fn validate_slot(
    root: &[u8],
    direct: bool,
    slot: usize,
    entity: u32,
    root_tag: u32,
) -> AuthoringResult<()> {
    let matches = if direct {
        slot == 0 && entity == root_tag
    } else {
        word(root, 0x18 + slot * 0x10)? == entity
    };
    if !matches {
        return Err(invalid("Firing presentation slot changed"));
    }
    Ok(())
}

fn patch_parent(
    owner: &mut [u8],
    events: &[&Value],
    parent: u32,
    private: u32,
) -> AuthoringResult<()> {
    let mut patched = BTreeSet::new();
    for event in events {
        let at = event["native_presentation"]["parent_offset"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| invalid("Presentation parent offset missing"))?;
        if patched.insert(at) {
            if word(owner, at)? != parent {
                return Err(invalid("Presentation parent link changed"));
            }
            put(owner, at, private)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn author(
    manager: &PackageManager,
    audio: &ImportedAudio,
    pattern: &mut [u8],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
    assets: AppendedTagAllocator,
    asset_tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<usize> {
    let mut count = 0;
    for ((pattern_owner, parent_tag, root_tag, direct), events) in presentation_groups(audio)? {
        let start = tags.len();
        let asset_start = asset_tags.len();
        let mut root = root(manager, parent_tag, root_tag, direct)?;
        let mut remap = BTreeMap::new();
        let mut owners = BTreeMap::<(u32, u32), Vec<u8>>::new();
        let mut links = BTreeSet::new();
        for event in &events {
            let source = &event["sounds"][0];
            let native = &event["native_sounds"][0];
            let entity_tag = hex(&native["entity"], "firing entity")?;
            let owner_tag = hex(&native["component_owner"], "firing component")?;
            let offset = native["offset"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| invalid("Firing sound offset missing"))?;
            let slot = native["root_slot"]
                .as_u64()
                .filter(|n| *n < 6)
                .ok_or_else(|| invalid("Firing presentation slot missing"))?
                as usize;
            validate_slot(&root, direct, slot, entity_tag, root_tag)?;
            links.insert((slot, entity_tag));
            let owner = match owners.entry((entity_tag, owner_tag)) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(stock(manager, owner_tag, OWNER_CLASS, "firing component")?)
                }
            };
            let native_sound = hex(&native["tag"], "firing sound")?;
            if word(owner, offset)? != native_sound {
                return Err(invalid("Firing component sound reference changed"));
            }
            let name = format!("ph_{:08x}_fire_{owner_tag:08x}_{offset:x}", audio.item);
            let private = private_sound(
                manager, audio, source, native, &name, assets, asset_tags, &mut remap,
            )?;
            put(owner, offset, private)?;
            count += 1;
        }
        let mut entities = BTreeMap::<u32, Vec<u8>>::new();
        for ((entity_tag, owner_tag), mut owner) in owners {
            let entity = match entities.entry(entity_tag) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(stock(manager, entity_tag, 0x80809C0F, "firing entity")?)
                }
            };
            let private = allocator
                .assigned_tag(tags.len(), "Imported audio", "firing component")?
                .0;
            retarget_weapon_component_owner_payload(&mut owner, entity, owner_tag, private)
                .map_err(invalid)?;
            retarget_weapon_component_owner(entity, owner_tag, private).map_err(invalid)?;
            let emitted = append(
                manager,
                allocator,
                tags,
                owner_tag,
                owner,
                crate::NewTagStorageMode::InheritTemplate,
                "firing component",
            )?;
            if emitted != private {
                return Err(invalid("Firing component allocation changed"));
            }
            remap.insert(owner_tag, private);
        }
        for (entity_tag, entity) in entities {
            let private = append(
                manager,
                allocator,
                tags,
                entity_tag,
                entity,
                crate::NewTagStorageMode::InheritTemplate,
                "firing entity",
            )?;
            remap.insert(entity_tag, private);
            for (slot, old) in &links {
                if !direct && *old == entity_tag {
                    put(&mut root, 0x18 + slot * 0x10, private)?;
                }
            }
        }
        let private_parent = if direct {
            *remap
                .get(&root_tag)
                .ok_or_else(|| invalid("Direct firing entity was not authored"))?
        } else {
            let private_root = append(
                manager,
                allocator,
                tags,
                root_tag,
                root,
                crate::NewTagStorageMode::InheritTemplate,
                "presentation root",
            )?;
            remap.insert(root_tag, private_root);
            let mut parent = stock(manager, parent_tag, PARENT_CLASS, "presentation parent")?;
            if word(&parent, 16)? != root_tag {
                return Err(invalid("Presentation root link changed"));
            }
            put(&mut parent, 16, private_root)?;
            let private_parent = append(
                manager,
                allocator,
                tags,
                parent_tag,
                parent,
                crate::NewTagStorageMode::InheritTemplate,
                "presentation parent",
            )?;
            remap.insert(parent_tag, private_parent);
            let private_companion =
                allocator.assigned_tag(tags.len(), "Imported audio", "presentation companion")?;
            let (template, payload) = stock_companion(manager, parent_tag)?;
            remap.insert(template.0, private_companion.0);
            let mut dependencies = BTreeSet::new();
            for index in start..tags.len() {
                dependencies.insert(
                    allocator
                        .assigned_tag(index, "Imported audio", "presentation dependency")?
                        .0,
                );
            }
            for index in asset_start..asset_tags.len() {
                dependencies.insert(
                    assets
                        .assigned_tag(index, "Imported audio", "sound dependency")?
                        .0,
                );
            }
            dependencies.insert(private_companion.0);
            let payload = clone_scoped_dependencies(
                (
                    &payload,
                    LoadingOwner {
                        owner: TagHash(parent_tag),
                        companion: template,
                    },
                ),
                LoadingOwner {
                    owner: TagHash(private_parent),
                    companion: private_companion,
                },
                &dependencies.into_iter().map(TagHash).collect::<Vec<_>>(),
                &[],
            )?;
            append(
                manager,
                allocator,
                tags,
                template.0,
                payload,
                crate::NewTagStorageMode::InheritTemplate,
                "presentation companion",
            )?;
            private_parent
        };
        // HUD and runtime edits may already have cloned this component. Apply
        // the presentation link to the live private owner so those edits survive.
        let entity = Payload(pattern.to_vec());
        let bound = entity
            .array(16, 12, Some(0x80809C04))
            .map_err(|error| invalid(format!("Presentation component bindings: {error:#}")))?
            .into_iter()
            .map(|at| word(pattern, at))
            .collect::<AuthoringResult<BTreeSet<_>>>()?;
        let existing = live_owner(tags, allocator, pattern_owner, &bound)?;
        let mut owner = if let Some(ordinal) = existing {
            tags[ordinal].payload.clone()
        } else {
            stock(
                manager,
                pattern_owner,
                OWNER_CLASS,
                "presentation pattern owner",
            )?
        };
        patch_parent(&mut owner, &events, parent_tag, private_parent)?;
        if let Some(ordinal) = existing {
            tags[ordinal].payload = owner;
            continue;
        }
        let private = allocator
            .assigned_tag(tags.len(), "Imported audio", "presentation pattern owner")?
            .0;
        retarget_weapon_component_owner_payload(&mut owner, pattern, pattern_owner, private)
            .map_err(invalid)?;
        retarget_weapon_component_owner(pattern, pattern_owner, private).map_err(invalid)?;
        append(
            manager,
            allocator,
            tags,
            pattern_owner,
            owner,
            crate::NewTagStorageMode::InheritTemplate,
            "presentation pattern owner",
        )?;
    }
    Ok(count)
}
