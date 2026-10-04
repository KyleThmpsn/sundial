//! Rebuilds the event connections of component owners moved between weapon entities.
//!
//! A moved owner keeps its donor's connections. Each endpoint in another owner is re-pointed at
//! the owner that backs the same bindings in the authored entity, and at the object in it that
//! plays the same part there. Anything without such a counterpart is refused, never guessed.

use std::{cell::RefCell, rc::Rc};

use super::owner::{EVENT_ROW_CLASS, EVENT_ROW_SIZE, EVENTS_DESCRIPTOR, event_rows};
use super::*;

/// A second event array whose rows carry a name. Stock weapons use it for a connection into the
/// skeleton owner.
const NAMED_EVENTS_DESCRIPTOR: usize = 0x30;
const SOURCE: usize = 0x08;
const DESTINATION: usize = 0x28;
/// Within an endpoint: owner tag, object class, object offset, then the owner's component index.
const ENDPOINT_CLASS: usize = 0x04;
const ENDPOINT_OFFSET: usize = 0x08;
const ENDPOINT_COMPONENT: usize = 0x10;
/// The Channel Interpolation variable that a connection into its receiver writes.
const RECEIVER_VARIABLE: usize = 0x40;
const NULL_TAG: u32 = u32::MAX;
const NULL_COMPONENT: u64 = 0xFFFF;
/// A destination outside the entity. In 5,191 stock entities all 820 such endpoints carry this
/// index, every one is a destination and names `80C70C06`, which no entity lists as a component.
const EXTERNAL_COMPONENT: u64 = 0x2_0000;
const ARRAY_MARKER: u32 = 0x8080_9FBD;

/// Channel Interpolation. Emitters elsewhere write variables of its receiver, and each channel
/// entry feeds one reader elsewhere.
const CHANNEL_EMITTER: u32 = 0x8080_9789;
const CHANNEL_RECEIVER: u32 = 0x8080_97C2;
const CHANNEL_ENTRY: u32 = 0x8080_97A8;
const CHANNEL_ENTRY_SIZE: usize = 0x20;
const CHANNEL_VARIABLE: u32 = 0x8080_97A1;
const CHANNEL_VARIABLE_SIZE: usize = 0x70;
const CHANNEL_NAME: u32 = 0x8080_97A0;
const CHANNEL_NAME_SIZE: usize = 0x0C;
/// Arrays the receiver object describes: its channel entries, its variables and the table that
/// names each channel by index.
const RECEIVER_ENTRIES: usize = 0x68;
const RECEIVER_VARIABLES: usize = 0x78;
const RECEIVER_CHANNEL_NAMES: usize = 0xB8;
/// The hash of the empty name, which a channel without a namespace carries in its place.
const EMPTY_NAME: u32 = 0x811C_9DC5;

/// One component owner replaced by a donor's.
pub(super) struct Move<'a> {
    pub(super) target_owner: u32,
    pub(super) donor_owner: u32,
    pub(super) donor: &'a [u8],
}

type Row = [u8; EVENT_ROW_SIZE];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Object {
    owner: u32,
    class: u32,
    offset: u64,
}

/// Replaces the event table of `authored`, which is `target` with the moved owner partitions
/// already copied in, by one that connects the moved owners the way their donors do.
pub(super) fn rewire(
    target: &[u8],
    authored: &mut Vec<u8>,
    moves: &[Move<'_>],
    read_owner: &dyn Fn(u32) -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    let plan = Plan::new(target, authored, moves, read_owner)?;
    let rows = plan.rows()?;
    write_events(authored, &rows)?;
    plan.rewire_named(authored)?;
    check_events(authored)
}

/// What an extension does with a donor connection that has no counterpart in this weapon.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Unmatched {
    /// Refuse the composition.
    Refuse,
    /// Empty a composed owner's own optional outgoing connection, and drop an event that a source
    /// owner left out of the composition sends. A connection the client requires is still refused.
    /// Every change is reported.
    Detach,
}

/// Additional owners keep their native connections, resolved against the complete composite.
/// Returns the connections `Unmatched::Detach` emptied or dropped.
pub(super) fn extend(
    target: &[u8],
    authored: &mut Vec<u8>,
    source: &[u8],
    owners: &BTreeSet<u32>,
    read_owner: &dyn Fn(u32) -> Result<Vec<u8>, String>,
    unmatched: Unmatched,
) -> Result<Vec<String>, String> {
    let mut plan = Plan::new(target, authored, &[], read_owner)?;
    plan.unmatched = unmatched;
    plan.donors.push(Donor {
        side: Side::new(source)?,
        moved: owners.clone(),
    });
    let rows = plan.rows()?;
    write_events(authored, &rows)?;
    plan.rewire_named(authored)?;
    check_events(authored)?;
    Ok(plan.detached.into_inner())
}

