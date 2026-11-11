//! The perk editor's native declarations: the classes it declares (`classes`), each read from the
//! client's definition table of build 86657.20.08.23.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

mod classes;

#[derive(Clone, Debug)]
pub struct Record {
    pub class: u32,
    pub size: usize,
    pub base: u32,
    pub array: bool,
    /// Inline byte offset, native class and native field-name hash.
    pub members: Vec<(usize, u32, u32)>,
    /// Native layout byte offset and code, without the class operand row after each code 11.
    pub fields: Vec<(usize, u32)>,
}

static RECORDS: OnceLock<Result<BTreeMap<u32, Record>, String>> = OnceLock::new();

pub fn record(class: u32) -> Result<&'static Record, String> {
    let records = RECORDS
        .get_or_init(|| {
            classes::CLASSES
                .iter()
                .map(|&class| Ok((class, declared(class)?)))
                .collect()
        })
        .as_ref()
        .map_err(Clone::clone)?;
    records
        .get(&class)
        .ok_or_else(|| format!("Native class 0x{class:08X} has no recovered perk declaration."))
}

/// `class` as the definition table declares it, with the editor's array flag.
fn declared(class: u32) -> Result<Record, String> {
    let declaration = crate::runtime::native_declaration(class)
        .map_err(|error| format!("Native perk declarations are unavailable: {error}"))?
        .ok_or_else(|| format!("Native class 0x{class:08X} is outside the definition table."))?;
    let mut fields = Vec::with_capacity(declaration.layout.len());
    let mut rows = declaration.layout.iter();
    while let Some(&(offset, code)) = rows.next() {
        match code {
            // The perk editor reads no repeated layout, so a repeat would be misread here.
            10 => {
                return Err(format!(
                    "Native class 0x{class:08X} has a repeated layout the perk editor does not read."
                ));
            }
            11 => {
                fields.push((offset, code));
                rows.next()
                    .filter(|(_, operand)| *operand == 0)
                    .ok_or_else(|| {
                        format!("Native class 0x{class:08X} has a truncated indexed value.")
                    })?;
            }
            _ => fields.push((offset, code)),
        }
    }
    Ok(Record {
        class,
        size: declaration.size,
        base: declaration.base,
        array: classes::ARRAYS.contains(&class),
        members: declaration.members,
        fields,
    })
}

/// Whether `class` is `base` or derives from it through the perk editor's declarations.
pub fn is_a(class: u32, base: u32) -> bool {
    crate::runtime::inherits(class, base, |class| {
        record(class).ok().map(|record| record.base)
    })
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
pub(super) fn template(kind: super::NodeKind) -> Option<Vec<u8>> {
    if kind == super::NodeKind::Condition(33) {
        // Original typed copying accepts this blockless condition allocation and
        // dispatch consumes its explicit kind and byte. It has no stock template.
        return super::super::layout::blank_condition(33);
    }
    let rows = TEMPLATES.get_or_init(|| {
        serde_json::from_str(include_str!("schema/templates.json"))
            .expect("validated native templates")
    });
    let row = rows
        .iter()
        .find(|row| row.0 == kind.is_condition() && row.1 == kind.byte())?;
    let text = &row.4;
    let bytes = (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    // Templates enter the editor in the same physical layout that edits emit.
    // Opening a node must not count string-envelope normalization as an edit.
    let nodes: &[_] = if kind.is_condition() {
        &crate::sandbox_perk::nodes::CONDITIONS
    } else {
        &crate::sandbox_perk::nodes::EFFECTS
    };
    let class = nodes.iter().find(|node| node.kind == kind.byte())?.class;
    super::Graph::read(&bytes, 0, class).ok()?.emit().ok()
}

type Targets = Vec<(u32, usize, Vec<(u32, bool)>)>;
static TARGETS: OnceLock<Targets> = OnceLock::new();
/// Observed concrete classes accepted by the indicated declared pointer field.
pub fn targets(class: u32, field: usize) -> &'static [(u32, bool)] {
    TARGETS
        .get_or_init(|| {
            serde_json::from_str(include_str!("schema/targets.json"))
                .expect("validated native pointer targets")
        })
        .iter()
        .find(|row| row.0 == class && row.1 == field)
        .map_or(&[], |row| row.2.as_slice())
}

/// Polymorphic node slots accept every recovered concrete member of their family. The action
/// root's policy configuration accepts each registered policy's configuration class, including
/// those no stock action selects, such as policy 4's `80803E08`.
pub fn choices(class: u32, field: usize) -> Vec<(u32, bool)> {
    let mut choices = targets(class, field).to_vec();
    if class == crate::sandbox_perk::action::ACTION_ROOT_CLASS
        && field == crate::sandbox_perk::action::POLICY_CONFIGURATION
    {
        choices.extend(
            crate::sandbox_perk::nodes::POLICIES
                .iter()
                .filter(|policy| policy.configuration_class != 0)
                .map(|policy| (policy.configuration_class, false)),
        );
    }
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
                    .filter(|entry| entry.has_layout())
                    .map(|entry| (entry.class, false)),
            );
        }
    }
    choices.sort_unstable();
    choices.dedup();
    choices
}
