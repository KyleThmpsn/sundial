//! The shared plug set table: private plugs offered everywhere, and the private sets sockets roll
//! from.
//!
//! The investment root names the table at slot 51. Its root array at +8 holds 0x18-byte set rows
//! (class `0x80802DFF`): the set hash at +0 and the member array at +8. A member (0x20 bytes,
//! class `0x80802E03`) is the item index at +0, a condition array at +8 and a weight at +0x18.
//! A condition is one `0x80807D31` instruction, Flag `1` naming the unlock flag that makes the
//! member available. A member without one is always available, as Arc Resistance is in set 8.
//!
//! Sunrise lets a socket take its initial plug and the members of its embedded list, its reusable
//! set and its randomized set, read from these packages. The client presents a member only in
//! sockets that take its plug category, which is why Sunrise gives four armor mods the leg
//! category before routing them into the leg set. A private plug offered everywhere therefore
//! joins each set that offers the stock plug its classification comes from, so it carries a
//! category those sockets take. Its member copies that plug's row with no condition.
//!
//! A socket that rolls from its own choices gets a randomized set of its own, appended after the
//! stock rows, as a stock random-roll perk column keeps its curated perks in its embedded list and
//! rolls from a randomized set. A roll resolves against a randomized set by its member count, so
//! no stock set a socket might roll from is grown for it.
//!
//! A stock set that gains members moves its member array to the end of the tag, the stock
//! members' conditions pointed where they already are. Appended sets move the root row array the
//! same way, each stock row's members pointed where they already are. The authored table is
//! decoded again: every stock set must read back as it shipped, plus only its offered members,
//! and each appended set as planned.
use super::*;
use crate::tag_payload::{
    append_native_array, array_at, read_array, read_i64, read_u64, relative_target,
    synchronize_payload_size, write_i64, write_u16, write_u32, write_u64,
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
const WEIGHT: usize = 0x18;

/// One set as the table holds it: its hash and its members.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Set {
    hash: u32,
    members: Vec<Member>,
}

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

    /// A member naming item `index`, with a socket choice's weight and availability condition.
    fn authored(
        index: u16,
        weight_bits: u32,
        condition: &[WeaponNumericInstruction],
    ) -> AuthoringResult<Self> {
        let mut row = [0; MEMBER_ROW];
        write_u16(&mut row, 0, index)?;
        write_u64(&mut row, CONDITION, condition.len() as u64)?;
        write_u32(&mut row, WEIGHT, weight_bits)?;
        // An instruction row holds its opcode at +0 and its operand at +4, as a socket member's
        // condition does.
        let condition = condition
            .iter()
            .map(|instruction| {
                let mut row = [0; CONDITION_ROW];
                row[0] = instruction.opcode;
                row[4..6].copy_from_slice(&instruction.operand.to_le_bytes());
                row
            })
            .collect();
        Ok(Self { row, condition })
    }
}

fn decode(table: &[u8]) -> AuthoringResult<Vec<Set>> {
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
            let members = (0..members)
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
                .collect::<AuthoringResult<Vec<_>>>()?;
            Ok(Set {
                hash: read_u32(table, rows + set * SET_ROW)?,
                members,
            })
        })
        .collect()
}

/// The stock sets with each `(stock plug, private plug)` offer's private plug after the stock
/// members of every set that offers its stock plug, and the sets each offer joined, in offer
/// order.
fn offer(stock: &[Set], offers: &[(u16, u16)]) -> AuthoringResult<(Vec<Set>, Vec<Vec<u16>>)> {
    let mut planned = stock.to_vec();
    let mut joined = vec![Vec::new(); offers.len()];
    for (set, (shipped, grown)) in stock.iter().zip(&mut planned).enumerate() {
        for (offer, &(like, authored)) in offers.iter().enumerate() {
            let Some(template) = shipped.members.iter().find(|member| member.index() == like)
            else {
                continue;
            };
            let set_index =
                u16::try_from(set).map_err(|_| invalid("The plug set table has too many rows"))?;
            joined[offer].push(set_index);
            // The stock plug's own row, always available, naming the private plug.
            let mut row = template.row;
            write_u16(&mut row, 0, authored)?;
            row[CONDITION..CONDITION + 16].fill(0);
            grown.members.push(Member {
                row,
                condition: Vec::new(),
            });
        }
    }
    Ok((planned, joined))
}

/// Where `append_native_array` places the rows of the next array appended to `out`.
fn next_rows(out: &[u8]) -> usize {
    (out.len() + 0x23) & !0xF
}

/// Writes into `bytes` at `at` the relative pointer that a field placed at `field` needs to reach
/// `target`.
fn write_pointer(bytes: &mut [u8], at: usize, field: usize, target: usize) -> AuthoringResult<()> {
    let relative = i64::try_from(target)
        .and_then(|target| i64::try_from(field).map(|field| target - field))
        .map_err(|_| invalid("A plug set pointer does not fit 64 bits"))?;
    write_i64(bytes, at, relative)
}

