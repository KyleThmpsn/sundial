//! Follow physical metalness from the final deferred target back to the painted recipe.
use super::*;

fn output(code: &Code) -> Option<Value> {
    let target = code
        .outputs
        .iter()
        .find(|s| s.name == "SV_TARGET" && s.index == 2)?;
    let (at, instruction) = code.instructions.iter().enumerate().rev().find(|(_, i)| {
        !matches!(i.code, 13 | 18 | 21 | 31 | 62)
            && i.operands.first().is_some_and(|d| {
                d.kind == 2
                    && d.indices.len() == 1
                    && d.indices[0].base as usize == target.register
                    && d.mask & 1 != 0
            })
    })?;
    let depth = code.instructions[..at]
        .iter()
        .fold(0usize, |d, i| match i.code {
            31 => d + 1,
            21 => d.saturating_sub(1),
            _ => d,
        });
    (instruction.code == 54 && !instruction.saturate && depth == 0)
        .then(|| Value::scalar(&instruction.operands[1], 0, at))
}

fn matched(
    code: &Code,
    final_value: Value,
    base: Bank,
    wear: &Value,
    selector: &Value,
) -> Option<Constant> {
    let final_op = final_value.node(code)?;
    if !final_op.is(50, false) || !final_op.arg(1).same(code, selector) {
        return None;
    }
    let (painted, raw) = remap::difference(code, &final_op)?;
    let constant = raw.constant(code)?;
    let mix = painted.node(code)?;
    if !mix.is(50, false) || !mix.arg(1).same(code, wear) {
        return None;
    }
    let (pristine, worn) = remap::difference(code, &mix)?;
    (pristine.bank(code)?.matches(base, 2, [3; 3], true)
        && worn.bank(code)?.matches(base, 8, [3; 3], true))
    .then_some(constant)
}

pub(super) fn recover(
    code: &Code,
    base: Bank,
    wear: &Value,
    selector: &Value,
    constants: &[[f32; 4]],
) -> Result<Option<Constant>, ()> {
    let value = output(code).ok_or(())?;
    // Generated color-only programs deliberately have no physical-material output.
    if value.literal(0.0) {
        return Ok(None);
    }
    matched(code, value, base, wear, selector)
        .filter(|c| {
            c.valid(constants)
                && code
                    .buffers
                    .iter()
                    .any(|&(buffer, count)| buffer == 0 && c.row < count)
        })
        .map(Some)
        .ok_or(())
}
