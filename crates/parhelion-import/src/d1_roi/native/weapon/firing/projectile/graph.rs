//! Match bound input identities through their owning response destinations.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum Role {
    Movement,
    Sequence,
    Category,
    Bank,
    /// A native named-parameter context. Rise of Iron graphs have none.
    Context,
    /// A named scalar property receiver. The carrier's is retained when it declares the source's
    /// rows.
    Receiver,
    /// Rocket components the carrier keeps natively. Every examined Rise of Iron rocket graph
    /// names the same owner in each role, so they hold no per-weapon values. Model `808072BD`
    /// selects the projectile model, body `80808A0C` its physics body, tracking `8080377D` reads
    /// the same four programs as source `80802AD9`, and query `80803813` emits the target query.
    Model,
    Body,
    Tracking,
    Query,
    Markers,
    Auxiliary,
    Network,
    /// An attachment spawner, Rise of Iron `80802A9A` and Shadowkeep `808037A9`, whose programs
    /// supply acquisition scale, per-target limit, chance, attempts and generation and total
    /// limits. It publishes one count to the bank.
    Attachment,
    /// A carrier component the source lacks, `80803F6E`, retained when it supplies only getters
    /// for names the source lacks.
    Extra,
    /// A value provider, Rise of Iron `808030BA` and Shadowkeep `8080378A`, with three typed
    /// slots, a published name and a block of 0.3, zero, a value and a count of one. Natively its
    /// provider slot supplies the bank getter for that name, `CD7422C6`, as Dragon's Breath graph
    /// `80BA016E` reads it. The block's runtime meaning is not established.
    Provider,
}

impl Role {
    fn read(class: u32, old: bool) -> Result<Self> {
        Ok(match (old, class) {
            (true, 0x808020AC) | (false, 0x8080388F) => Self::Movement,
            (true, 0x80800610) | (false, 0x808084E9) => Self::Sequence,
            (true, 0x80802202) | (false, 0x80804AF6) => Self::Category,
            (true, 0x808008F6) | (false, 0x80809790) => Self::Bank,
            (false, 0x80808BE6) => Self::Context,
            (true, 0x808005A8) | (false, 0x80808BF7) => Self::Receiver,
            (true, 0x80801A9C) | (false, 0x808072BD) => Self::Model,
            (true, 0x8080081D) | (false, 0x80808A0C) => Self::Body,
            (true, 0x80802AD9) | (false, 0x8080377D) => Self::Tracking,
            (true, 0x80802947) | (false, 0x80803813) => Self::Query,
            (true, 0x808008FB) | (false, 0x80808507) => Self::Markers,
            (true, 0x8080046A) | (false, 0x80809426) => Self::Auxiliary,
            (true, 0x80AACEA8) | (false, 0x80C70EDD) => Self::Network,
            (true, 0x80802A9A) | (false, 0x808037A9) => Self::Attachment,
            (false, 0x80803F6E) => Self::Extra,
            (true, 0x808030BA) | (false, 0x8080378A) => Self::Provider,
            _ => anyhow::bail!("Projectile component {class:08X} has no retained role"),
        })
    }
    fn name(self) -> &'static str {
        match self {
            Self::Movement => "movement",
            Self::Sequence => "sequence",
            Self::Category => "category",
            Self::Bank => "bank",
            Self::Context => "context",
            Self::Receiver => "receiver",
            Self::Model => "model",
            Self::Body => "body",
            Self::Tracking => "tracking",
            Self::Query => "query",
            Self::Markers => "markers",
            Self::Auxiliary => "auxiliary",
            Self::Network => "network",
            Self::Attachment => "attachment",
            Self::Extra => "extra",
            Self::Provider => "provider",
        }
    }
    /// A role whose carrier owner is kept natively, with the source's checked against it.
    fn retained(self) -> bool {
        matches!(
            self,
            Self::Model
                | Self::Body
                | Self::Tracking
                | Self::Query
                | Self::Markers
                | Self::Auxiliary
                | Self::Network
                | Self::Attachment
                | Self::Provider
        )
    }
}

/// Owners outside the graph that rocket inputs read, source then native. Their component index
/// `20000` names no owner of the graph.
const EXTERNAL: [(u32, u32); 1] = [(0x80AAD50E, 0x80C70C06)];

/// The shared Rise of Iron owner of each retained role.
const SHARED: [(Role, u32); 7] = [
    (Role::Model, 0x80A7E38C),
    (Role::Body, 0x80A7E38D),
    (Role::Tracking, 0x80AADD5A),
    (Role::Query, 0x80AADD5D),
    (Role::Markers, 0x80AA2D63),
    (Role::Auxiliary, 0x80AA8350),
    (Role::Network, 0x80AACEA6),
];

/// Retained owners that may differ from the shared one at one value field with an established
/// native counterpart: the role, its source and native definition fields, and the stock native
/// owner that stands for the shared source owner. Rise of Iron query `80AA8B6C` holds scale 1.3 at
/// definition `+450` where shared `80AADD5D` holds 1.0, and Shadowkeep query `8152A37C` holds 1.3
/// at `+26C` where `81533F18` holds 1.0. Neither pair differs anywhere else but its self tags.
const VARIANTS: [(Role, usize, usize, u32); 1] = [(Role::Query, 0x450, 0x26C, 0x81533F18)];

/// Definition fields of a tracking owner beside its programs: the relative reference to the
/// optional speed program, the turn angle in radians and, natively, the flag bit that marks the
/// speed program present.
struct Form {
    speed: usize,
    angle: usize,
    flag: Option<(usize, u32)>,
}

/// Tracking owners may take the form with a speed program. Against the shared owner, Rise of Iron
/// tracking `80AADD4A` differs only in its programs, the speed program reference at definition
/// `+198` and the turn angle at `+208`, 5.236 where shared `80AADD5A` holds 0.8727. Shadowkeep
/// tracking `80EF4355` differs from the common `80EF3974` only in the same programs, the reference
/// at `+160`, the angle at `+1C0` and bit 1 of the flags at `+48`. Each source owner, shared
/// owner, then the common native owner with its form.
const TRACKING: (u32, Form, u32, Form) = (
    0x80AADD5A,
    Form {
        speed: 0x198,
        angle: 0x208,
        flag: None,
    },
    0x80EF3974,
    Form {
        speed: 0x160,
        angle: 0x1C0,
        flag: Some((0x48, 2)),
    },
);

/// Definition-relative offsets of the 48-byte program records [`programs`] finds.
fn program_records(owner: &Owner) -> Vec<usize> {
    let p = &owner.payload;
    let mut found = Vec::new();
    let mut at = owner.definition;
    while at + 32 <= p.0.len() {
        let record = (|| -> Result<_> {
            let count = p.u64(at)?;
            ensure!((1..512).contains(&count));
            let header = p.pointer(at + 8)?;
            ensure!(p.u64(header)? == count && p.u32(header + 8)? == 0x80800009);
            Ok(())
        })();
        if record.is_ok() {
            found.push(at - owner.definition);
            at += 48;
        } else {
            at += 8;
        }
    }
    found
}

