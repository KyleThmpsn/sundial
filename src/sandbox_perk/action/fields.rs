//! Per kind field readers for the mapped parts of an action node.
//!
//! Only fields with a traced role are read. Where the client traces a field's position
//! but not its gameplay name, the label names the position (`First Event Byte`) rather
//! than inventing a meaning. An unmapped kind returns no facts.

use crate::package_payload::{bytes_at, i64_at, native_array_at, relative_offset, u32_at, u64_at};

use super::{Fact, FactValue, MAX_LIST, label_hashes};

const MAX_PATH: usize = 1024;

fn flag(payload: &[u8], node: usize, offset: usize) -> Result<bool, String> {
    Ok(bytes_at::<1>(payload, node + offset)?[0] != 0)
}

fn byte(payload: &[u8], node: usize, offset: usize) -> Result<u8, String> {
    Ok(bytes_at::<1>(payload, node + offset)?[0])
}

fn float(payload: &[u8], node: usize, offset: usize) -> Result<f32, String> {
    Ok(f32::from_bits(u32_at(payload, node + offset)?))
}

fn key(payload: &[u8], node: usize, offset: usize) -> Result<u32, String> {
    u32_at(payload, node + offset)
}

/// A signed 32-bit integer, without a lossy float conversion.
fn integer(payload: &[u8], node: usize, offset: usize) -> Result<i32, String> {
    Ok(u32_at(payload, node + offset)? as i32)
}

/// A package tag lane, or `None` when the lane is empty.
fn tag(payload: &[u8], node: usize, offset: usize) -> Result<Option<u32>, String> {
    let lane = u32_at(payload, node + offset)?;
    Ok((lane != 0 && lane != u32::MAX).then_some(lane))
}

/// A relative pointer that points somewhere other than at itself.
fn pointer_present(payload: &[u8], node: usize, offset: usize) -> Result<bool, String> {
    Ok(i64_at(payload, node + offset)? != 0)
}

/// The four source arrays of 0x808093F3, matched to consumers 57F9B0 and 57FAC0.
/// Array zero includes label groups as well as individual labels. It is not a tree pointer.
const SHORT_FILTER_LISTS: [(&str, usize); 4] = [
    (REQUIRED_LABELS, 0x00),
    ("Requires All Labels", 0x10),
    ("Excludes Any Label", 0x20),
    ("Excludes All Labels Together", 0x30),
];

/// 0x80804C83 starts with two object-reference lists, then the source label filter.
/// Object references are not label hashes. The kill condition's KILL_LABELS is its any list.
const LONG_FILTER_LISTS: [(&str, usize); 4] = [
    (REQUIRED_LABELS, 0x20),
    ("Requires All Labels", 0x30),
    ("Excludes Any Label", 0x40),
    ("Excludes All Labels Together", 0x50),
];

/// At least one label in this list must match. Multiple entries are alternatives.
pub const REQUIRED_LABELS: &str = "Matches Any Label";

/// The same three lists inside a node's second source label filter.
const SECOND_FILTER_LISTS: [(&str, usize); 4] = [
    ("Second Filter Matches Any Label", 0x00),
    ("Second Filter Requires All Labels", 0x10),
    ("Second Filter Excludes Any Label", 0x20),
    ("Second Filter Excludes All Labels Together", 0x30),
];

/// The five lists of an object filter carried beside a source label filter.
const OBJECT_FILTER_LISTS: [(&str, usize); 4] = [
    ("Object Matches Any Label", 0x20),
    ("Object Requires All Labels", 0x30),
    ("Object Excludes Any Label", 0x40),
    ("Object Excludes All Labels Together", 0x50),
];

/// Pushes every nonempty label list of the 80-byte source label filter at `base`.
fn label_filter_facts(payload: &[u8], base: usize, facts: &mut Vec<Fact>) -> Result<(), String> {
    filter_list_facts(payload, base, &SHORT_FILTER_LISTS, facts)
}

/// Pushes every nonempty flat list of the 112-byte target predicate at `base`.
fn target_filter_facts(payload: &[u8], base: usize, facts: &mut Vec<Fact>) -> Result<(), String> {
    filter_list_facts(payload, base, &LONG_FILTER_LISTS, facts)
}

