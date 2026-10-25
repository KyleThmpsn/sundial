//! Explicit encounter openings for native squad bring-up. Only the selected script's prefix
//! before its first wait runs. This does not translate its later mission progression.
use super::{Node, Program, vocabulary};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Clone, Serialize)]
pub struct Placement {
    pub source: String,
    pub count: Option<usize>,
    pub expression: u32,
}

#[derive(Serialize)]
pub struct Opening {
    pub placements: Vec<Placement>,
    pub omitted_operations: BTreeSet<String>,
    pub stopped_at: Vec<u32>,
}

struct Reader<'a> {
    source: &'a Program,
    opening: Opening,
    work: usize,
}

impl Reader<'_> {
    fn controls_placement(&self, datum: u32, depth: usize) -> Result<bool> {
        ensure!(depth < 64, "Recursive encounter branch");
        let node = self.source.node(datum)?;
        if node.kind() != 8 {
            return Ok(false);
        }
        let name = vocabulary::get()
            .functions
            .get(&node.opcode)
            .map(String::as_str);
        if matches!(
            name,
            Some("ai_place" | "sleep" | "sleep_until" | "sleep_forever")
        ) {
            return Ok(true);
        }
        for argument in self.args(node)? {
            if self.controls_placement(argument, depth + 1)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn args(&self, node: &Node) -> Result<Vec<u32>> {
        let mut next = self.source.node(node.value)?.next;
        let mut args = Vec::new();
        while let Some(datum) = next {
            ensure!(args.len() < 256, "Encounter call has too many arguments");
            args.push(datum);
            next = self.source.node(datum)?.next;
        }
        Ok(args)
    }

    fn constant(&self, datum: u32, depth: usize) -> Result<Option<f64>> {
        ensure!(depth < 64, "Recursive encounter constant");
        let node = self.source.node(datum)?;
        match node.kind() {
            9 => Ok(match node.literal_type() {
                5 => Some(f64::from(u8::from(node.value as u8 != 0))),
                6 => Some(f64::from(f32::from_bits(node.value))),
                7 => Some(f64::from(node.value as i16)),
                8 => Some(f64::from(node.value as i32)),
                53 => Some(f64::from(node.value as u16)),
                _ => None,
            }),
            13 => match self
                .source
                .globals
                .get(node.value as u16 as usize)
                .and_then(|g| g.root)
            {
                Some(root) => self.constant(root, depth + 1),
                None => Ok(None),
            },
            8 => {
                let name = vocabulary::get()
                    .functions
                    .get(&node.opcode)
                    .map(String::as_str);
                if name == Some("game_difficulty_get") {
                    return Ok(Some(1.));
                }
                if name == Some("editor_mode") || name == Some("game_is_cooperative") {
                    return Ok(Some(0.));
                }
                let args = self.args(node)?;
                if name == Some("begin") && args.len() == 1 {
                    return self.constant(args[0], depth + 1);
                }
                let values = args
                    .into_iter()
                    .map(|a| self.constant(a, depth + 1))
                    .collect::<Result<Option<Vec<_>>>>()?;
                let Some(values) = values else {
                    return Ok(None);
                };
                let boolean = |value| Some(f64::from(u8::from(value)));
                Ok(match (name, values.as_slice()) {
                    (Some("not"), [a]) => boolean(*a == 0.),
                    (Some("="), [a, b]) => boolean(a == b),
                    (Some("!="), [a, b]) => boolean(a != b),
                    (Some(">="), [a, b]) => boolean(a >= b),
                    (Some("<="), [a, b]) => boolean(a <= b),
                    (Some(">"), [a, b]) => boolean(a > b),
                    (Some("<"), [a, b]) => boolean(a < b),
                    (Some("and"), values) => boolean(values.iter().all(|v| *v != 0.)),
                    (Some("or"), values) => boolean(values.iter().any(|v| *v != 0.)),
                    _ => None,
                })
            }
            10 => {
                let script = self
                    .source
                    .scripts
                    .get(node.opcode as usize)
                    .context("Encounter script index is outside its table")?;
                if script.parameters.is_empty() && script.kind == 3 {
                    match script.root {
                        Some(root) => self.constant(root, depth + 1),
                        None => Ok(None),
                    }
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    fn walk(&mut self, datum: u32, depth: usize) -> Result<bool> {
        self.work += 1;
        ensure!(
            depth < 64 && self.work < 100_000,
            "Encounter opening exceeds its work limit"
        );
        let node = self.source.node(datum)?;
        if node.kind() == 10 {
            let script = self
                .source
                .scripts
                .get(node.opcode as usize)
                .context("Encounter script index is outside its table")?;
            self.opening
                .omitted_operations
                .insert(format!("script:{}", script.name));
            return Ok(true);
        }
        if node.kind() != 8 {
            return Ok(true);
        }
        let name = vocabulary::get()
            .functions
            .get(&node.opcode)
            .map(String::as_str)
            .unwrap_or("unknown");
        let args = self.args(node)?;
        match name {
            "begin" => {
                for arg in args {
                    if !self.walk(arg, depth + 1)? {
                        return Ok(false);
                    }
                }
            }
            "if" => {
                ensure!(
                    (2..=3).contains(&args.len()),
                    "Invalid encounter conditional"
                );
                let mut relevant = false;
                for &branch in &args[1..] {
                    relevant |= self.controls_placement(branch, 0)?;
                }
                if !relevant {
                    self.opening
                        .omitted_operations
                        .insert("conditional_nonplacement".into());
                    return Ok(true);
                }
                match self.constant(args[0], 0)? {
                    Some(value) => {
                        if let Some(&branch) = args.get(if value != 0. { 1 } else { 2 }) {
                            return self.walk(branch, depth + 1);
                        }
                    }
                    None => bail!(
                        "Encounter opening has an unresolved condition at {:08X}",
                        node.datum
                    ),
                }
            }
            "ai_place" => {
                ensure!((1..=2).contains(&args.len()), "Invalid encounter placement");
                let target = self.source.node(args[0])?;
                ensure!(
                    target.kind() == 9,
                    "Encounter placement needs a literal source squad"
                );
                let source = target
                    .text
                    .clone()
                    .context("Encounter squad has no source name")?;
                let count = args
                    .get(1)
                    .map(|&datum| -> Result<usize> {
                        let count = self
                            .constant(datum, 0)?
                            .context("Encounter count is not constant")?;
                        ensure!(
                            count.is_finite()
                                && count.fract() == 0.
                                && (0. ..=64.).contains(&count),
                            "Invalid encounter member count"
                        );
                        Ok(count as usize)
                    })
                    .transpose()?;
                self.opening.placements.push(Placement {
                    source,
                    count,
                    expression: node.datum,
                });
            }
            "sleep" | "sleep_until" | "sleep_forever" => {
                self.opening.stopped_at.push(node.datum);
                return Ok(false);
            }
            _ => {
                self.opening.omitted_operations.insert(name.to_owned());
            }
        }
        Ok(true)
    }
}

pub fn read(program: &Program, scripts: &[String]) -> Result<Opening> {
    let mut reader = Reader {
        source: program,
        work: 0,
        opening: Opening {
            placements: Vec::new(),
            omitted_operations: BTreeSet::new(),
            stopped_at: Vec::new(),
        },
    };
    for name in scripts {
        let script = program
            .scripts
            .iter()
            .find(|script| script.name == *name)
            .with_context(|| format!("Missing encounter script {name}"))?;
        ensure!(
            script.parameters.is_empty(),
            "Encounter opening takes parameters"
        );
        reader.walk(script.root.context("Encounter opening has no root")?, 0)?;
    }
    ensure!(
        !reader.opening.placements.is_empty(),
        "Selected encounter openings place no squads"
    );
    Ok(reader.opening)
}
