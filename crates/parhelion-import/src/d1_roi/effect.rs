//! Rise of Iron connected effect graphs as private Shadowkeep graphs.
//!
//! A source effect graph, class `80800734`, lists component owners and the connections between
//! them. Each owner family has a counterpart in a checked native graph, which supplies the
//! registration, lifecycle and allocation contracts. Source values replace the native ones, and
//! connections follow the source rows by role and channel. An owner family without a
//! translation is left out and reported, never dropped silently. Any subclass ability can use
//! these graphs, whichever controller or bank resource names them.
use super::{Source, dependency};
use crate::{
    d1_roi::{connections, native::perk::pattern::effect as pattern},
    presentation::put,
    tiger::{
        entity::{compiled, links},
        payload::Payload,
        reader::Reader,
    },
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
mod choice;
mod component;
mod emission;
mod incoming;
mod intake;
mod owner;
mod retained;
mod shared;
mod volume;
mod wiring;
pub use shared::{OMITTED, Resources, Shared, merge, occurrences};

/// Native graphs whose owners serve as templates. Each holds one owner of every family it
/// provides, connected in the roles of the matching source form. Graph `81A69987` has network,
/// status, sequence, modifier and channel owners. Graph `80BB7555` adds a named scalar receiver
/// whose channel drives the status lifetime. Victim explosion graph `8152BB5C` has network,
/// effect context, sequence, category and channel owners, and created presentation graph
/// `80BFA463` has effect context, sequence and channel owners. Graph `80B80F16` has a named
/// scalar receiver, effect context, sequence, category and channel owners, the form of the
/// impact graphs Rise of Iron projectiles spawn, such as hand cannon graph `80A6A859`. Graph
/// `80BC299F` has network, sequence, category and channel owners, its sequence reading channels
/// 1 and 0 of the bank, the form of Outbreak Prime's Virulence victim graph `80A88C33`. Of the 41
/// stock graphs with these families, the five others examined connect the bank otherwise. Graph
/// `80FF06FD` is `81A69987` without the modifier, with the same rows and stock sequence
/// `815B8E49`, the form of the Demotion and Lich Bane victim graph `80A7E1B4`. It is one of 166
/// stock graphs with network, status, sequence and channel owners. Graph `80C1934A` adds the
/// category, with the status lifetime from the sequence, sequence inputs on bank channels 0 and 1
/// and the bank getter from the status. Of the 78 stock graphs with these families, only it and
/// `80FEEBF7` have those rows. It is the form of Better Already `80A7E179` and Infusion
/// `80A6A14E`. Graph `81A699C6` has network and status owners only, with the status lifetime
/// requests at `+458` and `+7A8` unbound, as in `81A69987`. It is one of the two of 188 stock
/// network and status graphs with network `80C709E1`, and the form of Eliksni Immunity
/// `80A890C8`, whose status has no lifetime sequence. Graph `80B810D2` has network,
/// invisibility, Duration host, scalar receiver, sequence and channel owners. Its invisibility
/// owner and sequence port the shared ones of Rise of Iron invisibility graphs such as Defensive
/// Reflex `80A7E16B`, whose Duration lifetime reads a bank channel and whose bank reads the
/// Duration getter, as in this graph.
/// `80FF0758` is a perk-attached overshield: network `80C709E1`, a status, sequence `81579597`, a
/// channel bank and damage owner `8161E6F7` with 55 base vitality, attached by three stock perks.
/// `80B5839E` has a status, two sequences, incoming damage modifier `815B905A`, network
/// `80C709E1`, a scalar receiver, the stock category, a channel bank and Shadowkeep-only category
/// provider `815B9058`. Its incoming damage input reads bank channel 0, the receiver getter feeds
/// the bank's first incoming declaration and the status getter its second. It is one of 13 stock
/// graphs of that shape, the form of Rise of Iron Accelerant `80A7E16E`. `80FEEBEF` has a
/// condition volume script, network `80C70CAE` and a status whose lifetime requests are unbound,
/// one of four stock graphs of that shape and the form of Devil's Touch `80A7E16F`. A perk record
/// attaches it. `815B5BC7` has a status, two sequences, network `80C709E1`, scalar receiver
/// `80C23039`, the stock category and a channel bank, one of 17 stock graphs with those families,
/// the form of Force Multiplier's created graph `80A6A182`, whose receiver feeds the bank's two
/// incoming declarations. No examined stock graph of that shape connects its receiver, so the
/// receiver's getters follow the source rows. `8162AB5C` has a status and a sequence only, the
/// status's first lifetime request (`+458`) read from the sequence and the second (`+7A8`)
/// unbound. It is one of nine stock graphs with exactly those two owners, and the form of the
/// Rise of Iron status graphs whose status lifetime reads the shared presentation sequence
/// `80A7F1D3`, such as Perfectionist `80A7F178` and Grave Robber's nested `80A7F52E`.
/// `80BAB153` has network `80C709E1`, a status with both lifetime requests unbound and incoming
/// damage modifier `80BC49BE`, one of 45 stock graphs with a network, a status and an incoming
/// damage modifier only. It is the form of Tempered Light's `80A7F145` once its unconnected
/// Armor bank attaches separately. `80EFAE57` is the only stock graph with exactly a status,
/// network `80C70CAE`, a modifier bank and a channel bank, which reads the status getter, the form
/// of In Your Element's `80A890E1`. `80FD754E` has a status and the same bank shape, the form of
/// Bomb Disposal Shield's `80A890D0`. Both banks are private, so the source's public bank takes
/// stock bank `80B4681D` below. `80B81ACE` has network `80C709E1`, a status, sequence `80FEEB09`,
/// nested controller `80B81ACB`, a one-row modifier bank and channel bank `815B8E48`, in that
/// order, with the rows of `81A69987`. It is the only one of 19 stock graphs with a status, a
/// sequence and a nested controller that also holds the modifier, the form of Capture King's
/// `80A7E1B2` and Second Wind's `80A7E170`. Bungie's port of that talent family, perk action
/// `80BBCBD9`, attaches it, and its status keeps their Duration key `AD2C92E6`. `80C199A2` has
/// built-in network `80C70CAE`, a one-row named scalar receiver, effect context, a sequence,
/// category and a channel bank with one incoming declaration, in that order. Its sequence input
/// reads the bank and the bank's getter reads the receiver, with the category request unbound. It
/// is the only one of 35 stock graphs with exactly those families that also has those three rows,
/// that bank shape, a built-in network and a one-row receiver, the form of With a Laser Beam's
/// `80A88C36`, whose network is built-in.
const TEMPLATES: [u32; 20] = [
    0x81A69987, 0x80BB7555, 0x8152BB5C, 0x80BFA463, 0x80B80F16, 0x80BC299F, 0x80FF06FD, 0x80C1934A,
    0x81A699C6, 0x80B810D2, 0x80FF0758, 0x80B5839E, 0x80FEEBEF, 0x815B5BC7, 0x8162AB5C, 0x80BAB153,
    0x80EFAE57, 0x80FD754E, 0x80B81ACE, 0x80C199A2,
];

/// Template owners with no source counterpart, left out of the translated graph. Category provider
/// class `808094A4` feeds only the stock category's `80809BD9` request, which stock graphs such as
/// `80B83905` and `80B8070F` leave unbound, as the source category's request is.
const TEMPLATE_ONLY: [u32; 1] = [0x808094A4];

/// Templates tried only when no listed template fits, which may lack a framework family the
/// source has. `80C00348` holds the stock network `80C709E1`, a Duration host, a sequence and the
/// stock category `80C70C0E`, one of four stock graphs of that shape among the 384 that name a
/// Duration host. Wrath Conductors' target graph adds the effect context `80C70B84`, which joins
/// from victim template `8152BB5C`. `81A699C6` is Eliksni Immunity's network and status, which
/// Touch of Malice's victim graph extends with the shared Duration host.
const FALLBACK_TEMPLATES: [u32; 2] = [0x80C00348, 0x81A699C6];

/// Stock graphs that only lend shared framework owners to fallback templates. `80BC830F` names
/// Duration host `8157808D` with its one unbound request, as nine other stock graphs do.
const LENDERS: [u32; 1] = [0x80BC830F];

/// Shared framework owners, source then native. Every examined victim, swarm and presentation
/// graph in both builds names the same owner in each role, so a graph keeps the stock owner
/// rather than a private copy. Invisibility owner `80AA2EA8` is shared by ten Rise of Iron graphs
/// and `80B9FEC4` by nine stock graphs. Both hold strength 1.0, 0.25, 49, identity `14DF82B5`,
/// four recovery programs `3C 00 23 3E 00` and otherwise default settings, with their embedded
/// objects in the same order. The invisibility graphs also share presentation sequence
/// `80A7E1DF`, which stock `80FA302E` ports with the same particle names and the native forms of
/// its material, texture and presentation events. Those events have no converter, so a
/// translated copy would play none of the camouflage.
///
/// Duration host `80A7F676`, named by six Rise of Iron graphs, and `8157808D`, named by ten stock
/// graphs, hold the same flags 0, key `BD15623E`, priority 0 and indefinite constant lifetime,
/// with no connection in any graph. That key occurs in no other tag of either build.
///
/// Overshield sequence `80A7E1E8`, shared by six Rise of Iron graphs, is ported as `81579597`,
/// which sits beside the stock overshield owners. Its main input `414428AF` and auxiliary inputs
/// `414428AF` and `BBE0B10F` match by name, and it adds auxiliary input `E729F6E8`.
const FRAMEWORK: [(u32, u32); 7] = [
    (0x80AACEA6, 0x80C709E1),
    (0x80AAD54B, 0x80C70B84),
    (0x80AAD0F9, 0x80C70C0E),
    (0x80AA2EA8, 0x80B9FEC4),
    (0x80A7E1DF, 0x80FA302E),
    (0x80A7F676, 0x8157808D),
    (0x80A7E1E8, 0x81579597),
];

/// Stock channel banks for source banks whose declaration roles the template's bank lacks, with
/// the graph that registers each. Bank `80BC38B7` has an incoming getter at ordinal 0 and a
/// default-namespace inherited channel at ordinal 1, the shape of Rise of Iron hand cannon spawn
/// `80A6A859`, which inherits Explosive Rounds `5EE266FC` beside its getter. Bank `815B204C` has
/// one inherited declaration and no getter, the shape of Ice Breaker's victim bank `80A7F1FC`. It
/// is one of 319 stock banks of that shape, and six stock graphs with the victim template's owner
/// families name it, among them `80B81B42`.
/// Banks `80C001F6` and `80F9F860` have an incoming getter at ordinal 0 and two or three inherited
/// declarations, the shapes of Patience and Time's bank `80A7478A` and Defensive Reflex's
/// `80A74B21`. They are among 32 and 20 stock banks of those shapes.
/// Bank `815B8E48` has an incoming declaration at ordinal 0 and an inherited one at ordinal 1, the
/// shape of overshield bank `80AA9BB1`, in the overshield graph `81A698D0`. Bank `80B74A8D` has
/// three inherited declarations and span `00000200`, the shape of Force Multiplier's victim bank
/// `80A7F584`, and `80BC2E5B` two incoming ones and no span, the shape of Accelerant's
/// `80AAEA31`. Bank `80B74225` has two inherited declarations and span `00000100`, the shape of
/// Force Multiplier's created-graph bank `80A74B3F`. A scan of all 240,543 stock owners found them
/// among a few banks of each shape. Bank `80B4681D` (graph `80B4681E`) has one incoming declaration
/// it also exports publicly at `+F8`, the shape of source bank `80A89155` that In Your Element and
/// Bomb Disposal Shield share.
const BANKS: [(u32, u32); 9] = [
    (0x80BC2F21, 0x80BC38B7),
    (0x80B81B42, 0x815B204C),
    (0x80B82431, 0x80C001F6),
    (0x80F9F861, 0x80F9F860),
    (0x81A698D0, 0x815B8E48),
    (0x80B3A308, 0x80B74A8D),
    (0x80B8070F, 0x80BC2E5B),
    (0x80B3A133, 0x80B74225),
    (0x80B4681E, 0x80B4681D),
];

/// Stock Duration hosts for source hosts whose lifetime form the template host lacks, with the
/// graph that registers each. Host `80FD758F` has a constant lifetime and no inputs, the form
/// of Patience and Time's host `80A7478B`, beside the same invisibility owner in `80BEBFE8`.
const DURATIONS: [(u32, u32); 1] = [(0x80BEBFE8, 0x80FD758F)];

/// Stock statuses for source statuses whose lifetime receiver policy the template status lacks,
/// with the graph that registers each. `81A699C5`, Eliksni Immunity's template status, keeps
/// policy 2 with its two lifetime requests unbound.
const STATUSES: [(u32, u32); 1] = [(0x81A699C6, 0x81A699C5)];

/// Source owner definitions without a translation. Their graphs leave them out and say why.
const UNTRANSLATED: [(u32, &str); 1] = [(
    0x8080046A,
    "Owner class 80800372 has no established role or Shadowkeep counterpart",
)];

/// Owners left out only when no connection row names them, since nothing in the graph then
/// reads what they publish.
const UNCONNECTED: [(u32, &str); 1] = [(
    0x80802818,
    "State selector has no connection or reader in this graph",
)];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Family {
    Network,
    Status,
    Sequence,
    Receiver,
    Modifier,
    Channel,
    Context,
    Category,
    Invisibility,
    Duration,
    /// A damage component that hops on to its host's, as an overshield layer.
    Damage,
    /// A factor on the damage its entity takes.
    IncomingDamage,
    /// A volume that gives a graph to each selected entity inside it.
    Volume,
    /// A component that owns a child action controller for its lifetime.
    Nested,
}

impl Family {
    fn name(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::Status => "status",
            Self::Sequence => "sequence",
            Self::Receiver => "receiver",
            Self::Modifier => "modifier",
            Self::Channel => "channel",
            Self::Context => "context",
            Self::Category => "category",
            Self::Invisibility => "invisibility",
            Self::Duration => "duration",
            Self::Damage => "damage",
            Self::IncomingDamage => "incoming",
            Self::Volume => "volume",
            Self::Nested => "nested",
        }
    }
}

