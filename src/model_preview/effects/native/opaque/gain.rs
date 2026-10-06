//! Recover the material multiplier of the unpainted plate from its exact producers.
use super::*;

pub(in crate::model_preview) struct Gain {
    row: usize,
    lanes: [usize; 3],
}

impl Gain {
    pub(in crate::model_preview) fn value(&self, frame: &Frame) -> Option<[f32; 3]> {
        let rgb = self.lanes.map(|lane| frame[self.row][lane]);
        rgb.iter().all(|v| v.is_finite()).then_some(rgb)
    }
    pub(in crate::model_preview) fn glsl(&self) -> String {
        let lane = self.lanes.map(|i| ['x', 'y', 'z', 'w'][i]);
        format!(
            "nConstant(0,{}).{}{}{}",
            self.row, lane[0], lane[1], lane[2]
        )
    }
}

pub(super) fn writer(code: &Code, value: &Operand, lane: usize, before: usize) -> Option<usize> {
    if value.kind != 0 || value.indices.len() != 1 || value.indices[0].relative.is_some() {
        return None;
    }
    code.instructions[..before].iter().rposition(|i| {
        let destinations = match i.code {
            13 | 18 | 21 | 31 | 62 => 0,
            38 | 77 | 78 => 2,
            _ => 1,
        };
        i.operands.iter().take(destinations).any(|d| {
            d.kind == 0
                && d.indices[0].base == value.indices[0].base
                && d.mask & (1 << value.lanes[lane]) != 0
        })
    })
}

pub(super) fn producer(code: &Code, value: &Operand, lane: usize, before: usize) -> Option<usize> {
    let at = writer(code, value, lane, before)?;
    let depth = code.instructions[..at]
        .iter()
        .fold(0usize, |d, i| match i.code {
            31 => d + 1,
            21 => d.saturating_sub(1),
            _ => d,
        });
    (depth == 0).then_some(at)
}

pub(super) fn rgb_producer(code: &Code, value: &Operand, before: usize) -> Option<usize> {
    let at = producer(code, value, 0, before)?;
    (1..3)
        .all(|lane| producer(code, value, lane, before) == Some(at))
        .then_some(at)
}

pub(super) fn sample(
    code: &Code,
    source: &Operand,
    lanes: &[usize],
    before: usize,
    slot: u32,
    raw: &[usize],
) -> Option<usize> {
    let at = producer(code, source, lanes[0], before)?;
    let i = &code.instructions[at];
    if !matches!(i.code, 69 | 72 | 73) || source.modifier != 0 || i.saturate {
        return None;
    }
    let r = &i.operands[2];
    (r.kind == 7
        && r.indices[0].base == slot
        && lanes.iter().zip(raw).all(|(&lane, &raw)| {
            producer(code, source, lane, before) == Some(at) && r.lanes[source.lanes[lane]] == raw
        }))
    .then_some(at)
}

pub(super) fn selector(code: &Code, s: &Operand, before: usize) -> Option<usize> {
    if s.modifier != 0 || !(1..3).all(|lane| read_eq(s, lane, s, 0)) {
        return None;
    }
    let at = producer(code, s, 0, before)?;
    let and = &code.instructions[at];
    let lane = s.lanes[0];
    if and.code != 1 || and.saturate || !literal(&and.operands[2], lane, 1.0) {
        return None;
    }
    let flag = &and.operands[1];
    if flag.modifier != 0 {
        return None;
    }
    let ge_at = producer(code, flag, lane, at)?;
    let ge = &code.instructions[ge_at];
    let lane = flag.lanes[lane];
    if ge.code != 29 || ge.saturate {
        return None;
    }
    let threshold = &ge.operands[2];
    if threshold.kind != 4
        || threshold.modifier != 0
        || threshold.literal[threshold.lanes[lane]] != (40.0f32 / 255.0).to_bits()
    {
        return None;
    }
    sample(code, &ge.operands[1], &[lane], ge_at, 2, &[3])
}

fn fallback<'a>(code: &'a Code, final_at: usize, base: &Operand) -> Option<(&'a Operand, usize)> {
    let final_op = &code.instructions[final_at];
    let subtract_at = rgb_producer(code, &final_op.operands[2], final_at)?;
    let subtract = &code.instructions[subtract_at];
    let mul_at = rgb_producer(code, &final_op.operands[3], final_at)?;
    let mul = &code.instructions[mul_at];
    if final_op.saturate
        || final_op.operands[3].modifier != 0
        || subtract.code != 50
        || mul.code != 56
        || subtract.saturate
        || mul.saturate
        || !rgb_eq(&final_op.operands[2], base)
    {
        return None;
    }
    let mut plate = subtract.operands[1].clone();
    if plate.modifier != 1 {
        return None;
    }
    plate.modifier = 0;
    let gain = &subtract.operands[2];
    if gain.kind != 8
        || gain.modifier != 0
        || gain.indices.len() != 2
        || gain.indices[0].base != 0
        || gain.indices.iter().any(|v| v.relative.is_some())
    {
        return None;
    }
    for lane in 0..3 {
        let m = final_op.operands[3].lanes[lane];
        if !read_eq(&plate, lane, &mul.operands[1], m)
            || !read_eq(gain, lane, &mul.operands[2], m)
            || producer(code, &plate, lane, subtract_at)
                != producer(code, &mul.operands[1], m, mul_at)
        {
            return None;
        }
    }
    Some((
        gain,
        sample(code, &plate, &[0, 1, 2], subtract_at, 0, &[0, 1, 2])?,
    ))
}

pub(super) fn recover(
    code: &Code,
    at: usize,
    base: &Operand,
    constants: &[[f32; 4]],
    uv: [f32; 4],
) -> Result<Option<Gain>, String> {
    let Some(final_at) = rgb_producer(code, base, at) else {
        return Ok(None);
    };
    let final_op = &code.instructions[final_at];
    if final_op.code != 50 {
        return Ok(None);
    }
    let fail = || "Opaque base gain has an unsupported producer".to_owned();
    let (gain, plate_at) = fallback(code, final_at, base).ok_or_else(fail)?;
    let mask_at = selector(code, &final_op.operands[1], final_at).ok_or_else(fail)?;
    for sample_at in [plate_at, mask_at] {
        let sample = &code.instructions[sample_at];
        for lane in 0..2 {
            let actual = gear::value(code, constants, &sample.operands[1], lane, sample_at, 0)
                .ok_or_else(fail)?;
            let mut expected = [0.0; 3];
            expected[lane] = 1.0 / uv[lane];
            expected[2] = -uv[lane + 2] / uv[lane];
            if actual
                .iter()
                .zip(expected)
                .any(|(a, b)| (a - b).abs() > 1e-5)
            {
                return Err(fail());
            }
        }
    }
    let row = gain.indices[1].base as usize;
    if row >= constants.len() || row >= 128 {
        return Err(fail());
    }
    Ok(Some(Gain {
        row,
        lanes: gain.lanes[..3].try_into().unwrap(),
    }))
}
