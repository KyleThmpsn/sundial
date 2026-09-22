use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub fn check(
    graph: &Value,
    globals: &Payload,
    items: &Payload,
    read: &impl Fn(u32) -> Result<Payload>,
) -> Result<Option<Value>> {
    let Some(link) = graph.get("ornament") else {
        return Ok(None);
    };
    let rows = items.array(8, 24, None)?;
    let lookup = |hash: u64| -> Result<usize> {
        let found = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| items.u32(**row).ok().map(u64::from) == Some(hash))
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        ensure!(found.len() == 1, "ornament item missing or ambiguous");
        Ok(found[0])
    };
    let owner_index = lookup(link["target_weapon"].as_u64().context("ornament owner")?)?;
    let plug_index = lookup(graph["item_hash"].as_u64().context("ornament plug")?)?;
    let owner = read(items.u32(rows[owner_index] + 16)?)?;
    let sockets = owner.array(owner.pointer(0x68)?, 0x50, Some(0x808077C4))?;
    let socket = *sockets
        .get(link["target_socket"].as_u64().context("ornament socket")? as usize)
        .context("ornament socket outside owner")?;
    let resolve = |index: u16| -> Result<u32> {
        items.u32(
            *rows
                .get(index as usize)
                .context("ornament choice outside table")?,
        )
    };
    ensure!(
        resolve(owner.u16(socket + 2)?)? == 0xAEBAE371,
        "default appearance reset missing"
    );
    ensure!(
        u64::from(owner.u16(socket)?)
            == link["native_socket_type"]
                .as_u64()
                .context("native ornament type")?,
        "ornament socket category changed"
    );
    let choices = owner.array(socket + 0x40, 0x20, Some(0x80802E03))?;
    let header = owner.pointer(socket + 0x48)?;
    ensure!(
        header >= 4 && owner.u32(header - 4)? >> 16 == 0x8080,
        "ornament choices lack native header marker"
    );
    let found = choices
        .iter()
        .filter(|&&o| owner.u16(o).ok().map(usize::from) == Some(plug_index))
        .copied()
        .collect::<Vec<_>>();
    ensure!(
        found.len() == 1 && owner.u64(found[0] + 8)? == 0,
        "ornament must be one unconditional socket choice"
    );
    if let Some(disable) = link["disable_duplicate_socket"].as_u64() {
        let old = *sockets
            .get(disable as usize)
            .context("disabled ornament socket")?;
        ensure!(
            owner.u16(old)? == u16::MAX
                && owner.u16(old + 2)? == u16::MAX
                && owner.u64(old + 0x40)? == 0,
            "duplicate ornament lane remains active"
        );
    }
    let strings = read(globals.u32(0x220)?)?;
    let string_rows = strings.array(8, 24, None)?;
    let s = read(
        strings.u32(
            *string_rows
                .get(plug_index)
                .context("ornament string index")?
                + 16,
        )?,
    )?;
    let icons = read(globals.u32(16 + 75 * 16)?)?;
    let icon_rows = icons.array(8, 24, None)?;
    let icon_row = *icon_rows
        .get(s.u16(0x80)? as usize)
        .context("ornament icon index")?;
    let icon_tag = icons.u32(icon_row + 16)?;
    ensure!(
        tiger_pkg::TagHash(icon_tag).pkg_id() == 0x0AA0,
        "ornament icon is not private imported artwork"
    );
    let container = read(icon_tag)?;
    let layer = read(container.u32(0x14)?)?;
    ensure!(!layer.0.is_empty(), "ornament icon layer missing");
    let dense = read(globals.u32(16 + 74 * 16)?)?;
    let presentations = dense.array(0x38, 16, None)?;
    let selectors = dense.array(0x28, 8, None)?;
    let tags = dense.array(8, 4, None)?;
    let selector = dense.u32(
        *presentations
            .get(plug_index)
            .context("ornament dense row")?
            + 8,
    )? as usize;
    let tag_index =
        dense.u32(*selectors.get(selector).context("ornament dense selector")?)? as usize;
    ensure!(
        dense.u32(*tags.get(tag_index).context("ornament dense tag")?)? == icon_tag,
        "ornament dense icon differs from item strings"
    );
    let collection = collections(globals, &s, plug_index, owner_index, read)?;
    Ok(Some(
        json!({"owner":link["target_weapon"],"plug":graph["item_hash"],"socket":link["target_socket"],"default_reset":true,"unconditional_choice":true,"icon":format!("{icon_tag:08X}"),"collections":collection,"gameplay_verified":false}),
    ))
}