fn filter_list_facts(
    payload: &[u8],
    base: usize,
    lists: &[(&'static str, usize)],
    facts: &mut Vec<Fact>,
) -> Result<(), String> {
    for &(label, offset) in lists {
        let labels = label_hashes(payload, base + offset)?;
        if !labels.is_empty() {
            facts.push(Fact::new(label, FactValue::Labels(labels)));
        }
    }
    Ok(())
}

/// The facts of a plain scalar node through its field map, when the kind has one.
fn layout_facts(
    payload: &[u8],
    node: usize,
    layout: Option<&super::layout::Layout>,
    size: Option<usize>,
) -> Option<Vec<Fact>> {
    let bytes = payload.get(node..node + size?)?;
    Some(layout?.facts(bytes))
}

/// Mapped fields of one condition node.
pub(super) fn condition_facts(payload: &[u8], node: usize, kind: u8) -> Result<Vec<Fact>, String> {
    if let Some(facts) = layout_facts(
        payload,
        node,
        super::layout::condition_layout(kind),
        super::layout::condition_size(kind),
    ) {
        return Ok(facts);
    }
    match kind {
        1 => Ok(vec![Fact::new(
            "Duration",
            FactValue::Seconds(float(payload, node, 0x08)?),
        )]),
        2 => kill_event_facts(payload, node),
        3 => label_and_value_facts(payload, node),
        4 => object_and_numeric_facts(payload, node),
        5 => distance_and_numeric_facts(payload, node),
        6 => Ok(vec![
            Fact::new(
                "First Flag Mask",
                FactValue::Mask(byte(payload, node, 8)?.into()),
            ),
            Fact::new(
                "Second Flag Mask",
                FactValue::Mask(byte(payload, node, 9)?.into()),
            ),
        ]),
        7 | 8 => Ok(vec![
            Fact::new("Slot Mask", FactValue::Mask(byte(payload, node, 8)?.into())),
            Fact::new(
                "Player Filter",
                FactValue::Selector(byte(payload, node, 0x10)?),
            ),
        ]),
        9 | 28 => Ok(vec![Fact::new(
            "Selected Bits",
            FactValue::Mask(key(payload, node, 8)?.into()),
        )]),
        10 | 11 => Ok(vec![Fact::new(
            "Event Key",
            FactValue::Key(key(payload, node, 0x10)?),
        )]),
        12 => Ok(vec![
            Fact::new("Event Value", FactValue::Key(key(payload, node, 8)?)),
            Fact::new("Context Key", FactValue::Key(key(payload, node, 0x0C)?)),
        ]),
        13..=19 => weapon_event_facts(payload, node, kind),
        20 => Ok(vec![Fact::new(
            "Invert Result",
            FactValue::Flag(flag(payload, node, 0xF8)?),
        )]),
        21 => Ok(vec![Fact::new(
            "Scan Related Player State",
            FactValue::Flag(flag(payload, node, 0x158)?),
        )]),
        22 | 24 | 25 | 36 | 42 => Ok(vec![Fact::new(
            "Event Byte",
            FactValue::Selector(byte(payload, node, 8)?),
        )]),
        23 => Ok(vec![
            Fact::new("Event Byte", FactValue::Selector(byte(payload, node, 8)?)),
            Fact::new(
                "Host State Filter",
                FactValue::Selector(byte(payload, node, 9)?),
            ),
            Fact::new(
                "Requires Owning Weapon",
                FactValue::Flag(flag(payload, node, 0x0A)?),
            ),
        ]),
        27 => two_objects_and_event_state_facts(payload, node),
        29 | 30 | 37 => Ok(vec![Fact::new(
            "Event Key",
            FactValue::Key(key(payload, node, 8)?),
        )]),
        32 => {
            let mut facts = Vec::new();
            label_filter_facts(payload, node + 8, &mut facts)?;
            Ok(facts)
        }
        34 => Ok(vec![
            Fact::new("Event Key", FactValue::Key(key(payload, node, 0x10)?)),
            Fact::new(
                "First Range",
                FactValue::Range(float(payload, node, 0x18)?, float(payload, node, 0x1C)?),
            ),
            Fact::new(
                "Second Range",
                FactValue::Range(float(payload, node, 0x20)?, float(payload, node, 0x24)?),
            ),
        ]),
        35 => Ok(vec![Fact::new(
            "Invert Result",
            FactValue::Flag(flag(payload, node, 0xF8)?),
        )]),
        38 => Ok(vec![
            Fact::new("Target Key", FactValue::Key(key(payload, node, 8)?)),
            Fact::new(
                "Minimum Distance",
                FactValue::Number(float(payload, node, 0x0C)?),
            ),
        ]),
        40 => Ok(vec![Fact::new(
            "Named Event Key",
            FactValue::Key(key(payload, node, 8)?),
        )]),
        41 => Ok(vec![
            Fact::new("Slot Mask", FactValue::Mask(byte(payload, node, 8)?.into())),
            Fact::new(
                "Object Filter Selector",
                FactValue::Selector(byte(payload, node, 0x18)?),
            ),
        ]),
        _ => Ok(Vec::new()),
    }
}

pub(super) const KILL_LABELS: usize = 0xD0;
pub(super) const KILL_REQUIRES_WEAPON: usize = 0x141;
/// The kill condition's target predicate, whose flat lists include `KILL_LABELS`.
const KILL_TARGET_FILTER: usize = 0xB0;

fn kill_event_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let mut facts = vec![
        Fact::new(
            REQUIRED_LABELS,
            FactValue::Labels(label_hashes(payload, node + KILL_LABELS)?),
        ),
        Fact::new(
            "Requires Owning Weapon",
            FactValue::Flag(flag(payload, node, KILL_REQUIRES_WEAPON)?),
        ),
    ];
    for (label, offset) in LONG_FILTER_LISTS {
        if KILL_TARGET_FILTER + offset == KILL_LABELS {
            continue;
        }
        let labels = label_hashes(payload, node + KILL_TARGET_FILTER + offset)?;
        if !labels.is_empty() {
            facts.push(Fact::new(label, FactValue::Labels(labels)));
        }
    }
    Ok(facts)
}

/// Kind 3: two source filters, an inclusive value range and an optional weapon key.
fn label_and_value_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let mut facts = Vec::new();
    label_filter_facts(payload, node + 0x08, &mut facts)?;
    filter_list_facts(payload, node + 0xB0, &SECOND_FILTER_LISTS, &mut facts)?;
    facts.push(Fact::new(
        "Value Range",
        FactValue::Range(float(payload, node, 0x110)?, float(payload, node, 0x114)?),
    ));
    facts.push(Fact::new(
        "Requires Owning Weapon",
        FactValue::Flag(flag(payload, node, 0x118)?),
    ));
    let weapon_key = key(payload, node, 0x11C)?;
    if weapon_key != EMPTY_KEY {
        facts.push(Fact::new("Weapon Key", FactValue::Key(weapon_key)));
    }
    Ok(facts)
}