/// Drops the events that removed owners send and renumbers the rest to the components that
/// remain. `components` maps each original component index to its new one, or to none for a
/// removed owner. An event a remaining owner sends into a removed one is refused, since nothing
/// would receive it, and so is an entity with named events, which a removal does not renumber.
/// So is a removed owner that is the only sender into a remaining Channel Interpolation
/// variable: the client faulted setting up Armor of the Colossus's overshield when its gear model,
/// the only sender into four of its receiver's variables, was left out.
/// A destination outside the entity keeps its endpoint.
pub(super) fn remove(entity: &mut Vec<u8>, components: &[Option<usize>]) -> Result<(), String> {
    if !named_rows(entity)?.is_empty() {
        return Err("Removing components from an entity with named events is not supported".into());
    }
    let events = event_rows(entity)?;
    if events.is_empty() {
        return check_events(entity);
    }
    let owners = component_owners(entity)?;
    // Whether an endpoint's original component index names a removed owner.
    let removed = |row: &[u8], at: usize| -> Result<bool, String> {
        if read_u32(row, at)? == NULL_TAG {
            return Ok(false);
        }
        let index = usize::try_from(read_u64(row, at + ENDPOINT_COMPONENT)?)
            .map_err(|_| "Event component overflow")?;
        Ok(matches!(components.get(index), Some(None)))
    };
    // Each remaining receiver variable, by owner, receiver object and variable, with whether a
    // remaining owner still writes it.
    let mut variables = BTreeMap::<(u32, u64, u32), bool>::new();
    for &at in &events {
        let row = crate::package_payload::bytes_at::<EVENT_ROW_SIZE>(entity, at)?;
        if read_u32(&row, DESTINATION)? == NULL_TAG
            || read_u32(&row, DESTINATION + ENDPOINT_CLASS)? != CHANNEL_RECEIVER
            || removed(&row, DESTINATION)?
        {
            continue;
        }
        let key = (
            read_u32(&row, DESTINATION)?,
            read_u64(&row, DESTINATION + ENDPOINT_OFFSET)?,
            read_u32(&row, RECEIVER_VARIABLE)?,
        );
        *variables.entry(key).or_default() |= !removed(&row, SOURCE)?;
    }
    if let Some(((owner, _, variable), _)) = variables.iter().find(|(_, written)| !**written) {
        return Err(format!(
            "A removed component is the only sender into variable {variable} of owner 0x{owner:08X}"
        ));
    }
    let mut rows = Vec::new();
    for at in events {
        let mut row = crate::package_payload::bytes_at::<EVENT_ROW_SIZE>(entity, at)?;
        let [source, destination] = [SOURCE, DESTINATION].map(|at| -> Result<Remaining, String> {
            if read_u32(&row, at)? == NULL_TAG {
                return Ok(Remaining::Null);
            }
            let component = read_u64(&row, at + ENDPOINT_COMPONENT)?;
            if at == DESTINATION && external(&owners, read_u32(&row, at)?, component) {
                return Ok(Remaining::External);
            }
            let index = usize::try_from(component).map_err(|_| "Event component overflow")?;
            match components.get(index) {
                Some(Some(new)) => Ok(Remaining::Kept(*new)),
                Some(None) => Ok(Remaining::Removed),
                None => Err(format!(
                    "Event component {index} is outside the component list"
                )),
            }
        });
        match (source?, destination?) {
            (Remaining::Removed, _) => continue,
            (_, Remaining::Removed) => {
                return Err(format!(
                    "Owner 0x{:08X} sends an event into a removed component",
                    read_u32(&row, SOURCE)?
                ));
            }
            (source, destination) => {
                for (at, endpoint) in [(SOURCE, source), (DESTINATION, destination)] {
                    if let Remaining::Kept(index) = endpoint {
                        let index = u64::try_from(index).map_err(|_| "Event component overflow")?;
                        write_u64(&mut row, at + ENDPOINT_COMPONENT, index)?;
                    }
                }
            }
        }
        rows.push(row);
    }
    write_events(entity, &rows)?;
    check_events(entity)
}

/// One side of an event row while components are removed.
enum Remaining {
    Null,
    Kept(usize),
    Removed,
    /// A destination outside the entity, which removing components does not renumber.
    External,
}

/// Whether an endpoint names the receiver outside the entity rather than one of its components.
fn external(components: &[u32], owner: u32, component: u64) -> bool {
    component == EXTERNAL_COMPONENT && !components.contains(&owner)
}

/// One entity's bindings and event graph, as evidence for matching.
struct Side<'a> {
    rows: Vec<Row>,
    aliases: Vec<ComponentAlias>,
    /// Every object the event graph addresses, which proves the owner carries it.
    addressed: BTreeSet<Object>,
    entity: &'a [u8],
}

impl<'a> Side<'a> {
    fn new(entity: &'a [u8]) -> Result<Self, String> {
        let rows = event_rows(entity)?
            .into_iter()
            .map(|row| crate::package_payload::bytes_at::<EVENT_ROW_SIZE>(entity, row))
            .collect::<Result<Vec<_>, _>>()?;
        let mut addressed = BTreeSet::new();
        for row in &rows {
            for at in [SOURCE, DESTINATION] {
                let object = object(row, at)?;
                if object.owner != NULL_TAG {
                    addressed.insert(object);
                }
            }
        }
        Ok(Self {
            rows,
            aliases: weapon_component_aliases(entity)?,
            addressed,
            entity,
        })
    }

    fn identities(&self, owner: u32) -> BTreeSet<(u32, usize)> {
        self.aliases
            .iter()
            .filter(|alias| alias.owner_tag == owner)
            .map(|alias| (alias.binding_hash, alias.resource_index))
            .collect()
    }

    fn resources(&self, binding: u32) -> usize {
        resource_count(&self.aliases, binding)
    }

    fn addressed_of_class(&self, owner: u32, class: u32) -> Vec<Object> {
        self.addressed
            .iter()
            .filter(|object| object.owner == owner && object.class == class)
            .copied()
            .collect()
    }

    /// The owner's Channel Interpolation receiver, the one object of its class the graph addresses.
    fn receiver(&self, owner: u32) -> Result<u64, String> {
        match self.addressed_of_class(owner, CHANNEL_RECEIVER)[..] {
            [receiver] => Ok(receiver.offset),
            _ => Err(format!(
                "Owner 0x{owner:08X} has no single Channel Interpolation receiver"
            )),
        }
    }
}

/// How many resources a binding selects.
fn resource_count(aliases: &[ComponentAlias], binding: u32) -> usize {
    aliases
        .iter()
        .filter(|alias| alias.binding_hash == binding)
        .count()
}

fn component_owners(entity: &[u8]) -> Result<Vec<u32>, String> {
    let components = native_array(entity, ENTITY_COMPONENTS_DESCRIPTOR)?;
    (0..components.count)
        .map(|index| {
            read_u32(
                entity,
                components.rows + index * WEAPON_ENTITY_COMPONENT_ROW_SIZE,
            )
        })
        .collect()
}

