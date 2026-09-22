//! Field maps of the plain scalar node kinds: the kinds whose bytes hold only fixed-width
//! values, no pointers and no nested lists, so a node can be carried verbatim and edited
//! field by field.
//!
//! Every stock node of every kind listed here was dumped and censused against the clean
//! packages. The only bytes that vary between stock nodes are the fields below, which is
//! what lets the compiler write such a node back byte for byte. A field is named by its
//! traced position, not by a gameplay meaning the client does not give it.

use super::{Fact, FactValue, nodes};

/// How a field is stored.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldFormat {
    /// One byte read as a selector or enumeration.
    Byte,
    /// One byte read as a boolean.
    Flag,
    /// One byte read as a bit mask.
    Mask8,
    /// Four bytes read as a bit mask.
    Mask32,
    /// A 32-bit name key.
    Key,
    /// A 32-bit float.
    Float,
    /// A duration stored as a 32-bit float in seconds.
    Seconds,
    /// Two 32-bit floats, an inclusive range.
    Range,
}

impl FieldFormat {
    /// Bytes the field occupies.
    #[must_use]
    pub const fn width(self) -> usize {
        match self {
            Self::Byte | Self::Flag | Self::Mask8 => 1,
            Self::Mask32 | Self::Key | Self::Float | Self::Seconds => 4,
            Self::Range => 8,
        }
    }
}

/// One mapped field of a node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Field {
    pub label: &'static str,
    pub offset: usize,
    pub format: FieldFormat,
}

impl Field {
    const fn new(label: &'static str, offset: usize, format: FieldFormat) -> Self {
        Self {
            label,
            offset,
            format,
        }
    }

    /// The field's value in `node`, which must hold the whole node.
    #[must_use]
    pub fn read(&self, node: &[u8]) -> Option<FactValue> {
        let at = self.offset;
        node.get(at..at.checked_add(self.format.width())?)?;
        let byte = |offset: usize| node.get(offset).copied();
        let word = |offset: usize| {
            Some(u32::from_le_bytes(
                node.get(offset..offset + 4)?.try_into().ok()?,
            ))
        };
        Some(match self.format {
            FieldFormat::Byte => FactValue::Selector(byte(at)?),
            FieldFormat::Flag => FactValue::Flag(byte(at)? != 0),
            FieldFormat::Mask8 => FactValue::Mask(byte(at)?.into()),
            FieldFormat::Mask32 => FactValue::Mask(word(at)?.into()),
            FieldFormat::Key => FactValue::Key(word(at)?),
            FieldFormat::Float => FactValue::Number(f32::from_bits(word(at)?)),
            FieldFormat::Seconds => FactValue::Seconds(f32::from_bits(word(at)?)),
            FieldFormat::Range => {
                FactValue::Range(f32::from_bits(word(at)?), f32::from_bits(word(at + 4)?))
            }
        })
    }

    /// Stores `value` into `node`. A value of the wrong shape, or a node too short to hold
    /// the field, leaves the node unchanged and returns false.
    pub fn write(&self, node: &mut [u8], value: &FactValue) -> bool {
        let at = self.offset;
        let Some(end) = at.checked_add(self.format.width()) else {
            return false;
        };
        let Some(slot) = node.get_mut(at..end) else {
            return false;
        };
        match (self.format, value) {
            (FieldFormat::Byte, FactValue::Selector(byte)) => slot[0] = *byte,
            (FieldFormat::Flag, FactValue::Flag(flag)) => slot[0] = u8::from(*flag),
            (FieldFormat::Mask8, FactValue::Mask(mask)) => {
                let Ok(value) = u8::try_from(*mask) else {
                    return false;
                };
                slot[0] = value;
            }
            (FieldFormat::Mask32, FactValue::Mask(mask)) => {
                let Ok(value) = u32::try_from(*mask) else {
                    return false;
                };
                slot.copy_from_slice(&value.to_le_bytes());
            }
            (FieldFormat::Key, FactValue::Key(key)) => slot.copy_from_slice(&key.to_le_bytes()),
            (FieldFormat::Float, FactValue::Number(value)) if value.is_finite() => {
                slot.copy_from_slice(&value.to_le_bytes());
            }
            (FieldFormat::Seconds, FactValue::Seconds(value))
                if value.is_finite() && *value >= 0.0 =>
            {
                slot.copy_from_slice(&value.to_le_bytes());
            }
            (FieldFormat::Range, FactValue::Range(low, high))
                if low.is_finite() && high.is_finite() =>
            {
                slot[..4].copy_from_slice(&low.to_le_bytes());
                slot[4..].copy_from_slice(&high.to_le_bytes());
            }
            _ => return false,
        }
        true
    }
}

