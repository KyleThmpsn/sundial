//! Source inclusive scalar predicates mapped to the native half-open comparison.
use super::*;

pub(super) struct Range {
    minimum: f32,
    maximum: f32,
    input: u32,
}

pub(super) fn read(source: &Payload, field: usize, inputs: usize) -> Result<Option<Range>> {
    if source.u64(field)? == 0 {
        return Ok(None);
    }
    let at = source.pointer(field)?;
    ensure!(
        source.u32(
            at.checked_sub(4)
                .context("Source sequence predicate marker")?
        )? == 0x8080045E
            && source.u32(at)? == 0,
        "Source sequence predicate requires another condition converter"
    );
    let minimum = source.f32(at + 4)?;
    let maximum = source.f32(at + 8)?;
    let input = source.u32(at + 12)?;
    ensure!(
        minimum <= maximum && (input as usize) < inputs,
        "Source sequence predicate range or input differs"
    );
    Ok(Some(Range {
        minimum,
        maximum,
        input,
    }))
}

pub(super) fn exclusive_maximum(maximum: f32) -> f32 {
    // Original source 3B95FC..3B960A accepts both endpoints. Native 587D8D..587D9F
    // excludes the upper one. Advance only that Float32 endpoint, including MAX to
    // positive infinity. Infinite inputs still fail the native strict comparison.
    if maximum == 0. {
        f32::from_bits(1)
    } else if maximum > 0. {
        f32::from_bits(maximum.to_bits() + 1)
    } else {
        f32::from_bits(maximum.to_bits() - 1)
    }
}

pub(super) fn write(target: &mut Payload, field: usize, range: Option<Range>) -> Result<()> {
    let Some(range) = range else {
        return put(&mut target.0, field, &0u64.to_le_bytes());
    };
    let maximum = exclusive_maximum(range.maximum);
    let at = native::object(target, 0x808093ED, 16)?;
    put(&mut target.0, at + 4, &range.minimum.to_le_bytes())?;
    put(&mut target.0, at + 8, &maximum.to_le_bytes())?;
    put(&mut target.0, at + 12, &range.input.to_le_bytes())?;
    native::relative(target, field, at)
}
