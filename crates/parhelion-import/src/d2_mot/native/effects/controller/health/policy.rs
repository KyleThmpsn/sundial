//! Multiplicative health damage policies with category-preserving predicates.
use super::*;
use crate::d2_mot::native::effects::resources::selectors;

pub const SOURCE_CLASS: u32 = 0x8080402D;
pub const NATIVE_CLASS: u32 = 0x80804BFB;

/// The original source 16D3F90 and native B88F80/CD36F0 consumers multiply
/// each matching row's +20 value. The source's second policy array is a separate
/// extension and is not represented by this native resource.
pub fn emit(source: &Payload, resources: &selectors::Resources) -> Result<Payload> {
    ensure!(
        source.u64(0)? == source.0.len() as u64 && source.0.len() >= 40,
        "health damage policy size differs"
    );
    ensure!(
        source.bytes::<16>(24)? == [0; 16],
        "health damage policy secondary array requires translation"
    );
    let rows = source.array(8, 40, Some(0x80804032))?;
    let mut data = Vec::new();
    for &row in &rows {
        ensure!(
            source.u32(row)? & !0xFF == 0 && source.u32(row + 4)? & !0xFF == 0,
            "health damage policy ability mask exceeds native input classes"
        );
        ensure!(
            source.u64(row + 8)? == 0
                && source.u8(row + 24)? <= 1
                && source.u8(row + 25)? <= 1
                && source.bytes::<6>(row + 26)? == [0; 6]
                && source.u32(row + 36)? == 0,
            "health damage policy predicate extension differs"
        );
        let multiplier = source.f32(row + 32)?;
        ensure!(
            multiplier.is_finite() && multiplier >= 0.0,
            "health damage multiplier is invalid"
        );
        let mut record = source.bytes::<40>(row)?;
        record[16..24].fill(0);
        data.extend(record);
    }
    let mut output = Payload(vec![0; 24]);
    append_array(&mut output.0, 8, 0x80804C00, &data, 40)?;
    let native_rows = output.array(8, 40, Some(0x80804C00))?;
    for (&from, &to) in rows.iter().zip(&native_rows) {
        let gates = selectors::append_mask(source, from + 16, &mut output, to + 16, resources)?;
        ensure!(
            gates.is_empty(),
            "health damage policy has untranslated category predicates"
        );
    }
    let len = output.0.len() as u64;
    put(&mut output.0, 0, &len.to_le_bytes())?;
    Ok(output)
}
