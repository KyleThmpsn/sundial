//! Explicit creation and destruction of named scenario objects. A static map cannot preserve
//! both states of a manually placed object. This is source dependency analysis, not execution.
use super::{Node, Program, vocabulary};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

#[derive(Default, Serialize)]
pub(in crate::halo_reach::level) struct Lifetime {
    pub name: String,
    pub create: Vec<u32>,
    pub destroy: Vec<u32>,
}

#[derive(Default, Serialize)]
pub(in crate::halo_reach::level) struct Lifetimes {
    /// Scenario object-name indices with both explicit creation and destruction calls.
    pub transient: BTreeMap<u16, Lifetime>,
    /// Calls with at least one unknown argument context, including unbound script roots.
    pub unresolved: Vec<u32>,
    /// Static helper calls with their separately resolved object-name arguments.
    pub forwarded: Vec<Call>,
}

#[derive(Serialize)]
pub(in crate::halo_reach::level) struct Call {
    pub expression: u32,
    pub script: usize,
    pub arguments: Vec<Option<u16>>,
}

fn operation(opcode: u16) -> Option<bool> {
    match vocabulary::get().functions.get(&opcode).map(String::as_str) {
        Some("object_create" | "object_create_anew" | "object_create_if_necessary") => Some(true),
        Some("object_destroy") => Some(false),
        _ => None,
    }
}

/// Only scripts that contain or call lifetime operations need separate argument contexts.
fn relevant(program: &Program) -> BTreeSet<usize> {
    let mut selected = BTreeSet::new();
    loop {
        let before = selected.len();
        for (index, dependency) in program.analysis.scripts.iter().enumerate() {
            if dependency
                .operations
                .iter()
                .any(|&op| operation(op).is_some())
                || dependency
                    .calls
                    .iter()
                    .any(|script| selected.contains(script))
            {
                selected.insert(index);
            }
        }
        if selected.len() == before {
            return selected;
        }
    }
}

type Frame = (usize, Vec<Option<u16>>);

struct Reader<'a> {
    program: &'a Program,
    literals: BTreeMap<u32, u16>,
    names: BTreeMap<u16, String>,
    scripts: BTreeSet<usize>,
    pending: Vec<Frame>,
    visited: BTreeSet<Frame>,
    forwarded: BTreeSet<(u32, usize, Vec<Option<u16>>)>,
    unresolved: BTreeSet<u32>,
    result: Lifetimes,
    steps: usize,
}

