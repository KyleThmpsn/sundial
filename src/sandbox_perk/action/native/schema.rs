//! Declarations recovered from build 86657.20.08.23 and exercised by stock perk data.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

#[derive(Clone, Debug)]
pub struct Record {
    pub class: u32,
    pub size: usize,
    pub base: u32,
    pub array: bool,
    /// Inline byte offset, native class and native field-name hash.
    pub members: Vec<(usize, u32, u32)>,
    /// Expanded native layout byte offset and declaration opcode.
    pub fields: Vec<(usize, u32)>,
}

#[cfg(test)]
mod decompile_coverage;
#[cfg(test)]
mod field_map;
#[cfg(test)]
mod key_harvest;

type Encoded = (
    u32,
    usize,
    u32,
    bool,
    Vec<(usize, u32, u32)>,
    Vec<(usize, u32)>,
);
static RECORDS: OnceLock<Result<BTreeMap<u32, Record>, String>> = OnceLock::new();

pub fn record(class: u32) -> Result<&'static Record, String> {
    let records = RECORDS
        .get_or_init(|| {
            let rows: Vec<Encoded> = serde_json::from_str(include_str!("schema.json"))
                .map_err(|error| format!("Native perk declarations are invalid: {error}"))?;
            let mut result = BTreeMap::new();
            for (class, size, base, array, members, fields) in rows {
                if result
                    .insert(
                        class,
                        Record {
                            class,
                            size,
                            base,
                            array,
                            members,
                            fields,
                        },
                    )
                    .is_some()
                {
                    return Err("Duplicate native perk declaration.".into());
                }
            }
            Ok(result)
        })
        .as_ref()
        .map_err(Clone::clone)?;
    records
        .get(&class)
        .ok_or_else(|| format!("Native class 0x{class:08X} has no recovered perk declaration."))
}

pub fn is_a(mut class: u32, base: u32) -> bool {
    let mut visited = BTreeSet::new();
    while visited.insert(class) {
        if class == base {
            return true;
        }
        let Ok(record) = record(class) else {
            return false;
        };
        class = record.base;
    }
    false
}

/// Inline native records, including their inherited views, with stable member routes.
pub fn inline(class: u32) -> Result<Vec<(usize, u32, Vec<u32>)>, String> {
    let mut result = Vec::new();
    let mut pending = vec![(0, class, Vec::new())];
    let mut seen = BTreeSet::new();
    while let Some((offset, class, route)) = pending.pop() {
        if !seen.insert((offset, class)) {
            continue;
        }
        if seen.len() > 4096 {
            return Err("Native inline declarations contain a cycle.".into());
        }
        let row = record(class)?;
        result.push((offset, class, route.clone()));
        if record(row.base).is_ok() {
            let mut path = route.clone();
            path.push(0);
            pending.push((offset, row.base, path));
        }
        for &(at, child, name) in &row.members {
            if at
                .checked_add(record(child)?.size)
                .is_none_or(|end| end > row.size)
            {
                return Err("An inline native field exceeds its object.".into());
            }
            let mut path = route.clone();
            path.push(name);
            pending.push((offset + at, child, path));
        }
    }
    Ok(result)
}

type Template = (bool, u8, u32, usize, String);
static TEMPLATES: OnceLock<Vec<Template>> = OnceLock::new();
pub(super) fn template(condition: bool, kind: u8) -> Option<Vec<u8>> {
    let rows = TEMPLATES.get_or_init(|| {
        serde_json::from_str(include_str!("templates.json")).expect("validated native templates")
    });
    let text = &rows
        .iter()
        .find(|row| row.0 == condition && row.1 == kind)?
        .4;
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok())
        .collect()
}

type Targets = Vec<(u32, usize, Vec<(u32, bool)>)>;
static TARGETS: OnceLock<Targets> = OnceLock::new();
/// Observed concrete classes accepted by the indicated declared pointer field.
pub fn targets(class: u32, field: usize) -> &'static [(u32, bool)] {
    TARGETS
        .get_or_init(|| {
            serde_json::from_str(include_str!("targets.json"))
                .expect("validated native pointer targets")
        })
        .iter()
        .find(|row| row.0 == class && row.1 == field)
        .map_or(&[], |row| row.2.as_slice())
}

/// Polymorphic node slots accept every recovered concrete member of their family.
pub fn choices(class: u32, field: usize) -> Vec<(u32, bool)> {
    let mut choices = targets(class, field).to_vec();
    for (base, entries) in [
        (
            0x808040BC,
            crate::sandbox_perk::nodes::CONDITIONS.as_slice(),
        ),
        (0x808040AE, crate::sandbox_perk::nodes::EFFECTS.as_slice()),
    ] {
        if choices
            .iter()
            .any(|(target, array)| !array && is_a(*target, base))
        {
            choices.extend(
                entries
                    .iter()
                    .filter(|entry| entry.observed())
                    .map(|entry| (entry.class, false)),
            );
        }
    }
    choices.sort_unstable();
    choices.dedup();
    choices
}
