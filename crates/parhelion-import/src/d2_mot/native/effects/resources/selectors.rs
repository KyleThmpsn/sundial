//! Conditional enum resources with explicit dictionary and category bindings.
use super::{Payload, Reference};
use crate::d2_mot::native::effects::put;
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

mod fragment;
mod read;
pub use fragment::{
    Fragment, FragmentContract, GroupBindings, append_category, append_value_selector,
    emit_with_groups,
};
mod resolver;
#[cfg(test)]
mod tests;
mod write;
pub use resolver::ResourceResolver;

pub const MODERN_CLASS: u32 = 0x808091B2;
pub const NATIVE_CLASS: u32 = 0x80809311;

pub struct Resources {
    pub tags: BTreeMap<u32, u32>,
    pub names: Vec<u32>,
    pub categories: Vec<Option<u16>>,
}

impl Resources {
    /// Use one validated private dictionary correspondence for both name rows
    /// and compiled masks. The caller supplies completed dictionary tag bindings.
    pub fn from_namespace(
        namespace: &crate::d2_mot::native::categories::Namespace,
        tags: BTreeMap<u32, u32>,
    ) -> Self {
        Self {
            tags,
            names: namespace.source_names.clone(),
            categories: namespace.source_indices.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct Gate {
    pub offset: usize,
    pub index: usize,
    pub name: u32,
}

pub struct Selector {
    pub payload: Payload,
    pub references: Vec<Reference>,
    pub gates: Vec<Gate>,
    /// Named input publication must be verified by the complete assembly caller.
    pub named_conditions: Vec<u32>,
    /// Native entry classes required by selector and dictionary dependency slots.
    pub external_classes: BTreeMap<usize, u32>,
    pub group_aliases: Vec<GroupAliasUse>,
}

#[derive(Serialize)]
pub struct GroupAliasUse {
    pub source: u32,
    pub native: u32,
    pub source_offset: usize,
    pub native_offset: usize,
}

struct Mask {
    source: usize,
    words: Vec<u32>,
    tail: [u8; 8],
}

struct Row {
    source: usize,
    name: u32,
    debug: Option<String>,
    dictionary: u32,
}

enum Node {
    External {
        debug: Option<String>,
        source: u32,
    },
    Raw {
        class: u32,
        data: Vec<u8>,
    },
    Selector {
        flags: u64,
        rows: Vec<(u64, Node)>,
    },
    Factions(Vec<u8>),
    Category {
        rows: [Vec<Row>; 4],
        debug: Option<String>,
        dictionary: u32,
        kind: u64,
        masks: Option<Mask>,
    },
}

/// Emit inspected selector forms. Missing bindings and category names remain
/// explicit and must prevent complete entity assembly.
pub fn emit(source: &Payload, resources: &Resources) -> Result<Selector> {
    emit_checked(source, resources, None, None)
}

/// Resolve external selector references using source and native entry metadata.
/// Hash resolution identifies a source resource, never a native resource.
pub fn emit_with_resolver(
    source: &Payload,
    resources: &Resources,
    resolver: &ResourceResolver,
) -> Result<Selector> {
    emit_checked(source, resources, Some(resolver), None)
}

fn emit_checked(
    source: &Payload,
    resources: &Resources,
    resolver: Option<&ResourceResolver>,
    groups: Option<&GroupBindings>,
) -> Result<Selector> {
    ensure!(
        source.u64(0)? == source.0.len() as u64,
        "selector payload size differs"
    );
    ensure!(source.u64(32)? == 0, "selector root padding differs");
    ensure!(
        resources.names.len() == resources.categories.len() && resources.names.len() <= 448,
        "selector category correspondence differs"
    );
    let mut read = read::Read::new(source, resolver);
    read.claim(0, 40)?;
    let root = read.selector(8, 0)?;
    read.finish()?;
    let write = write::Write::new(resources, resolver);
    let write = if let Some(groups) = groups {
        write.with_groups(groups)
    } else {
        write
    };
    write.finish(root)
}

/// Supply both external resource metadata and validated named group membership.
pub fn emit_with_bindings(
    source: &Payload,
    resources: &Resources,
    resolver: &ResourceResolver,
    groups: &GroupBindings,
) -> Result<Selector> {
    emit_checked(source, resources, Some(resolver), Some(groups))
}
