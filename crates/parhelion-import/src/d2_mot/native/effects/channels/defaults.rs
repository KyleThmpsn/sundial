//! Keep authored base emission when its multiplier has no source binding.
use super::*;

/// An authored base value is eligible only when the source shader uses it as RGB gain.
pub(in crate::d2_mot::native::effects) fn validate_material_fallbacks(
    text: &str,
    fallbacks: &[Value],
) -> Result<()> {
    for fallback in fallbacks {
        let output = fallback["output"]
            .as_u64()
            .context("base emission output")?;
        let buffer = format!("cb0[{output}]");
        let body = text
            .split_once("void main(")
            .context("source pixel entry point")?
            .1;
        let consumers = body
            .lines()
            .filter(|line| line.contains(&buffer))
            .collect::<Vec<_>>();
        let rgb_gain = inputs::reads(text, 0).is_some()
            && !consumers.is_empty()
            && consumers.iter().all(|line| {
                let Some((target, expression)) = line.trim().split_once(" = ") else {
                    return false;
                };
                target
                    .strip_prefix('r')
                    .and_then(|s| s.strip_suffix(".xyz"))
                    .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                    && expression == format!("{buffer}.xxx * {target};")
            });
        if !rgb_gain {
            return Err(crate::d2_mot::source_limit(anyhow::anyhow!(
                "base emission output {output} has an uninspected shader consumer"
            )));
        }
    }
    Ok(())
}

/// The fallback is an explicit import policy, not a discovered runtime default.
/// Retain a bound multiplier whenever available and always retain the live addend.
pub(in crate::d2_mot::native::effects) fn material_program(
    material: &Payload,
    base: usize,
    owner: &Payload,
    objects: &BTreeMap<String, u8>,
) -> Result<(Vec<u8>, Vec<Value>)> {
    let code = array_bytes(material, base + 0x20, 1)?;
    if base != 0x2B0 || material.u8(48)? & 127 != 8 || material.u64(0x20)? & (1 << 13) == 0 {
        return Ok((code, vec![]));
    }
    let instructions = program::parse(&code)?;
    let mut result = Vec::with_capacity(code.len());
    let mut fallbacks = Vec::new();
    let mut index = 0;
    while index < instructions.len() {
        if let Some([constant, multiplier, addend, mad, swizzle, output]) =
            instructions.get(index..index + 6)
            && constant.op == 0x42
            && multiplier.op == 0x5C
            && addend.op == 0x5C
            && mad.op == 0x15
            && swizzle.op == 0x29
            && swizzle.args == [0]
            && output.op == 0x52
        {
            let missing = hex::encode_upper(multiplier.args);
            let live = hex::encode_upper(addend.args);
            if !objects.contains_key(&missing)
                && source_input(owner, &missing)?.is_none()
                && source_input(owner, &live)?.is_some()
            {
                let constants = material.array(base + 0x30, 16, Some(0x80800090))?;
                let at = *constants
                    .get(usize::from(constant.args[0]))
                    .context("base emission constant index")?;
                let value = [
                    material.f32(at)?,
                    material.f32(at + 4)?,
                    material.f32(at + 8)?,
                    material.f32(at + 12)?,
                ];
                if value.iter().all(|v| *v >= 0. && *v == value[0]) {
                    // constant * 1 + live, then the original X swizzle and output.
                    result.extend([constant.op, constant.args[0], addend.op]);
                    result.extend_from_slice(addend.args);
                    result.extend([0x01, swizzle.op, 0, output.op, output.args[0]]);
                    fallbacks.push(json!({
                        "output":output.args[0], "constant":constant.args[0],
                        "base_value":value, "missing_multiplier":missing,
                        "retained_addend":live, "multiplier":1,
                        "policy":"authored_base_emission",
                        "source_runtime_default_verified":false
                    }));
                    index += 6;
                    continue;
                }
            }
        }
        let instruction = &instructions[index];
        result.push(instruction.op);
        result.extend_from_slice(instruction.args);
        index += 1;
    }
    Ok((result, fallbacks))
}
