//! Private presentation closure for source firing sounds and prepared firing flows.
use super::effect::{FiringEffect, Prepared};
use super::*;

/// A presentation chain: its owner, parent, root and direct-entity layout.
type Chain = (u32, u32, u32, bool);
/// Firing events keyed by the presentation chain they patch.
type PresentationGroups<'a> = BTreeMap<Chain, Vec<&'a Value>>;

fn chain(presentation: &Value) -> AuthoringResult<Chain> {
    Ok((
        hex(&presentation["owner"], "presentation owner")?,
        hex(&presentation["parent"], "presentation parent")?,
        hex(&presentation["root"], "presentation root")?,
        presentation["direct_entity"].as_bool().unwrap_or(false),
    ))
}

fn parent_offset(presentation: &Value) -> AuthoringResult<usize> {
    presentation["parent_offset"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| invalid("Presentation parent offset missing"))
}

/// The firing events grouped by the presentation chain they patch, with the prepared flow's
/// chain present even when no sound shares it.
fn presentation_groups<'a>(
    audio: Option<&'a ImportedAudio>,
    effect: Option<&FiringEffect>,
) -> AuthoringResult<PresentationGroups<'a>> {
    let mut groups = PresentationGroups::new();
    for event in audio.into_iter().flat_map(|audio| &audio.firing) {
        groups
            .entry(chain(&event["native_presentation"])?)
            .or_default()
            .push(event);
    }
    if let Some(effect) = effect {
        groups.entry(chain(&effect.presentation)?).or_default();
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

/// The word a presentation's owner row stores at its parent offset before the private parent
/// replaces it: the stock parent, or for an adopted presentation the unset `FFFFFFFF` of a row
/// that bound none.
fn bound_parent(presentation: &Value) -> AuthoringResult<u32> {
    match presentation.get("bound_parent") {
        Some(value) => hex(value, "presentation bound parent"),
        None => hex(&presentation["parent"], "presentation parent"),
    }
}

/// The root slots an adopted presentation does not use. The adopted structure belongs to a stock
/// weapon, whose other slot entities must not play.
fn cleared_slots(presentation: &Value) -> AuthoringResult<Vec<usize>> {
    presentation["cleared_slots"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|slot| {
            slot.as_u64()
                .filter(|slot| *slot < 6)
                .map(|slot| slot as usize)
                .ok_or_else(|| invalid("Presentation cleared slot is invalid"))
        })
        .collect()
}

fn patch_parent(
    owner: &mut [u8],
    offsets: &BTreeMap<usize, u32>,
    private: u32,
) -> AuthoringResult<()> {
    for (&at, &bound) in offsets {
        if word(owner, at)? != bound {
            return Err(invalid("Presentation parent link changed"));
        }
        put(owner, at, private)?;
    }
    Ok(())
}

/// Record the word an owner row's parent offset must hold. Two presentations of one chain that
/// expect different words at one offset cannot both be patched.
fn expect_parent(offsets: &mut BTreeMap<usize, u32>, at: usize, bound: u32) -> AuthoringResult<()> {
    if *offsets.entry(at).or_insert(bound) != bound {
        return Err(invalid("Presentation parent offset is bound twice"));
    }
    Ok(())
}

/// Gives the stock slot entity the connection rows a prepared flow needs, while its stock rows are
/// still the ones the flow was prepared against. The rows name stock owners, so retargeting each
/// owner afterwards moves them with the rest.
fn connect(
    entity: &mut Vec<u8>,
    entity_tag: u32,
    prepared: &BTreeMap<(u32, u32), &Prepared>,
) -> AuthoringResult<()> {
    let mut rows = prepared
        .values()
        .filter(|flow| flow.entity == entity_tag)
        .filter_map(|flow| flow.connections.as_ref());
    let Some((before, after)) = rows.next() else {
        return Ok(());
    };
    if rows.next().is_some() {
        return Err(invalid(
            "Prepared firing owners replace one entity's connections twice",
        ));
    }
    let offsets = Payload(entity.clone())
        .array(0x20, 72, Some(0x80809BC9))
        .map_err(|error| invalid(format!("Firing entity connections: {error:#}")))?;
    let current = offsets
        .iter()
        .flat_map(|&row| entity[row..row + 72].to_vec())
        .collect::<Vec<_>>();
    if current != *before {
        return Err(invalid(
            "Firing entity connections changed since import. Import this weapon again",
        ));
    }
    crate::tag_payload::append_native_array(entity, 0x20, 0x80809BC9, after.len() / 72, after)?;
    let length = u64::try_from(entity.len()).map_err(|_| invalid("Firing entity is too large"))?;
    entity[..8].copy_from_slice(&length.to_le_bytes());
    Ok(())
}

/// Where a sealed prepared flow keeps the live copy of the word at `offset`. Sealing moves the
/// schema view into a copy of the record stream, so a patched definition field has two places. A
/// prepared flow whose records did not grow, such as one that only names private particle
/// systems, is not sealed and holds the field once, which gives `None`.
fn sealed_copy(owner: &[u8], offset: usize) -> AuthoringResult<Option<usize>> {
    parhelion_import::tiger::instance::sealed_copy(&Payload(owner.to_vec()), offset)
        .map_err(|error| invalid(format!("Prepared firing flow: {error:#}")))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn author(
    manager: &PackageManager,
    audio: Option<&ImportedAudio>,
    effect: Option<(&FiringEffect, &BTreeMap<String, TagHash>)>,
    pattern: &mut [u8],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
    (assets, asset_tags): (AppendedTagAllocator, &mut Vec<NewTagSpec>),
    mut media: Option<&mut Media<'_>>,
) -> AuthoringResult<usize> {
    let mut count = 0;
    let effect_chain = effect
        .map(|(effect, _)| chain(&effect.presentation))
        .transpose()?;
    for ((pattern_owner, parent_tag, root_tag, direct), events) in
        presentation_groups(audio, effect.map(|(effect, _)| effect))?
    {
        let start = tags.len();
        let asset_start = asset_tags.len();
        if let Some(media) = media.as_deref_mut() {
            media.borrowed();
        }
        let mut root = root(manager, parent_tag, root_tag, direct)?;
        let mut remap = BTreeMap::new();
        let mut owners = BTreeMap::<(u32, u32), Vec<u8>>::new();
        let mut prepared = BTreeMap::<(u32, u32), &Prepared>::new();
        let mut links = BTreeSet::new();
        let mut offsets = BTreeMap::new();
        let mut cleared = Vec::new();
        if let Some((effect, symbols)) = effect
            && effect_chain == Some((pattern_owner, parent_tag, root_tag, direct))
        {
            expect_parent(
                &mut offsets,
                parent_offset(&effect.presentation)?,
                bound_parent(&effect.presentation)?,
            )?;
            cleared = cleared_slots(&effect.presentation)?;
            for flow in &effect.owners {
                validate_slot(&root, direct, flow.slot, flow.entity, root_tag)?;
                links.insert((flow.slot, flow.entity));
                stock(manager, flow.owner, OWNER_CLASS, "firing flow")?;
                if owners
                    .insert((flow.entity, flow.owner), flow.payload(symbols)?)
                    .is_some()
                {
                    return Err(invalid("Prepared firing flow is repeated"));
                }
                prepared.insert((flow.entity, flow.owner), flow);
            }
        }
        for event in &events {
            let audio = audio.ok_or_else(|| invalid("Firing sounds need imported audio"))?;
            expect_parent(
                &mut offsets,
                parent_offset(&event["native_presentation"])?,
                bound_parent(&event["native_presentation"])?,
            )?;
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
            let media = media
                .as_deref_mut()
                .ok_or_else(|| invalid("Firing sounds need the build's shared media"))?;
            let private = private_sound(
                manager, audio, source, native, &name, assets, asset_tags, &mut remap, media,
            )?;
            put(owner, offset, private)?;
            if prepared.contains_key(&(entity_tag, owner_tag))
                && let Some(copy) = sealed_copy(owner, offset)?
            {
                if word(owner, copy)? != native_sound {
                    return Err(invalid("Prepared firing flow sound copy changed"));
                }
                put(owner, copy, private)?;
            }
            count += 1;
        }
        emit_components(
            manager,
            (allocator, tags),
            owners,
            &prepared,
            (direct, &links, &mut root),
            &mut remap,
        )?;
        clear_unbound_slots(&mut root, &cleared, &links, direct)?;
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
            allocator.reserve(tags.len(), 2)?;
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
            // Media an earlier sound of the build allocated load with this presentation too.
            if let Some(media) = media.as_deref_mut() {
                dependencies.extend(media.borrowed());
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
        patch_parent(&mut owner, &offsets, private_parent)?;
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

type PreparedOwners<'a> = BTreeMap<(u32, u32), &'a Prepared>;
type Owners = BTreeMap<(u32, u32), Vec<u8>>;

fn clear_unbound_slots(
    root: &mut [u8],
    cleared: &[usize],
    links: &BTreeSet<(usize, u32)>,
    direct: bool,
) -> AuthoringResult<()> {
    if cleared.is_empty() {
        return Ok(());
    }
    if direct || links.iter().any(|(slot, _)| cleared.contains(slot)) {
        return Err(invalid(
            "Adopted firing presentation clears a slot it binds",
        ));
    }
    for &slot in cleared {
        put(root, 0x18 + slot * 0x10, u32::MAX)?;
    }
    Ok(())
}

fn emit_components(
    manager: &PackageManager,
    (allocator, tags): (AppendedTagAllocator, &mut Vec<NewTagSpec>),
    owners: Owners,
    prepared: &PreparedOwners<'_>,
    (direct, links, root): (bool, &BTreeSet<(usize, u32)>, &mut [u8]),
    remap: &mut BTreeMap<u32, u32>,
) -> AuthoringResult<()> {
    let mut entities = BTreeMap::<u32, Vec<u8>>::new();
    for ((entity_tag, owner_tag), mut owner) in owners {
        let entity = match entities.entry(entity_tag) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let mut entity = stock(manager, entity_tag, 0x80809C0F, "firing entity")?;
                connect(&mut entity, entity_tag, prepared)?;
                entry.insert(entity)
            }
        };
        // A rebuilt flow's controls and events grow its instance allocation, which the
        // private owner names in place of the shared stock one.
        if let Some(flow) = prepared.get(&(entity_tag, owner_tag)) {
            let (template, bytes) = &flow.allocation;
            if word(&owner, 0x44)? != *template {
                return Err(invalid("Prepared firing flow names another allocation"));
            }
            let allocation = append(
                manager,
                allocator,
                tags,
                *template,
                bytes.clone(),
                crate::NewTagStorageMode::InheritTemplate,
                "firing flow allocation",
            )?;
            put(&mut owner, 0x44, allocation)?;
        }
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
        for (slot, old) in links {
            if !direct && *old == entity_tag {
                put(root, 0x18 + slot * 0x10, private)?;
            }
        }
    }
    Ok(())
}
