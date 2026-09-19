//! Readable roles established by native consumers, independent of asset test notes.
use super::*;

/// A role name is supplied only for an exact type with an established contract.
/// A shared base class never gives a derived type an invented gameplay name.
pub fn native_type_name(schema: u32) -> Option<&'static str> {
    let known = match schema {
        // Movement reset, integration and curve consumers, shared with parameters.rs
        // and structure/labels.rs. These describe roles, not recovered C++ symbols.
        0x8080_3B73 => Some("Projectile Movement"),
        0x8080_388F => Some("Projectile Movement Settings"),
        0x8080_37C9 => Some("Flight Curve State"),
        0x8080_3803 => Some("Flight Curve Settings"),
        0x8080_37BA => Some("Projectile Simulation State"),
        // CA2830 dispatches the first three target categories to these interfaces.
        0x8080_3A12 => Some("Weapon Controller Interface"),
        0x8080_3A29 => Some("Magazine Interface"),
        0x8080_3A08 => Some("Barrel Interface"),
        0x8080_3B06 => Some("Component Property Modifier Settings"),
        0x8080_40B5 => Some("Perk Action"),
        0x8080_93F3 => Some("Object Label Filter"),
        0x8080_4C83 => Some("Object Reference and Label Filter"),
        0x8080_2F1A => Some("Numeric Value Pair"),
        _ => None,
    };
    if known.is_some() {
        return known;
    }
    // Reuse the action decoder's proven operation names. Some native storage
    // classes serve more than one operation, so conflicting names stay unresolved.
    let mut nodes = crate::sandbox_perk::nodes::CONDITIONS
        .iter()
        .chain(crate::sandbox_perk::nodes::EFFECTS.iter())
        .filter(|node| schema != 0 && node.class == schema);
    let first = nodes.next()?.name;
    nodes.all(|node| node.name == first).then_some(first)
}

/// Known member names are useful evidence even when the whole component's role
/// is unresolved. Inferred hash candidates are deliberately excluded.
pub fn native_member_names(schema: u32) -> Vec<String> {
    let Ok(registry) = runtime_registry() else {
        return Vec::new();
    };
    let mut current = schema;
    let mut visited = BTreeSet::new();
    let mut result = BTreeSet::new();
    while let Some(record) = registry.records.get(&current) {
        if !visited.insert(current) {
            break;
        }
        for member in &record.members {
            if let Some(names) = registry.names.get(&member.name_hash) {
                for name in names {
                    result.insert(humanize_identifier(name.strip_prefix("m_").unwrap_or(name)));
                }
            }
        }
        current = record.base_type;
    }
    result.into_iter().collect()
}
