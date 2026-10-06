//! Recover signed normal decode only from the final deferred normal consumer.
use super::*;

pub(super) struct Normal {
    base: [Constant; 2],
    pub(super) detail: [[Constant; 2]; 3],
    pub(super) samples: [usize; 3],
}

impl Normal {
    pub(super) fn valid_glsl(&self) -> String {
        let values: Vec<_> = [self.base, self.detail[0], self.detail[1], self.detail[2]]
            .into_iter()
            .flatten()
            .map(Constant::glsl)
            .collect();
        let finite = |v: &[String]| {
            format!(
                "!(any(isnan(vec4({})))||any(isinf(vec4({}))))",
                v.join(","),
                v.join(",")
            )
        };
        format!("({}&&{})", finite(&values[..4]), finite(&values[4..]))
    }
    pub(super) fn frame(&self, frame: &Frame) -> Option<[[f32; 2]; 4]> {
        let result = [self.base, self.detail[0], self.detail[1], self.detail[2]]
            .map(|row| row.map(|v| v.value(frame)));
        result
            .iter()
            .flatten()
            .all(|v| v.is_finite())
            .then_some(result)
    }
    pub(super) fn glsl(&self) -> String {
        let base = self.base.map(Constant::glsl);
        let rows = self.detail.each_ref().map(|v| v.map(Constant::glsl));
        format!(
            "vec4({},{},uMapSlot/2==0?{}:(uMapSlot/2==1?{}:{}),uMapSlot/2==0?{}:(uMapSlot/2==1?{}:{}))",
            base[0],
            base[1],
            rows[0][0],
            rows[1][0],
            rows[2][0],
            rows[0][1],
            rows[1][1],
            rows[2][1]
        )
    }
}

fn scalar(value: &Value, lane: usize) -> Value {
    Value::scalar(&value.operand, lane, value.before)
}

fn unit(code: &Code, value: Value) -> Option<Value> {
    let product = value.node(code)?;
    if !product.is(56, false) {
        return None;
    }
    for (scale, vector) in [
        (product.arg(1), product.arg(2)),
        (product.arg(2), product.arg(1)),
    ] {
        let Some(inverse) = scale.node(code).filter(|n| n.is(68, false)) else {
            continue;
        };
        let Some(length) = inverse.arg(1).node(code).filter(|n| n.is(16, false)) else {
            continue;
        };
        let a = Value::new(&length.instruction.operands[1], length.at);
        let b = Value::new(&length.instruction.operands[2], length.at);
        if a.same(code, &b) && vector.same(code, &a) {
            return Some(vector);
        }
    }
    None
}

fn input(code: &Code, value: &Value, index: u32, lane: usize) -> bool {
    value.operand.kind == 1
        && value.operand.modifier == 0
        && value.operand.lanes[..3] == [lane; 3]
        && code.inputs.iter().any(|s| {
            s.name == "TEXCOORD"
                && s.index == index
                && s.register == value.operand.indices[0].base as usize
        })
}

fn tangent(code: &Code, world: Value) -> Option<Value> {
    let mut tangent = None;
    for axis in 0..3 {
        let final_op = scalar(&world, axis).node(code)?;
        if !final_op.is(50, false) {
            return None;
        }
        let z = factor(code, &final_op, 0, axis)?;
        let add = final_op.arg(3).node(code)?;
        if !add.is(50, false) {
            return None;
        }
        let x = factor(code, &add, 1, axis)?;
        let multiply = add.arg(3).node(code)?;
        if !multiply.is(56, false) {
            return None;
        }
        let y = factor(code, &multiply, 2, axis)?;
        let vector = tangent_scalar(code, &x, 0)?;
        if !tangent_scalar(code, &y, 1)?.same(code, &vector)
            || !tangent_scalar(code, &z, 2)?.same(code, &vector)
            || tangent
                .as_ref()
                .is_some_and(|v: &Value| !v.same(code, &vector))
        {
            return None;
        }
        tangent = Some(vector);
    }
    tangent
}

fn factor(code: &Code, node: &Node<'_>, input_index: u32, axis: usize) -> Option<Value> {
    if input(code, &node.arg(1), input_index, axis) {
        Some(node.arg(2))
    } else if input(code, &node.arg(2), input_index, axis) {
        Some(node.arg(1))
    } else {
        None
    }
}

