//! Compiled MCC U13 HaloScript intake. Source graph identities remain cache-local.
//! Decoding and dependency analysis do not install a Destiny mission controller.
mod analysis;
pub(super) mod audio;
pub mod binding;
pub mod execution;
mod vocabulary;
pub use audio::AudioOptions;

use super::super::cache::{Cache, Tag};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

const SCRIPTS: usize = 0x430;
const GLOBALS: usize = 0x43C;
const EXPRESSIONS: usize = 0x504;

#[derive(Debug, Serialize)]
pub struct Program {
    pub schema: u32,
    pub source_build: String,
    pub source_metadata_sha256: String,
    pub scenario: Tag,
    pub execution: &'static str,
    pub scripts: Vec<Script>,
    pub globals: Vec<Global>,
    /// Slot order is part of each salted datum. Unallocated slots remain null.
    pub nodes: Vec<Option<Node>>,
    pub tag_references: Vec<Option<Tag>>,
    pub bindings: binding::Bindings,
    pub analysis: analysis::Analysis,
    pub audio: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct Parameter {
    pub name: String,
    pub value_type: u16,
}

#[derive(Debug, Serialize)]
pub struct Script {
    pub name_id: u32,
    pub name: String,
    pub kind: u16,
    pub return_type: u16,
    pub root: Option<u32>,
    pub parameters: Vec<Parameter>,
}

#[derive(Debug, Serialize)]
pub struct Global {
    pub name: String,
    pub value_type: u16,
    pub root: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct Node {
    pub datum: u32,
    /// Function ordinal, script ordinal or literal type according to flags.
    pub opcode: u16,
    pub value_type: u16,
    pub flags: u16,
    pub next: Option<u32>,
    pub source_offset: u32,
    /// Raw bits preserve short sign extension, float payloads and source identities.
    pub value: u32,
    pub line: i16,
    pub padding: u16,
    /// Retained symbolic bytes are not necessarily UTF-8, including dialogue literals.
    pub text_bytes: Option<Vec<u8>>,
    pub text: Option<String>,
}

impl Node {
    fn kind(&self) -> u16 {
        self.flags & 0x1F
    }

    fn child(&self) -> Option<u32> {
        matches!(self.kind(), 8 | 10).then_some(self.value)
    }

    fn literal_type(&self) -> u16 {
        if self.opcode == u16::MAX {
            self.value_type
        } else {
            self.opcode
        }
    }
}

fn optional(datum: u32) -> Option<u32> {
    (datum != u32::MAX).then_some(datum)
}

/// Reads and validates the entire compiled scenario program, including dormant scripts.
pub fn read(path: &Path) -> Result<Program> {
    from_cache(&Cache::open(path)?)
        .with_context(|| format!("Reading mission scripts from {}", path.display()))
}

/// Exports an inspectable source program into a new directory.
pub fn export(path: &Path, output: &Path) -> Result<()> {
    let program = read(path)?;
    let bytes = serde_json::to_vec_pretty(&program)?;
    fs::create_dir(output)
        .with_context(|| format!("Creating fresh mission export {}", output.display()))?;
    fs::write(output.join("mission.json"), bytes)?;
    Ok(())
}

/// Preserves mission cues and decodes their recordings without scheduling target playback.
pub fn export_with_audio(path: &Path, output: &Path, options: &AudioOptions) -> Result<()> {
    let source = path.canonicalize()?;
    let mut pages = super::super::resource::Pages::default();
    pages.record(&source)?;
    let program = read(&source)?;
    fs::create_dir(output)
        .with_context(|| format!("Creating fresh mission export {}", output.display()))?;
    let report = audio::export(&program, options, &mut pages, &output.join("audio"))?;
    pages.verify_inputs()?;
    fs::write(
        output.join("mission.json"),
        serde_json::to_vec_pretty(&program)?,
    )?;
    fs::write(
        output.join("source.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "inputs":pages.inputs,"audio":report,
        }))?,
    )?;
    Ok(())
}

pub(super) fn from_cache(cache: &Cache) -> Result<Program> {
    let scenario = cache.only("scnr")?.clone();
    let at = scenario.address()?;
    let scripts = read_scripts(cache, at)?;
    let globals = read_globals(cache, at)?;
    let nodes = read_nodes(cache, at)?;
    let tag_references = cache
        .block(at + 0x448, 0x10)?
        .into_iter()
        .map(|row| cache.reference(row, None))
        .collect::<Result<_>>()?;
    let mut program = Program {
        schema: 1,
        source_build: cache.build.clone(),
        source_metadata_sha256: hex::encode(Sha256::digest(cache.metadata().1)),
        scenario,
        execution: "unsupported",
        scripts,
        globals,
        nodes,
        tag_references,
        bindings: binding::Bindings::default(),
        analysis: analysis::Analysis::default(),
        audio: serde_json::Value::Null,
    };
    validate(&program)?;
    program.bindings = binding::read(cache, &program)?;
    program.analysis = analysis::read(&program)?;
    program.audio = audio::read(cache, &program)?;
    Ok(program)
}

