//! Read content defaults and the paired Barrel slots directly from original source objects.
use super::*;

pub(super) fn graph(source: &Source, entity: &Payload, key: u32) -> Result<(u32, &'static str)> {
    let mut content = Vec::new();
    let mut barrels = BTreeSet::new();
    for row in entity.array(32, 12, Some(0x80800715))? {
        let tag = entity.u32(row)?;
        let owner = source.read(tag)?;
        let d = owner.pointer(24)?;
        if owner.u32(d - 4)? == 0x8080248D {
            let variants = owner.array(d + 0x1F0, 0x140, Some(0x80802312))?;
            let selected = variants
                .into_iter()
                .filter(|&row| owner.u32(row + 24).ok() == Some(key))
                .collect::<Vec<_>>();
            ensure!(selected.len() <= 1, "Ambiguous source content key");
            let (row, route) = selected
                .first()
                .map_or((d + 0xB0, "content_default"), |&row| {
                    (row, "content_variant")
                });
            content.push((owner.u32(row + 0x98)?, route));
        }
        for instance in (0..owner.0.len().saturating_sub(24)).step_by(8) {
            if owner.u32(instance)? != tag || owner.u64(instance + 16)? != 0x808022BD {
                continue;
            }
            let definition = usize::try_from(owner.u64(instance + 8)?)?;
            ensure!(
                owner.u32(definition)? == tag
                    && owner.u64(definition + 8)? == instance as u64
                    && owner.u64(definition + 16)? == 0x8080221A,
                "Source Barrel pair is inconsistent"
            );
            barrels.insert((
                owner.u32(definition + 0x9B8)?,
                owner.u32(definition + 0x9C8)?,
            ));
        }
    }
    ensure!(content.len() == 1, "Source content owner is ambiguous");
    let selected = content[0];
    if !matches!(selected.0, 0 | u32::MAX) {
        return Ok(selected);
    }
    ensure!(barrels.len() == 1, "Source Barrel selection is ambiguous");
    let (normal, alternate) = barrels.into_iter().next().context("Source Barrel")?;
    ensure!(
        normal == alternate && !matches!(normal, 0 | u32::MAX),
        "Source launch modes select different projectiles"
    );
    Ok((normal, "barrel"))
}
