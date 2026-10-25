//! Normal-blue grain is recovered independently from the live deferred smoothness path.
use super::*;
use std::collections::BTreeSet;

pub(super) struct Grain([Constant; 3]);
impl Grain {
    pub(super) fn frame(&self, frame: &Frame) -> Option<[f32; 3]> {
        let value = self.0.map(|v| v.value(frame));
        value.iter().all(|v| v.is_finite()).then_some(value)
    }
    pub(super) fn glsl(&self) -> String {
        let values = self.0.map(Constant::glsl);
        format!(
            "(uMapSlot/2==0?{}:(uMapSlot/2==1?{}:{}))",
            values[0], values[1], values[2]
        )
    }
    pub(super) fn valid_glsl(&self) -> String {
        let values = self.0.map(Constant::glsl);
        let value = format!("vec3({})", values.join(","));
        format!("!(any(isnan({value}))||any(isinf({value})))")
    }
}

fn weighted(code: &Code, value: Value, bank: Bank, params: usize) -> Option<Value> {
    let layer = value.node(code)?;
    if !layer.is(50, false)
        || !layer.arg(1).bank(code)?.matches(bank, params, [1; 3], true)
        || !layer.arg(3).literal(1.0)
    {
        return None;
    }
    let delta = layer.arg(2).node(code)?;
    if !delta.is(0, false) || !delta.arg(2).literal(-1.0) {
        return None;
    }
    Some(delta.arg(1))
}

fn limit(code: &Code, value: Value, bank: Bank, wear: &Value) -> Option<Value> {
    let mix = value.node(code)?;
    if !mix.is(50, false) || !mix.arg(1).same(code, wear) {
        return None;
    }
    let (pristine, worn) = remap::difference(code, &mix)?;
    let pristine = weighted(code, pristine, bank, 2)?;
    let worn = weighted(code, worn, bank, 8)?;
    pristine.same(code, &worn).then_some(pristine)
}

fn offsets(
    code: &Code,
    value: &Value,
    normal: &normal::Normal,
    constants: &[[f32; 4]],
) -> Option<[Constant; 3]> {
    let definitions = detail::definitions(code, value)?;
    let mut found = [None; 3];
    for at in &definitions[0] {
        let at = (*at)?;
        let i = &code.instructions[at];
        if i.code != 0 || !i.saturate {
            return None;
        }
        let lane = value.operand.lanes[0];
        let a = Value::scalar(&i.operands[1], lane, at);
        let b = Value::scalar(&i.operands[2], lane, at);
        let (sample, bias) = if let Some(c) = b.constant(code) {
            (a, c)
        } else {
            (b, a.constant(code)?)
        };
        if sample.operand.modifier != 0 {
            return None;
        }
        let sample_at = gain::writer(code, &sample.operand, 0, at)?;
        let s = &code.instructions[sample_at];
        if !matches!(s.code, 69 | 72 | 73) || s.saturate {
            return None;
        }
        let resource = &s.operands[2];
        let slot = resource.indices[0].base as usize;
        if resource.kind != 7
            || ![5, 7, 9].contains(&slot)
            || resource.lanes[sample.operand.lanes[0]] != 2
        {
            return None;
        }
        let channel = (slot - 5) / 2;
        if sample_at != normal.samples[channel]
            || bias.row != normal.detail[channel][0].row
            || bias.lane != 2
            || !bias.valid(constants)
        {
            return None;
        }
        if found[channel].is_some_and(|old| old != bias) {
            return None;
        }
        found[channel] = Some(bias);
    }
    Some([found[0]?, found[1]?, found[2]?])
}

fn minimum(
    code: &Code,
    node: &Node<'_>,
    bank: Bank,
    wear: &Value,
    detail: &Value,
    smooth: Constant,
) -> Option<Value> {
    if !node.is(51, false) {
        return None;
    }
    for (surface, grain) in [(node.arg(1), node.arg(2)), (node.arg(2), node.arg(1))] {
        if remap::blended(code, surface, bank, wear, detail) == Some(smooth) {
            return limit(code, grain, bank, wear);
        }
    }
    None
}

pub(super) fn recover(
    code: &Code,
    constants: &[[f32; 4]],
    bank: Bank,
    wear: &Value,
    detail: &Value,
    smooth: Constant,
    normal: &normal::Normal,
) -> Option<Grain> {
    let mut pending = vec![(normal::smooth(code)?, 0)];
    let mut visited = BTreeSet::new();
    let mut found = None;
    while let Some((mut value, depth)) = pending.pop() {
        if depth > 48 || visited.len() > 256 {
            return None;
        }
        // Sign does not change whether an instruction contributes to the final output.
        if value.operand.modifier == 1 {
            value.operand.modifier = 0;
        }
        let Some(node) = value.node(code) else {
            continue;
        };
        if !visited.insert((node.at, value.operand.lanes[0])) {
            continue;
        }
        if let Some(raw) = minimum(code, &node, bank, wear, detail, smooth) {
            let current = offsets(code, &raw, normal, constants)?;
            if found.is_some_and(|old| old != current) {
                return None;
            }
            found = Some(current);
            continue;
        }
        // Only scalar arithmetic and selection edges can carry this smoothness term.
        if !matches!(
            node.instruction.code,
            0 | 1 | 29 | 49 | 50 | 51 | 52 | 54 | 55 | 56
        ) {
            continue;
        }
        for arg in 1..node.instruction.operands.len() {
            pending.push((node.arg(arg), depth + 1));
        }
    }
    found.map(Grain)
}