/// One private resource and the symbols its unresolved words take.
pub struct Pending {
    pub symbol: String,
    pub template: u32,
    pub payload: Payload,
    pub patches: Vec<(usize, String)>,
}

pub struct Graph {
    pub hud: Vec<Value>,
    /// HUD identities of statuses that need a private HUD entry, which the caller supplies.
    pub private_hud: Vec<u32>,
    pub symbol: String,
    /// Temporary owner identities, which the pending patches replace with private symbols.
    pub components: Vec<u32>,
    pub pending: Vec<Pending>,
    pub dependencies: Vec<dependency::Asset>,
    pub evidence: Value,
}

/// Every effect sequence owner of a source entity graph.
pub fn sequences(source: &Source, graph: u32) -> Result<BTreeSet<u32>> {
    let components = connections::Graph::read(&source.read(graph)?)?.components;
    Ok(occurrences(source, &components)?.into_values().collect())
}

/// The graphs the condition volume owners of a source entity graph give the entities inside them.
pub fn recipients(source: &Source, graph: u32) -> Result<BTreeSet<u32>> {
    let mut found = BTreeSet::new();
    for owner in connections::Graph::read(&source.read(graph)?)?.components {
        let payload = source.read(owner)?;
        if source_family(source, &payload).ok().flatten() == Some(Family::Volume) {
            found.insert(volume::graph(&payload)?);
        }
    }
    Ok(found)
}

