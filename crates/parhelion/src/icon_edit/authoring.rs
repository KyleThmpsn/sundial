//! Private package graph emission and native icon-layout validation.
use super::WeaponIconEdit;
use crate::{
    AuthoringResult, NewTagReference, NewTagReferenceOverride, NewTagSpec, NewTagStorageMode,
    appended_tags::AppendedTagAllocator,
    error::{input as invalid, validation},
    shared_tag_memory::SharedTagDependencies,
    tag_payload::{
        bounded_relative_target as relative_target, read_u16, read_u32, read_u64, write_u32,
    },
};
use std::collections::{BTreeMap, BTreeSet};
use sundial::package_authoring::PackageManager;
use sundial::package_authoring::{
    icon_schema::{
        ICON_DEFINITION_CLASS, ICON_DEFINITION_SIZE, ICON_LAYER_ARRAY_CLASS, ICON_LAYER_CLASS,
        ICON_LAYER_LANE_CLASS, ICON_LAYER_TEXTURE_CLASS, ICON_PRIMARY_LAYER_OFFSET,
        MAX_ICON_LAYER_LANES as MAX_LAYER_LANES,
        MAX_ICON_TEXTURES_PER_LANE as MAX_TEXTURES_PER_LANE,
    },
    is_valid_package_tag,
};
use tiger_pkg::TagHash;

const TEXTURE_HEADER_SIZE: usize = 40;

type Paint<'a> = dyn Fn(&mut [u8], usize, usize) -> AuthoringResult<()> + 'a;

/// Appended tags that privately reproduce one icon layer with its textures repainted.
#[derive(Clone, Debug)]
pub(crate) struct LayerRepaintPlan {
    pub layer_tag: TagHash,
    pub new_tags: Vec<NewTagSpec>,
    pub reference_overrides: Vec<NewTagReferenceOverride>,
    pub dependencies: SharedTagDependencies,
}

/// Authors a private primary-image graph for a non-identity edit.
pub(crate) fn build_weapon_icon_edit_plan(
    manager: &PackageManager,
    destination_package_id: u16,
    current_entry_count: usize,
    appended_ordinal_base: usize,
    donor_container_tag: TagHash,
    edit: &WeaponIconEdit,
) -> AuthoringResult<Option<LayerRepaintPlan>> {
    edit.validate()?;
    if edit.is_identity() {
        return Ok(None);
    }
    let layer = read_primary_layer_tag(manager, donor_container_tag)?;
    build_layer_repaint_plan(
        manager,
        destination_package_id,
        current_entry_count,
        appended_ordinal_base,
        (layer, donor_container_tag),
        &|pixels, width, height| edit.apply_to_rgba8_sized(pixels, width, height),
    )
    .map(Some)
}

/// Authors a private copy of one icon layer with every texture repainted by `paint`, which gets
/// each texture's tightly packed RGBA8 pixels and size.
///
/// `appended_ordinal_base` is an absolute ordinal in the caller's complete appended-tag slice.
/// Every unique donor texture header/data pair is cloned once, even when several layer lanes refer
/// to it. The cloned layer is appended after all reciprocal data/header pairs.
pub(crate) fn build_layer_repaint_plan(
    manager: &PackageManager,
    destination_package_id: u16,
    current_entry_count: usize,
    appended_ordinal_base: usize,
    layer: (TagHash, TagHash),
    paint: &Paint<'_>,
) -> AuthoringResult<LayerRepaintPlan> {
    build_layer_texture_plan(
        manager,
        destination_package_id,
        current_entry_count,
        appended_ordinal_base,
        layer,
        &|_| TextureChange::Paint { group: 0, paint },
    )
    .map(|(plan, _)| plan)
}

/// What a private layer copy does with one of its textures, by the texture's place among the
/// layer's texture references.
pub(crate) enum TextureChange<'a> {
    /// The layer's own texture.
    Keep,
    /// Another stock texture header in its place.
    Use(TagHash),
    /// A copy of the layer's own texture, repainted. References to one header in one group share
    /// a single copy.
    Paint { group: usize, paint: &'a Paint<'a> },
}

