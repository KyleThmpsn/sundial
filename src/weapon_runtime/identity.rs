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
        // Weapon component classes named by the binding that selects them, surveyed across
        // the 129 distinct runtime entities in a Shadowkeep install. The binding key is the
        // evidence and the name is its role; none of these is a recovered C++ symbol. The
        // count after each is how many of the entities exposing that binding agree.
        0x8080_43DA => Some("Input"),    // input 0xC18BD28D, 76 of 76
        0x8080_388A => Some("Trigger"),  // trigger 0xD5A123FF, 67 of 67
        0x8080_3889 => Some("Barrel"),   // barrel 0xEC711FA3, 67 of 67
        0x8080_3A0F => Some("Magazine"), // magazine 0xB1AAA2CB, 67 of 76
        0x8080_3C63 => Some("Magazine (second form)"), // the same binding, other 9
        0x8080_3844 => Some("Reload"),   // reload 0xB9CAA3BC, 62 of 67
        0x8080_3842 => Some("Reload (second form)"), // the same binding, other 5
        0x8080_3ABD => Some("Trigger Charge"), // trigger charge 0x7E4A5223, 13 of 13
        0x80BF_DFEA => Some("Weapon Controller"), // controller 0x39AFD7D3, 57 of 67
        0x80BF_DFEB => Some("Weapon Controller Data"),
        0x8080_3ACB => Some("HUD Content"), // HUD content 0x5F0DD954, 76 of 76
        // The four presentation components a cross-family appearance moves together. Their
        // data classes are corroborated by the model preview, which reads the skeleton's bone
        // nodes and the animation lookup's clip bank directly.
        0x8080_8545 => Some("Weapon Skeleton"), // skeleton 0x3DBC2FC8 and 0x4637A966, 88 of 88
        0x8080_8546 => Some("Weapon Skeleton Data"),
        0x8080_3466 => Some("Weapon Animation Lookup"), // lookup 0x681C2C0D, 74 of 74
        0x8080_344B => Some("Weapon Animation Lookup Data"),
        0x8080_36E2 => Some("Weapon Animation Set"), // set 0x89834B2B, 74 of 74
        0x8080_36CF => Some("Weapon Animation Set Data"),
        0x8080_3E70 => Some("First-Person Attachment"), // attachment 0xD3A5500E, 78 of 78
        0x8080_4221 => Some("First-Person Attachment Data"),
        // Components named by the members the native content registry already knows them to
        // expose. The member list is the evidence and the name is the component's role; a
        // reader who needs the exact identity still has the class on the same line, which is
        // why two components that expose the same kind of marker can share a name.
        // The other weapon components a survey of 42 entities turns up stay unnamed. Their
        // members are the shared falloff member and unreflected bytes, and the registry
        // knows no member names for their classes at all, so nothing establishes a role.
        // Field names seen inside their resources belong to nested records, not to the
        // component, which is why they are not evidence here.
        0x8080_92D8 => Some("Constraints"), // Constraint Metadata
        0x8080_72CB => Some("Channels"),    // Channels
        0x8080_3A68 => Some("Interaction"), // Interaction Marker
        0x8080_4CA3 => Some("Interaction"), // Interaction Marker
        0x8080_4491 => Some("Interaction"), // Interaction Marker
        0x8080_5FB3 => Some("Interaction"), // Interaction Marker
        0x8080_4044 => Some("Interaction"), // Interaction Marker
        0x8080_4DC2 => Some("Interaction"), // Interaction Marker
        0x8080_3C23 => Some("Aiming and Turrets"), // Aiming Locations, Camera Anchor Marker, Turrets
        0x8080_8506 => Some("Query Markers"),      // Query Marker Set
        0x8080_7132 => Some("Light Markers"),      // Light Markers
        0x8080_6CBD => Some("Light Markers"),      // Light Markers
        0x8080_4F7E => Some("Attachment Markers"), // Attachment Marker
        0x8080_3991 => Some("Character Markers"),  // Aim, Body, Damage Owner, Head, Waypoint
        0x8080_4580 => Some("Target Markers"),     // Target Markers
        0x8080_3D4C => Some("Anti-Gravity Points"), // Anti Gravity Points
        0x8080_42AD => Some("Entry Markers"),      // Entry Marker, Marker
        0x8080_3BBC => Some("Character Effect Markers"), // foot, grenade, melee and hand markers
        0x8080_3E63 => Some("Item Anchors"),       // equipped and stowed item markers
        0x8080_3C3E => Some("Behaviors"),          // Behaviors
        // Components named by the members they decode to across ten families. "Adjust Falloff
        // Layer" is shared by nearly every component and carries no evidence, so it is not
        // used here. Nor is a member that names another component rather than this one's own
        // contents: a slot referring to a label component does not make its holder one.
        0x8080_8569 => Some("Sight and Fire Markers"), // ADS focus, iron sight, primary fire and trigger markers
        0x8080_40D2 => Some("Perk States"), // Perk States, Perk State Stack, Active Status Icons
        0x8080_4069 => Some("Perk States"), // Perk States, Perk State Stack, Active Status Icons
        0x8080_8BF1 => Some("Local Channels"), // Local Channels
        0x8080_8F96 => Some("Rig Controls"), // Rig Controls
        0x8080_366D => Some("Start Timing"), // Start Delay, Start Ratio
        // Every class the stat-translator owner is built from. Bindings 0x2F981564,
        // 0x747605C6 and 0xAE4CC974 share that owner and select exactly these twenty classes
        // across the installed families, so each one is that family's own implementation of
        // the same slot rather than twenty different components.
        0x80BC_1272 => Some("Weapon Stats / Translator"),
        0x81A6_B612 => Some("Weapon Stats / Translator"),
        0x8157_93F8 => Some("Weapon Stats / Translator"),
        0x80BB_B906 => Some("Weapon Stats / Translator"),
        0x80BC_067C => Some("Weapon Stats / Translator"),
        0x80C1_C53B => Some("Weapon Stats / Translator"),
        0x80EF_0DC3 => Some("Weapon Stats / Translator"),
        0x8152_532D => Some("Weapon Stats / Translator"),
        0x80BB_D12D => Some("Weapon Stats / Translator"),
        0x80EF_31FD => Some("Weapon Stats / Translator"),
        0x80BB_CAB2 => Some("Weapon Stats / Translator"),
        0x80FF_0158 => Some("Weapon Stats / Translator"),
        0x8161_EE7D => Some("Weapon Stats / Translator"),
        0x8161_F4F2 => Some("Weapon Stats / Translator"),
        0x8161_F5F5 => Some("Weapon Stats / Translator"),
        0x81A6_A143 => Some("Weapon Stats / Translator"),
        0x81A6_AA4A => Some("Weapon Stats / Translator"),
        0x81A6_AF8D => Some("Weapon Stats / Translator"),
        0x81A6_AF93 => Some("Weapon Stats / Translator"),
        0x81A6_B06D => Some("Weapon Stats / Translator"),
        // The definition half of each component named above. A resource carries one instance
        // root and one definition root, and across fourteen families every one of these
        // instance classes pairs with exactly one definition class, so the pairing is the
        // evidence rather than an assumption about the layout.
        0x8080_366C => Some("Start Timing Data"),
        0x8080_3843 => Some("Reload Data (second form)"),
        0x8080_3845 => Some("Reload Data"),
        0x8080_3865 => Some("Barrel Data"),
        0x8080_388B => Some("Trigger Data"),
        0x8080_384B => Some("Magazine Data"),
        0x8080_3AC9 => Some("HUD Content Data"),
        0x8080_406A => Some("Perk States Data"),
        0x8080_4064 => Some("Perk States Data"),
        0x8080_43D8 => Some("Input Data"),
        0x8080_72C7 => Some("Channels Data"),
        0x8080_8507 => Some("Query Markers Data"),
        0x8080_8568 => Some("Sight and Fire Markers Data"),
        0x8080_8BE6 => Some("Local Channels Data"),
        0x8080_8F8F => Some("Rig Controls Data"),
        0x8080_8A0C => Some("Constraints Data"),
        0x80BB_B907 => Some("Weapon Stats / Translator Data"),
        0x80BC_1273 => Some("Weapon Stats / Translator Data"),
        0x80C1_C53A => Some("Weapon Stats / Translator Data"),
        0x80EF_0DC4 => Some("Weapon Stats / Translator Data"),
        0x8157_93F9 => Some("Weapon Stats / Translator Data"),
        // Components named from how the rest of this repository already reads them.
        0x8080_72B8 => Some("Gear Model"), // model tag at data+0x1DC; see model_preview
        0x8080_72BD => Some("Gear Model Data"),
        0x8080_979F => Some("Channel Interpolation"), // parhelion-import audit/channels.rs
        0x8080_9790 => Some("Channel Interpolation Data"),
        0x8080_84D7 => Some("Effect Nodes"), // particle and sound nodes at data+0x168
        0x8080_84E9 => Some("Effect Nodes Data"),
        // Components named from the field names and values of stock content, surveyed over the
        // 3,264 decodable graphs that carry them. Each instance class pairs with exactly one
        // definition class across every resource, which names the Data halves.
        // Generated schemas name this component `self_destruction` in 480 graphs across 16
        // object types, and the recovered member `m_destroy_timer` holds it. Its value program
        // holds the time in seconds, as the Duration evidence below shows.
        0x8080_3C50 => Some("Self-Destruct Timer"),
        0x8080_3C51 => Some("Self-Destruct Timer Data"),
        // Holds a Self-Destruct Timer whose one constant is the effect's length in seconds. It
        // matches stock descriptions (Taken, Fallen and Hive Barrier, 10) and known timings
        // (Rampage 3.5 on all four of its effects, Kill Clip and Multikill Clip 5). Effects that
        // perks attach Always store -1, for no time limit.
        0x8080_3B32 => Some("Duration"),
        0x8080_3B33 => Some("Duration Data"),
        // One effect's generated schema names this component `status_icon`. It is the first
        // member of the component 2,023 attached effects share, and it holds their Duration.
        0x8080_4211 => Some("Status Icon"),
        0x8080_4212 => Some("Status Icon Data"),
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

