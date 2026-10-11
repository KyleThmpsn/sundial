//! Preserve source visibility conditions and the model's group indices together.
use super::*;

pub(super) fn translate(source: &Payload, model: &Payload, owner: &mut Payload) -> Result<()> {
    let source_instance = source.pointer(16)?;
    let source_resource = source.pointer(24)?;
    ensure!(
        source_instance >= 4
            && source_resource >= 4
            && source.u32(source_instance - 4)? == 0x80806D5B
            && source.u32(source_resource - 4)? == 0x80806D6C,
        "Source cloth activation component differs"
    );
    let source_groups = source.array(source_resource + 0x38, 24, Some(0x80809AF7))?;
    ensure!(
        source_groups.len() <= i16::MAX as usize,
        "Cloth activation group count exceeds the native range"
    );
    let groups = source_groups
        .into_iter()
        .map(|at| {
            ensure!(
                source.u64(at)? == 24,
                "Cloth activation record size differs"
            );
            let mut terms = Vec::new();
            for term in source.array(at + 8, 8, Some(0x80809AFB))? {
                terms.extend_from_slice(&source.bytes::<8>(term)?);
            }
            Ok(terms)
        })
        .collect::<Result<Vec<_>>>()?;
    for mesh in model.array(16, 136, Some(0x80807378))? {
        for part in model.array(mesh + 24, 32, Some(0x8080737E))? {
            let group = usize::try_from(model.i16(part + 20)?)
                .context("Cloth draw has a negative activation group")?;
            ensure!(
                group < groups.len(),
                "Cloth draw activation group is absent from its source owner"
            );
        }
    }

    // A native template supplies layout and callbacks, but its condition values
    // select the template's body variant. Keep every source group and term in
    // order so the unchanged draw indices select their own source conditions.
    let resource = owner.pointer(24)?;
    let mut records = vec![0; groups.len() * 24];
    for record in records.chunks_exact_mut(24) {
        record[..8].copy_from_slice(&24u64.to_le_bytes());
    }
    append_array(&mut owner.0, resource + 0x38, 0x80809C2C, &records, 24)?;
    for (at, terms) in owner
        .array(resource + 0x38, 24, Some(0x80809C2C))?
        .into_iter()
        .zip(groups)
    {
        append_array(&mut owner.0, at + 8, 0x80809C31, &terms, 8)?;
    }
    Ok(())
}