fn object(row: &Row, at: usize) -> Result<Object, String> {
    Ok(Object {
        owner: read_u32(row, at)?,
        class: read_u32(row, at + ENDPOINT_CLASS)?,
        offset: read_u64(row, at + ENDPOINT_OFFSET)?,
    })
}

fn set_endpoint(row: &mut Row, at: usize, object: Object, component: u64) -> Result<(), String> {
    write_u32(row, at, object.owner)?;
    write_u32(row, at + ENDPOINT_CLASS, object.class)?;
    write_u64(row, at + ENDPOINT_OFFSET, object.offset)?;
    write_u64(row, at + ENDPOINT_COMPONENT, component)
}

/// Owner payloads by tag, and each Channel Interpolation owner's names, read once.
struct Owners<'r> {
    read: &'r dyn Fn(u32) -> Result<Vec<u8>, String>,
    payloads: RefCell<BTreeMap<u32, Rc<[u8]>>>,
    channels: RefCell<BTreeMap<u32, Rc<Channels>>>,
}

impl Owners<'_> {
    fn payload(&self, owner: u32) -> Result<Rc<[u8]>, String> {
        if let Some(payload) = self.payloads.borrow().get(&owner) {
            return Ok(Rc::clone(payload));
        }
        let payload: Rc<[u8]> = (self.read)(owner)
            .map_err(|error| format!("Component owner 0x{owner:08X} could not be read: {error}"))?
            .into();
        self.payloads
            .borrow_mut()
            .insert(owner, Rc::clone(&payload));
        Ok(payload)
    }

    fn channels(&self, side: &Side<'_>, owner: u32) -> Result<Rc<Channels>, String> {
        if let Some(channels) = self.channels.borrow().get(&owner) {
            return Ok(Rc::clone(channels));
        }
        let receiver = side.receiver(owner)?;
        let channels = Rc::new(
            Channels::read(&self.payload(owner)?, receiver)
                .map_err(|error| format!("Channel owner 0x{owner:08X}: {error}"))?,
        );
        self.channels
            .borrow_mut()
            .insert(owner, Rc::clone(&channels));
        Ok(channels)
    }
}

/// The names Channel Interpolation gives its channels and variables.
struct Channels {
    /// Each channel entry's offset, by channel index.
    entries: Vec<u64>,
    /// Each channel's namespace and name, by channel index.
    names: Vec<(u32, u32)>,
    /// Each variable's name, by variable index. The channels are the first variables.
    variables: Vec<u32>,
}

impl Channels {
    fn read(payload: &[u8], receiver: u64) -> Result<Self, String> {
        let receiver = usize::try_from(receiver).map_err(|_| "Receiver offset is too large")?;
        let (entry_count, entry_rows) = typed_rows(
            payload,
            receiver + RECEIVER_ENTRIES,
            CHANNEL_ENTRY,
            CHANNEL_ENTRY_SIZE,
        )?;
        let (variable_count, variable_rows) = typed_rows(
            payload,
            receiver + RECEIVER_VARIABLES,
            CHANNEL_VARIABLE,
            CHANNEL_VARIABLE_SIZE,
        )?;
        let variables = (0..variable_count)
            .map(|index| read_u32(payload, variable_rows + index * CHANNEL_VARIABLE_SIZE))
            .collect::<Result<Vec<_>, _>>()?;
        let names = channel_names(payload, receiver, entry_count)?;
        if variables.len() < names.len() {
            return Err("it has fewer variables than channels".into());
        }
        // A channel's value is the variable of the same index. Names without a namespace are
        // stored verbatim in both places, which proves the two tables are read the same way.
        for (index, &(namespace, name)) in names.iter().enumerate() {
            if namespace == EMPTY_NAME && variables[index] != name {
                return Err(format!(
                    "channel {index} and its variable have different names"
                ));
            }
        }
        let entries = (0..entry_count)
            .map(|index| (entry_rows + index * CHANNEL_ENTRY_SIZE) as u64)
            .collect();
        Ok(Self {
            entries,
            names,
            variables,
        })
    }

    fn entry_index(&self, offset: u64) -> Result<usize, String> {
        self.entries
            .iter()
            .position(|&entry| entry == offset)
            .ok_or_else(|| format!("0x{offset:X} is not one of its channel entries"))
    }

    fn channel(&self, name: (u32, u32)) -> Option<usize> {
        self.names.iter().position(|&candidate| candidate == name)
    }

    fn variable(&self, name: u32) -> Result<Option<usize>, String> {
        let mut found = self
            .variables
            .iter()
            .enumerate()
            .filter(|(_, candidate)| **candidate == name)
            .map(|(index, _)| index);
        let first = found.next();
        if found.next().is_some() {
            return Err(format!("two of its variables are named 0x{name:08X}"));
        }
        Ok(first)
    }
}

/// The table naming every channel exactly once by index.
fn channel_names(payload: &[u8], receiver: usize, count: usize) -> Result<Vec<(u32, u32)>, String> {
    let (rows_count, rows) = typed_rows(
        payload,
        receiver + RECEIVER_CHANNEL_NAMES,
        CHANNEL_NAME,
        CHANNEL_NAME_SIZE,
    )?;
    if rows_count != count {
        return Err("its channel name table does not name every channel".into());
    }
    let mut names = vec![None; count];
    for index in 0..rows_count {
        let row = rows + index * CHANNEL_NAME_SIZE;
        let channel = usize::try_from(read_u32(payload, row + 8)?)
            .map_err(|_| "Channel index does not fit usize")?;
        let slot = names
            .get_mut(channel)
            .ok_or("its channel name table names a channel it does not have")?;
        if slot
            .replace((read_u32(payload, row)?, read_u32(payload, row + 4)?))
            .is_some()
        {
            return Err("its channel name table names one channel twice".into());
        }
    }
    names
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "its channel name table leaves a channel unnamed".into())
}

