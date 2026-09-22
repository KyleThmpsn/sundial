//! Byte-complete native field views with recovered names where the contract is known.
use super::{Block, schema};
use crate::sandbox_perk::{action::layout, nodes};

pub mod fixed_values;
pub mod keys;
pub mod scripts;
pub mod stock_values;
mod values;
pub use values::{ValueContract, contract};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    Byte,
    Flag,
    Float,
    Integer,
    Unsigned,
    Mask32,
    Key,
    Tag,
    Bytes,
    Pointer,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Field {
    pub offset: usize,
    pub width: usize,
    pub label: String,
    pub format: Format,
    pub editable: bool,
}

pub fn name(class: u32) -> String {
    if let Some(node) = nodes::CONDITIONS
        .iter()
        .chain(&nodes::EFFECTS)
        .find(|n| n.class == class && class != 0)
    {
        return node.name.to_owned();
    }
    match class {
        0 => "Native Path",
        0x808040B5 => "Action Program",
        0x8080407D => "Action Group",
        0x808040BA => "Condition",
        0x808040AC => "Effect",
        0x80803E32 => "Accumulator Contribution",
        0x80803E06 => "Condition Subgroup",
        0x80803E22 => "Event Numeric Modifier Entry",
        0x808093F3 => "Label Filter",
        0x808094A8 => "Runtime Label Predicate",
        0x808093F5 | 0x808093F6 => "Compiled Label Masks",
        0x808094B3 => "Label",
        0x80808E76 => "Value Program",
        0x80800009 => "Instruction Bytes",
        0x80800090 => "Constant Vector",
        0x80803E02 => "Object Filter",
        0x80804C81 => "Ability Filter",
        0x80804C83 => "Target Filter",
        0x80802F18 => "Program Value",
        0x80802F1A => "Event Value Pair",
        // Names the client itself uses. Recovered 2026-09-17 by scanning the running offline
        // client's memory for identifier strings and matching their FNV-1 hashes against the
        // member-name hashes the perk schema declares. The on-disk binary is VMProtect packed
        // so the strings exist only once the loader has unpacked them. `filter` and `trigger`
        // were already known and served as the controls, and every match sits where its name
        // makes sense: bytecode beside constant_buffer is the value program the decompiler
        // already models as instructions plus a constant pool.
        0x808073F4 => "Compiled Expression",
        0x80809419 => "Expression Binding",
        // Program Value's `function` points here and its own member is `m_data`, so this is
        // the function's payload record. Recovered from the client's UTF-16 strings.
        0x80809CC6 => "Function Data",
        0x80804D78 => "Faction Filter",
        0x80809312 => "Filter List",
        0x80809316 => "Filter Entry",
        _ => return format!("Native Record 0x{class:08X}"),
    }
    .to_owned()
}

/// Every byte belongs to exactly one field, including unnamed native values.
pub fn describe(class: u32) -> Result<Vec<Field>, String> {
    let record = schema::record(class)?;
    let mut fields = Vec::new();
    let mut covered = vec![false; record.size];
    for &(offset, code) in &record.fields {
        schema_field(&mut fields, &mut covered, offset, code);
    }
    for (offset, child, _) in schema::inline(class)? {
        if schema::record(child)?.array {
            insert(
                &mut fields,
                &mut covered,
                offset,
                8,
                Format::Bytes,
                "Entry Count",
                false,
            );
        }
    }
    headers(class, &mut fields, &mut covered);
    mapped(class, &mut fields, &mut covered);
    for (offset, width, format, label) in known(class) {
        insert(
            &mut fields,
            &mut covered,
            offset,
            width,
            format,
            label,
            true,
        );
    }
    // A verified inline member has the same storage contract in every owner.
    // Root mappings take precedence, and pointer/count metadata remains protected.
    for (base, child, _) in schema::inline(class)? {
        if child == class {
            continue;
        }
        for (offset, width, format, label) in known(child) {
            insert(
                &mut fields,
                &mut covered,
                base + offset,
                width,
                format,
                label,
                true,
            );
        }
    }
    let mut at = 0;
    while at < covered.len() {
        if covered[at] {
            at += 1;
            continue;
        }
        // Native records are word aligned, so a gap is chunked on four-byte boundaries. A
        // gap that opens after a packed byte first takes the bytes up to the next boundary,
        // otherwise every later chunk would straddle two real fields.
        let to_boundary = 4 - at % 4;
        let width = (1..=to_boundary.min(covered.len() - at))
            .take_while(|width| !covered[at + width - 1])
            .last()
            .unwrap_or(1);
        let format = if width == 1 {
            Format::Byte
        } else {
            Format::Bytes
        };
        // A byte range every stock node of the class stores identically is locked at that
        // value and labelled as fixed. Its role is still unresolved, but a reader can tell it
        // apart from a setting that varies.
        let fixed = fixed_values::fixed(class, at).is_some_and(|value| value.len() == width);
        let label = if fixed {
            format!("Fixed Native Value +0x{at:02X}")
        } else {
            format!("Native Value +0x{at:02X}")
        };
        insert(&mut fields, &mut covered, at, width, format, &label, !fixed);
        at += width;
    }
    fields.sort_by_key(|field| field.offset);
    Ok(fields)
}

