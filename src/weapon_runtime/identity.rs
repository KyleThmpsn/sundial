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
        0x8080_3A18 => Some("Movement Interface"),
        0x8080_3C07 => Some("Ability Controller Interface"),
        0x8080_3930 => Some("Player Stat Interface"),
        0x8080_3921 => Some("Weapon Stat Interface"),
        // 815B886A binds this interface to 4BEE. Its methods include damage
        // registration CD3020/CDDDD0 and the health/shield update consumers.
        0x8080_4BE4 => Some("Health and Shield Interface"),
        0x8080_4BEE => Some("Health and Shields"),
        0x8080_4B8A => Some("Health and Shield Settings"),
        // B8F7F0 joins these separate arrays. B8B6E0/B8BED0/B8BA30 consume
        // the 80-byte region settings, EC00D0 classifies the 312-byte regions.
        0x8080_4C11 => Some("Health Region"),
        0x8080_4C5F => Some("Health Region Recovery Settings"),
        // D20A40 initializes 43E1 inside the attachment. Its damage callback
        // D24AC0 and update D2B880 feed the shared invisibility interface 44E7.
        0x8080_43DF => Some("Invisibility Attachment"),
        0x8080_43EC => Some("Invisibility Attachment Settings"),
        0x8080_43E1 => Some("Invisibility Response"),
        0x8080_43E2 => Some("Invisibility Response Settings"),
        0x8080_44E7 => Some("Invisibility Interface"),
        0x8080_43E5 => Some("Invisibility Controller"),
        // DF7400 enables/disables every modifier through F433A0. DF0600
        // links each modifier to the component/input resolved by CA9850.
        0x8080_3B00 => Some("Component Property Modifiers"),
        0x8080_3B01 => Some("Component Property Modifiers Settings"),
        0x8080_3B05 => Some("Component Property Modifier"),
        0x8080_3B06 => Some("Component Property Modifier Settings"),
        // F83970 registers with the health component through CD3020. F83E80
        // evaluates filters and value programs, consumed as multipliers by CD3570.
        // Barrier mods and Oppressive Darkness independently exercise both directions.
        0x8080_3F8B => Some("Incoming Damage Modifiers"),
        0x8080_3F8C => Some("Incoming Damage Modifier Settings"),
        0x8080_2A1B => Some("Conditional Damage Multiplier"),
        0x8080_2A1C => Some("Conditional Damage Multiplier Settings"),
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
