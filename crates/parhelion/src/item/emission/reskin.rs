//! Private copies of an appearance's gear parts, for two reasons.
//!
//! Cross-family appearances pin every gear part to the runtime rig's root bone. Gear parts are
//! rigid meshes whose 8-byte vertex rows carry a bone index into the family's weapon skeleton
//! (auto rifles have four bones, hand cannons eight). A hand cannon part on an auto rifle runtime
//! therefore names bones that do not exist. The private copies made here select bone 0
//! everywhere, so each part rides the grip and every family's clips resolve. Moving parts stay
//! still, and nothing dereferences a missing bone.
//!
//! Moved markers copy the parts whose marker sets carry them, with the rows moved. A weapon moved
//! in the hand copies every part: its model's position offset and every marker move together, so
//! the sights stay on the model while the model moves away from the handle the hand holds. The
//! model is otherwise copied only when it is pinned.
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
/// The model's position offset. A stored position is its quantized value times the scale at
/// `+0x50` plus this, so adding to it moves every vertex of the model.
const POSITION_OFFSET: usize = 0x60;

pub(super) fn apply(
    directory: &Path,
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    weapons: &[WeaponCloneSpec],
    replacements: &mut Vec<ReplacementSpec>,
) -> AuthoringResult<()> {
    let mut companions = Companions::new();
    for spec in weapons {
        // An imported model's parts are private already, and its own pass moves their markers.
        #[cfg(feature = "d2-model-importer")]
        if spec.overrides.imported_graph.is_some() {
            if spec.overrides.shader_glow {
                return Err(invalid(
                    "Shader Glow currently requires a native weapon appearance",
                ));
            }
            if spec.overrides.held_offset.is_some() {
                return Err(invalid(format!(
                    "Weapon {:?}: an imported model keeps its own place in the hand",
                    spec.text.name
                )));
            }
            continue;
        }
        let offsets = spec.overrides.marker_offsets.as_slice();
        let shift = spec.overrides.held_offset;
        let glow = spec.overrides.shader_glow;
        if spec.presentation_donor.is_none() && offsets.is_empty() && shift.is_none() && !glow {
            continue;
        }
        let item = spec.identity.item_hash;
        let ordinal = definition_ordinal(emission, item)?;
        let pin = match &spec.presentation_donor {
            Some(presentation) => {
                let authored = group(
                    &emission.sandbox_patterns,
                    weapon_pattern_index(&emission.host_new_tags[ordinal].payload)?,
                )?;
                let donor_definition = stock_definition(emission, manager, presentation.item_hash)?;
                let donor = group(
                    &emission.sandbox_patterns,
                    weapon_pattern_index(&donor_definition)?,
                )?;
                matches!((authored, donor), (Some(authored), Some(donor)) if authored != donor)
            }
            None => false,
        };
        if !pin && offsets.is_empty() && shift.is_none() && !glow {
            continue;
        }
        private_parts(
            directory,
            emission,
            manager,
            &mut companions,
            replacements,
            item,
            ordinal,
            Copies {
                pin,
                offsets,
                shift,
                glow,
            },
        )
        .map_err(|error| {
            error.context(match &spec.presentation_donor {
                Some(presentation) if pin => format!(
                    "Weapon {:?}: pinning appearance 0x{:08X} to the runtime rig",
                    spec.text.name, presentation.item_hash
                ),
                _ => format!(
                    "Weapon {:?}: authoring private appearance resources",
                    spec.text.name
                ),
            })
        })?;
    }
    Ok(())
}

/// What a part's private copy changes: its vertices pinned to the root bone, its markers moved,
/// the whole part moved in the hand, or any of them together.
#[derive(Clone, Copy)]
struct Copies<'a> {
    pin: bool,
    offsets: &'a [(u32, [f32; 3])],
    /// How far the model and every marker move, in metres along the model's own axes.
    shift: Option<[f32; 3]>,
    glow: bool,
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