/// The instance and its reciprocal definition.
pub(crate) fn pair(owner: &Payload) -> Result<(usize, usize)> {
    let instance = owner.pointer(16)?;
    Ok((instance, usize::try_from(owner.u64(instance + 8)?)?))
}

pub(crate) fn is_sequence(owner: &Payload) -> Result<bool> {
    let (instance, definition) = pair(owner)?;
    Ok(owner.u32(instance - 4)? == 0x808084D7 && owner.u32(definition - 4)? == 0x808084E9)
}

/// Whether a native sequence has a scalar input row pair to copy.
pub(crate) fn has_inputs(owner: &Payload) -> bool {
    pair(owner).is_ok_and(|(_, definition)| {
        owner
            .array(definition + 0x188, 40, Some(0x80809789))
            .is_ok_and(|rows| !rows.is_empty())
    })
}

pub(crate) fn endpoint(owner: u32, class: u32, offset: usize) -> Result<links::Endpoint> {
    Ok(links::Endpoint {
        namespace: 0x811C9DC5,
        object: Some(links::Object {
            owner,
            class,
            offset: u64::try_from(offset)?,
        }),
        // Native row lowering derives the ordinal from the owner's component position.
        selector: 0,
    })
}

/// Replace a template's typed self-references with a temporary identity. Only words followed
/// by a class and an in-payload offset are owner fields. Generated owners pair their own
/// generated root classes, which are ordinary tags rather than built-in class identities.
pub(crate) fn retarget(payload: &mut Payload, before: u32, after: u32) -> Result<()> {
    let (instance, definition) = pair(payload)?;
    let roots = [payload.u32(instance - 4)?, payload.u32(definition - 4)?];
    for at in (0..payload.0.len().saturating_sub(15)).step_by(4) {
        if payload.u32(at)? == before {
            let class = payload.u32(at + 4)?;
            ensure!(
                (class & 0xFFFF0000 == 0x80800000 || roots.contains(&class))
                    && payload.u64(at + 8)? < payload.0.len() as u64,
                "Native owner template has an untyped owner reference"
            );
            put(&mut payload.0, at, &after.to_le_bytes())?;
        }
    }
    Ok(())
}