fn schema_field(fields: &mut Vec<Field>, covered: &mut [bool], offset: usize, code: u32) {
    if code == 9 {
        // A typed resource reference is an owner, native type and 64-bit
        // offset. Editing one lane independently can redirect a callback to
        // a different object. Package relocation owns all three lanes.
        for (at, width, format, label) in [
            (offset, 4, Format::Tag, "Resource"),
            (offset + 4, 4, Format::Unsigned, "Resource Type"),
            (offset + 8, 8, Format::Bytes, "Resource Offset"),
        ] {
            insert(fields, covered, at, width, format, label, false);
        }
        return;
    }
    let (width, format, label) = match code {
        1..=3 => (8, Format::Pointer, "Reference"),
        4 => (4, Format::Tag, "Resource"),
        5 => (8, Format::Bytes, "Native Type"),
        11 => (4, Format::Key, "Key"),
        _ => return,
    };
    insert(
        fields,
        covered,
        offset,
        width,
        format,
        label,
        !matches!(code, 1..=3 | 5),
    );
}

fn insert(
    fields: &mut Vec<Field>,
    covered: &mut [bool],
    offset: usize,
    width: usize,
    format: Format,
    label: &str,
    editable: bool,
) {
    let Some(span) = covered.get_mut(offset..offset + width) else {
        return;
    };
    if span.iter().any(|value| *value) {
        if let Some(field) = fields.iter_mut().find(|field| {
            field.offset == offset
                && field.width == width
                && field.format == format
                && matches!(field.label.as_str(), "Key" | "Resource")
        }) {
            field.label = label.into();
        }
        return;
    }
    span.fill(true);
    fields.push(Field {
        offset,
        width,
        format,
        label: label.into(),
        editable,
    });
}

fn headers(class: u32, fields: &mut Vec<Field>, covered: &mut [bool]) {
    // A subgroup row stores the event mask of its own conditions and their children, right
    // after the condition list it summarises. Checked against every stock subgroup: 21 of 22
    // rows equal the mask over their listed conditions, and the one that differs carries the
    // extra bit of a nested General Predicate, which is what the root masks also fold in.
    // The compiler derives it, so it is not editable.
    if class == 0x8080_3E06 {
        insert(
            fields,
            covered,
            0x18,
            4,
            Format::Mask32,
            "Nested Event Mask",
            false,
        );
    }
    if class == 0x808040B5 {
        for (offset, width, format, label, editable) in [
            (0, 8, Format::Bytes, "Compiled Size", false),
            (0x88, 8, Format::Bytes, "Activation Event Mask", false),
            (0x90, 8, Format::Bytes, "Removal Event Mask", false),
            (0x98, 8, Format::Bytes, "Rearm Event Mask", false),
            (0xA0, 8, Format::Bytes, "Timer Extension Mask", false),
            (0xB8, 1, Format::Byte, "Execution Policy", true),
            (0xCC, 1, Format::Byte, "State Reservation", false),
            (0xCD, 1, Format::Byte, "Timer Reservation", false),
        ] {
            insert(fields, covered, offset, width, format, label, editable);
        }
    } else if nodes::CONDITIONS.iter().any(|node| node.class == class) {
        for (offset, width, format, label, editable) in [
            (0, 4, Format::Float, "Probability", true),
            (4, 1, Format::Byte, "Probability Source", true),
            (5, 1, Format::Byte, "Condition Kind", false),
            (6, 1, Format::Flag, "Linked State", false),
            (7, 1, Format::Byte, "Evaluation Order", false),
        ] {
            insert(fields, covered, offset, width, format, label, editable);
        }
    } else if nodes::EFFECTS.iter().any(|node| node.class == class) {
        insert(fields, covered, 0, 1, Format::Byte, "Effect Kind", false);
        // Extend Timers carries the event mask of its nested conditions, which the compiler
        // derives in `metadata::rebuild` the way it derives the root masks.
        if class == 0x80803E3B {
            insert(
                fields,
                covered,
                0x20,
                8,
                Format::Bytes,
                "Nested Event Mask",
                false,
            );
        }
        insert(
            fields,
            covered,
            1,
            1,
            Format::Flag,
            "Retain Effect State",
            true,
        );
    }
}

fn mapped(class: u32, fields: &mut Vec<Field>, covered: &mut [bool]) {
    let layout = nodes::CONDITIONS
        .iter()
        .find(|n| n.class == class)
        .and_then(|n| layout::condition_layout(n.kind))
        .or_else(|| {
            nodes::EFFECTS
                .iter()
                .find(|n| n.class == class)
                .and_then(|n| layout::effect_layout(n.kind))
        });
    if let Some(layout) = layout {
        for field in layout.fields {
            if field.format == layout::FieldFormat::Range {
                for (offset, bound) in [(field.offset, "Minimum"), (field.offset + 4, "Maximum")] {
                    insert(
                        fields,
                        covered,
                        offset,
                        4,
                        Format::Float,
                        &format!("{bound} {}", field.label),
                        true,
                    );
                }
                continue;
            }
            let format = match field.format {
                layout::FieldFormat::Byte | layout::FieldFormat::Mask8 => Format::Byte,
                layout::FieldFormat::Flag => Format::Flag,
                layout::FieldFormat::Float | layout::FieldFormat::Seconds => Format::Float,
                layout::FieldFormat::Key => Format::Key,
                layout::FieldFormat::Mask32 => Format::Mask32,
                _ => Format::Bytes,
            };
            insert(
                fields,
                covered,
                field.offset,
                field.format.width(),
                format,
                field.label,
                true,
            );
        }
    }
}

