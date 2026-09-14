//! Byte-complete native field views with recovered names where the contract is known.
use super::{Block, schema};
use crate::sandbox_perk::{action::layout, nodes};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    Byte,
    Flag,
    Float,
    Integer,
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
            let format = match field.format {
                layout::FieldFormat::Byte | layout::FieldFormat::Mask8 => Format::Byte,
                layout::FieldFormat::Flag => Format::Flag,
                layout::FieldFormat::Float | layout::FieldFormat::Seconds => Format::Float,
                layout::FieldFormat::Key => Format::Key,
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
            (2, 1, Byte, "Orb Position"),
            (4, 4, Integer, "Orb Count"),
            (8, 4, Float, "Orb Value"),
        ],
        0x80803DE5 => vec![
            (0x110, 4, Float, "Minimum Value"),
            (0x114, 4, Float, "Maximum Value"),
            (0x118, 1, Flag, "Requires Owning Weapon"),
        ],
        0x80802F5F => vec![
            (0xA0, 4, Float, "Value Threshold"),
            (0xA8, 1, Byte, "Object Selection"),
            (0xC1, 1, Byte, "Source Mask"),
        ],
        0x80803DDC => vec![
            (0x98, 4, Float, "Value Threshold"),
            (0xA4, 4, Float, "Maximum Distance"),
            (0xB8, 1, Byte, "First Event Mask"),
            (0xB9, 1, Byte, "Second Event Mask"),
        ],
        0x80803DFD | 0x80803E01 => {
            vec![(8, 1, Byte, "Slot Mask"), (0x10, 1, Byte, "Player Filter")]
        }
        0x80803DCE | 0x80803DCC => vec![
            (0xE0, 4, Float, "Hold Duration"),
            (0xF8, 1, Flag, "Invert Result"),
        ],
        0x80803DFC => vec![(0x158, 1, Flag, "Scan Related Player State")],
        0x80803E30 => vec![
            (0x20, 4, Float, "Trigger Threshold"),
            (0x24, 4, Float, "Reset Threshold"),
            (0x28, 4, Float, "Minimum Value"),
            (0x2C, 4, Float, "Maximum Value"),
        ],
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
        0x80803E44 => vec![
            (0x50, 1, Byte, "Input Source"),
            (0x51, 1, Flag, "Normalize Input"),
        ],
        0x80803E46 => vec![(2, 1, Byte, "Target Selection")],
        0x80803E47 => vec![
            (3, 1, Flag, "Owner Path"),
            (4, 1, Flag, "Related Player Path"),
            (0x20, 4, Float, "First Weight"),
            (0x2C, 4, Float, "Second Weight"),
            (0x38, 4, Float, "Third Weight"),
        ],
        0x80803E4D => vec![(0x48, 1, Byte, "Input Source")],
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
        _ => Vec::new(),
    };
    if matches!(class, 0x80803DDE | 0x80803DDD | 0x80803DE2) {
        result.push((8, 1, Flag, "Requires Owning Weapon"));
        result.push((0x0B, 1, Byte, "Slot Mask"));
        if class == 0x80803DE2 {
            result.push((0x0C, 1, Byte, "Time Restriction"));
        }
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