/// Offsets of every object of `class` that `owner` names through a typed reference.
fn typed(payload: &Payload, owner: u32, class: u32) -> Result<Vec<usize>> {
    let mut found = BTreeSet::new();
    for at in (0..payload.0.len().saturating_sub(15)).step_by(4) {
        if payload.u32(at)? == owner && payload.u32(at + 4)? == class {
            found.insert(usize::try_from(payload.u64(at + 8)?)?);
        }
    }
    Ok(found.into_iter().collect())
}

/// Offsets of a source channel bank's getter requests, class `80800668`, in order. Its runtime
/// names each through a typed reference: owner, offset and class, each in eight bytes.
fn getters(payload: &Payload, owner: u32) -> Result<Vec<usize>> {
    let mut found = BTreeSet::new();
    for at in (0..payload.0.len().saturating_sub(23)).step_by(4) {
        if payload.u32(at)? == owner
            && payload.u32(at + 4)? == 0
            && payload.u32(at + 16)? == 0x80800668
            && payload.u32(at + 20)? == 0
        {
            let offset = usize::try_from(payload.u64(at + 8)?)?;
            ensure!(
                offset < payload.0.len(),
                "Source channel getter reference leaves its owner"
            );
            found.insert(offset);
        }
    }
    Ok(found.into_iter().collect())
}