fn tangent_scalar(code: &Code, value: &Value, lane: usize) -> Option<Value> {
    let product = value.node(code)?;
    if !product.is(56, false) {
        return None;
    }
    for (scale, component) in [
        (product.arg(1), product.arg(2)),
        (product.arg(2), product.arg(1)),
    ] {
        let Some(inverse) = scale.node(code).filter(|n| n.is(68, false)) else {
            continue;
        };
        let Some(length) = inverse.arg(1).node(code).filter(|n| n.is(16, false)) else {
            continue;
        };
        let vector = Value::new(&length.instruction.operands[1], length.at);
        let other = Value::new(&length.instruction.operands[2], length.at);
        if vector.same(code, &other) && component.same(code, &scalar(&vector, lane)) {
            return Some(vector);
        }
    }
    None
}

fn reconstructed(code: &Code, vector: &Value) -> Option<()> {
    let sqrt = scalar(vector, 2).node(code)?;
    if !sqrt.is(75, false) {
        return None;
    }
    let maximum = sqrt.arg(1).node(code)?;
    if !maximum.is(52, false) || !maximum.arg(2).literal(0.0) {
        return None;
    }
    let difference = maximum.arg(1).node(code)?;
    if !difference.is(0, false) || !difference.arg(2).literal(1.0) {
        return None;
    }
    let length = difference
        .arg(1)
        .positive()?
        .node(code)
        .filter(|n| n.is(15, false))?;
    let a = Value::new(&length.instruction.operands[1], length.at);
    let b = Value::new(&length.instruction.operands[2], length.at);
    if !a.same(code, &b) {
        return None;
    }
    (0..2)
        .all(|lane| scalar(&a, lane).same(code, &scalar(vector, lane)))
        .then_some(())
}

pub(super) fn vector(code: &Code) -> Option<Value> {
    let vector = tangent(code, unit(code, output(code)?.0)?)?;
    reconstructed(code, &vector)?;
    Some(vector)
}

fn output(code: &Code) -> Option<(Value, Option<Value>)> {
    let target = code
        .outputs
        .iter()
        .find(|s| s.name == "SV_TARGET" && s.index == 1)?;
    let (at, instruction) = code.instructions.iter().enumerate().rev().find(|(_, i)| {
        i.operands.first().is_some_and(|d| {
            d.kind == 2 && d.indices[0].base as usize == target.register && d.mask & 7 != 0
        })
    })?;
    if instruction.code != 50 || !instruction.saturate || instruction.operands[0].mask & 7 != 7 {
        return None;
    }
    let scale = Value::scalar(&instruction.operands[2], 0, at);
    if !(0..3).all(|i| scale.same(code, &Value::scalar(&instruction.operands[2], i, at)))
        || !normal_weight(code, &scale)
    {
        return None;
    }
    let node = Value::new(&instruction.operands[1], at);
    (0..3)
        .all(|lane| literal(&instruction.operands[3], lane, 0.5))
        .then_some((node, scale.node(code).map(|n| n.arg(1))))
}

pub(super) fn smooth(code: &Code) -> Option<Value> {
    output(code)?.1
}

fn normal_weight(code: &Code, value: &Value) -> bool {
    if value.literal(0.375) {
        return true;
    }
    value
        .node(code)
        .is_some_and(|n| n.is(50, false) && n.arg(2).literal(0.125) && n.arg(3).literal(0.375))
}

fn decode(code: &Code, value: Value, constants: &[[f32; 4]]) -> Option<([Constant; 2], Value)> {
    let node = value.node(code)?;
    if !node.is(50, false) {
        return None;
    }
    let scale = node.arg(2).constant(code)?;
    let bias = node.arg(3).constant(code)?;
    (scale.row == bias.row
        && scale.lane == 0
        && bias.lane == 1
        && scale.valid(constants)
        && bias.valid(constants))
    .then_some(([scale, bias], node.arg(1)))
}

fn layer(code: &Code, value: Value, base: Bank, params: usize) -> Option<(Value, Value)> {
    let node = value.node(code)?;
    if !node.is(50, false) || !node.arg(2).bank(code)?.matches(base, params, [1; 3], true) {
        return None;
    }
    Some((node.arg(1), node.arg(3)))
}

fn component(
    code: &Code,
    value: Value,
    base: Bank,
    wear: &Value,
    selector: &Value,
) -> Option<(Value, Value)> {
    let final_op = value.node(code)?;
    let selected = final_op.arg(1);
    if !final_op.is(50, false)
        || gain::selector(code, &selected.operand, selected.before)?
            != gain::selector(code, &selector.operand, selector.before)?
    {
        return None;
    }
    let (painted, plain) = remap::difference(code, &final_op)?;
    let mix = painted.node(code)?;
    if !mix.is(50, false) || !mix.arg(1).same(code, wear) {
        return None;
    }
    let (pristine, worn) = remap::difference(code, &mix)?;
    let (detail, pbase) = layer(code, pristine, base, 2)?;
    let (wdetail, wbase) = layer(code, worn, base, 8)?;
    (detail.same(code, &wdetail) && plain.same(code, &pbase) && plain.same(code, &wbase))
        .then_some((plain, detail))
}

