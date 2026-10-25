//! The directly bound native 27-vector normal ABI, separate from merged imported banks.
use super::*;

pub(in crate::model_preview) struct Normal {
    decode: usize,
    selector: Constant,
    surface: u8,
    sampling: Option<[usize; 5]>,
}

impl Normal {
    pub(in crate::model_preview) fn sampling(&self) -> Option<[usize; 5]> {
        self.sampling
    }
    pub(in crate::model_preview) fn frame(&self, frame: &Frame) -> Option<[f32; 3]> {
        let decode = frame.get(self.decode)?;
        let selector = self.selector.value(frame);
        let result = [decode[0], decode[1], decode[2]];
        (selector == f32::from(self.surface % 2) && result.iter().all(|v| v.is_finite()))
            .then_some(result)
    }
}

fn color_sample(code: &Code, bank: usize, detail: bool) -> Option<()> {
    let slot = if detail { 3 + (7 - bank) * 2 } else { 0 };
    let mut uses = code.instructions.iter().enumerate().filter(|(_, i)| {
        matches!(i.code, 45 | 61 | 69 | 70 | 72 | 73 | 74 | 108)
            && i.operands
                .iter()
                .any(|v| v.kind == 7 && v.indices[0].base as usize == slot)
    });
    let (at, sample) = uses.next()?;
    if uses.next().is_some()
        || sample.code != 69
        || sample.saturate
        || sample.offset != [0; 3]
        || sample.operands[3].indices[0].base != if detail { 1 } else { 3 }
        || sample.operands[3].indices[0].relative.is_some()
        || !code
            .resources
            .iter()
            .any(|r| r.slot == slot && r.dimension == 3 && !r.integer)
    {
        return None;
    }
    for axis in 0..2 {
        let value = Value::scalar(&sample.operands[1], axis, at);
        if detail {
            let coordinate = value.node(code)?;
            if !coordinate.is(50, false)
                || !input(code, &coordinate.arg(1), 3, axis + 2)
                || !cb(&coordinate.arg(2), bank, 0, axis)
                || !cb(&coordinate.arg(3), bank, 0, axis + 2)
            {
                return None;
            }
        } else if !input(code, &value, 3, axis) {
            return None;
        }
    }
    Some(())
}

fn sampling(code: &Code, bank: usize) -> Option<[usize; 5]> {
    if code
        .instructions
        .iter()
        .any(|i| matches!(i.code, 18 | 21 | 31 | 48 | 76))
    {
        return None;
    }
    color_sample(code, bank, false)?;
    color_sample(code, bank, true)?;
    Some([2, 4, 3, 0, 1])
}

fn cb(value: &Value, slot: usize, row: usize, lane: usize) -> bool {
    let v = &value.operand;
    v.kind == 8
        && v.modifier == 0
        && v.indices.len() == 2
        && v.indices.iter().all(|i| i.relative.is_none())
        && v.indices[0].base as usize == slot
        && v.indices[1].base as usize == row
        && v.lanes[..3] == [lane; 3]
}

fn bank_value(code: &Code, value: Value, slot: usize, row: usize, lane: usize, sat: bool) -> bool {
    if sat {
        value
            .node(code)
            .is_some_and(|n| n.is(54, true) && cb(&n.arg(1), slot, row, lane))
    } else {
        cb(&value, slot, row, lane)
    }
}

fn selected(
    code: &Code,
    value: Value,
    row: usize,
    lane: usize,
    sat: bool,
) -> Option<(usize, Constant)> {
    let mix = value.node(code)?;
    if !mix.is(50, false) {
        return None;
    }
    let selector = mix.arg(1).constant(code)?;
    if selector.lane != 0 {
        return None;
    }
    let (secondary, primary) = remap::difference(code, &mix)?;
    (6..=7).find_map(|slot| {
        (code.buffers.contains(&(slot, 27))
            && bank_value(code, primary.clone(), slot, row, lane, sat)
            && bank_value(code, secondary.clone(), slot, row + 4, lane, sat))
        .then_some((slot, selector))
    })
}

fn selection(
    code: &Code,
    value: Value,
    bank: usize,
    selector: Constant,
    row: usize,
    lane: usize,
    sat: bool,
) -> bool {
    selected(code, value, row, lane, sat) == Some((bank, selector))
}

