//! Armor 2.0 appearances can borrow a legacy armor perk socket without borrowing its effects, and
//! an exotic's signature talent borrows a Year 1 exotic's intrinsic socket.
use super::*;

/// The investment item table: its payload and its 24-byte rows of hash and definition tag.
fn item_table(native: &mut Reader) -> Result<std::sync::Arc<Payload>> {
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
    native.tag(root.u32(8 + 48 * 16)?, None)
}

/// The donor's equipment bucket, checked to be an armor slot.
fn armor_bucket(native: &mut Reader, donor: u32) -> Result<u8> {
    let tag = crate::tiger::shadowkeep::item_tag(native, donor)?;
    let definition = native.tag(tag, Some(0x80807BEA))?;
    let bucket = definition.u8(0xB8)?;
    ensure!(
        (3..=7).contains(&bucket),
        "Armor perk host has a different equipment bucket"
    );
    Ok(bucket)
}

pub(super) fn layout(native: &mut Reader, donor: u32) -> Result<Value> {
    let bucket = armor_bucket(native, donor)?;
    let items = item_table(native)?;
    // The search can read most of the item table, thousands of definitions, so it keeps its
    // reads in memory. The export records the host it chooses.
    let mut scan = native.shared_discovery();
    for row in items.array(8, 24, Some(0x80807BE8))? {
        let item = scan.tag(items.u32(row + 16)?, Some(0x80807BEA))?;
        if item.u8(0xB8)? != bucket || item.u8(0xBA)? == 5 || item.u64(0x68)? == 0 {
            continue;
        }
        let sockets = crate::tiger::shadowkeep::sockets(&mut scan, items.u32(row)?)?;
        let (_, layouts) = perk_sockets(
            sockets["sockets"]
                .as_array()
                .context("Armor socket layouts")?,
            Host::Armor,
        )?;
        if !layouts.is_empty() {
            native.tag(items.u32(row + 16)?, Some(0x80807BEA))?;
            return crate::tiger::shadowkeep::sockets(native, items.u32(row)?);
        }
    }
    anyhow::bail!("No native armor perk socket layout exists for this equipment bucket")
}

/// A Year 1 exotic armor piece's intrinsic layout. Its signature perk is the default of a socket
/// holding one perk plug (item kind 15) of the intrinsic category, socket type 14 in the inspected
/// Shadowkeep corpus, and its stat intrinsic is the default of a socket holding an item-kind-13
/// plug of the same category, type 676, which the inspect screen does not list.
pub(super) struct Intrinsic {
    pub socket_type: u16,
    /// The stock signature plug that lends a private signature its layout and classification,
    /// so the inspect screen labels it Intrinsic and the tooltip lists it as an exotic perk.
    pub plug: u32,
    /// The socket type that holds a stat intrinsic out of the perk row.
    pub stat_socket_type: u16,
}

/// The default plug of each socket of the item at `definition`, as socket type, plug hash, plug
/// item kind and plug category.
fn default_plugs(
    scan: &mut Reader,
    items: &Payload,
    rows: &[usize],
    definition: &Payload,
) -> Result<Vec<SocketDefault>> {
    let mut defaults = Vec::new();
    for row in definition.array(definition.pointer(0x68)?, 0x50, Some(0x808077C4))? {
        let initial = definition.u16(row + 2)?;
        if initial == u16::MAX {
            continue;
        }
        let at = *rows
            .get(usize::from(initial))
            .context("Native default plug row")?;
        let plug = scan.tag(items.u32(at + 16)?, None)?;
        defaults.push((
            definition.u16(row)?,
            items.u32(at)?,
            plug.u8(0xB8)?,
            crate::tiger::shadowkeep::plug_category(&plug)?,
        ));
    }
    Ok(defaults)
}

/// The first stock legacy exotic armor piece of the donor's equipment bucket, in item table order,
/// with both a signature socket and a stat intrinsic socket. Armor 2.0 exotics also carry a kind-15
/// intrinsic, in socket type 677, beside random stat sockets, and are told apart by their mod
/// sockets, whose default plugs are item kind 37. Plugs are read directly rather than through the
/// socket report, which would replace the donor's recorded `native-sockets.json`.
pub(super) fn intrinsic(native: &mut Reader, donor: u32) -> Result<Option<Intrinsic>> {
    let bucket = armor_bucket(native, donor)?;
    let items = item_table(native)?;
    let rows = items.array(8, 24, Some(0x80807BE8))?;
    let mut scan = native.shared_discovery();
    for &row in &rows {
        let item = scan.tag(items.u32(row + 16)?, Some(0x80807BEA))?;
        if item.u8(0xB8)? != bucket || item.u8(0xBA)? != 5 || item.u64(0x68)? == 0 {
            continue;
        }
        let defaults = default_plugs(&mut scan, &items, &rows, &item)?;
        if defaults.iter().any(|&(_, _, kind, _)| kind == 37) {
            continue;
        }
        let intrinsic = |kind: u8| {
            defaults
                .iter()
                .find(|&&(_, _, plug_kind, category)| {
                    plug_kind == kind && category.map(u64::from) == Some(INTRINSIC_CATEGORY)
                })
                .copied()
        };
        if let (Some((socket_type, plug, _, _)), Some((stat_socket_type, _, _, _))) =
            (intrinsic(15), intrinsic(13))
        {
            return Ok(Some(Intrinsic {
                socket_type,
                plug,
                stat_socket_type,
            }));
        }
    }
    Ok(None)
}

type SocketDefault = (u16, u32, u8, Option<u32>);
