use super::*;

pub(super) fn read_component_owner(
    manager: &PackageManager,
    owner_tag: u32,
) -> Result<Vec<u8>, String> {
    let owner = TagHash(owner_tag);
    let entry = manager
        .get_entry(owner)
        .ok_or_else(|| format!("Runtime component owner {owner} is not live"))?;
    if entry.file_type != 8 {
        return Err(format!(
            "Runtime component owner {owner} has file type {}, expected structured content",
            entry.file_type
        ));
    }
    if entry.reference != STRUCTURED_RESOURCE_CLASS {
        return Err(format!(
            "Runtime component owner {owner} has class 0x{:08X}, expected 0x{STRUCTURED_RESOURCE_CLASS:08X}",
            entry.reference
        ));
    }
    let payload = manager
        .read_tag(owner)
        .map_err(|error| format!("Could not read runtime component owner {owner}: {error}"))?;
    if usize::try_from(read_u64(&payload, 0)?).ok() != Some(payload.len()) {
        return Err(format!(
            "Runtime component owner {owner} has an inconsistent native file-size field"
        ));
    }
    Ok(payload)
}

pub(super) fn decode_component_resource(
    manager: &PackageManager,
    owner_payload: &[u8],
    binding: &WeaponRuntimeBinding,
    registry: &RuntimeRegistry,
) -> Result<WeaponRuntimeResource, String> {
    let start = usize::try_from(binding.resource_offset).map_err(|_| {
        format!(
            "Runtime binding 0x{:08X} resource {} offset does not fit this platform",
            binding.binding_hash, binding.resource_index
        )
    })?;
    if start >= owner_payload.len() {
        return Err(format!(
            "Runtime binding 0x{:08X} resource {} starts beyond owner 0x{:08X}",
            binding.binding_hash, binding.resource_index, binding.owner_tag
        ));
    }

    let instance_limit = inferred_resource_limit(owner_payload, binding)?;
    let instance = decode_component_root(
        manager,
        owner_payload,
        binding,
        WeaponRuntimeRootKind::ComponentInstance,
        start,
        binding.concrete_class,
        instance_limit,
        registry,
    )?;
    let definition = component_definition_reference(owner_payload, binding, manager, registry)?
        .map(|(definition_start, definition_class)| {
            decode_component_root(
                manager,
                owner_payload,
                binding,
                WeaponRuntimeRootKind::ComponentDefinition,
                definition_start,
                definition_class,
                inferred_definition_limit(owner_payload, definition_start)?,
                registry,
            )
        })
        .transpose()?;
    Ok(WeaponRuntimeResource {
        binding_hash: binding.binding_hash,
        binding_label: binding.binding_label.clone(),
        resource_index: binding.resource_index,
        resource_count: binding.resource_count,
        owner_tag: binding.owner_tag,
        concrete_class: binding.concrete_class,
        alias_bindings: Vec::new(),
        instance,
        definition,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decode_component_root(
    manager: &PackageManager,
    owner_payload: &[u8],
    binding: &WeaponRuntimeBinding,
    kind: WeaponRuntimeRootKind,
    start: usize,
    schema: u32,
    inferred_limit: usize,
    registry: &RuntimeRegistry,
) -> Result<WeaponRuntimeRoot, String> {
    let generated_schema = manager
        .get_entry(TagHash(schema))
        .is_some_and(|entry| entry.reference == GENERATED_SCHEMA_CLASS);
    let (root_size, inferred_is_boundary) = if let Some(record) = registry.records.get(&schema) {
        (
            usize::try_from(record.struct_size)
                .map_err(|_| "Runtime component schema size does not fit this platform")?,
            false,
        )
    } else if generated_schema {
        (
            inferred_limit
                .checked_sub(start)
                .ok_or("Generated runtime component range is reversed")?,
            true,
        )
    } else {
        return Err(format!(
            "Runtime binding 0x{:08X} resource {} uses {} class 0x{schema:08X}, which is absent from the client schema closure",
            binding.binding_hash,
            binding.resource_index,
            kind.label().to_ascii_lowercase()
        ));
    };
    if root_size == 0 {
        return Err(format!(
            "Runtime binding 0x{:08X} resource {} has a zero-size {} class 0x{schema:08X}",
            binding.binding_hash,
            binding.resource_index,
            kind.label().to_ascii_lowercase()
        ));
    }
    let limit = start
        .checked_add(root_size)
        .ok_or("Runtime component range overflowed")?;
    if limit > owner_payload.len() || (inferred_is_boundary && limit > inferred_limit) {
        return Err(format!(
            "Runtime binding 0x{:08X} resource {} {} class 0x{schema:08X} needs 0x{root_size:X} bytes at owner offset 0x{start:X}, outside its verified 0x{inferred_limit:X} limit in owner 0x{:08X}",
            binding.binding_hash,
            binding.resource_index,
            kind.label().to_ascii_lowercase(),
            binding.owner_tag
        ));
    }
    let descriptor = OwnerRootDescriptor {
        kind,
        target: start,
        schema,
        limit,
    };
    let mut fields = if generated_schema {
        let schema_payload = manager.read_tag(TagHash(schema)).map_err(|error| {
            format!("Could not read generated runtime component schema 0x{schema:08X}: {error}")
        })?;
        decode_generated_root_fields(
            &schema_payload,
            owner_payload,
            descriptor,
            root_size,
            binding.binding_hash,
            binding.resource_index,
            registry,
        )?
    } else {
        decode_registry_root_fields(
            owner_payload,
            descriptor,
            root_size,
            binding.binding_hash,
            binding.resource_index,
            registry,
        )?
    };
    fields.sort_by_key(|field| {
        (
            field.locator.value_offset,
            field.locator.byte_size,
            field.path_label.clone(),
        )
    });
    fields.dedup_by(|left, right| left.locator == right.locator);
    append_uncovered_runtime_ranges(
        owner_payload,
        descriptor,
        &mut fields,
        binding.binding_hash,
        binding.resource_index,
        &[],
    )?;
    fields.sort_by_key(|field| {
        (
            field.locator.value_offset,
            field.locator.byte_size,
            field.path_label.clone(),
        )
    });
    Ok(WeaponRuntimeRoot {
        kind,
        schema,
        owner_offset: u32::try_from(start)
            .map_err(|_| "Runtime component offset does not fit 32 bits")?,
        byte_size: u32::try_from(root_size)
            .map_err(|_| "Runtime component size does not fit 32 bits")?,
        generated_schema,
        fields,
    })
}

pub(super) fn append_uncovered_runtime_ranges(
    owner_payload: &[u8],
    root: OwnerRootDescriptor,
    fields: &mut Vec<WeaponRuntimeField>,
    binding_hash: u32,
    resource_index: u16,
    excluded_owner_ranges: &[(usize, usize)],
) -> Result<(), String> {
    let root_size = root
        .limit
        .checked_sub(root.target)
        .ok_or("Runtime component range is reversed")?;
    let mut covered = fields
        .iter()
        .filter_map(|field| {
            let start = usize::try_from(field.locator.value_offset).ok()?;
            let end = start.checked_add(field.locator.byte_size as usize)?;
            (start < root_size).then_some((start, end.min(root_size)))
        })
        .collect::<Vec<_>>();
    covered.extend(excluded_owner_ranges.iter().filter_map(|&(start, end)| {
        let clipped_start = start.max(root.target);
        let clipped_end = end.min(root.limit);
        (clipped_start < clipped_end).then_some((
            clipped_start.saturating_sub(root.target),
            clipped_end.saturating_sub(root.target),
        ))
    }));
    covered.sort_unstable();
    let mut merged = Vec::<(usize, usize)>::new();
    for (start, end) in covered {
        if let Some((_, previous_end)) = merged.last_mut()
            && start <= *previous_end
        {
            *previous_end = (*previous_end).max(end);
        } else {
            merged.push((start, end));
        }
    }
    let mut cursor = 0usize;
    for (start, end) in merged
        .into_iter()
        .chain(std::iter::once((root_size, root_size)))
    {
        while cursor < start {
            let chunk_end = cursor
                .saturating_add(MAX_TECHNICAL_RUNTIME_FIELD_BYTES)
                .min(start);
            let absolute = root
                .target
                .checked_add(cursor)
                .ok_or("Technical runtime field offset overflowed")?;
            let bytes = owner_payload
                .get(absolute..absolute + (chunk_end - cursor))
                .ok_or("Technical runtime field extends beyond its owner payload")?
                .to_vec();
            let value_offset =
                u32::try_from(cursor).map_err(|_| "Technical runtime offset exceeds 32 bits")?;
            let byte_size = u32::try_from(bytes.len())
                .map_err(|_| "Technical runtime field size exceeds 32 bits")?;
            let label = if byte_size == 1 {
                format!("Unreflected byte +0x{value_offset:X}")
            } else {
                format!(
                    "Unreflected bytes +0x{value_offset:X}..+0x{:X}",
                    value_offset + byte_size
                )
            };
            fields.push(WeaponRuntimeField {
                locator: WeaponRuntimeFieldLocator {
                    binding_hash,
                    resource_index,
                    root: root.kind,
                    root_schema: root.schema,
                    path: vec![WeaponRuntimePathElement {
                        name_hash: TECHNICAL_BYTES_PATH_HASH,
                        type_handle: root.schema,
                        byte_offset: value_offset,
                    }],
                    type_handle: root.schema,
                    value_offset,
                    byte_size,
                },
                owner_offset: u32::try_from(absolute)
                    .map_err(|_| "Technical runtime owner offset exceeds 32 bits")?,
                name: label.clone(),
                path_label: label,
                kind: WeaponRuntimeValueKind::FixedBytes { size: byte_size },
                value: WeaponRuntimeValue::Bytes(bytes),
                source: WeaponRuntimeFieldSource::OpaqueNativeType,
                generated_kind: None,
            });
            cursor = chunk_end;
        }
        cursor = cursor.max(end);
    }
    Ok(())
}

pub(super) fn component_definition_reference(
    owner_payload: &[u8],
    binding: &WeaponRuntimeBinding,
    manager: &PackageManager,
    registry: &RuntimeRegistry,
) -> Result<Option<(usize, u32)>, String> {
    let Some((target, definition_class)) =
        native_component_definition_reference(owner_payload, binding)?
    else {
        return Ok(None);
    };
    let known_class = registry.records.contains_key(&definition_class)
        || manager
            .get_entry(TagHash(definition_class))
            .is_some_and(|entry| entry.reference == GENERATED_SCHEMA_CLASS);
    if !known_class {
        return Ok(None);
    }
    if let Some(record) = registry.records.get(&definition_class) {
        let Some(end) = target.checked_add(record.struct_size as usize) else {
            return Ok(None);
        };
        if end > owner_payload.len() {
            return Ok(None);
        }
    }
    Ok(Some((target, definition_class)))
}

pub(super) fn native_component_definition_reference(
    owner_payload: &[u8],
    binding: &WeaponRuntimeBinding,
) -> Result<Option<(usize, u32)>, String> {
    let start = usize::try_from(binding.resource_offset)
        .map_err(|_| "Runtime resource offset does not fit this platform")?;
    if start
        .checked_add(16)
        .is_none_or(|end| end > owner_payload.len())
        || read_u32(owner_payload, start)? != binding.owner_tag
    {
        return Ok(None);
    }
    let definition_class = read_u32(owner_payload, start + 4)?;
    if matches!(definition_class, 0 | u32::MAX) {
        return Ok(None);
    }
    // Component resource prefixes store the definition as an absolute offset inside the owner
    // payload. It is not one of the self-relative pointers used by the owner's root descriptors.
    let raw_target = read_u64(owner_payload, start + 8)?;
    let Ok(target) = usize::try_from(raw_target) else {
        return Ok(None);
    };
    if target == 0
        || target == start
        || target % 8 != 0
        || target
            .checked_add(size_of::<u32>() * 2)
            .is_none_or(|end| end > owner_payload.len())
        || read_u32(owner_payload, target)? != binding.owner_tag
        || read_u32(owner_payload, target + size_of::<u32>())? != binding.concrete_class
    {
        return Ok(None);
    }
    Ok(Some((target, definition_class)))
}

pub(super) fn inferred_resource_limit(
    owner_payload: &[u8],
    binding: &WeaponRuntimeBinding,
) -> Result<usize, String> {
    let start = usize::try_from(binding.resource_offset)
        .map_err(|_| "Runtime resource offset does not fit this platform")?;
    let limit = containing_owner_root_limit(owner_payload, start)?.unwrap_or(owner_payload.len());
    if limit <= start {
        return Err(format!(
            "Could not infer a positive serialized range for runtime binding 0x{:08X} resource {}",
            binding.binding_hash, binding.resource_index
        ));
    }
    Ok(limit)
}

pub(super) fn inferred_definition_limit(
    owner_payload: &[u8],
    definition_start: usize,
) -> Result<usize, String> {
    containing_owner_root_limit(owner_payload, definition_start)?
        .filter(|limit| *limit > definition_start)
        .ok_or_else(|| {
            format!(
                "Could not infer a serialized owner-root boundary for component definition at 0x{definition_start:X}"
            )
        })
}

pub(super) fn containing_owner_root_limit(
    owner_payload: &[u8],
    offset: usize,
) -> Result<Option<usize>, String> {
    let mut targets = Vec::new();
    for pointer in [OWNER_INSTANCE_POINTER, OWNER_DEFINITION_POINTER] {
        if read_i64(owner_payload, pointer)? != 0 {
            targets.push(relative_target(owner_payload, pointer)?);
        }
    }
    targets.sort_unstable();
    for (index, &target) in targets.iter().enumerate() {
        let limit = targets
            .get(index + 1)
            .map_or(owner_payload.len(), |next| next.saturating_sub(4));
        if target <= offset && offset < limit {
            return Ok(Some(limit));
        }
    }
    Ok(None)
}

pub(super) fn decode_owner_roots(
    manager: &PackageManager,
    payload: &[u8],
    owner_tag: u32,
    anchor_binding_hash: u32,
    anchor_resource_index: u16,
    registry: &RuntimeRegistry,
) -> Result<Vec<WeaponRuntimeRoot>, String> {
    let mut descriptors = Vec::new();
    for (kind, pointer) in [
        (WeaponRuntimeRootKind::Instance, OWNER_INSTANCE_POINTER),
        (WeaponRuntimeRootKind::Definition, OWNER_DEFINITION_POINTER),
    ] {
        if read_i64(payload, pointer)? == 0 {
            continue;
        }
        let target = relative_target(payload, pointer)?;
        if target < 4 {
            return Err(format!(
                "Runtime component owner 0x{owner_tag:08X} {} root has no schema word",
                kind.label().to_ascii_lowercase()
            ));
        }
        descriptors.push((kind, target, read_u32(payload, target - 4)?));
    }
    descriptors.sort_by_key(|(_, target, _)| *target);
    let descriptors = descriptors
        .iter()
        .enumerate()
        .map(|(index, &(kind, target, schema))| {
            let limit = descriptors
                .get(index + 1)
                .map_or(payload.len(), |(_, next, _)| next.saturating_sub(4));
            if target >= limit || limit > payload.len() {
                return Err(format!(
                    "Runtime component owner 0x{owner_tag:08X} has overlapping native roots"
                ));
            }
            Ok(OwnerRootDescriptor {
                kind,
                target,
                schema,
                limit,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let mut roots = Vec::with_capacity(descriptors.len());
    for descriptor in descriptors {
        let generated_schema = manager
            .get_entry(TagHash(descriptor.schema))
            .is_some_and(|entry| entry.reference == GENERATED_SCHEMA_CLASS);
        let declared_size = if generated_schema {
            descriptor.limit - descriptor.target
        } else {
            let record = registry.records.get(&descriptor.schema).ok_or_else(|| {
                format!(
                    "Runtime component owner 0x{owner_tag:08X} uses unknown native schema 0x{:08X}",
                    descriptor.schema
                )
            })?;
            usize::try_from(record.struct_size)
                .map_err(|_| "Runtime schema size does not fit this platform")?
        };
        let available = descriptor.limit - descriptor.target;
        if declared_size > available {
            return Err(format!(
                "Runtime component owner 0x{owner_tag:08X} {} schema 0x{:08X} needs 0x{declared_size:X} bytes, but only 0x{available:X} are available",
                descriptor.kind.label().to_ascii_lowercase(),
                descriptor.schema
            ));
        }
        let root_size = if generated_schema {
            available
        } else {
            declared_size
        };
        let mut fields = if generated_schema {
            let schema_payload = manager
                .read_tag(TagHash(descriptor.schema))
                .map_err(|error| {
                    format!(
                        "Could not read generated runtime schema 0x{:08X}: {error}",
                        descriptor.schema
                    )
                })?;
            decode_generated_root_fields(
                &schema_payload,
                payload,
                descriptor,
                root_size,
                anchor_binding_hash,
                anchor_resource_index,
                registry,
            )?
        } else {
            decode_registry_root_fields(
                payload,
                descriptor,
                root_size,
                anchor_binding_hash,
                anchor_resource_index,
                registry,
            )?
        };
        fields.sort_by_key(|field| {
            (
                field.owner_offset,
                field.locator.byte_size,
                field.path_label.clone(),
            )
        });
        fields.dedup_by(|left, right| left.locator == right.locator);
        roots.push(WeaponRuntimeRoot {
            kind: descriptor.kind,
            schema: descriptor.schema,
            owner_offset: u32::try_from(descriptor.target)
                .map_err(|_| "Runtime root offset does not fit 32 bits")?,
            byte_size: u32::try_from(root_size)
                .map_err(|_| "Runtime root size does not fit 32 bits")?,
            generated_schema,
            fields,
        });
    }
    roots.sort_by_key(|root| root.kind);
    Ok(roots)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decode_generated_root_fields(
    schema_payload: &[u8],
    owner_payload: &[u8],
    root: OwnerRootDescriptor,
    root_size: usize,
    anchor_binding_hash: u32,
    anchor_resource_index: u16,
    registry: &RuntimeRegistry,
) -> Result<Vec<WeaponRuntimeField>, String> {
    let generated = generated_schema_fields(schema_payload, root_size, registry)?;
    let mut fields = Vec::new();
    for field in generated {
        let path = vec![WeaponRuntimePathElement {
            name_hash: field.name_hash,
            type_handle: field.type_handle,
            byte_offset: field.value_offset,
        }];
        let labels = vec![humanize_identifier(&field.name)];
        let absolute = root
            .target
            .checked_add(field.value_offset as usize)
            .ok_or("Generated runtime field offset overflowed")?;
        flatten_runtime_type(
            owner_payload,
            root,
            root_size,
            absolute,
            field.type_handle,
            Some((field.metadata & 0xFF) as u8),
            path,
            labels,
            WeaponRuntimeFieldSource::GeneratedSchema,
            anchor_binding_hash,
            anchor_resource_index,
            registry,
            &mut BTreeSet::new(),
            &mut fields,
            0,
        )?;
    }
    Ok(fields)
}

pub(super) fn decode_registry_root_fields(
    owner_payload: &[u8],
    root: OwnerRootDescriptor,
    root_size: usize,
    anchor_binding_hash: u32,
    anchor_resource_index: u16,
    registry: &RuntimeRegistry,
) -> Result<Vec<WeaponRuntimeField>, String> {
    let mut fields = Vec::new();
    flatten_runtime_type(
        owner_payload,
        root,
        root_size,
        root.target,
        root.schema,
        None,
        Vec::new(),
        Vec::new(),
        WeaponRuntimeFieldSource::NativeMember,
        anchor_binding_hash,
        anchor_resource_index,
        registry,
        &mut BTreeSet::new(),
        &mut fields,
        0,
    )?;
    Ok(fields)
}

#[allow(clippy::too_many_arguments, clippy::cognitive_complexity)]
pub(super) fn flatten_runtime_type(
    owner_payload: &[u8],
    root: OwnerRootDescriptor,
    root_size: usize,
    absolute: usize,
    type_handle: u32,
    generated_kind: Option<u8>,
    path: Vec<WeaponRuntimePathElement>,
    labels: Vec<String>,
    source: WeaponRuntimeFieldSource,
    anchor_binding_hash: u32,
    anchor_resource_index: u16,
    registry: &RuntimeRegistry,
    active_types: &mut BTreeSet<u32>,
    output: &mut Vec<WeaponRuntimeField>,
    depth: usize,
) -> Result<(), String> {
    if depth >= MAX_RUNTIME_SCHEMA_DEPTH {
        return Err(format!(
            "Runtime schema 0x{:08X} exceeds the supported native nesting depth",
            root.schema
        ));
    }
    let (kind, size, resolved_source) = runtime_type_kind(type_handle, generated_kind, registry)?;
    ensure_field_range(owner_payload, root, root_size, absolute, size)?;
    if let Some(kind) = kind {
        return push_runtime_leaf(
            owner_payload,
            root,
            absolute,
            type_handle,
            generated_kind,
            path,
            labels,
            kind,
            resolved_source.unwrap_or(source),
            anchor_binding_hash,
            anchor_resource_index,
            registry,
            output,
        );
    }

    let record = registry.records.get(&type_handle).ok_or_else(|| {
        format!("Runtime type 0x{type_handle:08X} is missing from the embedded schema closure")
    })?;
    if !active_types.insert(type_handle) {
        let kind = WeaponRuntimeValueKind::FixedBytes { size };
        return push_runtime_leaf(
            owner_payload,
            root,
            absolute,
            type_handle,
            generated_kind,
            path,
            labels,
            kind,
            WeaponRuntimeFieldSource::OpaqueNativeType,
            anchor_binding_hash,
            anchor_resource_index,
            registry,
            output,
        );
    }
    let before = output.len();
    if !matches!(record.base_type, 0 | u32::MAX) {
        flatten_runtime_type(
            owner_payload,
            root,
            root_size,
            absolute,
            record.base_type,
            generated_kind,
            path.clone(),
            labels.clone(),
            source,
            anchor_binding_hash,
            anchor_resource_index,
            registry,
            active_types,
            output,
            depth + 1,
        )?;
    }
    for member in &record.members {
        if member.name_hash == EMPTY_NAME_HASH {
            continue;
        }
        let mut member_path = path.clone();
        member_path.push(WeaponRuntimePathElement {
            name_hash: member.name_hash,
            type_handle: member.type_handle,
            byte_offset: member.byte_offset,
        });
        let mut member_labels = labels.clone();
        member_labels.push(runtime_member_label(member.name_hash, registry));
        let member_absolute = absolute
            .checked_add(member.byte_offset as usize)
            .ok_or("Runtime member offset overflowed")?;
        flatten_runtime_type(
            owner_payload,
            root,
            root_size,
            member_absolute,
            member.type_handle,
            None,
            member_path,
            member_labels,
            WeaponRuntimeFieldSource::NativeMember,
            anchor_binding_hash,
            anchor_resource_index,
            registry,
            active_types,
            output,
            depth + 1,
        )?;
    }
    active_types.remove(&type_handle);
    if output.len() == before {
        push_runtime_leaf(
            owner_payload,
            root,
            absolute,
            type_handle,
            generated_kind,
            path,
            labels,
            WeaponRuntimeValueKind::FixedBytes { size },
            WeaponRuntimeFieldSource::OpaqueNativeType,
            anchor_binding_hash,
            anchor_resource_index,
            registry,
            output,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn push_runtime_leaf(
    owner_payload: &[u8],
    root: OwnerRootDescriptor,
    absolute: usize,
    type_handle: u32,
    generated_kind: Option<u8>,
    path: Vec<WeaponRuntimePathElement>,
    mut labels: Vec<String>,
    kind: WeaponRuntimeValueKind,
    source: WeaponRuntimeFieldSource,
    anchor_binding_hash: u32,
    anchor_resource_index: u16,
    registry: &RuntimeRegistry,
    output: &mut Vec<WeaponRuntimeField>,
) -> Result<(), String> {
    if labels.is_empty() {
        labels.push(format!("Value 0x{type_handle:08X}"));
    }
    let owner_offset =
        u32::try_from(absolute).map_err(|_| "Runtime field offset does not fit 32 bits")?;
    let value_offset = absolute
        .checked_sub(root.target)
        .and_then(|offset| u32::try_from(offset).ok())
        .ok_or("Runtime field root-relative offset does not fit 32 bits")?;
    let value = decode_runtime_value(owner_payload, absolute, &kind)?;
    let path_label = labels.join(" › ");
    let name = labels
        .last()
        .cloned()
        .unwrap_or_else(|| format_runtime_path(&path, registry));
    output.push(WeaponRuntimeField {
        locator: WeaponRuntimeFieldLocator {
            binding_hash: anchor_binding_hash,
            resource_index: anchor_resource_index,
            root: root.kind,
            root_schema: root.schema,
            path,
            type_handle,
            value_offset,
            byte_size: kind.byte_size(),
        },
        owner_offset,
        name,
        path_label,
        kind,
        value,
        source,
        generated_kind,
    });
    Ok(())
}