/// Require `owner` to differ from `reference` within its definition only in program records, the
/// form's fields and words naming each owner. A definition ends at the first array marker after
/// it, the array data that follows differing with the programs.
fn same_form(owner: &Owner, reference: &Owner, form: &Form, marker: u32) -> Result<()> {
    let records = program_records(owner)
        .into_iter()
        .chain(program_records(reference))
        .collect::<Vec<_>>();
    let (p, r) = (&owner.payload, &reference.payload);
    let mut at = 0;
    while owner.definition + at + 4 <= p.0.len() && reference.definition + at + 4 <= r.0.len() {
        let (a, b) = (
            p.u32(owner.definition + at)?,
            r.u32(reference.definition + at)?,
        );
        if a == marker || b == marker {
            break;
        }
        let allowed = a == b
            || (a == owner.tag && b == reference.tag)
            || records
                .iter()
                .any(|&record| (record..record + 48).contains(&at))
            || at == form.speed
            || at == form.speed + 4
            || at == form.angle
            || form
                .flag
                .is_some_and(|(field, bit)| at == field && (a ^ b) & !bit == 0);
        ensure!(
            allowed,
            "Projectile tracking owner {:08X} differs from {:08X} at definition +{at:X}",
            owner.tag,
            reference.tag
        );
        at += 4;
    }
    ensure!(
        at > form.angle,
        "Projectile tracking definition ends before its form fields"
    );
    Ok(())
}

/// The word at definition `+field` of `owner`, when `owner` equals `reference` everywhere else
/// apart from words naming each owner itself.
fn variant_value(owner: &Owner, reference: &Owner, field: usize) -> Result<u32> {
    let (p, r) = (&owner.payload, &reference.payload);
    ensure!(
        p.0.len() == r.0.len() && owner.definition == reference.definition,
        "Projectile owner variant layout differs"
    );
    let at = owner.definition + field;
    for word in (0..p.0.len().saturating_sub(3)).step_by(4) {
        let (a, b) = (p.u32(word)?, r.u32(word)?);
        ensure!(
            a == b || word == at || (a == owner.tag && b == reference.tag),
            "Projectile owner {:08X} differs from {:08X} at {word:X}",
            owner.tag,
            reference.tag
        );
    }
    p.u32(at)
}

/// Attachment spawner fields beside its programs, Rise of Iron then Shadowkeep definition
/// offsets: flags, scale and two 175.0 extents in a block of `20` bytes, and the published
/// channel name with its count. Zhalo Supercell's `80AADD36` and the stock spawner `80BC10B8`
/// hold the same bytes in each.
const ATTACHMENT: [(usize, usize, usize); 2] = [(0x120, 0x120, 0x20), (0x410, 0x3D8, 8)];

/// Provider fields, Rise of Iron then Shadowkeep definition offsets with their length: the
/// published name, then the block of 0.3, zero, the value and the count.
const PROVIDER: [(usize, usize, usize); 2] = [(0xA8, 0x90, 4), (0xB0, 0x9C, 16)];

/// Every stock provider. `80BF9E19` and `80C1C03F` differ only in the value, 6.0 and 60.0, and the
/// words naming each owner.
const PROVIDERS: [u32; 2] = [0x80BF9E19, 0x80C1C03F];

/// Definition fields a source model owner may change from the shared one: four floats at `+C0`
/// and the model at `+1C4`. The carrier's model owner supplies them natively, like all projectile
/// presentation. Thorn's `80A6A424` differs from shared `80A7E38C` only there, naming model
/// `80A6A875` where the shared owner names `80A6A714`.
const MODEL: [std::ops::Range<usize>; 2] = [0xC0..0xD0, 0x1C4..0x1C8];

