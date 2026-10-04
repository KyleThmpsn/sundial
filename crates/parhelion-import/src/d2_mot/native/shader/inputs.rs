//! Executable input layouts. Producer meaning is a separate conversion contract.
use super::{identity::Stage, program::Program};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Read {
    pub expression: String,
    pub lanes: String,
    /// None preserves dynamic addressing rather than inventing a static bound.
    pub index: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inputs {
    pub stage: Stage,
    pub bytecode_sha256: String,
    pub constant_buffers: BTreeMap<u32, u32>,
    pub constant_indexing: BTreeMap<u32, String>,
    pub constant_reads: BTreeMap<u32, Vec<Read>>,
    /// Complete Microsoft declarations, with the register replaced by @.
    pub resources: BTreeMap<u32, String>,
    pub samplers: BTreeMap<u32, String>,
    pub unordered_access: BTreeMap<u32, String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub other_declarations: Vec<String>,
    pub thread_group: Option<[u32; 3]>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Bindings {
    pub constant_buffers: BTreeMap<u32, u32>,
    pub resources: BTreeMap<u32, String>,
    pub samplers: BTreeMap<u32, String>,
    pub unordered_access: BTreeMap<u32, String>,
}

pub struct Inspection {
    pub inputs: Inputs,
    pub assembly: String,
}

fn identifier(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn registers(line: &str, kind: u8) -> Result<Vec<(usize, usize, u32)>> {
    let bytes = line.as_bytes();
    let mut found = Vec::new();
    for start in 0..bytes.len() {
        if bytes[start] != kind || start > 0 && identifier(bytes[start - 1]) {
            continue;
        }
        let mut end = start + 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end == start + 1 || end < bytes.len() && identifier(bytes[end]) {
            continue;
        }
        found.push((start, end, line[start + 1..end].parse()?));
    }
    Ok(found)
}

fn declaration(
    line: &str,
    kind: u8,
    maximum: u32,
    values: &mut BTreeMap<u32, String>,
) -> Result<()> {
    let registers = registers(line, kind)?;
    ensure!(
        registers.len() == 1,
        "ambiguous resource declaration: {line}"
    );
    let (start, end, slot) = registers[0];
    ensure!(
        slot < maximum,
        "resource register exceeds stage limits: {line}"
    );
    let shape = format!("{}@{}", &line[..start], &line[end..]);
    ensure!(
        values.insert(slot, shape).is_none(),
        "duplicate resource declaration: {line}"
    );
    Ok(())
}

fn reads(line: &str, inputs: &Inputs, output: &mut BTreeMap<u32, BTreeSet<Read>>) -> Result<()> {
    let bytes = line.as_bytes();
    for (start, _) in line.match_indices("cb") {
        if start > 0 && identifier(bytes[start - 1]) {
            continue;
        }
        let mut end = start + 2;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end == start + 2 || bytes.get(end) != Some(&b'[') {
            continue;
        }
        let slot: u32 = line[start + 2..end].parse()?;
        let close = end
            + 1
            + line[end + 1..]
                .find(']')
                .context("unclosed constant read")?;
        let expression = &line[end + 1..close];
        ensure!(!expression.is_empty(), "empty constant read");
        let index = if expression.bytes().all(|byte| byte.is_ascii_digit()) {
            Some(expression.parse::<u32>()?)
        } else {
            None
        };
        let capacity = inputs
            .constant_buffers
            .get(&slot)
            .context("constant read has no declaration")?;
        if let Some(index) = index {
            ensure!(index < *capacity, "constant read exceeds declared vectors");
        } else {
            ensure!(
                inputs.constant_indexing.get(&slot).map(String::as_str) == Some("dynamicIndexed"),
                "dynamic read has an immediate-only declaration"
            );
        }
        let mut lane_end = close + 1;
        let lanes = if bytes.get(lane_end) == Some(&b'.') {
            let lane_start = lane_end + 1;
            lane_end = lane_start;
            while lane_end < bytes.len() && matches!(bytes[lane_end], b'x' | b'y' | b'z' | b'w') {
                lane_end += 1;
            }
            ensure!(
                lane_end > lane_start && lane_end - lane_start <= 4,
                "constant read swizzle differs"
            );
            &line[lane_start..lane_end]
        } else {
            ""
        };
        output.entry(slot).or_default().insert(Read {
            expression: expression.to_owned(),
            lanes: lanes.to_owned(),
            index,
        });
    }
    Ok(())
}

impl Inspection {
    pub fn read(stage: Stage, code: &[u8]) -> Result<Self> {
        // Validate the complete envelope, executable stage and signatures before
        // calling the platform disassembler or interpreting its declarations.
        Program::new(stage, code.to_vec())?;
        let assembly = super::disassemble(code)?;
        let mut lines = assembly
            .lines()
            .map(|line| line.split_once("//").map_or(line, |(head, _)| head).trim())
            .filter(|line| !line.is_empty());
        let expected = match stage {
            Stage::Pixel => "ps_5_0",
            Stage::Vertex => "vs_5_0",
            Stage::Compute => "cs_5_0",
        };
        ensure!(lines.next() == Some(expected), "disassembled stage differs");
        let mut inputs = Inputs {
            stage,
            bytecode_sha256: format!("{:x}", Sha256::digest(code)),
            constant_buffers: BTreeMap::new(),
            constant_indexing: BTreeMap::new(),
            constant_reads: BTreeMap::new(),
            resources: BTreeMap::new(),
            samplers: BTreeMap::new(),
            unordered_access: BTreeMap::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            other_declarations: Vec::new(),
            thread_group: None,
        };
        let mut accesses = BTreeMap::new();
        for line in lines {
            if let Some(tail) = line.strip_prefix("dcl_constantbuffer CB") {
                let (slot, tail) = tail.split_once('[').context("constant declaration slot")?;
                let (count, indexing) = tail
                    .split_once("], ")
                    .context("constant declaration count")?;
                let slot: u32 = slot.parse()?;
                let count: u32 = count.parse()?;
                ensure!(
                    slot < 14 && (1..=4096).contains(&count),
                    "constant declaration exceeds limits"
                );
                ensure!(
                    matches!(indexing, "immediateIndexed" | "dynamicIndexed"),
                    "constant addressing mode differs"
                );
                ensure!(
                    inputs.constant_buffers.insert(slot, count).is_none(),
                    "duplicate constant declaration"
                );
                inputs.constant_indexing.insert(slot, indexing.to_owned());
            } else if line.starts_with("dcl_resource_") {
                declaration(line, b't', 128, &mut inputs.resources)?;
            } else if line.starts_with("dcl_uav_") {
                declaration(line, b'u', 8, &mut inputs.unordered_access)?;
            } else if line.starts_with("dcl_sampler ") {
                declaration(line, b's', 16, &mut inputs.samplers)?;
            } else if let Some(group) = line.strip_prefix("dcl_thread_group ") {
                let group = group
                    .split(", ")
                    .map(str::parse::<u32>)
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let group: [u32; 3] = group
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("compute group dimensions differ"))?;
                ensure!(
                    (1..=1024).contains(&group[0])
                        && (1..=1024).contains(&group[1])
                        && (1..=64).contains(&group[2])
                        && group.iter().product::<u32>() <= 1024,
                    "compute thread group exceeds stage limits"
                );
                ensure!(
                    inputs.thread_group.replace(group).is_none(),
                    "duplicate compute group"
                );
            } else if line.starts_with("dcl_input") {
                inputs.inputs.push(line.to_owned());
            } else if line.starts_with("dcl_output") {
                inputs.outputs.push(line.to_owned());
            } else if line.starts_with("dcl_") {
                ensure!(
                    [
                        "dcl_globalFlags ",
                        "dcl_temps ",
                        "dcl_indexableTemp ",
                        "dcl_indexRange "
                    ]
                    .iter()
                    .any(|prefix| line.starts_with(prefix)),
                    "shader declaration needs an input contract: {line}"
                );
                inputs.other_declarations.push(line.to_owned());
            } else {
                reads(line, &inputs, &mut accesses)?;
            }
        }
        ensure!(
            inputs.thread_group.is_some() == (stage == Stage::Compute),
            "compute group and stage differ"
        );
        inputs.constant_reads = accesses
            .into_iter()
            .map(|(slot, values)| (slot, values.into_iter().collect()))
            .collect();
        Ok(Self { inputs, assembly })
    }
}

impl Inputs {
    /// Check only the capacity and declared resource layout of supplied inputs.
    /// Their meaning, per-frame updates and vertex/system-value producers must
    /// be validated by the material and renderer conversion that supplies them.
    pub fn verify_layout(&self, bindings: &Bindings) -> Result<()> {
        for (&slot, &count) in &self.constant_buffers {
            let supplied = bindings
                .constant_buffers
                .get(&slot)
                .with_context(|| format!("missing constant buffer b{slot}"))?;
            ensure!(
                *supplied >= count && *supplied <= 4096,
                "constant buffer b{slot} capacity differs"
            );
        }
        for (kind, needed, supplied) in [
            ('t', &self.resources, &bindings.resources),
            ('s', &self.samplers, &bindings.samplers),
            ('u', &self.unordered_access, &bindings.unordered_access),
        ] {
            for (slot, shape) in needed {
                ensure!(
                    supplied.get(slot) == Some(shape),
                    "missing or incompatible {kind}{slot} resource layout"
                );
            }
        }
        Ok(())
    }
}