/// The rows of the native array at `descriptor`, after checking its class and extent.
fn typed_rows(
    payload: &[u8],
    descriptor: usize,
    class: u32,
    row_size: usize,
) -> Result<(usize, usize), String> {
    let array = native_array(payload, descriptor)?;
    if array.omitted || array.row_class != class {
        return Err(format!(
            "the array at 0x{descriptor:X} is not of class 0x{class:08X}"
        ));
    }
    checked_rows_end(array, row_size, payload.len(), "Channel Interpolation")?;
    Ok((array.count, array.rows))
}

/// The entities being combined, and what the authored entity looks like after the owner grafts.
struct Plan<'a, 'r> {
    host: Side<'a>,
    donors: Vec<Donor<'a>>,
    /// Target owners whose connections are replaced.
    replaced: BTreeSet<u32>,
    /// The target owner each moved owner replaces.
    replacing: BTreeMap<u32, u32>,
    authored_components: Vec<u32>,
    authored_aliases: Vec<ComponentAlias>,
    owners: Owners<'r>,
    unmatched: Unmatched,
    /// The connections emptied or dropped under `Unmatched::Detach`.
    detached: RefCell<Vec<String>>,
}

/// One donor entity and the owners taken from it.
struct Donor<'a> {
    side: Side<'a>,
    moved: BTreeSet<u32>,
}