/// Whether `owner` equals `reference` apart from definition `ranges` and words naming each owner.
fn same_except(
    owner: &Owner,
    reference: &Owner,
    ranges: &[std::ops::Range<usize>],
) -> Result<bool> {
    let (p, r) = (&owner.payload, &reference.payload);
    if p.0.len() != r.0.len() || owner.definition != reference.definition {
        return Ok(false);
    }
    for word in (0..p.0.len().saturating_sub(3)).step_by(4) {
        let (a, b) = (p.u32(word)?, r.u32(word)?);
        let inside = ranges.iter().any(|range| {
            (owner.definition + range.start..owner.definition + range.end).contains(&word)
        });
        if a != b && !inside && !(a == owner.tag && b == reference.tag) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Retained roles admitted as variants, and the values their private copies take from the
/// source: carrier owner, payload offset and bytes.
#[derive(Default)]
pub(super) struct Variants {
    pub admitted: BTreeSet<Role>,
    pub values: Vec<(u32, usize, Vec<u8>)>,
}

/// Source and native classes of the same retained input or provider object.
const CLASSES: [(u32, u32); 5] = [
    (0x80800921, 0x80809BE3),
    (0x8080073D, 0x80809BD7),
    (0x8080099E, 0x80809BD9),
    (0x808024E3, 0x80803891),
    (0x8080053B, 0x80809AE1),
];

/// The native class standing for a source class, or the class itself when native.
fn class(class: u32, old: bool) -> u32 {
    if !old {
        return class;
    }
    CLASSES
        .iter()
        .find(|(from, _)| *from == class)
        .map_or(class, |(_, to)| *to)
}

/// The programs a retained owner holds, in definition order: code and constants.
fn programs(owner: &Owner) -> Vec<(Vec<u8>, Vec<[u8; 16]>)> {
    let p = &owner.payload;
    let mut found = Vec::new();
    let mut at = owner.definition;
    while at + 32 <= p.0.len() {
        let program = (|| -> Result<_> {
            let count = p.u64(at)?;
            ensure!((1..512).contains(&count));
            let header = p.pointer(at + 8)?;
            ensure!(p.u64(header)? == count && p.u32(header + 8)? == 0x80800009);
            response::program(p, at)
        })();
        match program {
            Ok(program) => {
                found.push((program.code().to_vec(), program.constant_rows().to_vec()));
                at += 48;
            }
            Err(_) => at += 8,
        }
    }
    found
}

/// A receiver row's name, flag word and Float4 default.
pub(super) type ReceiverRow = (u32, u32, [u8; 16]);

/// A receiver's declared rows in order. Rise of Iron rows are `80800884`, stride `50`, at
/// definition `+90`, with the name at `+30` and the default at `+40`. Shadowkeep rows are
/// `80808BFA`, stride `40`, at `+78`, with the name at `+28` and the default at `+30`. Both
/// versions follow the name with the flag word. Each Shadowkeep row supplies its scalar through
/// the provider at row `+10`.
fn receiver(owner: &Owner, old: bool) -> Result<Vec<ReceiverRow>> {
    let p = &owner.payload;
    p.array(
        owner.definition + if old { 0x90 } else { 0x78 },
        if old { 0x50 } else { 0x40 },
        Some(if old { 0x80800884 } else { 0x80808BFA }),
    )?
    .into_iter()
    .map(|row| {
        let at = row + if old { 0x30 } else { 0x28 };
        ensure!(
            !old || p.u64(at + 8)? == 0,
            "Projectile receiver row has unexpected fields"
        );
        Ok((
            p.u32(at)?,
            p.u32(at + 4)?,
            p.bytes::<16>(at + if old { 0x10 } else { 8 })?,
        ))
    })
    .collect()
}

pub(super) struct Owner {
    pub tag: u32,
    pub payload: Payload,
    pub definition: usize,
    pub instance: usize,
}

pub(super) struct Graph {
    pub payload: Payload,
    pub owners: BTreeMap<Role, Owner>,
    /// Carrier components with no source counterpart, left out of the private graph.
    pub left_out: Vec<Owner>,
    order: Vec<u32>,
    old: bool,
}

#[derive(PartialEq, Eq)]
struct Channel {
    mode: u32,
    program: response::Expression,
    dependencies: Vec<u32>,
    inherited: Vec<(u32, u32)>,
    incoming: Vec<(u32, u32)>,
}

fn bank(owner: &Owner, old: bool) -> Result<(Vec<u32>, BTreeMap<u32, Channel>)> {
    let p = &owner.payload;
    let d = owner.definition;
    let rows = p.array(
        d + 0xD8,
        112,
        Some(if old { 0x80800883 } else { 0x808097A1 }),
    )?;
    let names = rows.iter().map(|&r| p.u32(r)).collect::<Result<Vec<_>>>()?;
    ensure!(
        !names.is_empty() && names.iter().collect::<BTreeSet<_>>().len() == names.len(),
        "Projectile bank has ambiguous channel names"
    );
    let lookup = |field| -> Result<BTreeMap<u32, Vec<(u32, u32)>>> {
        let mut values = BTreeMap::<u32, Vec<(u32, u32)>>::new();
        let mut keys = BTreeSet::new();
        for r in p.array(d + field, 12, None)? {
            let pair = (p.u32(r)?, p.u32(r + 4)?);
            let ordinal = p.u32(r + 8)?;
            ensure!(
                (ordinal as usize) < names.len() && keys.insert(pair),
                "Projectile channel lookup is invalid"
            );
            values.entry(ordinal).or_default().push(pair);
        }
        Ok(values)
    };
    let inherited = lookup(0x108)?;
    let incoming = lookup(0x118)?;
    let mut channels = BTreeMap::new();
    for (index, row) in rows.into_iter().enumerate() {
        let dependencies = p
            .array(row + 56, 4, Some(0x80800007))?
            .into_iter()
            .map(|r| {
                names
                    .get(p.u32(r)? as usize)
                    .copied()
                    .context("Projectile channel dependency is absent")
            })
            .collect::<Result<Vec<_>>>()?;
        channels.insert(
            names[index],
            Channel {
                mode: p.u32(row + 4)?,
                program: response::program(p, row + 8)?,
                dependencies,
                inherited: inherited.get(&(index as u32)).cloned().unwrap_or_default(),
                incoming: incoming.get(&(index as u32)).cloned().unwrap_or_default(),
            },
        );
    }
    Ok((names, channels))
}

/// Names both banks declare need identical producers, apart from a plain declaration that takes
/// its value from the creator in one bank and from a replaced receiver row in the other.
/// Dragon's Breath's rocket inherits `042DE7F5` and takes `A026F207` from receiver row 4, where
/// carrier `80BBD01A` takes `042DE7F5` from that row and inherits `A026F207`. The private bank
/// keeps the source's producers and the private receiver row the source's name, so both read what
/// the source reads.
fn same_producers(
    source: &BTreeMap<u32, Channel>,
    native: &BTreeMap<u32, Channel>,
    swaps: &BTreeMap<u32, u32>,
) -> Result<()> {
    let unnamed = 0x811C9DC5;
    let plain = |channel: &Channel| channel.dependencies.is_empty() && channel.mode == 0;
    // One bank inherits the name and the other takes it from a receiver row, both unnamespaced.
    let inherited_from_row = |inherits: &Channel, receives: &Channel, name: u32| {
        inherits.incoming.is_empty()
            && inherits.inherited == [(unnamed, name)]
            && receives.inherited.is_empty()
            && receives.incoming == [(unnamed, name)]
    };
    for (name, channel) in source {
        let Some(theirs) = native.get(name) else {
            continue;
        };
        let crossed = plain(channel)
            && plain(theirs)
            && channel.program == theirs.program
            && ((swaps.contains_key(name) && inherited_from_row(channel, theirs, *name))
                || (swaps.values().any(|row| row == name)
                    && inherited_from_row(theirs, channel, *name)));
        ensure!(
            theirs == channel || crossed,
            "Projectile channel {name:08X} has another producer"
        );
    }
    Ok(())
}

/// Points each receiver connection to a replaced row's getter at the getter of the source row's
/// name, and returns the renamed connections.
fn rename_receiver_getters(links: &mut [Value], swaps: &BTreeMap<u32, u32>) -> Vec<Value> {
    let mut renamed = Vec::new();
    for link in links {
        if link["provider"] != "receiver" {
            continue;
        }
        let Some(path) = link["consumer"].as_str() else {
            continue;
        };
        let Some((namespace, name)) = path
            .strip_prefix("bank/")
            .and_then(|rest| rest.split_once('/'))
        else {
            continue;
        };
        let Some(row) = u32::from_str_radix(name, 16)
            .ok()
            .filter(|_| namespace == "811C9DC5")
            .and_then(|name| swaps.get(&name))
        else {
            continue;
        };
        link["consumer"] = json!(format!("bank/{namespace}/{row:08X}"));
        renamed.push(link.clone());
    }
    renamed
}

struct Endpoint {
    namespace: u32,
    tag: u32,
    class: u32,
    offset: usize,
    selector: u64,
}

impl Graph {
    /// Retained roles whose source owner is a qualified variant of the shared one, with the
    /// source values their private copies take. In every role with a variant field, the carrier's
    /// owner must hold the source's value there, whether the source names the shared owner or a
    /// variant.
    pub fn variants(
        &self,
        native: &Self,
        mut source: impl FnMut(u32) -> Result<Payload>,
        mut stock: impl FnMut(u32) -> Result<Payload>,
    ) -> Result<Variants> {
        let mut admitted = BTreeSet::new();
        let mut values = Vec::new();
        let read = |tag: u32, payload: Payload| -> Result<Owner> {
            let definition = payload.pointer(24)?;
            Ok(Owner {
                tag,
                instance: payload.pointer(16)?,
                definition,
                payload,
            })
        };
        for (role, from, to, reference) in VARIANTS {
            let (Some(owner), Some(carrier)) = (self.owners.get(&role), native.owners.get(&role))
            else {
                continue;
            };
            let shared = SHARED
                .iter()
                .find(|(r, _)| *r == role)
                .map(|(_, tag)| *tag)
                .context("Projectile variant role has no shared owner")?;
            let shared = read(shared, source(shared)?)?;
            let reference = read(reference, stock(reference)?)?;
            ensure!(
                shared.payload.u32(shared.definition + from)?
                    == reference.payload.u32(reference.definition + to)?,
                "Projectile {} variant field counterpart differs",
                role.name()
            );
            let value = if owner.tag == shared.tag {
                shared.payload.u32(shared.definition + from)?
            } else {
                admitted.insert(role);
                variant_value(owner, &shared, from)?
            };
            let carried = if carrier.tag == reference.tag {
                reference.payload.u32(reference.definition + to)?
            } else {
                variant_value(carrier, &reference, to)?
            };
            ensure!(
                value == carried,
                "Projectile {} variant value differs from the carrier's",
                role.name()
            );
        }
        // An attachment spawner is kept when the carrier's holds the same fields beside its
        // programs, which compare with every retained role's.
        if let (Some(owner), Some(carrier)) = (
            self.owners.get(&Role::Attachment),
            native.owners.get(&Role::Attachment),
        ) {
            for (from, to, length) in ATTACHMENT {
                ensure!(
                    owner
                        .payload
                        .0
                        .get(owner.definition + from..owner.definition + from + length)
                        == carrier
                            .payload
                            .0
                            .get(carrier.definition + to..carrier.definition + to + length),
                    "Projectile attachment spawner fields differ from the carrier's"
                );
            }
            admitted.insert(Role::Attachment);
        }
        // A tracking owner of the speed program form is kept when the carrier's has the same
        // form, turn angle and speed program. The programs compare with every retained role's.
        if let (Some(owner), Some(carrier)) = (
            self.owners.get(&Role::Tracking),
            native.owners.get(&Role::Tracking),
        ) {
            let (shared, from, common, to) = &TRACKING;
            let shared = read(*shared, source(*shared)?)?;
            let common = read(*common, stock(*common)?)?;
            if owner.tag != shared.tag {
                same_form(owner, &shared, from, 0x80800184)?;
                admitted.insert(Role::Tracking);
            }
            if carrier.tag != common.tag {
                same_form(carrier, &common, to, 0x80809FBD)?;
            }
            let angle =
                |owner: &Owner, form: &Form| owner.payload.u32(owner.definition + form.angle);
            let speed = |owner: &Owner, form: &Form| -> Result<bool> {
                Ok(owner.payload.u64(owner.definition + form.speed)? != 0)
            };
            ensure!(
                angle(&shared, from)? == angle(&common, to)?
                    && speed(&shared, from)? == speed(&common, to)?,
                "Projectile tracking form counterpart differs"
            );
            ensure!(
                angle(owner, from)? == angle(carrier, to)?
                    && speed(owner, from)? == speed(carrier, to)?,
                "Projectile tracking turn angle or speed program differs from the carrier's"
            );
        }
        // A source model owner may differ from the shared one in presentation alone.
        if let Some(owner) = self.owners.get(&Role::Model) {
            let (_, shared) = SHARED
                .iter()
                .find(|(role, _)| *role == Role::Model)
                .context("Projectile model role has no shared owner")?;
            if owner.tag != *shared {
                let shared = read(*shared, source(*shared)?)?;
                ensure!(
                    same_except(owner, &shared, &MODEL)?,
                    "Projectile model owner {:08X} differs from the shared one beyond its model",
                    owner.tag
                );
                admitted.insert(Role::Model);
            }
        }
        // A provider keeps the carrier's owner with the source's published name. Its private copy
        // takes the source's block when a stock provider equal to the carrier's apart from that
        // block holds it, as `80C1C03F` holds Dragon's Breath's 60.0 where carrier `80BF9E19`
        // holds 6.0.
        if let (Some(owner), Some(carrier)) = (
            self.owners.get(&Role::Provider),
            native.owners.get(&Role::Provider),
        ) {
            let field = |owner: &Owner, at: usize, length: usize| -> Result<Vec<u8>> {
                let at = owner.definition + at;
                Ok(owner
                    .payload
                    .0
                    .get(at..at + length)
                    .context("Projectile provider field")?
                    .to_vec())
            };
            let [(name, native_name, length), (from, to, width)] = PROVIDER;
            ensure!(
                field(owner, name, length)? == field(carrier, native_name, length)?,
                "Projectile provider name differs from the carrier's"
            );
            let block = field(owner, from, width)?;
            if block != field(carrier, to, width)? {
                let mut held = false;
                for tag in PROVIDERS {
                    let candidate = read(tag, stock(tag)?)?;
                    if field(&candidate, to, width)? == block
                        && same_except(
                            &candidate,
                            carrier,
                            std::slice::from_ref(&(to..to + width)),
                        )?
                    {
                        held = true;
                        break;
                    }
                }
                ensure!(
                    held,
                    "No stock projectile provider holds the source's values"
                );
                values.push((carrier.tag, carrier.definition + to, block));
            }
            admitted.insert(Role::Provider);
        }
        Ok(Variants { admitted, values })
    }

    /// The kept owners in graph order.
    pub fn ordered(&self) -> Result<Vec<&Owner>> {
        self.order
            .iter()
            .filter(|tag| !self.left_out.iter().any(|owner| owner.tag == **tag))
            .map(|tag| {
                self.owners
                    .values()
                    .find(|owner| owner.tag == *tag)
                    .context("Projectile component order is incomplete")
            })
            .collect()
    }
    pub fn read(tag: u32, old: bool, mut read: impl FnMut(u32) -> Result<Payload>) -> Result<Self> {
        let payload = read(tag)?;
        ensure!(
            payload.u64(0)? == payload.0.len() as u64,
            "Projectile graph extent differs"
        );
        let mut owners = BTreeMap::new();
        let mut left_out = Vec::new();
        let mut order = Vec::new();
        for r in payload.array(
            if old { 32 } else { 16 },
            12,
            Some(if old { 0x80800715 } else { 0x80809C04 }),
        )? {
            let tag = payload.u32(r)?;
            let p = read(tag)?;
            ensure!(
                p.u64(0)? == p.0.len() as u64,
                "Projectile owner extent differs"
            );
            let definition = p.pointer(24)?;
            let instance = p.pointer(16)?;
            ensure!(
                p.u64(definition + 8)? == instance as u64
                    && p.u64(instance + 8)? == definition as u64,
                "Projectile owner has invalid reciprocal objects"
            );
            let owner = Owner {
                tag,
                payload: p,
                definition,
                instance,
            };
            // A carrier component of a class the source has no role for is left out, as
            // attachment spawner `808037A9` of machine gun carrier `81529CCB` is. Its
            // connections are checked when the graphs compare.
            let role = match Role::read(owner.payload.u32(definition - 4)?, old) {
                Ok(role) => role,
                Err(_) if !old => {
                    left_out.push(owner);
                    order.push(tag);
                    continue;
                }
                Err(error) => return Err(error),
            };
            ensure!(
                owners.insert(role, owner).is_none(),
                "Ambiguous projectile component role"
            );
            order.push(tag);
        }
        ensure!(
            [Role::Movement, Role::Category, Role::Bank]
                .iter()
                .all(|role| owners.contains_key(role))
                && order.iter().collect::<BTreeSet<_>>().len() == owners.len() + left_out.len(),
            "Projectile needs distinct movement, category and bank owners"
        );
        Ok(Self {
            payload,
            owners,
            left_out,
            order,
            old,
        })
    }

    fn left_out_tag(&self, tag: u32) -> bool {
        self.left_out.iter().any(|owner| owner.tag == tag)
    }

    /// Leave out the carrier's retained-role owners the source has no owner for, as machine gun
    /// carrier `81529CCB` has an attachment spawner where Rise of Iron's Thunderlord has none.
    pub fn leave_out_absent(&mut self, source: &Self) {
        let absent = self
            .owners
            .keys()
            .copied()
            .filter(|role| role.retained() && !source.owners.contains_key(role))
            .collect::<Vec<_>>();
        for role in absent {
            if let Some(owner) = self.owners.remove(&role) {
                self.left_out.push(owner);
            }
        }
    }

    pub fn owner(&self, role: Role) -> Result<&Owner> {
        self.owners
            .get(&role)
            .context("Projectile component role is missing")
    }

    fn endpoint(&self, at: usize) -> Result<Endpoint> {
        let p = &self.payload;
        let e = if self.old {
            Endpoint {
                namespace: u32::try_from(p.u64(at)?)?,
                tag: u32::try_from(p.u64(at + 8)?)?,
                offset: usize::try_from(p.u64(at + 16)?)?,
                class: u32::try_from(p.u64(at + 24)?)?,
                selector: p.u64(at + 32)?,
            }
        } else {
            ensure!(p.u32(at + 4)? == 0, "Projectile endpoint padding differs");
            Endpoint {
                namespace: p.u32(at)?,
                tag: p.u32(at + 8)?,
                class: p.u32(at + 12)?,
                offset: usize::try_from(p.u64(at + 16)?)?,
                selector: p.u64(at + 24)?,
            }
        };
        if e.selector == 0xFFFF {
            ensure!(
                e.tag == u32::MAX && e.class == u32::MAX && e.offset == 0,
                "Invalid disconnected projectile endpoint"
            );
        } else if e.selector == 0x20000 {
            ensure!(
                self.external(&e).is_some() && !self.order.contains(&e.tag),
                "Projectile endpoint names an unknown external owner"
            );
        } else {
            ensure!(
                self.order.get(usize::try_from(e.selector)?).copied() == Some(e.tag),
                "Projectile endpoint has an external or inconsistent selector"
            );
            self.owners
                .values()
                .chain(&self.left_out)
                .find(|o| o.tag == e.tag)
                .context("Projectile endpoint owner")?
                .payload
                .bytes::<1>(e.offset)?;
        }
        Ok(e)
    }

    /// The native owner an external endpoint names.
    fn external(&self, e: &Endpoint) -> Option<u32> {
        if e.selector != 0x20000 {
            return None;
        }
        EXTERNAL
            .iter()
            .find(|(from, to)| e.tag == if self.old { *from } else { *to })
            .map(|(_, to)| *to)
    }

    fn role(&self, e: &Endpoint) -> Result<Option<Role>> {
        if e.selector == 0xFFFF || e.selector == 0x20000 {
            return Ok(None);
        }
        Ok(Some(
            *self
                .owners
                .iter()
                .find(|(_, o)| o.tag == e.tag)
                .context("Projectile endpoint owner role")?
                .0,
        ))
    }

    fn consumer(
        &self,
        e: &Endpoint,
        inputs: &BTreeMap<usize, String>,
        seen: &mut BTreeSet<usize>,
        (occurrences, inputs_seen): OccurrenceCounters<'_>,
    ) -> Result<(String, Option<u32>)> {
        let role = self.role(e)?.context("Disconnected projectile consumer")?;
        let owner = self.owner(role)?;
        let named = if self.old { 0x80800927 } else { 0x80809789 };
        let settings = if self.old { 0x8080099E } else { 0x80809BD9 };
        if e.class == named {
            let name = owner
                .payload
                .u32(e.offset + if self.old { 40 } else { 32 })?;
            let path = match role {
                Role::Movement => {
                    ensure!(seen.insert(e.offset), "Projectile input is connected twice");
                    inputs
                        .get(&e.offset)
                        .context("Projectile input has no response destination")?
                        .clone()
                }
                // Sequence inputs are numbered within each name, so a carrier input reading a
                // name the source lacks leaves the others' paths unchanged.
                Role::Sequence => {
                    let occurrence = occurrences.entry(name).or_default();
                    *occurrence += 1;
                    format!("sequence/{name:08X}/{}", *occurrence - 1)
                }
                role if role.retained() => format!("{}/{name:08X}", role.name()),
                _ => anyhow::bail!("Named projectile input belongs to another role"),
            };
            return Ok((path, Some(name)));
        }
        if e.class == settings && matches!(role, Role::Movement | Role::Category) {
            return Ok((format!("{}/settings", role.name()), None));
        }
        // Retained owners' other inputs are numbered within their class.
        if role.retained() {
            let occurrence = inputs_seen
                .entry((role, class(e.class, self.old)))
                .or_default();
            *occurrence += 1;
            return Ok((
                format!(
                    "{}/{:08X}/{}",
                    role.name(),
                    class(e.class, self.old),
                    *occurrence - 1
                ),
                None,
            ));
        }
        ensure!(
            role == Role::Bank && e.class == if self.old { 0x80800668 } else { 0x808097A8 },
            "Unsupported projectile consumer class"
        );
        let getters = owner.payload.array(
            owner.definition + 0xC8,
            if self.old { 40 } else { 32 },
            Some(e.class),
        )?;
        let ordinal = getters
            .iter()
            .position(|&at| at == e.offset)
            .context("Projectile bank consumer is not a getter")?;
        let lookup = owner.payload.array(owner.definition + 0x118, 12, None)?;
        let names = lookup
            .into_iter()
            .filter(|&at| owner.payload.u32(at + 8).ok() == Some(ordinal as u32))
            .collect::<Vec<_>>();
        ensure!(names.len() == 1, "Projectile bank getter name is ambiguous");
        Ok((
            format!(
                "bank/{:08X}/{:08X}",
                owner.payload.u32(names[0])?,
                owner.payload.u32(names[0] + 4)?
            ),
            None,
        ))
    }

    fn links(&self, names: &[u32], inputs: &BTreeMap<usize, String>) -> Result<Vec<Value>> {
        let mut links = Vec::new();
        let mut occurrences = BTreeMap::new();
        let mut inputs_seen = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let fields = if self.old { [48, 64] } else { [32, 48] };
        for (kind, field) in fields.into_iter().enumerate() {
            for row in self
                .payload
                .array(field, if self.old { 88 } else { 72 }, None)?
            {
                let a = self.endpoint(row)?;
                let b = self.endpoint(row + if self.old { 40 } else { 32 })?;
                let channel = self.payload.u32(row + if self.old { 80 } else { 64 })?;
                let flags = self.payload.u32(row + if self.old { 84 } else { 68 })?;
                // What a left-out component reads leaves with it. What it supplies is named, so
                // the comparison can check that nothing kept reads it.
                if self.left_out_tag(a.tag) && a.selector < 0xFFFF {
                    continue;
                }
                if self.left_out_tag(b.tag) && b.selector < 0xFFFF {
                    let (path, _) =
                        self.consumer(&a, inputs, &mut seen, (&mut occurrences, &mut inputs_seen))?;
                    links.push(json!({"kind":kind,"consumer":path,"provider":"left_out",
                        "owner":b.tag,"class":b.class,"channel":channel}));
                    continue;
                }
                let (path, name) =
                    self.consumer(&a, inputs, &mut seen, (&mut occurrences, &mut inputs_seen))?;
                let role = self.role(&b)?;
                let selected = match role {
                    Some(Role::Bank) => {
                        ensure!(
                            b.class == if self.old { 0x808005AB } else { 0x808097C2 },
                            "Projectile bank provider class differs"
                        );
                        let selected = *names
                            .get(channel as usize)
                            .context("Projectile channel ordinal is absent")?;
                        ensure!(
                            name.is_none_or(|name| name == selected),
                            "Projectile bound input name differs from its provider"
                        );
                        json!({"name":selected})
                    }
                    Some(Role::Category) => {
                        ensure!(
                            b.class == if self.old { 0x8080053B } else { 0x80809AE1 },
                            "Projectile category provider class differs"
                        );
                        json!({"channel":channel})
                    }
                    Some(Role::Context) => {
                        ensure!(
                            b.class == 0x80809AE1,
                            "Projectile context provider class differs"
                        );
                        json!({"channel":channel})
                    }
                    Some(Role::Receiver) => {
                        ensure!(
                            b.class == if self.old { 0x8080053B } else { 0x80809AE1 },
                            "Projectile receiver provider class differs"
                        );
                        json!({"channel":channel})
                    }
                    // Movement outputs, attachment counts and carrier-only suppliers name their
                    // output class.
                    Some(Role::Movement | Role::Extra | Role::Attachment) => {
                        json!({"channel":channel,"class":class(b.class, self.old)})
                    }
                    None if b.selector == 0x20000 => {
                        json!({"channel":channel,"owner":self.external(&b),"class":class(b.class, self.old)})
                    }
                    None => json!({"channel":channel}),
                    _ => anyhow::bail!("Projectile graph has another provider role"),
                };
                let provider = if b.selector == 0x20000 {
                    Some("external")
                } else {
                    role.map(Role::name)
                };
                links.push(json!({"kind":kind,"consumer":path,"consumer_namespace":a.namespace,
                    "provider":provider,"provider_namespace":b.namespace,"selected":selected,"flags":flags}));
            }
        }
        ensure!(
            seen.iter().eq(inputs.keys()),
            "A named response input is disconnected"
        );
        ensure!(!links.is_empty(), "Empty projectile connections");
        Ok(links)
    }

    /// Bank channels selected for unnamed settings inputs, by consumer role.
    pub fn settings(&self) -> Result<BTreeMap<Role, u32>> {
        let class = if self.old { 0x8080099E } else { 0x80809BD9 };
        let mut found = BTreeMap::new();
        for field in if self.old { [48, 64] } else { [32, 48] } {
            for row in self
                .payload
                .array(field, if self.old { 88 } else { 72 }, None)?
            {
                let consumer = self.endpoint(row)?;
                let provider = self.endpoint(row + if self.old { 40 } else { 32 })?;
                if consumer.class != class || self.role(&provider)? != Some(Role::Bank) {
                    continue;
                }
                let role = self
                    .role(&consumer)?
                    .context("Disconnected projectile settings input")?;
                let channel = self.payload.u32(row + if self.old { 80 } else { 64 })?;
                ensure!(
                    found.insert(role, channel).is_none(),
                    "Projectile settings input is bound twice"
                );
            }
        }
        Ok(found)
    }

    /// Carrier receiver rows that take the source's row, by row index. Named property writes find
    /// a row by name, and its default stands until one arrives, so the private receiver declares
    /// the source's rows. Both receivers hold the same number. Sniper and hand cannon carriers
    /// already declare the source's `736524DE`. Scout carrier `80BBC8B2` declares `158B5592`,
    /// which nothing in the carrier reads, while its context supplies `736524DE`.
    pub fn receiver_rows(&self, native: &Self) -> Result<Vec<(usize, ReceiverRow)>> {
        let Some(owner) = self.owners.get(&Role::Receiver) else {
            return Ok(Vec::new());
        };
        let source = receiver(owner, true)?;
        let carrier = receiver(native.owner(Role::Receiver)?, false)?;
        ensure!(
            source.len() == carrier.len()
                && source
                    .iter()
                    .map(|row| row.0)
                    .collect::<BTreeSet<_>>()
                    .len()
                    == source.len(),
            "Projectile receiver rows differ"
        );
        Ok(source
            .into_iter()
            .zip(carrier)
            .enumerate()
            .filter(|(_, (from, to))| from != to)
            .map(|(index, (from, _))| (index, from))
            .collect())
    }

    /// Each replaced carrier receiver row's name, with the source row name its provider supplies
    /// in the private receiver.
    pub fn receiver_swaps(&self, native: &Self) -> Result<BTreeMap<u32, u32>> {
        let replaced = self.receiver_rows(native)?;
        if replaced.is_empty() {
            return Ok(BTreeMap::new());
        }
        let carrier = receiver(native.owner(Role::Receiver)?, false)?;
        replaced
            .iter()
            .map(|(index, row)| {
                Ok((
                    carrier.get(*index).context("Projectile receiver row")?.0,
                    row.0,
                ))
            })
            .collect()
    }

    /// Names both banks declare need identical producers, because the carrier's retained
    /// consumers were built against them, apart from plain values a replaced receiver row moves
    /// between the creator and the row. Names only one bank declares join the private bank.
    pub fn compare(
        &self,
        native: &Self,
        inputs: &BTreeMap<usize, String>,
        target_inputs: &BTreeMap<usize, String>,
        additional_inputs: &BTreeSet<String>,
        created: &BTreeSet<String>,
        variants: &BTreeSet<Role>,
    ) -> Result<Value> {
        // Only the carrier can have a context, and a receiver the source lacks is the carrier's
        // own, retained when it supplies only names the source lacks.
        let carrier_receiver = !self.owners.contains_key(&Role::Receiver);
        ensure!(
            self.owners.keys().eq(native.owners.keys().filter(|role| {
                !matches!(role, Role::Context | Role::Extra)
                    && (**role != Role::Receiver || !carrier_receiver)
            })),
            "Projectile component roles differ"
        );
        // A retained role keeps the carrier's owner, so the source must name the shared owner
        // every examined rocket names in that role, or a qualified variant of it, with the
        // carrier's programs.
        for (&role, owner) in self.owners.iter().filter(|(role, _)| role.retained()) {
            ensure!(
                SHARED.contains(&(role, owner.tag)) || variants.contains(&role),
                "Projectile {} owner {:08X} is not the shared one",
                role.name(),
                owner.tag
            );
            ensure!(
                programs(owner) == programs(native.owner(role)?),
                "Projectile {} programs differ",
                role.name()
            );
        }
        let replaced = self.receiver_rows(native)?;
        let receiver_rows = match self.owners.get(&Role::Receiver) {
            Some(owner) => receiver(owner, true)?
                .into_iter()
                .map(|row| row.0)
                .collect::<Vec<_>>(),
            None => Vec::new(),
        };
        let swaps = self.receiver_swaps(native)?;
        let (sn, sc) = bank(self.owner(Role::Bank)?, true)?;
        let (nn, nc) = bank(native.owner(Role::Bank)?, false)?;
        same_producers(&sc, &nc, &swaps)?;
        let mut source_links = self.links(&sn, inputs)?;
        let mut native_links = native.links(&nn, target_inputs)?;
        let getter = |link: &Value| {
            link["consumer"]
                .as_str()
                .and_then(|path| path.strip_prefix("bank/811C9DC5/"))
                .and_then(|name| u32::from_str_radix(name, 16).ok())
        };
        // A receiver row supplies the bank getter of its own name, so a replaced row's getter
        // connection now reads the getter of the source row's name. Connections from rows that
        // keep their name compare as they are. A getter the carrier context supplies for a name
        // the source receiver declares reads the private receiver instead.
        let mut moved = Vec::new();
        let swapped = rename_receiver_getters(&mut native_links, &swaps);
        if !replaced.is_empty() {
            let names = replaced
                .iter()
                .map(|(_, row)| row.0)
                .collect::<BTreeSet<_>>();
            for link in &mut native_links {
                if link["provider"] == "context" && getter(link).is_some_and(|n| names.contains(&n))
                {
                    link["provider"] = json!("receiver");
                    moved.push(link.clone());
                }
            }
        }
        // Source inputs the private movement gains have no carrier connection yet. The bridge
        // creates one from the bank channel each names.
        let mut created_links = Vec::new();
        source_links.retain(|link| {
            if link["consumer"]
                .as_str()
                .is_some_and(|path| created.contains(path))
                && link["provider"] == "bank"
            {
                created_links.push(link.clone());
                false
            } else {
                true
            }
        });
        ensure!(
            created_links.len() == created.len(),
            "Created projectile inputs have no source connection"
        );
        // Inputs of a source child parameter left out for want of a reader.
        let mut omitted_links = Vec::new();
        source_links.retain(|link| {
            if link["consumer"]
                .as_str()
                .is_some_and(|path| path.starts_with("dropped/"))
            {
                omitted_links.push(link.clone());
                false
            } else {
                true
            }
        });
        let mut additional_links = Vec::new();
        native_links.retain(|link| {
            if link["consumer"]
                .as_str()
                .is_some_and(|path| additional_inputs.contains(path))
            {
                additional_links.push(link.clone());
                false
            } else {
                true
            }
        });
        ensure!(
            additional_links.len() == additional_inputs.len(),
            "Native child parameter connections are incomplete"
        );
        // Inputs of carrier groups left out of the private movement keep their connections, which
        // no remaining response reads.
        let mut dropped_links = Vec::new();
        native_links.retain(|link| {
            if link["consumer"]
                .as_str()
                .is_some_and(|path| path.starts_with("dropped/"))
            {
                dropped_links.push(link.clone());
                false
            } else {
                true
            }
        });
        // The carrier's effect sequence may read a bank name more often than the source's does,
        // including a name only the carrier declares, as automatic rifle carrier `80BBC8AC`
        // reads `B120D867`. Shared names keep identical producers and the private bank keeps the
        // carrier's own declarations, so each such input reads what it reads natively. Sequence
        // inputs are numbered within each name, so the extra ones are those past the source's.
        let sequence_name = |link: &Value| {
            link["consumer"]
                .as_str()
                .and_then(|path| path.strip_prefix("sequence/"))
                .and_then(|rest| {
                    let (name, occurrence) = rest.split_once('/')?;
                    Some((
                        u32::from_str_radix(name, 16).ok()?,
                        occurrence.parse::<usize>().ok()?,
                    ))
                })
        };
        let mut source_reads = BTreeMap::<u32, usize>::new();
        for link in &source_links {
            if let Some((name, _)) = sequence_name(link) {
                *source_reads.entry(name).or_default() += 1;
            }
        }
        let mut native_reads = BTreeMap::<u32, usize>::new();
        for link in &native_links {
            if let Some((name, _)) = sequence_name(link) {
                *native_reads.entry(name).or_default() += 1;
            }
        }
        // The private graph keeps the carrier's effect sequence, so a source sequence read past
        // the carrier's count for that name has no reader there. It is left out and reported, as
        // The Jade Rabbit's sequence reads `49FCE899`, which carrier `815294C1`'s does not.
        let mut source_sequence_links = Vec::new();
        source_links.retain(|link| {
            let extra = sequence_name(link).is_some_and(|(name, occurrence)| {
                occurrence >= native_reads.get(&name).copied().unwrap_or(0)
            });
            if extra && link["provider"] == "bank" {
                source_sequence_links.push(link.clone());
                false
            } else {
                true
            }
        });
        let mut sequence_links = Vec::new();
        native_links.retain(|link| {
            let extra = sequence_name(link).is_some_and(|(name, occurrence)| {
                occurrence >= source_reads.get(&name).copied().unwrap_or(0) && nn.contains(&name)
            });
            if extra && link["provider"] == "bank" {
                sequence_links.push(link.clone());
                false
            } else {
                true
            }
        });
        let CarrierLinks {
            context_links,
            carrier_receiver_links,
            left_out_inputs,
        } = retained_carrier_links(&mut native_links, getter, (&nn, &sn), carrier_receiver)?;
        // Getters for names only the carrier declares keep their carrier suppliers, as rocket
        // carrier `80BBD121` reads `DC44E70B` and three more for its own sequence.
        let mut carrier_getters = Vec::new();
        native_links.retain(|link| {
            let carrier_only = link["provider"] != "bank"
                && getter(link).is_some_and(|name| nn.contains(&name) && !sn.contains(&name));
            if carrier_only {
                carrier_getters.push(link.clone());
            }
            !carrier_only
        });
        // A source movement output into a retained owner input the carrier's owner lacks cannot
        // be kept. Rise of Iron query `80802947` reads one that Shadowkeep `80803813` does not.
        let mut unkept = Vec::new();
        source_links.retain(|link| {
            let retained_input = link["provider"] == "movement"
                && link["consumer"].as_str().is_some_and(|path| {
                    path.split('/').next().is_some_and(|role| {
                        [Role::Query, Role::Tracking]
                            .iter()
                            .any(|candidate| candidate.name() == role)
                    }) && !native_links.iter().any(|other| other["consumer"] == path)
                });
            if retained_input {
                unkept.push(link.clone());
            }
            !retained_input
        });
        // A settings input has no name, and Shadowkeep's binder keeps only the provider for its
        // class `80809BD9`, not the connection's channel. Rise of Iron shotgun rows select
        // `8A0D4131` where the native carrier selects `CAEA8F7A`. The private row keeps the
        // source selection, and both are reported rather than compared.
        let unnamed = |link: &Value| {
            link["provider"] == "bank"
                && link["consumer"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("/settings"))
        };
        // Settings inputs pair by consumer. Connection rows have no meaningful order, so both
        // sides compare as sorted lists, which keeps repeated connections counted.
        let mut settings = Vec::new();
        for from in source_links.iter_mut().filter(|link| unnamed(link)) {
            if let Some(to) = native_links.iter_mut().find(|to| {
                unnamed(to) && to["consumer"] == from["consumer"] && !to["selected"].is_null()
            }) {
                settings.push(json!({"consumer":from["consumer"],
                    "source":from["selected"].take(),"native":to["selected"].take()}));
            }
        }
        let key = |link: &Value| link.to_string();
        source_links.sort_by_cached_key(key);
        native_links.sort_by_cached_key(key);
        // Name the first connection one side has more often, for diagnosis.
        let only = |from: &[Value], to: &[Value]| {
            from.iter()
                .find(|link| {
                    from.iter().filter(|other| other == link).count()
                        > to.iter().filter(|other| other == link).count()
                })
                .map(|link| format!("{} from {}", link["consumer"], link["provider"]))
                .unwrap_or_default()
        };
        ensure!(
            source_links == native_links,
            "Projectile bound response destinations differ: source {} carrier {}",
            only(&source_links, &native_links),
            only(&native_links, &source_links)
        );
        Ok(
            json!({"source":sn,"native":nn,"connections":source_links,"additional_child_parameters":additional_links,
            "settings":settings,"context":context_links,"carrier_only":dropped_links,"created":created_links,"carrier_sequence_inputs":sequence_links,"source_sequence_inputs":source_sequence_links,
            "left_out_components":native.left_out.iter().map(|owner| owner.tag).collect::<Vec<_>>(),
            "left_out_inputs":left_out_inputs,
            "source_omitted":omitted_links,"carrier_getters":carrier_getters,"unkept_source_inputs":unkept,
            "receiver":{"rows":receiver_rows,"replaced":replaced.iter().map(|(index, row)| json!({"row":index,"name":row.0})).collect::<Vec<_>>(),
                "moved_from_context":moved,"renamed_getters":swapped,"carrier_only":carrier_receiver_links},
            "additional_native":nc.keys().filter(|name|!sc.contains_key(name)).copied().collect::<Vec<_>>(),
            "source_only":sc.keys().filter(|name|!nc.contains_key(name)).copied().collect::<Vec<_>>() }),
        )
    }
}

type OccurrenceCounters<'a> = (
    &'a mut BTreeMap<u32, usize>,
    &'a mut BTreeMap<(Role, u32), usize>,
);

struct CarrierLinks {
    context_links: Vec<Value>,
    carrier_receiver_links: Vec<Value>,
    left_out_inputs: Vec<Value>,
}

fn retained_carrier_links(
    native_links: &mut Vec<Value>,
    getter: impl Fn(&Value) -> Option<u32>,
    (nn, sn): (&[u32], &[u32]),
    carrier_receiver: bool,
) -> Result<CarrierLinks> {
    // A named-parameter context passes creation values only to bank getters for names the
    // source lacks. The private bank keeps those declarations and the context is retained.
    let mut context_links = Vec::new();
    native_links.retain(|link| {
        if link["provider"] == "context" {
            context_links.push(link.clone());
            false
        } else {
            true
        }
    });
    // A context whose only getters moved to the receiver stays, supplying nothing, as does
    // one with no connection at all, such as rocket carrier `80BBD01A`'s `8152A386`.
    for link in &context_links {
        let name = getter(link);
        ensure!(
            name.is_some_and(|name| nn.contains(&name) && !sn.contains(&name)),
            "Projectile context feeds more than a native-only bank getter"
        );
    }
    // Sidearm carrier `80BBDC54` has a receiver whose `736524DE` row supplies the getter of
    // that name, which its Rise of Iron counterpart lacks.
    let mut carrier_receiver_links = Vec::new();
    if carrier_receiver {
        native_links.retain(|link| {
            if link["provider"] == "receiver" {
                carrier_receiver_links.push(link.clone());
                false
            } else {
                true
            }
        });
        for link in &carrier_receiver_links {
            ensure!(
                getter(link).is_some_and(|name| nn.contains(&name) && !sn.contains(&name)),
                "Carrier-only projectile receiver supplies more than a native-only getter"
            );
        }
    }
    // A left-out component may only supply a bank getter for a name the source lacks and no
    // other connection reads. The private bank leaves that name out with it, as machine gun
    // carrier `81529CCB` declares `5D64368A` only for its attachment spawner's count.
    let mut left_out_inputs = Vec::new();
    native_links.retain(|link| {
        let left = link["provider"] == "left_out";
        if left {
            left_out_inputs.push(link.clone());
        }
        !left
    });
    for link in &left_out_inputs {
        let name = link["consumer"]
            .as_str()
            .and_then(|path| path.strip_prefix("bank/"))
            .and_then(|rest| rest.split_once('/'))
            .and_then(|(_, name)| u32::from_str_radix(name, 16).ok())
            .context("Left-out projectile component supplies a kept component")?;
        // A settings input takes the source's selection in the private graph, as the
        // carrier movement's settings read `5D64368A`.
        ensure!(
            !sn.contains(&name)
                && !native_links.iter().any(|other| {
                    other["selected"]["name"].as_u64() == Some(u64::from(name))
                        && !other["consumer"]
                            .as_str()
                            .is_some_and(|path| path.ends_with("/settings"))
                }),
            "Left-out projectile component supplies a name the private graph reads"
        );
    }
    Ok(CarrierLinks {
        context_links,
        carrier_receiver_links,
        left_out_inputs,
    })
}