fn detail_decode(
    code: &Code,
    value: &Value,
    raw_lane: usize,
    constants: &[[f32; 4]],
) -> Option<([[Constant; 2]; 3], [usize; 3])> {
    let definitions = detail::definitions(code, value)?;
    let mut rows = [None; 3];
    let mut samples = [None; 3];
    for at in &definitions[0] {
        let at = (*at)?;
        let instruction = &code.instructions[at];
        if instruction.code != 50 || instruction.saturate {
            return None;
        }
        let lane = value.operand.lanes[0];
        let coefficient = |arg| {
            Value::scalar(&instruction.operands[arg], lane, at)
                .constant(code)
                .filter(|c| c.valid(constants))
        };
        let scale = coefficient(2)?;
        let bias = coefficient(3)?;
        if scale.row != bias.row || scale.lane != 0 || bias.lane != 1 {
            return None;
        }
        let source = Value::scalar(&instruction.operands[1], lane, at);
        let sampled = gain::writer(code, &source.operand, 0, at)?;
        if code.instructions[sampled + 1..at]
            .iter()
            .any(|i| matches!(i.code, 18 | 21 | 31))
        {
            return None;
        }
        let sample = &code.instructions[sampled];
        if !matches!(sample.code, 69 | 72 | 73) || sample.saturate {
            return None;
        }
        let resource = &sample.operands[2];
        let sampler = &sample.operands[3];
        let slot = resource.indices[0].base as usize;
        if resource.kind != 7
            || ![5, 7, 9].contains(&slot)
            || resource.lanes[source.operand.lanes[0]] != raw_lane
            || sampler.kind != 6
            || sampler.indices[0].base != 1
            || !code
                .resources
                .iter()
                .any(|r| r.slot == slot && r.dimension == 3 && !r.integer)
        {
            return None;
        }
        let x = detail::coordinate(code, constants, &sample.operands[1], 0, sampled, 0)?;
        let y = detail::coordinate(code, constants, &sample.operands[1], 1, sampled, 0)?;
        if x[1] != 0.0 || y[0] != 0.0 {
            return None;
        }
        let row = &mut rows[(slot - 5) / 2];
        if row.is_some_and(|old| old != [scale, bias]) {
            return None;
        }
        *row = Some([scale, bias]);
        let previous = &mut samples[(slot - 5) / 2];
        if previous.is_some_and(|old| old != sampled) {
            return None;
        }
        *previous = Some(sampled);
    }
    Some((
        [rows[0]?, rows[1]?, rows[2]?],
        [samples[0]?, samples[1]?, samples[2]?],
    ))
}

pub(super) fn recover(
    code: &Code,
    constants: &[[f32; 4]],
    base: Bank,
    wear: &Value,
    selector: &Value,
    plate: &Value,
) -> Option<Normal> {
    let vector = tangent(code, unit(code, output(code)?.0)?)?;
    reconstructed(code, &vector)?;
    let mut found = None;
    for lane in 0..2 {
        let (plain, detail) = component(code, scalar(&vector, lane), base, wear, selector)?;
        let (decode, sample) = decode(code, plain, constants)?;
        let at = gain::sample(code, &sample.operand, &[0], sample.before, 1, &[lane])?;
        let instruction = &code.instructions[at];
        let albedo_at = gain::sample(
            code,
            &plate.operand,
            &[0, 1, 2],
            plate.before,
            0,
            &[0, 1, 2],
        )?;
        let albedo = &code.instructions[albedo_at];
        if !(0..2).all(|i| {
            Value::scalar(&instruction.operands[1], i, at)
                .same(code, &Value::scalar(&albedo.operands[1], i, albedo_at))
        }) {
            return None;
        }
        if (0..4).any(|i| {
            instruction.operands[0].mask & (1 << i) != 0 && instruction.operands[2].lanes[i] > 1
        }) {
            return None;
        }
        let (detail, samples) = detail_decode(code, &detail, lane, constants)?;
        if found.as_ref().is_some_and(|old: &Normal| {
            old.base != decode || old.detail != detail || old.samples != samples
        }) {
            return None;
        }
        found = Some(Normal {
            base: decode,
            detail,
            samples,
        });
    }
    found
}

pub(super) fn present(code: &Code) -> bool {
    output(code).is_some()
}
