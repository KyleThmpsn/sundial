//! Immutable source material values can be part of the shader itself.
use super::*;

/// Specialize only a material with no executable TFX. Preserve floating-point
/// bits, including signed zero, without relying on a carrier's buffer lifetime.
pub fn specialize(text: &str, values: &[u8], program: &[u8]) -> Result<Option<String>> {
    if !program.is_empty() || values.is_empty() {
        return Ok(None);
    }
    ensure!(
        values.len().is_multiple_of(16),
        "partial immutable material vector"
    );
    let count = values.len() / 16;
    ensure!(
        inputs::cb_count(text, 0)? == Some(count),
        "immutable material buffer size differs"
    );
    let mut declaration = format!("static const float4 cb0[{count}] = {{\n");
    for vector in values.chunks_exact(16) {
        let components = vector
            .chunks_exact(4)
            .map(|word| format!("0x{:08X}u", u32::from_le_bytes(word.try_into().unwrap())))
            .collect::<Vec<_>>()
            .join(", ");
        declaration.push_str(&format!("  asfloat(uint4({components})),\n"));
    }
    declaration.push_str("};");
    Ok(Some(crate::d2_mot::native::shader::replace_once(
        text,
        &inputs::cb_decl(0, count),
        &declaration,
    )?))
}
