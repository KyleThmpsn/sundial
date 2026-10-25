//! Reading and validating the donor icon graph before a watermark plan is built.
use super::*;

pub(super) fn read_and_validate_watermark_donor(
    manager: &PackageManager,
) -> AuthoringResult<Vec<u8>> {
    let layer = read_typed_tag(
        manager,
        DONOR_WATERMARK_LAYER,
        WATERMARK_LAYER_SIZE,
        0x08,
        0x00,
        "stock watermark template layer",
    )?;
    validate_entry_reference(
        manager,
        DONOR_WATERMARK_LAYER,
        WATERMARK_CLASS_HANDLE,
        "stock watermark template layer",
    )?;
    validate_layer_texture_graph(
        manager,
        DONOR_WATERMARK_LAYER,
        &layer,
        "stock watermark template",
    )?;
    if read_u64(&layer, 0x20)? != 1
        || read_i64(&layer, 0x28)? != 0x18
        || read_u32(&layer, 0x3C)? != 0x8080_9FBD
        || read_u64(&layer, 0x40)? != 1
        || read_u32(&layer, 0x48)? != 0x8080_4A6C
        || read_u64(&layer, 0x50)? != WATERMARK_TEXTURE_REFERENCE_COUNT as u64
        || read_i64(&layer, 0x58)? != 0x18
        || read_u32(&layer, 0x6C)? != 0x8080_9FBD
        || read_u64(&layer, 0x70)? != WATERMARK_TEXTURE_REFERENCE_COUNT as u64
        || read_u32(&layer, 0x78)? != 0x8080_4A6F
    {
        return Err(invalid(
            "Stock watermark template no longer has one lane with six texture variants",
        ));
    }
    for (index, expected) in DONOR_TEXTURE_HEADERS.iter().copied().enumerate() {
        if read_tag(&layer, WATERMARK_TEXTURE_REFERENCE_START + index * 4)? != expected {
            return Err(invalid(format!(
                "Stock watermark template slot {index} no longer references {expected}"
            )));
        }
    }
    Ok(layer)
}

pub(super) fn read_and_validate_texture_header(
    manager: &PackageManager,
    header_tag: TagHash,
    data_tag: TagHash,
    width: u32,
    height: u32,
) -> AuthoringResult<(Vec<u8>, Vec<u8>)> {
    let data_size = width as usize * height as usize * 4;
    let data = read_typed_tag(
        manager,
        data_tag,
        data_size,
        0x28,
        0x01,
        "stock watermark template texture data",
    )?;
    let header = read_typed_tag(
        manager,
        header_tag,
        STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE,
        0x20,
        0x01,
        "stock watermark template texture header",
    )?;
    validate_entry_reference(
        manager,
        data_tag,
        u32::from(header_tag),
        "stock watermark template texture data",
    )?;
    validate_entry_reference(
        manager,
        header_tag,
        u32::from(data_tag),
        "stock watermark template texture header",
    )?;
    if data.len() != data_size
        || !is_stock_straight_rgba8_texture_header(&header, width, height, data_size)
    {
        return Err(invalid(format!(
            "Stock watermark template header {header_tag} no longer describes {width}x{height} straight RGBA8"
        )));
    }
    Ok((header, data))
}

pub(super) fn read_and_validate_icon_container(
    manager: &PackageManager,
    container_tag: TagHash,
) -> AuthoringResult<(Vec<u8>, IconDefinitionCompanion)> {
    let container = read_typed_tag(
        manager,
        container_tag,
        ICON_CONTAINER_SIZE,
        0x10,
        0x00,
        "donor item-icon container",
    )?;
    validate_entry_reference(
        manager,
        container_tag,
        ICON_CONTAINER_CLASS_HANDLE,
        "donor item-icon container",
    )?;
    if read_u32(&container, 0)? != ICON_CONTAINER_SIZE as u32 {
        return Err(invalid(format!(
            "Donor item-icon container {container_tag} is not a complete Shadowkeep weapon icon"
        )));
    }
    let mut dependencies = SharedTagDependencies::new();
    for offset in ICON_LAYER_REFERENCE_OFFSETS {
        let raw = read_u32(&container, offset)?;
        let required = offset == ICON_PRIMARY_LAYER_OFFSET;
        if let Some(layer_dependencies) = validate_optional_resource_reference(
            raw,
            required,
            &format!("donor icon-container {container_tag} layer at +0x{offset:02X}"),
            |layer_tag| validate_icon_layer(manager, layer_tag, container_tag, offset),
        )? {
            dependencies.extend(layer_dependencies);
        }
    }
    let companion = read_and_validate_icon_companion(manager, container_tag)?;
    dependencies.insert(u32::from(container_tag));
    dependencies.insert(u32::from(companion.tag));
    if dependencies != companion.dependencies {
        let missing = dependencies
            .difference(&companion.dependencies)
            .map(|raw| format!("0x{raw:08X}"))
            .collect::<Vec<_>>();
        let unexpected = companion
            .dependencies
            .difference(&dependencies)
            .map(|raw| format!("0x{raw:08X}"))
            .collect::<Vec<_>>();
        return Err(invalid(format!(
            "Donor icon definition {container_tag} companion {} does not exactly describe its reachable graph (missing: {}; unexpected: {})",
            companion.tag,
            display_set_difference(&missing),
            display_set_difference(&unexpected)
        )));
    }
    Ok((container, companion))
}

