//! Match the native packed intensity writer before retaining its target-Y dependencies.
use super::*;
use crate::model_preview::vertex::table;

#[derive(Clone, Copy)]
struct Value<'a> {
    operand: &'a Operand,
    lane: usize,
    before: usize,
}

impl<'a> Value<'a> {
    fn node(self, code: &'a Code) -> Option<(&'a Instruction, usize, usize)> {
        let source = self.operand;
        if source.kind != 0
            || source.modifier != 0
            || source.indices.len() != 1
            || source.indices[0].relative.is_some()
        {
            return None;
        }
        let lane = source.lanes[self.lane];
        code.instructions[..self.before]
            .iter()
            .enumerate()
            .rev()
            .find_map(|(at, row)| {
                let destination = row.operands.first()?;
                (destination.kind == 0
                    && destination.indices[0].base == source.indices[0].base
                    && destination.mask & (1 << lane) != 0)
                    .then_some((row, at, lane))
            })
    }
    fn number(self, number: f32) -> bool {
        literal(self.operand, self.lane, number)
    }
}

fn value(row: &Instruction, before: usize, lane: usize, source: usize) -> Value<'_> {
    Value {
        operand: &row.operands[source],
        lane,
        before,
    }
}

fn encoder<'a>(code: &'a Code, encoded: Value<'a>) -> Option<Value<'a>> {
    let (affine, at, lane) = encoded.node(code)?;
    if affine.code != 50
        || !affine.saturate
        || !value(affine, at, lane, 2).number(1.0 / 13.0)
        || !value(affine, at, lane, 3).number(7.0 / 13.0)
    {
        return None;
    }
    let (log, at, lane) = value(affine, at, lane, 1).node(code)?;
    if log.code != 47 || log.saturate {
        return None;
    }
    let (add, at, lane) = value(log, at, lane, 1).node(code)?;
    if add.code != 0 || add.saturate {
        return None;
    }
    if value(add, at, lane, 1).number(1.0 / 128.0) {
        Some(value(add, at, lane, 2))
    } else if value(add, at, lane, 2).number(1.0 / 128.0) {
        Some(value(add, at, lane, 1))
    } else {
        None
    }
}

fn visibility(code: &Code, ambient: Value<'_>) -> bool {
    ambient
        .node(code)
        .is_some_and(|(row, _, _)| row.code == 54 && row.saturate)
}

fn writer(code: &Code) -> Option<(usize, &Instruction)> {
    code.instructions.iter().enumerate().rev().find(|(_, row)| {
        row.operands
            .first()
            .is_some_and(|v| v.kind == 2 && v.indices[0].base == 2 && v.mask & 2 != 0)
    })
}

pub(super) fn present(code: &Code) -> bool {
    writer(code).is_some_and(|(at, row)| !(row.code == 54 && value(row, at, 1, 1).number(0.0)))
}

pub(super) fn recover(code: &Code) -> bool {
    let Some((at, row)) = writer(code) else {
        return false;
    };
    if row.code != 56 || row.saturate {
        return false;
    }
    let packed = if value(row, at, 1, 2).number(0.5) {
        value(row, at, 1, 1)
    } else if value(row, at, 1, 1).number(0.5) {
        value(row, at, 1, 2)
    } else {
        return false;
    };
    let Some((sum, at, lane)) = packed.node(code) else {
        return false;
    };
    if sum.code != 0 || sum.saturate {
        return false;
    }
    let a = value(sum, at, lane, 1);
    let b = value(sum, at, lane, 2);
    (encoder(code, a).is_some() && visibility(code, b))
        || (encoder(code, b).is_some() && visibility(code, a))
}

/// Native vehicle lights select packed emission only when its source exceeds the
/// authored threshold. The other branch retains fully visible, nonemitting material.
pub(in crate::model_preview::effects::native) fn recover_surface(code: &Code) -> bool {
    let Some((at, row)) = writer(code) else {
        return false;
    };
    if row.code != 55 || row.saturate || !value(row, at, 1, 3).number(0.5) {
        return false;
    }
    let Some(power) = surface_power(code, value(row, at, 1, 2)) else {
        return false;
    };
    let Some((condition, at, lane)) = value(row, at, 1, 1).node(code) else {
        return false;
    };
    if condition.code != 49 || condition.saturate || !value(condition, at, lane, 1).number(0.00001)
    {
        return false;
    }
    let compared = value(condition, at, lane, 2);
    same_value(code, power, compared)
}

fn surface_power<'a>(code: &'a Code, packed: Value<'a>) -> Option<Value<'a>> {
    let (half, at, lane) = packed.node(code)?;
    if half.code != 56 || half.saturate || !value(half, at, lane, 2).number(0.5) {
        return None;
    }
    let (sum, at, lane) = value(half, at, lane, 1).node(code)?;
    if sum.code != 0 || sum.saturate || !value(sum, at, lane, 2).number(1.0 + 2.0 / 255.0) {
        return None;
    }
    encoder(code, value(sum, at, lane, 1))
}

fn same_value(code: &Code, a: Value<'_>, b: Value<'_>) -> bool {
    if !read_eq(a.operand, a.lane, b.operand, b.lane) {
        return false;
    }
    if a.operand.kind == 4 {
        return a.operand.literal[a.operand.lanes[a.lane]]
            == b.operand.literal[b.operand.lanes[b.lane]];
    }
    if a.operand.kind != 0 {
        return a.operand.kind != 2;
    }
    a.node(code)
        .zip(b.node(code))
        .is_some_and(|((_, a, x), (_, b, y))| (a, x) == (b, y))
}

fn quantize(value: f32) -> f32 {
    (value.clamp(0.0, 1.0) * 255.0).round_ties_even() / 255.0
}

pub(in crate::model_preview) fn decode(value: f32) -> f32 {
    // Quantize the native eight-bit material target before its recovered 2/255 correction.
    let packed = quantize(value);
    let encoded = (2.0 * packed - (1.0 + 2.0 / 255.0)).clamp(0.0, 1.0);
    (13.0 * encoded - 7.0).exp2() - 1.0 / 128.0
}

pub(in crate::model_preview) fn ambient(y: f32, w: f32, power: f32) -> f32 {
    let visibility = (2.0 * quantize(y)).clamp(0.0, 1.0) * quantize(w);
    visibility.powi(2).max(0.0001).powf(power)
}

/// Stored Shadowkeep global default consumed by CB0[58].x in the ambient technique.
/// Scene controllers can drive this channel in game. The preview uses its stored value.
pub(in crate::model_preview::effects::native) fn ambient_power(
    manager: &PackageManager,
) -> Option<f32> {
    let name = crate::hash::fnv1_name_hash("ao_ambient_weight");
    let mut result: Option<f32> = None;
    for (tag, _) in manager.get_all_by_reference(0x8080_858D) {
        let bytes = checked(manager, tag.0, 0x8080_858D).ok()?;
        let (count, names) = table(&bytes, 8, 0x8080_0070, 4, 1024).ok()?;
        let (values, rows) = table(&bytes, 0x18, 0x8080_0090, 16, 1024).ok()?;
        if count != values {
            return None;
        }
        for index in 0..count {
            if u32_at(&bytes, names + index * 4).ok()? != name {
                continue;
            }
            let value = f32::from_bits(u32_at(&bytes, rows + index * 16).ok()?);
            if !value.is_finite()
                || !(0.0..=16.0).contains(&value)
                || result.is_some_and(|previous| previous != value)
            {
                return None;
            }
            result = Some(value);
        }
    }
    result
}
