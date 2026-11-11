//! The source effect's first-person systems, which stop when a zoomed weapon's body discards.
//!
//! Reach chooses a particle system's pass in `c_particle_system::calculate_pass`, `82B1E618` in
//! the 2011-07-07 `halo3_tag_play` build. When the effect is in first person for the viewing
//! player and `player_control_get_zoom_level` is not -1, the system draws only when its
//! definition flags at `+0x24` set bit 8, Render When Zoomed. This holds for every camera mode
//! that draws in first person. A Reach weapon zooms through its own zoom levels or the player's
//! binoculars, and a converted one zooms by aiming, so its first-person systems play under a
//! native scalar gate on aim visibility. The gate covers the flagged systems too. In MCC U13 they
//! are every casing system and the plasma glows of the needle rifle, focus rifle, plasma rifle and
//! plasma repeater, all emitted at markers on the body, which no longer draws at full aim. Reach
//! keeps them while zoomed, but in the converted view they appear beside the scope or binocular
//! overlay without their weapon.
//!
//! The gate is a range control (`808093CD`, flags `0x10`) with its repeat byte clear, as the
//! per-shot stock firing flows `80EF09D0` and `80EF09D1` gate their sounds, so a shot fired while
//! not zoomed plays its systems once. Stock flows `80C69DE3` and `80EF020B` wrap a muzzle particle
//! event in the repeating form. The gate holds below 0.99, where a zoomed weapon's body discards
//! and its source HUD scope appears. It reads `79FD498A`, which the slot
//! entity's channel bank inherits as parent `43A361CE` in namespace `5F6317D5`, the row stock
//! optic banks and the converted body read. Stock firing flows `80F7B073` and `81A6B2AC` gate
//! range controls on `5F6317D5` rows of their slot banks in the same way. The private bank keeps
//! every stock declaration ordinal. The flow gains one scalar input after its stock inputs, and
//! its entity gains one connection from the bank's vector provider (`808097C2`, bank schema
//! `+0x60`) to it. Growing the input arrays moves them to the end of the flow, so the entity's
//! connections into the stock inputs follow them.
use super::*;
use crate::{
    presentation::{append, put},
    tiger::{channel::inherited, instance},
};

/// MCC Reach U13 particle system flag 8, Render When Zoomed.
pub(super) const RENDER_WHEN_ZOOMED: u64 = 1 << 8;
/// Aim visibility from which a zoomed weapon draws its scope in place of itself.
const ZOOMED: f32 = 0.99;
const INSTANCE: u32 = 0x8080_9788;
const DEFINITION: u32 = 0x8080_9789;
const BANK: u32 = 0x8080_979f;
const PROVIDER: u32 = 0x8080_97c2;
const CONNECTION: u32 = 0x8080_9bc9;
/// The flow allocation row that sizes its scalar input instances.
const INPUTS: u32 = 0x8637_4c3c;

/// The scalar input added after the flow's stock inputs.
pub(super) struct Input {
    pub ordinal: u32,
    /// Its definition row, the consumer its connection names.
    definition: usize,
    /// Each stock definition row's new place.
    moved: BTreeMap<usize, usize>,
}

/// Appends a scalar input named `name` after the flow's stock inputs, as a copy of its first
/// input in that input's unbound state, and grows the allocation row that sizes them.
pub(super) fn input(p: &mut Payload, allocation: &mut Payload, name: u32) -> Result<Input> {
    let root = p.pointer(16)?;
    let definition = p.pointer(24)?;
    let owner = p.u32(root)?;
    let instances = p.array(root + 0xd0, 96, Some(INSTANCE))?;
    let definitions = p.array(definition + 0x188, 40, Some(DEFINITION))?;
    ensure!(
        !instances.is_empty() && instances.len() == definitions.len(),
        "Native firing flow inputs differ"
    );
    for (&i, &d) in instances.iter().zip(&definitions) {
        ensure!(
            p.u32(i)? == owner
                && p.u32(i + 4)? == DEFINITION
                && p.u64(i + 8)? == d as u64
                && p.pointer(i + 16)? == root
                && p.u32(d)? == owner
                && p.u32(d + 4)? == INSTANCE
                && p.u64(d + 8)? == i as u64,
            "Native firing flow input pair differs"
        );
    }
    let count = instances.len();
    let rows = instances
        .iter()
        .chain(&instances[..1])
        .flat_map(|&i| p.0[i..i + 96].to_vec())
        .collect::<Vec<_>>();
    let links = definitions
        .iter()
        .chain(&definitions[..1])
        .flat_map(|&d| p.0[d..d + 40].to_vec())
        .collect::<Vec<_>>();
    append(&mut p.0, root + 0xd0, u64::from(INSTANCE), &rows, 96)?;
    append(
        &mut p.0,
        definition + 0x188,
        u64::from(DEFINITION),
        &links,
        40,
    )?;
    let moved_instances = p.array(root + 0xd0, 96, Some(INSTANCE))?;
    let moved_definitions = p.array(definition + 0x188, 40, Some(DEFINITION))?;
    for (&i, &d) in moved_instances.iter().zip(&moved_definitions) {
        put(&mut p.0, i + 8, &(d as u64).to_le_bytes())?;
        sequence::relative(p, i + 16, root)?;
        put(&mut p.0, d + 8, &(i as u64).to_le_bytes())?;
    }
    put(
        &mut p.0,
        moved_definitions[count] + 0x20,
        &name.to_le_bytes(),
    )?;
    let selected = allocation
        .array(0x20, 40, Some(0x8080_8852))?
        .into_iter()
        .filter(|&row| allocation.u32(row).ok() == Some(INPUTS))
        .collect::<Vec<_>>();
    let [row] = selected[..] else {
        bail!("Native firing flow input allocation is absent or ambiguous");
    };
    ensure!(
        allocation.u32(row + 16)? == INSTANCE && allocation.u32(row + 20)? as usize == count,
        "Native firing flow input allocation differs"
    );
    put(
        &mut allocation.0,
        row + 20,
        &u32::try_from(count + 1)?.to_le_bytes(),
    )?;
    Ok(Input {
        ordinal: u32::try_from(count)?,
        definition: moved_definitions[count],
        moved: definitions.into_iter().zip(moved_definitions).collect(),
    })
}

