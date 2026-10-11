//! Checked Renegades-to-Shadowkeep material-expression lowering.
//! Opcode identities follow Charm's TfxBytecode_EoF and TfxBytecode_BL.
use crate::d2_mot::reader::write_json;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub mod inputs;

#[derive(Debug)]
pub struct Instruction<'a> {
    pub op: u8,
    pub native: u8,
    pub args: &'a [u8],
    pub arity: usize,
}

pub fn parse(data: &[u8]) -> Result<Vec<Instruction<'_>>> {
    let mut at = 0;
    let mut result = vec![];
    while at < data.len() {
        let offset = at;
        let op = data[at];
        at += 1;
        let (native, size, arity) = match op {
            // IsZero is unary. The multiply and add aliases at 5 and 6 are binary.
            7 => (op, 0, 1),
            1..=15 => (op, 0, 2),
            0x13..=0x16 => (op - 3, 0, 3),
            0x18..=0x23 => (op - 3, 0, 1),
            0x28 => (0x21, 0, 1),
            0x29 => (0x22, 1, 1),
            0x2A => (0x23, 0, 1),
            // Matching source and shipped native dye programs use this log2
            // instruction. Only its opcode number changes.
            0x2C => (0x25, 0, 1),
            0x2E..=0x32 => (op - 7, 0, 1),
            0x35 => (0x2E, 0, 5),
            0x42 => (0x34, 1, 0),
            0x47 => (0x39, 1, 2),
            0x43..=0x49 => (op - 14, 1, 1),
            0x4A..=0x4F => (op - 14, 2, 0),
            0x51..=0x5B => (op - 15, 1, 0),
            0x5C => (0x4D, 4, 0),
            0x5D => (0x4E, 1, 0),
            0x60..=0x62 => (op - 15, 2, 0),
            _ => bail!("unsupported source TFX opcode {op:02X} at {offset}"),
        };
        let args = data
            .get(at..at + size)
            .context("truncated TFX instruction")?;
        at += size;
        result.push(Instruction {
            op,
            native,
            args,
            arity,
        });
    }
    Ok(result)
}

pub fn relocate(data: &[u8], base: usize, outputs: &BTreeMap<u8, u8>) -> Result<Vec<u8>> {
    let mut result = vec![];
    for i in parse(data)? {
        result.push(i.op);
        match i.op {
            0x42..=0x49 => result.push(
                u8::try_from(
                    base.checked_add(i.args[0] as usize)
                        .context("constant index overflow")?,
                )
                .context("relocated TFX constant exceeds byte index")?,
            ),
            0x52 => result.push(
                *outputs
                    .get(&i.args[0])
                    .context("relocated TFX output outside table")?,
            ),
            0x51 | 0x53 | 0x56..=0x5B | 0x60..=0x62 => {
                bail!("dye scope contains nonnumeric output or binding")
            }
            _ => result.extend_from_slice(i.args),
        }
    }
    Ok(result)
}

#[derive(Default)]
pub struct Bindings {
    pub objects: BTreeMap<String, u8>,
    pub globals: BTreeMap<u8, u8>,
    pub global_defaults: BTreeMap<u8, u8>,
    pub constant_count: usize,
    pub output_count: usize,
    pub sampler_count: usize,
    /// Packed TFX shader stage. Existing callers default to pixel stage 1.
    pub sampler_stage: Option<u8>,
    pub settled: BTreeMap<String, u8>,
    pub textures: BTreeMap<u8, u8>,
    pub texture_metadata: BTreeMap<[u8; 2], u8>,
    pub external_textures: BTreeMap<[u8; 3], [u8; 3]>,
    pub texture_slots: BTreeMap<u8, u8>,
}

#[derive(Clone, Default)]
struct Expression {
    code: Vec<u8>,
    missing: BTreeSet<String>,
    matrix: bool,
}

#[derive(Debug)]
pub struct Lowered {
    pub code: Vec<u8>,
    pub evidence: Vec<Value>,
    pub samplers: BTreeMap<u8, u8>,
    pub textures: BTreeMap<u8, u8>,
}

