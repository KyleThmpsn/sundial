//! Silence sources reference a private FxCustom preset rather than a media resource.
use super::*;

pub(super) const PLUGIN: u32 = 0x00650002;

pub(super) fn source(r: &mut Read<'_>, w: &mut Write, presets: &BTreeMap<u32, u32>) -> Result<()> {
    ensure!(r.u32()? == 0, "early silence source cannot stream media");
    let preset = r.u32()?;
    ensure!(
        presets.get(&preset) == Some(&PLUGIN),
        "early silence source needs a matching local FxCustom preset"
    );
    ensure!(
        r.u32()? == 0 && r.u32()? == 0 && r.u32()? == 0 && r.u8()? == 0,
        "early silence source has media, localization or external storage"
    );
    ensure!(r.u32()? == 0, "early silence source has inline parameters");
    // Version 89 has a UInt32 mode and separate file/offset fields. Version 113
    // narrows the mode and removes those empty fields. The source ID names the
    // generator preset, so it joins the object namespace and never the media map.
    w.u32(PLUGIN);
    w.u8(0);
    w.reference(preset, Kind::Object);
    w.u32(0);
    w.u8(0);
    w.u32(0);
    Ok(())
}

pub(super) fn preset(r: &mut Read<'_>, w: &mut Write) -> Result<()> {
    ensure!(
        r.u32()? == PLUGIN && r.u32()? == 12,
        "unsupported early FxCustom preset"
    );
    // Both plugin versions consume the same three Float32 operands: duration,
    // randomized length minus and randomized length plus. Preserve their bits.
    w.u32(PLUGIN);
    w.u32(12);
    w.copy(r, 12)?;
    ensure!(
        r.u8()? == 0 && r.u16()? == 0,
        "early silence preset has media or RTPC state"
    );
    w.u8(0);
    w.u16(0);
    // Native 113 adds an initial-property count absent from version 89.
    w.u16(0);
    Ok(())
}
