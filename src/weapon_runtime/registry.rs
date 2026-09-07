use super::*;

pub(super) fn runtime_registry() -> Result<&'static RuntimeRegistry, String> {
    RUNTIME_REGISTRY
        .get_or_init(build_runtime_registry)
        .as_ref()
        .map_err(Clone::clone)
}

pub(super) fn build_runtime_registry() -> Result<RuntimeRegistry, String> {
    let mut records = serde_json::from_str::<Vec<RegistryRecord>>(include_str!("schema.json"))
        .map_err(|error| format!("Embedded weapon runtime schema is invalid: {error}"))?;
    records.extend(
        serde_json::from_str::<Vec<RegistryRecord>>(include_str!("projectile_schema.json"))
            .map_err(|error| format!("Embedded projectile runtime schema is invalid: {error}"))?,
    );
    if records.len() < 900 {
        return Err(format!(
            "Embedded weapon runtime schema is incomplete ({} records)",
            records.len()
        ));
    }
    let mut records_by_handle = BTreeMap::new();
    for record in records {
        if record.handle & 0xFFF0_0000 != 0x8080_0000 {
            return Err(format!(
                "Embedded runtime schema contains invalid handle 0x{:08X}",
                record.handle
            ));
        }
        if record
            .members
            .iter()
            .any(|member| member.byte_offset >= record.struct_size && record.struct_size != 0)
        {
            return Err(format!(
                "Embedded runtime schema 0x{:08X} contains an out-of-range member",
                record.handle
            ));
        }
        if record
            .native_layout
            .iter()
            .any(|entry| entry.type_code > 13)
        {
            return Err(format!(
                "Embedded runtime schema 0x{:08X} contains invalid native-layout opcode",
                record.handle
            ));
        }
        if records_by_handle.insert(record.handle, record).is_some() {
            return Err("Embedded runtime schema contains duplicate handles".into());
        }
    }
    for (handle, size) in [
        (0x8080_0004, 1),
        (0x8080_0005, 1),
        (0x8080_0006, 2),
        (0x8080_0007, 4),
        (0x8080_000C, 8),
        (0x8080_000F, 4),
        (0x8080_0014, 4),
        (0x8080_0091, 16),
    ] {
        if records_by_handle
            .get(&handle)
            .map(|record| record.struct_size)
            != Some(size)
        {
            return Err(format!(
                "Embedded runtime schema anchor 0x{handle:08X} does not have size {size}"
            ));
        }
    }
    let encoded_names =
        serde_json::from_str::<BTreeMap<String, Vec<String>>>(include_str!("names.json"))
            .map_err(|error| format!("Embedded weapon runtime name map is invalid: {error}"))?;
    let mut names = BTreeMap::new();
    for (hash, mut candidates) in encoded_names {
        let hash = u32::from_str_radix(&hash, 16)
            .map_err(|_| format!("Embedded runtime name key {hash:?} is not hexadecimal"))?;
        candidates.retain(|candidate| fnv1_name_hash(candidate) == hash);
        candidates.sort();
        candidates.dedup();
        if !candidates.is_empty() {
            names.insert(hash, candidates);
        }
    }
    Ok(RuntimeRegistry {
        records: records_by_handle,
        names,
    })
}

pub(super) fn runtime_binding_label(binding_hash: u32, registry: &RuntimeRegistry) -> String {
    let known = match binding_hash {
        WEAPON_INPUT_COMPONENT_KEY => Some("Input"),
        WEAPON_TRIGGER_COMPONENT_KEY => Some("Trigger"),
        WEAPON_BARREL_COMPONENT_KEY => Some("Barrel"),
        WEAPON_CONTROLLER_COMPONENT_KEY => Some("Weapon controller"),
        WEAPON_MAGAZINE_COMPONENT_KEY => Some("Magazine"),
        WEAPON_RELOAD_COMPONENT_KEY => Some("Reload"),
        WEAPON_TRIGGER_CHARGE_COMPONENT_KEY => Some("Trigger charge"),
        WEAPON_STAT_TRANSLATOR_COMPONENT_KEY => Some("Weapon stats / translator"),
        _ => None,
    };
    known.map(str::to_owned).unwrap_or_else(|| {
        registry
            .names
            .get(&binding_hash)
            .and_then(|names| names.first())
            .map(|name| humanize_identifier(name))
            .unwrap_or_else(|| format!("Binding 0x{binding_hash:08X}"))
    })
}

pub(super) fn generated_schema_fields(
    data: &[u8],
    root_size: usize,
    registry: &RuntimeRegistry,
) -> Result<Vec<GeneratedField>, String> {
    let mut fields = BTreeMap::new();
    for record in (0..data.len().saturating_sub(20)).step_by(8) {
        let Ok(target) = relative_target(data, record) else {
            continue;
        };
        let Some(length) = data[target..]
            .iter()
            .position(|byte| *byte == 0)
            .filter(|length| (2..=MAX_GENERATED_SCHEMA_NAME).contains(length))
        else {
            continue;
        };
        let identifier = &data[target..target + length];
        if !identifier
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'/' | b'\\' | b'.'))
        {
            continue;
        }
        let type_handle = read_u32(data, record + 8)?;
        let value_offset = read_u32(data, record + 12)?;
        let metadata = read_u32(data, record + 16)?;
        let metadata_kind = (metadata & 0xFF) as u8;
        if metadata_kind == 0 || metadata_kind == 0x15 || metadata_kind > 0x2A {
            continue;
        }
        let Ok((_, size, _)) = runtime_type_kind(type_handle, Some(metadata_kind), registry) else {
            continue;
        };
        if size == 0
            || usize::try_from(value_offset)
                .ok()
                .and_then(|offset| offset.checked_add(size as usize))
                .is_none_or(|end| end > root_size)
        {
            continue;
        }
        let name = String::from_utf8(identifier.to_vec())
            .map_err(|_| "Generated runtime schema identifier is not UTF-8")?;
        let field = GeneratedField {
            name_hash: fnv1_name_hash(&name),
            name,
            type_handle,
            value_offset,
            metadata,
        };
        fields
            .entry((value_offset, field.name_hash, type_handle))
            .or_insert(field);
    }
    Ok(fields.into_values().collect())
}