/// The field map of one node kind.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub kind: u8,
    pub fields: &'static [Field],
}

impl Layout {
    /// Every field of `node` as facts, in field order.
    #[must_use]
    pub fn facts(&self, node: &[u8]) -> Vec<Fact> {
        self.fields
            .iter()
            .filter_map(|field| Some(Fact::new(field.label, field.read(node)?)))
            .collect()
    }

    /// The field with this label.
    #[must_use]
    pub fn field(&self, label: &str) -> Option<&'static Field> {
        self.fields.iter().find(|field| field.label == label)
    }
}

use FieldFormat::{Byte, Flag, Float, Key, Mask8, Mask32, Range};

const NONE: &[Field] = &[];
const OPERATION: &[Field] = &[Field::new("Operation", 2, Byte)];
const PROPERTY_KEY: &[Field] = &[Field::new("Property Key", 4, Key)];
const THREE_VALUES: &[Field] = &[
    Field::new("First Value", 4, Float),
    Field::new("Second Value", 8, Float),
    Field::new("Third Value", 0x0C, Float),
];
// The three triples are per ammo type, established by the three Finder mods: Primary Ammo
// Finder writes only the first, Special Ammo Finder only the second and Heavy Ammo Finder
// only the third.
const HOST_NUMERIC_MODIFIERS: &[Field] = &[
    Field::new("Shared Value 1", 0x04, Float),
    Field::new("Shared Value 2", 0x08, Float),
    Field::new("Primary Ammo Value 1", 0x0C, Float),
    Field::new("Primary Ammo Value 2", 0x10, Float),
    Field::new("Primary Ammo Value 3", 0x14, Float),
    Field::new("Special Ammo Value 1", 0x18, Float),
    Field::new("Special Ammo Value 2", 0x1C, Float),
    Field::new("Special Ammo Value 3", 0x20, Float),
    Field::new("Heavy Ammo Value 1", 0x24, Float),
    Field::new("Heavy Ammo Value 2", 0x28, Float),
    Field::new("Heavy Ammo Value 3", 0x2C, Float),
];