/// FNV-1 of the empty string, the client's empty key marker.
const EMPTY_KEY: u32 = 0x811C_9DC5;

/// Kind 4: the event object filter, a value threshold, a source mask and an optional
/// stateful predicate that keeps per target state between evaluations.
fn object_and_numeric_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let mut facts = Vec::new();
    target_filter_facts(payload, node + 0x08, &mut facts)?;
    facts.push(Fact::new(
        "Value Threshold",
        FactValue::Number(float(payload, node, 0xA0)?),
    ));
    let named = key(payload, node, 0x9C)?;
    if named != EMPTY_KEY {
        facts.push(Fact::new("Named Key", FactValue::Key(named)));
    }
    facts.push(Fact::new(
        "Object Filter Selector",
        FactValue::Selector(byte(payload, node, 0xA8)?),
    ));
    facts.push(Fact::new(
        "Source Mask",
        FactValue::Mask(byte(payload, node, 0xC1)?.into()),
    ));
    facts.push(Fact::new(
        "Stateful Predicate",
        FactValue::Flag(pointer_present(payload, node, 0xC8)?),
    ));
    Ok(facts)
}

/// Kind 5: the event object filter, a distance limit and a numeric threshold.
fn distance_and_numeric_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let mut facts = Vec::new();
    target_filter_facts(payload, node + 0x08, &mut facts)?;
    facts.push(Fact::new(
        "Value Threshold",
        FactValue::Number(float(payload, node, 0x98)?),
    ));
    let named = key(payload, node, 0xA0)?;
    if named != EMPTY_KEY {
        facts.push(Fact::new("Named Key", FactValue::Key(named)));
    }
    let distance = float(payload, node, 0xA4)?;
    if distance >= 0.0 {
        facts.push(Fact::new("Maximum Distance", FactValue::Number(distance)));
    }
    facts.push(Fact::new(
        "First Event Mask",
        FactValue::Mask(byte(payload, node, 0xB8)?.into()),
    ));
    facts.push(Fact::new(
        "Second Event Mask",
        FactValue::Mask(byte(payload, node, 0xB9)?.into()),
    ));
    Ok(facts)
}

/// Kind 27: owner and slot restrictions, two event bytes and a mode selector around the
/// shared source label filter.
fn two_objects_and_event_state_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let mut facts = vec![
        Fact::new(
            "Requires Owning Weapon",
            FactValue::Flag(flag(payload, node, 8)?),
        ),
        Fact::new("Slot Mask", FactValue::Mask(byte(payload, node, 9)?.into())),
        Fact::new(
            "First Event Byte",
            FactValue::Selector(byte(payload, node, 0x0C)?),
        ),
        Fact::new(
            "Second Event Byte",
            FactValue::Selector(byte(payload, node, 0x0D)?),
        ),
    ];
    label_filter_facts(payload, node + 0x10, &mut facts)?;
    facts.push(Fact::new(
        "Object Filter Selector",
        FactValue::Selector(byte(payload, node, 0x80)?),
    ));
    facts.push(Fact::new(
        "Mode",
        FactValue::Selector(byte(payload, node, 0x90)?),
    ));
    Ok(facts)
}

/// Kinds 13 through 19 share one layout: an owning-weapon flag, a slot mask and the
/// source label filter at `+0x10`.
fn weapon_event_facts(payload: &[u8], node: usize, kind: u8) -> Result<Vec<Fact>, String> {
    let mut facts = vec![Fact::new(
        "Requires Owning Weapon",
        FactValue::Flag(flag(payload, node, 8)?),
    )];
    // Kind 19 carries two event flags before its slot mask and a window in seconds after it.
    // The other weapon events keep the slot mask right after the owning-weapon flag.
    if kind == 19 {
        facts.push(Fact::new(
            "Time Restriction",
            FactValue::Seconds(float(payload, node, 0x0C)?),
        ));
    }
    let slot_mask = byte(payload, node, if kind == 19 { 0x0B } else { 0x09 })?;
    if slot_mask != 0 {
        facts.push(Fact::new("Slot Mask", FactValue::Mask(slot_mask.into())));
    }
    label_filter_facts(payload, node + 0x10, &mut facts)?;
    Ok(facts)
}