fn read_scripts(cache: &Cache, at: usize) -> Result<Vec<Script>> {
    let rows = cache.block(at + SCRIPTS, 0x18)?;
    ensure!(rows.len() <= 65536, "Script table exceeds its index domain");
    rows.into_iter()
        .map(|row| {
            let name_id = cache.u32(row)?;
            let parameters = cache
                .block(row + 0xC, 0x24)?
                .into_iter()
                .map(|parameter| {
                    Ok(Parameter {
                        name: cache.string(parameter, 32)?,
                        value_type: cache.u16(parameter + 0x20)?,
                    })
                })
                .collect::<Result<_>>()?;
            Ok(Script {
                name_id,
                name: cache.string_id(name_id)?,
                kind: cache.u16(row + 4)?,
                return_type: cache.u16(row + 6)?,
                root: optional(cache.u32(row + 8)?),
                parameters,
            })
        })
        .collect()
}

fn read_globals(cache: &Cache, at: usize) -> Result<Vec<Global>> {
    let rows = cache.block(at + GLOBALS, 0x28)?;
    ensure!(
        rows.len() <= 32768,
        "Local globals overlap the external domain"
    );
    rows.into_iter()
        .map(|row| {
            Ok(Global {
                name: cache.string(row, 32)?,
                value_type: cache.u16(row + 0x20)?,
                root: optional(cache.u32(row + 0x24)?),
            })
        })
        .collect()
}

fn read_nodes(cache: &Cache, at: usize) -> Result<Vec<Option<Node>>> {
    let rows = cache.block(at + EXPRESSIONS, 0x18)?;
    ensure!(
        rows.len() <= 65536,
        "Expression table exceeds its datum domain"
    );
    let size = usize::try_from(cache.i32(at + 0x41C)?)?;
    let strings = if size == 0 {
        &[][..]
    } else {
        let pointer = cache.u32(at + 0x428)?;
        ensure!(pointer != 0, "Script strings have a null pointer");
        cache.meta(cache.expand(pointer)?, size)?
    };
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            let salt = cache.u16(row)?;
            if salt == 0 {
                return Ok(None);
            }
            let mut node = Node {
                datum: (u32::from(salt) << 16) | index as u32,
                opcode: cache.u16(row + 2)?,
                value_type: cache.u16(row + 4)?,
                flags: cache.u16(row + 6)?,
                next: optional(cache.u32(row + 8)?),
                source_offset: cache.u32(row + 0xC)?,
                value: cache.u32(row + 0x10)?,
                line: cache.i16(row + 0x14)?,
                padding: cache.u16(row + 0x16)?,
                text_bytes: None,
                text: None,
            };
            // Group source offsets and numeric literal offsets may still refer to stripped
            // source text. Only symbolic leaves address the retained string table.
            if vocabulary::has_text(&node)
                && let Some(tail) = strings.get(node.source_offset as usize..)
                && !tail.is_empty()
            {
                let end = tail
                    .iter()
                    .position(|&b| b == 0)
                    .context("Unterminated mission symbol")?;
                node.text = std::str::from_utf8(&tail[..end]).ok().map(str::to_owned);
                node.text_bytes = Some(tail[..end].to_vec());
            }
            Ok(Some(node))
        })
        .collect()
}

impl Program {
    fn node(&self, datum: u32) -> Result<&Node> {
        let node = self
            .nodes
            .get((datum & 0xFFFF) as usize)
            .and_then(Option::as_ref)
            .with_context(|| format!("Missing mission expression {datum:08X}"))?;
        ensure!(
            node.datum == datum,
            "Mission expression salt differs for {datum:08X}"
        );
        Ok(node)
    }
}

fn validate(program: &Program) -> Result<()> {
    for root in program
        .scripts
        .iter()
        .filter_map(|s| s.root)
        .chain(program.globals.iter().filter_map(|g| g.root))
    {
        program.node(root)?;
    }
    for node in program.nodes.iter().flatten() {
        for datum in node.next.into_iter().chain(node.child()) {
            program.node(datum)?;
        }
        if node.kind() == 8 {
            let head = program.node(node.value)?;
            ensure!(
                head.kind() == 9 && head.value_type == 2 && head.opcode == node.opcode,
                "Mission function group has an invalid head"
            );
        }
        if node.kind() == 10 {
            ensure!(
                usize::from(node.opcode) < program.scripts.len(),
                "Mission script call is outside its table"
            );
        }
        if node.kind() == 13 && node.value as u16 & 0x8000 == 0 {
            ensure!(
                usize::from(node.value as u16) < program.globals.len(),
                "Mission global reference is outside its table"
            );
        }
        if node.kind() == 9 && node.value_type != 2 && matches!(node.literal_type(), 10 | 21) {
            let target = node.value as u16;
            ensure!(
                target == u16::MAX || usize::from(target) < program.scripts.len(),
                "Mission script value is outside its table"
            );
        }
    }
    validate_graph(program)
}

fn validate_graph(program: &Program) -> Result<()> {
    // Expression structure must be a DAG. Script recursion uses script ordinals and is not
    // an expression edge, so it remains legal. The explicit stack avoids source-driven recursion.
    let mut colors = vec![0u8; program.nodes.len()];
    for root in program.nodes.iter().flatten() {
        let mut pending = vec![(root.datum, false)];
        while let Some((datum, leaving)) = pending.pop() {
            let index = (datum & 0xFFFF) as usize;
            if leaving {
                colors[index] = 2;
                continue;
            }
            ensure!(
                colors[index] != 1,
                "Mission expression graph contains a cycle"
            );
            if colors[index] == 2 {
                continue;
            }
            colors[index] = 1;
            pending.push((datum, true));
            let node = program.node(datum)?;
            pending.extend(
                node.next
                    .into_iter()
                    .chain(node.child())
                    .map(|d| (d, false)),
            );
        }
    }
    Ok(())
}