/// The scalar effect kinds. Kinds 1, 3, 10, 14, 15, 26 and 32 have their own program
/// actions. The complete native editor handles kinds with pointers or lists.
pub const EFFECT_LAYOUTS: &[Layout] = &[
    Layout {
        kind: 0,
        fields: NONE,
    },
    Layout {
        kind: 6,
        fields: &[
            Field::new("Mode", 2, Byte),
            Field::new("Keep After Removal", 3, Flag),
        ],
    },
    Layout {
        kind: 7,
        fields: &[
            Field::new("Target Selector", 2, Byte),
            Field::new("Property Key", 4, Key),
            Field::new("Operation", 8, Byte),
        ],
    },
    Layout {
        kind: 11,
        fields: HOST_NUMERIC_MODIFIERS,
    },
    // Long March writes 80 to the third float and Radar Booster 56, the only two stock uses,
    // both reading as radar detection range. The first two stay at -1, which the callback
    // leaves unchanged.
    Layout {
        kind: 18,
        fields: &[
            Field::new("Other Setting 1", 4, Float),
            Field::new("Other Setting 2", 8, Float),
            Field::new("Radar Detection Range", 0x0C, Float),
        ],
    },
    Layout {
        kind: 20,
        fields: OPERATION,
    },
    Layout {
        kind: 22,
        fields: &[
            Field::new("Slot Mask", 2, Mask8),
            Field::new("Value", 4, Byte),
        ],
    },
    Layout {
        kind: 24,
        fields: NONE,
    },
    Layout {
        kind: 25,
        fields: PROPERTY_KEY,
    },
    Layout {
        kind: 27,
        fields: NONE,
    },
    Layout {
        kind: 28,
        fields: &[
            Field::new("First State", 2, Byte),
            Field::new("Second State", 3, Byte),
        ],
    },
    Layout {
        kind: 29,
        fields: THREE_VALUES,
    },
    Layout {
        kind: 30,
        fields: OPERATION,
    },
    Layout {
        kind: 35,
        fields: &[
            Field::new("Target Selector", 2, Byte),
            Field::new("Interface Selector", 3, Byte),
            Field::new("Replacement Key", 4, Key),
            Field::new("Apply to Player", 8, Flag),
        ],
    },
    Layout {
        kind: 41,
        fields: &[
            Field::new("Target Selector", 2, Byte),
            Field::new("Slot Mask", 3, Mask8),
            Field::new("Property Key", 4, Key),
        ],
    },
    Layout {
        kind: 42,
        fields: &[Field::new("Mode", 2, Byte), Field::new("Value", 4, Float)],
    },
    Layout {
        kind: 43,
        fields: PROPERTY_KEY,
    },
    Layout {
        kind: 47,
        fields: PROPERTY_KEY,
    },
    Layout {
        kind: 49,
        fields: &[
            Field::new("Target Selector", 2, Byte),
            Field::new("Binding Key", 4, Key),
        ],
    },
    Layout {
        kind: 51,
        fields: OPERATION,
    },
    Layout {
        kind: 52,
        fields: &[
            Field::new("Player Key", 4, Key),
            Field::new("Added Value", 8, Float),
        ],
    },
];

const EVENT_BYTE: &[Field] = &[Field::new("Event Byte", 8, Byte)];
const EVENT_KEY: &[Field] = &[Field::new("Event Key", 8, Key)];
const EVENT_KEY_AT_10: &[Field] = &[Field::new("Event Key", 0x10, Key)];
const SELECTED_BITS: &[Field] = &[Field::new("Selected Bits", 8, Mask32)];

/// The scalar condition kinds. The first eight bytes of every condition are the header the
/// compiler derives (probability, kind and ordinal), so no field maps them. Kinds 0, 1, 2,
/// 14 through 17 and 30 as an ending key have their own trigger and removal handling.
pub const CONDITION_LAYOUTS: &[Layout] = &[
    Layout {
        kind: 0,
        fields: NONE,
    },
    Layout {
        kind: 1,
        fields: &[Field::new("Duration", 8, FieldFormat::Seconds)],
    },
    Layout {
        kind: 6,
        fields: &[
            Field::new("First Flag Mask", 8, Mask8),
            Field::new("Second Flag Mask", 9, Mask8),
        ],
    },
    Layout {
        kind: 9,
        fields: SELECTED_BITS,
    },
    Layout {
        kind: 10,
        fields: EVENT_KEY_AT_10,
    },
    Layout {
        kind: 11,
        fields: EVENT_KEY_AT_10,
    },
    Layout {
        kind: 12,
        fields: &[
            Field::new("Event Value", 8, Key),
            Field::new("Context Key", 0x0C, Key),
        ],
    },
    Layout {
        kind: 22,
        fields: EVENT_BYTE,
    },
    Layout {
        kind: 23,
        fields: &[
            Field::new("Event Byte", 8, Byte),
            Field::new("Host State Filter", 9, Byte),
            Field::new("Requires Owning Weapon", 0x0A, Flag),
        ],
    },
    Layout {
        kind: 24,
        fields: EVENT_BYTE,
    },
    Layout {
        kind: 25,
        fields: EVENT_BYTE,
    },
    Layout {
        kind: 28,
        fields: SELECTED_BITS,
    },
    Layout {
        kind: 29,
        fields: EVENT_KEY,
    },
    Layout {
        kind: 30,
        fields: EVENT_KEY,
    },
    Layout {
        kind: 34,
        fields: &[
            Field::new("Event Key", 0x10, Key),
            Field::new("First Range", 0x18, Range),
            Field::new("Second Range", 0x20, Range),
        ],
    },
    Layout {
        kind: 36,
        fields: EVENT_BYTE,
    },
    Layout {
        kind: 37,
        fields: EVENT_KEY,
    },
    Layout {
        kind: 38,
        fields: &[
            Field::new("Target Key", 8, Key),
            Field::new("Minimum Distance", 0x0C, Float),
        ],
    },
    Layout {
        kind: 42,
        fields: EVENT_BYTE,
    },
];