impl<'a, 'r> Plan<'a, 'r> {
    fn new(
        target: &'a [u8],
        authored: &[u8],
        moves: &[Move<'a>],
        read_owner: &'r dyn Fn(u32) -> Result<Vec<u8>, String>,
    ) -> Result<Self, String> {
        let host = Side::new(target)?;
        let mut replaced = BTreeSet::new();
        let mut replacing = BTreeMap::new();
        let mut donors = Vec::<Donor<'a>>::new();
        // An owner kept under its own tag needs no new connections: the target's own rows
        // already describe that exact payload.
        for moved in moves.iter().filter(|m| m.target_owner != m.donor_owner) {
            replaced.insert(moved.target_owner);
            replacing.insert(moved.donor_owner, moved.target_owner);
            // Callers load one entity per binding, so the same donor can arrive twice.
            let index = match donors
                .iter()
                .position(|donor| donor.side.entity == moved.donor)
            {
                Some(index) => index,
                None => {
                    donors.push(Donor {
                        side: Side::new(moved.donor)?,
                        moved: BTreeSet::new(),
                    });
                    donors.len() - 1
                }
            };
            donors[index].moved.insert(moved.donor_owner);
        }
        Ok(Self {
            host,
            donors,
            replaced,
            replacing,
            authored_components: component_owners(authored)?,
            authored_aliases: weapon_component_aliases(authored)?,
            owners: Owners {
                read: read_owner,
                payloads: RefCell::new(BTreeMap::new()),
                channels: RefCell::new(BTreeMap::new()),
            },
            unmatched: Unmatched::Refuse,
            detached: RefCell::new(Vec::new()),
        })
    }

    /// The authored event table: the target's rows that do not touch a replaced owner, then
    /// every donor row that touches a moved one, re-pointed and placed in order.
    fn rows(&self) -> Result<Vec<Row>, String> {
        let mut kept = Vec::new();
        let mut lost = BTreeSet::new();
        for row in &self.host.rows {
            let source = object(row, SOURCE)?;
            let destination = object(row, DESTINATION)?;
            if self.replaced.contains(&destination.owner)
                && !self.replaced.contains(&source.owner)
                && source.owner != NULL_TAG
            {
                lost.insert(source);
            }
            if !self.replaced.contains(&source.owner) && !self.replaced.contains(&destination.owner)
            {
                kept.push(*row);
            }
        }
        let mut added = Vec::<Row>::new();
        for donor in &self.donors {
            for row in &donor.side.rows {
                let touches = [SOURCE, DESTINATION]
                    .into_iter()
                    .map(|at| object(row, at))
                    .collect::<Result<Vec<_>, _>>()?
                    .iter()
                    .any(|object| donor.moved.contains(&object.owner));
                if touches {
                    let Some(translated) = self.translate_or_detach(donor, row)? else {
                        continue;
                    };
                    if !added.contains(&translated) {
                        added.push(translated);
                    }
                }
            }
        }
        let added = self.keep_host_sources(&kept, added)?;
        let kept = self.settle_sources(kept, &added)?;
        let added = self.reconnect_lost(&kept, added, &lost)?;
        // No stock table connects one source twice.
        let mut sources = BTreeSet::new();
        for row in kept.iter().chain(&added) {
            let source = object(row, SOURCE)?;
            if !sources.insert(source) {
                return Err(format!("{} would feed two receivers", describe(source)));
            }
        }
        order(kept, added, &self.host.rows)
    }

    /// Under `Unmatched::Detach`, a donor row whose source is this weapon's own object, which
    /// already feeds a receiver here, is dropped so this weapon keeps its own connection. So is a
    /// second donor row from one such source. Composed owners' sources are never dropped.
    fn keep_host_sources(&self, kept: &[Row], added: Vec<Row>) -> Result<Vec<Row>, String> {
        if self.unmatched != Unmatched::Detach {
            return Ok(added);
        }
        let mut busy = BTreeSet::new();
        for row in kept {
            if object(row, DESTINATION)?.owner != NULL_TAG {
                busy.insert(object(row, SOURCE)?);
            }
        }
        let mut result = Vec::with_capacity(added.len());
        for row in added {
            let source = object(&row, SOURCE)?;
            let composed = self.donors.iter().any(|d| d.moved.contains(&source.owner));
            if !composed && source.owner != NULL_TAG && !busy.insert(source) {
                self.detached.borrow_mut().push(format!(
                    "Dropped {} to {}: this weapon keeps that source's own connection",
                    describe(source),
                    describe(object(&row, DESTINATION)?)
                ));
                continue;
            }
            result.push(row);
        }
        Ok(result)
    }

    /// Drops a kept row that left a source empty when a donor row now fills it, and refuses a
    /// source that would feed two different receivers.
    fn settle_sources(&self, kept: Vec<Row>, added: &[Row]) -> Result<Vec<Row>, String> {
        let mut filled = BTreeMap::<Object, Vec<Row>>::new();
        for row in added {
            filled.entry(object(row, SOURCE)?).or_default().push(*row);
        }
        let mut settled = Vec::with_capacity(kept.len());
        for row in kept {
            let source = object(&row, SOURCE)?;
            let Some(replacements) = filled.get(&source) else {
                settled.push(row);
                continue;
            };
            if object(&row, DESTINATION)?.owner == NULL_TAG {
                continue;
            }
            if !replacements.contains(&row) {
                return Err(format!(
                    "{} would feed both this weapon's own receiver and the donor's",
                    describe(source)
                ));
            }
        }
        Ok(settled)
    }

    /// A target source whose only connection went into a replaced owner must be connected again,
    /// or left empty where the donor leaves its counterpart empty. Anything else is refused.
    fn reconnect_lost(
        &self,
        kept: &[Row],
        mut added: Vec<Row>,
        lost: &BTreeSet<Object>,
    ) -> Result<Vec<Row>, String> {
        let mut connected = BTreeSet::new();
        for row in kept.iter().chain(&added) {
            connected.insert(object(row, SOURCE)?);
        }
        for &source in lost.iter().filter(|source| !connected.contains(source)) {
            let empty = self.empty_counterpart(source)?.ok_or_else(|| {
                format!(
                    "{} would be left without its connection, and the donor has no empty counterpart for it",
                    describe(source)
                )
            })?;
            added.push(empty);
        }
        Ok(added)
    }

    /// A donor row that leaves the counterpart of `source` empty, re-pointed at `source`.
    fn empty_counterpart(&self, source: Object) -> Result<Option<Row>, String> {
        for donor in &self.donors {
            for row in &donor.side.rows {
                if object(row, DESTINATION)?.owner != NULL_TAG {
                    continue;
                }
                let donor_source = object(row, SOURCE)?;
                if donor_source.owner == NULL_TAG || donor.moved.contains(&donor_source.owner) {
                    continue;
                }
                let Ok(translated) = self.translate(donor, row) else {
                    continue;
                };
                if object(&translated, SOURCE)? == source {
                    return Ok(Some(translated));
                }
            }
        }
        Ok(None)
    }

    /// A donor row translated, or under `Unmatched::Detach` emptied when its own source was
    /// composed, and dropped when its source was left out.
    fn translate_or_detach(&self, donor: &Donor<'_>, row: &Row) -> Result<Option<Row>, String> {
        let error = match self.translate(donor, row) {
            Ok(translated) => return Ok(Some(translated)),
            Err(error) if self.unmatched == Unmatched::Detach => error,
            Err(error) => return Err(error),
        };
        let source = object(row, SOURCE)?;
        let destination = object(row, DESTINATION)?;
        if source.owner == NULL_TAG || !donor.moved.contains(&source.owner) {
            self.detached.borrow_mut().push(format!(
                "Dropped {} to {}: {error}",
                describe(source),
                describe(destination)
            ));
            return Ok(None);
        }
        // The client abandons creating the entity when a source whose byte +0x10 is zero has no
        // destination (A20150 clears its success flag, and 5946B0 then returns no handle). No stock
        // entity leaves such a source empty, so it is refused, never emptied.
        let required = self
            .owners
            .payload(source.owner)?
            .get(
                usize::try_from(source.offset)
                    .ok()
                    .and_then(|at| at.checked_add(0x10))
                    .ok_or("Event source offset overflows")?,
            )
            .copied()
            .ok_or_else(|| format!("{} lies outside its owner", describe(source)))?
            == 0;
        if required {
            return Err(format!(
                "{error}. The client requires {} to stay connected",
                describe(source)
            ));
        }
        // A composed owner keeps its own optional source, now connected to nothing.
        let mut emptied = *row;
        set_endpoint(&mut emptied, SOURCE, source, self.slot(source.owner)?)?;
        let empty = Object {
            owner: NULL_TAG,
            class: NULL_TAG,
            offset: 0,
        };
        set_endpoint(&mut emptied, DESTINATION, empty, NULL_COMPONENT)?;
        write_u64(&mut emptied, RECEIVER_VARIABLE, 0)?;
        self.detached.borrow_mut().push(format!(
            "Emptied {} to {}: {error}",
            describe(source),
            describe(destination)
        ));
        Ok(Some(emptied))
    }

    fn translate(&self, donor: &Donor<'_>, row: &Row) -> Result<Row, String> {
        let mut translated = *row;
        for at in [SOURCE, DESTINATION] {
            let original = object(row, at)?;
            if original.owner == NULL_TAG {
                if read_u64(row, at + ENDPOINT_COMPONENT)? != NULL_COMPONENT {
                    return Err("A donor event row has an empty endpoint with a component".into());
                }
                continue;
            }
            let counterpart = match self.counterpart(donor, original) {
                Ok(counterpart) => counterpart,
                // A moved owner's connection into something this weapon lacks stays empty where
                // the owner it replaces leaves the same connection empty.
                Err(_)
                    if at == DESTINATION && self.target_leaves_empty(object(row, SOURCE)?)? =>
                {
                    let empty = Object {
                        owner: NULL_TAG,
                        class: NULL_TAG,
                        offset: 0,
                    };
                    set_endpoint(&mut translated, at, empty, NULL_COMPONENT)?;
                    write_u64(&mut translated, RECEIVER_VARIABLE, 0)?;
                    return Ok(translated);
                }
                Err(error) => return Err(error),
            };
            set_endpoint(
                &mut translated,
                at,
                counterpart,
                self.slot(counterpart.owner)?,
            )?;
        }
        let original = object(row, DESTINATION)?;
        let counterpart = object(&translated, DESTINATION)?;
        if original.class == CHANNEL_RECEIVER && original.owner != counterpart.owner {
            let source = object(row, SOURCE)?;
            let variable = self.variable(donor, source, original, counterpart, row)?;
            write_u64(&mut translated, RECEIVER_VARIABLE, variable)?;
        }
        Ok(translated)
    }

    /// Re-points named events at the moved owners. A named row that touches a replaced owner
    /// needs the donor's row of the same name at the same object of the owner replacing it, and
    /// every donor named row that touches a moved owner needs its target row.
    fn rewire_named(&self, authored: &mut [u8]) -> Result<(), String> {
        let counterpart = |object: Object| {
            self.replacement(object.owner)
                .map_or(object, |(_, owner)| Object { owner, ..object })
        };
        let donor_named = self
            .donors
            .iter()
            .map(|donor| named_rows(donor.side.entity))
            .collect::<Result<Vec<_>, _>>()?;
        let mut matched = BTreeSet::new();
        for (at, row) in named_rows(self.host.entity)? {
            let owners = [object(&row, SOURCE)?, object(&row, DESTINATION)?];
            let Some((donor, _)) = owners
                .iter()
                .find_map(|object| self.replacement(object.owner))
            else {
                continue;
            };
            let mut found = None;
            for (donor_at, candidate) in &donor_named[donor] {
                if found.is_none() && same_named_row(&row, candidate, &counterpart)? {
                    found = Some(*donor_at);
                }
            }
            let donor_at = found.ok_or(
                "A named event connects to this component, and the donor has no such event",
            )?;
            matched.insert((donor, donor_at));
            let mut rewritten = row;
            for (endpoint, object) in [SOURCE, DESTINATION].into_iter().zip(owners) {
                if self.replacement(object.owner).is_some() {
                    let moved = counterpart(object);
                    set_endpoint(&mut rewritten, endpoint, moved, self.slot(moved.owner)?)?;
                }
            }
            write_array(authored, at, rewritten)?;
        }
        for (donor, rows) in donor_named.iter().enumerate() {
            for (donor_at, row) in rows {
                let touches = [object(row, SOURCE)?, object(row, DESTINATION)?]
                    .iter()
                    .any(|object| self.donors[donor].moved.contains(&object.owner));
                if touches && !matched.contains(&(donor, *donor_at)) {
                    return Err("The donor component has a named event this weapon does not".into());
                }
            }
        }
        Ok(())
    }

    /// The donor and owner that replace a target owner.
    fn replacement(&self, target: u32) -> Option<(usize, u32)> {
        let (&owner, _) = self
            .replacing
            .iter()
            .find(|(_, replaced)| **replaced == target)?;
        let donor = self
            .donors
            .iter()
            .position(|donor| donor.moved.contains(&owner))?;
        Some((donor, owner))
    }

    /// Whether the target owner that a moved source's owner replaces has the same source, and
    /// connects it to nothing.
    fn target_leaves_empty(&self, source: Object) -> Result<bool, String> {
        let Some(&target) = self.replacing.get(&source.owner) else {
            return Ok(false);
        };
        for row in &self.host.rows {
            if object(row, SOURCE)?
                == (Object {
                    owner: target,
                    ..source
                })
            {
                return Ok(object(row, DESTINATION)?.owner == NULL_TAG);
            }
        }
        Ok(false)
    }

    /// The authored component index of an owner.
    fn slot(&self, owner: u32) -> Result<u64, String> {
        self.authored_components
            .iter()
            .position(|&component| component == owner)
            .map(|index| index as u64)
            .ok_or_else(|| format!("Owner 0x{owner:08X} is not a component of the authored entity"))
    }

    /// The object in the authored entity that plays the part `object` plays in the donor.
    fn counterpart(&self, donor: &Donor<'_>, object: Object) -> Result<Object, String> {
        // The donor's moved owners and any owner the authored entity already carries keep every
        // offset.
        if donor.moved.contains(&object.owner) || self.authored_components.contains(&object.owner) {
            return Ok(object);
        }
        let owner = self.counterpart_owner(&donor.side, object.owner)?;
        let evidence = self.evidence(owner);
        if object.class == CHANNEL_ENTRY {
            return self.channel_entry(&donor.side, object, evidence, owner);
        }
        if let Some(bound) = self.bound_resource(&donor.side, object, owner)? {
            return Ok(bound);
        }
        let same_place = Object { owner, ..object };
        if evidence.addressed.contains(&same_place) {
            return Ok(same_place);
        }
        // The only object of its class on both sides, such as the Channel Interpolation receiver.
        match (
            &donor.side.addressed_of_class(object.owner, object.class)[..],
            &evidence.addressed_of_class(owner, object.class)[..],
        ) {
            ([_], [only]) => Ok(*only),
            _ => Err(format!(
                "The donor connects to {}, which has no counterpart in this weapon",
                describe(object)
            )),
        }
    }

    /// The side whose event graph describes an authored owner: its donor if it was moved.
    fn evidence(&self, owner: u32) -> &Side<'a> {
        self.donors
            .iter()
            .find(|donor| donor.moved.contains(&owner))
            .map_or(&self.host, |donor| &donor.side)
    }