/// Appends a member array for the set row whose member descriptor is at `descriptor`: the
/// `shipped` member rows at `from` in `table`, each condition pointed where it already is, then
/// `added`, whose conditions follow the array.
fn append_members(
    out: &mut Vec<u8>,
    table: &[u8],
    descriptor: usize,
    (from, shipped): (usize, usize),
    added: &[Member],
) -> AuthoringResult<()> {
    let start = next_rows(out);
    let mut rows = vec![0; (shipped + added.len()) * MEMBER_ROW];
    for index in 0..shipped {
        let at = from + index * MEMBER_ROW;
        let to = index * MEMBER_ROW;
        rows[to..to + MEMBER_ROW].copy_from_slice(&read_array::<MEMBER_ROW>(table, at)?);
        if read_i64(table, at + CONDITION + 8)? == 0 {
            continue;
        }
        let target = relative_target(table, at + CONDITION + 8)?;
        write_pointer(
            &mut rows,
            to + CONDITION + 8,
            start + to + CONDITION + 8,
            target,
        )?;
    }
    for (offset, member) in added.iter().enumerate() {
        let to = (shipped + offset) * MEMBER_ROW;
        rows[to..to + MEMBER_ROW].copy_from_slice(&member.row);
    }
    append_native_array(
        out,
        descriptor,
        ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_CLASS,
        shipped + added.len(),
        &rows,
    )?;
    if array_at(out, descriptor)?.2 != start {
        return Err(validation(
            "The plug set members did not land where they were planned",
        ));
    }
    for (offset, member) in added.iter().enumerate() {
        if member.condition.is_empty() {
            continue;
        }
        append_native_array(
            out,
            start + (shipped + offset) * MEMBER_ROW + CONDITION,
            CONDITION_EXPRESSION_ROW_CLASS,
            member.condition.len(),
            &member.condition.concat(),
        )?;
    }
    Ok(())
}

/// Moves the root row array to the end of `out` with `sets` after the stock rows, each stock row's
/// members pointed where they already are, then appends the members of each new set.
fn append_sets(out: &mut Vec<u8>, sets: &[Set]) -> AuthoringResult<()> {
    let (count, _, rows, _) = array_at(out, 8)?;
    let start = next_rows(out);
    let mut row_bytes = vec![0; (count + sets.len()) * SET_ROW];
    for index in 0..count {
        let at = rows + index * SET_ROW;
        let to = index * SET_ROW;
        row_bytes[to..to + SET_ROW].copy_from_slice(&read_array::<SET_ROW>(out, at)?);
        if read_i64(out, at + MEMBERS + 8)? == 0 {
            continue;
        }
        let target = relative_target(out, at + MEMBERS + 8)?;
        write_pointer(
            &mut row_bytes,
            to + MEMBERS + 8,
            start + to + MEMBERS + 8,
            target,
        )?;
    }
    for (offset, set) in sets.iter().enumerate() {
        write_u32(&mut row_bytes, (count + offset) * SET_ROW, set.hash)?;
    }
    append_native_array(out, 8, SET_CLASS, count + sets.len(), &row_bytes)?;
    if array_at(out, 8)?.2 != start {
        return Err(validation(
            "The plug set rows did not land where they were planned",
        ));
    }
    for (offset, set) in sets.iter().enumerate() {
        let descriptor = start + (count + offset) * SET_ROW + MEMBERS;
        append_members(out, &[], descriptor, (0, 0), &set.members)?;
    }
    Ok(())
}

/// `table` rewritten to hold `planned`: the stock sets first, each with its stock members and then
/// any it gained, and then the appended sets. Decoded again, it must read back as planned.
fn author(table: &[u8], stock: &[Set], planned: &[Set]) -> AuthoringResult<Vec<u8>> {
    let (_, _, rows, _) = array_at(table, 8)?;
    let mut out = table.to_vec();
    for (set, (shipped, members)) in stock.iter().zip(planned).enumerate() {
        if shipped.hash != members.hash
            || members.members.get(..shipped.members.len()) != Some(shipped.members.as_slice())
        {
            return Err(validation(format!(
                "Plug set {set} lost a stock member while it was planned"
            )));
        }
        let added = &members.members[shipped.members.len()..];
        if added.is_empty() {
            continue;
        }
        let descriptor = rows + set * SET_ROW + MEMBERS;
        let (count, _, from, _) = array_at(table, descriptor)?;
        append_members(&mut out, table, descriptor, (from, count), added)?;
    }
    if let Some(appended) = planned.get(stock.len()..).filter(|sets| !sets.is_empty()) {
        append_sets(&mut out, appended)?;
    }
    let out = synchronize_payload_size(out)?;
    if decode(&out)? != planned {
        return Err(validation(
            "The authored plug set table does not read back as planned",
        ));
    }
    Ok(out)
}