impl Lowered {
    pub(crate) fn require_runtime_inputs(&self) -> Result<()> {
        let missing = self
            .evidence
            .iter()
            .filter(|row| row["translated"] != true && row["required_by_shader"] != false)
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return Ok(());
        }
        // Global channels are renderer-wide. Unlike object inputs, choosing
        // another animation donor cannot supply one absent from the target.
        let global = missing.iter().any(|row| {
            row["unresolved"].as_array().is_some_and(|inputs| {
                inputs.iter().any(|input| {
                    input.as_str().is_some_and(|s| {
                        s.starts_with("global channel ") || s.starts_with("extern ")
                    })
                })
            })
        });
        let error = anyhow::anyhow!("source material has unresolved runtime inputs: {missing:?}");
        Err(if global {
            crate::d2_mot::source_limit(error)
        } else {
            error
        })
    }
}

fn pop(stack: &mut Vec<Expression>) -> Result<Expression> {
    stack.pop().context("TFX stack underflow")
}

pub fn lower(data: &[u8], b: &Bindings) -> Result<Lowered> {
    ensure!(
        b.constant_count <= 256 && b.output_count <= 256 && b.sampler_count <= 256,
        "TFX table exceeds byte indices"
    );
    let mut stack: Vec<Expression> = vec![];
    let mut temps = BTreeMap::new();
    let mut outputs = BTreeMap::new();
    let mut result = Lowered {
        code: vec![],
        evidence: vec![],
        samplers: BTreeMap::new(),
        textures: BTreeMap::new(),
    };
    for i in parse(data)? {
        let mut e = Expression::default();
        match i.op {
            0x42 => {
                ensure!(
                    (i.args[0] as usize) < b.constant_count,
                    "TFX constant outside table"
                );
                e.code.extend([i.native, i.args[0]]);
            }
            0x5C => {
                let key = format!("{:08X}", u32::from_be_bytes(i.args.try_into()?));
                if let Some(index) = b.objects.get(&key) {
                    e.code.extend([i.native, *index]);
                } else if let Some(index) = b.settled.get(&key) {
                    ensure!(
                        (*index as usize) < b.constant_count,
                        "settled TFX constant outside table"
                    );
                    e.code.extend([0x34, *index]);
                    e.missing.insert(format!("unbounded source-time {key}"));
                } else {
                    e.missing.insert(format!("object channel {key}"));
                }
            }
            0x5D => {
                if let Some(index) = b.globals.get(&i.args[0]) {
                    e.code.extend([i.native, *index]);
                } else if let Some(index) = b.global_defaults.get(&i.args[0]) {
                    ensure!(
                        (*index as usize) < b.constant_count,
                        "global default outside constants"
                    );
                    e.code.extend([0x34, *index]);
                } else {
                    e.missing.insert(format!("global channel {}", i.args[0]));
                }
            }
            0x60 => {
                let index = b
                    .textures
                    .get(&i.args[0])
                    .filter(|v| (**v as usize) < b.sampler_count)
                    .context("texture dimensions require a validated resource-table mapping")?;
                e.code.extend([i.native, *index, i.args[1]]);
            }
            0x61 | 0x62 => {
                let index = b
                    .texture_metadata
                    .get(&[i.op, i.args[0]])
                    .context("texture tiling requires verified source header constants")?;
                ensure!(
                    (*index as usize) < b.constant_count,
                    "texture metadata constant outside table"
                );
                // Tiling metadata was added to texture headers after Shadowkeep.
                // It is immutable asset data, so preserve its exact authored value.
                e.code.extend([0x34, *index, 0x22, i.args[1]]);
            }
            0x4A..=0x4F => {
                e.matrix = i.op == 0x4C;
                // Frame dither, time and exposure scale retain their addresses.
                // Native material programs read exposure scale at float index 7
                // with 3C 01 07, matching the source Frame +0x1C scalar. Frame
                // scalars 4 and 28 keep theirs too: material programs identical in
                // both games read them at the same instruction (140 and 93 reads in
                // 33 and 13 shipped programs).
                if let Some(native) = b.external_textures.get(&[i.op, i.args[0], i.args[1]]) {
                    e.code.extend(native);
                } else if (i.op == 0x4B && i.args == [1, 26])
                    || (i.op == 0x4A
                        && matches!(i.args, [1, 0] | [1, 1] | [1, 4] | [1, 7] | [1, 8] | [1, 28]))
                {
                    e.code.push(i.native);
                    e.code.extend_from_slice(i.args);
                } else {
                    e.missing.insert(format!("extern {}", hex::encode(i.args)));
                }
            }
            0x55 => {
                temps.insert(i.args[0], pop(&mut stack)?);
                continue;
            }
            0x54 => {
                e = temps
                    .get(&i.args[0])
                    .context("TFX temp read before write")?
                    .clone()
            }
            0x52 => {
                ensure!(
                    (i.args[0] as usize) < b.output_count,
                    "TFX output outside constant buffer"
                );
                e = pop(&mut stack)?;
                ensure!(!e.matrix, "matrix consumed by vector output");
                outputs.insert(i.args[0], e.clone());
                if e.missing.is_empty() {
                    result.code.extend(e.code);
                    result.code.extend([i.native, i.args[0]]);
                }
                result.evidence.push(json!({"output":i.args[0], "translated":e.missing.is_empty(), "unresolved":e.missing}));
                continue;
            }
            0x53 => {
                let first = usize::from(i.args[0]);
                ensure!(
                    first + 4 <= b.output_count,
                    "TFX matrix outside constant buffer"
                );
                e = pop(&mut stack)?;
                ensure!(
                    e.matrix && e.code.first() == Some(&0x3E) && e.missing.is_empty(),
                    "unverified matrix output expression"
                );
                result.code.extend(e.code);
                result.code.extend([i.native, i.args[0]]);
                for output in first..first + 4 {
                    // A vector read needs a separately verified row expression.
                    outputs.remove(&(output as u8));
                    result
                        .evidence
                        .push(json!({"output":output,"translated":true,"unresolved":[]}));
                }
                continue;
            }
            0x5B => {
                ensure!(
                    (i.args[0] as usize) < b.sampler_count,
                    "TFX sampler outside table"
                );
                e.code.extend([i.native, i.args[0]]);
            }
            0x58 => {
                e = pop(&mut stack)?;
                ensure!(
                    e.missing.is_empty()
                        && e.code.len() == 2
                        && e.code[0] == 0x4C
                        && i.args[0] >> 5 == b.sampler_stage.unwrap_or(1),
                    "unsupported TFX sampler expression"
                );
                result.samplers.insert(i.args[0] & 31, e.code[1]);
                result.code.extend(e.code);
                result.code.extend([i.native, i.args[0]]);
                continue;
            }
            0x56 | 0x57 => {
                e = pop(&mut stack)?;
                let slot = *b.texture_slots.get(&i.args[0]).with_context(|| {
                    format!(
                        "unmapped texture output slot {:02X}, expression {:02X?}",
                        i.args[0], e.code
                    )
                })?;
                ensure!(
                    e.missing.is_empty()
                        && b.external_textures.values().any(|v| e.code.as_slice() == v),
                    "unverified external texture expression"
                );
                ensure!(
                    i.op != 0x57 || e.code.first() == Some(&0x3F),
                    "buffer binding requires an external resource"
                );
                result.code.extend(e.code);
                result.code.extend([i.native, slot]);
                result.textures.insert(i.args[0], slot);
                continue;
            }
            0x51 => {
                ensure!(
                    (i.args[0] as usize) < b.output_count,
                    "TFX output read outside constant buffer"
                );
                // Snapshot the expression that produced this version of the output.
                // Emitting a native output read later would observe intervening writes,
                // including writes between capture into a source temp and its use.
                e = outputs
                    .get(&i.args[0])
                    .context("TFX output read before a translated write")?
                    .clone();
            }
            _ if i.arity > 0 => {
                if (0x43..=0x49).contains(&i.op) {
                    let width = [2, 2, 5, 10, 10, 6, 11][(i.op - 0x43) as usize];
                    ensure!(
                        i.args[0] as usize + width <= b.constant_count,
                        "TFX curve constants outside table"
                    );
                }
                ensure!(stack.len() >= i.arity, "TFX expression stack underflow");
                for value in stack.drain(stack.len() - i.arity..) {
                    ensure!(!value.matrix, "matrix consumed by vector arithmetic");
                    e.code.extend(value.code);
                    e.missing.extend(value.missing);
                }
                if i.op == 0x2A {
                    e.missing
                        .retain(|s| !s.starts_with("unbounded source-time "));
                }
                e.code.push(i.native);
                e.code.extend_from_slice(i.args);
                ensure!(e.code.len() <= 65536, "expanded TFX expression too large");
            }
            _ => bail!("unsupported TFX side effect {:02X}", i.op),
        }
        stack.push(e);
    }
    // Unconsumed expressions have no observable side effects. The native program
    // emits only expressions reaching output writes, so dead source stack values
    // need neither an artificial output nor a conversion failure.
    Ok(result)
}