/// Mapped fields of one effect node.
pub(super) fn effect_facts(payload: &[u8], node: usize, kind: u8) -> Result<Vec<Fact>, String> {
    if let Some(facts) = layout_facts(
        payload,
        node,
        super::layout::effect_layout(kind),
        super::layout::effect_size(kind),
    ) {
        return Ok(facts);
    }
    match kind {
        1 => create_entity_facts(payload, node),
        2 => Ok(vec![
            Fact::new(
                "Input Selector",
                FactValue::Selector(byte(payload, node, 0x50)?),
            ),
            Fact::new(
                "Normalize Input",
                FactValue::Flag(flag(payload, node, 0x51)?),
            ),
        ]),
        3 => Ok(vec![Fact::new(
            "Position Selector",
            FactValue::Selector(byte(payload, node, 4)?),
        )]),
        4 => Ok(vec![Fact::new(
            "Target Selector",
            FactValue::Selector(byte(payload, node, 2)?),
        )]),
        5 => spawn_and_player_operation_facts(payload, node),
        6 => Ok(vec![
            Fact::new("Mode", FactValue::Selector(byte(payload, node, 2)?)),
            Fact::new(
                "Keep After Removal",
                FactValue::Flag(flag(payload, node, 3)?),
            ),
        ]),
        7 | 25 | 41 | 43 | 47 => Ok(vec![Fact::new(
            "Property Key",
            FactValue::Key(key(payload, node, 4)?),
        )]),
        8 => component_value_adjustment_facts(payload, node),
        10 => named_property_facts(payload, node),
        11 => host_numeric_modifier_facts(payload, node),
        13 => Ok(vec![
            Fact::new("Owner Path", FactValue::Flag(flag(payload, node, 3)?)),
            Fact::new(
                "Related Player Path",
                FactValue::Flag(flag(payload, node, 4)?),
            ),
            // The three categories are the ammo types, established by the stock perks that
            // weight exactly one (Snapload, Special and Heavy Finisher among them).
            Fact::new(
                "Primary Ammo Weight",
                FactValue::Number(float(payload, node, 0x20)?),
            ),
            Fact::new(
                "Special Ammo Weight",
                FactValue::Number(float(payload, node, 0x2C)?),
            ),
            Fact::new(
                "Heavy Ammo Weight",
                FactValue::Number(float(payload, node, 0x38)?),
            ),
        ]),
        14 => fixed_ammunition_facts(payload, node),
        15 => proportional_ammunition_facts(payload, node),
        16 => reserve_transfer_facts(payload, node),
        18 | 29 => Ok(vec![
            Fact::new("First Value", FactValue::Number(float(payload, node, 4)?)),
            Fact::new("Second Value", FactValue::Number(float(payload, node, 8)?)),
            Fact::new(
                "Third Value",
                FactValue::Number(float(payload, node, 0x0C)?),
            ),
        ]),
        20 | 30 | 51 => Ok(vec![Fact::new(
            "Operation",
            FactValue::Selector(byte(payload, node, 2)?),
        )]),
        22 => Ok(vec![
            Fact::new("Slot Mask", FactValue::Mask(byte(payload, node, 2)?.into())),
            Fact::new("Value", FactValue::Selector(byte(payload, node, 4)?)),
        ]),
        28 => Ok(vec![
            Fact::new("First State", FactValue::Selector(byte(payload, node, 2)?)),
            Fact::new("Second State", FactValue::Selector(byte(payload, node, 3)?)),
        ]),
        32 => Ok(vec![
            Fact::new("Extend By", FactValue::Seconds(float(payload, node, 4)?)),
            Fact::new("Up To", FactValue::Seconds(float(payload, node, 8)?)),
            Fact::new(
                EXTEND_TIMERS_MASK_LABEL,
                FactValue::Mask(u64_at(payload, node + EXTEND_TIMERS_MASK)?),
            ),
        ]),
        33 => register_host_modifier_facts(payload, node),
        35 => Ok(vec![
            Fact::new(
                "Target Selector",
                FactValue::Selector(byte(payload, node, 2)?),
            ),
            Fact::new(
                "Interface Selector",
                FactValue::Selector(byte(payload, node, 3)?),
            ),
            Fact::new("Replacement Key", FactValue::Key(key(payload, node, 4)?)),
        ]),
        37 => Ok(vec![Fact::new(
            ADDED_LABELS,
            FactValue::Labels(label_hashes(payload, node + 0x98)?),
        )]),
        36 => Ok(vec![Fact::new(
            "Typed Record Present",
            FactValue::Flag(pointer_present(payload, node, 8)?),
        )]),
        40 => event_numeric_modifier_facts(payload, node),
        42 => Ok(vec![
            Fact::new("Mode", FactValue::Selector(byte(payload, node, 2)?)),
            Fact::new("Value", FactValue::Number(float(payload, node, 4)?)),
        ]),
        48 => referenced_runtime_operation_facts(payload, node),
        49 => Ok(vec![
            Fact::new(
                "Target Selector",
                FactValue::Selector(byte(payload, node, 2)?),
            ),
            Fact::new("Binding Key", FactValue::Key(key(payload, node, 4)?)),
        ]),
        52 => Ok(vec![
            Fact::new("Player Key", FactValue::Key(key(payload, node, 4)?)),
            Fact::new("Added Value", FactValue::Number(float(payload, node, 8)?)),
        ]),
        53 => adjust_named_component_facts(payload, node),
        54 => Ok(vec![
            Fact::new(
                ADDED_LABELS,
                FactValue::Labels(label_hashes(payload, node + 0x68)?),
            ),
            Fact::new("Event Flag", FactValue::Flag(flag(payload, node, 0xA0)?)),
        ]),
        _ => Ok(Vec::new()),
    }
}

/// Label of the source label list an event-label effect unions into the event.
pub const ADDED_LABELS: &str = "Added Labels";

pub(crate) const CREATE_ENTITY_MODE: usize = 0x02;
pub(crate) const CREATE_ENTITY_KEYS: [usize; 2] = [0x18, 0x1C];
pub(crate) const CREATE_ENTITY_FLOATS: [usize; 4] = [0x20, 0x24, 0x28, 0x2C];

/// Label of the attachment mode byte stored at `+0x02` of a Create Entity node.
pub const CREATE_ENTITY_MODE_LABEL: &str = "Attachment Mode";
/// Labels of the two keys stored at `+0x18` and `+0x1C` of a Create Entity node.
pub const CREATE_ENTITY_KEY_LABELS: [&str; 2] = ["First Key", "Second Key"];
/// Labels of the four floats stored at `+0x20` through `+0x2C` of a Create Entity node.
pub const CREATE_ENTITY_FLOAT_LABELS: [&str; 4] =
    ["First Float", "Second Float", "Third Float", "Fourth Float"];

