//! Native contact-effect compatibility preserves the row's operands and subclass restriction.
use super::*;

pub(super) fn lower(
    source: &Source,
    reader: &mut Reader,
    p: &Payload,
    row: usize,
    native: &Payload,
) -> Result<(u32, Value)> {
    let key = p.u32(row)?;
    let rows = native.array(native.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    let rows = rows
        .into_iter()
        .filter(|&r| native.u32(r + 16).ok() == Some(key))
        .collect::<Vec<_>>();
    ensure!(
        rows.len() == 1,
        "Native contact-effect property is absent or ambiguous"
    );
    let n = rows[0];
    let sm = p.pointer(row + 32)?;
    let nm = native.pointer(n + 48)?;
    let sg = p.pointer(row + 24)?;
    let ng = native.pointer(n + 40)?;
    ensure!(
        p.u32(sm - 4)? == 0x80802AA8
            && native.u32(nm + 4)? == 0x8080442C
            && native.u32(nm - 4)? == 0x8080442D
            && p.bytes::<8>(sm)? == native.bytes::<8>(nm + 16)?
            && p.u64(sm + 8)? == 0
            && native.u64(nm + 24)? == 0
            && p.u32(sm + 20)? == 0
            && native.u32(nm + 36)? == 0
            && p.f32(sm + 4)?.is_finite()
            && p.bytes::<8>(row + 4)? == native.bytes::<8>(n + 20)?
            && p.u32(row + 8)? == 255
            && native.u32(n + 32)? == p.u32(row + 16)?
            && native.u32(n + 36)? == 0
            && p.u32(sg - 4)? == 0x80802981
            && native.u32(ng - 4)? == 0x8080454A
            && p.u32(sg)? == native.u32(ng)?
            && native.u32(ng + 4)? == 0
            && ![0, u32::MAX, 0x811C9DC5].contains(&p.u32(sg)?),
        "Contact effect has different controls, selection or subclass gate"
    );
    let twin = usize::try_from(native.u64(nm + 8)?)?;
    ensure!(
        native.u32(twin + 4)? == 0x8080442D && native.u64(twin + 8)? == nm as u64,
        "Native contact-effect modifier pair is broken"
    );
    let original = p.u32(sm + 16)?;
    let target = native.u32(nm + 32)?;
    ensure!(
        source.reference(original)? == 0x80800734,
        "Source contact effect is not an entity"
    );
    source.read(original)?;
    reader.tag(target, Some(0x80809C0F))?;
    // Source 10E12C3 and native 105C77C append the same two operand words and
    // entity tag to the contact-effect list, capped at three. The entity stays native.
    Ok((
        key,
        json!({"source_effect":original,"native_effect":target}),
    ))
}
