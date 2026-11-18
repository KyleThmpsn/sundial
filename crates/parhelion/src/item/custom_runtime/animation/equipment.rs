//! Link converted equipment clips without changing shared character or stock runtime assets.
use super::*;
use parhelion_import::tiger::payload::Payload;

struct Rig {
    original: (u32, Vec<u8>),
    converted: Vec<u8>,
    relocation: Option<(usize, usize)>,
}

struct Clip {
    native: u32,
    payload: Vec<u8>,
    cues: Vec<(usize, String)>,
}

fn clip(graph: &Inputs, value: &serde_json::Value) -> AuthoringResult<Clip> {
    let payload = payload(graph, &value["file"])?;
    let parsed = Payload(payload.clone());
    let mut fields = BTreeSet::new();
    for row in parsed
        .array(0x160, 8, Some(0x8080902B))
        .map_err(|e| invalid(e.to_string()))?
    {
        let at = parsed.pointer(row).map_err(|e| invalid(e.to_string()))?;
        if at >= 4 && read_u32(&payload, at - 4)? == 0x80809033 {
            fields.insert(at + 24);
        }
    }
    let mut cues = Vec::new();
    for patch in value["patches"].as_array().into_iter().flatten() {
        let at = patch["offset"]
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| invalid("Equipment audio patch has no offset"))?;
        let symbol = patch["symbol"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| invalid("Equipment audio patch has no symbol"))?;
        if !fields.remove(&at) || read_u32(&payload, at)? != u32::MAX {
            return Err(invalid(
                "Equipment audio patch is not a unique resource-event placeholder",
            ));
        }
        cues.push((at, symbol.to_owned()));
    }
    Ok(Clip {
        native: tag(value, "native")?,
        payload,
        cues,
    })
}

/// A private layer table for the pose controller `owner`, which plays none of the stock table's
/// supplemental layers.
struct Layers {
    owner: u32,
    original: (u32, Vec<u8>),
    converted: Vec<u8>,
}

impl Layers {
    fn load(graph: &Inputs, section: &serde_json::Value) -> AuthoringResult<Option<Self>> {
        let Some(layers) = section.get("pose_layers").filter(|v| !v.is_null()) else {
            return Ok(None);
        };
        Ok(Some(Self {
            owner: tag(layers, "owner")?,
            original: (
                tag(layers, "tag")?,
                payload(graph, &layers["template_file"])?,
            ),
            converted: payload(graph, &layers["file"])?,
        }))
    }