/// The stored fields of a Create Entity node whose bytes the compiler reproduces verbatim.
///
/// The offsets are measured from the stock data. Their gameplay roles are not traced, so
/// the labels name the position of each field rather than a meaning.
fn create_entity_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let mut facts = vec![Fact::new(
        CREATE_ENTITY_MODE_LABEL,
        FactValue::Selector(byte(payload, node, CREATE_ENTITY_MODE)?),
    )];
    for (label, offset) in CREATE_ENTITY_KEY_LABELS.into_iter().zip(CREATE_ENTITY_KEYS) {
        facts.push(Fact::new(
            label,
            FactValue::Key(key(payload, node, offset)?),
        ));
    }
    for (label, offset) in CREATE_ENTITY_FLOAT_LABELS
        .into_iter()
        .zip(CREATE_ENTITY_FLOATS)
    {
        facts.push(Fact::new(
            label,
            FactValue::Number(float(payload, node, offset)?),
        ));
    }
    Ok(facts)
}

/// Kind 5: a positioned spawn operation with a count, a value and an optional resource.
fn spawn_and_player_operation_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let mut facts = vec![
        Fact::new(
            "Position Selector",
            FactValue::Selector(byte(payload, node, 2)?),
        ),
        Fact::new("Count", FactValue::Count(u32_at(payload, node + 4)?.into())),
        Fact::new("Value", FactValue::Number(float(payload, node, 8)?)),
    ];
    if let Some(resource) = tag(payload, node, 0x18)? {
        facts.push(Fact::new("Resource", FactValue::Tag(resource)));
    }
    Ok(facts)
}

/// Kind 8: a component value scaled by a value program, with an optional limit.
fn component_value_adjustment_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let mut facts = vec![
        Fact::new(
            "Target Selector",
            FactValue::Selector(byte(payload, node, 2)?),
        ),
        Fact::new("Flag Byte", FactValue::Selector(byte(payload, node, 3)?)),
        Fact::new("Option Byte", FactValue::Selector(byte(payload, node, 4)?)),
        Fact::new("Scale", FactValue::Number(float(payload, node, 8)?)),
        Fact::new("Limit", FactValue::Number(float(payload, node, 0x0C)?)),
        Fact::new(
            "Input Selector",
            FactValue::Selector(byte(payload, node, 0x48)?),
        ),
    ];
    facts.push(program_fact(
        payload,
        node + 0x18,
        CONSTANT_VALUE,
        PROGRAM_WORDS,
    ));
    Ok(facts)
}

/// Kind 11: two shared floats and three slot specific triples, added on activation and
/// subtracted on removal.
fn host_numeric_modifier_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    // The three triples are per ammo type, established by the three Finder mods.
    const LABELS: [&str; 11] = [
        "Shared Value 1",
        "Shared Value 2",
        "Primary Ammo Value 1",
        "Primary Ammo Value 2",
        "Primary Ammo Value 3",
        "Special Ammo Value 1",
        "Special Ammo Value 2",
        "Special Ammo Value 3",
        "Heavy Ammo Value 1",
        "Heavy Ammo Value 2",
        "Heavy Ammo Value 3",
    ];
    LABELS
        .into_iter()
        .enumerate()
        .map(|(index, label)| {
            Ok(Fact::new(
                label,
                FactValue::Number(float(payload, node, 4 + index * 4)?),
            ))
        })
        .collect()
}

/// The seven contributions the two ammunition adjustment kinds store at `+0x6C`.
const AMMUNITION_CONTRIBUTIONS: [&str; 7] = [
    "Owning Slot Amount",
    "Slot 1 Amount",
    "Slot 2 Amount",
    "Slot 3 Amount",
    "Category 1 Amount",
    "Category 2 Amount",
    "Category 3 Amount",
];

/// Kind 14: signed integer contributions and the selectors around them.
fn fixed_ammunition_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let mut facts = Vec::new();
    label_filter_facts(payload, node + 0x08, &mut facts)?;
    facts.push(Fact::new(
        "Storage Path",
        FactValue::Selector(byte(payload, node, 0x68)?),
    ));
    facts.push(Fact::new(
        "Allow Magazine Overflow",
        FactValue::Flag(flag(payload, node, 0x69)?),
    ));
    facts.push(Fact::new(
        "Scale by Ammunition Unit",
        FactValue::Flag(flag(payload, node, 0x6A)?),
    ));
    facts.push(Fact::new(
        "Scale by Action Value",
        FactValue::Flag(flag(payload, node, 0x6B)?),
    ));
    for (index, label) in AMMUNITION_CONTRIBUTIONS.into_iter().enumerate() {
        let value = integer(payload, node, 0x6C + index * 4)?;
        if value != 0 {
            facts.push(Fact::new(label, FactValue::Integer(value)));
        }
    }
    Ok(facts)
}

/// Kind 15: float contributions scaled by a selected capacity.
fn proportional_ammunition_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let mut facts = Vec::new();
    label_filter_facts(payload, node + 0x08, &mut facts)?;
    facts.push(Fact::new(
        "Destination",
        FactValue::Selector(byte(payload, node, 0x68)?),
    ));
    facts.push(Fact::new(
        "Allow Magazine Overflow",
        FactValue::Flag(flag(payload, node, 0x69)?),
    ));
    facts.push(Fact::new(
        "Capacity Source",
        FactValue::Selector(byte(payload, node, 0x6A)?),
    ));
    facts.push(Fact::new(
        "Scale by Action Value",
        FactValue::Flag(flag(payload, node, 0x6B)?),
    ));
    for (index, label) in AMMUNITION_CONTRIBUTIONS.into_iter().enumerate() {
        let value = float(payload, node, 0x6C + index * 4)?;
        if value != 0.0 {
            facts.push(Fact::new(label, FactValue::Number(value)));
        }
    }
    Ok(facts)
}