pub use crate::tiger::channel::object_channel_map;

fn read(path: &Path) -> Result<Value> {
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(
        bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes),
    )?)
}

pub fn native_channels(root: &Path, owner: Option<u32>) -> Result<BTreeMap<String, u8>> {
    let report = read(&root.join("template-report.json"))?;
    let mut candidates = BTreeSet::new();
    for m in report["models"].as_array().context("native models")? {
        let tag = u32::from_str_radix(m["owner"].as_str().context("native owner")?, 16)?;
        if owner.is_none_or(|owner| tag == owner) {
            candidates.insert(tag);
        }
    }
    let mut maps = vec![];
    for tag in candidates {
        let map = object_channel_map(&fs::read(root.join(format!("raw/{tag:08X}.bin")))?)?;
        if !maps.contains(&map) {
            maps.push(map);
        }
    }
    ensure!(
        maps.len() == 1,
        "missing or ambiguous native model object inputs"
    );
    Ok(maps.remove(0))
}

fn string_map(value: &Value) -> Result<BTreeMap<String, u8>> {
    if value.is_null() {
        return Ok(BTreeMap::new());
    }
    value
        .as_object()
        .context("TFX mapping object")?
        .iter()
        .map(|(k, v)| {
            Ok((
                k.clone(),
                u8::try_from(v.as_u64().context("TFX mapping index")?)?,
            ))
        })
        .collect()
}
fn index_map(value: &Value) -> Result<BTreeMap<u8, u8>> {
    string_map(value)?
        .into_iter()
        .map(|(k, v)| Ok((k.parse()?, v)))
        .collect()
}

