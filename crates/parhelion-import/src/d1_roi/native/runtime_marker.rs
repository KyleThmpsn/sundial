//! A weapon runtime's marker rows, compared between the Rise of Iron source runtime and the
//! native runtime the build keeps.
//!
//! Each runtime root entity owns at most one marker set. The source set has resource class
//! `808008FB` with rows of class `80800428` at its definition plus `0xD8`, read with the native
//! row layout. The native set has class `80808507`. A name is comparable when each set holds
//! exactly one row of it and both rows bind the same bone. Callers choose the names, the role
//! each row plays and the wording of their reports.
use super::*;
use crate::tiger::markers::{self, Marker};

/// The source runtime marker set's resource class and its row class.
const SOURCE_MARKERS: u32 = 0x8080_08FB;
const SOURCE_ROW: u32 = 0x8080_0428;
/// The native runtime marker set's resource class.
const NATIVE_MARKERS: u32 = 0x8080_8507;

/// Both runtimes' marker sets and their owners.
pub(super) struct Sets {
    pub(super) source_owner: u32,
    pub(super) native_owner: u32,
    source: Vec<Marker>,
    native: Vec<Marker>,
}

/// What comparing one name's rows found.
pub(super) enum Comparison {
    /// One row each, same binding and bit-identical transforms.
    Same,
    /// One row each with the same binding and a different transform, with the distance the
    /// position moves from the native row to the source row.
    Moved {
        from: Marker,
        to: Marker,
        moved: f32,
    },
    /// The name is absent or repeated in a set, so its native row stays.
    Counts { source: usize, native: usize },
    /// The rows bind different bones, so the native row stays.
    Bindings { from: Marker, to: Marker },
}

impl Sets {
    /// Reads the marker set of `source_runtime`'s root entity and of `native_rig`'s, or `None`
    /// when either root entity has no marker set.
    pub(super) fn read(
        source: &Source,
        native: &mut Reader,
        source_runtime: &Value,
        native_rig: &Value,
    ) -> Result<Option<Self>> {
        let (Some(source_owner), Some(native_owner)) = (
            owner(source_runtime, SOURCE_MARKERS, true)?,
            owner(native_rig, NATIVE_MARKERS, false)?,
        ) else {
            return Ok(None);
        };
        let source_markers = source.read(source_owner)?;
        let from = markers::read(
            &source_markers,
            source_markers.pointer(0x18)? + 0xD8,
            markers::Layout {
                row_class: SOURCE_ROW,
                ..markers::NATIVE
            },
        )?;
        let native_markers = native.tag(native_owner, Some(0x80809C36))?;
        let to = markers::read_native(&native_markers)?;
        Ok(Some(Self {
            source_owner,
            native_owner,
            source: from,
            native: to,
        }))
    }

    /// Compares the source and native rows of `name`.
    pub(super) fn compare(&self, name: u32) -> Comparison {
        let named = |set: &[Marker]| {
            set.iter()
                .filter(|m| m.name == name)
                .copied()
                .collect::<Vec<_>>()
        };
        let (source_rows, native_rows) = (named(&self.source), named(&self.native));
        let (&[from], &[to]) = (source_rows.as_slice(), native_rows.as_slice()) else {
            return Comparison::Counts {
                source: source_rows.len(),
                native: native_rows.len(),
            };
        };
        if from.binding != to.binding {
            return Comparison::Bindings { from, to };
        }
        let bits = |values: [f32; 4]| values.map(f32::to_bits);
        if bits(from.position) == bits(to.position)
            && bits(from.orientation) == bits(to.orientation)
        {
            return Comparison::Same;
        }
        let moved = (0..3)
            .map(|axis| (from.position[axis] - to.position[axis]).powi(2))
            .sum::<f32>()
            .sqrt();
        Comparison::Moved { from, to, moved }
    }
}

/// The only owner of resource class `class` on the runtime root entity, if any. Source rows name
/// tags as numbers and native rows as hexadecimal text.
fn owner(runtime: &Value, class: u32, source: bool) -> Result<Option<u32>> {
    let root = u32::from_str_radix(
        runtime["runtime_entity"]
            .as_str()
            .context("Runtime entity")?,
        16,
    )?;
    let tag = |v: &Value| -> Result<u32> {
        if source {
            Ok(u32::try_from(v.as_u64().context("Source component tag")?)?)
        } else {
            Ok(u32::from_str_radix(
                v.as_str().context("Native component tag")?,
                16,
            )?)
        }
    };
    let mut found = Vec::new();
    for c in runtime["components"]
        .as_array()
        .context("Runtime components")?
    {
        if tag(&c["entity"])? == root && tag(&c["class"])? == class {
            found.push(tag(&c["owner"])?);
        }
    }
    ensure!(
        found.len() <= 1,
        "Runtime entity has multiple {class:08X} marker sets"
    );
    Ok(found.first().copied())
}