pub(super) fn stock_definition(
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

#[allow(clippy::too_many_arguments)]
fn private_parts(
    directory: &Path,
    emission: &mut PackageEmission,
    manager: &sundial::package_authoring::PackageManager,
    companions: &mut Companions,
    replacements: &mut Vec<ReplacementSpec>,
    item: u32,
    ordinal: usize,
    copies: Copies<'_>,
) -> AuthoringResult<()> {
    let art_rows = weapon_art_arrangements(&emission.host_new_tags[ordinal].payload)?;
    let indices = art_rows
        .iter()
        .map(|row| row.arrangement)
        .collect::<BTreeSet<_>>();
    let [donor_row] = indices.into_iter().collect::<Vec<_>>()[..] else {
        return Err(invalid(if copies.pin {
            "A cross-family appearance must use one gear-art row for every class"
        } else {
            "Moving markers or the model needs one gear-art row for every class"
        }));
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
    for key in &keys {
        let relation_tag = *map
            .get(key)
            .ok_or_else(|| invalid(format!("Gear-art key 0x{key:08X} has no assignment")))?;
        // An empty part names no entity and draws nothing, so its stock key can stay.
        if read_u32(&read(manager, relation_tag)?, 0x10)? == u32::MAX {
            continue;
        }
        let prefix = format!("part{}", entries.len());
        if !part_nodes(manager, &prefix, relation_tag, &mut nodes, copies)? {
            continue;
        }
        let private = private_key(item, *key);
        entries.push((private, format!("{prefix}-parent")));
        substitutions.insert(*key, private);
    }
    if entries.is_empty() {
        return Ok(());
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
    Ok(())
}

/// Private copies of one gear part: the entity and the relation that the assignment map names,
/// with its cloned loading companion, then the model and model owner when it is pinned or moved,
/// and each marker set whose markers move. A part that does not change stays stock, and `false`
/// says so without adding a node.
fn part_nodes(
    manager: &sundial::package_authoring::PackageManager,
    prefix: &str,
    relation_tag: u32,
    nodes: &mut Vec<Node>,
    copies: Copies<'_>,
) -> AuthoringResult<bool> {
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
    // Each marker set with a moved marker: its tag, its stock bytes and its moved bytes.
    let mut markers = Vec::new();
    for i in 0..count {
        let tag = read_u32(&entity, rows + i * 12)?;
        if manager
            .get_entry(TagHash(tag))
            .is_none_or(|entry| entry.reference != RESOURCE_CLASS)
        {
            continue;
        }
        let bytes = read(manager, tag)?;
        if (!copies.offsets.is_empty() || copies.shift.is_some())
            && sundial::package_authoring::gear_markers::is_marker_set(&bytes)
        {
            use sundial::package_authoring::gear_markers::{offset_markers, shift_markers};
            let mut moved = bytes.clone();
            let mut rows = offset_markers(&mut moved, copies.offsets)
                .map_err(|error| invalid(format!("Marker set 0x{tag:08X}: {error}")))?;
            if let Some(shift) = copies.shift {
                rows += shift_markers(&mut moved, shift)
                    .map_err(|error| invalid(format!("Marker set 0x{tag:08X}: {error}")))?;
            }
            if rows > 0 && !markers.iter().any(|(seen, _, _)| *seen == tag) {
                markers.push((tag, bytes, moved));
            }
            continue;
        }
        let header = relative_target(&bytes, 0x10)?;
        if header >= 4
            && read_u32(&bytes, header - 4)? == MODEL_OWNER_HEADER
            && owner.replace((tag, bytes)).is_some()
        {
            return Err(invalid("A gear part owns several models"));
        }
    }
    if !copies.pin && copies.shift.is_none() && markers.is_empty() && !copies.glow {
        return Ok(false);
    }
    let entity_symbol = format!("{prefix}-entity");
    let parent_symbol = format!("{prefix}-parent");
    let mut entity_node = Node::new(entity_symbol.clone(), entity_tag, entity.clone());
    for (index, (tag, stock, moved)) in markers.into_iter().enumerate() {
        let symbol = format!("{prefix}-markers{index}");
        let mut node = Node::new(symbol.clone(), tag, moved);
        for offset in (0..stock.len().saturating_sub(3)).step_by(4) {
            if read_u32(&stock, offset)? == tag {
                node.patch(offset, symbol.clone())?;
            }
        }
        for slot in owner_slots(&entity, &stock, tag)? {
            entity_node.patch(slot, symbol.clone())?;
        }
        nodes.push(node);
    }
    // A pinned part needs its model. A moved one moves the model it has, and a part with only
    // markers has nothing more to move.
    let owner = match owner {
        None if copies.pin => return Err(invalid("A gear part owns no model")),
        owner if copies.pin || copies.shift.is_some() || copies.glow => owner,
        _ => None,
    };
    if let Some((owner_tag, owner_bytes)) = owner {
        let owner_symbol = model_nodes(manager, prefix, owner_tag, &owner_bytes, nodes, copies)?;
        for slot in owner_slots(&entity, &owner_bytes, owner_tag)? {
            entity_node.patch(slot, owner_symbol.clone())?;
        }
    }
    nodes.push(entity_node);
    let mut parent = Node::new(parent_symbol.clone(), relation_tag, relation);
    parent.patch(0x10, entity_symbol)?;
    nodes.push(parent);
    let mut companion = Node::new(format!("{prefix}-parent-companion"), 0, Vec::new());
    companion.companion = Some(Companion::new(parent_symbol, relation_tag));
    nodes.push(companion);
    Ok(true)
}

/// A part's private model owner and model, with pinned position buffers when it is pinned and a
/// moved position offset when it moves in the hand. Returns the owner's symbol.
fn model_nodes(
    manager: &sundial::package_authoring::PackageManager,
    prefix: &str,
    owner_tag: u32,
    owner_bytes: &[u8],
    nodes: &mut Vec<Node>,
    copies: Copies<'_>,
) -> AuthoringResult<String> {
    let data = relative_target(owner_bytes, 0x18)?;
    if data < 4 || read_u32(owner_bytes, data - 4)? != MODEL_OWNER_DATA {
        return Err(invalid(
            "The gear part's model owner has an unsupported layout",
        ));
    }
    let model_tag = read_u32(owner_bytes, data + MODEL_SLOT)?;
    if manager
        .get_entry(TagHash(model_tag))
        .is_none_or(|entry| entry.reference != MODEL_CLASS)
    {
        return Err(invalid("The gear part's model slot does not name a model"));
    }
    let model_bytes = read(manager, model_tag)?;
    let model_symbol = format!("{prefix}-model");
    let owner_symbol = format!("{prefix}-owner");
    let mut model = Node::new(model_symbol.clone(), model_tag, model_bytes.clone());
    let (meshes, _, mesh_rows, class) = array(&model_bytes, 0x10)?;
    if class != MESH_ROW_CLASS || meshes == 0 {
        return Err(invalid(
            "The gear part's model has an unsupported mesh table",
        ));
    }
    if let Some(shift) = copies.shift {
        for (axis, delta) in shift.into_iter().enumerate() {
            let at = POSITION_OFFSET + axis * 4;
            let value = f32::from_bits(read_u32(&model.payload, at)?) + delta;
            if !value.is_finite() {
                return Err(invalid("The moved model has an unusable position offset"));
            }
            write_u32(&mut model.payload, at, value.to_bits())?;
        }
    }
    for index in (0..meshes).filter(|_| copies.pin) {
        let mesh = mesh_rows + index * 0x88;
        let header_tag = read_u32(&model_bytes, mesh)?;
        let entry = manager
            .get_entry(TagHash(header_tag))
            .filter(|entry| entry.file_type == 32 && entry.file_subtype == 4)
            .ok_or_else(|| invalid("A mesh names no vertex buffer"))?;
        let header = read(manager, header_tag)?;
        let positions = read(manager, entry.reference)?;
        if read_u32(&header, 0)? as usize != positions.len() {
            return Err(invalid("A gear vertex header does not match its buffer"));
        }
        let pinned = pin_to_root(&positions, crate::tag_payload::read_u16(&header, 4)?)
            .map_err(|error| error.context(format!("Model 0x{model_tag:08X} mesh {index}")))?;
        let data_symbol = format!("{prefix}-mesh{index}-positions");
        let header_symbol = format!("{prefix}-mesh{index}-positions-header");
        nodes.push(Node::new(data_symbol.clone(), entry.reference, pinned));
        let mut header_node = Node::new(header_symbol.clone(), header_tag, header);
        header_node.reference = Some(data_symbol);
        nodes.push(header_node);
        model.patch(mesh, header_symbol)?;
    }
    if copies.pin {
        write_u32(&mut model.payload, BONE_PALETTE, 1)?;
    }
    if copies.glow {
        super::glow::model(manager, prefix, &mut model, nodes)?;
    }
    nodes.push(model);
    let mut owner_node = Node::new(owner_symbol.clone(), owner_tag, owner_bytes.to_vec());
    owner_node.patch(data + MODEL_SLOT, model_symbol)?;
    for offset in (0..owner_bytes.len().saturating_sub(3)).step_by(4) {
        if read_u32(owner_bytes, offset)? == owner_tag {
            owner_node.patch(offset, owner_symbol.clone())?;
        }
    }
    nodes.push(owner_node);
    Ok(owner_symbol)
}

/// Every vertex is bound to bone 0 alone. The selector word at bytes 6..8 names one bone when
/// it is below 0x800. At 0x7FFF the row blends bones instead: a 12-byte row holds two bone
/// indices and then two weights, a 16-byte row four weights and then four bone indices. Weights
/// sum to 255 and an unused slot names bone 254. Pinned blends take the native one-bone form.
fn pin_to_root(positions: &[u8], stride: u16) -> AuthoringResult<Vec<u8>> {
    let stride = usize::from(stride);
    let (bones, weights) = match stride {
        8 => (0..0, 0..0),
        12 => (8..10, 10..12),
        16 => (12..16, 8..12),
        _ => {
            return Err(invalid(format!(
                "Gear vertices use an unsupported {stride}-byte layout"
            )));
        }
    };
    if positions.is_empty() || positions.len() % stride != 0 {
        return Err(invalid("Gear vertex buffer is not a whole number of rows"));
    }
    let mut pinned = positions.to_vec();
    for row in pinned.chunks_exact_mut(stride) {
        match i16::from_le_bytes([row[6], row[7]]) {
            0..0x800 => row[6..8].fill(0),
            0x7FFF if !bones.is_empty() => {
                let total = row[weights.clone()]
                    .iter()
                    .map(|&w| u32::from(w))
                    .sum::<u32>();
                if total != 255 {
                    return Err(invalid("Gear vertex weights do not sum to 255"));
                }
                row[bones.clone()].fill(0xFE);
                row[bones.start] = 0;
                row[weights.clone()].fill(0);
                row[weights.start] = 0xFF;
            }
            _ => return Err(invalid("Gear vertices use an unsupported bone selector")),
        }
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
        let pinned = pin_to_root(&rows, 8).unwrap();
        assert_eq!(&pinned[..6], &rows[..6]);
        assert_eq!(&pinned[6..8], &[0, 0]);
        assert_eq!(&pinned[8..14], &rows[8..14]);
        assert_eq!(&pinned[14..16], &[0, 0]);
        let weighted = [0i16, 0, 0, 0x800]
            .into_iter()
            .flat_map(i16::to_le_bytes)
            .collect::<Vec<_>>();
        assert!(pin_to_root(&weighted, 8).is_err());
        assert!(pin_to_root(&rows[..12], 8).is_err());
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
}
