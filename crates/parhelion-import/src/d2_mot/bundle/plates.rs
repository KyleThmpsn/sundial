//! Native plate metadata supplies storage layout, never retained donor artwork.
use super::*;
use std::sync::Arc;

fn checked(r: &mut Reader, tag: u32) -> Result<Arc<Payload>> {
    let set = r.tag(tag, Some(0x808072D2))?;
    ensure!(
        set.0.len() == 0x30 && set.u32(0x20)? == 3,
        "Unsupported native plate-set layout"
    );
    for at in [0x24, 0x28, 0x2C] {
        let plate = r.tag(set.u32(at)?, Some(0x80809EBB))?;
        let rows = plate.array(0x10, 20, Some(0x80809EBD))?;
        ensure!(
            rows.first() == Some(&0x40),
            "Unsupported native plate layout"
        );
    }
    Ok(set)
}

pub(super) fn template(r: &mut Reader, preferred: u32) -> Result<(u32, Arc<Payload>)> {
    if !matches!(preferred, 0 | u32::MAX | 0x811C9DC5) {
        return Ok((preferred, checked(r, preferred)?));
    }
    // Unplated gear still needs a native plate container for the converted draw carrier.
    // Resolve its schema from installed metadata instead of requiring weapon dye textures.
    for tag in r.classes(0x808072D2) {
        if let Ok(set) = checked(r, tag) {
            return Ok((tag, set));
        }
    }
    anyhow::bail!("No native plate-set template has all three supported channels")
}

pub(super) fn single(r: &mut Reader, tag: u32, size: [u32; 2]) -> Result<Vec<u8>> {
    let source = r.tag(tag, Some(0x80809EBB))?;
    let rows = source.array(0x10, 20, Some(0x80809EBD))?;
    ensure!(
        rows.first() == Some(&0x40),
        "Unsupported native plate layout"
    );
    let mut plate = source.0[..0x54].to_vec();
    put(&mut plate, 0, &0x54u64.to_le_bytes())?;
    put(&mut plate, 0x10, &1u64.to_le_bytes())?;
    put(&mut plate, 0x30, &1u64.to_le_bytes())?;
    put(&mut plate, 0x40, &u32::MAX.to_le_bytes())?;
    // A composed atlas occupies the full canvas. Leaving other donor rows or the donor's
    // tile origin would overlay stock artwork and shift the imported textures.
    plate[0x44..0x4C].fill(0);
    put(&mut plate, 0x4C, &size[0].to_le_bytes())?;
    put(&mut plate, 0x50, &size[1].to_le_bytes())?;
    Ok(plate)
}