fn input(code: &Code, value: &Value, semantic: u32, lane: usize) -> bool {
    value.operand.kind == 1
        && value.operand.modifier == 0
        && value.operand.lanes[..3] == [lane; 3]
        && code.inputs.iter().any(|s| {
            s.name == "TEXCOORD"
                && s.index == semantic
                && s.register == value.operand.indices[0].base as usize
        })
}

fn sample(code: &Code, value: &Value, slot: usize, lane: usize, sampler: usize) -> Option<usize> {
    let at = gain::sample(
        code,
        &value.operand,
        &[0],
        value.before,
        slot as u32,
        &[lane],
    )?;
    let instruction = &code.instructions[at];
    (instruction.code == 69
        && !instruction.saturate
        && instruction.operands[3].indices[0].base as usize == sampler
        && code
            .resources
            .iter()
            .any(|r| r.slot == slot && r.dimension == 3 && !r.integer))
    .then_some(at)
}

fn wear(code: &Code, value: &Value, bank: usize, selector: Constant, mask: usize) -> Option<()> {
    let outer = value.node(code)?;
    if !outer.is(50, true)
        || !selection(code, outer.arg(1), bank, selector, 18, 3, false)
        || !selection(code, outer.arg(3), bank, selector, 18, 2, false)
    {
        return None;
    }
    let inner = outer.arg(2).node(code)?;
    if !inner.is(50, true)
        || !selection(code, inner.arg(1), bank, selector, 18, 1, false)
        || !selection(code, inner.arg(3), bank, selector, 18, 0, false)
    {
        return None;
    }
    let multiply = inner.arg(2).node(code)?;
    if !multiply.is(56, true) || !multiply.arg(2).literal(255.0 / 207.0) {
        return None;
    }
    let add = multiply.arg(1).node(code)?;
    (add.is(0, false)
        && add.arg(2).literal(-48.0 / 255.0)
        && sample(code, &add.arg(1), 2, 3, 5) == Some(mask))
    .then_some(())
}

fn detail(code: &Code, value: Value, lane: usize, bank: usize) -> Option<usize> {
    let decode = value.node(code)?;
    if !decode.is(50, false) || !cb(&decode.arg(2), bank, 2, 0) || !cb(&decode.arg(3), bank, 2, 1) {
        return None;
    }
    let at = sample(code, &decode.arg(1), 4 + (7 - bank) * 2, lane, 2)?;
    let sampled = &code.instructions[at];
    for axis in 0..2 {
        let coordinate = Value::scalar(&sampled.operands[1], axis, at).node(code)?;
        if !coordinate.is(50, false)
            || !input(code, &coordinate.arg(1), 3, axis + 2)
            || !cb(&coordinate.arg(2), bank, 1, axis)
            || !cb(&coordinate.arg(3), bank, 1, axis + 2)
        {
            return None;
        }
    }
    Some(at)
}

struct Direction {
    row: usize,
    bank: usize,
    selector: Constant,
    wear: Value,
    base: usize,
    detail: usize,
}

fn direction(
    code: &Code,
    vector: &Value,
    lane: usize,
    constants: &[[f32; 4]],
) -> Option<Direction> {
    let op = Value::scalar(&vector.operand, lane, vector.before).node(code)?;
    if !op.is(50, false) {
        return None;
    }
    let mask = gain::selector(code, &op.arg(1).operand, op.at)?;
    let mix = op.arg(2).node(code).filter(|n| n.is(50, false))?;
    let delta = mix.arg(2).node(code).filter(|n| n.is(50, false))?;
    let worn = mix.arg(3).node(code).filter(|n| n.is(56, false))?;
    if !delta.arg(3).positive()?.same(code, &mix.arg(3)) || !delta.arg(1).same(code, &worn.arg(2)) {
        return None;
    }
    let (bank, selector) = selected(code, delta.arg(2), 10, 1, false)?;
    if !selector.valid(constants) || !selection(code, worn.arg(1), bank, selector, 20, 1, true) {
        return None;
    }
    wear(code, &mix.arg(1), bank, selector, mask)?;
    let detail = detail(code, delta.arg(1), lane, bank)?;
    let decode = op.arg(3).node(code).filter(|n| n.is(50, false))?;
    let scale = decode.arg(2).constant(code)?;
    let bias = decode.arg(3).constant(code)?;
    if scale.row != bias.row || scale.lane != 0 || bias.lane != 1 || !scale.valid(constants) {
        return None;
    }
    let base = sample(code, &decode.arg(1), 1, lane, 4)?;
    for at in [base, mask] {
        let sampled = &code.instructions[at];
        if !(0..2).all(|axis| {
            input(
                code,
                &Value::scalar(&sampled.operands[1], axis, at),
                3,
                axis,
            )
        }) {
            return None;
        }
    }
    Some(Direction {
        row: scale.row,
        bank,
        selector,
        wear: mix.arg(1),
        base,
        detail,
    })
}

