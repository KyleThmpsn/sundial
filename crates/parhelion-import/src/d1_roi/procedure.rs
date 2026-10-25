//! The one-byte-operand value dialect shared by numeric channels and sequence events.
use anyhow::{Context, Result, ensure};
use sundial::package_authoring::sandbox_perk::action::native::value::{Instruction, Program};

pub(super) fn validate(code: &[u8], constants: &[[u32; 4]], inputs: usize) -> Result<()> {
    let mut instructions = Vec::new();
    let mut at = 0;
    while at < code.len() {
        let opcode = code[at];
        at += 1;
        let operand = match opcode {
            0x01 | 0x02 | 0x03 | 0x04 | 0x0F | 0x23 => None,
            0x22 | 0x34 | 0x35 | 0x37 | 0x38 | 0x3C | 0x3E => {
                let operand = *code.get(at).context("Truncated numeric instruction")?;
                at += 1;
                Some(operand)
            }
            _ => anyhow::bail!("Source numeric opcode {opcode:02X} requires translation"),
        };
        ensure!(
            opcode != 0x3E || at == code.len(),
            "Source numeric output must end its procedure"
        );
        instructions.push(Instruction { opcode, operand });
    }
    // Material-expression extern operands have a different width. Reuse native stack and
    // constant-span checks only after decoding this dialect. Channel declaration flags do
    // not select the scalar fast path.
    Program {
        instructions,
        constants: constants.to_vec(),
        fast_path: 0,
    }
    .validate_inputs(inputs)
    .map_err(anyhow::Error::msg)
    .context("Source numeric procedure")
}