    /// The authored owner that backs the bindings `owner` backs in the donor. A binding that
    /// selects one resource on both sides names its owner outright. The positions inside a span
    /// differ between weapons, so spans decide only for an owner without such a binding.
    fn counterpart_owner(&self, donor: &Side<'_>, owner: u32) -> Result<u32, String> {
        let (sole, spans): (BTreeSet<_>, BTreeSet<_>) = donor
            .identities(owner)
            .into_iter()
            .partition(|&(binding, _)| {
                donor.resources(binding) == 1
                    && resource_count(&self.authored_aliases, binding) == 1
            });
        let identities = if sole.is_empty() { spans } else { sole };
        let owners = self
            .authored_aliases
            .iter()
            .filter(|alias| identities.contains(&(alias.binding_hash, alias.resource_index)))
            .map(|alias| alias.owner_tag)
            .collect::<BTreeSet<_>>();
        match owners.into_iter().collect::<Vec<_>>()[..] {
            [counterpart] => Ok(counterpart),
            [] => Err(format!(
                "The donor connects to owner 0x{owner:08X}, whose bindings this weapon does not have"
            )),
            _ => Err(format!(
                "The donor connects to owner 0x{owner:08X}, whose bindings are split across several owners here"
            )),
        }
    }

    /// A bound resource's prefix or concrete object, followed through its binding identity. Only
    /// a binding that selects as many resources on both sides pairs its positions.
    fn bound_resource(
        &self,
        donor: &Side<'_>,
        object: Object,
        owner: u32,
    ) -> Result<Option<Object>, String> {
        let payload = self.owners.payload(object.owner)?;
        let mut identities = BTreeSet::new();
        for alias in donor.aliases.iter().filter(|alias| {
            alias.owner_tag == object.owner
                && alias.concrete_class == object.class
                && donor.resources(alias.binding_hash)
                    == resource_count(&self.authored_aliases, alias.binding_hash)
        }) {
            if alias.resource_offset == object.offset {
                identities.insert((alias.binding_hash, alias.resource_index, false));
            } else if concrete_object(&payload, alias) == Some(object.offset) {
                identities.insert((alias.binding_hash, alias.resource_index, true));
            }
        }
        if identities.is_empty() {
            return Ok(None);
        }
        let payload = self.owners.payload(owner)?;
        let mut found = BTreeSet::new();
        for alias in self
            .authored_aliases
            .iter()
            .filter(|alias| alias.owner_tag == owner)
        {
            for concrete in [false, true] {
                if !identities.contains(&(alias.binding_hash, alias.resource_index, concrete)) {
                    continue;
                }
                let offset = if concrete {
                    concrete_object(&payload, alias).ok_or_else(|| {
                        format!("A bound resource of owner 0x{owner:08X} has no concrete object")
                    })?
                } else {
                    alias.resource_offset
                };
                found.insert(Object {
                    owner,
                    class: alias.concrete_class,
                    offset,
                });
            }
        }
        match found.into_iter().collect::<Vec<_>>()[..] {
            [] => Ok(None),
            [bound] => Ok(Some(bound)),
            _ => Err(format!(
                "{} is bound to several resources in this weapon",
                describe(object)
            )),
        }
    }