/// Every aligned word naming a temporary identity, for the symbol that replaces it.
fn words(payload: &Payload, id: u32, symbol: &str) -> Result<Vec<(usize, String)>> {
    let mut patches = Vec::new();
    for at in (0..payload.0.len().saturating_sub(3)).step_by(4) {
        if payload.u32(at)? == id {
            patches.push((at, symbol.to_owned()));
        }
    }
    Ok(patches)
}

fn source_family(source: &Source, owner: &Payload) -> Result<Option<Family>> {
    let class = owner.u32(owner.pointer(24)? - 4)?;
    Ok(Some(match class {
        0x80802698 => Family::Status,
        0x80800610 => Family::Sequence,
        0x808005A8 => Family::Receiver,
        0x808020DE => Family::Modifier,
        0x808008F6 => Family::Channel,
        0x80800362 => Family::Network,
        0x80800617 => Family::Context,
        0x80802202 => Family::Category,
        0x808026FB => Family::Invisibility,
        0x808029C3 => Family::Duration,
        0x808027FA => Family::IncomingDamage,
        0x80802A94 => Family::Nested,
        _ if source.reference(class).ok() == Some(0x80800000)
            && source.read(class)?.u32(16)? == 0x80800362 =>
        {
            Family::Network
        }
        // `damage_component.bs` definitions name base `80802872`, Shadowkeep's `80804B8A`.
        _ if source.reference(class).ok() == Some(0x80800000)
            && source.read(class)?.u32(16)? == 0x80802872 =>
        {
            Family::Damage
        }
        _ if source.reference(class).ok() == Some(0x80800000)
            && volume::is_script(&source.read(class)?, 0x80802361) =>
        {
            Family::Volume
        }
        // A player status icon is a status's HUD block without the host.
        _ if source.reference(class).ok() == Some(0x80800000)
            && pattern::is_icon(&source.read(class)?) =>
        {
            Family::Status
        }
        _ if UNTRANSLATED
            .iter()
            .chain(&UNCONNECTED)
            .any(|(untranslated, _)| *untranslated == class) =>
        {
            return Ok(None);
        }
        _ => anyhow::bail!("Source effect owner class {class:08X} has no translation"),
    }))
}