/// The field map of an effect kind, when the kind is a plain scalar node.
#[must_use]
pub fn effect_layout(kind: u8) -> Option<&'static Layout> {
    EFFECT_LAYOUTS.iter().find(|layout| layout.kind == kind)
}

/// The field map of a condition kind, when the kind is a plain scalar node.
#[must_use]
pub fn condition_layout(kind: u8) -> Option<&'static Layout> {
    CONDITION_LAYOUTS.iter().find(|layout| layout.kind == kind)
}

/// The native size of an effect kind's node.
#[must_use]
pub fn effect_size(kind: u8) -> Option<usize> {
    nodes::effect(kind)
        .filter(|node| node.struct_size != 0)
        .map(|node| node.struct_size as usize)
}

/// The native size of a condition kind's node.
#[must_use]
pub fn condition_size(kind: u8) -> Option<usize> {
    nodes::condition(kind)
        .filter(|node| node.struct_size != 0)
        .map(|node| node.struct_size as usize)
}

/// A fresh node of an effect kind: the kind byte, the retained flag stock nodes of the
/// kind carry, and zeroed fields.
#[must_use]
pub fn blank_effect(kind: u8) -> Option<Vec<u8>> {
    effect_layout(kind)?;
    let mut bytes = vec![0; effect_size(kind)?];
    bytes[0] = kind;
    // The retained byte is not a per-node setting: every stock node of a kind agrees on it.
    // Read it from the captured stock template, which is the same evidence a carried node
    // arrives with, so a blank node and a stock one of the kind cannot disagree. A template
    // can be longer than the node when the kind carries nested records, so only this byte
    // is taken from it.
    bytes[1] = super::native::template(false, kind)
        .and_then(|template| template.get(1).copied())
        .unwrap_or_default();
    Some(bytes)
}

/// A fresh node of a condition kind: the literal probability header and zeroed fields.
#[must_use]
pub fn blank_condition(kind: u8) -> Option<Vec<u8>> {
    condition_layout(kind)?;
    let mut bytes = vec![0; condition_size(kind)?];
    bytes[..4].copy_from_slice(&1.0_f32.to_le_bytes());
    bytes[4] = 0xFF;
    bytes[5] = kind;
    Some(bytes)
}

