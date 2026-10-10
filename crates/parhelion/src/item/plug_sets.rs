//! The shared reusable plug set table, with private plugs offered everywhere.
//!
//! The investment root names the table at slot 51. Its root array at +8 holds 0x18-byte set rows
//! (class `0x80802DFF`): the set hash at +0 and the member array at +8. A member (0x20 bytes,
//! class `0x80802E03`) is the item index at +0, a condition array at +8 and a weight at +0x18.
//! A condition is one `0x80807D31` instruction, Flag `1` naming the unlock flag that makes the
//! member available. A member without one is always available, as Arc Resistance is in set 8.
//!
//! Dawn and Sunrise offer a socket the members of its embedded list and of its reusable set,
//! read from these packages. The client presents a member only in sockets that take its plug
//! category, which is why Sunrise gives four armor mods the leg category before routing them
//! into the leg set. A private plug offered everywhere therefore joins each set that offers the
//! stock plug its classification comes from, so it carries a category those sockets take. Its
//! member copies that plug's row with no condition.
//!
//! A set that gains members moves its member array to the end of the tag, the stock members'
//! conditions pointed where they already are. The authored table is decoded again and every
//! stock member must read back unchanged.
use super::*;
use crate::tag_payload::{
    append_native_array, array_at, read_array, read_i64, read_u64, relative_target,
    synchronize_payload_size, write_i64, write_u16,
};
use sundial::package_authoring::investment_schema::{
    CONDITION_EXPRESSION_ROW_CLASS, ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_CLASS,
    ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE, ROOT_REUSABLE_PLUG_SET_TABLE_SLOT,
};

const SET_CLASS: u32 = 0x8080_2DFF;
const SET_ROW: usize = 0x18;
const MEMBERS: usize = 8;
const MEMBER_ROW: usize = ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE;
const CONDITION: usize = 8;
const CONDITION_ROW: usize = 8;

/// One member as the table holds it: its row without the condition pointer, which moves with the
/// row, and its condition instructions.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Member {
    row: [u8; MEMBER_ROW],
    condition: Vec<[u8; CONDITION_ROW]>,
}

impl Member {
    fn index(&self) -> u16 {
        u16::from_le_bytes([self.row[0], self.row[1]])
    }
}

fn decode(table: &[u8]) -> AuthoringResult<Vec<Vec<Member>>> {
    if read_u64(table, 0)? != table.len() as u64 {
        return Err(invalid(
            "The plug set table's size field differs from its length",
        ));
    }
    let (count, _, rows, class) = array_at(table, 8)?;
    if class != SET_CLASS {
        return Err(invalid("The plug set table's rows are not plug sets"));
    }
    (0..count)
        .map(|set| {
            let (members, _, member_rows, member_class) =
                array_at(table, rows + set * SET_ROW + MEMBERS)?;
            if members != 0 && member_class != ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_CLASS {
                return Err(invalid(format!("Plug set {set} lists unknown members")));
            }
            (0..members)
                .map(|member| {
                    let at = member_rows + member * MEMBER_ROW;
                    let (conditions, _, condition_rows, condition_class) =
                        array_at(table, at + CONDITION)?;
                    if conditions != 0 && condition_class != CONDITION_EXPRESSION_ROW_CLASS {
                        return Err(invalid(format!(
                            "Plug set {set} member {member} has an unknown condition"
                        )));
                    }
                    let mut row: [u8; MEMBER_ROW] = read_array(table, at)?;
                    row[CONDITION + 8..CONDITION + 16].fill(0);
                    let condition = (0..conditions)
                        .map(|index| read_array(table, condition_rows + index * CONDITION_ROW))
                        .collect::<AuthoringResult<Vec<_>>>()?;
                    Ok(Member { row, condition })
                })
                .collect()
        })
        .collect()
}

