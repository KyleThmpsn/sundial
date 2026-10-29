//! Shared HUD identities retain native rows or receive private source text and artwork.
use super::*;
use base64::Engine;
use sundial::package_authoring::sandbox_perk::program::HudStatus;

pub(super) fn read(source: &Source, native: &mut Reader, name: u32) -> Result<Option<HudStatus>> {
    // Both original HUD insertion methods discard the empty identity. Preserve it
    // without inventing a display row or dropping the attached status's other behavior.
    if name == 0x811C9DC5 {
        return Ok(None);
    }
    let statuses = native.tag(0x80D5FFD3, Some(0x80804A55))?;
    let rows = statuses.array(8, 112, Some(0x80804A59))?;
    if rows.iter().any(|&a| statuses.u32(a).ok() == Some(name)) {
        return Ok(None);
    }
    let mut matching = BTreeSet::new();
    for tag in source.classes(0x808022D2) {
        let table = source.read(tag)?;
        for at in table.array(8, 112, Some(0x8080295C))? {
            if table.u32(at)? == name {
                matching.insert(table.bytes::<112>(at)?);
            }
        }
    }
    ensure!(
        matching.len() == 1,
        "Source HUD identity is missing or ambiguous"
    );
    let row = Payload(matching.into_iter().next().expect("one HUD row").to_vec());
    ensure!(
        row.bytes::<16>(8)? == [0xFF; 16] && row.bytes::<88>(24)? == [0; 88],
        "Source HUD row has additional display policy"
    );
    // Rapid Hit supplies a generic buff row with a plain RGBA layer. The private builder
    // replaces its identity, every locale and its artwork, preserving the source icon.
    let template = rows
        .iter()
        .find(|&&a| statuses.u32(a).ok() == Some(0xC35DD059))
        .context("Native HUD image template is absent")?;
    ensure!(
        statuses.bytes::<20>(*template + 8)? == [0xFF; 20]
            && statuses.bytes::<84>(*template + 28)? == [0; 84],
        "Native HUD image template has additional policy"
    );
    let icon = source.layer_icon(row.u32(4)?)?;
    let image = image::RgbaImage::from_raw(icon.width.into(), icon.height.into(), icon.rgba)
        .context("Source HUD image dimensions differ")?;
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png)?;
    Ok(Some(HudStatus {
        name: crate::d1_roi::localization::hashed_text(source, name)?,
        icon: Some(0xC35DD059),
        image: Some(base64::engine::general_purpose::STANDARD.encode(png.into_inner())),
    }))
}
