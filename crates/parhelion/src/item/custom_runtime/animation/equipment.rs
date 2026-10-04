//! Link converted equipment clips without changing shared character or stock runtime assets.
use super::*;
use parhelion_import::d2_mot::payload::Payload;

pub(in crate::item) struct EquipmentAnimation {
    entity: (u32, Vec<u8>),
    lookup: (u32, Vec<u8>),
    bank: (u32, Vec<u8>),
    converted_bank: Vec<u8>,
    consumers: Vec<(u32, Vec<u8>)>,
    clips: Vec<(u32, Vec<u8>)>,
}

impl EquipmentAnimation {
    pub(in crate::item) fn asset_bounds(&self) -> Vec<usize> {
        self.clips
            .iter()
            .map(|(_, payload)| payload.len())
            .collect()
    }
}

pub(in crate::item) fn load(graph: &GraphReference) -> AuthoringResult<Option<EquipmentAnimation>> {
    let value: Value = serde_json::from_slice(
        &fs::read(graph.directory.join("asset-graph.json"))
            .map_err(|error| invalid(format!("Imported equipment graph: {error}")))?,
    )
    .map_err(|error| invalid(format!("Imported equipment graph: {error}")))?;
    let section = &value["equipment_animation"];
    if section["status"] != "linked" {
        return Ok(None);
    }
    // Equipment graphs name their kind. A weapon graph names none, and its section carries the
    // runtime's own clips, as an imported glaive's turning part does.
    if !matches!(
        value["kind"].as_str(),
        None | Some("ghost_shell" | "ship" | "sparrow")
    ) {
        return Err(invalid(
            "Equipment animation requires an equipment or weapon graph",
        ));
    }
    let files = &section["files"];
    let clips = section["clips"]
        .as_array()
        .ok_or_else(|| invalid("Equipment clips missing"))?
        .iter()
        .map(|clip| Ok((tag(clip, "native")?, payload(graph, &clip["file"])?)))
        .collect::<AuthoringResult<Vec<_>>>()?;
    if clips.is_empty() {
        return Err(invalid("Linked equipment animation has no converted clips"));
    }
    let consumers = section["bank_consumers"]
        .as_array()
        .ok_or_else(|| invalid("Equipment bank consumers missing"))?
        .iter()
        .map(|consumer| {
            let key = consumer["file_key"]
                .as_str()
                .ok_or_else(|| invalid("Equipment bank consumer file missing"))?;
            Ok((tag(consumer, "owner")?, payload(graph, &files[key])?))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    if consumers.is_empty() {
        return Err(invalid("Equipment animation has no clip-bank consumers"));
    }
    Ok(Some(EquipmentAnimation {
        entity: (
            tag(section, "runtime_entity")?,
            payload(graph, &files["entity"])?,
        ),
        lookup: (
            tag(section, "lookup_owner")?,
            payload(graph, &files["lookup_owner"])?,
        ),
        bank: (tag(section, "bank")?, payload(graph, &files["bank"])?),
        converted_bank: payload(graph, &files["converted_bank"])?,
        consumers,
        clips,
    }))
}

fn validate_bank(animation: &EquipmentAnimation) -> AuthoringResult<()> {
    let mut expected = animation.bank.1.clone();
    let (count, _, clips, class) = array_at(&expected, 8)?;
    let (descriptors, _, rows, descriptor_class) = array_at(&expected, 0x68)?;
    if class != 0x80808F48 || descriptor_class != 0x80809002 {
        return Err(invalid("Equipment clip bank has an unexpected layout"));
    }
    for index in 0..descriptors {
        let row = rows + index * 32;
        let index = usize::from(read_u16(&expected, row + 24)?);
        if index >= count {
            return Err(invalid("Equipment descriptor clip is outside its bank"));
        }
        let old = read_u32(&expected, clips + index * 4)?;
        let Some((_, bytes)) = animation.clips.iter().find(|(tag, _)| *tag == old) else {
            continue;
        };
        if expected.get(row..row + 16) != Some([0; 16].as_slice()) {
            return Err(invalid("Equipment descriptor has inline tracks"));
        }
        let frames = read_u16(bytes, 0x13C)?;
        if frames == 0 {
            return Err(invalid("Equipment clip has no frames"));
        }
        write_u32(
            &mut expected,
            row + 20,
            (f32::from(frames - 1) / 30.0).to_bits(),
        )?;
    }
    if expected != animation.converted_bank {
        return Err(invalid(
            "Equipment bank changed outside converted clip durations",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(in crate::item) fn author(
    manager: &PackageManager,
    animation: &EquipmentAnimation,
    entity_tag: TagHash,
    entity: &mut [u8],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
    (assets, asset_tags): (AppendedTagAllocator, &mut Vec<NewTagSpec>),
    cache: &mut AnimationTagCache,
) -> AuthoringResult<()> {
    if animation.entity.0 != entity_tag.0 {
        return Err(invalid(
            "Imported equipment animation was prepared for a different runtime",
        ));
    }
    stock(
        manager,
        &animation.entity,
        FIRST_PERSON_ENTITY_CLASS,
        "equipment runtime",
    )?;
    stock(
        manager,
        &animation.lookup,
        RESOURCE_OWNER_CLASS,
        "equipment lookup",
    )?;
    stock(manager, &animation.bank, CLIP_BANK_CLASS, "equipment bank")?;
    validate_bank(animation)?;
    let mut bank = animation.converted_bank.clone();
    let mut seen = BTreeSet::new();
    for (old, bytes) in &animation.clips {
        if !seen.insert(*old)
            || manager
                .get_entry(TagHash(*old))
                .is_none_or(|e| e.reference != CLIP_CLASS)
            || bytes.len() < 0x190
            || read_u64(bytes, 0)? != bytes.len() as u64
        {
            return Err(invalid(
                "Imported equipment clip has an invalid template or payload",
            ));
        }
        let original = read_tag(manager, TagHash(*old), "equipment clip")?;
        if read_u32(&original, 0x120)? != read_u32(bytes, 0x120)? {
            return Err(invalid(
                "Imported equipment clip changed its native trigger name",
            ));
        }
        use sha2::Digest;
        let key = (CLIP_CLASS, sha2::Sha256::digest(bytes).into());
        let private = if let Some(private) = cache.get(&key) {
            *private
        } else {
            let private = assets
                .assigned_tag(asset_tags.len(), "Equipment animation", "clip")?
                .0;
            asset_tags.push(NewTagSpec {
                template_tag: TagHash(*old),
                payload: bytes.clone(),
                storage: crate::NewTagStorageMode::InheritTemplate,
            });
            cache.insert(key, private);
            private
        };
        relink(&mut bank, *old, private, "equipment bank")?;
    }
    let bank_tag = allocator
        .assigned_tag(tags.len(), "Equipment animation", "clip bank")?
        .0;
    tags.push(NewTagSpec {
        template_tag: TagHash(animation.bank.0),
        payload: bank,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    let mut owners = vec![(&animation.lookup, true)];
    owners.extend(animation.consumers.iter().map(|owner| (owner, false)));
    let mut seen = BTreeSet::new();
    for (original, lookup) in owners {
        if !seen.insert(original.0) {
            return Err(invalid("Equipment animation owner is repeated"));
        }
        stock(
            manager,
            original,
            RESOURCE_OWNER_CLASS,
            "equipment animation owner",
        )?;
        let mut owner = Payload(original.1.clone());
        let field = if lookup {
            let at = owner.pointer(24).map_err(|e| invalid(e.to_string()))?;
            let class = at
                .checked_sub(4)
                .ok_or_else(|| invalid("Equipment lookup has no resource header"))?;
            if read_u32(&owner.0, class)? != 0x8080344B {
                return Err(invalid("Equipment lookup owner has an unexpected class"));
            }
            at + 0x90
        } else {
            consumers::bank_field(&owner).map_err(|e| invalid(e.to_string()))?
        };
        if read_u32(&owner.0, field)? != animation.bank.0 {
            return Err(invalid("Equipment animation owner names another clip bank"));
        }
        write_u32(&mut owner.0, field, bank_tag)?;
        let private = allocator
            .assigned_tag(tags.len(), "Equipment animation", "owner")?
            .0;
        retarget_weapon_component_owner_payload(
            &mut owner.0,
            &animation.entity.1,
            original.0,
            private,
        )
        .map_err(invalid)?;
        retarget_weapon_component_owner(entity, original.0, private).map_err(invalid)?;
        tags.push(NewTagSpec {
            template_tag: TagHash(original.0),
            payload: owner.0,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
    }
    validate_weapon_entity(entity).map_err(invalid)
}
