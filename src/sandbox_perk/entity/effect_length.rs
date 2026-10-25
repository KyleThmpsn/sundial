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
//! owner. This finds the row that is the length, and for a timer that scales an input the row
//! that scales it. A program of any other shape offers nothing, nor does one whose constants
//! another program of the owner also reads, since an edit would change that program too.
//!
//! In the scaled shape (`3C50`/`3C51` timers, `EDE520` evaluates it, `EDF2E0` converts to
//! ticks), constant 1 is a seconds offset added to the scaled input, not the whole length: a
//! negative offset can still give a positive length. Constant 0 is a plain coefficient of the
//! named input, whose unit is not established.
//!
//! A result of zero or less never ends when the definition's byte at `+0x70` is set: `EDF2E0`
//! tests it at `EDF356` and stops the timer through `EDF1E0`. With the byte clear, the timer
//! ends at once. In the stock ability graphs, every timer that stores -1 has the byte set and
//! none that stores zero does. Unlimited is written as those timers are: -1 in every lane with
//! the byte set, and zero for the scale of a timer that scales an input, so no input makes it
//! positive.
use std::collections::{BTreeMap, BTreeSet};

use crate::runtime::{
    WeaponRuntimeField, WeaponRuntimeFieldSource, WeaponRuntimeGraph, WeaponRuntimePathElement,
    WeaponRuntimeRoot, WeaponRuntimeValue, WeaponRuntimeValueKind,
};

const ROOT: u32 = 0x504E_5200;
const INLINE: u32 = 0x504E_4900;
const POINTER: u32 = 0x504E_5000;
const ELEMENT: u32 = 0x504E_4100;
const ARRAY: u32 = 0x8080_9FBD;
const BYTECODE_ROW: u32 = 0x8080_0009;
const CONSTANT_ROW: u32 = 0x8080_0090;
/// The timer definition, and its byte that makes a length of zero or less never end.
const TIMER_DEFINITION: u32 = 0x8080_3C51;
const UNLIMITED_FLAG: u32 = 0x70;
/// The timer definition and the duration definition that holds it.
const TIMER_CLASSES: [u32; 2] = [TIMER_DEFINITION, 0x8080_3B33];
/// Push constant 0, store.
const PUSH_AND_STORE: [u8; 4] = [0x34, 0x00, 0x3E, 0x00];
/// Push input 1, push constant 0, push constant 1, multiply and add, store.
const SCALE_INPUT_AND_ADD: [u8; 9] = [0x3C, 0x01, 0x34, 0x00, 0x34, 0x01, 0x12, 0x3E, 0x00];

/// The length an Unlimited timer stores, as the stock ones do.
pub const UNLIMITED: f32 = -1.0;

/// One program's rows as the walk lists them: the bytecode by row, and the constant rows.
#[derive(Default)]
struct Rows<'a> {
    bytecode: Vec<(u32, u8)>,
    constants: Vec<(u32, &'a WeaponRuntimeField)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectLength {
    pub owner_tag: u32,
    /// The constant row, a vec4 whose four lanes hold the seconds. For a timer that scales an
    /// input, the seconds it adds to the scaled input.
    pub field: WeaponRuntimeField,
    /// The seconds an input adds per unit, for a timer that scales an input before adding
    /// the length. What feeds the input is not established.
    pub per_input: Option<f32>,
    /// The constant row that scales the input, for a timer that scales one.
    pub input_scale: Option<WeaponRuntimeField>,
    /// The definition's Unlimited flag, when editable bytes of the graph hold it.
    pub unlimited: Option<UnlimitedFlag>,
}

impl EffectLength {
    /// The stock seconds.
    #[must_use]
    pub fn stock(&self) -> f32 {
        seconds(&self.field.value).unwrap_or(f32::NAN)
    }

    /// Whether the stock timer never ends.
    #[must_use]
    pub fn stock_unlimited(&self) -> bool {
        is_unlimited(
            self.stock(),
            self.per_input,
            self.unlimited.as_ref().is_some_and(UnlimitedFlag::stock),
        )
    }

    /// A constant row holding `seconds` in every lane, as the stock rows do.
    #[must_use]
    pub fn encode(seconds: f32) -> WeaponRuntimeValue {
        WeaponRuntimeValue::Vector4Float32Bits([seconds.to_bits(); 4])
    }
}

/// Where a timer definition's Unlimited flag is: the editable bytes that hold it, and its index
/// in them.
#[derive(Clone, Debug, PartialEq)]
pub struct UnlimitedFlag {
    pub field: WeaponRuntimeField,
    pub at: usize,
}

impl UnlimitedFlag {
    /// Whether the flag is set in `value`, a value of its field.
    #[must_use]
    pub fn is_set(&self, value: &WeaponRuntimeValue) -> bool {
        matches!(value, WeaponRuntimeValue::Bytes(bytes)
            if bytes.get(self.at).is_some_and(|byte| *byte != 0))
    }