fn collections(
    globals: &Payload,
    strings: &Payload,
    plug: usize,
    owner: usize,
    read: &impl Fn(u32) -> Result<Payload>,
) -> Result<Value> {
    let root = read(globals.u32(16)?)?;
    let collectibles = read(root.u32(8 + 19 * 16)?)?;
    let rows = collectibles.array(8, 0xB8, Some(0x80803475))?;
    let index = |item: usize| -> Result<usize> {
        let found = rows
            .iter()
            .enumerate()
            .filter(|(_, o)| collectibles.u16(**o + 0x2C).ok().map(usize::from) == Some(item))
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        ensure!(
            found.len() == 1,
            "ornament or owner collectible missing/duplicated"
        );
        Ok(found[0])
    };
    let index = index(plug)?;
    let owner_row = *rows
        .iter()
        .find(|&&o| collectibles.u16(o + 0x2C).ok().map(usize::from) == Some(owner))
        .context("owner collectible")?;
    let flags = |row: usize| -> Result<Vec<(u8, u16)>> {
        collectibles
            .array(row + 0x70, 8, Some(0x80807D31))?
            .into_iter()
            .map(|o| Ok((collectibles.u8(o)?, collectibles.u16(o + 4)?)))
            .collect()
    };
    ensure!(
        flags(rows[index])? == flags(owner_row)? && !flags(owner_row)?.is_empty(),
        "ornament acquisition differs from its owner unlock"
    );
    let parents = collectibles.array(rows[index] + 0x18, 2, Some(0x80803962))?;
    ensure!(
        parents.len() == 1,
        "ornament must have one Collections leaf"
    );
    let leaf = collectibles.u16(parents[0])? as usize;
    let nodes = read(root.u32(8 + 63 * 16)?)?;
    let node_rows = nodes.array(8, 0xA8, Some(0x80803056))?;
    let parent = |index: usize| -> Result<usize> {
        let row = *node_rows
            .get(index)
            .context("collection node outside table")?;
        let parents = nodes.array(row + 0x18, 2, Some(0x80803962))?;
        ensure!(parents.len() == 1, "ambiguous collection ancestry");
        Ok(nodes.u16(parents[0])? as usize)
    };
    let ornaments = parent(leaf)?;
    let exotics = parent(ornaments)?;
    ensure!(
        nodes.u32(node_rows[ornaments] + 0x28)? == 0x47D9E328
            && nodes.u32(node_rows[exotics] + 0x28)? == 0x3FB0E331,
        "ornament is outside Exotics / Weapon Ornaments"
    );
    let children = nodes.array(node_rows[leaf] + 0x78, 4, Some(0x80803068))?;
    ensure!(
        children
            .iter()
            .filter(|&&o| nodes.u16(o).ok().map(usize::from) == Some(index))
            .count()
            == 1,
        "ornament leaf child missing/duplicated"
    );
    let objectives = read(root.u32(8 + 58 * 16)?)?;
    let objective_rows = objectives.array(8, 0xA0, None)?;
    let objective = nodes.u16(node_rows[leaf] + 0x50)? as usize;
    ensure!(
        objectives.u32(objective_rows[objective] + 0x30)? as usize == children.len(),
        "ornament collection count target mismatch"
    );
    let displays = read(globals.u32(16 + 15 * 16)?)?;
    let display_rows = displays.array(8, 0x60, Some(0x80802E5F))?;
    let display = display_rows[index];
    ensure!(
        displays.u32(display)? == collectibles.u32(rows[index] + 0x28)?
            && displays.u16(display + 4)? == strings.u16(0x80)?
            && displays.u64(display + 8)? == strings.u64(0x84)?,
        "ornament collection display identity/name/icon mismatch"
    );
    Ok(
        json!({"collectible_index":index,"leaf_index":leaf,"path":"Exotics / Weapon Ornaments / weapon type","owner_unlock":true,"leaf_count":children.len()}),
    )
}