/// The control that plays `children` only while aim visibility, `input`, stays below
/// [`ZOOMED`].
pub(super) fn gate(input: u32, children: Vec<Node>) -> Node {
    Node {
        name: hash("reach/firing/unzoomed"),
        timing: Timing {
            flags: 0x10,
            ..Timing::default()
        },
        kind: Kind::RangeOnce {
            input,
            bounds: [-f32::MAX, ZOOMED],
            children,
        },
    }
}

/// The slot entity's channel bank and the declaration its vector provider exports aim visibility
/// at. A bank that does not declare it takes a private copy inheriting it, with a private
/// allocation and its mutable arrays sealed into the runtime span.
pub(super) struct Bank {
    pub tag: u32,
    schema: usize,
    pub channel: u32,
    pub private: Option<(Payload, u32, Payload)>,
}

/// The declaration ordinal of aim visibility in `bank`, the channel its vector provider exports
/// it at. The inherited lookup names the parent `43A361CE`, so the local name is found among the
/// declarations.
fn declared(bank: &Payload) -> Result<Option<u32>> {
    let schema = bank.pointer(24)?;
    let rows = bank.array(schema + 0xd8, 112, Some(0x8080_97a1))?;
    for (ordinal, row) in rows.into_iter().enumerate() {
        if bank.u32(row)? == inherited::AIM_VISIBILITY {
            return Ok(Some(u32::try_from(ordinal)?));
        }
    }
    Ok(None)
}

pub(super) fn bank(native: &mut Reader, slot: &Payload) -> Result<Bank> {
    let mut banks = Vec::new();
    for row in slot.array(16, 12, Some(0x8080_9c04))? {
        let tag = slot.u32(row)?;
        let owner = native.tag(tag, Some(0x8080_9c36))?;
        let instance = owner.pointer(16)?;
        if instance >= 4 && owner.u32(instance - 4)? == BANK {
            banks.push((tag, (*owner).clone()));
        }
    }
    ensure!(
        banks.len() == 1,
        "The native firing entity has no single channel bank"
    );
    let (tag, stock) = banks.remove(0);
    let schema = stock.pointer(24)?;
    if let Some(channel) = declared(&stock)? {
        return Ok(Bank {
            tag,
            schema,
            channel,
            private: None,
        });
    }
    let allocation_tag = stock.u32(0x44)?;
    let allocation = native.tag(allocation_tag, None)?;
    let (mut private, allocation) =
        inherited::extend(&stock, &allocation, &[inherited::AIM_VISIBILITY])?;
    let channel =
        declared(&private)?.context("The private firing bank does not declare aim visibility")?;
    let layout = instance::LAYOUTS
        .iter()
        .find(|layout| layout.symbol == "object-channels")
        .context("Channel bank instance layout")?;
    instance::seal(&mut private, &mut Vec::new(), layout)?;
    Ok(Bank {
        tag,
        schema,
        channel,
        private: Some((private, allocation_tag, allocation)),
    })
}

/// The slot entity's connection rows before and after the gate: the stock rows with each
/// consumer of a moved input following it, then a connection from the bank's vector provider to
/// the added input, copied from a stock input's connection to that provider.
pub(super) fn connections(
    slot: &Payload,
    owner: u32,
    input: &Input,
    bank: &Bank,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let consumed = |row: usize| -> Result<Option<usize>> {
        if slot.u32(row + 8)? != owner || slot.u32(row + 12)? != DEFINITION {
            return Ok(None);
        }
        Ok(Some(usize::try_from(slot.u64(row + 16)?)?))
    };
    for row in slot.array(0x30, 72, Some(CONNECTION))? {
        ensure!(
            consumed(row)?.is_none_or(|at| !input.moved.contains_key(&at)),
            "A named connection reads a native firing flow input"
        );
    }
    let rows = slot.array(0x20, 72, Some(CONNECTION))?;
    let before = rows
        .iter()
        .map(|&row| slot.bytes::<72>(row))
        .collect::<Result<Vec<_>>>()?;
    let mut after = before.clone();
    let mut template = None;
    for (&row, bytes) in rows.iter().zip(&mut after) {
        let Some(to) = consumed(row)?.and_then(|at| input.moved.get(&at)) else {
            continue;
        };
        put(bytes, 16, &(*to as u64).to_le_bytes())?;
        if slot.u32(row + 40)? == bank.tag
            && slot.u32(row + 44)? == PROVIDER
            && slot.u64(row + 48)? == (bank.schema + 0x60) as u64
        {
            template.get_or_insert(*bytes);
        }
    }
    let mut added = template.context("No native firing flow input reads its channel bank")?;
    put(&mut added, 16, &(input.definition as u64).to_le_bytes())?;
    put(&mut added, 64, &bank.channel.to_le_bytes())?;
    after.push(added);
    Ok((before.concat(), after.concat()))
}
