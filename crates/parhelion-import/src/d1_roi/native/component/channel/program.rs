//! Checked channel expressions with an explicit bank-local input list.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::value::{
    Instruction, Program as NativeProgram,
};

pub(super) struct Program {
    code: Vec<u8>,
    constants: Vec<u8>,
    pub(super) inputs: Vec<u32>,
    pub(super) flags: u16,
}

pub(in crate::d1_roi::native::component) fn validate(
    code: &[u8],
    constants: &[[u32; 4]],
    inputs: usize,
) -> Result<()> {
    let mut instructions = Vec::new();
    let mut at = 0;
    while at < code.len() {
        let opcode = code[at];
        at += 1;
        let operand = match opcode {
            0x01 | 0x02 | 0x03 | 0x04 | 0x0F | 0x23 => None,
            0x22 | 0x34 | 0x35 | 0x37 | 0x38 | 0x3C | 0x3E => {
                let operand = *code.get(at).context("Truncated channel instruction")?;
                at += 1;
                Some(operand)
            }
            _ => anyhow::bail!("Source channel opcode {opcode:02X} requires translation"),
        };
        ensure!(
            opcode != 0x3E || at == code.len(),
            "Source channel output must end its procedure"
        );
        instructions.push(Instruction { opcode, operand });
    }
    // Channel input/output operands are one byte. Material-expression extern operands have a
    // different width. Reuse the native stack and constant-span checks only after decoding this
    // dialect. Declaration +74 is preserved separately and is not the scalar fast-path field.
    NativeProgram {
        instructions,
        constants: constants.to_vec(),
        fast_path: 0,
    }
    .validate_inputs(inputs)
    .map_err(anyhow::Error::msg)
    .context("Source channel procedure")
}

pub(super) fn read(source: &Payload, row: usize, channels: usize) -> Result<Program> {
    let code = source
        .array(row + 8, 1, Some(0x80800009))?
        .iter()
        .map(|at| source.u8(*at))
        .collect::<Result<Vec<_>>>()?;
    let values = source.array(row + 24, 16, Some(0x80800090))?;
    let constants = values
        .iter()
        .map(|at| source.bytes::<16>(*at))
        .collect::<Result<Vec<_>>>()?
        .concat();
    let lanes = constants
        .chunks_exact(16)
        .map(|row| {
            std::array::from_fn(|i| {
                u32::from_le_bytes(row[i * 4..i * 4 + 4].try_into().expect("four-byte lane"))
            })
        })
        .collect::<Vec<[u32; 4]>>();
    ensure!(
        lanes
            .iter()
            .flatten()
            .all(|bits| f32::from_bits(*bits).is_finite()),
        "Nonfinite source channel constant"
    );
    let inputs = source.array(row + 56, 4, Some(0x80800007))?;
    let flags = source.u16(row + 74)?;
    ensure!(
        source.u64(row + 40)? == inputs.len() as u64
            && source.u64(row + 48)? == 1
            && source.u16(row + 72)? == u16::MAX
            && flags <= 1
            && source.u64(row + 80)? == 0
            && source.bytes::<2>(row + 104)? == [255, 254],
        "Source channel metadata requires translation"
    );
    let inputs = inputs
        .iter()
        .map(|at| source.u32(*at))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        inputs.iter().all(|&input| (input as usize) < channels),
        "Source procedure input is not a channel of its bank"
    );
    validate(&code, &lanes, inputs.len())?;
    Ok(Program {
        code,
        constants,
        inputs,
        flags,
    })
}

impl Program {
    pub(super) fn write(&self, p: &mut Payload, row: usize) -> Result<()> {
        array(&mut p.0, row + 8, 0x80800009, &self.code, 1)?;
        array(&mut p.0, row + 24, 0x80800090, &self.constants, 16)?;
        let inputs = self
            .inputs
            .iter()
            .flat_map(|input| input.to_le_bytes())
            .collect::<Vec<_>>();
        array(&mut p.0, row + 56, 0x80800007, &inputs, 4)?;
        Ok(())
    }
}

/// Pure expressions cannot recursively require their own output. Callback channels are stateful
/// producers, so they form boundaries here rather than being treated as pure dependencies.
pub(super) fn validate_graph(programs: &BTreeMap<usize, Program>) -> Result<()> {
    fn visit(
        ordinal: usize,
        programs: &BTreeMap<usize, Program>,
        active: &mut BTreeSet<usize>,
        complete: &mut BTreeSet<usize>,
    ) -> Result<()> {
        if complete.contains(&ordinal) {
            return Ok(());
        }
        let Some(program) = programs.get(&ordinal) else {
            return Ok(());
        };
        ensure!(
            active.insert(ordinal),
            "Source channel procedure dependency cycle"
        );
        for &input in &program.inputs {
            visit(input as usize, programs, active, complete)?;
        }
        active.remove(&ordinal);
        complete.insert(ordinal);
        Ok(())
    }
    let mut complete = BTreeSet::new();
    for &ordinal in programs.keys() {
        visit(ordinal, programs, &mut BTreeSet::new(), &mut complete)?;
    }
    Ok(())
}
