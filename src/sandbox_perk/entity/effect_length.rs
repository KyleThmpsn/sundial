//! Effect Length: how long an attached effect lasts, from the constant its self-destruct
//! timer stores.
//!
//! A hop-on's Self-Destruct Timer (class `80803C50`) reaches, through its definition
//! (`80803C51`), a value program (`808089F6`) with two arrays: bytecode rows (`80800009`) and
//! constant vec4 rows (`80800090`), each held one step under the program. Devour's program is
//! `34 00 3E 00`, push constant 0 and store, with 11.0 in every lane. Invisibility's is
//! `3C 01 34 00 34 01 12 3E 00`, input 1 times constant 0 plus constant 1, with 2.0 and 6.0.
//! The rows reach the runtime graph as native declarations under the timer's definition, so
//! an edit is an ordinary field override that the build writes into a private copy of the
//! owner. This finds the row that is the length; a program of any other shape offers nothing.
use std::collections::BTreeMap;

use crate::runtime::{
    WeaponRuntimeField, WeaponRuntimeFieldSource, WeaponRuntimeGraph, WeaponRuntimePathElement,
    WeaponRuntimeValue,
};

const POINTER: u32 = 0x504E_5000;
const ELEMENT: u32 = 0x504E_4100;
const ARRAY: u32 = 0x8080_9FBD;
const BYTECODE_ROW: u32 = 0x8080_0009;
const CONSTANT_ROW: u32 = 0x8080_0090;
/// The timer definition and the duration definition that holds it.
const TIMER_CLASSES: [u32; 2] = [0x8080_3C51, 0x8080_3B33];
/// Push constant 0, store.
const PUSH_AND_STORE: [u8; 4] = [0x34, 0x00, 0x3E, 0x00];
/// Push input 1, push constant 0, push constant 1, multiply and add, store.
const SCALE_INPUT_AND_ADD: [u8; 9] = [0x3C, 0x01, 0x34, 0x00, 0x34, 0x01, 0x12, 0x3E, 0x00];

/// One program's rows as the walk lists them: the bytecode by row, and the constant rows.
#[derive(Default)]
struct Rows<'a> {
    bytecode: Vec<(u32, u8)>,
    constants: Vec<(u32, &'a WeaponRuntimeField)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectLength {
    pub owner_tag: u32,
    /// The constant row, a vec4 whose four lanes hold the seconds.
    pub field: WeaponRuntimeField,
    /// The seconds an input adds per unit, for a timer that scales an input before adding
    /// the length. What feeds the input is not established.
    pub per_input: Option<f32>,
}

impl EffectLength {
    /// The stock seconds.
    #[must_use]
    pub fn stock(&self) -> f32 {
        seconds(&self.field.value).unwrap_or(f32::NAN)
    }

    /// A constant row holding `seconds` in every lane, as the stock rows do.
    #[must_use]
    pub fn encode(seconds: f32) -> WeaponRuntimeValue {
        WeaponRuntimeValue::Vector4Float32Bits([seconds.to_bits(); 4])
    }
}

/// The seconds a constant row holds, when its lanes agree as the stock rows do.
#[must_use]
pub fn seconds(value: &WeaponRuntimeValue) -> Option<f32> {
    let WeaponRuntimeValue::Vector4Float32Bits(lanes) = value else {
        return None;
    };
    lanes[1..]
        .iter()
        .all(|lane| *lane == lanes[0])
        .then(|| f32::from_bits(lanes[0]))
}

/// Every timer in the graph whose program has one of the known shapes.
#[must_use]
pub fn discover(graph: &WeaponRuntimeGraph) -> Vec<EffectLength> {
    let roots = graph
        .resources
        .iter()
        .flat_map(|resource| {
            std::iter::once(&resource.instance)
                .chain(resource.definition.iter())
                .map(move |root| (resource.owner_tag, root))
        })
        .chain(
            graph
                .owners
                .iter()
                .flat_map(|owner| owner.roots.iter().map(move |root| (owner.owner_tag, root))),
        );
    let mut found = Vec::new();
    for (owner, root) in roots {
        // The rows of each program, by the path to it. An array sits in a holder one step
        // under the program, so the program is two steps above the array pointer.
        let mut programs: BTreeMap<Vec<WeaponRuntimePathElement>, Rows<'_>> = BTreeMap::new();
        for field in &root.fields {
            if field.source != WeaponRuntimeFieldSource::NativeDeclaration
                || field.locator.value_offset != 0
            {
                continue;
            }
            let path = &field.locator.path;
            let Some(pointer) = path
                .iter()
                .rposition(|step| step.name_hash == POINTER && step.type_handle.get() == ARRAY)
                .filter(|pointer| *pointer >= 1)
            else {
                continue;
            };
            let Some(element) = path
                .get(pointer + 1)
                .filter(|step| step.name_hash == ELEMENT)
            else {
                continue;
            };
            let program_path = &path[..pointer - 1];
            if !program_path
                .iter()
                .any(|step| TIMER_CLASSES.contains(&step.type_handle.get()))
            {
                continue;
            }
            let program = programs.entry(program_path.to_vec()).or_default();
            match (element.type_handle.get(), &field.value) {
                (BYTECODE_ROW, WeaponRuntimeValue::Unsigned(byte)) => program
                    .bytecode
                    .push((element.byte_offset, u8::try_from(*byte).unwrap_or(u8::MAX))),
                (CONSTANT_ROW, WeaponRuntimeValue::Vector4Float32Bits(_)) => {
                    program.constants.push((element.byte_offset, field));
                }
                _ => {}
            }
        }
        for mut rows in programs.into_values() {
            rows.bytecode.sort_by_key(|(row, _)| *row);
            rows.constants.sort_by_key(|(row, _)| *row);
            let constants = rows.constants;
            let bytecode = rows
                .bytecode
                .iter()
                .map(|(_, byte)| *byte)
                .collect::<Vec<_>>();
            let (row, per_input) = if bytecode == PUSH_AND_STORE {
                (0, None)
            } else if bytecode == SCALE_INPUT_AND_ADD {
                let Some(scale) = constants
                    .first()
                    .and_then(|(_, field)| seconds(&field.value))
                else {
                    continue;
                };
                (1, Some(scale))
            } else {
                continue;
            };
            let Some((_, field)) = constants.get(row) else {
                continue;
            };
            if seconds(&field.value).is_none_or(|value| !value.is_finite()) {
                continue;
            }
            found.push(EffectLength {
                owner_tag: owner,
                field: (*field).clone(),
                per_input,
            });
        }
    }
    found
}