/// One reflected member of a native type, as the runtime registry records it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMember {
    /// The type that declares the member.
    pub holder: u32,
    pub byte_offset: u32,
    pub name_hash: u32,
    /// A readable name, when a verified or recovered one is known.
    pub name: Option<String>,
    /// Whether the name is a recovered candidate rather than a verified source name.
    pub inferred: bool,
    pub type_handle: u32,
    /// Whether the member holds a plain value, such as a number or a flag, not an object.
    pub scalar: bool,
}

/// The reflected members a type declares itself, in offset order. Base types are left out,
/// since they are shared engine plumbing rather than what sets the type apart.
pub fn native_members(schema: u32) -> Vec<NativeMember> {
    let Ok(registry) = runtime_registry() else {
        return Vec::new();
    };
    let Some(record) = registry.records.get(&schema) else {
        return Vec::new();
    };
    let mut members = record
        .members
        .iter()
        .map(|member| native_member(schema, member, registry))
        .collect::<Vec<_>>();
    members.sort_by_key(|member| member.byte_offset);
    members
}

/// Every reflected member, in any type, that holds a `schema`.
pub fn native_holders(schema: u32) -> Vec<NativeMember> {
    let Ok(registry) = runtime_registry() else {
        return Vec::new();
    };
    registry
        .records
        .iter()
        .flat_map(|(&holder, record)| {
            record
                .members
                .iter()
                .filter(|member| member.type_handle == schema)
                .map(move |member| native_member(holder, member, registry))
        })
        .collect()
}

fn native_member(holder: u32, member: &RegistryMember, registry: &RuntimeRegistry) -> NativeMember {
    let verified = registry
        .names
        .get(&member.name_hash)
        .and_then(|names| names.first());
    let name = verified.or_else(|| registry.inferred.get(&member.name_hash));
    NativeMember {
        holder,
        byte_offset: member.byte_offset,
        name_hash: member.name_hash,
        name: name.map(|name| humanize_identifier(name.strip_prefix("m_").unwrap_or(name))),
        inferred: verified.is_none() && name.is_some(),
        type_handle: member.type_handle,
        scalar: matches!(
            runtime_type_kind(member.type_handle, None, registry),
            Ok((Some(_), _, _))
        ),
    }
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