fn known(class: u32) -> Vec<(usize, usize, Format, &'static str)> {
    use Format::*;
    let mut result = match class {
        0x80803E42 => vec![
            (2, 1, Byte, "Spawn Position"),
            (4, 4, Unsigned, "Count"),
            (8, 4, Float, "Value"),
            (0x18, 4, Tag, "Orb Entity"),
        ],
        0x80803DE5 => vec![
            (0xA9, 1, Flag, "Second Filter Flag"),
            (0x110, 4, Float, "Minimum Value"),
            (0x114, 4, Float, "Maximum Value"),
            (0x118, 1, Flag, "Requires Owning Weapon"),
            (0x11C, 4, Key, "Weapon Key"),
        ],
        // The byte at +0x98 is one bit per ability slot, `1 << slot` of the Component Value
        // Adjustment targets: 1 Grenade, 2 Super, 4 Melee. Grenade perks set 1 (Chaotic
        // Exchanger, Oppressive Darkness, Overload Grenades), super perks set 2 (Horns of
        // Doom), melee perks set 4 (Impact Induction, Enhanced Impact Induction).
        // The two lanes at +A4 and +A7 move independently across the stock rows, which is what
        // the four value combinations of the word covering them show, so each is read as its
        // own flag rather than one opaque word. +99 sits beside the ability slot mask and
        // splits 96 to 67, so it is a setting rather than padding.
        0x80802F5F => vec![
            (0x98, 1, Byte, "Ability Slot Mask"),
            (0x99, 1, Flag, "Ability Slot Flag"),
            (0x9C, 4, Key, "Named Key"),
            (0xA0, 4, Float, "Value Threshold"),
            (0xA4, 1, Flag, "First Object Flag"),
            (0xA5, 1, Flag, "Second Object Flag"),
            (0xA6, 1, Flag, "Third Object Flag"),
            (0xA7, 1, Flag, "Fourth Object Flag"),
            (0xA8, 1, Byte, "Object Filter Selector"),
            (0xC0, 1, Flag, "First Source Flag"),
            (0xC1, 1, Byte, "Source Mask"),
            (0xC2, 1, Flag, "Second Source Flag"),
        ],
        // +9C and +9D move independently across the stock rows, so each is its own flag.
        0x80803DDC => vec![
            (0x9C, 1, Flag, "First Distance Flag"),
            (0x9D, 1, Flag, "Second Distance Flag"),
            (0x98, 4, Float, "Value Threshold"),
            (0xA0, 4, Key, "Named Key"),
            (0xA4, 4, Float, "Maximum Distance"),
            (0xB8, 1, Byte, "First Event Mask"),
            (0xB9, 1, Byte, "Second Event Mask"),
        ],
        0x80803DFD | 0x80803E01 => {
            vec![(8, 1, Byte, "Slot Mask"), (0x10, 1, Byte, "Player Filter")]
        }
        // 107FC20 passes pairs beginning at +18/+20 to F006B0/F08390 (normalized
        // health/shields), +28 to F0B4D0 (living teammate fraction) and +30 to
        // EFD430 (fireteam size including the owner). Last Stand and Celerity
        // require a zero living fraction and at least two fireteam members.
        // EF5EB0 reads the three weapon ability interfaces at +84/+8C/+94.
        // EF9060 reads magazine fractions at +9C/+A4/+AC. Reservoir Burst and
        // Together Forever check a full Energy magazine. These are min/max
        // pairs starting at +84, not multiply/add pairs starting at +88.
        // +BC is a separate byte selector and must not be edited as a float.
        0x80803DCE | 0x80803DCC => {
            let mut fields = vec![
                (0x10, 4, Float, "Second Range Minimum"),
                (0x14, 4, Float, "Second Range Maximum"),
                (0x18, 4, Float, "Minimum Health Fraction"),
                (0x1C, 4, Float, "Maximum Health Fraction"),
                (0x20, 4, Float, "Minimum Shield Fraction"),
                (0x24, 4, Float, "Maximum Shield Fraction"),
                (0x28, 4, Float, "Minimum Living Fireteam Fraction"),
                (0x2C, 4, Float, "Maximum Living Fireteam Fraction"),
                (0x30, 4, Float, "Minimum Fireteam Size"),
                (0x34, 4, Float, "Maximum Fireteam Size"),
                (0x38, 1, Byte, "Player State"),
                (0x81, 1, Byte, "Weapon State"),
                (0xD4, 4, Key, "Named Key"),
                (0xD8, 4, Float, "Minimum Value"),
                (0xDC, 4, Float, "Maximum Value"),
                (0xE0, 4, Float, "Hold Duration"),
                (0xF8, 1, Flag, "Invert Result"),
            ];
            const PAIRS: [(usize, &str, &str); 7] = [
                (
                    0x84,
                    "Minimum Kinetic Weapon Interface Value",
                    "Maximum Kinetic Weapon Interface Value",
                ),
                (
                    0x8C,
                    "Minimum Energy Weapon Interface Value",
                    "Maximum Energy Weapon Interface Value",
                ),
                (
                    0x94,
                    "Minimum Power Weapon Interface Value",
                    "Maximum Power Weapon Interface Value",
                ),
                (
                    0x9C,
                    "Minimum Kinetic Magazine Fraction",
                    "Maximum Kinetic Magazine Fraction",
                ),
                (
                    0xA4,
                    "Minimum Energy Magazine Fraction",
                    "Maximum Energy Magazine Fraction",
                ),
                (
                    0xAC,
                    "Minimum Power Magazine Fraction",
                    "Maximum Power Magazine Fraction",
                ),
                (0xB4, "Minimum Global Value", "Maximum Global Value"),
            ];
            for (offset, minimum, maximum) in PAIRS {
                fields.push((offset, 4, Float, minimum));
                fields.push((offset + 4, 4, Float, maximum));
            }
            fields
        }
        0x80803DFC => vec![(0x158, 1, Flag, "Scan Related Player State")],
        // The accumulator's +18 word is a bit set: every stock value is a sum of single bits and
        // perk families share them (catalysts 0x04, the Scavenger perks 0x40).
        // Ability Filter, inline in Object and Numeric Event Filter (+0x78), Kill Event
        // (+0x120), Event Numeric Modifier (+0x98) and Register Host Modifier (+0x80). The
        // mask is one bit per damage type, `1 << mode` of the Set Host Mode damage byte: 1
        // Kinetic, 2 Solar, 4 Arc, 8 Void. Stock witnesses agree across all four owners: void
        // perks set 8 (Abyssal Extractors, Horns of Doom, Oppressive Darkness), arc perks set
        // 4 (Conduction Tines, Volatile Conduction, Trinity Ghoul Catalyst), solar perks set
        // 2 (Bring the Heat, Helium Spirals, Solar Rampart, Solar Plexus), and 0x0E is every
        // element. The flag at +0x18 is 1 in exactly the rows whose mask is 0, in every owner.
        //
        // The filter carries a second mask and flag on the same pattern. The flag at +0x19 is
        // set in exactly the nodes whose word at +0x04 is zero, with no exception in any of the
        // 610 stock nodes across the three owners that carry them: Event Numeric Modifier 169
        // against 169, Register Host Modifier 42 against 42, Kill Event 395 against 395. Which
        // axis the second mask restricts is not resolved, so the labels say only that it is the
        // filter's second one. Its non-zero values are 0x02 and 0x1B, one node each.
        0x80804C81 => vec![
            (0x00, 4, Mask32, "Damage Type Mask"),
            (0x04, 4, Mask32, "Second Filter Mask"),
            (0x18, 1, Flag, "Any Damage Type"),
            (0x19, 1, Flag, "Any Second Filter"),
        ],
        // Runtime Label Predicate, inline in the Ability Filter at +8 and in Kill Event at
        // +0x58. The word takes -1, 0 and 1 in stock perks. Its role is not resolved, so the
        // name says only what it is: the predicate's mode word.
        0x808094A8 => vec![(0x00, 4, Integer, "Predicate Mode")],
        // The mask spans both words. Condition kinds run past 31, so an event mask needs a
        // full 64 bits here as it does at the action root and on Extend Timers, and the high
        // word carries exactly the single bits that reading predicts.
        0x80803E30 => vec![
            (0x18, 8, Bytes, "Source Event Mask"),
            (0x20, 4, Float, "Trigger Threshold"),
            (0x24, 4, Float, "Reset Threshold"),
            (0x28, 4, Float, "Minimum Value"),
            (0x2C, 4, Float, "Maximum Value"),
        ],
        // Kind 33 registers +A0. The typed reference at +A8 points back to this
        // node, not to an independent modifier table. Keep it compiler-managed.
        0x80803E3C => vec![
            (0xA8, 4, Tag, "Descriptor Owner"),
            (0xB0, 4, Unsigned, "Descriptor Offset"),
        ],
        // The entity or resource each spawning kind references, which is the reference
        // `effect_reference` already reads and `Action::asset` carries. The schema types the
        // lane and the kind's traced behavior says what it points at, so the label names that
        // rather than leaving a bare Resource.
        0x80803E45 => vec![
            (0x10, 4, Tag, "Attached Entity"),
            (0x30, 4, Key, "Action Value Parameter"),
        ],
        // +2 and +3 move independently, so each is its own byte rather than one opaque word.
        0x80803E43 => vec![
            (2, 1, Byte, "Spawn Mode"),
            (3, 1, Flag, "Spawn Flag"),
            (0x10, 4, Tag, "Spawned Entity"),
        ],
        0x80803E46 => vec![
            (2, 1, Byte, "Target Selection"),
            (0x10, 4, Tag, "Applied Resource"),
        ],
        0x80803E12 => vec![(0x10, 4, Tag, "Projectile Pattern")],
        0x80803E32 => vec![
            (8, 1, Byte, "Success Operation"),
            (9, 1, Flag, "Success Uses Event Value"),
            (12, 4, Float, "Success Value"),
            (16, 4, Float, "Hold Duration"),
            (20, 1, Byte, "Failure Operation"),
            (21, 1, Flag, "Failure Uses Event Value"),
            (24, 4, Float, "Failure Value"),
        ],
        0x80803E06 => vec![(0, 4, Float, "Hold Duration")],
        // The assignment and multiplication arrays share this exact row contract.
        // Selector 255 uses the literal, other values select a native stat.
        0x80803E22 => vec![
            // 108EFFD/108F06D reads only the signed selector byte. Preserve the
            // next three bytes instead of treating them as part of the index.
            (0, 1, Byte, "Damage Field"),
            (4, 4, Float, "Literal Value"),
            (8, 1, Byte, "Stat Selector"),
        ],
        0x80803E44 => vec![
            (2, 1, Byte, "Attachment Target"),
            (0x10, 4, Tag, "Attached Entity"),
            (0x50, 1, Byte, "Input Source"),
            (0x51, 1, Flag, "Normalize Input"),
        ],
        // The three weighted categories are the ammo types, established by the stock perks
        // that weight exactly one: Snapload Finisher ("generate Primary ammo") the first,
        // Special Finisher, Extra Reserves and Swift Charge ("Special ammo") the second, and
        // Heavy Finisher, Giving Hand and the Voltaic Ammo Collectors ("Heavy ammo") the third.
        // Each category is a twelve byte triple ending in its weight, so the two floats before
        // a weight belong to the same ammo type. The triple naming follows the one kind 11
        // already uses. Their roles inside the triple are not resolved, so the labels say only
        // the category and the position.
        0x80803E47 => vec![
            (3, 1, Flag, "Owner Path"),
            (4, 1, Flag, "Related Player Path"),
            (0x10, 4, Tag, "Spawned Resource"),
            (0x18, 4, Float, "Primary Ammo Value 1"),
            (0x1C, 4, Float, "Primary Ammo Value 2"),
            (0x20, 4, Float, "Primary Ammo Weight"),
            (0x24, 4, Float, "Special Ammo Value 1"),
            (0x28, 4, Float, "Special Ammo Value 2"),
            (0x2C, 4, Float, "Special Ammo Weight"),
            (0x30, 4, Float, "Heavy Ammo Value 1"),
            (0x34, 4, Float, "Heavy Ammo Value 2"),
            (0x38, 4, Float, "Heavy Ammo Weight"),
        ],
        // The same traced lanes read by component_value_adjustment_facts.
        0x80803E4D => vec![
            (2, 1, Byte, "Target Selector"),
            (3, 1, Byte, "Ability State"),
            (4, 1, Byte, "Ability Version"),
            (8, 4, Float, "Scale"),
            (0x0C, 4, Float, "Limit"),
            (0x48, 1, Byte, "Input Selector"),
        ],
        0x80802F18 => vec![
            (0x38, 1, Byte, "Input Source"),
            (0x39, 1, Flag, "Normalize Input"),
        ],
        0x80802F1A => vec![
            (0, 4, Float, "Default Scalar Adjustment"),
            (4, 4, Float, "Alternate Scalar Adjustment"),
        ],
        0x80800090 => (0..4)
            .map(|lane| (lane * 4, 4, Float, ["X", "Y", "Z", "W"][lane]))
            .collect(),
        0x808094B3 => vec![(0, 4, Key, "Label")],
        // One-byte records nested in conditions, named by the stock perks that set them
        // (see `values.rs`): the event's damage type and the target's enemy faction.
        0x80806B02 => vec![(0, 1, Byte, "Damage Type")],
        0x80806829 => vec![(0, 1, Byte, "Enemy Faction")],
        // The client's own member names for these lanes, from the live memory scan described
        // above `name`. Each is the array or record the named member points at.
        0x808073F4 => vec![
            (0x00, 8, Pointer, "Bytecode"),
            (0x10, 8, Pointer, "Constant Buffer"),
        ],
        0x80809419 => vec![
            (0x00, 8, Pointer, "Inputs"),
            (0x10, 8, Pointer, "Expression"),
        ],
        0x80804D78 => vec![(0x00, 8, Pointer, "Factions")],
        0x80809312 => vec![(0x08, 8, Pointer, "Filters")],
        0x80809316 => vec![(0x08, 8, Pointer, "Filter")],
        // Lanes whose role is unresolved but whose shape the stock values settle, so the
        // workbench renders a checkbox or a number instead of four hex bytes. Each label
        // states the position and the shape, never a meaning this project has not recovered.
        //
        // Event Numeric Modifier: +B8 splits 100 to 71 across its rows, a setting not padding.
        0x80802F16 => vec![(0xB8, 1, Flag, "Applies After Filters")],
        // Two Objects and Event State: +78 is set in one row of 43.
        0x808029E0 => vec![(0x78, 1, Flag, "Second Object Flag")],
        // Add Event Labels: every stock value at these three lanes is a single bit, so each
        // reads as a mask rather than a count.
        0x80803E1A => vec![
            (0xA8, 4, Mask32, "First Label Mask"),
            (0xC8, 4, Mask32, "Second Label Mask"),
            (0xCC, 4, Mask32, "Third Label Mask"),
        ],
        // The record is 0xA8 bytes, so it has no lane at 0xA8: a third mask was named there
        // and silently dropped, because the bytes it pointed at belong to the array header
        // that follows the node. Its two real masks are below.
        0x8080281C => vec![
            (0x78, 4, Mask32, "Object Filter Mask"),
            (0x9C, 4, Mask32, "Label Mask"),
        ],
        _ => Vec::new(),
    };
    if let Some(node) = nodes::CONDITIONS.iter().find(|node| node.class == class) {
        result.extend(match node.kind {
            // The lanes around and after the inline ability filter, which ends at +140. Their
            // roles are unresolved, so the labels state only the shape the stock values prove,
            // measured over the 396 stock Kill Event nodes: +A8 takes 0 and 4, +A9 is a flag
            // set in 5, +140 is mostly single bits (1, 2 and 4) with one node at 0x87 so it is
            // read as a plain byte, +141 is set in 72, +142 is a flag set in 7, +14C counts 0
            // through 2, and +150 is a float that 61 rows set to 0.01 and the rest leave at
            // zero. +143 never moves, so it stays an unnamed byte.
            2 => vec![
                (0xA8, 1, Byte, "Event Source Bits"),
                (0xA9, 1, Flag, "Event Source Flag"),
                (0x140, 1, Byte, "Source Bits"),
                (0x141, 1, Flag, "Requires Owning Weapon"),
                (0x142, 1, Flag, "Source Flag"),
                (0x14C, 4, Unsigned, "Source Selector"),
                (0x150, 4, Float, "Source Threshold"),
            ],
            9 | 28 => vec![(8, 4, Mask32, "Selected Bits")],
            // The schema declares a resource reference at +10 and every stock value is a tag,
            // so the lane is read as one. It was declared a key before, which did not match
            // the resource format the describer already assigns and so never took effect.
            10 | 11 => vec![(0x10, 4, Tag, "Referenced Resource")],
            34 => vec![
                (0x10, 4, Key, "Event Key"),
                (0x18, 4, Float, "First Range Minimum"),
                (0x1C, 4, Float, "First Range Maximum"),
                (0x20, 4, Float, "Second Range Minimum"),
                (0x24, 4, Float, "Second Range Maximum"),
            ],
            // The slot mask is one bit per weapon slot: 1 Kinetic, 2 Energy, 4 Power. Mecha
            // Holster's hand cannon nodes set 1 and 2, Lucent Blade's sword nodes set 4, and
            // Cobra Totemic and Move to Survive, which apply to every weapon, set 7.
            13..=19 => {
                let mut fields = vec![(8, 1, Flag, "Requires Owning Weapon")];
                if node.kind == 19 {
                    // The event's own byte selects which of these two flags must be set.
                    // Every stock perk with +9 set reads as reloading, from Kill Clip
                    // starting to Under Pressure ending, 18 perks in all. The perks with
                    // +A set (Ravenous Beast, Gathering Light, Revolution, Gift of the
                    // Traveler) share no description, so that event keeps a plain name.
                    // The restriction is a window in seconds: 3.5 for Kill Clip and Memento
                    // Mori, 3 for Rat King, 5 for Ambitious Assassin and Impetus.
                    fields.push((9, 1, Flag, "On Reload"));
                    fields.push((0x0A, 1, Flag, "On Second Weapon Event"));
                    fields.push((0x0B, 1, Byte, "Slot Mask"));
                    fields.push((0x0C, 4, Float, "Time Restriction"));
                } else {
                    // Without the two flags the slot mask follows the owning-weapon flag.
                    // Stock draw, holster and weapon event filter nodes keep +0x0B zero.
                    fields.push((9, 1, Byte, "Slot Mask"));
                }
                fields
            }
            23 => vec![
                (8, 1, Byte, "Event Byte"),
                (9, 1, Byte, "Host State Filter"),
                (0x0A, 1, Flag, "Requires Owning Weapon"),
            ],
            27 => vec![
                (8, 1, Flag, "Requires Owning Weapon"),
                (9, 1, Byte, "Slot Mask"),
                (0x0C, 1, Byte, "First Event Byte"),
                (0x0D, 1, Byte, "Second Event Byte"),
                (0x80, 1, Byte, "Object Filter Selector"),
                (0x90, 1, Byte, "Mode"),
            ],
            41 => vec![
                (8, 1, Byte, "Slot Mask"),
                (0x18, 1, Byte, "Object Filter Selector"),
            ],
            _ => Vec::new(),
        });
    }
    if let Some(node) = nodes::EFFECTS.iter().find(|node| node.class == class) {
        // Complex records share the scalar contracts already decoded in action/fields.
        // Their pointers, filters and value programs continue to use the native schema.
        result.extend(match node.kind {
            3 => vec![(4, 1, Byte, "Position Selector")],
            1 => vec![
                (2, 1, Byte, super::super::fields::CREATE_ENTITY_MODE_LABEL),
                (
                    0x18,
                    4,
                    Key,
                    super::super::fields::CREATE_ENTITY_KEY_LABELS[0],
                ),
                (
                    0x1C,
                    4,
                    Key,
                    super::super::fields::CREATE_ENTITY_KEY_LABELS[1],
                ),
                (
                    0x20,
                    4,
                    Float,
                    super::super::fields::CREATE_ENTITY_FLOAT_LABELS[0],
                ),
                (
                    0x24,
                    4,
                    Float,
                    super::super::fields::CREATE_ENTITY_FLOAT_LABELS[1],
                ),
                (
                    0x28,
                    4,
                    Float,
                    super::super::fields::CREATE_ENTITY_FLOAT_LABELS[2],
                ),
                (
                    0x2C,
                    4,
                    Float,
                    super::super::fields::CREATE_ENTITY_FLOAT_LABELS[3],
                ),
            ],
            // Long March ("detect enemies on your radar from farther away") writes 80 to the
            // third float and Radar Booster ("slightly increases the range") 56, both leaving
            // the first two at -1, the value the callback leaves unchanged.
            18 => vec![
                (0x04, 4, Float, "Other Setting 1"),
                (0x08, 4, Float, "Other Setting 2"),
                (0x0C, 4, Float, "Radar Detection Range"),
            ],
            // The three triples are per ammo type, established by the three Finder mods:
            // Primary Ammo Finder writes only the first triple, Special Ammo Finder only
            // the second and Heavy Ammo Finder only the third. Which of a triple's three
            // values does what is not established, so they keep their position.
            11 => vec![
                (0x04, 4, Float, "Shared Value 1"),
                (0x08, 4, Float, "Shared Value 2"),
                (0x0C, 4, Float, "Primary Ammo Value 1"),
                (0x10, 4, Float, "Primary Ammo Value 2"),
                (0x14, 4, Float, "Primary Ammo Value 3"),
                (0x18, 4, Float, "Special Ammo Value 1"),
                (0x1C, 4, Float, "Special Ammo Value 2"),
                (0x20, 4, Float, "Special Ammo Value 3"),
                (0x24, 4, Float, "Heavy Ammo Value 1"),
                (0x28, 4, Float, "Heavy Ammo Value 2"),
                (0x2C, 4, Float, "Heavy Ammo Value 3"),
            ],
            10 => vec![
                (2, 1, Byte, "Target Selector"),
                (3, 1, Byte, "Flag Byte"),
                (4, 4, Mask32, "Ability Slot Mask"),
                (8, 4, Key, "Property Key"),
                (0x48, 1, Byte, "Input Selector"),
                (0x49, 1, Byte, "Operation"),
                (0x4A, 1, Byte, "Removal Policy"),
                (0x4C, 4, Float, "Removal Value"),
            ],
            14 | 15 => {
                let mut fields = vec![
                    (
                        0x68,
                        1,
                        Byte,
                        if node.kind == 14 {
                            "Storage Path"
                        } else {
                            "Destination"
                        },
                    ),
                    (0x69, 1, Flag, "Allow Magazine Overflow"),
                    (
                        0x6A,
                        1,
                        if node.kind == 14 { Flag } else { Byte },
                        if node.kind == 14 {
                            "Scale by Ammunition Unit"
                        } else {
                            "Capacity Source"
                        },
                    ),
                    (0x6B, 1, Flag, "Scale by Action Value"),
                ];
                for (index, name) in [
                    "Owning Slot Amount",
                    "Slot 1 Amount",
                    "Slot 2 Amount",
                    "Slot 3 Amount",
                    "Category 1 Amount",
                    "Category 2 Amount",
                    "Category 3 Amount",
                ]
                .into_iter()
                .enumerate()
                {
                    fields.push((
                        0x6C + index * 4,
                        4,
                        if node.kind == 14 { Integer } else { Float },
                        name,
                    ));
                }
                fields
            }
            16 => vec![
                (0x68, 1, Flag, "Allow Magazine Overflow"),
                (0x69, 1, Byte, "Capacity Basis"),
                (0x6A, 1, Flag, "Publish Slot Event"),
                (0x6B, 1, Byte, "Input Selector"),
                (0x6C, 1, Flag, "Normalize Input"),
            ],
            32 => vec![(4, 4, Float, "Extend By"), (8, 4, Float, "Up To")],
            33 => vec![
                (4, 4, Float, "Damage Multiplier"),
                (8, 1, Byte, "Multiplier Stat"),
                (12, 4, Float, "Maximum Source Distance"),
            ],
            40 => vec![(0x148, 1, Flag, "Uses Ability Scalar Cap")],
            48 => vec![
                (2, 1, Byte, "First Target Selector"),
                (3, 1, Byte, "Second Target Selector"),
                (4, 1, Byte, "Third Target Selector"),
                (0x10, 4, Tag, "Runtime Resource"),
            ],
            53 => vec![(0x18, 4, Float, "Upper Cap"), (0x5A, 1, Byte, "Operation")],
            54 => vec![(0xA0, 1, Flag, "Event Flag")],
            _ => Vec::new(),
        });
    }
    result
}