/// JSON bridge for the remaining experimental tools. Conversion logic lives here.
pub fn request(v: &Value) -> Result<Value> {
    let mode = v["mode"].as_str().context("TFX mode")?;
    if mode == "channels" {
        let owner = v["owner"].as_u64().map(u32::try_from).transpose()?;
        return Ok(json!(native_channels(
            Path::new(v["root"].as_str().context("native root")?),
            owner
        )?));
    }
    let data = hex::decode(v["data"].as_str().context("TFX hex data")?)?;
    match mode {
        "object-map" => Ok(json!(object_channel_map(&data)?)),
        "parse" => Ok(json!(
            parse(&data)?
                .iter()
                .map(|i| json!([i.op, i.native, hex::encode(i.args), i.arity]))
                .collect::<Vec<_>>()
        )),
        "relocate" => Ok(
            json!({"code":hex::encode(relocate(&data, usize::try_from(v["constant_base"].as_u64().context("constant base")?)?, &index_map(&v["output_map"])?)?)}),
        ),
        "lower" => {
            let result = lower(
                &data,
                &Bindings {
                    objects: string_map(&v["object_channels"])?,
                    globals: index_map(&v["global_channels"])?,
                    constant_count: usize::try_from(
                        v["constant_count"].as_u64().context("constant count")?,
                    )?,
                    output_count: usize::try_from(
                        v["output_count"].as_u64().context("output count")?,
                    )?,
                    sampler_count: usize::try_from(
                        v["sampler_count"].as_u64().context("sampler count")?,
                    )?,
                    settled: string_map(&v["settled_constants"])?,
                    textures: index_map(&v["texture_resources"])?,
                    ..Default::default()
                },
            )?;
            Ok(
                json!({"code":hex::encode(result.code), "evidence":result.evidence, "samplers":result.samplers}),
            )
        }
        _ => bail!("unknown TFX mode {mode}"),
    }
}

pub fn run(input: &Path, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "TFX output already exists");
    let result = request(&read(input)?)?;
    fs::create_dir_all(out)?;
    write_json(&out.join("result.json"), &result)?;
    Ok(result)
}

#[cfg(test)]
mod tests;