pub(super) fn collect_unchanged_container_dependencies(
    manager: &PackageManager,
    donor_container_tag: TagHash,
    donor_container: &[u8],
    primary_is_authored: bool,
) -> AuthoringResult<SharedTagDependencies> {
    let mut dependencies = SharedTagDependencies::new();
    for offset in ICON_LAYER_REFERENCE_OFFSETS {
        if offset == ICON_WATERMARK_LAYER_OFFSET
            || (primary_is_authored && offset == ICON_PRIMARY_LAYER_OFFSET)
        {
            continue;
        }
        let raw = read_u32(donor_container, offset)?;
        let required = offset == ICON_PRIMARY_LAYER_OFFSET;
        if let Some(layer_dependencies) = validate_optional_resource_reference(
            raw,
            required,
            &format!(
                "authored clone of icon-container {donor_container_tag} layer at +0x{offset:02X}"
            ),
            |layer_tag| validate_icon_layer(manager, layer_tag, donor_container_tag, offset),
        )? {
            dependencies.extend(layer_dependencies);
        }
    }
    Ok(dependencies)
}

fn display_set_difference(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_owned()
    } else {
        values.join(", ")
    }
}

pub(crate) fn validate_icon_layer(
    manager: &PackageManager,
    layer_tag: TagHash,
    container_tag: TagHash,
    container_offset: usize,
) -> AuthoringResult<SharedTagDependencies> {
    let description = format!(
        "donor icon-container {container_tag} layer {layer_tag} at +0x{container_offset:02X}"
    );
    let entry = manager
        .get_entry(layer_tag)
        .ok_or_else(|| invalid(format!("{description} has no package entry")))?;
    if entry.file_type != 0x08
        || entry.file_subtype != 0x00
        || entry.reference != WATERMARK_CLASS_HANDLE
    {
        return Err(invalid(format!(
            "{description} has type/reference {:02X}/{:02X}/0x{:08X}; expected 08/00/0x{WATERMARK_CLASS_HANDLE:08X}",
            entry.file_type, entry.file_subtype, entry.reference
        )));
    }
    let layer = manager
        .read_tag(layer_tag)
        .map_err(|error| invalid(format!("Could not read {description}: {error}")))?;
    if layer.len() != entry.file_size as usize || read_u32(&layer, 0)? as usize != layer.len() {
        return Err(invalid(format!(
            "{description} decoded to {} bytes while its entry/payload declares {}/{}",
            layer.len(),
            entry.file_size,
            read_u32(&layer, 0)?
        )));
    }
    validate_layer_texture_graph(manager, layer_tag, &layer, &description)
}

fn validate_layer_texture_graph(
    manager: &PackageManager,
    layer_tag: TagHash,
    layer: &[u8],
    description: &str,
) -> AuthoringResult<SharedTagDependencies> {
    let mut dependencies = SharedTagDependencies::new();
    dependencies.insert(u32::from(layer_tag));
    validate_layer_texture_references(layer, description, |header_tag| {
        let data_tag = validate_texture_resource_pair(manager, layer_tag, header_tag, description)?;
        dependencies.insert(u32::from(header_tag));
        dependencies.insert(u32::from(data_tag));
        Ok(())
    })?;
    Ok(dependencies)
}

