//! Every family member's entries before the stock entries of a stock Collections list.
//!
//! Sunrise 0.6 writes the stock rows of a nested array row by row, each row from the mod that
//! changed it, and a row two mods changed to the same bytes counts once. Rows past the stock count
//! follow, each mod's in load order. So the members share the front of a list by changing
//! different rows:
//!
//! - The main mod puts its entries first and pushes the stock entries down, as a build of one mod
//!   always has. After its entries it keeps as many stock rows unchanged as the importers fill
//!   ([`keep_front_slots`]), and the stock entries follow those rows in their own order.
//! - Each importer writes its entries over the rows the stock entries held, after the main mod's
//!   and every earlier importer's ([`place_first`]), and changes no other row.
//!
//! Merged, the list holds the main mod's entries, each importer's in compile order, then every
//! stock entry in its stock order. A member loaded alone shows its own entries first, beside
//! stock rows the others would have written over, so a stock entry can show twice or not at all
//! until the family is installed together again.
use crate::AuthoringResult;
use crate::progression::presentation::{
    collectible_child_row, collectible_children, insert_collectible_children,
    write_collectible_children_from,
};
use crate::progression::{
    PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE, PRESENTATION_NODE_COLLECTIBLES_OFFSET,
};
use sundial::package_authoring::PackageManager;
use tiger_pkg::TagHash;

/// One collectible child row.
pub(crate) type Row = [u8; PRESENTATION_NODE_COLLECTIBLE_ROW_SIZE];

/// Puts `added` before the stock entries of stock node `node`'s collectibles. The main mod's go
/// ahead of every stock entry. An importer's take the rows after the main mod's and every earlier
/// importer's (`mod_family::build::front`). A member puts its entries first before it adds any
/// after the stock entries, so the node's length then counts its stock entries.
pub(crate) fn place_first(nodes: &mut Vec<u8>, node: usize, added: &[Row]) -> AuthoringResult<()> {
    if added.is_empty() {
        return Ok(());
    }
    let (_, rows) = collectible_children(nodes, node)?;
    let slot = crate::mod_family::build::front(
        node,
        PRESENTATION_NODE_COLLECTIBLES_OFFSET,
        added.len(),
        rows.len(),
    );
    if crate::package_profile::member().is_none() {
        insert_collectible_children(nodes, node, 0, added)
    } else {
        write_collectible_children_from(nodes, node, added, slot.row, slot.stock)
    }
}

/// A row for each of `members`, a donor and an authored collectible index, copied from the
/// donor's row among node `node`'s collectibles and naming the authored collectible.
pub(crate) fn donor_rows(
    nodes: &[u8],
    node: usize,
    members: &[(usize, usize)],
) -> AuthoringResult<Vec<Row>> {
    members
        .iter()
        .map(|&(donor, authored)| {
            let mut row = collectible_child_row(nodes, node, donor)?;
            crate::tag_payload::write_u16(
                &mut row,
                0,
                u16::try_from(authored).map_err(|_| {
                    crate::error::invalid("Authored collectible index does not fit 16 bits")
                })?,
            )?;
            Ok(row)
        })
        .collect()
}

/// In the main mod's finished presentation nodes `nodes`, keeps the rows the importers write
/// their entries over: after the main mod's own entries of each list where the importers put
/// some first, the stock entries that held those rows, read from the stock table `table`. Only
/// rows the stock entries held are kept. An importer's entries past them are added rows, which
/// follow the stock entries.
pub(crate) fn keep_front_slots(
    manager: &PackageManager,
    nodes: &mut Vec<u8>,
    table: TagHash,
) -> AuthoringResult<()> {
    let kept = crate::mod_family::build::kept_fronts();
    if kept.is_empty() {
        return Ok(());
    }
    let stock = crate::tag_payload::read_tag(manager, table, "Collections presentation nodes")?;
    for ((node, array), (own, importers)) in kept {
        if array != PRESENTATION_NODE_COLLECTIBLES_OFFSET {
            continue;
        }
        let (_, rows) = collectible_children(&stock, node)?;
        let end = (own + importers).min(rows.len());
        if own < end {
            insert_collectible_children(nodes, node, own, &rows[own..end])?;
        }
    }
    Ok(())
}
