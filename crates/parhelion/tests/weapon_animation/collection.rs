//! Verify a new family page through reciprocal native membership and unchanged donor placement.
use super::*;

fn placement(reader: &mut Reader, hash: u32) -> Result<(u16, Vec<u16>, Payload)> {
    let globals = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|n| n.name == "investment_globals")
        .context("Globals")?
        .hash
        .0;
    let globals = reader.tag(globals, None)?;
    let root = reader.tag(globals.u32(16)?, None)?;
    let items = reader.tag(root.u32(8 + 48 * 16)?, None)?;
    let index = items
        .array(8, 24, Some(0x80807BE8))?
        .into_iter()
        .position(|r| items.u32(r).ok() == Some(hash))
        .context("Collection item")?;
    let collectibles = reader.tag(root.u32(8 + 19 * 16)?, None)?;
    let matches = collectibles
        .array(8, 0xB8, Some(0x80803475))?
        .into_iter()
        .enumerate()
        .filter(|(_, r)| collectibles.u16(r + 0x2C).ok() == u16::try_from(index).ok())
        .collect::<Vec<_>>();
    ensure!(matches.len() == 1, "Ambiguous item collectible");
    let (collectible, row) = matches[0];
    let parents = collectibles
        .array(row + 0x18, 2, Some(0x80803962))?
        .into_iter()
        .map(|r| collectibles.u16(r))
        .collect::<Result<Vec<_>>>()?;
    let nodes = reader.tag(root.u32(8 + 63 * 16)?, None)?;
    let rows = nodes.array(8, 0xA8, Some(0x80803056))?;
    for parent in &parents {
        let row = *rows
            .get(usize::from(*parent))
            .context("Collection parent")?;
        ensure!(
            nodes
                .array(row + 0x78, 4, None)?
                .into_iter()
                .filter(|&r| nodes.u16(r).ok() == u16::try_from(collectible).ok())
                .count()
                == 1,
            "Collection membership is not reciprocal"
        );
    }
    Ok((u16::try_from(collectible)?, parents, (*nodes).clone()))
}

pub(super) fn check(
    stock: &mut Reader,
    staged: &mut Reader,
    hash: u32,
    donor: u32,
) -> Result<Value> {
    let (_, original, _) = placement(stock, donor)?;
    let (_, retained, _) = placement(staged, donor)?;
    ensure!(
        original == retained,
        "Stock donor collection placement changed"
    );
    let (collectible, parents, nodes) = placement(staged, hash)?;
    ensure!(
        [925, 926, 927].iter().all(|p| parents.contains(p)),
        "Sunrise badge membership lost"
    );
    let pages = parents
        .iter()
        .copied()
        .filter(|p| ![925, 926, 927].contains(p))
        .collect::<Vec<_>>();
    ensure!(
        pages.len() == 1 && pages[0] >= 928,
        "New family has no private Collections page"
    );
    ensure!(
        !original.contains(&pages[0]),
        "Non-Exotic item retained its Exotic donor page"
    );
    let rows = nodes.array(8, 0xA8, Some(0x80803056))?;
    let page = rows[usize::from(pages[0])];
    let parents = nodes.array(page + 0x18, 2, None)?;
    ensure!(parents.len() == 1, "New family page has ambiguous parent");
    let parent = nodes.u16(parents[0])?;
    ensure!(
        nodes
            .array(rows[usize::from(parent)] + 0x68, 24, None)?
            .into_iter()
            .any(|r| nodes.u16(r).ok() == Some(pages[0])),
        "New family page is unreachable"
    );
    Ok(
        json!({"collectible":collectible,"page":pages[0],"parent":parent,
        "stock_donor_parents":original,"reciprocal_membership":true}),
    )
}
