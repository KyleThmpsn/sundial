use super::{Program, vocabulary};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default, Serialize)]
pub struct Analysis {
    pub startup: Vec<usize>,
    pub continuous: Vec<usize>,
    pub scripts: Vec<Dependencies>,
    pub global_initializers: Vec<Dependencies>,
    pub operations: Vec<Operation>,
    pub unknown_expression_kinds: BTreeSet<u16>,
    pub unknown_value_types: BTreeSet<u16>,
    pub absent_script_roots: Vec<usize>,
    pub limits: Vec<&'static str>,
}

#[derive(Debug, Default, Serialize)]
pub struct Dependencies {
    pub calls: BTreeSet<usize>,
    pub script_values: BTreeSet<usize>,
    pub globals: BTreeSet<usize>,
    pub external_globals: BTreeMap<u16, Option<String>>,
    pub operations: BTreeSet<u16>,
    pub tag_values: BTreeSet<u32>,
    pub unresolved_script_values: BTreeSet<u32>,
}

#[derive(Debug, Serialize)]
pub struct Operation {
    pub opcode: u16,
    pub name: Option<String>,
    pub call_sites: usize,
    pub target_binding: &'static str,
}

pub(super) fn read(program: &Program) -> Result<Analysis> {
    let vocabulary = vocabulary::get();
    let mut result = Analysis::default();
    let mut operations = BTreeMap::<u16, usize>::new();
    for node in program.nodes.iter().flatten() {
        if !matches!(node.kind(), 8 | 9 | 10 | 13 | 29) {
            result.unknown_expression_kinds.insert(node.kind());
        }
        if !vocabulary.types.contains_key(&node.value_type) {
            result.unknown_value_types.insert(node.value_type);
        }
        if node.kind() == 8 {
            *operations.entry(node.opcode).or_default() += 1;
        }
    }
    for (index, script) in program.scripts.iter().enumerate() {
        match script.kind {
            0 => result.startup.push(index),
            2 => result.continuous.push(index),
            _ => {}
        }
        if script.root.is_none() {
            result.absent_script_roots.push(index);
        }
        result.scripts.push(dependencies(
            program,
            script.root,
            Some(script.parameters.len()),
        )?);
    }
    result.operations = operations
        .into_iter()
        .map(|(opcode, call_sites)| Operation {
            opcode,
            name: vocabulary.functions.get(&opcode).cloned(),
            call_sites,
            target_binding: "unsupported",
        })
        .collect();
    result.limits = vec![
        "Compiled source graph and direct dependencies only. Selected execution is reported separately.",
        "Script values include wake and command-script targets, without proving when they execute.",
        "Source scenario and tag identities require target bindings before execution.",
        "AI, objectives, checkpoints, streaming, dialogue and cinematics remain untranslated.",
    ];
    // Global initializer structure is validated even though it has no script parameters.
    for global in &program.globals {
        result
            .global_initializers
            .push(dependencies(program, global.root, None)?);
    }
    Ok(result)
}

fn dependencies(
    program: &Program,
    root: Option<u32>,
    parameters: Option<usize>,
) -> Result<Dependencies> {
    let vocabulary = vocabulary::get();
    let mut result = Dependencies::default();
    let mut visited = BTreeSet::new();
    let mut pending: Vec<_> = root.into_iter().collect();
    while let Some(datum) = pending.pop() {
        if !visited.insert(datum) {
            continue;
        }
        let node = program.node(datum)?;
        pending.extend(node.next.into_iter().chain(node.child()));
        match node.kind() {
            8 => {
                result.operations.insert(node.opcode);
            }
            10 => {
                result.calls.insert(usize::from(node.opcode));
            }
            13 => {
                let index = node.value as u16;
                if index & 0x8000 == 0 {
                    result.globals.insert(usize::from(index));
                } else {
                    let index = index & 0x7FFF;
                    result
                        .external_globals
                        .insert(index, vocabulary.globals.get(&index).cloned());
                }
            }
            29 => {
                ensure!(
                    parameters.is_some_and(|count| (node.value as usize) < count),
                    "Mission parameter reference is outside its script"
                );
            }
            9 if node.value_type != 2 && matches!(node.literal_type(), 10 | 21) => {
                if node.value as u16 == u16::MAX {
                    result.unresolved_script_values.insert(datum);
                } else {
                    result.script_values.insert(usize::from(node.value as u16));
                }
            }
            9 if node.value_type != 2
                && vocabulary
                    .types
                    .get(&node.literal_type())
                    .is_some_and(|ty| ty.tag.is_some())
                && node.value != u32::MAX =>
            {
                result.tag_values.insert(node.value);
            }
            _ => {}
        }
    }
    Ok(result)
}