fn blue(code: &Code, value: Value, direction: &Direction, base: bool) -> bool {
    let Some(add) = value.node(code).filter(|n| n.is(0, true)) else {
        return false;
    };
    let offset = if base {
        add.arg(2).constant(code)
            == Some(Constant {
                row: direction.row,
                lane: 2,
            })
    } else {
        cb(&add.arg(2), direction.bank, 2, 2)
    };
    offset
        && sample(
            code,
            &add.arg(1),
            if base {
                1
            } else {
                4 + (7 - direction.bank) * 2
            },
            2,
            if base { 4 } else { 2 },
        ) == Some(if base {
            direction.base
        } else {
            direction.detail
        })
}

fn blue_layer(code: &Code, value: Value, direction: &Direction, worn: bool) -> Option<Value> {
    let op = value.node(code)?;
    if !op.is(50, false)
        || !op.arg(3).literal(1.0)
        || !selection(
            code,
            op.arg(1),
            direction.bank,
            direction.selector,
            if worn { 20 } else { 10 },
            1,
            worn,
        )
    {
        return None;
    }
    let subtract = op.arg(2).node(code)?;
    (subtract.is(0, false) && subtract.arg(2).literal(-1.0)).then(|| subtract.arg(1))
}

fn minimum(code: &Code, node: &Node<'_>, direction: &Direction) -> bool {
    if !node.is(51, false) {
        return false;
    }
    [(node.arg(1), node.arg(2)), (node.arg(2), node.arg(1))]
        .into_iter()
        .any(|(base, detail)| {
            blue(code, base, direction, true) && detail_limit(code, detail, direction).is_some()
        })
}

fn detail_limit(code: &Code, value: Value, direction: &Direction) -> Option<()> {
    let mix = value.node(code)?;
    if !mix.is(50, false) || !mix.arg(1).same(code, &direction.wear) {
        return None;
    }
    let (pristine, worn) = remap::difference(code, &mix)?;
    let a = blue_layer(code, pristine, direction, false)?;
    let b = blue_layer(code, worn, direction, true)?;
    (a.same(code, &b) && blue(code, a, direction, false)).then_some(())
}

fn live_minimum(code: &Code, direction: &Direction) -> bool {
    let Some(root) = normal::smooth(code) else {
        return false;
    };
    let mut pending = vec![(root, 0)];
    let mut visited = BTreeSet::new();
    while let Some((mut value, depth)) = pending.pop() {
        if depth > 48 || visited.len() > 256 {
            return false;
        }
        if value.operand.modifier == 1 {
            value.operand.modifier = 0;
        }
        let Some(node) = value.node(code) else {
            continue;
        };
        if !visited.insert((node.at, value.operand.lanes[0])) {
            continue;
        }
        if minimum(code, &node, direction) {
            return true;
        }
        if matches!(
            node.instruction.code,
            0 | 1 | 29 | 49 | 50 | 51 | 52 | 54 | 55 | 56
        ) {
            for arg in 1..node.instruction.operands.len() {
                pending.push((node.arg(arg), depth + 1));
            }
        }
    }
    false
}

pub(in crate::model_preview::effects::native::opaque) fn recover(
    code: &Code,
    constants: &[[f32; 4]],
    surface: u8,
) -> Option<Normal> {
    let vector = normal::vector(code)?;
    let a = direction(code, &vector, 0, constants)?;
    let b = direction(code, &vector, 1, constants)?;
    (a.row == b.row
        && a.bank == b.bank
        && a.selector == b.selector
        && a.base == b.base
        && a.detail == b.detail
        && a.wear.same(code, &b.wear)
        && live_minimum(code, &a))
    .then_some(Normal {
        decode: a.row,
        selector: a.selector,
        surface,
        sampling: sampling(code, a.bank),
    })
}
