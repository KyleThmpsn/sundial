//! Derived inputs use earlier cached outputs and transitive refresh masks.
use super::*;

pub(super) fn computed(p: &Payload, rows: &[usize], input: usize, consumer: usize) -> Result<bool> {
    let row = *rows.get(input).context("computed input declaration")?;
    if input >= consumer
        || p.u64(row + 8)? == 0
        || p.u16(row + 72)? != u16::MAX
        || p.u64(row + 80)? != 0
        || p.u64(row + 96)? != 0
    {
        return Ok(false);
    }
    let d = p.pointer(24)?;
    ensure!(
        p.u32(d - 4)? == 0x80809597,
        "computed input bank type differs"
    );
    // Exporting a computed value is valid. Importing another value into that
    // declaration would introduce a second writer outside this contract.
    for field in [0x178, 0x188] {
        for name in p.array(d + field, 12, Some(0x808095A8))? {
            if p.u32(name + 8)? as usize == input {
                return Ok(false);
            }
        }
    }
    for &declaration in rows {
        let words = p.array(declaration + 80, 4, Some(0x8080000B))?;
        let selected = |index: usize| -> Result<bool> {
            words
                .get(index / 32)
                .map(|at| p.u32(*at).map(|word| word & (1 << (index % 32)) != 0))
                .transpose()
                .map(|value| value.unwrap_or(false))
        };
        ensure!(
            !selected(input)? || selected(consumer)?,
            "computed bank input {input} lacks transitive refresh for consumer {consumer}"
        );
    }
    Ok(true)
}
