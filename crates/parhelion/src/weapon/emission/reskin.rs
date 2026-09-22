//! Cross-family appearances: pin every gear part to the runtime rig's root bone.
//!
//! Gear parts are rigid meshes whose 8-byte vertex rows carry a bone index into the family's
//! weapon skeleton (auto rifles have four bones, hand cannons eight). A hand cannon part on an
//! auto rifle runtime therefore names bones that do not exist. The private copies made here
//! select bone 0 everywhere, so each part rides the grip and every family's clips resolve.
//! Moving parts stay still; nothing dereferences a missing bone.
use super::*;
use crate::tag_payload::relative_target;
use linking::{Companion, Companions, Node};

const ENTITY_CLASS: u32 = 0x8080_9C0F;
const RESOURCE_CLASS: u32 = 0x8080_9C36;
const MODEL_OWNER_HEADER: u32 = 0x8080_72B8;
const MODEL_OWNER_DATA: u32 = 0x8080_72BD;
const MODEL_CLASS: u32 = 0x8080_73A5;
const MESH_ROW_CLASS: u32 = 0x8080_7378;
const MODEL_SLOT: usize = 0x1DC;
const BONE_PALETTE: usize = 0x40;

pub(super) fn apply(
    directory: &Path,
    emission: &mut PackageEmission,
    weapons: &[WeaponCloneSpec],
    replacements: &mut Vec<ReplacementSpec>,
) -> AuthoringResult<()> {
    let mut manager = None;
    let mut companions = Companions::new();
    for spec in weapons {
        let Some(presentation) = &spec.presentation_donor else {
            continue;
        };
        let manager = match &manager {
            Some(manager) => manager,
            None => manager.insert(
                sundial::package_authoring::PackageManager::new(
                    directory,
                    tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
                    None,
                )
                .map_err(|e| invalid(e.to_string()))?,
            ),
        };
        let item = spec.identity.item_hash;
        let ordinal = definition_ordinal(emission, item)?;
        let authored = group(
            &emission.sandbox_patterns,
            weapon_pattern_index(&emission.host_new_tags[ordinal].payload)?,
        )?;
        let donor_definition = stock_definition(emission, manager, presentation.item_hash)?;
        let donor = group(
            &emission.sandbox_patterns,
            weapon_pattern_index(&donor_definition)?,
        )?;
        let (Some(authored), Some(donor)) = (authored, donor) else {
            continue;
        };
        if authored == donor {
            continue;
        }
        reskin(
            directory,
            emission,
            manager,
            &mut companions,
            replacements,
            item,
            ordinal,
        )
        .map_err(|error| {
            error.context(format!(
                "Weapon {:?}: pinning appearance 0x{:08X} to the runtime rig",
                spec.text.name, presentation.item_hash
            ))
        })?;
    }
    Ok(())
}

/// The translation group of a sandbox pattern row, when the row names one.
fn group(patterns: &[u8], index: Option<u16>) -> AuthoringResult<Option<u32>> {
    let Some(index) = index else {
        return Ok(None);
    };
    let pattern = sandbox_pattern_identity_at(patterns, usize::from(index)).map_err(invalid)?;
    Ok(pattern
        .map(|pattern| pattern.weapon_translation_group_hash)
        .filter(|hash| !matches!(*hash, 0 | 0x811C_9DC5)))
}

fn stock_definition(
    emission: &PackageEmission,
    manager: &sundial::package_authoring::PackageManager,
    item: u32,
) -> AuthoringResult<Vec<u8>> {
    let (count, _, rows, _) = array(&emission.item_table, 8)?;
    let matches = (0..count)
        .map(|i| rows + i * 24)
        .filter(|&row| read_u32(&emission.item_table, row).ok() == Some(item))
        .collect::<Vec<_>>();
    let [row] = matches.as_slice() else {
        return Err(invalid("Appearance donor is missing or ambiguous"));
    };
    read(manager, read_u32(&emission.item_table, row + 16)?)
}

fn array(data: &[u8], at: usize) -> AuthoringResult<(usize, usize, usize, u32)> {
    sundial::package_authoring::native_payload::native_array_at(data, at).map_err(invalid)
}

fn read(
    manager: &sundial::package_authoring::PackageManager,
    tag: u32,
) -> AuthoringResult<Vec<u8>> {
    manager
        .read_tag(TagHash(tag))
        .map_err(|e| invalid(format!("Reading 0x{tag:08X}: {e}")))
}