impl Field {
    pub fn bytes<'a>(&self, block: &'a Block, row: usize) -> Option<&'a [u8]> {
        let stride = schema::record(block.class).ok()?.size;
        let at = row.checked_mul(stride)?.checked_add(self.offset)?;
        block.bytes.get(at..at + self.width)
    }

    /// Edit a scalar atomically. Pointer addresses and array lengths are compiler owned.
    pub fn write(&self, block: &mut Block, row: usize, bytes: &[u8]) -> Result<(), String> {
        if !self.editable || bytes.len() != self.width {
            return Err("This native field is not directly editable.".into());
        }
        if self.format == Format::Float
            && !f32::from_le_bytes(bytes.try_into().map_err(|_| "Invalid float width.")?)
                .is_finite()
        {
            return Err("Enter a finite number.".into());
        }
        if self.format == Format::Flag && bytes[0] > 1 {
            return Err("A flag must be zero or one.".into());
        }
        let stride = schema::record(block.class)?.size;
        let at = row
            .checked_mul(stride)
            .and_then(|at| at.checked_add(self.offset))
            .ok_or("Native field offset overflow.")?;
        block
            .bytes
            .get_mut(at..at + self.width)
            .ok_or("The native field exceeds its record.")?
            .copy_from_slice(bytes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_lane_and_predicate_bound_edits_preserve_neighboring_native_bytes() {
        let mut row = Block {
            class: 0x80803E22,
            count: Some(2),
            bytes: vec![0xA5; 24],
            links: Default::default(),
        };
        let before = row.bytes.clone();
        let selector = describe(row.class)
            .unwrap()
            .into_iter()
            .find(|field| field.label == "Damage Field")
            .unwrap();
        selector.write(&mut row, 1, &[1]).unwrap();
        assert_eq!(row.bytes[12], 1);
        assert_eq!(&row.bytes[..12], &before[..12]);
        assert_eq!(&row.bytes[13..], &before[13..]);

        for class in [0x80803DCE, 0x80803DCC] {
            let mut block = Block {
                class,
                count: None,
                bytes: vec![0xA5; schema::record(class).unwrap().size],
                links: Default::default(),
            };
            let fields = describe(class).unwrap();
            for (label, offset, value) in [
                ("Maximum Living Fireteam Fraction", 0x2C, 0.0f32),
                ("Minimum Fireteam Size", 0x30, 2.0),
                ("Minimum Energy Magazine Fraction", 0xA4, 1.0),
                ("Maximum Global Value", 0xB8, 0.5),
            ] {
                let before = block.bytes.clone();
                let field = fields.iter().find(|field| field.label == label).unwrap();
                field.write(&mut block, 0, &value.to_le_bytes()).unwrap();
                assert_eq!(&block.bytes[offset..offset + 4], &value.to_le_bytes());
                assert_eq!(&block.bytes[..offset], &before[..offset]);
                assert_eq!(&block.bytes[offset + 4..], &before[offset + 4..]);
            }
            // +BC is a separate native selector, never the end of a float pair.
            assert!(!fields.iter().any(|field| {
                field.format == Format::Float
                    && field.offset <= 0xBC
                    && field.offset + field.width > 0xBC
            }));
        }
    }

    /// A declared field whose bytes fall outside the record is dropped by `insert` without a
    /// word, so a name recovered for a lane that does not exist never reaches the workbench
    /// and reads as an unmapped byte instead. Every entry `known` returns has to land inside
    /// the record it names, and inside every object that inlines that record.
    #[test]
    fn every_declared_field_lands_inside_its_record() {
        type Encoded = (
            u32,
            usize,
            u32,
            bool,
            Vec<(usize, u32, u32)>,
            Vec<(usize, u32)>,
        );
        let rows: Vec<Encoded> = serde_json::from_str(include_str!("schema.json"))
            .expect("native perk declarations parse");
        let mut checked = 0;
        for (class, size, ..) in &rows {
            for (offset, width, _, label) in known(*class) {
                checked += 1;
                assert!(
                    offset + width <= *size,
                    "0x{class:08X} names \"{label}\" at +0x{offset:X} width {width},                      past the end of its {size} byte record"
                );
            }
            // The same fields are applied again wherever the record is inlined, at the owner's
            // offset, so they have to fit there too.
            for (base, child, _) in schema::inline(*class).expect("inline declarations") {
                if child == *class {
                    continue;
                }
                for (offset, width, _, label) in known(child) {
                    assert!(
                        base + offset + width <= *size,
                        "0x{class:08X} inlines 0x{child:08X} at +0x{base:X}, putting \"{label}\"                          past the end of its {size} byte record"
                    );
                }
            }
        }
        assert!(checked > 100, "only {checked} fields were checked");
    }

    /// The module's contract: every byte of a record belongs to exactly one field, so the
    /// workbench can show a node completely and an edit can never land between fields. The
    /// gap pass is what makes it hold, and it has to hold for every declared record, not only
    /// the node kinds.
    #[test]
    fn describe_covers_every_byte_of_every_record_exactly_once() {
        type Encoded = (
            u32,
            usize,
            u32,
            bool,
            Vec<(usize, u32, u32)>,
            Vec<(usize, u32)>,
        );
        let rows: Vec<Encoded> = serde_json::from_str(include_str!("schema.json"))
            .expect("native perk declarations parse");
        for (class, size, ..) in &rows {
            let described =
                describe(*class).unwrap_or_else(|e| panic!("0x{class:08X} has no field view: {e}"));
            let mut covered = vec![0_u32; *size];
            for field in &described {
                assert!(field.width > 0, "0x{class:08X} has a zero width field");
                for byte in field.offset..field.offset + field.width {
                    let seen = covered.get_mut(byte).unwrap_or_else(|| {
                        panic!("0x{class:08X} describes byte {byte} past {size}")
                    });
                    *seen += 1;
                }
            }
            if let Some(byte) = covered.iter().position(|seen| *seen != 1) {
                panic!(
                    "0x{class:08X} covers byte 0x{byte:X} {} times, not once",
                    covered[byte]
                );
            }
        }
    }

    /// `insert` keeps the first field to claim a byte, so a second entry overlapping it is
    /// dropped as quietly as an out-of-range one. A width that disagrees with the format is
    /// the same kind of slip: `Field::write` refuses a float that is not four bytes wide, so
    /// the control would be built and then reject every edit.
    #[test]
    fn declared_fields_do_not_overlap_and_match_their_format_width() {
        type Encoded = (
            u32,
            usize,
            u32,
            bool,
            Vec<(usize, u32, u32)>,
            Vec<(usize, u32)>,
        );
        let rows: Vec<Encoded> = serde_json::from_str(include_str!("schema.json"))
            .expect("native perk declarations parse");
        for (class, ..) in &rows {
            let declared = known(*class);
            for (index, (offset, width, format, label)) in declared.iter().enumerate() {
                let natural = match format {
                    Format::Byte | Format::Flag => Some(1),
                    Format::Float
                    | Format::Integer
                    | Format::Unsigned
                    | Format::Mask32
                    | Format::Key
                    | Format::Tag => Some(4),
                    Format::Pointer => Some(8),
                    Format::Bytes => None,
                };
                assert!(
                    natural.is_none_or(|natural| natural == *width),
                    "0x{class:08X} names \"{label}\" as {format:?} but {width} bytes wide"
                );
                for (other, other_width, _, other_label) in &declared[index + 1..] {
                    assert!(
                        offset + width <= *other || other + other_width <= *offset,
                        "0x{class:08X} declares \"{label}\" at +0x{offset:X} and \"{other_label}\"                          at +0x{other:X} over the same bytes; only the first survives"
                    );
                }
            }
        }
    }
}
