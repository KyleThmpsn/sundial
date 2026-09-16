//! Byte-complete native field views with recovered names where the contract is known.
use super::{Block, schema};
use crate::sandbox_perk::{action::layout, nodes};

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
        let (width, format, label) = match code {
            1..=3 => (8, Format::Pointer, "Reference"),
            4 | 9 => (4, Format::Tag, "Resource"),
            11 => (4, Format::Key, "Key"),
            _ => continue,
        };
        insert(
            &mut fields,
            &mut covered,
            offset,
            width,
            format,
            label,
            code != 3 && code != 1 && code != 2,
        );
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
        let width = (1..=4.min(covered.len() - at))
            .take_while(|width| !covered[at + width - 1])
            .last()
            .unwrap_or(1);
        let format = if width == 1 {
            Format::Byte
        } else {
            Format::Bytes
        };
        insert(
            &mut fields,
            &mut covered,
            at,
            width,
            format,
            &format!("Native Value +0x{at:02X}"),
            true,
        );
        at += width;
    }
    fields.sort_by_key(|field| field.offset);
    Ok(fields)
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
        ],
        0x80803DE5 => vec![
            (0x110, 4, Float, "Minimum Value"),
            (0x114, 4, Float, "Maximum Value"),
            (0x118, 1, Flag, "Requires Owning Weapon"),
            (0x11C, 4, Key, "Weapon Key"),
        ],
        0x80802F5F => vec![
            (0x9C, 4, Key, "Named Key"),
            (0xA0, 4, Float, "Value Threshold"),
            (0xA8, 1, Byte, "Object Filter Selector"),
            (0xC1, 1, Byte, "Source Mask"),
        ],
        0x80803DDC => vec![
            (0x98, 4, Float, "Value Threshold"),
            (0xA0, 4, Key, "Named Key"),
            (0xA4, 4, Float, "Maximum Distance"),
            (0xB8, 1, Byte, "First Event Mask"),
            (0xB9, 1, Byte, "Second Event Mask"),
        ],
        0x80803DFD | 0x80803E01 => {
            vec![(8, 1, Byte, "Slot Mask"), (0x10, 1, Byte, "Player Filter")]
        }
        // The general predicate's inline restrictions, read off the stock perks that set them
        // (see `values.rs`): a player state mask at +38, a weapon state at +81, and four
        // floats only the health perks touch (Pulse Monitor, Armor of the Colossus, Underdog,
        // Eye of the Storm), whose bounds are not established beyond that. The key at +D4 and
        // the float pair at +D8/+DC form a named value range, the same contract as 0x80803DE5.
        // Across 1613 stock actions every keyed row keeps +D8 at or below +DC (298 of 298) and
        // unkeyed rows sit at (1, 1). A second selector byte at +80 takes five stock values and
        // is not named yet (see the template field map under docs).
        0x80803DCE | 0x80803DCC => vec![
            (0x18, 4, Float, "Health Value 1"),
            (0x1C, 4, Float, "Health Value 2"),
            (0x20, 4, Float, "Health Value 3"),
            (0x24, 4, Float, "Health Value 4"),
            (0x38, 1, Byte, "Player State"),
            (0x81, 1, Byte, "Weapon State"),
            (0xD4, 4, Key, "Named Key"),
            (0xD8, 4, Float, "Minimum Value"),
            (0xDC, 4, Float, "Maximum Value"),
            (0xE0, 4, Float, "Hold Duration"),
            (0xF8, 1, Flag, "Invert Result"),
        ],
        0x80803DFC => vec![(0x158, 1, Flag, "Scan Related Player State")],
        // The accumulator's +18 word is a bit set: every stock value is a sum of single bits and
        // perk families share them (catalysts 0x04, the Scavenger perks 0x40).
        0x80803E30 => vec![
            (0x18, 4, Mask32, "Source Event Mask"),
            (0x20, 4, Float, "Trigger Threshold"),
            (0x24, 4, Float, "Reset Threshold"),
            (0x28, 4, Float, "Minimum Value"),
            (0x2C, 4, Float, "Maximum Value"),
        ],
        // Register Host Modifier names its modifier table by the tag at +A8 and selects the row
        // by this offset. Every stock value is a multiple of eight, an eight byte stride.
        0x80803E3C => vec![(0xB0, 4, Unsigned, "Modifier Row Offset")],
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
            (0, 4, Unsigned, "Event Slot"),
            (4, 4, Float, "Literal Value"),
            (8, 1, Byte, "Stat Selector"),
        ],
        0x80803E44 => vec![
            (0x50, 1, Byte, "Input Source"),
            (0x51, 1, Flag, "Normalize Input"),
        ],
        0x80803E46 => vec![(2, 1, Byte, "Target Selection")],
        // The three weighted categories are the ammo types, established by the stock perks
        // that weight exactly one: Snapload Finisher ("generate Primary ammo") the first,
        // Special Finisher, Extra Reserves and Swift Charge ("Special ammo") the second, and
        // Heavy Finisher, Giving Hand and the Voltaic Ammo Collectors ("Heavy ammo") the third.
        0x80803E47 => vec![
            (3, 1, Flag, "Owner Path"),
            (4, 1, Flag, "Related Player Path"),
            (0x20, 4, Float, "Primary Ammo Weight"),
            (0x2C, 4, Float, "Special Ammo Weight"),
            (0x38, 4, Float, "Heavy Ammo Weight"),
        ],
        // The same traced lanes read by component_value_adjustment_facts.
        0x80803E4D => vec![
            (2, 1, Byte, "Target Selector"),
            (3, 1, Byte, "Flag Byte"),
            (4, 1, Byte, "Option Byte"),
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
        _ => Vec::new(),
    };
    if let Some(node) = nodes::CONDITIONS.iter().find(|node| node.class == class) {
        result.extend(match node.kind {
            2 => vec![(0x141, 1, Flag, "Requires Owning Weapon")],
            9 | 28 => vec![(8, 4, Mask32, "Selected Bits")],
            10 | 11 => vec![(0x10, 4, Key, "Event Key")],
            34 => vec![
                (0x10, 4, Key, "Event Key"),
                (0x18, 4, Float, "First Range Minimum"),
                (0x1C, 4, Float, "First Range Maximum"),
                (0x20, 4, Float, "Second Range Minimum"),
                (0x24, 4, Float, "Second Range Maximum"),
            ],
            13..=19 => {
                let mut fields = vec![
                    (8, 1, Flag, "Requires Owning Weapon"),
                    (0x0B, 1, Byte, "Slot Mask"),
                ];
                if node.kind == 19 {
                    // The event's own byte selects which of these two flags must be set.
                    // Every stock perk with +9 set reads as reloading, from Kill Clip
                    // starting to Under Pressure ending, 18 perks in all. The perks with
                    // +A set (Ravenous Beast, Gathering Light, Revolution, Gift of the
                    // Traveler) share no description, so that event keeps a plain name.
                    fields.push((9, 1, Flag, "On Reload"));
                    fields.push((0x0A, 1, Flag, "On Second Weapon Event"));
                    fields.push((0x0C, 1, Byte, "Time Restriction"));
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
                (2, 1, Byte, "Attachment Mode"),
                (0x18, 4, Key, "First Key"),
                (0x1C, 4, Key, "Second Key"),
                (0x20, 4, Float, "First Float"),
                (0x24, 4, Float, "Second Float"),
                (0x28, 4, Float, "Third Float"),
                (0x2C, 4, Float, "Fourth Float"),
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
                (4, 4, Float, "Modifier Value"),
                (8, 1, Byte, "Input Selector"),
                (12, 4, Float, "Modifier Limit"),
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
