//! Effects that sound nodes name in their effect slots: share sets (hierarchy type 18) and
//! custom effects (type 19). Both versions store the same record, a plugin, its parameter block,
//! bank media and RTPC rows, and version 113 ends it with an initial-property count.
//!
//! Two plugins convert, each where Shadowkeep's stock banks store the same parameter block.
//!
//! The McDSP ML1 limiter. Rise of Iron weapon banks such as `80AA32A9` share one ML1 set,
//! `278C8C34`, and Shadowkeep's stock banks hold 410 ML1 share sets and 19 custom ML1 effects. Both
//! games store the same 139-byte parameter block, with the same fields at the same offsets, so the
//! block keeps its bits. Version 89 writes the effect's RTPC rows with a 32-bit parameter number
//! and no type or accumulation bytes. Version 113 writes a type byte, an accumulation byte and a
//! one-byte parameter number. Every stock ML1 RTPC row (351) is type 0 with accumulation 0, and
//! 349 of them drive parameter 42 from game parameter `2ACC9909`, the distance the Rise of Iron
//! set uses for the same parameter.
//!
//! Audiokinetic effect `0x7E`, whose 126-byte block is six 17-byte filter bands (type, gain,
//! frequency, Q and an enable byte) followed by six 4-byte values. The Plan C shot bank `80AA86A4`
//! defines custom effect `3F2A9729` with it. Shadowkeep's stock banks `80B4FCCB`, `80B4FCCD` and
//! `80B4FCD1` define the same object with the identical block, no media and no RTPC rows. Its RTPC
//! parameter numbers have no established correspondence between the versions, so a source row
//! refuses the effect.
use super::*;

/// McDSP ML1 (company `0x100`, plugin `0x6E`, effect type 3).
const ML1: u32 = 0x006e1003;
const ML1_PARAMETERS: u32 = 139;
/// The ML1 parameter that stock rows drive from distance.
const ML1_DISTANCE_PARAMETER: u32 = 42;
/// Audiokinetic effect `0x7E` (company 0, effect type 3).
const BANDS: u32 = 0x007e0003;
const BANDS_PARAMETERS: u32 = 126;

pub(super) fn share_set(
    r: &mut Read<'_>,
    w: &mut Write,
    dependencies: &mut settings::Dependencies,
) -> Result<()> {
    record(r, w, dependencies)
}

/// A custom effect, or the generator preset that a silence source names.
pub(super) fn custom(
    r: &mut Read<'_>,
    w: &mut Write,
    dependencies: &mut settings::Dependencies,
) -> Result<()> {
    let plugin = r
        .bytes
        .get(r.at..r.at + 4)
        .context("early custom effect has no plugin")?;
    if u32::from_le_bytes(plugin.try_into()?) == silence::PLUGIN {
        return silence::preset(r, w);
    }
    record(r, w, dependencies)
}

fn record(
    r: &mut Read<'_>,
    w: &mut Write,
    dependencies: &mut settings::Dependencies,
) -> Result<()> {
    let plugin = r.u32()?;
    let size = r.u32()?;
    ensure!(
        (plugin, size) == (ML1, ML1_PARAMETERS) || (plugin, size) == (BANDS, BANDS_PARAMETERS),
        "unsupported early effect plugin {plugin:08X} with {size} parameter bytes"
    );
    w.u32(plugin);
    w.u32(size);
    w.copy(r, size as usize)?;
    ensure!(r.u8()? == 0, "early effect has bank media");
    w.u8(0);
    let count = r.u16()?;
    ensure!(
        plugin == ML1 || count == 0,
        "early effect {plugin:08X} has RTPC rows without established native parameter numbers"
    );
    w.u16(count);
    for _ in 0..count {
        let id = r.u32()?;
        let parameter = r.u32()?;
        ensure!(
            parameter == ML1_DISTANCE_PARAMETER,
            "early ML1 RTPC drives parameter {parameter}, which no stock ML1 row drives"
        );
        dependencies.params.insert(id);
        w.u32(id);
        w.u8(0); // Game parameter.
        w.u8(0); // Accumulation, as every stock ML1 row writes it.
        w.u8(parameter as u8);
        w.copy(r, 5)?; // Curve identity and scaling.
        let points = r.u16()?;
        w.u16(points);
        parameter::curve(r, w, usize::from(points))?;
    }
    // Native 113 adds an initial-property count absent from version 89.
    w.u16(0);
    Ok(())
}