    /// Whether the stock definition has the flag set.
    #[must_use]
    pub fn stock(&self) -> bool {
        self.is_set(&self.field.value)
    }

    /// `value`, a value of its field, with the flag set or clear and its other bytes kept.
    #[must_use]
    pub fn with(&self, value: &WeaponRuntimeValue, set: bool) -> Option<WeaponRuntimeValue> {
        let WeaponRuntimeValue::Bytes(bytes) = value else {
            return None;
        };
        let mut bytes = bytes.clone();
        *bytes.get_mut(self.at)? = u8::from(set);
        Some(WeaponRuntimeValue::Bytes(bytes))
    }
}

/// Whether a timer never ends: its definition's Unlimited flag is set and its result is zero
/// or less whatever its input. `scale` is the input's coefficient, for a timer that scales one.
#[must_use]
pub fn is_unlimited(seconds: f32, scale: Option<f32>, flag: bool) -> bool {
    flag && seconds <= 0.0 && scale.is_none_or(|scale| scale == 0.0)
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
    // Every program that reads each constant row of an owner, by the classes and offsets to it,
    // which tell a constant buffer two programs share apart.
    let mut readers = BTreeMap::<(u32, u32), BTreeSet<Vec<(u32, u32)>>>::new();
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
            if element.type_handle.get() == CONSTANT_ROW {
                readers
                    .entry((owner, field.owner_offset))
                    .or_default()
                    .insert(
                        program_path
                            .iter()
                            .map(|step| (step.type_handle.get(), step.byte_offset))
                            .collect(),
                    );
            }
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
        for (path, mut rows) in programs {
            rows.bytecode.sort_by_key(|(row, _)| *row);
            rows.constants.sort_by_key(|(row, _)| *row);
            let constants = rows.constants;
            let bytecode = rows
                .bytecode
                .iter()
                .map(|(_, byte)| *byte)
                .collect::<Vec<_>>();
            let (row, input_scale) = if bytecode == PUSH_AND_STORE && constants.len() == 1 {
                (0, None)
            } else if bytecode == SCALE_INPUT_AND_ADD && constants.len() == 2 {
                let scale = constants[0].1;
                if seconds(&scale.value).is_none_or(|value| !value.is_finite()) {
                    continue;
                }
                (1, Some(scale))
            } else {
                continue;
            };
            let (_, field) = constants[row];
            if seconds(&field.value).is_none_or(|value| !value.is_finite()) {
                continue;
            }
            found.push(EffectLength {
                owner_tag: owner,
                field: field.clone(),
                per_input: input_scale.and_then(|scale| seconds(&scale.value)),
                input_scale: input_scale.cloned(),
                unlimited: flag_offset(root, &path).and_then(|offset| flag(graph, owner, offset)),
            });
        }
    }
    // A constant another program also reads would change that program too.
    let unshared = |field: &WeaponRuntimeField, owner: u32| {
        readers
            .get(&(owner, field.owner_offset))
            .is_none_or(|programs| programs.len() == 1)
    };
    found.retain(|length| {
        unshared(&length.field, length.owner_tag)
            && length
                .input_scale
                .as_ref()
                .is_none_or(|scale| unshared(scale, length.owner_tag))
    });
    found
}

/// The owner offset of the Unlimited flag of the timer definition on `path`, when the
/// definition lies in `root` itself rather than behind a pointer.
fn flag_offset(root: &WeaponRuntimeRoot, path: &[WeaponRuntimePathElement]) -> Option<u32> {
    let mut offset = root.owner_offset;
    for (index, step) in path.iter().enumerate() {
        match step.name_hash {
            ROOT if index == 0 => {}
            INLINE => offset = offset.checked_add(step.byte_offset)?,
            _ => return None,
        }
        if step.type_handle.get() == TIMER_DEFINITION {
            return offset.checked_add(UNLIMITED_FLAG);
        }
    }
    None
}

/// The narrowest editable bytes of `owner` in `graph` that hold the byte at `offset`.
fn flag(graph: &WeaponRuntimeGraph, owner: u32, offset: u32) -> Option<UnlimitedFlag> {
    graph
        .resources
        .iter()
        .filter(|resource| resource.owner_tag == owner)
        .flat_map(|resource| std::iter::once(&resource.instance).chain(resource.definition.iter()))
        .chain(
            graph
                .owners
                .iter()
                .filter(|each| each.owner_tag == owner)
                .flat_map(|each| &each.roots),
        )
        .flat_map(|root| &root.fields)
        .filter(|field| {
            matches!(field.kind, WeaponRuntimeValueKind::FixedBytes { .. })
                && matches!(field.value, WeaponRuntimeValue::Bytes(_))
                && offset
                    .checked_sub(field.owner_offset)
                    .is_some_and(|at| at < field.locator.byte_size)
        })
        .min_by_key(|field| field.locator.byte_size)
        .map(|field| UnlimitedFlag {
            field: field.clone(),
            at: (offset - field.owner_offset) as usize,
        })
}
