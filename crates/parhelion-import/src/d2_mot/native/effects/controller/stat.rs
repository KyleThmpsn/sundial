//! Stat modifier components inside an assembled attachment graph.
//!
//! The modern component `80802D2B` and Shadowkeep's `80803AFE` nest the same records: a bank
//! record at instance `+30` and settings `+48`, and one row for each modifier, whose runtime
//! records are 128 bytes apart from instance `+90` on both sides. The settings rows are 112
//! bytes from settings `+118` in the modern component and 88 bytes from `+D8` in Shadowkeep's.
//! Each side publishes its providers through interface metadata, which pairs them:
//!
//! | Modern | Interface | Shadowkeep | Interface |
//! | --- | --- | --- | --- |
//! | Settings `+68` | `808098CD` | Settings `+68` | `80809ADE` |
//! | Settings `+88` | `808091A8` | None | |
//! | Settings `+A8` | `80802D31` | Settings `+80` | `80803B04` |
//! | Settings `+D0` | `8080955E` | Settings `+A0` | `8080974B` |
//! | Row `+18` | `80802E9F` | Row `+10` | `80803C4C` |
//! | Row `+38` | `808098CD` | Row `+30` | `80809ADE` |
//!
//! The template is a shipped owner with as many rows as the source, so only the rows change.
use super::{component::*, *};
use crate::d2_mot::gameplay::perks::{lower, modifier};

/// The shipped modifier owner with `rows` rows and the graph that holds it.
pub fn template(rows: usize) -> Result<(u32, u32)> {
    Ok(match rows {
        0 => (0x80F9_F7CE, 0x80F9_F7CF),
        1 => (0x8162_C919, 0x8162_C91A),
        2 => (0x80C1_9709, 0x8161_E50E),
        3 => (0x80BC_5844, 0x8161_E47B),
        4 => (0x8161_E47F, 0x8161_E480),
        5 => (0x80FE_FA50, 0x80C1_9A4F),
        // Shipped graphs of one modifier owner and nothing else.
        6 => (0x80EF_ADDC, 0x80BC_57F3),
        7 => (0x80EF_AD9F, 0x80BC_5798),
        8 => (0x80BC_5805, 0x80BC_5806),
        9 => (0x8153_2CE4, 0x80BC_58BA),
        10 => (0x80BC_57B2, 0x80BC_57B3),
        11 => (0x80BC_58A9, 0x80BC_58AA),
        12 => (0x8153_2C96, 0x80BC_57B1),
        13 => (0x8161_E71C, 0x8161_E71D),
        14 => (0x80B7_CB8C, 0x80B7_CB8D),
        15 => (0x80EF_ADD3, 0x80BC_57E0),
        17 => (0x8153_2C8F, 0x80BC_5796),
        21 => (0x80EF_ADB1, 0x80BC_57C3),
        _ => anyhow::bail!("no Shadowkeep modifier owner holds {rows} rows"),
    })
}

/// Every modifier row of the modern bank whose settings record is at `settings`.
fn all_rows(source: &Payload, settings: usize) -> Result<Vec<usize>> {
    source.array(settings + 0x10, 112, Some(0x8080_2D33))
}

/// The rows of the modern bank at `settings` that Shadowkeep can hold. A row whose input
/// Shadowkeep's component lacks is left out, so the template has one row fewer.
pub fn bank_rows(source: &Payload, settings: usize) -> Result<Vec<usize>> {
    let mut kept = Vec::new();
    for row in all_rows(source, settings)? {
        if !modifier::absent_row(source, row)? {
            kept.push(row);
        }
    }
    Ok(kept)
}

/// The modifier rows of a modern stat modifier owner that Shadowkeep can hold.
pub fn rows(source: &Payload) -> Result<Vec<usize>> {
    bank_rows(source, source.pointer(24)? + 0x48)
}