/// Authors a private copy of one icon layer with each texture changed as `change` says. Returns
/// the plan and the stock texture headers and data the copy still names, which whatever loads the
/// layer must keep resident.
pub(crate) fn build_layer_texture_plan<'a>(
    manager: &PackageManager,
    destination_package_id: u16,
    current_entry_count: usize,
    appended_ordinal_base: usize,
    (donor_layer_tag, donor_container_tag): (TagHash, TagHash),
    change: &dyn Fn(usize) -> TextureChange<'a>,
) -> AuthoringResult<(LayerRepaintPlan, BTreeSet<u32>)> {
    let mut donor_layer = read_icon_layer(manager, donor_layer_tag, donor_container_tag)?;
    let references = texture_reference_offsets(&donor_layer, donor_layer_tag)?;

    let mut new_tags = Vec::new();
    let mut reference_overrides = Vec::new();
    let mut authored_headers = BTreeMap::<(u32, usize), TagHash>::new();
    let mut dependencies = BTreeSet::new();
    let mut stock = BTreeSet::new();
    let mut targets = Vec::with_capacity(references.len());

    for (index, (_, donor_header_tag)) in references.iter().enumerate() {
        let (group, paint) = match change(index) {
            TextureChange::Keep => {
                stock.extend(stock_texture(manager, *donor_header_tag)?);
                targets.push(*donor_header_tag);
                continue;
            }
            TextureChange::Use(header) => {
                stock.extend(stock_texture(manager, header)?);
                targets.push(header);
                continue;
            }
            TextureChange::Paint { group, paint } => (group, paint),
        };
        if let Some(header) = authored_headers.get(&(u32::from(*donor_header_tag), group)) {
            targets.push(*header);
            continue;
        }
        let (donor_data_tag, mut data, header) =
            read_rgba8_texture_pair(manager, donor_layer_tag, *donor_header_tag)?;
        let width = usize::from(read_u16(&header, 0x0E)?);
        let height = usize::from(read_u16(&header, 0x10)?);
        paint(data.as_mut_slice(), width, height)?;

        let data_ordinal = AppendedTagAllocator::checked_ordinal(
            appended_ordinal_base,
            new_tags.len(),
            "icon texture data",
        )?;
        let header_ordinal = AppendedTagAllocator::checked_ordinal(
            appended_ordinal_base,
            new_tags.len() + 1,
            "icon texture header",
        )?;
        let data_tag = assigned_tag(
            destination_package_id,
            current_entry_count,
            data_ordinal,
            "icon texture data",
        )?;
        let header_tag = assigned_tag(
            destination_package_id,
            current_entry_count,
            header_ordinal,
            "icon texture header",
        )?;
        new_tags.push(NewTagSpec {
            template_tag: donor_data_tag,
            payload: data,
            storage: NewTagStorageMode::InheritTemplate,
        });
        new_tags.push(NewTagSpec {
            template_tag: *donor_header_tag,
            payload: header,
            storage: NewTagStorageMode::InheritTemplate,
        });
        reference_overrides.push(NewTagReferenceOverride {
            new_tag_ordinal: data_ordinal,
            reference: NewTagReference::Appended(header_ordinal),
        });
        reference_overrides.push(NewTagReferenceOverride {
            new_tag_ordinal: header_ordinal,
            reference: NewTagReference::Appended(data_ordinal),
        });
        dependencies.insert(u32::from(data_tag));
        dependencies.insert(u32::from(header_tag));
        authored_headers.insert((u32::from(*donor_header_tag), group), header_tag);
        targets.push(header_tag);
    }

    let patched_offsets = references
        .iter()
        .zip(&targets)
        .map(|((offset, _), target)| {
            write_tag(&mut donor_layer, *offset, *target)?;
            Ok(*offset)
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    validate_layer_patch(manager, donor_layer_tag, &donor_layer, &patched_offsets)?;

    let layer_ordinal =
        AppendedTagAllocator::checked_ordinal(appended_ordinal_base, new_tags.len(), "icon layer")?;
    let layer_tag = assigned_tag(
        destination_package_id,
        current_entry_count,
        layer_ordinal,
        "icon layer",
    )?;
    dependencies.insert(u32::from(layer_tag));
    new_tags.push(NewTagSpec {
        template_tag: donor_layer_tag,
        payload: donor_layer,
        storage: NewTagStorageMode::InheritTemplate,
    });

    validate_plan(
        &new_tags,
        &reference_overrides,
        appended_ordinal_base,
        layer_tag,
        &dependencies,
    )?;
    Ok((
        LayerRepaintPlan {
            layer_tag,
            new_tags,
            reference_overrides,
            dependencies,
        },
        stock,
    ))
}

/// A stock texture header and the data it names, checked as a texture pair.
fn stock_texture(manager: &PackageManager, header: TagHash) -> AuthoringResult<[u32; 2]> {
    let entry = manager
        .get_entry(header)
        .ok_or_else(|| invalid(format!("Texture header {header} has no package entry")))?;
    if entry.file_type != 0x20 || !is_valid_package_tag(TagHash(entry.reference)) {
        return Err(invalid(format!("{header} is not a texture header")));
    }
    Ok([u32::from(header), entry.reference])
}

/// A complete Shadowkeep icon definition, checked against its package entry.
pub(crate) fn read_icon_container(
    manager: &PackageManager,
    container_tag: TagHash,
) -> AuthoringResult<Vec<u8>> {
    let entry = manager.get_entry(container_tag).ok_or_else(|| {
        invalid(format!(
            "Icon definition {container_tag} has no package entry"
        ))
    })?;
    if entry.file_size as usize != ICON_DEFINITION_SIZE
        || entry.file_type != 0x10
        || entry.file_subtype != 0x00
        || entry.reference != ICON_DEFINITION_CLASS
    {
        return Err(invalid(format!(
            "Icon definition {container_tag} has size/type/reference {}/{:02X}/{:02X}/0x{:08X}; expected {ICON_DEFINITION_SIZE}/10/00/0x{ICON_DEFINITION_CLASS:08X}",
            entry.file_size, entry.file_type, entry.file_subtype, entry.reference
        )));
    }
    let payload = manager.read_tag(container_tag).map_err(|error| {
        invalid(format!(
            "Could not read icon definition {container_tag}: {error}"
        ))
    })?;
    if payload.len() != ICON_DEFINITION_SIZE
        || read_u32(&payload, 0)? as usize != ICON_DEFINITION_SIZE
    {
        return Err(invalid(format!(
            "Icon definition {container_tag} is not a complete Shadowkeep icon definition"
        )));
    }
    Ok(payload)
}

pub(crate) fn read_primary_layer_tag(
    manager: &PackageManager,
    container_tag: TagHash,
) -> AuthoringResult<TagHash> {
    let payload = read_icon_container(manager, container_tag)?;
    let primary_layer_tag = read_tag(&payload, ICON_PRIMARY_LAYER_OFFSET)?;
    if !is_valid_package_tag(primary_layer_tag) {
        return Err(invalid(format!(
            "Weapon icon donor definition {container_tag} has malformed primary-layer reference 0x{:08X}",
            u32::from(primary_layer_tag)
        )));
    }
    Ok(primary_layer_tag)
}

pub(crate) fn read_icon_layer(
    manager: &PackageManager,
    layer_tag: TagHash,
    container_tag: TagHash,
) -> AuthoringResult<Vec<u8>> {
    let entry = manager.get_entry(layer_tag).ok_or_else(|| {
        invalid(format!(
            "Weapon icon donor {container_tag} primary layer {layer_tag} has no package entry"
        ))
    })?;
    if entry.file_type != 0x08 || entry.file_subtype != 0x00 || entry.reference != ICON_LAYER_CLASS
    {
        return Err(invalid(format!(
            "Weapon icon primary layer {layer_tag} has type/reference {:02X}/{:02X}/0x{:08X}; expected 08/00/0x{ICON_LAYER_CLASS:08X}",
            entry.file_type, entry.file_subtype, entry.reference
        )));
    }
    let payload = manager.read_tag(layer_tag).map_err(|error| {
        invalid(format!(
            "Could not read weapon icon primary layer {layer_tag}: {error}"
        ))
    })?;
    if payload.len() != entry.file_size as usize || read_u32(&payload, 0)? as usize != payload.len()
    {
        return Err(invalid(format!(
            "Weapon icon primary layer {layer_tag} decoded to {} bytes while its entry/payload declares {}/{}",
            payload.len(),
            entry.file_size,
            read_u32(&payload, 0)?
        )));
    }
    Ok(payload)
}

pub(crate) fn texture_reference_offsets(
    layer: &[u8],
    layer_tag: TagHash,
) -> AuthoringResult<Vec<(usize, TagHash)>> {
    let description = format!("weapon icon primary layer {layer_tag}");
    let lane_count = usize::try_from(read_u64(layer, 0x20)?)
        .map_err(|_| invalid(format!("{description} lane count exceeds this platform")))?;
    if lane_count == 0 || lane_count > MAX_LAYER_LANES {
        return Err(invalid(format!(
            "{description} has invalid lane count {lane_count}"
        )));
    }
    let lanes = relative_target(layer, 0x28, &description)?;
    if lanes < 4
        || read_u32(layer, lanes - 4)? != ICON_LAYER_ARRAY_CLASS
        || usize::try_from(read_u64(layer, lanes)?).ok() != Some(lane_count)
        || read_u32(layer, lanes + 8)? != ICON_LAYER_LANE_CLASS
    {
        return Err(invalid(format!("{description} has an invalid lane array")));
    }

    let descriptors = checked_add(lanes, 0x10, &description)?;
    let mut references = Vec::new();
    for lane_index in 0..lane_count {
        let descriptor = checked_add(
            descriptors,
            lane_index
                .checked_mul(0x10)
                .ok_or_else(|| invalid(format!("{description} lane offset overflowed")))?,
            &description,
        )?;
        let texture_count = usize::try_from(read_u64(layer, descriptor)?).map_err(|_| {
            invalid(format!(
                "{description} lane {lane_index} texture count exceeds this platform"
            ))
        })?;
        if texture_count == 0 || texture_count > MAX_TEXTURES_PER_LANE {
            return Err(invalid(format!(
                "{description} lane {lane_index} has invalid texture count {texture_count}"
            )));
        }
        let textures = relative_target(layer, descriptor + 8, &description)?;
        if textures < 4
            || read_u32(layer, textures - 4)? != ICON_LAYER_ARRAY_CLASS
            || usize::try_from(read_u64(layer, textures)?).ok() != Some(texture_count)
            || read_u32(layer, textures + 8)? != ICON_LAYER_TEXTURE_CLASS
        {
            return Err(invalid(format!(
                "{description} lane {lane_index} has an invalid texture array"
            )));
        }
        let tags = checked_add(textures, 0x10, &description)?;
        for texture_index in 0..texture_count {
            let offset = checked_add(
                tags,
                texture_index
                    .checked_mul(4)
                    .ok_or_else(|| invalid(format!("{description} texture offset overflowed")))?,
                &description,
            )?;
            let header_tag = read_tag(layer, offset)?;
            if !is_valid_package_tag(header_tag) {
                return Err(invalid(format!(
                    "{description} lane {lane_index} texture {texture_index} has malformed header reference 0x{:08X}",
                    u32::from(header_tag)
                )));
            }
            references.push((offset, header_tag));
        }
    }
    Ok(references)
}

pub(super) fn read_rgba8_texture_pair(
    manager: &PackageManager,
    layer_tag: TagHash,
    header_tag: TagHash,
) -> AuthoringResult<(TagHash, Vec<u8>, Vec<u8>)> {
    let header_entry = manager.get_entry(header_tag).ok_or_else(|| {
        invalid(format!(
            "Weapon icon layer {layer_tag} texture header {header_tag} has no package entry"
        ))
    })?;
    if header_entry.file_size as usize != TEXTURE_HEADER_SIZE
        || header_entry.file_type != 0x20
        || header_entry.file_subtype != 0x01
    {
        return Err(invalid(format!(
            "Weapon icon texture header {header_tag} has size/type {}/{:02X}/{:02X}; expected {TEXTURE_HEADER_SIZE}/20/01",
            header_entry.file_size, header_entry.file_type, header_entry.file_subtype
        )));
    }
    let data_tag = TagHash(header_entry.reference);
    if !is_valid_package_tag(data_tag) {
        return Err(invalid(format!(
            "Weapon icon texture header {header_tag} has malformed data reference 0x{:08X}",
            header_entry.reference
        )));
    }
    let header = manager.read_tag(header_tag).map_err(|error| {
        invalid(format!(
            "Could not read weapon icon texture header {header_tag}: {error}"
        ))
    })?;
    if header.len() != TEXTURE_HEADER_SIZE {
        return Err(invalid(format!(
            "Weapon icon texture header {header_tag} decoded to {} bytes; expected {TEXTURE_HEADER_SIZE}",
            header.len()
        )));
    }
    let format = read_u32(&header, 4)?;
    let width = usize::from(read_u16(&header, 14)?);
    let height = usize::from(read_u16(&header, 16)?);
    let declared_size = read_u32(&header, 0)? as usize;
    if !matches!(format, 28 | 29) {
        return Err(invalid(format!(
            "Weapon icon texture header {header_tag} uses unsupported DXGI format {format}; only RGBA8 formats 28 and 29 can be edited"
        )));
    }
    if read_u16(&header, 12)? != 0xCAFE || width == 0 || height == 0 || declared_size == 0 {
        return Err(invalid(format!(
            "Weapon icon texture header {header_tag} has malformed dimensions or resource marker"
        )));
    }

    let data_entry = manager.get_entry(data_tag).ok_or_else(|| {
        invalid(format!(
            "Weapon icon texture header {header_tag} data {data_tag} has no package entry"
        ))
    })?;
    if data_entry.file_size as usize != declared_size
        || data_entry.file_type != 0x28
        || data_entry.file_subtype != 0x01
        || data_entry.reference != u32::from(header_tag)
    {
        return Err(invalid(format!(
            "Weapon icon texture data {data_tag} has size/type/reference {}/{:02X}/{:02X}/0x{:08X}; expected {declared_size}/28/01/{header_tag}",
            data_entry.file_size,
            data_entry.file_type,
            data_entry.file_subtype,
            data_entry.reference
        )));
    }
    let data = manager.read_tag(data_tag).map_err(|error| {
        invalid(format!(
            "Could not read weapon icon texture data {data_tag}: {error}"
        ))
    })?;
    let base_level_size = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            invalid(format!(
                "Weapon icon texture {header_tag} dimensions overflowed"
            ))
        })?;
    if data.len() != declared_size || data.len() != base_level_size {
        return Err(invalid(format!(
            "Weapon icon texture data {data_tag} has RGBA8 length {}; declared {declared_size}, but safe editing requires exactly one tightly packed {width}x{height} surface ({base_level_size} bytes)",
            data.len()
        )));
    }
    Ok((data_tag, data, header))
}