    /// The target channel entry with the same channel name.
    fn channel_entry(
        &self,
        donor: &Side<'_>,
        object: Object,
        evidence: &Side<'_>,
        owner: u32,
    ) -> Result<Object, String> {
        let from = self.owners.channels(donor, object.owner)?;
        let to = self.owners.channels(evidence, owner)?;
        let name = from.names[from.entry_index(object.offset)?];
        let index = to.channel(name).ok_or_else(|| {
            format!(
                "The donor reads channel 0x{:08X}, which this weapon's Channel Interpolation does not have",
                name.1
            )
        })?;
        Ok(Object {
            owner,
            class: CHANNEL_ENTRY,
            offset: to.entries[index],
        })
    }

    /// The target variable an emitter writes, matched by name. Other sources of a receiver
    /// connection address the receiver itself and carry no variable.
    fn variable(
        &self,
        donor: &Donor<'_>,
        source: Object,
        original: Object,
        counterpart: Object,
        row: &Row,
    ) -> Result<u64, String> {
        let index = read_u64(row, RECEIVER_VARIABLE)?;
        if source.class != CHANNEL_EMITTER {
            return if index == 0 {
                Ok(0)
            } else {
                Err(format!(
                    "{} writes a Channel Interpolation variable without being an emitter",
                    describe(source)
                ))
            };
        }
        let from = self.owners.channels(&donor.side, original.owner)?;
        let to = self
            .owners
            .channels(self.evidence(counterpart.owner), counterpart.owner)?;
        let name = usize::try_from(index)
            .ok()
            .and_then(|index| from.variables.get(index))
            .copied()
            .ok_or("A donor emitter writes a variable its Channel Interpolation does not have")?;
        let target = to.variable(name)?.ok_or_else(|| {
            format!(
                "The donor writes variable 0x{name:08X}, which this weapon's Channel Interpolation does not have"
            )
        })?;
        Ok(target as u64)
    }
}

/// The concrete object a bound resource's prefix points at, when the prefix and the object are
/// both where the binding says.
fn concrete_object(payload: &[u8], alias: &ComponentAlias) -> Option<u64> {
    let prefix = usize::try_from(alias.resource_offset).ok()?;
    let concrete = read_u64(payload, prefix + 8).ok()?;
    let at = usize::try_from(concrete).ok()?;
    (read_u32(payload, prefix).ok()? == alias.owner_tag
        && read_u32(payload, at).ok()? == alias.owner_tag
        && read_u32(payload, at + 4).ok()? == alias.concrete_class)
        .then_some(concrete)
}

/// The named event rows, with their offsets.
fn named_rows(entity: &[u8]) -> Result<Vec<(usize, Row)>, String> {
    if read_u64(entity, NAMED_EVENTS_DESCRIPTOR)? == 0 {
        return Ok(Vec::new());
    }
    let named = native_array(entity, NAMED_EVENTS_DESCRIPTOR)?;
    if named.row_class != EVENT_ROW_CLASS {
        return Err("The weapon entity's named events are not event rows".into());
    }
    checked_rows_end(
        named,
        EVENT_ROW_SIZE,
        entity.len(),
        "Weapon entity named event",
    )?;
    (0..named.count)
        .map(|index| {
            let at = named.rows + index * EVENT_ROW_SIZE;
            Ok((
                at,
                crate::package_payload::bytes_at::<EVENT_ROW_SIZE>(entity, at)?,
            ))
        })
        .collect()
}