fn native_family(native: &mut Reader, owner: &Payload) -> Result<Family> {
    let (instance, definition) = pair(owner)?;
    match owner.u32(definition - 4)? {
        0x808084CC => return Ok(Family::Context),
        0x80804AF6 => return Ok(Family::Category),
        class
            if native
                .tag(class, Some(0x80800000))
                .is_ok_and(|schema| volume::is_script(&schema, 0x80803C57)) =>
        {
            return Ok(Family::Volume);
        }
        _ => {}
    }
    let class = owner.u32(instance - 4)?;
    Ok(match class {
        0x80804209 => Family::Status,
        0x808084D7 => Family::Sequence,
        0x80808BF5 => Family::Receiver,
        0x80803AFD => Family::Modifier,
        0x8080979F => Family::Channel,
        0x808090E6 => Family::Network,
        0x808043DF => Family::Invisibility,
        0x80803B2F => Family::Duration,
        0x80804BEE => Family::Damage,
        0x80803F8B => Family::IncomingDamage,
        0x80803E51 => Family::Nested,
        // Generated network instances name the built-in network as their schema base.
        _ if native
            .tag(class, Some(0x80800000))
            .is_ok_and(|schema| schema.u32(16).ok() == Some(0x808090E6)) =>
        {
            Family::Network
        }
        _ => anyhow::bail!("Native effect template owner class {class:08X} has no family"),
    })
}

/// Whether a native Duration host's lifetime reads a named input, which a source host with a
/// lifetime program needs and a constant one must not have.
fn lifetime_inputs(owner: &Payload) -> Result<bool> {
    let root = owner.pointer(24)?;
    ensure!(
        owner.u32(root - 4)? == 0x80803B30,
        "Native Duration host root differs"
    );
    Ok(!owner
        .array(root + 0x48 + 0x128 + 0x40, 40, Some(0x80809789))?
        .is_empty())
}

/// A native template graph, its links and its owners by family.
struct Template {
    tag: u32,
    graph: Payload,
    links: links::Graph,
    owners: BTreeMap<Family, (usize, u32, Payload)>,
    /// Further template sequences, which every source sequence's envelope comes before, and
    /// [`TEMPLATE_ONLY`] owners. Both leave the translated graph with their rows.
    sequences: BTreeSet<u32>,
    dropped: BTreeSet<u32>,
}

impl Template {
    fn read(native: &mut Reader, tag: u32) -> Result<Self> {
        let graph = (*native.tag(tag, Some(0x80809C0F))?).clone();
        let links = links::Graph::read(&graph, false)?;
        let mut owners = BTreeMap::new();
        let mut sequences = BTreeSet::new();
        let mut dropped = BTreeSet::new();
        for (ordinal, &owner) in links.components.iter().enumerate() {
            let payload = (*native.tag(owner, Some(0x80809C36))?).clone();
            if TEMPLATE_ONLY.contains(&payload.u32(pair(&payload)?.0 - 4)?) {
                dropped.insert(owner);
                continue;
            }
            let family = native_family(native, &payload)?;
            if family == Family::Sequence && owners.contains_key(&family) {
                sequences.insert(owner);
                continue;
            }
            ensure!(
                owners.insert(family, (ordinal, owner, payload)).is_none(),
                "Native effect template repeats an owner family"
            );
        }
        Ok(Self {
            tag,
            graph,
            links,
            owners,
            sequences,
            dropped,
        })
    }

    fn family(&self, owner: u32) -> Option<Family> {
        self.owners
            .iter()
            .find(|(_, (_, tag, _))| *tag == owner)
            .map(|(family, _)| *family)
            .or_else(|| self.sequences.contains(&owner).then_some(Family::Sequence))
    }

    /// The template's one live row between these families and classes, with its position and
    /// the consumer and provider definition offsets.
    fn row(
        &self,
        consumer: (Family, u32),
        provider: (Family, u32),
    ) -> Result<(usize, usize, usize, u32)> {
        let rows = self
            .links
            .connections
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                let side = |endpoint: &links::Endpoint, (family, class): (Family, u32)| {
                    endpoint.object.is_some_and(|object| {
                        object.class == class && self.family(object.owner) == Some(family)
                    })
                };
                side(&row.consumer, consumer) && side(&row.provider, provider)
            })
            .collect::<Vec<_>>();
        let [(index, row)] = rows.as_slice() else {
            anyhow::bail!(
                "Native effect template has no single {} to {} connection",
                consumer.0.name(),
                provider.0.name()
            );
        };
        Ok((
            *index,
            offset(&row.consumer)?,
            offset(&row.provider)?,
            row.channel,
        ))
    }
}

