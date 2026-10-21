//! Bounded DXIL-to-SM5 translation of scalar pixel programs.
//!
//! DXC supplies a validated LLVM listing. Resource bindings, SSA values and
//! acyclic control flow are retained. Renderer scope and varying translation
//! belong to the caller. Unsupported instructions fail closed.
mod emit;
mod instruction;
mod parse;

pub(crate) use instruction::call;

use anyhow::{Context, Result, bail, ensure};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct Field {
    pub semantic: String,
    pub index: u32,
    pub width: usize,
    pub integer: bool,
}

#[derive(Clone)]
struct Instruction {
    id: Option<u32>,
    code: String,
}

pub struct Program {
    pub inputs: Vec<Field>,
    pub outputs: Vec<Field>,
    blocks: BTreeMap<u32, Vec<Instruction>>,
    order: Vec<u32>,
    edges: BTreeMap<u32, Vec<u32>>,
    post: BTreeMap<u32, BTreeSet<u32>>,
}

/// Retain selected render targets by semantic index. A discarded target's
/// computations are removed by the SM5 compiler, including velocity resources.
#[derive(Default)]
pub struct Options {
    pub targets: BTreeSet<u32>,
    /// Renderer adapters can replace a complete, statically indexed float4
    /// load. They must independently verify the producer and resulting reads.
    pub constants: BTreeMap<(u32, u32), String>,
    /// Checked enum encodings at an output lane. Only literal leaves and
    /// selections between those leaves may be remapped. Keys are float bits.
    pub output_values: BTreeMap<(u32, usize), BTreeMap<u32, u32>>,
}

/// Use the configured DXC or the compiler available on PATH. No package download or
/// process-wide environment mutation occurs during import.
pub fn disassemble(bytecode: &[u8]) -> Result<String> {
    ensure!(bytecode.starts_with(b"DXBC"), "Invalid DXIL container");
    let directory = tempfile::tempdir()?;
    let input = directory.path().join("source.dxil");
    std::fs::write(&input, bytecode)?;
    let tool = crate::tools::Tool::Dxc;
    let mut command = std::process::Command::new(crate::tools::executable(tool));
    command.arg("-dumpbin").arg(&input);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().with_context(|| tool.missing())?;
    ensure!(
        output.status.success(),
        "DXC rejected the source shader: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    ensure!(
        output.stdout.len() <= 16 * 1024 * 1024,
        "DXIL listing exceeds translation budget"
    );
    let text = String::from_utf8(output.stdout).context("DXIL listing encoding")?;
    ensure!(
        text.is_ascii() && text.contains("!dx.shaderModel"),
        "DXC did not return an ASCII DXIL program"
    );
    Ok(text)
}

pub fn signature(text: &str, input: bool) -> Result<Vec<Field>> {
    parse::signature(
        text,
        if input {
            "; Input signature:"
        } else {
            "; Output signature:"
        },
    )
}

pub(crate) fn arguments(text: &str) -> Result<Vec<&str>> {
    let mut result = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (at, c) in text.char_indices() {
        match c {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => {
                depth = depth.checked_sub(1).context("Unbalanced DXIL operand")?
            }
            ',' if depth == 0 => {
                result.push(text[start..at].trim());
                start = at + 1;
            }
            _ => {}
        }
    }
    ensure!(depth == 0, "Unbalanced DXIL argument list");
    result.push(text[start..].trim());
    Ok(result)
}

pub(crate) fn number(text: &str) -> Result<u32> {
    text.split_whitespace()
        .last()
        .context("Missing DXIL number")?
        .parse()
        .with_context(|| format!("Expected a literal DXIL index in {text:?}"))
}

pub(crate) fn id(text: &str) -> Result<u32> {
    text.split_whitespace()
        .last()
        .context("Missing DXIL value")?
        .strip_prefix('%')
        .context("Expected an SSA value")?
        .parse()
        .context("Non-numeric SSA value")
}

fn value(text: &str) -> Result<String> {
    let token = text
        .split_whitespace()
        .last()
        .context("Missing DXIL operand")?;
    if token.starts_with('%') {
        return Ok(format!("v{}", id(token)?));
    }
    if let Some(bits) = token.strip_prefix("0x") {
        ensure!(bits.len() == 16, "Unsupported DXIL floating literal");
        let value = f64::from_bits(u64::from_str_radix(bits, 16)?) as f32;
        return Ok(format!("asfloat(0x{:08X}u)", value.to_bits()));
    }
    if matches!(token, "true" | "false") {
        return Ok(token.into());
    }
    ensure!(
        token != "undef" && token.parse::<f64>().is_ok(),
        "Unsupported or undefined DXIL operand {token}"
    );
    Ok(token.into())
}

fn scalar_type(text: &str) -> Result<&'static str> {
    match text {
        "float" => Ok("float"),
        "i32" => Ok("uint"),
        "i1" => Ok("bool"),
        _ => bail!("Unsupported DXIL scalar type {text}"),
    }
}