/// `table` with each `(stock plug, private plug)` offer's private plug added to every set that
/// offers its stock plug, and the sets each offer joined, in offer order.
fn offer(table: &[u8], offers: &[(u16, u16)]) -> AuthoringResult<(Vec<u8>, Vec<Vec<u16>>)> {
    let stock = decode(table)?;
    let mut joined = vec![Vec::new(); offers.len()];
    let mut added = BTreeMap::<usize, Vec<Member>>::new();
    for (set, members) in stock.iter().enumerate() {
        for (offer, &(like, authored)) in offers.iter().enumerate() {
            let Some(template) = members.iter().find(|member| member.index() == like) else {
                continue;
            };
            let set_index =
                u16::try_from(set).map_err(|_| invalid("The plug set table has too many rows"))?;
            joined[offer].push(set_index);
            // The stock plug's own row, always available, naming the private plug.
            let mut row = template.row;
            write_u16(&mut row, 0, authored)?;
            row[CONDITION..CONDITION + 16].fill(0);
            added.entry(set).or_default().push(Member {
                row,
                condition: Vec::new(),
            });
        }
    }
    let (_, _, rows, _) = array_at(table, 8)?;
    let mut out = table.to_vec();
    for (&set, members) in &added {
        let descriptor = rows + set * SET_ROW + MEMBERS;
        let (count, _, from_rows, _) = array_at(table, descriptor)?;
        // The member array moves to the end: stock members first, each condition pointed at where
        // it already is, then the offered members.
        let start = (out.len() + 0x23) & !0xF;
        let mut row_bytes = vec![0; (count + members.len()) * MEMBER_ROW];
        for index in 0..count {
            let from = from_rows + index * MEMBER_ROW;
            let to = index * MEMBER_ROW;
            row_bytes[to..to + MEMBER_ROW].copy_from_slice(&table[from..from + MEMBER_ROW]);
            if read_i64(table, from + CONDITION + 8)? == 0 {
                continue;
            }
            let target = relative_target(table, from + CONDITION + 8)?;
            let relative = i64::try_from(target)
                .and_then(|target| i64::try_from(start + to + CONDITION + 8).map(|at| target - at))
                .map_err(|_| invalid("Plug set condition pointer does not fit 64 bits"))?;
            write_i64(&mut row_bytes, to + CONDITION + 8, relative)?;
        }
        for (offset, member) in members.iter().enumerate() {
            let to = (count + offset) * MEMBER_ROW;
            row_bytes[to..to + MEMBER_ROW].copy_from_slice(&member.row);
        }
        append_native_array(
            &mut out,
            descriptor,
            ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_CLASS,
            count + members.len(),
            &row_bytes,
        )?;
        if array_at(&out, descriptor)?.2 != start {
            return Err(validation(
                "The offered plug set members did not land where they were planned",
            ));
        }
    }
    let out = synchronize_payload_size(out)?;
    let read = decode(&out)?;
    let expected = stock.iter().enumerate().map(|(set, members)| {
        members
            .iter()
            .chain(added.get(&set).into_iter().flatten())
            .cloned()
            .collect::<Vec<_>>()
    });
    if read.len() != stock.len() || !read.iter().cloned().eq(expected) {
        return Err(validation(
            "The authored plug set table does not read back as planned",
        ));
    }
    Ok((out, joined))
}

/// Offers each private plug that is offered everywhere in every stock shared plug set that
/// offers the plug it is offered like, and returns the sets each plug joined, in plug order.
/// `None` when no plug is offered everywhere.
///
/// A roll resolves against a randomized set by its member count, so a set that gains members
/// must not be one a socket rolls from. No Shadowkeep item rolls from any of its 120 reusable
/// sets, and an authored socket that rolls from a joined set is refused.
pub(super) fn plan(
    sources: &sources::ProjectSources,
    resolved: &[resolve::ResolvedWeapon],
    plugs: &[ResolvedCustomPlug],
) -> AuthoringResult<Option<(ReplacementSpec, Vec<Vec<u16>>)>> {
    let offered = plugs
        .iter()
        .enumerate()
        .filter_map(|(position, plug)| Some((position, plug.offered_like?)))
        .collect::<Vec<_>>();
    if offered.is_empty() {
        return Ok(None);
    }
    let offers = offered
        .iter()
        .map(|&(position, like)| {
            Ok((
                u16::try_from(like)
                    .map_err(|_| invalid("An offered stock plug index does not fit 16 bits"))?,
                plugs[position].authored_item_index,
            ))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let root = read_tag(
        &sources.manager,
        globals_child_tag(&sources.globals_data, 0)?,
        "investment root",
    )?;
    let tag = root_child_tag(&root, ROOT_REUSABLE_PLUG_SET_TABLE_SLOT)?;
    let table = read_tag(&sources.manager, tag, "plug set table")?;
    let (payload, joined) = offer(&table, &offers)?;
    for donor in resolved {
        let rolled = donor
            .weapon
            .overrides
            .socket_columns
            .iter()
            .flatten()
            .filter_map(|column| column.randomized_plug_set_index)
            .find(|set| joined.iter().flatten().any(|joined| joined == set));
        if let Some(set) = rolled {
            return Err(donor.weapon.in_recipe(invalid(format!(
                "A socket rolls from plug set {set}, which a perk set to Offer Everywhere joins"
            ))));
        }
    }
    let mut sets = vec![Vec::new(); plugs.len()];
    for ((position, like), joined) in offered.into_iter().zip(joined) {
        if joined.is_empty() {
            let plug = &plugs[position];
            let hash = read_u32(
                &sources.stock_item_strings,
                sources.string_rows + like * ITEM_ROW_SIZE,
            )?;
            return Err(invalid(format!(
                "{} is set to Offer Everywhere, but no socket offers its type plug 0x{hash:08X}. Choose another Type or turn Offer Everywhere off.",
                plug.authored_name.as_deref().unwrap_or("A private perk")
            )));
        }
        sets[position] = joined;
    }
    Ok(Some((ReplacementSpec { tag, payload }, sets)))
}