/// Kind 16's independent value programs, in native field order.
pub const RESERVE_TRANSFER_PROGRAMS: [(usize, &str, &str); 5] = [
    (0x78, "Selected Slot Value", "Selected Slot Program Words"),
    (
        0xB0,
        "Second Selected Slot Value",
        "Second Selected Slot Program Words",
    ),
    (0xE8, "Slot 0 Value", "Slot 0 Program Words"),
    (0x120, "Slot 1 Value", "Slot 1 Program Words"),
    (0x158, "Slot 2 Value", "Slot 2 Program Words"),
];

/// Kind 16: five value programs selected per slot, plus the transfer selectors.
fn reserve_transfer_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let mut facts = Vec::new();
    label_filter_facts(payload, node + 0x08, &mut facts)?;
    facts.push(Fact::new(
        "Allow Magazine Overflow",
        FactValue::Flag(flag(payload, node, 0x68)?),
    ));
    facts.push(Fact::new(
        "Capacity Basis",
        FactValue::Selector(byte(payload, node, 0x69)?),
    ));
    facts.push(Fact::new(
        "Publish Slot Event",
        FactValue::Flag(flag(payload, node, 0x6A)?),
    ));
    facts.push(Fact::new(
        "Input Selector",
        FactValue::Selector(byte(payload, node, 0x6B)?),
    ));
    facts.push(Fact::new(
        "Normalize Input",
        FactValue::Flag(flag(payload, node, 0x6C)?),
    ));
    for (offset, value_label, words_label) in RESERVE_TRANSFER_PROGRAMS {
        facts.push(program_fact(
            payload,
            node + offset,
            value_label,
            words_label,
        ));
    }
    Ok(facts)
}

/// Kind 33: the modifier value, its input selector and limit, and the target predicate.
fn register_host_modifier_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let mut facts = vec![
        Fact::new(
            "Modifier Value",
            FactValue::Number(float(payload, node, 4)?),
        ),
        Fact::new(
            "Input Selector",
            FactValue::Selector(byte(payload, node, 8)?),
        ),
    ];
    let limit = float(payload, node, 0x0C)?;
    if limit >= 0.0 {
        facts.push(Fact::new("Modifier Limit", FactValue::Number(limit)));
    }
    target_filter_facts(payload, node + 0x10, &mut facts)?;
    Ok(facts)
}

/// Row class of an event numeric modifier assignment or multiplication row.
const EVENT_MODIFIER_ROW_CLASS: u32 = 0x8080_3E22;
const EVENT_MODIFIER_ROW_SIZE: usize = 12;
/// Selector byte value that marks a literal row value rather than a native stat.
const LITERAL_VALUE: u8 = 0xFF;

const ASSIGN_LABELS: [&str; 4] = [
    "Assign Slot 0",
    "Assign Slot 1",
    "Assign Slot 2",
    "Assign Slot 3",
];
const ASSIGN_STAT_LABELS: [&str; 4] = [
    "Assign Slot 0 From Stat",
    "Assign Slot 1 From Stat",
    "Assign Slot 2 From Stat",
    "Assign Slot 3 From Stat",
];
const MULTIPLY_LABELS: [&str; 4] = [
    "Multiply Slot 0",
    "Multiply Slot 1",
    "Multiply Slot 2",
    "Multiply Slot 3",
];
const MULTIPLY_STAT_LABELS: [&str; 4] = [
    "Multiply Slot 0 From Stat",
    "Multiply Slot 1 From Stat",
    "Multiply Slot 2 From Stat",
    "Multiply Slot 3 From Stat",
];

/// Kind 40: the source label filter, the assigned and multiplied event slots and whether
/// a further scalar expression is attached.
fn event_numeric_modifier_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let mut facts = Vec::new();
    filter_list_facts(payload, node + 0x28, &OBJECT_FILTER_LISTS, &mut facts)?;
    label_filter_facts(payload, node + 0xC0, &mut facts)?;
    event_modifier_rows(
        payload,
        node + 0x120,
        &ASSIGN_LABELS,
        &ASSIGN_STAT_LABELS,
        &mut facts,
    )?;
    event_modifier_rows(
        payload,
        node + 0x130,
        &MULTIPLY_LABELS,
        &MULTIPLY_STAT_LABELS,
        &mut facts,
    )?;
    event_scalar_facts(payload, node, &mut facts)?;
    Ok(facts)
}

