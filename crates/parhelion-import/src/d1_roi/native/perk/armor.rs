//! Armor 2.0 appearances can borrow a legacy armor perk socket without borrowing its effects.
use super::*;

pub(super) fn layout(native: &mut Reader, donor: u32) -> Result<Value> {
    let tag = crate::tiger::shadowkeep::item_tag(native, donor)?;
    let definition = native.tag(tag, Some(0x80807BEA))?;
    let bucket = definition.u8(0xB8)?;
    ensure!(
        (3..=7).contains(&bucket),
        "Armor perk host has a different equipment bucket"
    );
    let globals = native
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|tag| tag.name == "investment_globals")
        .context("Native investment globals")?
        .hash
        .0;
    let globals = native.tag(globals, None)?;
    let root = native.tag(globals.u32(16)?, Some(0x80807D84))?;
    let items = native.tag(root.u32(8 + 48 * 16)?, None)?;
    for row in items.array(8, 24, Some(0x80807BE8))? {
        let item = native.tag(items.u32(row + 16)?, Some(0x80807BEA))?;
        if item.u8(0xB8)? != bucket || item.u8(0xBA)? == 5 || item.u64(0x68)? == 0 {
            continue;
        }
        let sockets = crate::tiger::shadowkeep::sockets(native, items.u32(row)?)?;
        let (_, layouts) = perk_sockets(
            sockets["sockets"]
                .as_array()
                .context("Armor socket layouts")?,
            Host::Armor,
        )?;
        if !layouts.is_empty() {
            return Ok(sockets);
        }
    }
    anyhow::bail!("No native armor perk socket layout exists for this equipment bucket")
}