/// The members of the private set a column rolls from: every choice in column order, a private
/// plug's own item in place of the stock plug it was made from, with the choice's weight and
/// condition.
fn pool(
    column: &ResolvedSocketColumn,
    plugs: &[ResolvedCustomPlug],
    ordinal: usize,
    lane: usize,
) -> AuthoringResult<Vec<Member>> {
    let mut choices = column.choices.clone();
    for (plug, usage) in plugs
        .iter()
        .flat_map(|plug| plug.uses.iter().map(move |usage| (plug, usage)))
        .filter(|(_, usage)| usage.weapon_ordinal == ordinal && usage.socket_index == lane)
    {
        let source = u16::try_from(plug.source_item_index)
            .map_err(|_| invalid("Private socket-plug donor index does not fit 16 bits"))?;
        let selected = choices
            .get_mut(usage.choice_index)
            .filter(|selected| **selected == source)
            .ok_or_else(|| {
                invalid(format!(
                    "Private socket {lane} choice {} does not select source plug 0x{:08X}",
                    usage.choice_index, plug.source_item_hash
                ))
            })?;
        *selected = plug.authored_item_index;
    }
    choices
        .iter()
        .enumerate()
        .map(|(choice, &index)| {
            Member::authored(
                index,
                column
                    .choice_weight_bits
                    .get(choice)
                    .copied()
                    .unwrap_or_else(|| 1.0_f32.to_bits()),
                column
                    .choice_conditions
                    .get(choice)
                    .map_or(&[][..], Vec::as_slice),
            )
        })
        .collect()
}

/// Plans the shared plug set table. Each private plug that is offered everywhere joins every stock
/// set that offers the plug it is offered like. Each socket column that rolls from its own choices
/// gets a private randomized set of them, appended after the stock rows, and its resolved column
/// names that set. Returns the table and the sets each plug joined, in plug order, or `None` when
/// no plug is offered everywhere and no column rolls.
///
/// A roll resolves against a randomized set by its member count, so a set that gains members
/// must not be one a socket rolls from. No Shadowkeep item rolls from any of its 120 reusable
/// sets, and an authored socket that rolls from a joined set is refused. Appended sets take
/// indices after the stock ones and are never joined.
pub(super) fn plan(
    sources: &sources::ProjectSources,
    resolved: &mut [resolve::ResolvedWeapon],
    plugs: &[ResolvedCustomPlug],
) -> AuthoringResult<Option<(ReplacementSpec, Vec<Vec<u16>>)>> {
    let offered = plugs
        .iter()
        .enumerate()
        .filter_map(|(position, plug)| Some((position, plug.offered_like?)))
        .collect::<Vec<_>>();
    let rolled = resolved
        .iter()
        .enumerate()
        .flat_map(|(ordinal, donor)| {
            donor
                .socket_column_indices
                .iter()
                .enumerate()
                .filter(|(_, column)| {
                    column
                        .as_ref()
                        .is_some_and(|column| column.reusable_choices.is_some())
                })
                .map(move |(lane, _)| (ordinal, lane))
        })
        .collect::<Vec<_>>();
    if offered.is_empty() && rolled.is_empty() {
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
    let stock = decode(&table)?;
    let (mut planned, joined) = offer(&stock, &offers)?;
    for donor in resolved.iter() {
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
    // Each rolling column's set, in weapon and lane order, under a hash its weapon's namespace
    // allocates.
    let mut hashes = stock.iter().map(|set| set.hash).collect::<BTreeSet<_>>();
    for &(ordinal, lane) in &rolled {
        let donor = &resolved[ordinal];
        let column = donor.socket_column_indices[lane]
            .as_ref()
            .ok_or_else(|| validation("A rolling socket column disappeared while planned"))?;
        let members = pool(column, plugs, ordinal, lane)
            .and_then(|members| {
                let hash = allocate_identity_hash(
                    &donor.weapon.namespace,
                    &format!("socket/{lane}/randomized-plug-set"),
                    &mut hashes,
                    None,
                )?;
                Ok(Set { hash, members })
            })
            .map_err(|error| donor.weapon.in_recipe(error))?;
        planned.push(members);
    }
    let payload = author(&table, &stock, &planned)?;
    for (position, &(ordinal, lane)) in rolled.iter().enumerate() {
        let index = u16::try_from(stock.len() + position)
            .ok()
            .filter(|index| *index != u16::MAX)
            .ok_or_else(|| invalid("The plug set table has too many rows"))?;
        if let Some(Some(column)) = resolved[ordinal].socket_column_indices.get_mut(lane) {
            column.randomized_plug_set_index = Some(index);
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