/// 108ED90 selects a literal pair or evaluates a program, then multiplies the event
/// scalar by 1 + that result. A nonpositive alternate literal falls back to the default.
fn event_scalar_facts(payload: &[u8], node: usize, facts: &mut Vec<Fact>) -> Result<(), String> {
    let pointer = node + 0x140;
    let relative = i64_at(payload, pointer)?;
    if relative == 0 {
        return Ok(());
    }
    let value = relative_offset(pointer, 0, relative)?;
    let class = u32_at(
        payload,
        value
            .checked_sub(4)
            .ok_or("Invalid event scalar reference")?,
    )?;
    match class {
        0x80802F1A => {
            let default = float(payload, value, 0)?;
            let alternate = float(payload, value, 4)?;
            facts.push(Fact::new(
                "Default Scalar Adjustment",
                FactValue::Number(default),
            ));
            facts.push(Fact::new(
                "Default Scalar Multiplier",
                FactValue::Number(1.0 + default),
            ));
            facts.push(Fact::new(
                "Alternate Scalar Adjustment",
                FactValue::Number(alternate),
            ));
            if alternate > 0.0 {
                facts.push(Fact::new(
                    "Alternate Scalar Multiplier",
                    FactValue::Number(1.0 + alternate),
                ));
            }
        }
        0x80802F18 => {
            facts.push(Fact::new(
                "Scalar Input Source",
                FactValue::Selector(byte(payload, value, 0x38)?),
            ));
            facts.push(Fact::new(
                "Normalize Scalar Input",
                FactValue::Flag(flag(payload, value, 0x39)?),
            ));
            if let Some(constant) = constant_program_value(payload, value + 8) {
                facts.push(Fact::new(
                    "Default Scalar Adjustment",
                    FactValue::Number(constant),
                ));
                facts.push(Fact::new(
                    "Default Scalar Multiplier",
                    FactValue::Number(1.0 + constant),
                ));
            }
        }
        _ => facts.push(Fact::new("Scalar Expression Type", FactValue::Key(class))),
    }
    if flag(payload, node, 0x148)? {
        facts.push(Fact::new("Uses Ability Scalar Cap", FactValue::Flag(true)));
    }
    Ok(())
}

fn event_modifier_rows(
    payload: &[u8],
    descriptor: usize,
    literal_labels: &[&'static str; 4],
    stat_labels: &[&'static str; 4],
    facts: &mut Vec<Fact>,
) -> Result<(), String> {
    if u64_at(payload, descriptor)? == 0 && i64_at(payload, descriptor + 8)? == 0 {
        return Ok(());
    }
    let (count, _, rows, class) = native_array_at(payload, descriptor)?;
    if class != EVENT_MODIFIER_ROW_CLASS || count > MAX_LIST {
        return Ok(());
    }
    for index in 0..count {
        let row = rows + index * EVENT_MODIFIER_ROW_SIZE;
        let slot = usize::try_from(u32_at(payload, row)?).unwrap_or(usize::MAX);
        let selector = bytes_at::<1>(payload, row + 8)?[0];
        let Some(literal) = literal_labels.get(slot) else {
            continue;
        };
        if selector == LITERAL_VALUE {
            facts.push(Fact::new(
                literal,
                FactValue::Number(f32::from_bits(u32_at(payload, row + 4)?)),
            ));
        } else {
            facts.push(Fact::new(stat_labels[slot], FactValue::Selector(selector)));
        }
    }
    Ok(())
}

/// Kind 48: three target selectors and a path/tag resource reference.
/// The declaration marks +8 as a relative string pointer, not a numeric operation.
fn referenced_runtime_operation_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let mut facts = vec![
        Fact::new(
            "First Target Selector",
            FactValue::Selector(byte(payload, node, 2)?),
        ),
        Fact::new(
            "Second Target Selector",
            FactValue::Selector(byte(payload, node, 3)?),
        ),
        Fact::new(
            "Third Target Selector",
            FactValue::Selector(byte(payload, node, 4)?),
        ),
        Fact::new(
            "Has Resource Path",
            FactValue::Flag(reference_path(payload, node).is_some()),
        ),
    ];
    if let Some(resource) = tag(payload, node, 0x10)? {
        facts.push(Fact::new("Runtime Resource", FactValue::Tag(resource)));
    }
    Ok(facts)
}

/// Kind 53: the named component list, its operation, cap and value program.
fn adjust_named_component_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let mut facts = Vec::new();
    let names = u64_at(payload, node + 8)?;
    facts.push(Fact::new("Component Names", FactValue::Count(names)));
    if names != 0 && names <= MAX_LIST as u64 {
        let (_, _, rows, _) = native_array_at(payload, node + 8)?;
        facts.push(Fact::new(
            "First Component Name",
            FactValue::Key(u32_at(payload, rows)?),
        ));
    }
    facts.push(Fact::new(
        "Operation",
        FactValue::Selector(byte(payload, node, 0x5A)?),
    ));
    facts.push(Fact::new(
        "Upper Cap",
        FactValue::Number(float(payload, node, 0x18)?),
    ));
    facts.push(program_fact(
        payload,
        node + 0x28,
        CONSTANT_VALUE,
        PROGRAM_WORDS,
    ));
    Ok(facts)
}

/// Label of a value program that is the surveyed constant shape.
pub const CONSTANT_VALUE: &str = "Constant Value";
/// Label of a value program that is any other shape, reported by its code word count.
pub const PROGRAM_WORDS: &str = "Program Words";

/// The constant a value program pushes, or its code word count when it is not the
/// constant shape. A program the reader cannot follow at all reports zero words.
fn program_fact(
    payload: &[u8],
    program: usize,
    value_label: &'static str,
    words_label: &'static str,
) -> Fact {
    match constant_program_value(payload, program) {
        Some(value) => Fact::new(value_label, FactValue::Number(value)),
        None => Fact::new(
            words_label,
            FactValue::Count(u64_at(payload, program).unwrap_or(0)),
        ),
    }
}

/// Offset of the compiled event mask that routes events to an Extend Timers nested list.
pub(crate) const EXTEND_TIMERS_MASK: usize = 0x20;
/// Label of that mask.
pub const EXTEND_TIMERS_MASK_LABEL: &str = "Nested Event Mask";