pub(super) fn runtime_type_kind(
    type_handle: u32,
    generated_kind: Option<u8>,
    registry: &RuntimeRegistry,
) -> Result<
    (
        Option<WeaponRuntimeValueKind>,
        u32,
        Option<WeaponRuntimeFieldSource>,
    ),
    String,
> {
    let builtin = match type_handle {
        0x8080_0004 => Some(WeaponRuntimeValueKind::Boolean),
        0x8080_0005 => Some(WeaponRuntimeValueKind::SignedInteger { bits: 8 }),
        0x8080_0006 => Some(WeaponRuntimeValueKind::SignedInteger { bits: 16 }),
        0x8080_0007 => Some(WeaponRuntimeValueKind::SignedInteger { bits: 32 }),
        0x8080_0008 => Some(WeaponRuntimeValueKind::SignedInteger { bits: 64 }),
        0x8080_0009 => Some(WeaponRuntimeValueKind::UnsignedInteger { bits: 8 }),
        0x8080_000A => Some(WeaponRuntimeValueKind::UnsignedInteger { bits: 16 }),
        0x8080_000B => Some(WeaponRuntimeValueKind::UnsignedInteger { bits: 32 }),
        0x8080_000C => Some(WeaponRuntimeValueKind::UnsignedInteger { bits: 64 }),
        0x8080_000F => Some(WeaponRuntimeValueKind::Float32),
        0x8080_0012 => Some(WeaponRuntimeValueKind::HexIdentifier { bits: 64 }),
        0x8080_0014 => Some(WeaponRuntimeValueKind::HexIdentifier { bits: 32 }),
        0x8080_0090 | 0x8080_0091 => Some(WeaponRuntimeValueKind::Vector4Float32),
        _ => None,
    };
    if let Some(kind) = builtin {
        return Ok((Some(kind.clone()), kind.byte_size(), None));
    }
    let record = registry.records.get(&type_handle).ok_or_else(|| {
        format!("Runtime type 0x{type_handle:08X} is not present in the schema closure")
    })?;
    let size = record.struct_size;
    if let Some(kind) = generated_scalar_kind(generated_kind, size) {
        return Ok((Some(kind), size, None));
    }
    if record.members.is_empty() && !matches!(record.base_type, 0 | u32::MAX) {
        let (base_kind, base_size, _) = runtime_type_kind(record.base_type, None, registry)?;
        if base_size == size && base_kind.is_some() {
            return Ok((base_kind, size, None));
        }
    }
    if size == 0 {
        return Ok((None, 0, None));
    }
    Ok((None, size, None))
}

pub(super) fn generated_scalar_kind(kind: Option<u8>, size: u32) -> Option<WeaponRuntimeValueKind> {
    match (kind, size) {
        (Some(0x01), 4) => Some(WeaponRuntimeValueKind::SignedInteger { bits: 32 }),
        (Some(0x02), 2) => Some(WeaponRuntimeValueKind::SignedInteger { bits: 16 }),
        (Some(0x03), 1) => Some(WeaponRuntimeValueKind::SignedInteger { bits: 8 }),
        (Some(0x0A), 4) => Some(WeaponRuntimeValueKind::Float32),
        (Some(0x0C), 1) => Some(WeaponRuntimeValueKind::Boolean),
        (Some(0x13 | 0x22), 1 | 2 | 4 | 8) => Some(WeaponRuntimeValueKind::Enum {
            bits: (size * 8) as u8,
        }),
        (Some(0x23), 1 | 2 | 4 | 8) => Some(WeaponRuntimeValueKind::BitFlags {
            bits: (size * 8) as u8,
        }),
        (Some(0x14), 1 | 2 | 4 | 8) => Some(WeaponRuntimeValueKind::HexIdentifier {
            bits: (size * 8) as u8,
        }),
        (Some(0x2A), 16) => Some(WeaponRuntimeValueKind::Vector4Float32),
        _ => None,
    }
}

pub(super) fn runtime_member_label(name_hash: u32, registry: &RuntimeRegistry) -> String {
    registry
        .names
        .get(&name_hash)
        .and_then(|names| names.first())
        .map(|name| humanize_identifier(name))
        .unwrap_or_else(|| format!("Member 0x{name_hash:08X}"))
}

pub(super) fn format_runtime_path(
    path: &[WeaponRuntimePathElement],
    registry: &RuntimeRegistry,
) -> String {
    if path.is_empty() {
        return "native value".into();
    }
    path.iter()
        .map(|element| runtime_member_label(element.name_hash, registry))
        .collect::<Vec<_>>()
        .join(" › ")
}

pub(super) fn humanize_identifier(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut capitalize = true;
    for character in value.chars() {
        if matches!(character, '_' | '/' | '\\' | '.') {
            if !output.ends_with(' ') {
                output.push(' ');
            }
            capitalize = true;
        } else if capitalize {
            output.extend(character.to_uppercase());
            capitalize = false;
        } else {
            output.push(character);
        }
    }
    output.trim().to_owned()
}