/// Whether `candidate` is `row` with every endpoint mapped by `counterpart`, ignoring component
/// indices.
fn same_named_row(
    row: &Row,
    candidate: &Row,
    counterpart: &dyn Fn(Object) -> Object,
) -> Result<bool, String> {
    for at in [SOURCE, DESTINATION] {
        if object(candidate, at)? != counterpart(object(row, at)?) {
            return Ok(false);
        }
    }
    Ok(row[..SOURCE] == candidate[..SOURCE]
        && row[SOURCE + ENDPOINT_COMPONENT + 8..DESTINATION]
            == candidate[SOURCE + ENDPOINT_COMPONENT + 8..DESTINATION]
        && row[DESTINATION + ENDPOINT_COMPONENT + 8..]
            == candidate[DESTINATION + ENDPOINT_COMPONENT + 8..])
}

/// Rows sorted by source component, as stock tables are. A moved owner's rows keep the donor's
/// order and an untouched owner's keep the target's. In an owner that gained rows, a source the
/// target already connected keeps the target's place and a new source follows by offset.
fn order(kept: Vec<Row>, added: Vec<Row>, target: &[Row]) -> Result<Vec<Row>, String> {
    let mut places = BTreeMap::new();
    for (place, row) in target.iter().enumerate() {
        places.entry(object(row, SOURCE)?).or_insert(place as u64);
    }
    let sources = |rows: &[Row]| {
        rows.iter()
            .map(|row| read_u32(row, SOURCE))
            .collect::<Result<BTreeSet<_>, _>>()
    };
    let grown = sources(&kept)?
        .intersection(&sources(&added)?)
        .copied()
        .collect::<BTreeSet<_>>();
    let mut keyed = Vec::with_capacity(kept.len() + added.len());
    for (sequence, row) in kept.into_iter().chain(added).enumerate() {
        let source = object(&row, SOURCE)?;
        let place = match places.get(&source) {
            _ if !grown.contains(&source.owner) => (0, 0),
            Some(&place) => (0, place),
            None => (1, source.offset),
        };
        keyed.push((
            (
                read_u64(&row, SOURCE + ENDPOINT_COMPONENT)?,
                place,
                sequence,
            ),
            row,
        ));
    }
    keyed.sort_by_key(|(key, _)| *key);
    Ok(keyed.into_iter().map(|(_, row)| row).collect())
}

/// Replaces the event array, in place when the rows fit and appended to the payload otherwise.
fn write_events(entity: &mut Vec<u8>, rows: &[Row]) -> Result<(), String> {
    let events = native_array(entity, EVENTS_DESCRIPTOR)?;
    if events.omitted || events.row_class != EVENT_ROW_CLASS {
        return Err("The weapon entity has no event table to rewire".into());
    }
    let count = u64::try_from(rows.len()).map_err(|_| "Event count is too large")?;
    let old_end = checked_rows_end(events, EVENT_ROW_SIZE, entity.len(), "Weapon entity event")?;
    // Cleared either way, so no stale owner tag stays in the payload.
    entity[events.rows..old_end].fill(0);
    let header = if rows.len() <= events.count {
        events.header
    } else {
        // Out of line, like any grown native array: zero padding, the array marker, a 16-byte
        // aligned header. Every existing offset stays where it was.
        entity.resize((entity.len() + 8).next_multiple_of(16), 0);
        let header = entity.len();
        write_u32(entity, header - 4, ARRAY_MARKER)?;
        entity.resize(header + 16 + rows.len() * EVENT_ROW_SIZE, 0);
        write_u32(entity, header + 8, EVENT_ROW_CLASS)?;
        write_relative_pointer(entity, EVENTS_DESCRIPTOR + 8, header)?;
        header
    };
    write_u64(entity, EVENTS_DESCRIPTOR, count)?;
    write_u64(entity, header, count)?;
    for (index, row) in rows.iter().enumerate() {
        write_array(entity, header + 16 + index * EVENT_ROW_SIZE, *row)?;
    }
    let size = u64::try_from(entity.len()).map_err(|_| "Weapon entity is too large")?;
    write_u64(entity, FILE_SIZE_OFFSET, size)
}

/// Every endpoint names the owner at its component index, and rows follow component order.
fn check_events(entity: &[u8]) -> Result<(), String> {
    validate_weapon_entity(entity)?;
    let components = component_owners(entity)?;
    let mut previous = 0;
    for row in event_rows(entity)? {
        for at in [SOURCE, DESTINATION] {
            let owner = read_u32(entity, row + at)?;
            let component = read_u64(entity, row + at + ENDPOINT_COMPONENT)?;
            let valid = if owner == NULL_TAG {
                component == NULL_COMPONENT
            } else {
                usize::try_from(component)
                    .ok()
                    .and_then(|index| components.get(index))
                    == Some(&owner)
                    || at == DESTINATION && external(&components, owner, component)
            };
            if !valid {
                return Err("A rewired event row names an owner outside its component".into());
            }
        }
        let source = read_u64(entity, row + SOURCE + ENDPOINT_COMPONENT)?;
        if source < previous {
            return Err("Rewired event rows are out of component order".into());
        }
        previous = source;
    }
    // An empty named endpoint carries more than a component index, so only named owners are
    // checked there.
    for (_, row) in named_rows(entity)? {
        for at in [SOURCE, DESTINATION] {
            let owner = read_u32(&row, at)?;
            let component = read_u64(&row, at + ENDPOINT_COMPONENT)?;
            let valid = owner == NULL_TAG
                || usize::try_from(component)
                    .ok()
                    .and_then(|index| components.get(index))
                    == Some(&owner);
            if !valid {
                return Err("A rewired named event names an owner outside its component".into());
            }
        }
    }
    Ok(())
}

fn describe(object: Object) -> String {
    format!(
        "Class 0x{:08X} at 0x{:X} in owner 0x{:08X}",
        object.class, object.offset, object.owner
    )
}

#[cfg(test)]
mod tests;