/// The bytes a fresh node of a kind starts with so that it reads as its plain title. Each
/// is the configuration every stock perk of that reading uses, read off the perks' own
/// activation and end slots.
#[must_use]
pub fn stock_defaults(condition: bool, kind: u8) -> &'static [(usize, u8)] {
    if !condition {
        return match kind {
            // Three host floats at -1.0, the value the callback leaves unchanged, so a fresh
            // radar node changes nothing until its range is set. Zero would replace all
            // three host floats with zero.
            18 => &[
                (0x04, 0x00),
                (0x05, 0x00),
                (0x06, 0x80),
                (0x07, 0xBF),
                (0x08, 0x00),
                (0x09, 0x00),
                (0x0A, 0x80),
                (0x0B, 0xBF),
                (0x0C, 0x00),
                (0x0D, 0x00),
                (0x0E, 0x80),
                (0x0F, 0xBF),
            ],
            // A new Enhanced Radar action adds the stock contribution. Loaded nodes
            // retain their operation, including deliberately subtractive effects.
            20 => &[(2, 1)],
            _ => &[],
        };
    }
    match kind {
        // Reloading: the owning weapon and the reload flag alone, as Kill Clip and 17 others.
        19 => &[(8, 1), (9, 1), (0x0A, 0)],
        // Crouching started, the activation byte of Field Prep, Firmly Planted and Sneak Bow.
        22 => &[(8, 1)],
        // Aiming started on the owning weapon, the activation bytes of Rangefinder and the
        // eleven other aiming perks.
        23 => &[(8, 1), (0x0A, 1)],
        // A shot from the owning weapon, as every stock use of the kind.
        27 => &[(8, 1)],
        // A finisher final blow, as Bulwark Finisher and Empowered Finish.
        42 => &[(8, 1)],
        // Kind 26: the stock norm across 146 accumulator perks. Count Needed 1 fires on the
        // first contribution, Resets At -1 never resets, and the clamps run -9998 to 100. A
        // zero-filled node clamps its counter at zero, so it could never fire.
        26 => &[
            (0x20, 0x00),
            (0x21, 0x00),
            (0x22, 0x80),
            (0x23, 0x3F),
            (0x24, 0x00),
            (0x25, 0x00),
            (0x26, 0x80),
            (0x27, 0xBF),
            (0x28, 0x00),
            (0x29, 0x38),
            (0x2A, 0x1C),
            (0x2B, 0xC6),
            (0x2C, 0x00),
            (0x2D, 0x00),
            (0x2E, 0xC8),
            (0x2F, 0x42),
        ],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layout_fits_its_kind_and_stays_clear_of_the_header() {
        for (layouts, size_of, header) in [
            (EFFECT_LAYOUTS, effect_size as fn(u8) -> Option<usize>, 2),
            (CONDITION_LAYOUTS, condition_size, 8),
        ] {
            for layout in layouts {
                let size = size_of(layout.kind).expect("an observed kind");
                for field in layout.fields {
                    assert!(
                        field.offset >= header,
                        "{} of kind {}",
                        field.label,
                        layout.kind
                    );
                    assert!(
                        field.offset + field.format.width() <= size,
                        "{} of kind {}",
                        field.label,
                        layout.kind
                    );
                    assert!(!field.label.is_empty());
                }
            }
        }
    }

    #[test]
    fn fields_round_trip_through_read_and_write() {
        let layout = condition_layout(34).unwrap();
        let mut node = blank_condition(34).unwrap();
        assert_eq!(&node[..6], &[0, 0, 0x80, 0x3F, 0xFF, 34]);
        let key = layout.field("Event Key").unwrap();
        let range = layout.field("Second Range").unwrap();
        assert!(key.write(&mut node, &FactValue::Key(0x1234_5678)));
        assert!(range.write(&mut node, &FactValue::Range(0.5, 2.0)));
        assert!(!range.write(&mut node, &FactValue::Flag(true)));
        assert_eq!(key.read(&node), Some(FactValue::Key(0x1234_5678)));
        assert_eq!(range.read(&node), Some(FactValue::Range(0.5, 2.0)));
        assert_eq!(layout.facts(&node).len(), 3);
        let effect = blank_effect(42).unwrap();
        assert_eq!(&effect[..2], &[42, 0]);
        assert_eq!(blank_effect(30).unwrap()[1], 1);
        assert!(blank_effect(1).is_none());
    }

    /// A blank node of a kind and a stock node of the same kind must agree on the retained
    /// byte, since no stock node of a kind disagrees with another. The byte is read from the
    /// captured template, so this checks that every kind with a scalar layout has one and
    /// that the blank node leaves the rest of the template's settings zero.
    #[test]
    fn a_blank_effect_carries_the_stock_retained_byte_of_its_kind() {
        for layout in EFFECT_LAYOUTS {
            let blank = blank_effect(layout.kind)
                .unwrap_or_else(|| panic!("effect kind {} has no blank node", layout.kind));
            let template = crate::sandbox_perk::action::native::template(false, layout.kind)
                .unwrap_or_else(|| panic!("effect kind {} has no stock template", layout.kind));
            assert_eq!(blank[0], layout.kind);
            assert_eq!(
                blank[1], template[1],
                "blank effect kind {} disagrees with its stock template",
                layout.kind
            );
        }
    }
}