fn reskin(
    directory: &Path,
    emission: &mut PackageEmission,
    manager: &sundial::package_authoring::PackageManager,
    companions: &mut Companions,
    replacements: &mut Vec<ReplacementSpec>,
    item: u32,
    ordinal: usize,
) -> AuthoringResult<()> {
    let art_rows = weapon_art_arrangements(&emission.host_new_tags[ordinal].payload)?;
    let indices = art_rows
        .iter()
        .map(|row| row.arrangement)
        .collect::<BTreeSet<_>>();
    let [donor_row] = indices.into_iter().collect::<Vec<_>>()[..] else {
        return Err(invalid(
            "A cross-family appearance must use one gear-art row for every class",
        ));
    };
    let donor_row = usize::from(donor_row);
    let (count, _, rows, _) = array(&emission.item_metadata, 8)?;
    if donor_row >= count {
        return Err(invalid(
            "Appearance gear-art row is outside the metadata table",
        ));
    }
    let keys = art::row_keys(&emission.item_metadata, rows + donor_row * 32)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    if keys.is_empty() {
        return Err(invalid("The appearance lists no gear-art assignments"));
    }
    let table = match replacements
        .iter()
        .find(|replacement| replacement.tag == art::ASSIGNMENT_TABLE)
    {
        Some(replacement) => replacement.payload.clone(),
        None => read(manager, art::ASSIGNMENT_TABLE.0)?,
    };
    let (count, _, map_rows, _) = array(&table, 8)?;
    let map = (0..count)
        .map(|i| {
            Ok((
                read_u32(&table, map_rows + i * 8)?,
                read_u32(&table, map_rows + i * 8 + 4)?,
            ))
        })
        .collect::<AuthoringResult<BTreeMap<u32, u32>>>()?;
    let mut nodes = Vec::new();
    let mut entries = Vec::new();
    let mut substitutions = BTreeMap::new();
    for (index, key) in keys.iter().enumerate() {
        let relation_tag = *map
            .get(key)
            .ok_or_else(|| invalid(format!("Gear-art key 0x{key:08X} has no assignment")))?;
        let prefix = format!("part{index}");
        part_nodes(manager, &prefix, relation_tag, &mut nodes)?;
        let private = private_key(item, *key);
        entries.push((private, format!("{prefix}-parent")));
        substitutions.insert(*key, private);
    }
    let linked = linking::link(
        directory,
        emission,
        manager,
        nodes,
        "part0-parent",
        companions,
        0,
        |_, _| Ok(()),
        None,
    )?;
    let row_index = art::prepare_row_at(emission, item, donor_row)?;
    let (_, _, rows, _) = array(&emission.item_metadata, 8)?;
    art::rewrite(
        &mut emission.item_metadata,
        rows + row_index * 32,
        rows + donor_row * 32,
        |singles, slots| {
            for key in singles
                .iter_mut()
                .chain(slots.iter_mut().flat_map(|(_, keys)| keys))
            {
                if let Some(private) = substitutions.get(key) {
                    *key = *private;
                }
            }
            Ok(())
        },
    )?;
    let arrangement =
        u16::try_from(row_index).map_err(|_| invalid("Gear-art row index overflow"))?;
    let updated = art_rows
        .iter()
        .map(|row| WeaponArtArrangementOverride {
            character_class: row.character_class,
            arrangement,
        })
        .collect::<Vec<_>>();
    set_weapon_art_arrangements(&mut emission.host_new_tags[ordinal].payload, &updated)?;
    let entries = entries
        .iter()
        .map(|(key, symbol)| {
            Ok((
                *key,
                *linked
                    .symbols
                    .get(symbol)
                    .ok_or_else(|| invalid("Parent symbol missing"))?,
            ))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let replacement = art::insert_assignments(&table, &entries)?;
    replacements.retain(|existing| existing.tag != art::ASSIGNMENT_TABLE);
    replacements.push(replacement);
    eprintln!(
        "Pinned {} gear parts of item 0x{item:08X} to the runtime rig; art index {row_index}",
        keys.len()
    );
    Ok(())
}

/// Private copies of one gear part: pinned position buffers, model, model owner, entity and
/// the relation that the assignment map names, with its cloned loading companion.
fn part_nodes(
    manager: &sundial::package_authoring::PackageManager,
    prefix: &str,
    relation_tag: u32,
    nodes: &mut Vec<Node>,
) -> AuthoringResult<()> {
    let relation = read(manager, relation_tag)?;
    let entity_tag = read_u32(&relation, 0x10)?;
    if manager
        .get_entry(TagHash(entity_tag))
        .is_none_or(|entry| entry.reference != ENTITY_CLASS)
    {
        return Err(invalid(format!(
            "Gear-art relation 0x{relation_tag:08X} does not name an entity"
        )));
    }
    let entity = read(manager, entity_tag)?;
    let (count, _, rows, _) = array(&entity, 0x10)?;
    let mut owner = None;
    for i in 0..count {
        let tag = read_u32(&entity, rows + i * 12)?;
        if manager
            .get_entry(TagHash(tag))
            .is_none_or(|entry| entry.reference != RESOURCE_CLASS)
        {
            continue;
        }
        let bytes = read(manager, tag)?;
        let header = relative_target(&bytes, 0x10)?;
        if header >= 4
            && read_u32(&bytes, header - 4)? == MODEL_OWNER_HEADER
            && owner.replace((tag, bytes)).is_some()
        {
            return Err(invalid("A gear part owns several models"));
        }
    }
    let (owner_tag, owner_bytes) = owner.ok_or_else(|| invalid("A gear part owns no model"))?;
    let data = relative_target(&owner_bytes, 0x18)?;
    if data < 4 || read_u32(&owner_bytes, data - 4)? != MODEL_OWNER_DATA {
        return Err(invalid(
            "The gear part's model owner has an unsupported layout",
        ));
    }
    let model_tag = read_u32(&owner_bytes, data + MODEL_SLOT)?;
    if manager
        .get_entry(TagHash(model_tag))
        .is_none_or(|entry| entry.reference != MODEL_CLASS)
    {
        return Err(invalid("The gear part's model slot does not name a model"));
    }
    let model_bytes = read(manager, model_tag)?;
    let model_symbol = format!("{prefix}-model");
    let owner_symbol = format!("{prefix}-owner");
    let entity_symbol = format!("{prefix}-entity");
    let parent_symbol = format!("{prefix}-parent");
    let mut model = Node::new(model_symbol.clone(), model_tag, model_bytes.clone());
    let (meshes, _, mesh_rows, class) = array(&model_bytes, 0x10)?;
    if class != MESH_ROW_CLASS || meshes == 0 {
        return Err(invalid(
            "The gear part's model has an unsupported mesh table",
        ));
    }
    for index in 0..meshes {
        let mesh = mesh_rows + index * 0x88;
        let header_tag = read_u32(&model_bytes, mesh)?;
        let entry = manager
            .get_entry(TagHash(header_tag))
            .filter(|entry| entry.file_type == 32 && entry.file_subtype == 4)
            .ok_or_else(|| invalid("A mesh names no vertex buffer"))?;
        let header = read(manager, header_tag)?;
        let positions = read(manager, entry.reference)?;
        if crate::tag_payload::read_u16(&header, 4)? != 8
            || read_u32(&header, 0)? as usize != positions.len()
        {
            return Err(invalid("Gear vertices are not rigid 8-byte rows"));
        }
        let data_symbol = format!("{prefix}-mesh{index}-positions");
        let header_symbol = format!("{prefix}-mesh{index}-positions-header");
        nodes.push(Node::new(
            data_symbol.clone(),
            entry.reference,
            pin_to_root(&positions)?,
        ));
        let mut header_node = Node::new(header_symbol.clone(), header_tag, header);
        header_node.reference = Some(data_symbol);
        nodes.push(header_node);
        model.patch(mesh, header_symbol)?;
    }
    write_u32(&mut model.payload, BONE_PALETTE, 1)?;
    nodes.push(model);
    let mut owner_node = Node::new(owner_symbol.clone(), owner_tag, owner_bytes.clone());
    owner_node.patch(data + MODEL_SLOT, model_symbol)?;
    for offset in (0..owner_bytes.len().saturating_sub(3)).step_by(4) {
        if read_u32(&owner_bytes, offset)? == owner_tag {
            owner_node.patch(offset, owner_symbol.clone())?;
        }
    }
    nodes.push(owner_node);
    let mut entity_node = Node::new(entity_symbol.clone(), entity_tag, entity.clone());
    for slot in owner_slots(&entity, &owner_bytes, owner_tag)? {
        entity_node.patch(slot, owner_symbol.clone())?;
    }
    nodes.push(entity_node);
    let mut parent = Node::new(parent_symbol.clone(), relation_tag, relation);
    parent.patch(0x10, entity_symbol)?;
    nodes.push(parent);
    let mut companion = Node::new(format!("{prefix}-parent-companion"), 0, Vec::new());
    companion.companion = Some(Companion {
        shared_owner: parent_symbol,
        source_parent: relation_tag,
    });
    nodes.push(companion);
    Ok(())
}

/// Every rigid selector becomes bone 0. Weighted rows would need a real re-skin.
fn pin_to_root(positions: &[u8]) -> AuthoringResult<Vec<u8>> {
    if positions.len() % 8 != 0 {
        return Err(invalid("Gear vertex buffer is not a whole number of rows"));
    }
    let mut pinned = positions.to_vec();
    for row in pinned.chunks_exact_mut(8) {
        let selector = i16::from_le_bytes([row[6], row[7]]);
        if !(0..0x800).contains(&selector) {
            return Err(invalid("Gear vertices use weighted skinning"));
        }
        row[6..8].fill(0);
    }
    Ok(pinned)
}

/// Every word in the entity that names the model owner: its component row plus the typed
/// `(owner, class, offset)` resource pointers that must move with it.
fn owner_slots(entity: &[u8], owner: &[u8], owner_tag: u32) -> AuthoringResult<Vec<usize>> {
    let (count, _, rows, _) = array(entity, 0x10)?;
    let roots = (0..count)
        .map(|i| rows + i * 12)
        .filter(|&row| read_u32(entity, row).ok() == Some(owner_tag))
        .collect::<Vec<_>>();
    if roots.len() != 1 {
        return Err(invalid("Expected one primary model owner binding"));
    }
    let mut slots = Vec::new();
    for offset in (0..entity.len().saturating_sub(3)).step_by(4) {
        if read_u32(entity, offset)? != owner_tag {
            continue;
        }
        if !roots.contains(&offset) {
            let class = read_u32(entity, offset + 4)?;
            let target = crate::tag_payload::read_u64(entity, offset + 8)?;
            if class & 0xFFFF_0000 != 0x8080_0000
                || usize::try_from(target)
                    .ok()
                    .and_then(|target| target.checked_add(16))
                    .is_none_or(|end| end > owner.len())
            {
                return Err(invalid(format!(
                    "Owner occurrence at {offset:#x} is not a typed resource pointer"
                )));
            }
        }
        slots.push(offset);
    }
    Ok(slots)
}

/// A stable private assignment key that cannot collide with the sentinels.
fn private_key(item: u32, source: u32) -> u32 {
    let name = format!("parhelion/reskin/{item:08X}/{source:08X}");
    let mut hash = 0x811C_9DC5u32;
    for byte in name.bytes() {
        hash = hash.wrapping_mul(16_777_619) ^ u32::from(byte);
    }
    if matches!(hash, 0 | u32::MAX | 0x811C_9DC5) {
        hash ^= 0x10000;
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinning_keeps_positions_and_rejects_weighted_rows() {
        let rows = [1i16, 2, 3, 5, -4, 6, 7, 0]
            .into_iter()
            .flat_map(i16::to_le_bytes)
            .collect::<Vec<_>>();
        let pinned = pin_to_root(&rows).unwrap();
        assert_eq!(&pinned[..6], &rows[..6]);
        assert_eq!(&pinned[6..8], &[0, 0]);
        assert_eq!(&pinned[8..14], &rows[8..14]);
        assert_eq!(&pinned[14..16], &[0, 0]);
        let weighted = [0i16, 0, 0, 0x800]
            .into_iter()
            .flat_map(i16::to_le_bytes)
            .collect::<Vec<_>>();
        assert!(pin_to_root(&weighted).is_err());
        assert!(pin_to_root(&rows[..12]).is_err());
    }

    #[test]
    fn owner_slots_cover_the_component_row_and_typed_pointers_only() {
        let mut entity = vec![0u8; 192];
        entity[16..24].copy_from_slice(&1u64.to_le_bytes());
        entity[24..32].copy_from_slice(&56i64.to_le_bytes());
        entity[80..88].copy_from_slice(&1u64.to_le_bytes());
        entity[88..92].copy_from_slice(&0x80809C04u32.to_le_bytes());
        for offset in [96usize, 128, 160] {
            entity[offset..offset + 4].copy_from_slice(&0x80EC2727u32.to_le_bytes());
            if offset != 96 {
                entity[offset + 4..offset + 8].copy_from_slice(&0x808072B8u32.to_le_bytes());
                entity[offset + 8..offset + 16].copy_from_slice(&16u64.to_le_bytes());
            }
        }
        let owner = vec![0; 64];
        assert_eq!(
            owner_slots(&entity, &owner, 0x80EC2727).unwrap(),
            vec![96, 128, 160]
        );
        entity[132..136].fill(0);
        assert!(owner_slots(&entity, &owner, 0x80EC2727).is_err());
    }

    #[test]
    fn private_keys_are_stable_distinct_and_never_sentinels() {
        let a = private_key(0x1234_5678, 0xEDB8_4B17);
        assert_eq!(a, private_key(0x1234_5678, 0xEDB8_4B17));
        assert_ne!(a, private_key(0x1234_5678, 0x09DA_1AB0));
        assert_ne!(a, private_key(0x1234_5679, 0xEDB8_4B17));
        assert!(!matches!(a, 0 | u32::MAX | 0x811C_9DC5));
    }
}
