//! Constant scenery pose assignments retained from mission code. This is a presentation
//! analysis, not script execution. Unknown branches contribute every possible assignment and
//! objects with conflicting assignments keep their original pose.
use super::{Node, Program, vocabulary};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(in crate::halo_reach::level) struct Pose {
    pub graph: u32,
    pub clip: String,
}

#[derive(Default, Serialize)]
pub(in crate::halo_reach::level) struct Poses {
    pub objects: BTreeMap<u16, Pose>,
    pub conflicting: BTreeSet<u16>,
}

#[derive(Clone)]
struct Constant {
    bits: u32,
    text: Option<String>,
}

struct Reader<'a> {
    program: &'a Program,
    mutable: BTreeSet<usize>,
    scripts: BTreeSet<usize>,
    candidates: BTreeMap<u16, BTreeSet<Pose>>,
    steps: usize,
}

impl Reader<'_> {
    fn arguments(&self, node: &Node) -> Result<Vec<u32>> {
        let mut next = self.program.node(node.value)?.next;
        let mut args = Vec::new();
        while let Some(datum) = next {
            ensure!(
                args.len() < 256,
                "Scenery pose call exceeds the argument limit"
            );
            args.push(datum);
            next = self.program.node(datum)?.next;
        }
        Ok(args)
    }

    fn expression(
        &mut self,
        datum: u32,
        parameters: &[Option<Constant>],
        depth: usize,
    ) -> Result<Option<Constant>> {
        self.steps += 1;
        ensure!(
            self.steps <= 2_000_000,
            "Scenery pose analysis exceeds its work limit"
        );
        ensure!(
            depth < 128,
            "Scenery pose analysis has recursive or excessive nesting"
        );
        let node = self.program.node(datum)?;
        match node.kind() {
            9 if node.value_type != 2 => Ok(Some(Constant {
                bits: node.value,
                text: node.text.clone(),
            })),
            29 => Ok(parameters.get(node.value as usize).cloned().flatten()),
            13 => {
                let index = usize::from(node.value as u16);
                if self.mutable.contains(&index) {
                    return Ok(None);
                }
                match self
                    .program
                    .globals
                    .get(index)
                    .and_then(|global| global.root)
                {
                    Some(root) => self.expression(root, &[], depth + 1),
                    None => Ok(None),
                }
            }
            10 => {
                if !self.scripts.contains(&usize::from(node.opcode)) {
                    return Ok(None);
                }
                let script = &self.program.scripts[usize::from(node.opcode)];
                if script.kind != 3 {
                    return Ok(None);
                }
                let root = script.root;
                let args = self.arguments(node)?;
                let values = args
                    .iter()
                    .map(|&arg| self.expression(arg, parameters, depth + 1))
                    .collect::<Result<Vec<_>>>()?;
                match root {
                    Some(root) => self.expression(root, &values, depth + 1),
                    None => Ok(None),
                }
            }
            8 => {
                let operation = vocabulary::get()
                    .functions
                    .get(&node.opcode)
                    .map(String::as_str);
                let args = self.arguments(node)?;
                match operation {
                    Some("if") if args.len() >= 2 => self.conditional(&args, parameters, depth),
                    Some("=" | "!=") if args.len() == 2 => {
                        let a = self.expression(args[0], parameters, depth + 1)?;
                        let b = self.expression(args[1], parameters, depth + 1)?;
                        Ok(a.zip(b).map(|(a, b)| Constant {
                            bits: u32::from((a.bits == b.bits) == (operation == Some("="))),
                            text: None,
                        }))
                    }
                    Some("scenery") if args.len() == 1 => {
                        self.expression(args[0], parameters, depth + 1)
                    }
                    Some("scenery_animation_start") if args.len() == 3 => {
                        let values = args
                            .iter()
                            .map(|&arg| self.expression(arg, parameters, depth + 1))
                            .collect::<Result<Vec<_>>>()?;
                        if let [Some(object), Some(graph), Some(clip)] = values.as_slice()
                            && object.bits as u16 != u16::MAX
                            && let Some(clip) = &clip.text
                        {
                            self.candidates
                                .entry(object.bits as u16)
                                .or_default()
                                .insert(Pose {
                                    graph: graph.bits,
                                    clip: clip.clone(),
                                });
                        }
                        Ok(None)
                    }
                    // Walk arguments for presentation calls, without inventing the result or
                    // execution order of an operation the analysis does not implement.
                    _ => {
                        let mut last = None;
                        for arg in args {
                            last = self.expression(arg, parameters, depth + 1)?;
                        }
                        Ok(if operation == Some("begin") {
                            last
                        } else {
                            None
                        })
                    }
                }
            }
            _ => Ok(None),
        }
    }

    fn conditional(
        &mut self,
        args: &[u32],
        parameters: &[Option<Constant>],
        depth: usize,
    ) -> Result<Option<Constant>> {
        match self.expression(args[0], parameters, depth + 1)? {
            Some(value) if value.bits != 0 => self.expression(args[1], parameters, depth + 1),
            Some(_) => match args.get(2) {
                Some(&arg) => self.expression(arg, parameters, depth + 1),
                None => Ok(None),
            },
            None => {
                for &arg in &args[1..] {
                    self.expression(arg, parameters, depth + 1)?;
                }
                Ok(None)
            }
        }
    }
}

pub(in crate::halo_reach::level) fn read(program: &Program) -> Result<Poses> {
    let mut scripts = BTreeSet::new();
    loop {
        let before = scripts.len();
        for (index, dependency) in program.analysis.scripts.iter().enumerate() {
            if dependency.operations.iter().any(|op| {
                vocabulary::get().functions.get(op).map(String::as_str)
                    == Some("scenery_animation_start")
            }) || dependency
                .calls
                .iter()
                .any(|script| scripts.contains(script))
            {
                scripts.insert(index);
            }
        }
        if scripts.len() == before {
            break;
        }
    }
    let mut reader = Reader {
        program,
        mutable: BTreeSet::new(),
        scripts,
        candidates: BTreeMap::new(),
        steps: 0,
    };
    for node in program
        .nodes
        .iter()
        .flatten()
        .filter(|node| node.kind() == 8)
    {
        if vocabulary::get()
            .functions
            .get(&node.opcode)
            .map(String::as_str)
            == Some("set")
            && let Some(&target) = reader.arguments(node)?.first()
        {
            let target = program.node(target)?;
            if target.kind() == 13 {
                reader.mutable.insert(usize::from(target.value as u16));
            }
        }
    }
    // Scanning each script root covers named static calls even in dormant encounter setup.
    // Unknown parameters do not become object identities. Conflicts across roots are retained.
    for &index in &reader.scripts.clone() {
        let script = &program.scripts[index];
        if let Some(root) = script.root {
            reader.expression(root, &vec![None; script.parameters.len()], 0)?;
        }
    }
    let mut result = Poses::default();
    for (object, candidates) in reader.candidates {
        if candidates.len() == 1 {
            result
                .objects
                .insert(object, candidates.into_iter().next().unwrap());
        } else {
            result.conflicting.insert(object);
        }
    }
    Ok(result)
}
