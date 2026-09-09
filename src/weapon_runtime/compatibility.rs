//! Lightweight, conservative native resource shape evidence for donor screening.

use super::*;
use crate::weapon_entity::WeaponComponentBinding;

#[cfg(test)]
mod tests;

/// Native instance and optional definition schemas with verified serialized byte bounds.
///
/// A successful load proves that both schemas are known by the embedded native registry. It does
/// not prove that two resources have interchangeable behavior or compatible sibling components.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeaponRuntimeResourceShape {
    pub instance_schema: u32,
    /// `None` means no native definition was declared, not an unknown or invalid definition.
    pub definition_schema: Option<u32>,
}

/// Reads native resource schemas without decoding their reflected fields.
///
/// Unknown schemas, including generated-only schemas, are deliberately rejected so callers cannot
/// classify unverified data as a lower-risk match. Invalid declared native definitions are errors
/// rather than being treated as absent definitions. The owning tag is validated before inspection.
pub fn load_weapon_runtime_resource_shape(
    manager: &PackageManager,
    binding: &WeaponComponentBinding,
) -> Result<WeaponRuntimeResourceShape, String> {
    let registry = runtime_registry()?;
    let runtime_binding = runtime_shape_binding(binding)?;
    // Generated-only and unknown classes cannot qualify. Reject these common cases before
    // reading a component owner for each donor candidate.
    native_schema_size(binding.concrete_class, "instance", registry)?;
    let owner = read_component_owner(manager, binding.owner_tag)?;
    native_resource_shape(&owner, &runtime_binding, registry)
}

fn runtime_shape_binding(binding: &WeaponComponentBinding) -> Result<WeaponRuntimeBinding, String> {
    Ok(WeaponRuntimeBinding {
        binding_hash: binding.binding_hash,
        binding_label: String::new(),
        resource_index: u16::try_from(binding.resource_index)
            .map_err(|_| "Runtime resource index does not fit 16 bits")?,
        resource_count: u16::try_from(binding.resource_count)
            .map_err(|_| "Runtime resource count does not fit 16 bits")?,
        owner_tag: binding.owner_tag,
        concrete_class: binding.concrete_class,
        resource_offset: binding.resource_offset,
    })
}

fn native_resource_shape(
    owner_payload: &[u8],
    binding: &WeaponRuntimeBinding,
    registry: &RuntimeRegistry,
) -> Result<WeaponRuntimeResourceShape, String> {
    let start = usize::try_from(binding.resource_offset)
        .map_err(|_| "Runtime resource offset does not fit this platform")?;
    let instance_size = validate_native_root(
        owner_payload,
        start,
        binding.concrete_class,
        "instance",
        registry,
    )?;
    // Match the full decoder's owner descriptor validation. Native root sizes come from the
    // registry rather than inferred adjacent-root boundaries.
    inferred_resource_limit(owner_payload, binding)?;

    let native_definition = native_component_definition_reference(owner_payload, binding)?;
    let definition_schema = if let Some((definition_start, definition_schema)) = native_definition {
        if instance_size < 16 {
            return Err(
                "Native component definition prefix extends beyond its instance schema".into(),
            );
        }
        let definition_size = validate_native_root(
            owner_payload,
            definition_start,
            definition_schema,
            "definition",
            registry,
        )?;
        if definition_size < 8 {
            return Err("Native component identity extends beyond its definition schema".into());
        }
        inferred_definition_limit(owner_payload, definition_start)?;
        Some(definition_schema)
    } else {
        // The general field decoder deliberately ignores an unresolvable optional prefix. A
        // compatibility screen must not mistake a declared, malformed definition for absence.
        let definition_declared = instance_size >= 8
            && read_u32(owner_payload, start)? == binding.owner_tag
            && !matches!(read_u32(owner_payload, start + 4)?, 0 | u32::MAX);
        if definition_declared {
            return Err(format!(
                "Runtime binding 0x{:08X} resource {} declares a native definition with an invalid target or identity",
                binding.binding_hash, binding.resource_index
            ));
        }
        None
    };
    Ok(WeaponRuntimeResourceShape {
        instance_schema: binding.concrete_class,
        definition_schema,
    })
}

fn validate_native_root(
    owner_payload: &[u8],
    start: usize,
    schema: u32,
    kind: &str,
    registry: &RuntimeRegistry,
) -> Result<usize, String> {
    let size = native_schema_size(schema, kind, registry)?;
    if start
        .checked_add(size)
        .is_none_or(|end| end > owner_payload.len())
    {
        return Err(format!(
            "Runtime {kind} schema 0x{schema:08X} needs 0x{size:X} bytes at owner offset 0x{start:X}, beyond the owner payload"
        ));
    }
    Ok(size)
}

fn native_schema_size(
    schema: u32,
    kind: &str,
    registry: &RuntimeRegistry,
) -> Result<usize, String> {
    let record = registry.records.get(&schema).ok_or_else(|| {
        format!("Runtime {kind} schema 0x{schema:08X} is not verified by the native registry")
    })?;
    let size = usize::try_from(record.struct_size)
        .map_err(|_| "Native runtime schema size does not fit this platform")?;
    if size == 0 {
        return Err(format!(
            "Runtime {kind} schema 0x{schema:08X} has zero size"
        ));
    }
    Ok(size)
}