/// Write the rows of the modern bank at `from` (its runtime and settings records) over the
/// Shadowkeep bank at `to` of `owner`, which holds as many rows, and pair their records. The
/// second list holds the modern records with no Shadowkeep endpoint: the modern-only provider
/// and the records of a row that was left out. Assembly refuses a graph that connects them.
///
/// A stat modifier owner holds one bank, and a status holds one on each timed property input
/// that modifiers drive. Both sides nest the same records in both places.
pub(super) fn bank(
    source: &Payload,
    owner: &mut Payload,
    from: (usize, usize),
    to: (usize, usize),
) -> Result<(Vec<Relocation>, Vec<Object>)> {
    let (si, sd) = from;
    let (ni, nd) = to;
    // The weight every modifier twin holds: constant and unit, with no driver.
    ensure!(
        source.f32(si + 0x30)? == 1.0
            && source.f32(si + 0x34)? == 1.0
            && source.bytes::<4>(si + 0x38)? == [0, 0, 1, 1],
        "stat modifier component does not have a constant unit weight"
    );
    let rows = bank_rows(source, sd)?;
    let slots = owner.array(nd + 0x10, 88, Some(0x8080_3B06))?;
    ensure!(
        rows.len() == slots.len(),
        "stat modifier template holds {} rows for {} source rows",
        slots.len(),
        rows.len()
    );
    let mut objects = vec![
        object(source, owner, si, 0x8080_2D2D, ni, 0x8080_3B00)?,
        object(source, owner, sd, 0x8080_2D2E, nd, 0x8080_3B01)?,
        object(
            source,
            owner,
            sd + 0x20,
            0x8080_98CD,
            nd + 0x20,
            0x8080_9ADE,
        )?,
        object(
            source,
            owner,
            sd + 0x60,
            0x8080_2D31,
            nd + 0x38,
            0x8080_3B04,
        )?,
        object(
            source,
            owner,
            sd + 0x88,
            0x8080_955E,
            nd + 0x58,
            0x8080_974B,
        )?,
    ];
    for (&row, &slot) in rows.iter().zip(&slots) {
        let (live, native_live) = (
            usize::try_from(source.u64(row + 8)?)?,
            usize::try_from(owner.u64(slot + 8)?)?,
        );
        let bytes = lower::modifier_row_with(source, row, owner, slot, modifier::destination)?;
        put(&mut owner.0, slot, &bytes)?;
        objects.extend([
            object(source, owner, live, 0x8080_2D32, native_live, 0x8080_3B05)?,
            object(source, owner, row, 0x8080_2D33, slot, 0x8080_3B06)?,
            object(
                source,
                owner,
                row + 0x18,
                0x8080_2E9F,
                slot + 0x10,
                0x8080_3C4C,
            )?,
            object(
                source,
                owner,
                row + 0x38,
                0x8080_98CD,
                slot + 0x30,
                0x8080_9ADE,
            )?,
        ]);
    }
    let tag = source.u32(source.pointer(16)?)?;
    let mut unbound = vec![Object {
        owner: tag,
        class: 0x8080_91A8,
        offset: (sd + 0x40) as u64,
    }];
    for row in all_rows(source, sd)? {
        if rows.contains(&row) {
            continue;
        }
        let live = usize::try_from(source.u64(row + 8)?)?;
        for (class, offset) in [
            (0x8080_2D32, live),
            (0x8080_2D33, row),
            (0x8080_2E9F, row + 0x18),
            (0x8080_98CD, row + 0x38),
        ] {
            unbound.push(Object {
                owner: tag,
                class,
                offset: offset as u64,
            });
        }
    }
    Ok((objects, unbound))
}

pub fn emit(
    source: &Payload,
    template: &Payload,
    allocation: &Payload,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Converted> {
    let mut owner = envelope(
        source,
        template,
        [0x8080_2D2A, 0x8080_2D2B, 0x8080_3AFD, 0x8080_3AFE],
        owner_tag,
        allocation_tag,
    )?;
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = owner.pointer(16)?;
    let nd = owner.pointer(24)?;
    let mut objects = vec![
        object(source, &owner, si, 0x8080_2D2A, ni, 0x8080_3AFD)?,
        object(source, &owner, sd, 0x8080_2D2B, nd, 0x8080_3AFE)?,
    ];
    let (records, unbound) = bank(
        source,
        &mut owner,
        (si + 0x30, sd + 0x48),
        (ni + 0x30, nd + 0x48),
    )?;
    objects.extend(records);
    seal(&mut owner)?;
    Ok(Converted {
        owner,
        allocation: allocation.clone(),
        objects,
        unbound,
    })
}
