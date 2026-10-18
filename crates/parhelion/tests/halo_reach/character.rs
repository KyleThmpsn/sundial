//! Follow the staged human pose into every descriptor-indexed correction table.
use super::*;

pub(super) fn verify(staged: &mut Reader, stock: &mut Reader, request: &Value) -> Result<Value> {
    let Some(character) = request.get("character").filter(|value| !value.is_null()) else {
        return Ok(Value::Null);
    };
    let pose_tag = u32::try_from(
        character["native"]["pose"]
            .as_u64()
            .context("Requested character pose")?,
    )?;
    let pose = staged.tag(pose_tag, Some(0x80809c36))?;
    let definition = pose.pointer(24)?;
    ensure!(
        pose.u32(definition - 4)? == 0x80803640,
        "Character pose definition differs"
    );
    let bank_tag = pose.u32(definition + 0x108)?;
    let bank = staged.tag(bank_tag, Some(0x808036f6))?;
    let original = stock.tag(bank_tag, Some(0x808036f6))?;
    let descriptors = bank.array(0x68, 32, Some(0x80809002))?;
    let old_descriptors = original.array(0x68, 32, Some(0x80809002))?;
    ensure!(
        descriptors.len() > old_descriptors.len(),
        "Character import added no reachable bank descriptors"
    );
    let mut tables = Vec::new();
    for (field, class, aliases) in [
        (0x114, 0x8080348e, &[8, 0x38][..]),
        (0x11c, 0x808034eb, &[0x28][..]),
    ] {
        let tag = pose.u32(definition + field)?;
        let table = staged.tag(tag, Some(class))?;
        let original = stock.tag(tag, Some(class))?;
        for &alias in aliases {
            let rows = table.array(alias, 2, Some(0x80800006))?;
            let old_rows = original.array(alias, 2, Some(0x80800006))?;
            ensure!(
                rows.len() == descriptors.len() && old_rows.len() == old_descriptors.len(),
                "Character correction table {tag:08X} at {alias:X} does not cover its bank"
            );
            for (&old, &new) in old_rows.iter().zip(&rows) {
                ensure!(
                    original.u16(old)? == table.u16(new)?,
                    "Character import changed a stock correction route"
                );
            }
            for &row in &rows[old_rows.len()..] {
                ensure!(
                    table.u16(row)? == u16::MAX,
                    "Absolute authored pose still selects a donor correction"
                );
            }
            tables.push(json!({"tag":format!("{tag:08X}"),"field":alias,
                "descriptors":rows.len(),"stock_routes_preserved":old_rows.len(),
                "private_routes_disabled":rows.len()-old_rows.len()}));
        }
    }
    Ok(
        json!({"pose":format!("{pose_tag:08X}"),"bank":format!("{bank_tag:08X}"),
        "tables":tables,"gameplay_verified":false}),
    )
}