impl Reader<'_> {
    fn arguments(&self, node: &Node) -> Result<Vec<u32>> {
        let mut next = self.program.node(node.value)?.next;
        let mut arguments = Vec::new();
        while let Some(datum) = next {
            ensure!(
                arguments.len() < 256,
                "Object lifetime call exceeds the argument limit"
            );
            arguments.push(datum);
            next = self.program.node(datum)?.next;
        }
        Ok(arguments)
    }

    fn object(&self, datum: u32, parameters: &[Option<u16>]) -> Result<Option<u16>> {
        let node = self.program.node(datum)?;
        Ok(if node.kind() == 29 {
            parameters.get(node.value as usize).copied().flatten()
        } else {
            self.literals.get(&datum).copied()
        })
    }

    fn record(&mut self, node: &Node, parameters: &[Option<u16>]) -> Result<()> {
        let Some(create) = operation(node.opcode) else {
            return Ok(());
        };
        let arguments = self.arguments(node)?;
        ensure!(
            arguments.len() == 1,
            "Object lifetime call requires one argument"
        );
        let Some(index) = self.object(arguments[0], parameters)? else {
            self.unresolved.insert(node.datum);
            return Ok(());
        };
        let lifetime = self.result.transient.entry(index).or_default();
        lifetime.name = self.names[&index].clone();
        if create {
            lifetime.create.push(node.datum);
        } else {
            lifetime.destroy.push(node.datum);
        }
        Ok(())
    }

    fn enqueue(&mut self, frame: Frame) -> Result<()> {
        if self.visited.insert(frame.clone()) {
            ensure!(
                self.visited.len() <= 65_536,
                "Object lifetime analysis exceeds its call-frame limit"
            );
            self.pending.push(frame);
        }
        Ok(())
    }

    fn forward(&mut self, node: &Node, parameters: &[Option<u16>]) -> Result<()> {
        let index = usize::from(node.opcode);
        if !self.scripts.contains(&index) {
            return Ok(());
        }
        let script = &self.program.scripts[index];
        ensure!(
            script.kind == 3,
            "Object lifetime call requires a static script"
        );
        let arguments = self.arguments(node)?;
        ensure!(
            arguments.len() == script.parameters.len(),
            "Object lifetime call to {} has the wrong argument count",
            script.name
        );
        let values = arguments
            .iter()
            .zip(&script.parameters)
            .map(|(&argument, parameter)| {
                // Object and object-name types preserve the decoded source identity. A scalar
                // conversion or a runtime expression does not establish an object-name binding.
                if matches!(parameter.value_type, 71..=84) {
                    self.object(argument, parameters)
                } else {
                    Ok(None)
                }
            })
            .collect::<Result<Vec<_>>>()?;
        self.forwarded.insert((node.datum, index, values.clone()));
        self.enqueue((index, values))
    }

    fn walk(&mut self, root: u32, mut parameters: Vec<Option<u16>>) -> Result<()> {
        let mut pending = vec![root];
        let mut expressions = BTreeSet::new();
        while let Some(datum) = pending.pop() {
            if !expressions.insert(datum) {
                continue;
            }
            self.steps += 1;
            ensure!(
                self.steps <= 2_000_000,
                "Object lifetime analysis exceeds its work limit"
            );
            let node = self.program.node(datum)?;
            pending.extend(node.next.into_iter().chain(node.child()));
            // No source control-flow execution is claimed. If a parameter can be assigned,
            // its initial call argument is not a constant for any lifetime in this frame.
            if node.kind() == 8
                && vocabulary::get()
                    .functions
                    .get(&node.opcode)
                    .map(String::as_str)
                    == Some("set")
                && let Some(&target) = self.arguments(node)?.first()
            {
                let target = self.program.node(target)?;
                if target.kind() == 29
                    && let Some(value) = parameters.get_mut(target.value as usize)
                {
                    *value = None;
                }
            }
        }
        for datum in expressions {
            let node = self.program.node(datum)?;
            match node.kind() {
                8 => self.record(node, &parameters)?,
                10 => self.forward(node, &parameters)?,
                _ => {}
            }
        }
        Ok(())
    }
}

/// Follows literal object names through positional static-script arguments. Every distinct
/// frame is visited once, so recursive calls do not recurse on the native stack or merge
/// unrelated arguments. Branches, folders, globals and computed return values are not evaluated.
pub(in crate::halo_reach::level) fn read(program: &Program) -> Result<Lifetimes> {
    let mut reader = Reader {
        program,
        literals: BTreeMap::new(),
        names: BTreeMap::new(),
        scripts: relevant(program),
        pending: Vec::new(),
        visited: BTreeSet::new(),
        forwarded: BTreeSet::new(),
        unresolved: BTreeSet::new(),
        result: Lifetimes::default(),
        steps: 0,
    };
    for binding in &program.bindings.named {
        if binding.domain == "object_name"
            && let Some(name) = &binding.name
        {
            let index = u16::try_from(binding.index)?;
            reader.literals.insert(binding.expression, index);
            reader.names.insert(index, name.clone());
        }
    }
    // Keep direct literal evidence even for source expressions without a script root.
    for node in program
        .nodes
        .iter()
        .flatten()
        .filter(|node| node.kind() == 8)
    {
        reader.record(node, &[])?;
    }
    for index in reader.scripts.clone() {
        reader.enqueue((index, vec![None; program.scripts[index].parameters.len()]))?;
    }
    for global in &program.globals {
        if let Some(root) = global.root {
            reader.walk(root, Vec::new())?;
        }
    }
    while let Some((index, parameters)) = reader.pending.pop() {
        if let Some(root) = program.scripts[index].root {
            reader.walk(root, parameters)?;
        }
    }
    let mut result = reader.result;
    result
        .transient
        .retain(|_, lifetime| !lifetime.create.is_empty() && !lifetime.destroy.is_empty());
    for lifetime in result.transient.values_mut() {
        lifetime.create.sort_unstable();
        lifetime.create.dedup();
        lifetime.destroy.sort_unstable();
        lifetime.destroy.dedup();
    }
    result.unresolved = reader.unresolved.into_iter().collect();
    result.forwarded = reader
        .forwarded
        .into_iter()
        .map(|(expression, script, arguments)| Call {
            expression,
            script,
            arguments,
        })
        .collect();
    Ok(result)
}