const NAMED_PROPERTY_TARGET: usize = 0x02;
const NAMED_PROPERTY_FLAG: usize = 0x03;
const NAMED_PROPERTY_KEY: usize = 0x08;
const NAMED_PROPERTY_ABILITY_MASK: usize = 0x04;
const NAMED_PROPERTY_PROGRAM: usize = 0x18;
const NAMED_PROPERTY_INPUT: usize = 0x48;
const NAMED_PROPERTY_OPERATION: usize = 0x49;
const NAMED_PROPERTY_REMOVAL: usize = 0x4A;
const NAMED_PROPERTY_RESTORE: usize = 0x4C;

/// Labels of the Named Property facts the program model carries.
pub const NAMED_PROPERTY_LABELS: NamedPropertyLabels = NamedPropertyLabels {
    key: "Property Key",
    target: "Target Selector",
    flag: "Flag Byte",
    ability_mask: "Ability Slot Mask",
    input: "Input Selector",
    operation: "Operation",
    removal: "Removal Policy",
    restore: "Removal Value",
    value: CONSTANT_VALUE,
};

/// The fact labels of a Named Property node, named by position rather than by meaning.
pub struct NamedPropertyLabels {
    pub key: &'static str,
    pub target: &'static str,
    pub flag: &'static str,
    pub ability_mask: &'static str,
    pub input: &'static str,
    pub operation: &'static str,
    pub removal: &'static str,
    pub restore: &'static str,
    pub value: &'static str,
}

fn named_property_facts(payload: &[u8], node: usize) -> Result<Vec<Fact>, String> {
    let labels = &NAMED_PROPERTY_LABELS;
    let mut facts = vec![
        Fact::new(
            labels.key,
            FactValue::Key(key(payload, node, NAMED_PROPERTY_KEY)?),
        ),
        Fact::new(
            labels.target,
            FactValue::Selector(byte(payload, node, NAMED_PROPERTY_TARGET)?),
        ),
        Fact::new(
            labels.flag,
            FactValue::Selector(byte(payload, node, NAMED_PROPERTY_FLAG)?),
        ),
        Fact::new(
            labels.ability_mask,
            FactValue::Mask(key(payload, node, NAMED_PROPERTY_ABILITY_MASK)?.into()),
        ),
        Fact::new(
            labels.input,
            FactValue::Selector(byte(payload, node, NAMED_PROPERTY_INPUT)?),
        ),
        Fact::new(
            labels.operation,
            FactValue::Selector(byte(payload, node, NAMED_PROPERTY_OPERATION)?),
        ),
        Fact::new(
            labels.removal,
            FactValue::Selector(byte(payload, node, NAMED_PROPERTY_REMOVAL)?),
        ),
        Fact::new(
            labels.restore,
            FactValue::Number(float(payload, node, NAMED_PROPERTY_RESTORE)?),
        ),
    ];
    facts.push(program_fact(
        payload,
        node + NAMED_PROPERTY_PROGRAM,
        labels.value,
        PROGRAM_WORDS,
    ));
    Ok(facts)
}

/// The value a compiled value program pushes when it is the surveyed constant shape:
/// bytecode `34 00 3E 00` and one constant vector whose four lanes agree.
///
/// Any other program returns `None`. The program model only authors the constant shape.
pub fn constant_program_value(payload: &[u8], program: usize) -> Option<f32> {
    let code_count = u64_at(payload, program).ok()?;
    let constant_count = u64_at(payload, program + 0x10).ok()?;
    if code_count != 4 || constant_count != 1 || u32_at(payload, program + 0x2C).ok()? != 0 {
        return None;
    }
    let code = relative_offset(program + 8, 0, i64_at(payload, program + 8).ok()?).ok()? + 16;
    if payload.get(code..code + 4)? != [0x34, 0x00, 0x3E, 0x00] {
        return None;
    }
    let constants =
        relative_offset(program + 0x18, 0, i64_at(payload, program + 0x18).ok()?).ok()? + 16;
    let lanes = (0..4)
        .map(|lane| u32_at(payload, constants + lane * 4))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    lanes
        .iter()
        .all(|bits| *bits == lanes[0])
        .then(|| f32::from_bits(lanes[0]))
}

const REFERENCE_PATH: usize = 0x08;
const REFERENCE_TAG: usize = 0x10;

/// Entity graph or pattern referenced by an effect, with its native debug path.
pub(super) fn effect_reference(
    payload: &[u8],
    node: usize,
    kind: u8,
) -> Result<(Option<u32>, Option<String>), String> {
    if !matches!(kind, 1 | 2 | 3 | 4 | 13 | 26) {
        return Ok((None, None));
    }
    let lane = u64_at(payload, node + REFERENCE_TAG)?;
    let tag = u32::try_from(lane).ok().filter(|tag| *tag != u32::MAX);
    let Some(tag) = tag.filter(|tag| *tag != 0) else {
        return Ok((None, None));
    };
    Ok((Some(tag), reference_path(payload, node)))
}

fn reference_path(payload: &[u8], node: usize) -> Option<String> {
    let pointer = node + REFERENCE_PATH;
    let relative = i64_at(payload, pointer).ok()?;
    if relative == 0 {
        return None;
    }
    let start = relative_offset(pointer, 0, relative).ok()?;
    let tail = payload.get(start..)?;
    let end = tail.iter().take(MAX_PATH).position(|byte| *byte == 0)?;
    let text = std::str::from_utf8(&tail[..end]).ok()?;
    let usable = text.len() >= 4
        && text.contains('.')
        && text
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' ');
    usable.then(|| text.to_owned())
}