fn offset(endpoint: &links::Endpoint) -> Result<usize> {
    Ok(usize::try_from(
        endpoint
            .object
            .context("Native effect endpoint is unbound")?
            .offset,
    )?)
}

/// A translated component and the native owner whose registrations it keeps.
struct Component {
    id: u32,
    ordinal: usize,
    family: Family,
    symbol: String,
    template_tag: u32,
    template: Payload,
    entity: Payload,
    payload: Payload,
    patches: Vec<(usize, String)>,
    allocation: Option<(u32, Payload)>,
    evidence: Value,
}

/// Translate source entity graph `tag` as the private graph `symbol`, whose owners take temporary
/// identities from `base` until their symbols replace them. Each stage takes only what earlier
/// stages return. Template reads, owner translation and rows follow source order, and the
/// pending resources follow the components.
pub fn translate(
    source: &Source,
    native: &mut Reader,
    shared: &Shared,
    envelopes: &[Payload],
    tag: u32,
    symbol: &str,
    base: u32,
) -> Result<Graph> {
    translate_without(source, native, shared, envelopes, tag, symbol, base, None)
}

/// The graph's unconnected modifier bank, when no template holds the graph's owner families with
/// it and one does without it. Tempered Light's `80A7F145` has a timed status, the framework
/// network, an incoming damage provider and a one-row Armor bank, a set no stock graph holds,
/// while 45 stock graphs hold the network, a status and an incoming damage modifier alone.
pub fn detachable_modifier(source: &Source, native: &mut Reader, tag: u32) -> Result<Option<u32>> {
    let mut intake = intake::read(source, tag)?;
    if choice::choose(native, &intake.owners, &intake.families).is_ok() {
        return Ok(None);
    }
    let Some(bank) = intake.unconnected_modifier() else {
        return Ok(None);
    };
    intake.detach(bank);
    Ok(choice::choose(native, &intake.owners, &intake.families)
        .is_ok()
        .then_some(bank))
}

/// [`translate`] with `detached`, an owner [`detachable_modifier`] named, left out. The caller
/// attaches that bank beside the graph.
#[allow(clippy::too_many_arguments)]
pub fn translate_without(
    source: &Source,
    native: &mut Reader,
    shared: &Shared,
    envelopes: &[Payload],
    tag: u32,
    symbol: &str,
    base: u32,
    detached: Option<u32>,
) -> Result<Graph> {
    let mut intake = intake::read(source, tag)?;
    if let Some(bank) = detached {
        ensure!(
            intake.unconnected_modifier() == Some(bank),
            "Detached owner is not the graph's unconnected modifier bank"
        );
        intake.detach(bank);
    }
    let choice = choice::choose(native, &intake.owners, &intake.families)?;
    intake.check_unconnected()?;
    let scope = component::Scope {
        source,
        shared,
        envelopes,
        entity: &intake.entity,
        choice: &choice,
        symbol,
        base,
    };
    let mut translated = component::translate(native, &scope, &intake.owners)?;
    let template = &choice.template;
    let (mut rows, used) =
        wiring::source_rows(&intake.graph, &intake.owners, template, &translated)?;
    wiring::template_rows(template, &used, &translated.components, &mut rows)?;
    wiring::stock_rows(template, &translated.components, &mut rows)?;
    retained::connect(native, &mut translated.components, &mut rows)?;
    wiring::check_inputs(&translated.components, &translated.inputs, &rows)?;
    let emitted = emission::resources(template, translated.components, rows, symbol)?;
    source.unchanged()?;
    Ok(Graph {
        hud: translated.hud,
        private_hud: translated.private_hud,
        symbol: symbol.to_owned(),
        components: emitted.ids,
        pending: emitted.pending,
        dependencies: translated.dependencies.into_values().collect(),
        evidence: json!({"source":tag,"source_sha256":hex::encode(Sha256::digest(&intake.entity.0)),
            "template":template.tag,"owners":emitted.owners,"omitted_owners":translated.omitted,
            "connections":intake.graph.connections.len(),
            "limits":["Source display bindings require the prepared ability publication route.",
                "Omitted owners are listed with their reason.",
                "Package construction does not establish buff application, ally queries or recharge rates in play."]}),
    })
}