    /// Checks the table against stock and its controller, then appends it. It may differ from
    /// stock only in its dispatch, where every descriptor takes no route.
    fn author(
        &self,
        manager: &PackageManager,
        consumers: &[(u32, Vec<u8>)],
        allocator: AppendedTagAllocator,
        tags: &mut Vec<NewTagSpec>,
    ) -> AuthoringResult<u32> {
        stock(manager, &self.original, 0x808034EB, "equipment pose layers")?;
        let original = Payload(self.original.1.clone());
        let mut expected = self.original.1.clone();
        for at in original
            .array(40, 2, Some(0x80800006))
            .map_err(|e| invalid(e.to_string()))?
        {
            expected[at..at + 2].copy_from_slice(&u16::MAX.to_le_bytes());
        }
        if expected != self.converted {
            return Err(invalid(
                "Equipment pose layers changed outside their dispatch",
            ));
        }
        let mut owners = consumers.iter().filter(|(tag, _)| *tag == self.owner);
        let (Some((_, owner)), None) = (owners.next(), owners.next()) else {
            return Err(invalid(
                "Equipment pose layer controller is absent or repeated",
            ));
        };
        if read_u32(owner, Self::field(owner)?)? != self.original.0 {
            return Err(invalid("Equipment pose layer binding changed"));
        }
        let private = allocator
            .assigned_tag(tags.len(), "Equipment animation", "pose layers")?
            .0;
        tags.push(NewTagSpec {
            template_tag: TagHash(self.original.0),
            payload: self.converted.clone(),
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        Ok(private)
    }

    /// Where the pose controller `owner` names its layer table.
    fn field(owner: &[u8]) -> AuthoringResult<usize> {
        let at = Payload(owner.to_vec())
            .pointer(24)
            .map_err(|e| invalid(e.to_string()))?;
        if at < 4 || read_u32(owner, at - 4)? != 0x80803640 {
            return Err(invalid(
                "Equipment pose layer owner has an unexpected class",
            ));
        }
        Ok(at + 0x11C)
    }
}

pub(in crate::item) struct EquipmentAnimation {
    entity: (u32, Vec<u8>),
    lookup: (u32, Vec<u8>),
    bank: (u32, Vec<u8>),
    converted_bank: Vec<u8>,
    consumers: Vec<(u32, Vec<u8>)>,
    layers: Option<Layers>,
    clips: Vec<Clip>,
    rigs: Vec<Rig>,
    preserve_timing: bool,
}

pub(in crate::item) fn load(graph: &Inputs) -> AuthoringResult<Option<EquipmentAnimation>> {
    let value = graph.value();
    let section = &value["equipment_animation"];
    if section["status"] != "linked" {
        return Ok(None);
    }
    // Equipment graphs name their kind. A modern weapon graph names none and a Rise of Iron one
    // names `weapon`. Their sections carry the runtime's own clips, as an imported glaive's
    // turning part does.
    if !matches!(
        value["kind"].as_str(),
        None | Some("weapon" | "ghost_shell" | "ship" | "sparrow")
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
        .map(|value| clip(graph, value))
        .collect::<AuthoringResult<Vec<_>>>()?;
    let layers = Layers::load(graph, section)?;
    // A section can be linked only to take the donor's supplemental layers off the weapon.
    if clips.is_empty() && layers.is_none() {
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
    let rigs = section["rigs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|rig| {
            let relocation = if rig["relocation"].is_null() {
                None
            } else {
                Some(
                    serde_json::from_value::<(usize, usize)>(rig["relocation"].clone())
                        .map_err(|e| invalid(format!("Equipment rig relocation: {e}")))?,
                )
            };
            Ok(Rig {
                original: (tag(rig, "owner")?, payload(graph, &rig["original_file"])?),
                converted: payload(graph, &rig["file"])?,
                relocation,
            })
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
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
        layers,
        clips,
        rigs,
        preserve_timing: section["preserve_timing"] == true,
    }))
}

fn validate_bank(animation: &EquipmentAnimation) -> AuthoringResult<()> {
    if animation.preserve_timing {
        if animation.bank.1 != animation.converted_bank {
            return Err(invalid(
                "Equipment animation changed a retained native bank",
            ));
        }
        return Ok(());
    }
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
        let Some(clip) = animation.clips.iter().find(|clip| clip.native == old) else {
            continue;
        };
        if expected.get(row..row + 16) != Some([0; 16].as_slice()) {
            return Err(invalid("Equipment descriptor has inline tracks"));
        }
        let frames = read_u16(&clip.payload, 0x13C)?;
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
    symbols: &BTreeMap<String, TagHash>,
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
    for clip in &animation.clips {
        let old = &clip.native;
        let bytes = &clip.payload;
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
        if animation.preserve_timing && read_u16(&original, 0x13C)? != read_u16(bytes, 0x13C)? {
            return Err(invalid(
                "Equipment animation changed a retained native frame count",
            ));
        }
        let mut bytes = bytes.clone();
        for (at, symbol) in &clip.cues {
            let tag = symbols.get(symbol).ok_or_else(|| {
                invalid(format!("Equipment audio cue {symbol} was not allocated"))
            })?;
            write_u32(&mut bytes, *at, tag.0)?;
        }
        use sha2::Digest;
        let key = (CLIP_CLASS, sha2::Sha256::digest(&bytes).into());
        let private = if let Some(private) = cache.get(&key) {
            *private
        } else {
            assets.reserve_payload(asset_tags.len(), bytes.len())?;
            let private = assets
                .assigned_tag(asset_tags.len(), "Equipment animation", "clip")?
                .0;
            asset_tags.push(NewTagSpec {
                template_tag: TagHash(*old),
                payload: bytes,
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
    let layers = animation
        .layers
        .as_ref()
        .map(|layers| {
            let private = layers.author(manager, &animation.consumers, allocator, tags)?;
            Ok((layers, private))
        })
        .transpose()?;
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
        if let Some((layers, table)) = layers
            && layers.owner == original.0
        {
            let field = Layers::field(&owner.0)?;
            write_u32(&mut owner.0, field, table)?;
        }
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
    author_rigs(manager, animation, entity, allocator, tags)?;
    validate_weapon_entity(entity).map_err(invalid)
}

fn author_rigs(
    manager: &PackageManager,
    animation: &EquipmentAnimation,
    entity: &mut [u8],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<()> {
    let mut seen = BTreeSet::new();
    for rig in &animation.rigs {
        if !seen.insert(rig.original.0) {
            return Err(invalid("Equipment rig owner is repeated"));
        }
        stock(
            manager,
            &rig.original,
            RESOURCE_OWNER_CLASS,
            "equipment rig",
        )?;
        let original = Payload(rig.original.1.clone());
        let converted = Payload(rig.converted.clone());
        let before = original.pointer(24).map_err(|e| invalid(e.to_string()))?;
        let after = converted.pointer(24).map_err(|e| invalid(e.to_string()))?;
        let class = original
            .u32(before - 4)
            .map_err(|e| invalid(e.to_string()))?;
        if converted
            .u32(after - 4)
            .map_err(|e| invalid(e.to_string()))?
            != class
        {
            return Err(invalid("Equipment rig changed its native resource class"));
        }
        match (class, rig.relocation) {
            (0x80808546, Some((cut, delta)))
                if delta > 0
                    && delta.is_multiple_of(32)
                    && cut.checked_add(8) == Some(before)
                    && before.checked_add(delta) == Some(after) =>
            {
                parhelion_import::tiger::animation::sections::validate_native_instance(
                    &rig.converted,
                )
                .map_err(|e| invalid(format!("Equipment skeleton: {e:#}")))?;
                parhelion_import::tiger::animation::rig::rebase_entity(
                    entity,
                    &original,
                    rig.original.0,
                    cut,
                    delta,
                )
                .map_err(|e| invalid(format!("Equipment skeleton pointers: {e:#}")))?;
            }
            (0x80808F8F, None) if before == after => {
                parhelion_import::tiger::animation::rig::controls::validate(&rig.converted)
                    .map_err(|e| invalid(format!("Equipment controls: {e:#}")))?;
            }
            _ => return Err(invalid("Equipment rig has an unsupported relocation")),
        }
        let private = allocator
            .assigned_tag(tags.len(), "Equipment animation", "rig")?
            .0;
        let mut bytes = rig.converted.clone();
        retarget_weapon_component_owner_payload(&mut bytes, entity, rig.original.0, private)
            .map_err(invalid)?;
        retarget_weapon_component_owner(entity, rig.original.0, private).map_err(invalid)?;
        parhelion_import::tiger::animation::rig::retarget_rebased_references(
            entity,
            bytes.len(),
            rig.original.0,
            private,
        )
        .map_err(|e| invalid(format!("Equipment rig references: {e:#}")))?;
        tags.push(NewTagSpec {
            template_tag: TagHash(rig.original.0),
            payload: bytes,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
    }
    Ok(())
}