pub(super) fn validate_layer_texture_references(
    layer: &[u8],
    description: &str,
    mut validate_header: impl FnMut(TagHash) -> AuthoringResult<()>,
) -> AuthoringResult<()> {
    let lane_count = usize::try_from(read_u64(layer, 0x20)?)
        .map_err(|_| invalid(format!("{description} lane count exceeds this platform")))?;
    if lane_count == 0 || lane_count > MAX_LAYER_LANES {
        return Err(invalid(format!(
            "{description} has invalid icon-layer lane count {lane_count}"
        )));
    }
    let lanes = relative_target(layer, 0x28, description)?;
    if lanes < 4
        || read_u32(layer, lanes - 4)? != LAYER_ARRAY_CLASS
        || usize::try_from(read_u64(layer, lanes)?).ok() != Some(lane_count)
        || read_u32(layer, lanes + 8)? != LAYER_LANE_CLASS
    {
        return Err(invalid(format!(
            "{description} has an invalid icon-layer lane array"
        )));
    }
    let descriptors = lanes
        .checked_add(0x10)
        .ok_or_else(|| invalid(format!("{description} lane descriptors overflowed")))?;
    for lane_index in 0..lane_count {
        let descriptor = descriptors
            .checked_add(lane_index * 0x10)
            .ok_or_else(|| invalid(format!("{description} lane descriptor overflowed")))?;
        let texture_count = usize::try_from(read_u64(layer, descriptor)?)
            .map_err(|_| invalid(format!("{description} texture count exceeds this platform")))?;
        if texture_count == 0 || texture_count > MAX_TEXTURES_PER_LANE {
            return Err(invalid(format!(
                "{description} lane {lane_index} has invalid texture count {texture_count}"
            )));
        }
        let textures = relative_target(layer, descriptor + 8, description)?;
        if textures < 4
            || read_u32(layer, textures - 4)? != LAYER_ARRAY_CLASS
            || usize::try_from(read_u64(layer, textures)?).ok() != Some(texture_count)
            || read_u32(layer, textures + 8)? != LAYER_TEXTURE_CLASS
        {
            return Err(invalid(format!(
                "{description} lane {lane_index} has an invalid texture array"
            )));
        }
        let tags = textures
            .checked_add(0x10)
            .ok_or_else(|| invalid(format!("{description} texture tags overflowed")))?;
        for texture_index in 0..texture_count {
            let raw = read_u32(layer, tags + texture_index * 4)?;
            validate_optional_resource_reference(
                raw,
                true,
                &format!("{description} lane {lane_index} texture {texture_index} header"),
                &mut validate_header,
            )?;
        }
    }
    Ok(())
}

fn validate_texture_resource_pair(
    manager: &PackageManager,
    layer_tag: TagHash,
    header_tag: TagHash,
    description: &str,
) -> AuthoringResult<TagHash> {
    let header_description = format!("{description} texture header {header_tag} from {layer_tag}");
    let header = read_typed_tag(
        manager,
        header_tag,
        STOCK_STRAIGHT_RGBA8_TEXTURE_HEADER_SIZE,
        0x20,
        0x01,
        &header_description,
    )?;
    let header_entry = manager
        .get_entry(header_tag)
        .ok_or_else(|| invalid(format!("{header_description} has no package entry")))?;
    let data_tag = TagHash(header_entry.reference);
    let data_size = read_u32(&header, 0)? as usize;
    if data_size == 0 {
        return Err(invalid(format!(
            "{header_description} declares an empty texture payload"
        )));
    }
    validate_optional_resource_reference(
        u32::from(data_tag),
        true,
        &format!("{header_description} data"),
        |resolved_data_tag| {
            read_typed_tag(
                manager,
                resolved_data_tag,
                data_size,
                0x28,
                0x01,
                &format!("{header_description} data"),
            )?;
            validate_entry_reference(
                manager,
                resolved_data_tag,
                u32::from(header_tag),
                &format!("{header_description} data"),
            )
        },
    )?;
    Ok(data_tag)
}

pub(super) fn validate_optional_resource_reference<T>(
    raw: u32,
    required: bool,
    description: &str,
    resolve: impl FnOnce(TagHash) -> AuthoringResult<T>,
) -> AuthoringResult<Option<T>> {
    if raw == u32::MAX {
        if required {
            return Err(invalid(format!("{description} is required but absent")));
        }
        return Ok(None);
    }
    let tag = TagHash(raw);
    if !is_valid_package_tag(tag) {
        return Err(invalid(format!(
            "{description} contains malformed reference 0x{raw:08X}; absent references must be 0xFFFFFFFF"
        )));
    }
    resolve(tag).map(Some).map_err(|error| {
        invalid(format!(
            "{description} references unresolved or incompatible tag {tag}: {error}"
        ))
    })
}

fn read_typed_tag(
    manager: &PackageManager,
    tag: TagHash,
    expected_size: usize,
    expected_type: u8,
    expected_subtype: u8,
    description: &str,
) -> AuthoringResult<Vec<u8>> {
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| invalid(format!("{description} {tag} has no package entry")))?;
    if entry.file_size as usize != expected_size
        || entry.file_type != expected_type
        || entry.file_subtype != expected_subtype
    {
        return Err(invalid(format!(
            "{description} {tag} has size/type {}/{:02X}/{:02X}; expected {expected_size}/{expected_type:02X}/{expected_subtype:02X}",
            entry.file_size, entry.file_type, entry.file_subtype
        )));
    }
    let payload = manager
        .read_tag(tag)
        .map_err(|error| invalid(format!("Could not read {description} {tag}: {error}")))?;
    if payload.len() != expected_size {
        return Err(invalid(format!(
            "{description} {tag} decoded to {} bytes; expected {expected_size}",
            payload.len()
        )));
    }
    Ok(payload)
}

fn validate_entry_reference(
    manager: &PackageManager,
    tag: TagHash,
    expected: u32,
    description: &str,
) -> AuthoringResult<()> {
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| invalid(format!("{description} {tag} has no package entry")))?;
    if entry.reference != expected {
        return Err(invalid(format!(
            "{description} {tag} references 0x{:08X}; expected 0x{expected:08X}",
            entry.reference
        )));
    }
    Ok(())
}