fn validate_layer_patch(
    manager: &PackageManager,
    donor_layer_tag: TagHash,
    authored: &[u8],
    patched_offsets: &[usize],
) -> AuthoringResult<()> {
    let donor = manager.read_tag(donor_layer_tag).map_err(|error| {
        invalid(format!(
            "Could not reread weapon icon primary layer {donor_layer_tag}: {error}"
        ))
    })?;
    if donor.len() != authored.len()
        || donor
            .iter()
            .zip(authored)
            .enumerate()
            .any(|(offset, (before, after))| {
                before != after
                    && !patched_offsets
                        .iter()
                        .any(|field| (*field..*field + 4).contains(&offset))
            })
    {
        return Err(validation(
            "Authored weapon icon primary layer changed bytes outside texture references",
        ));
    }
    Ok(())
}

fn validate_plan(
    new_tags: &[NewTagSpec],
    overrides: &[NewTagReferenceOverride],
    ordinal_base: usize,
    layer_tag: TagHash,
    dependencies: &SharedTagDependencies,
) -> AuthoringResult<()> {
    if new_tags.len() % 2 != 1 || overrides.len() + 1 != new_tags.len() {
        return Err(validation(
            "Weapon icon edit append plan has an invalid data/header/layer shape",
        ));
    }
    let pair_count = (new_tags.len() - 1) / 2;
    for pair_index in 0..pair_count {
        let data_ordinal =
            AppendedTagAllocator::checked_ordinal(ordinal_base, pair_index * 2, "texture data")?;
        let header_ordinal = AppendedTagAllocator::checked_ordinal(
            ordinal_base,
            pair_index * 2 + 1,
            "texture header",
        )?;
        if new_tags[pair_index * 2].storage != NewTagStorageMode::InheritTemplate
            || new_tags[pair_index * 2 + 1].storage != NewTagStorageMode::InheritTemplate
            || overrides.get(pair_index * 2)
                != Some(&NewTagReferenceOverride {
                    new_tag_ordinal: data_ordinal,
                    reference: NewTagReference::Appended(header_ordinal),
                })
            || overrides.get(pair_index * 2 + 1)
                != Some(&NewTagReferenceOverride {
                    new_tag_ordinal: header_ordinal,
                    reference: NewTagReference::Appended(data_ordinal),
                })
        {
            return Err(validation(
                "Weapon icon edit texture references are not reciprocal",
            ));
        }
    }
    if new_tags.last().is_none_or(|layer| {
        layer.storage != NewTagStorageMode::InheritTemplate
            || read_u32(&layer.payload, 0).ok().map(|size| size as usize)
                != Some(layer.payload.len())
    }) || !dependencies.contains(&u32::from(layer_tag))
        || dependencies.len() != new_tags.len()
    {
        return Err(validation(
            "Weapon icon edit append plan failed dependency or layer validation",
        ));
    }
    Ok(())
}

fn assigned_tag(
    package_id: u16,
    current_entry_count: usize,
    ordinal: usize,
    description: &str,
) -> AuthoringResult<TagHash> {
    AppendedTagAllocator::new(package_id, current_entry_count).assigned_tag(
        ordinal,
        &format!("{description} destination"),
        description,
    )
}

fn checked_add(base: usize, relative: usize, description: &str) -> AuthoringResult<usize> {
    base.checked_add(relative)
        .ok_or_else(|| invalid(format!("{description} offset overflowed")))
}

pub(super) fn read_tag(data: &[u8], offset: usize) -> AuthoringResult<TagHash> {
    Ok(TagHash(read_u32(data, offset)?))
}

fn write_tag(data: &mut [u8], offset: usize, tag: TagHash) -> AuthoringResult<()> {
    write_u32(data, offset, u32::from(tag))
}
