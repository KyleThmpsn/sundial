//! Native runtime registry identities and readable value formatting.

use super::*;
use std::fmt::Write as _;

/// The effective runtime entity this weapon compiles against: the component bindings, the
/// resources they select, and every readable field of every component owner. This is the
/// registry a runtime value or binary patch addresses, so a locator in the recipe above can be
/// matched to the field it points at. The graph already includes the recipe's runtime baseline
/// and its component donors, so it is what the build sees rather than the bare donor.
/// Rendered separately from the rest of the report because it is the one section whose size
/// depends on the packages rather than the recipe: a weapon carries a few thousand readable
/// fields, so this is cached against the graph it came from instead of rebuilt every frame.
pub(super) fn runtime_registry_section(registry: Registry<'_>, include_fields: bool) -> String {
    let mut text = String::new();
    let out = &mut text;
    let graph = match registry {
        None => {
            let _ = writeln!(out, "\nRUNTIME REGISTRY  not read");
            return text;
        }
        Some(Err(error)) => {
            let _ = writeln!(out, "\nRUNTIME REGISTRY  unavailable: {error}");
            return text;
        }
        Some(Ok(graph)) => graph,
    };
    let resource_roots = || {
        graph.resources.iter().flat_map(|resource| {
            std::iter::once(&resource.instance).chain(resource.definition.iter())
        })
    };
    let owner_roots = || graph.owners.iter().flat_map(|owner| &owner.roots);
    let count_fields = |roots: &mut dyn Iterator<Item = &WeaponRuntimeRoot>| {
        roots.map(|root| root.fields.len()).sum::<usize>()
    };
    let _ = writeln!(
        out,
        "\nRUNTIME REGISTRY  entity {}  item {}  pattern {}",
        hex(graph.entity_tag),
        hex(graph.item_hash),
        hex(graph.pattern_global_id_hash)
    );
    field(out, "bindings", graph.bindings.len());
    field(out, "resources", graph.resources.len());
    field(out, "resource fields", count_fields(&mut resource_roots()));
    field(out, "owners", graph.owners.len());
    field(out, "owner-only fields", count_fields(&mut owner_roots()));

    let _ = writeln!(out, "\nRUNTIME BINDINGS  ({})", graph.bindings.len());
    for binding in &graph.bindings {
        let _ = writeln!(
            out,
            "  {}  {:<34}[{}/{}]  owner {}  class {}  +{:#x}",
            hex(binding.binding_hash),
            binding.binding_label,
            binding.resource_index,
            binding.resource_count,
            hex(binding.owner_tag),
            hex(binding.concrete_class),
            binding.resource_offset
        );
    }

    let _ = writeln!(out, "\nRUNTIME RESOURCES  ({})", graph.resources.len());
    for resource in &graph.resources {
        let aliases = if resource.alias_bindings.is_empty() {
            String::new()
        } else {
            format!(
                "  also {}",
                resource
                    .alias_bindings
                    .iter()
                    .map(|(hash, index)| format!("{}#{index}", hex(*hash)))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        let _ = writeln!(
            out,
            "  {}  {:<34}[{}/{}]  owner {}  class {}{aliases}",
            hex(resource.binding_hash),
            resource.binding_label,
            resource.resource_index,
            resource.resource_count,
            hex(resource.owner_tag),
            hex(resource.concrete_class)
        );
        for root in std::iter::once(&resource.instance).chain(resource.definition.iter()) {
            let _ = writeln!(out, "      {}", root_summary(root));
            if include_fields {
                for entry in &root.fields {
                    let _ = writeln!(out, "        {}", registry_field(entry));
                }
            }
        }
    }

    // An owner root keeps only what no bound resource above already covers, so this section is
    // the remainder of each component payload rather than a second copy of it.
    let _ = writeln!(
        out,
        "\nRUNTIME OWNERS  ({})  fields outside every bound resource",
        graph.owners.len()
    );
    for owner in &graph.owners {
        let _ = writeln!(
            out,
            "\n  OWNER {}  anchor {}#{}  roots {}",
            hex(owner.owner_tag),
            hex(owner.anchor_binding_hash),
            owner.anchor_resource_index,
            owner.roots.len()
        );
        for root in &owner.roots {
            let _ = writeln!(out, "    {}", root_summary(root));
            if include_fields {
                for entry in &root.fields {
                    let _ = writeln!(out, "      {}", registry_field(entry));
                }
            }
        }
    }
    if !include_fields {
        let _ = writeln!(out, "\n  Field values omitted.");
    }
    text
}

/// How many readable field values the registry would add, for the control that turns them on.
pub(super) fn runtime_registry_field_count(graph: &WeaponRuntimeGraph) -> usize {
    graph
        .resources
        .iter()
        .flat_map(|resource| std::iter::once(&resource.instance).chain(resource.definition.iter()))
        .chain(graph.owners.iter().flat_map(|owner| &owner.roots))
        .map(|root| root.fields.len())
        .sum()
}

pub(super) fn root_summary(root: &WeaponRuntimeRoot) -> String {
    // A root's own schema is often not the class its binding selects, so naming it here adds
    // what the binding line cannot: an instance and its definition are different components.
    let schema = match sundial::package_authoring::runtime::native_type_name(root.schema) {
        Some(name) => format!("{} {name}", hex(root.schema)),
        None => hex(root.schema),
    };
    format!(
        "{:<22}schema {:<34}+{:#x}  {} bytes  {} fields{}",
        root.kind.label(),
        schema,
        root.owner_offset,
        root.byte_size,
        root.fields.len(),
        if root.generated_schema {
            "  generated schema"
        } else {
            ""
        }
    )
}

/// One field line: what it is called, what it holds now, and where it lives. The path is the
/// locator's own label, so it matches a runtime value override recorded in the recipe, and is
/// left off when it only repeats the name. Long opaque values are cut so the columns hold;
/// the binary patch editor is the place to read a whole block byte for byte.
pub(super) fn registry_field(entry: &WeaponRuntimeField) -> String {
    let name = if entry.name_inferred {
        format!("{} (inferred)", entry.name)
    } else {
        entry.name.clone()
    };
    let redundant = !entry.path_label.contains(" / ")
        && (entry.path_label.ends_with(&entry.name) || entry.path_label.ends_with(&name));
    let path = if redundant {
        ""
    } else {
        entry.path_label.as_str()
    };
    let line = format!(
        "{:<40}{:<32}{:<9}{:<15}+{:<10}{path}",
        column(&name, 39),
        column(&registry_value(&entry.kind, &entry.value), 31),
        value_kind_label(&entry.kind),
        field_source_label(entry.source),
        format!("{:#x}", entry.owner_offset)
    );
    line.trim_end().to_owned()
}

/// Pad-or-cut, so one long value cannot push every later column out of line.
pub(super) fn column(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let kept = text
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>();
    format!("{kept}…")
}

pub(super) fn registry_value(kind: &WeaponRuntimeValueKind, value: &WeaponRuntimeValue) -> String {
    match value {
        WeaponRuntimeValue::Boolean(value) => value.to_string(),
        WeaponRuntimeValue::Signed(value) => value.to_string(),
        WeaponRuntimeValue::Unsigned(value) => match kind {
            WeaponRuntimeValueKind::HexIdentifier { bits }
            | WeaponRuntimeValueKind::BitFlags { bits } => {
                format!("0x{value:0>width$X}", width = (*bits as usize).div_ceil(4))
            }
            _ => value.to_string(),
        },
        WeaponRuntimeValue::Float32Bits(bits) => f32::from_bits(*bits).to_string(),
        WeaponRuntimeValue::Float64Bits(bits) => f64::from_bits(*bits).to_string(),
        WeaponRuntimeValue::Vector4Float32Bits(bits) => format!(
            "[{}]",
            bits.iter()
                .map(|bits| f32::from_bits(*bits).to_string())
                .collect::<Vec<_>>()
                .join(" ")
        ),
        // A long opaque block is identified, not reproduced; the bytes are in the packages.
        WeaponRuntimeValue::Bytes(bytes) if bytes.len() > 6 => format!(
            "{} +{} bytes",
            crate::app::runtime_fields::format_runtime_bytes(&bytes[..6]),
            bytes.len() - 6
        ),
        WeaponRuntimeValue::Bytes(bytes) => crate::app::runtime_fields::format_runtime_bytes(bytes),
    }
}

pub(super) fn value_kind_label(kind: &WeaponRuntimeValueKind) -> String {
    match kind {
        WeaponRuntimeValueKind::Boolean => "bool".to_owned(),
        WeaponRuntimeValueKind::SignedInteger { bits } => format!("i{bits}"),
        WeaponRuntimeValueKind::UnsignedInteger { bits } => format!("u{bits}"),
        WeaponRuntimeValueKind::Enum { bits } => format!("enum{bits}"),
        WeaponRuntimeValueKind::BitFlags { bits } => format!("flags{bits}"),
        WeaponRuntimeValueKind::HexIdentifier { bits } => format!("hex{bits}"),
        WeaponRuntimeValueKind::Float32 => "f32".to_owned(),
        WeaponRuntimeValueKind::Float64 => "f64".to_owned(),
        WeaponRuntimeValueKind::Vector4Float32 => "vec4".to_owned(),
        WeaponRuntimeValueKind::FixedBytes { size } => format!("bytes{size}"),
    }
}

pub(super) fn field_source_label(source: WeaponRuntimeFieldSource) -> &'static str {
    match source {
        WeaponRuntimeFieldSource::GeneratedSchema => "generated",
        WeaponRuntimeFieldSource::NativeMember => "native member",
        WeaponRuntimeFieldSource::OpaqueNativeType => "opaque type",
        WeaponRuntimeFieldSource::NativeDeclaration => "native decl",
    }
}
