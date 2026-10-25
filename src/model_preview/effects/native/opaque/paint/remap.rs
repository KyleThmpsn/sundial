//! Recover two saturated affine stages without confusing them with endpoint clamps.
use super::*;

pub(super) struct Remap {
    pub bank: Bank,
    pub raw: Value,
}

pub(super) fn recover(code: &Code, value: &Value) -> Option<Remap> {
    let outer = value.node(code)?;
    if !outer.is(50, true) {
        return None;
    }
    let bank = outer.arg(1).bank(code)?;
    let inner = outer.arg(2).node(code)?;
    if !inner.is(50, true) {
        return None;
    }
    // The caller checks the field offset. Here all four remap lanes must use the same row.
    if !bank.matches(bank, 0, [3; 3], false)
        || !outer.arg(3).bank(code)?.matches(bank, 0, [2; 3], false)
        || !inner.arg(1).bank(code)?.matches(bank, 0, [1; 3], false)
        || !inner.arg(3).bank(code)?.matches(bank, 0, [0; 3], false)
    {
        return None;
    }
    Some(Remap {
        bank,
        raw: inner.arg(2),
    })
}

pub(super) fn difference(code: &Code, mix: &Node<'_>) -> Option<(Value, Value)> {
    let delta = mix.arg(2).node(code)?;
    if !delta.is(0, false) {
        return None;
    }
    let (pristine, worn) = if let Some(worn) = delta.arg(1).positive() {
        (delta.arg(2), worn)
    } else {
        (delta.arg(1), delta.arg(2).positive()?)
    };
    worn.same(code, &mix.arg(3)).then_some((pristine, worn))
}

fn surface(code: &Code, value: Value, base: Bank, worn: bool, detail: &Value) -> Option<Constant> {
    let mix = value.node(code)?;
    let params = if worn { 8 } else { 2 };
    if !mix.is(50, false) || !mix.arg(1).bank(code)?.matches(base, params, [2; 3], true) {
        return None;
    }
    let (detailed, raw) = difference(code, &mix)?;
    let raw = recover(code, &raw)?;
    let detailed = recover(code, &detailed)?;
    let field = if worn { 7 } else { 4 };
    if !raw.bank.matches(base, field, [3; 3], false)
        || !detailed.bank.matches(base, field, [3; 3], false)
    {
        return None;
    }
    let smooth = raw.raw.constant(code)?;
    let overlay = detailed.raw.node(code)?;
    if !overlay.is(50, true) {
        return None;
    }
    let factors = super::factors(code, overlay.arg(2), overlay.arg(3))?;
    let mut alpha = detail.clone();
    alpha.operand.lanes = [3; 4];
    (factors.constant(code) == Some(smooth) && overlay.arg(1).same(code, &alpha)).then_some(smooth)
}

pub(super) fn blended(
    code: &Code,
    value: Value,
    base: Bank,
    wear: &Value,
    detail: &Value,
) -> Option<Constant> {
    let mix = value.node(code)?;
    if !mix.is(50, false) || !mix.arg(1).same(code, wear) {
        return None;
    }
    let (pristine, worn) = difference(code, &mix)?;
    let pristine = surface(code, pristine, base, false, detail)?;
    let worn = surface(code, worn, base, true, detail)?;
    (pristine == worn).then_some(pristine)
}

pub(super) fn smooth(
    code: &Code,
    base: Bank,
    constants: &[[f32; 4]],
    wear: &Value,
    detail: &Value,
) -> Option<Constant> {
    let mut found = None;
    for (at, instruction) in code.instructions.iter().enumerate() {
        if instruction.code != 50 || instruction.saturate {
            continue;
        }
        let destination = &instruction.operands[0];
        for lane in 0..4 {
            if destination.mask & (1 << lane) == 0 {
                continue;
            }
            let value = Value::scalar(destination, lane, at + 1);
            if let Some(raw) =
                blended(code, value, base, wear, detail).filter(|c| c.valid(constants))
            {
                if found.is_some_and(|previous| previous != raw) {
                    return None;
                }
                found = Some(raw);
            }
        }
    }
    found
}

pub(super) fn wear(code: &Code, value: &Value, base: Bank, mask_at: usize) -> Option<()> {
    let remap = recover(code, value)?;
    if !remap.bank.matches(base, 6, [3; 3], false) {
        return None;
    }
    let multiply = remap.raw.node(code)?;
    if !multiply.is(56, true) || !multiply.arg(2).literal(255.0 / 207.0) {
        return None;
    }
    let subtract = multiply.arg(1).node(code)?;
    if subtract.instruction.code != 0 || !subtract.arg(2).literal(-48.0 / 255.0) {
        return None;
    }
    // Saturating the initial subtraction is equivalent here, because alpha is UNORM
    // and the following multiplication is saturated too.
    let alpha = subtract.arg(1);
    (gain::sample(code, &alpha.operand, &[0], alpha.before, 2, &[3])? == mask_at).then_some(())
}
