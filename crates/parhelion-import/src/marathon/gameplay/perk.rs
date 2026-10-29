//! Weapon mods as private socket choices.
//!
//! A source weapon lists its sockets, each of a socket type. A mod socket type names the slot it
//! fills, such as a barrel or a chip, the mods every weapon's socket of that type accepts and the
//! glyph the slot shows. A weapon's socket can also accept mods made for that weapon alone. A mod
//! carries stat deltas on the source scale, scripted behavior, or both. A delta goes through the
//! effect match the weapon's own scores use, or moves a stat Shadowkeep shares by identity as far
//! through the donor's range as it moves the source score through the source range. A mod becomes
//! a private plug when one of its deltas has a native effect, and the others are reported.
//! Scripted chip behavior has no stored program in the item data, so a chip is carried only where
//! a Shadowkeep perk performs what its description states, with that perk's effects. Every choice
//! wears its slot's glyph. Everything else is reported.
mod icon;
use super::*;
use std::sync::Arc;

/// Item definitions by identity, with the definition tag at row +0x10.
const ITEMS: u32 = 0x8080_9685;
const ITEM_ROW: u32 = 0x8080_9689;
const ITEM_DEFINITION: u32 = 0x8080_968B;
/// The definition's stat deltas, read by the base stat reader when present.
const STAT_DELTAS: usize = 0x70;
const STAT_BLOCK: u32 = 0x8080_9245;
/// The weapon definition's socket list, 0x50-byte rows that start with the socket type index.
const SOCKETS: usize = 0xE0;
const SOCKET_LIST: u32 = 0x8080_92A5;
const SOCKET_ROW: u32 = 0x8080_6919;
const SOCKET_SIZE: usize = 0x50;
/// The mods a socket accepts on this weapon alone, as item ordinals, and the ordinal of the plug
/// it starts with, `FFFF` for none.
const EXCLUSIVE: usize = 0x08;
const ORDINAL_ROW: u32 = 0x8080_B1DD;
const INITIAL: usize = 0x28;
/// Socket types by index: the type's hash, the mods it accepts as item ordinals and the hash of
/// the slot's name, empty for cosmetic and progression sockets.
const SOCKET_TYPES: u32 = 0x8080_8C6B;
const SOCKET_TYPE_ROW: u32 = 0x8080_8ACC;
const SOCKET_TYPE_SIZE: usize = 0x48;
const ACCEPTS: usize = 0x18;
const SLOT: usize = 0x44;
const NO_SLOT: u32 = 0x811C_9DC5;
/// A plug record: the item grade, the plug family and whether the plug is made for one weapon.
const PLUG: usize = 0xD8;
const PLUG_RECORD: u32 = 0x8080_92AD;
const PLUG_GRADE: usize = 0x04;
const PLUG_FAMILY: usize = 0x06;
const PLUG_OWNED: usize = 0x08;
/// Item strings by identity, with the strings record's tag64 at row +0x10.
const ITEM_STRINGS: u32 = 0x8080_6EF0;
const NAME: usize = 0xBC;
const DESCRIPTION: usize = 0xD8;
/// Native plugs a perk socket offers, and the category of a weapon's intrinsic frame.
const PERK_ITEM_KIND: u64 = 15;
const INTRINSIC_CATEGORY: u64 = fnv1("intrinsics") as u64;
const NATIVE_ITEM: u32 = 0x8080_7BEA;
/// A lane the translation leaves without choices, so the donor's own perks do not stand in.
const DISABLED: u16 = u16::MAX;
/// The most choices a column offers freely, as few as a Shadowkeep random-roll column's curated
/// perks. The rest join a private randomized set the socket rolls one plug from.
const REUSABLE_CHOICES: usize = 4;
/// The most choices a column holds: the rows one 64-bit ownership mask covers, also the largest
/// Shadowkeep randomized set.
const ROLL_POOL: usize = 64;
/// Source scores run from 0 to 100. Weapons score 10 in most stats, and mods raise them to 100 at
/// most. A native group without a display maximum is taken to run to 100 as well.
const SOURCE_MAXIMUM: i64 = 100;
const NATIVE_MAXIMUM: i32 = 100;

/// FNV-1 of an identifier. Both games name slots and plug categories this way.
const fn fnv1(name: &str) -> u32 {
    let bytes = name.as_bytes();
    let mut hash = 0x811C_9DC5_u32;
    let mut at = 0;
    while at < bytes.len() {
        hash = hash.wrapping_mul(0x0100_0193) ^ bytes[at] as u32;
        at += 1;
    }
    hash
}

/// Source slots and the Shadowkeep plug categories whose lane presents the same part.
const PARTS: [(u32, &[u32]); 4] = [
    (fnv1("barrel"), &[fnv1("barrels"), fnv1("tubes")]),
    (fnv1("muzzle"), &[fnv1("barrels"), fnv1("tubes")]),
    (fnv1("sight"), &[fnv1("scopes")]),
    (
        fnv1("magazine"),
        &[fnv1("magazines"), fnv1("magazines_gl"), fnv1("batteries")],
    ),
];
/// Slot names the report spells out. The stock slot's name is not known.
const SLOT_NAMES: [&str; 8] = [
    "barrel",
    "chip",
    "foregrip",
    "generator",
    "magazine",
    "muzzle",
    "shield",
    "sight",
];

/// Source chips whose described behavior a Shadowkeep perk performs, by the identity of each
/// grade, with that perk's plug. Every grade of a chip shares one description. Some counterparts
/// differ in trigger or add an effect, as the last four do. The families of these chips hold
/// scripted behavior, so their other mods translate only through a counterpart.
const COUNTERPARTS: [(&[u32], u32); 10] = [
    // Afterburner: sliding reloads part of the magazine and steadies the weapon. Slideshot.
    (
        &[0xB579_AE38, 0xB561_E1BA, 0xB561_E1BB, 0xB561_E1B8],
        0xBC75_7E0C,
    ),
    // Bits per Second: rapid precision hits raise stability and reload speed. Rapid Hit.
    (
        &[0xD580_AB29, 0xCF26_670F, 0xCF26_670E, 0xCF26_670D],
        0x0EC3_FDC8,
    ),
    // Background Process: the stowed weapon reloads itself. Auto-Loading Holster.
    (
        &[0xC3DF_45A9, 0x5BB8_598F, 0x5BB8_598E, 0x5BB8_598D],
        0xC4BE_7564,
    ),
    // Ghillie Suit: crouching steadies the weapon. Firmly Planted.
    (
        &[0x4492_6F80, 0xDC92_1D4B, 0xDC92_1D4A, 0xDC92_1D4D],
        0x10B7_8E3B,
    ),
    // Hurricane: accuracy while airborne. Icarus Grip.
    (&[0x0A00_B80A], 0x3825_BDC2),
    // Buffer Overflow: precision eliminations speed reloads. Outlaw.
    (
        &[0xEE67_96FF, 0x348D_27C0, 0x348D_27C1, 0x348D_27C6],
        0x45A0_BDD7,
    ),
    // Circuit Tracers: eliminations reload part of the magazine. Subsistence, which also
    // reduces reserves.
    (
        &[0x139B_7850, 0x2478_0572, 0x2478_0573, 0x2478_0570],
        0x6C7E_97E1,
    ),
    // Torch Bug: an eliminated hostile explodes. Firefly, on precision kills.
    (
        &[0x3310_DB99, 0xB110_72BF, 0xB110_72BE, 0xB110_72BD],
        0x5D17_0526,
    ),
    // Stack Overflow: reloading an empty magazine overfills it. Clown Cartridge, on any reload
    // at random.
    (
        &[
            0x6740_DF2E,
            0x4637_185C,
            0x4637_185D,
            0x4637_185E,
            0x4862_ABB0,
            0x976B_7412,
            0x976B_7413,
        ],
        0x882F_1653,
    ),
    // Battle Runner: eliminations raise movement speed. Killing Wind, with range and handling.
    (
        &[0x6649_9A21, 0x5BA6_63D7, 0x6649_9A20, 0x6649_9A23],
        0x9214_10AB,
    ),
];

/// One item's plug record.
#[derive(Clone, Copy)]
struct Plug {
    identity: u32,
    grade: u16,
    family: u16,
    owned: bool,
}

/// A source socket: its type index, the plug it starts with and the mods it accepts on this
/// weapon alone.
struct Socket {
    kind: usize,
    initial: Option<usize>,
    exclusive: Vec<usize>,
}

/// A source socket type: its hash, the mods it accepts and the hash of its slot's name.
struct SocketType {
    hash: u32,
    accepts: Vec<usize>,
    slot: u32,
}

/// A relative resource pointer's target when set, checked against its class word.
fn optional(p: &Payload, at: usize, class: u32) -> Result<Option<usize>> {
    if p.u64(at)? == 0 {
        return Ok(None);
    }
    crate::tiger::shadowkeep::resource(p, at, class).map(Some)
}

fn item_ordinals(p: &Payload, at: usize) -> Result<Vec<usize>> {
    p.array(at, 2, Some(ORDINAL_ROW))?
        .into_iter()
        .map(|row| Ok(usize::from(p.u16(row)?)))
        .collect()
}

fn sockets(definition: &Payload) -> Result<Vec<Socket>> {
    let Some(list) = optional(definition, SOCKETS, SOCKET_LIST)? else {
        return Ok(Vec::new());
    };
    definition
        .array(list, SOCKET_SIZE, Some(SOCKET_ROW))?
        .into_iter()
        .map(|row| {
            let initial = definition.u16(row + INITIAL)?;
            Ok(Socket {
                kind: usize::from(definition.u16(row)?),
                initial: (initial != u16::MAX).then_some(usize::from(initial)),
                exclusive: item_ordinals(definition, row + EXCLUSIVE)?,
            })
        })
        .collect()
}

/// The slot's name where it is known, else its hash.
fn slot_name(slot: u32) -> Value {
    SLOT_NAMES
        .iter()
        .find(|name| fnv1(name) == slot)
        .map_or_else(|| json!(format!("{slot:08X}")), |name| json!(name))
}

/// A named mod: its family, text and definitions, one per grade, highest grade first.
struct Named {
    name: String,
    description: String,
    family: u16,
    ordinals: Vec<usize>,
}

/// The source item table and socket types, with plug records and item strings read as needed.
struct Catalog {
    items: Arc<Payload>,
    rows: Vec<usize>,
    ordinals: BTreeMap<u32, usize>,
    plugs: BTreeMap<usize, Option<Plug>>,
    types: Vec<SocketType>,
    scripted: BTreeSet<u16>,
    labels: BTreeMap<u32, u32>,
    strings: super::super::browse::Strings,
}

impl Catalog {
    fn read(r: &mut Reader) -> Result<Self> {
        let items = table(r, ITEMS)?;
        let rows = items.array(8, 32, Some(ITEM_ROW))?;
        let mut ordinals = BTreeMap::new();
        for (ordinal, &row) in rows.iter().enumerate() {
            ordinals.entry(items.u32(row)?).or_insert(ordinal);
        }
        let socket_types = table(r, SOCKET_TYPES)?;
        let types: Vec<SocketType> = socket_types
            .array(8, SOCKET_TYPE_SIZE, Some(SOCKET_TYPE_ROW))?
            .into_iter()
            .map(|row| {
                Ok(SocketType {
                    hash: socket_types.u32(row)?,
                    accepts: item_ordinals(&socket_types, row + ACCEPTS)?,
                    slot: socket_types.u32(row + SLOT)?,
                })
            })
            .collect::<Result<_>>()?;
        let strings = table(r, ITEM_STRINGS)?;
        let mut labels = BTreeMap::new();
        for row in strings.array(8, 32, None)? {
            labels.insert(
                strings.u32(row)?,
                r.ref64(&strings, row + 0x10).unwrap_or(u32::MAX),
            );
        }
        let mut catalog = Self {
            items,
            rows,
            ordinals,
            plugs: BTreeMap::new(),
            types,
            scripted: BTreeSet::new(),
            labels,
            strings: super::super::browse::Strings::new(r)?,
        };
        for &identity in COUNTERPARTS.iter().flat_map(|(tiers, _)| tiers.iter()) {
            if let Some(&ordinal) = catalog.ordinals.get(&identity)
                && let Some(plug) = catalog.plug(r, ordinal)?
            {
                catalog.scripted.insert(plug.family);
            }
        }
        Ok(catalog)
    }

    fn definition(&self, r: &mut Reader, ordinal: usize) -> Result<Arc<Payload>> {
        let row = *self
            .rows
            .get(ordinal)
            .context("Item ordinal outside the table")?;
        r.tag(self.items.u32(row + 0x10)?, Some(ITEM_DEFINITION))
    }

    /// The plug record of the item at `ordinal`, none for an item that is not a plug.
    fn plug(&mut self, r: &mut Reader, ordinal: usize) -> Result<Option<Plug>> {
        if let Some(&plug) = self.plugs.get(&ordinal) {
            return Ok(plug);
        }
        let row = *self
            .rows
            .get(ordinal)
            .context("Item ordinal outside the table")?;
        let tag = self.items.u32(row + 0x10)?;
        let mut plug = None;
        if r.reference(tag).ok() == Some(ITEM_DEFINITION) {
            let definition = r.tag(tag, Some(ITEM_DEFINITION))?;
            if let Some(record) = optional(&definition, PLUG, PLUG_RECORD)? {
                plug = Some(Plug {
                    identity: self.items.u32(row)?,
                    grade: definition.u16(record + PLUG_GRADE)?,
                    family: definition.u16(record + PLUG_FAMILY)?,
                    owned: definition.u32(record + PLUG_OWNED)? != 0,
                });
            }
        }
        self.plugs.insert(ordinal, plug);
        Ok(plug)
    }

    /// A label of the item's strings record, without the source's private-use icon glyphs.
    fn text(&mut self, r: &mut Reader, identity: u32, field: usize) -> Result<String> {
        let Some(&tag) = self.labels.get(&identity).filter(|&&tag| tag != u32::MAX) else {
            return Ok(String::new());
        };
        let record = r.tag(tag, None)?;
        let text = self.strings.label(r, &record, field)?.unwrap_or_default();
        let text = text
            .chars()
            .filter(|c| !('\u{E000}'..='\u{F8FF}').contains(c))
            .collect::<String>();
        Ok(text
            .lines()
            .map(str::trim)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_owned())
    }

    /// The stat deltas of the definition at `ordinal`, empty when it has none.
    fn deltas(&self, r: &mut Reader, ordinal: usize) -> Result<Vec<Value>> {
        let definition = self.definition(r, ordinal)?;
        if optional(&definition, STAT_DELTAS, STAT_BLOCK)?.is_none() {
            return Ok(Vec::new());
        }
        stats(r, &definition)
    }

    fn summary(&mut self, r: &mut Reader, ordinal: usize) -> Result<Value> {
        let Some(plug) = self.plug(r, ordinal)? else {
            return Ok(json!({"item_ordinal":ordinal}));
        };
        Ok(
            json!({"item":format!("{:08X}",plug.identity),"name":self.text(r, plug.identity, NAME)?,
            "family":plug.family,"grade":plug.grade,"weapon_specific":plug.owned}),
        )
    }

    /// The plugs among `ordinals`, one entry per name and family, in item order.
    fn named(&mut self, r: &mut Reader, ordinals: &BTreeSet<usize>) -> Result<Vec<Named>> {
        let mut named: Vec<Named> = Vec::new();
        let mut grades = BTreeMap::new();
        for &ordinal in ordinals {
            let Some(plug) = self.plug(r, ordinal)? else {
                continue;
            };
            grades.insert(ordinal, plug.grade);
            let name = self.text(r, plug.identity, NAME)?;
            // Grades of one mod share its name and family. Unnamed definitions stay apart.
            if let Some(entry) = named
                .iter_mut()
                .find(|entry| !name.is_empty() && entry.name == name && entry.family == plug.family)
            {
                entry.ordinals.push(ordinal);
                continue;
            }
            named.push(Named {
                description: self.text(r, plug.identity, DESCRIPTION)?,
                name,
                family: plug.family,
                ordinals: vec![ordinal],
            });
        }
        for entry in &mut named {
            entry.ordinals.sort_by_key(|ordinal| {
                (
                    std::cmp::Reverse(grades.get(ordinal).copied().unwrap_or_default()),
                    *ordinal,
                )
            });
        }
        Ok(named)
    }
}

/// A stat identity stored as a JSON number.
fn stat_of(value: &Value) -> u32 {
    value
        .as_u64()
        .and_then(|hash| u32::try_from(hash).ok())
        .unwrap_or_default()
}

/// What a mod's deltas are measured against: the source weapon's scores, stat curves and matched
/// effects, the donor's values and the source stats the import holds at fixed outputs.
struct Effects<'a> {
    donor: u32,
    curves: Option<(Payload, usize)>,
    base: &'a [Value],
    matched: Vec<Value>,
    omitted: Vec<Value>,
    definitions: &'a BTreeMap<u32, u16>,
    values: BTreeMap<u16, i32>,
    maximum: i32,
    held: BTreeSet<u32>,
}

/// The donor's investment values by stat definition index, with the values the import gives the
/// weapon in their place.
fn values(native: &mut Reader, donor: u32, gameplay: &Value) -> Result<BTreeMap<u16, i32>> {
    use sundial::package_authoring::investment_schema::{
        ITEM_INVESTMENT_STAT_POINTER_OFFSET, ITEM_INVESTMENT_STAT_RESOURCE_CLASS,
        ITEM_INVESTMENT_STAT_ROW_CLASS, ITEM_INVESTMENT_STAT_ROW_SIZE,
    };
    let tag = crate::tiger::shadowkeep::item_tag(native, donor)?;
    let item = native.tag(tag, Some(NATIVE_ITEM))?;
    let mut values = BTreeMap::new();
    if let Some(resource) = optional(
        &item,
        ITEM_INVESTMENT_STAT_POINTER_OFFSET,
        ITEM_INVESTMENT_STAT_RESOURCE_CLASS,
    )? && item.bytes::<16>(resource)? != [0; 16]
    {
        for row in item.array(
            resource,
            ITEM_INVESTMENT_STAT_ROW_SIZE,
            Some(ITEM_INVESTMENT_STAT_ROW_CLASS),
        )? {
            values.insert(
                u16::from(item.u8(row)?),
                i32::from_le_bytes(item.bytes(row + 4)?),
            );
        }
    }
    for row in gameplay["investment_stats"]
        .as_array()
        .into_iter()
        .flatten()
    {
        values.insert(
            u16::try_from(hash(row, "definition_index")?)?,
            i32::try_from(row["value"].as_i64().context("Imported stat value")?)?,
        );
    }
    Ok(values)
}

/// Source stats whose outputs the import writes as fixed values: rate of fire and magazine as
/// component inputs and magnification as the aim. A plug's investment bonus does not resample
/// them.
fn held(gameplay: &Value) -> BTreeSet<u32> {
    let components = gameplay["inputs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|input| input["component"].as_u64())
        .collect::<BTreeSet<_>>();
    gameplay["sampled"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| match row["input"].as_u64() {
            Some(0) => components.contains(&2),
            Some(1) => components.contains(&1),
            Some(3) => !gameplay["aim"].is_null(),
            _ => false,
        })
        .map(|row| stat_of(&row["stat"]))
        .collect()
}

/// The change that moves the donor's `value` as far through the native range as `delta` moves the
/// source score `base` through the source range: a rise by its share of the room above the
/// score, a fall by its share of the score.
fn relative(base: i64, delta: i64, value: i32, maximum: i32) -> i32 {
    let (share, room) = if delta > 0 {
        if base >= SOURCE_MAXIMUM {
            return 0;
        }
        (
            delta as f64 / (SOURCE_MAXIMUM - base) as f64,
            maximum - value,
        )
    } else {
        if base <= 0 {
            return 0;
        }
        (delta as f64 / base as f64, value)
    };
    (share.clamp(-1., 1.) * f64::from(room.max(0))).round() as i32
}

impl Effects<'_> {
    fn score(&self, stat: u32) -> Option<i64> {
        self.base
            .iter()
            .find(|row| row["hash"].as_u64() == Some(u64::from(stat)) && row["literal"] == true)
            .and_then(|row| row["value"].as_i64())
    }

    /// The matched donor values with every effect-matched delta added to the weapon's scores,
    /// and the reasons for stats left unmatched. None without curves or such a delta.
    fn moved(&self, native: &mut Reader, deltas: &[&Value]) -> Option<(Vec<Value>, Vec<Value>)> {
        let (curves, group) = self.curves.as_ref()?;
        let mut shifted = self.base.to_vec();
        let mut moved = false;
        for delta in deltas {
            let stat = stat_of(&delta["hash"]);
            if delta["literal"] != true || !translator::translates(stat) {
                continue;
            }
            if let Some(row) = shifted
                .iter_mut()
                .find(|row| row["hash"].as_u64() == Some(u64::from(stat)))
            {
                row["value"] = json!(
                    row["value"].as_i64().unwrap_or_default()
                        + delta["value"].as_i64().unwrap_or_default()
                );
                moved = true;
            }
        }
        moved.then(|| {
            let (_, after, omitted) =
                translator::matched(native, self.donor, curves, *group, &shifted);
            (after, omitted)
        })
    }

    /// The donor value matched at the weapon's score plus the delta, less the one matched at its
    /// own score.
    fn matched_change(
        &self,
        stat: u32,
        (moved, omitted): &(Vec<Value>, Vec<Value>),
    ) -> std::result::Result<(u16, i32), String> {
        let find = |rows: &[Value]| {
            rows.iter()
                .find(|row| row["stat"].as_u64() == Some(u64::from(stat)))
                .cloned()
        };
        let (Some(before), Some(after)) = (find(&self.matched), find(moved)) else {
            return Err(find(omitted)
                .or_else(|| find(&self.omitted))
                .and_then(|row| row["reason"].as_str().map(str::to_owned))
                .unwrap_or_else(|| "The effect could not be matched".into()));
        };
        let target = stat_of(&after["native"]);
        let index = *self
            .definitions
            .get(&target)
            .ok_or_else(|| format!("Native stat {target:08X} has no definition"))?;
        let change = after["matched"].as_i64().unwrap_or_default()
            - before["matched"].as_i64().unwrap_or_default();
        match i32::try_from(change) {
            Ok(0) => Err("The matched native value does not change".into()),
            Ok(change) => Ok((index, change)),
            Err(error) => Err(error.to_string()),
        }
    }

    /// The change [`relative`] gives the stat Shadowkeep shares by identity.
    fn relative_change(&self, stat: u32, delta: &Value) -> std::result::Result<(u16, i32), String> {
        let index = *self
            .definitions
            .get(&stat)
            .ok_or_else(|| "Shadowkeep has no stat with this identity".to_owned())?;
        let value = *self
            .values
            .get(&index)
            .ok_or_else(|| "The donor carries no value for this stat".to_owned())?;
        let base = self
            .score(stat)
            .ok_or_else(|| "The source weapon has no score for this stat".to_owned())?;
        let delta = delta["value"].as_i64().unwrap_or_default();
        match relative(base, delta, value, self.maximum) {
            0 => Err("The donor's value has no room to move".into()),
            change => Ok((index, change)),
        }
    }

    fn contribution(
        &self,
        stat: u32,
        delta: &Value,
        moved: Option<&(Vec<Value>, Vec<Value>)>,
    ) -> std::result::Result<(u16, i32), String> {
        if delta["literal"] != true {
            return Err("The delta is not a literal value".into());
        }
        if self.held.contains(&stat) {
            return Err(
                "The import writes this stat's source output as a fixed value, which no plug bonus changes"
                    .into(),
            );
        }
        let matched = match moved {
            Some(moved) if translator::translates(stat) => self.matched_change(stat, moved),
            _ => Err("The games' curves are not matched for this stat".into()),
        };
        matched.or_else(|reason| {
            self.relative_change(stat, delta)
                .map_err(|other| format!("{reason}. {other}"))
        })
    }

    /// Each nonzero delta's native contribution, summed by native stat, and why each other delta
    /// has none. A stat both games match by effect contributes the change in its matched donor
    /// value. A stat Shadowkeep shares by identity without a matched effect moves by
    /// [`relative`]. Stats the import holds at fixed outputs and stats without a native
    /// counterpart are left off.
    fn contributions(&self, native: &mut Reader, deltas: &[Value]) -> (Vec<Value>, Vec<Value>) {
        let deltas = deltas
            .iter()
            .filter(|delta| delta["value"].as_i64().is_some_and(|value| value != 0))
            .collect::<Vec<_>>();
        let moved = self.moved(native, &deltas);
        let mut rows = BTreeMap::<u16, i32>::new();
        let mut dropped = Vec::new();
        for delta in deltas {
            let stat = stat_of(&delta["hash"]);
            match self.contribution(stat, delta, moved.as_ref()) {
                Ok((index, change)) => *rows.entry(index).or_default() += change,
                Err(reason) => dropped.push(
                    json!({"stat":format!("{stat:08X}"),"value":delta["value"],"reason":reason}),
                ),
            }
        }
        let rows = rows
            .into_iter()
            .filter(|&(_, value)| value != 0)
            .map(|(index, value)| json!({"definition_index":index,"value":value}))
            .collect();
        (rows, dropped)
    }
}

/// A translated choice: the stock plug that lends its effects, or none for a stat mod, which
/// borrows its lane's plug layout and replaces its effects.
struct Choice {
    counterpart: Option<u32>,
    variant: Value,
    report: Value,
}

/// The private plug a named mod becomes, or why it does not. Only the mod's highest grade is
/// offered, since lower grades repeat it with weaker deltas. A mod of a scripted family needs a
/// counterpart, whose plug lends its effects. Any other mod needs a delta with a native effect.
/// Deltas without one are reported.
fn choose(
    r: &mut Reader,
    native: &mut Reader,
    catalog: &mut Catalog,
    effects: &Effects<'_>,
    named: &Named,
    glyph: Option<&str>,
) -> Result<std::result::Result<Choice, Value>> {
    let ordinal = *named.ordinals.first().context("Named mod definitions")?;
    let plug = catalog.plug(r, ordinal)?.context("Mod plug record")?;
    let item = format!("{:08X}", plug.identity);
    let counterpart = COUNTERPARTS
        .iter()
        .find(|(tiers, _)| tiers.contains(&plug.identity))
        .map(|&(_, counterpart)| counterpart);
    let rejected = |reason: &str, dropped: &[Value]| {
        json!({"item":item,"name":named.name,"family":named.family,"grade":plug.grade,
            "reason":reason,"dropped_deltas":dropped})
    };
    if named.name.is_empty() {
        return Ok(Err(rejected("The mod has no name", &[])));
    }
    if counterpart.is_none() && catalog.scripted.contains(&named.family) {
        return Ok(Err(rejected(
            "Scripted chip behavior without a Shadowkeep counterpart",
            &[],
        )));
    }
    let deltas = catalog.deltas(r, ordinal)?;
    let (stats, dropped) = effects.contributions(native, &deltas);
    let icon =
        glyph.map(|png| json!({"source":"image","name":named.name,"image":{"png_base64":png}}));
    let Some(counterpart) = counterpart else {
        if stats.is_empty() {
            let reason = if dropped.is_empty() {
                "No stat deltas, and scripted behavior has no stored program"
            } else {
                "No stat delta has a native effect"
            };
            return Ok(Err(rejected(reason, &dropped)));
        }
        let mut variant = json!({"name":named.name,"description":named.description,
            "replace_effects":true,"investment_stats":stats,"sandbox_perks":[]});
        if let Some(icon) = icon {
            variant["icon"] = icon;
        }
        return Ok(Ok(Choice {
            counterpart: None,
            variant,
            report: json!({"item":item,"name":named.name,"grade":plug.grade,
                "kind":"stat_mod","investment_stats":stats,"dropped_deltas":dropped}),
        }));
    };
    let tag = crate::tiger::shadowkeep::item_tag(native, counterpart)
        .with_context(|| format!("Stock perk plug {counterpart:08X}"))?;
    let stock = native.tag(tag, Some(NATIVE_ITEM))?;
    let category = crate::tiger::shadowkeep::plug_category(&stock)?;
    let perks = sundial::package_authoring::native_weapon::base_sandbox_perks(&stock.0)
        .map_err(anyhow::Error::msg)?;
    if perks.is_empty() {
        return Ok(Err(rejected(
            "The counterpart plug has no sandbox perk",
            &dropped,
        )));
    }
    // A private copy of the counterpart's plug runs every perk the stock plug lists, unedited,
    // and keeps its description, so the choice states what it does. The name, the slot's glyph
    // and the deltas with a native effect are the source's. The effect list is authored
    // complete, since a variant that inherits its effects must name an edited perk.
    let mut variant = json!({"name":named.name,"replace_effects":true,
        "additional_sandbox_perks":perks,"sandbox_perks":[]});
    if !stats.is_empty() {
        variant["investment_stats"] = json!(stats);
    }
    if let Some(icon) = icon {
        variant["icon"] = icon;
    }
    Ok(Ok(Choice {
        counterpart: Some(counterpart),
        variant,
        report: json!({"item":item,"name":named.name,"grade":plug.grade,"kind":"chip",
            "counterpart":format!("{counterpart:08X}"),"category":category,"stock_perks":perks,
            "source_description":named.description,"investment_stats":stats,
            "dropped_deltas":dropped}),
    }))
}

/// A translated source socket: its position, slot and choices.
struct Column {
    position: usize,
    slot: u32,
    choices: Vec<Choice>,
}

/// A mod slot's socket type, plug set and the positions it takes.
type ModSlot = (u32, u32, BTreeSet<usize>);

/// The socket's survey row, with the socket type when it is a mod slot.
fn survey_socket(
    r: &mut Reader,
    catalog: &mut Catalog,
    position: usize,
    socket: &Socket,
) -> Result<(Value, Option<ModSlot>)> {
    let initial = socket
        .initial
        .map(|ordinal| catalog.summary(r, ordinal))
        .transpose()?;
    let exclusive = socket
        .exclusive
        .iter()
        .map(|&ordinal| catalog.summary(r, ordinal))
        .collect::<Result<Vec<_>>>()?;
    let mut row = json!({"position":position,"socket_type":socket.kind,"initial":initial,
        "exclusive":exclusive});
    let Some(kind) = catalog
        .types
        .get(socket.kind)
        .filter(|kind| kind.slot != NO_SLOT)
    else {
        row["mod_slot"] = json!(false);
        return Ok((row, None));
    };
    row["slot"] = slot_name(kind.slot);
    row["accepts"] = json!(kind.accepts.len());
    // The weapon's own mods join the ones every socket of this type accepts.
    let candidates = kind
        .accepts
        .iter()
        .chain(&socket.exclusive)
        .chain(&socket.initial)
        .copied()
        .collect();
    Ok((row, Some((kind.hash, kind.slot, candidates))))
}

/// Each mod socket's choices, in source socket order, with the survey of every socket. The mod a
/// socket starts with leads its choices, then the weapon's own mods, then the rest in item order.
#[expect(
    clippy::too_many_arguments,
    reason = "The readers, catalog, glyphs and accumulators are separate borrows."
)]
fn columns(
    r: &mut Reader,
    native: &mut Reader,
    catalog: &mut Catalog,
    glyphs: &mut icon::Glyphs,
    effects: &Effects<'_>,
    weapon: &[Socket],
    survey: &mut Vec<Value>,
    unsupported: &mut Vec<Value>,
) -> Result<Vec<Column>> {
    let mut columns = Vec::new();
    for (position, socket) in weapon.iter().enumerate() {
        let (mut row, kind) = survey_socket(r, catalog, position, socket)?;
        let Some((hash, slot, candidates)) = kind else {
            survey.push(row);
            continue;
        };
        let icon = glyphs.png(r, hash);
        row["glyph"] = icon
            .as_ref()
            .map_or_else(|reason| json!(reason), |_| json!(true));
        survey.push(row);
        let mut named = catalog.named(r, &candidates)?;
        let lead = |entry: &Named| {
            if socket
                .initial
                .is_some_and(|ordinal| entry.ordinals.contains(&ordinal))
            {
                0
            } else if socket
                .exclusive
                .iter()
                .any(|ordinal| entry.ordinals.contains(ordinal))
            {
                1
            } else {
                2
            }
        };
        named.sort_by_key(|entry| (lead(entry), entry.ordinals.iter().min().copied()));
        let glyph = icon.as_deref().ok();
        let mut choices = Vec::new();
        for entry in &named {
            match choose(r, native, catalog, effects, entry, glyph)? {
                Ok(mut choice) => {
                    choice.report["socket"] = json!(position);
                    choices.push(choice);
                }
                Err(mut report) => {
                    report["socket"] = json!(position);
                    unsupported.push(report);
                }
            }
        }
        if !choices.is_empty() {
            columns.push(Column {
                position,
                slot,
                choices,
            });
        }
    }
    Ok(columns)
}

/// A native perk socket: its lane, socket type, plugs with the default first, and the plug
/// categories it offers.
#[derive(Clone)]
struct Lane {
    index: usize,
    socket_type: u64,
    plugs: Vec<u32>,
    categories: BTreeSet<u64>,
}

/// The donor's perk sockets in socket order: lanes whose every plug is an ordinary perk without
/// art of its own. The intrinsic frame keeps describing the donor firing the import retains.
fn lanes(native: &Value) -> Result<Vec<Lane>> {
    let mut lanes = Vec::new();
    for socket in native["sockets"].as_array().context("Native sockets")? {
        let choices = socket["choices"]
            .as_array()
            .context("Native socket choices")?;
        let perks = choices.iter().all(|choice| {
            choice["item_kind"].as_u64() == Some(PERK_ITEM_KIND)
                && choice["art_indices"].as_array().is_some_and(Vec::is_empty)
        });
        if socket["socket_type"] == DISABLED || choices.is_empty() || !perks {
            continue;
        }
        let default = choices
            .iter()
            .find(|choice| choice["default"] == true)
            .or_else(|| {
                choices
                    .iter()
                    .find(|choice| choice["hash"] == socket["curated_fallback"])
            })
            .unwrap_or(&choices[0]);
        if default["category"].as_u64() == Some(INTRINSIC_CATEGORY) {
            continue;
        }
        let mut plugs = Vec::new();
        for choice in std::iter::once(default).chain(choices) {
            let hash = u32::try_from(choice["hash"].as_u64().context("Native plug identity")?)?;
            if !plugs.contains(&hash) {
                plugs.push(hash);
            }
        }
        lanes.push(Lane {
            index: usize::try_from(socket["index"].as_u64().context("Native socket index")?)?,
            socket_type: socket["socket_type"]
                .as_u64()
                .context("Native socket type")?,
            plugs,
            categories: choices
                .iter()
                .filter_map(|choice| choice["category"].as_u64())
                .collect(),
        });
    }
    Ok(lanes)
}

/// Whether `lane` can hold `choice`. A stat mod borrows any lane's layout. A counterpart's plug
/// needs a lane that offers its category.
fn fits(lane: &Lane, choice: &Choice) -> bool {
    choice.counterpart.is_none()
        || choice.report["category"]
            .as_u64()
            .is_some_and(|category| lane.categories.contains(&category))
}

/// Lanes beyond the donor's, laid out as its last perk lane, up to the sockets a weapon holds.
/// Lanes the weapon's camos are planned to append are left to them.
struct Added {
    template: Option<Lane>,
    next: usize,
    reserved: BTreeSet<usize>,
}

impl Added {
    /// The index the next added lane takes, while the weapon can hold it.
    fn index(&self) -> Option<usize> {
        let mut next = self.next;
        while self.reserved.contains(&next) {
            next += 1;
        }
        (next < sundial::investment::MAX_WEAPON_SOCKETS).then_some(next)
    }
}

/// The free lane that presents the same part as `slot`.
fn part_lane(free: &[Lane], slot: u32) -> Option<usize> {
    let (_, categories) = PARTS.iter().find(|(part, _)| *part == slot)?;
    free.iter().position(|lane| {
        categories
            .iter()
            .any(|&category| lane.categories.contains(&u64::from(category)))
    })
}

/// The lane for `column`: the free lane that holds the most of its choices, the earliest on a
/// tie, and an added lane only when that holds more.
fn take(free: &mut Vec<Lane>, added: &mut Added, column: &Column) -> Option<Lane> {
    let holds = |lane: &Lane| column.choices.iter().filter(|c| fits(lane, c)).count();
    let best = free
        .iter()
        .enumerate()
        .map(|(at, lane)| (holds(lane), at))
        .filter(|&(count, _)| count > 0)
        .max_by_key(|&(count, at)| (count, std::cmp::Reverse(at)));
    let extra = added
        .template
        .as_ref()
        .filter(|_| added.index().is_some())
        .map_or(0, holds);
    match best {
        Some((count, at)) if count >= extra => Some(free.remove(at)),
        _ if extra > 0 => {
            let mut lane = added.template.clone()?;
            lane.index = added.index()?;
            added.next = lane.index + 1;
            Some(lane)
        }
        _ => None,
    }
}

/// The column `column` makes on `lane`, with each choice's report and private plug. A column
/// offers its first four choices freely and rolls from all of them, at most 64.
fn fill(
    lane: &Lane,
    column: Column,
    unsupported: &mut Vec<Value>,
) -> Option<(Value, Vec<(Value, Value)>)> {
    let position = column.position;
    let (mut choices, rejected): (Vec<_>, Vec<_>) = column
        .choices
        .into_iter()
        .partition(|choice| fits(lane, choice));
    for choice in rejected {
        unsupported.push(json!({"socket":position,"name":choice.report["name"],
            "reason":"The column's native socket does not offer this counterpart's plug category"}));
    }
    for choice in choices.drain(choices.len().min(ROLL_POOL)..) {
        unsupported.push(json!({"socket":position,"name":choice.report["name"],
            "reason":"The column's roll pool is full"}));
    }
    if choices.is_empty() {
        return None;
    }
    let mut plugs = Vec::new();
    let mut translated = Vec::new();
    for (choice_index, mut choice) in choices.into_iter().enumerate() {
        let plug = choice
            .counterpart
            .unwrap_or(lane.plugs[choice_index % lane.plugs.len()]);
        plugs.push(format!("0x{plug:08X}"));
        choice.variant["socket_index"] = json!(lane.index);
        choice.variant["choice_index"] = json!(choice_index);
        choice.variant["source_plug_hash"] = json!(format!("0x{plug:08X}"));
        choice.report["lane"] = json!(lane.index);
        choice.report["choice"] = json!(choice_index);
        choice.report["template_plug"] = json!(format!("{plug:08X}"));
        choice.report["reusable"] = json!(choice_index < REUSABLE_CHOICES);
        translated.push((choice.report, choice.variant));
    }
    let count = plugs.len();
    let mut placed = json!({"choices":plugs,"socket_type":lane.socket_type});
    // A source socket holds one mod at a time, so the socket rolls one plug from the choices
    // past the reusable ones.
    if count > REUSABLE_CHOICES {
        placed["reusable_choices"] = json!(REUSABLE_CHOICES);
        placed["randomized_selection_program"] = json!([{"opcode":11,"operand":1}]);
    }
    Some((placed, translated))
}

/// Placed columns by lane, and each choice's report and private plug.
#[derive(Default)]
struct Placed {
    columns: Vec<(usize, Value)>,
    choices: Vec<(Value, Value)>,
}

impl Placed {
    fn put(&mut self, lane: &Lane, column: Column, unsupported: &mut Vec<Value>) {
        if let Some((column, choices)) = fill(lane, column, unsupported) {
            self.columns.push((lane.index, column));
            self.choices.extend(choices);
        }
    }
}

/// Place each column on a donor perk lane. A part takes the free lane that presents the same
/// part. Columns with counterparts choose next, since their plugs need a lane's categories, then
/// the rest in source socket order. Columns left over get added lanes, past any lane in
/// `reserved`.
fn place(
    columns: Vec<Column>,
    free: &mut Vec<Lane>,
    (sockets, reserved): (usize, &BTreeSet<usize>),
    unsupported: &mut Vec<Value>,
) -> Placed {
    let mut added = Added {
        template: free.last().cloned(),
        next: sockets,
        reserved: reserved.clone(),
    };
    let mut placed = Placed::default();
    let mut rest = Vec::new();
    for column in columns {
        match part_lane(free, column.slot) {
            Some(at) => placed.put(&free.remove(at), column, unsupported),
            None => rest.push(column),
        }
    }
    rest.sort_by_key(|column| column.choices.iter().all(|c| c.counterpart.is_none()));
    for column in rest {
        match take(free, &mut added, &column) {
            Some(lane) => placed.put(&lane, column, unsupported),
            None => {
                for choice in column.choices {
                    unsupported.push(json!({"socket":column.position,"name":choice.report["name"],
                        "reason":"No native perk socket can hold this choice, and the weapon holds no further socket"}));
                }
            }
        }
    }
    placed.columns.sort_by_key(|&(lane, _)| lane);
    placed
}

/// Translate the source weapon `identity`'s mod sockets onto the donor's perk sockets, recorded
/// in `graph` as `source_perks`. Once a column is placed, the donor's other perk lanes are left
/// without choices so its own perks do not stand in for the source's. Lanes added past the
/// donor's skip the lanes in `reserved`, which the weapon's camos are planned to take. The survey
/// reads source tables through an in-memory reader, so the export keeps only the weapon's own
/// tags.
pub fn author_perks(
    source: &mut Reader,
    native: &mut Reader,
    graph: &mut Value,
    identity: u32,
    reserved: &BTreeSet<usize>,
) -> Result<()> {
    let r = &mut source.shared_discovery();
    let mut catalog = Catalog::read(r)?;
    let mut glyphs = icon::Glyphs::read(r)?;
    let ordinal = *catalog
        .ordinals
        .get(&identity)
        .context("Source weapon identity")?;
    let definition = catalog.definition(r, ordinal)?;
    let weapon = sockets(&definition)?;
    let base = stats(r, &definition)?;
    let donor = hash(graph, "native_item")?;
    let root = hash(
        graph["source_assembly"]
            .as_array()
            .and_then(|parts| parts.first())
            .context("Empty source gameplay assembly")?,
        "entity",
    )?;
    let curves = curves(r, root)?;
    let definitions = crate::tiger::shadowkeep::stat_definitions(native)?;
    let (_, matched, omitted) = match &curves {
        Some((p, group)) => translator::matched(native, donor, p, *group, &base),
        None => (BTreeMap::new(), Vec::new(), Vec::new()),
    };
    let gameplay = &graph["source_gameplay"];
    let effects = Effects {
        donor,
        curves,
        base: &base,
        matched,
        omitted,
        definitions: &definitions,
        values: values(native, donor, gameplay)?,
        maximum: gameplay["display_group"]["maximum_value"]
            .as_i64()
            .and_then(|maximum| i32::try_from(maximum).ok())
            .unwrap_or(NATIVE_MAXIMUM),
        held: held(gameplay),
    };
    let mut survey = Vec::new();
    let mut unsupported = Vec::new();
    let found = columns(
        r,
        native,
        &mut catalog,
        &mut glyphs,
        &effects,
        &weapon,
        &mut survey,
        &mut unsupported,
    )?;
    let report = crate::tiger::shadowkeep::sockets(native, donor)?;
    let count = report["sockets"].as_array().map_or(0, Vec::len);
    let mut free = lanes(&report)?;
    let Placed {
        columns: placed,
        choices: translated,
    } = place(found, &mut free, (count, reserved), &mut unsupported);
    let mut columns = Value::Null;
    let mut owned = Vec::new();
    let mut native_defaults = Vec::new();
    let mut skipped = Vec::new();
    if !placed.is_empty() {
        let width = placed
            .iter()
            .map(|&(lane, _)| lane + 1)
            .fold(count, usize::max);
        skipped.extend(reserved.range(count..width).copied());
        let mut lanes = vec![Value::Null; width];
        for lane in &free {
            lanes[lane.index] = json!({"choices":[],"socket_type":DISABLED});
            owned.push(lane.index);
        }
        for (index, column) in placed {
            lanes[index] = column;
            owned.push(index);
        }
        columns = json!(lanes);
        crate::tiger::shadowkeep::fill_native_defaults(
            &report,
            &mut columns,
            &mut native_defaults,
        )?;
    }
    owned.sort_unstable();
    let (translated, variants): (Vec<_>, Vec<_>) = translated.into_iter().unzip();
    let status = match (translated.is_empty(), unsupported.is_empty()) {
        (true, _) => "unsupported",
        (false, true) => "translated",
        (false, false) => "partial",
    };
    graph["source_perks"] = json!({"source":"marathon","revision":2,"sockets":survey,
        "columns":columns,"owned_lanes":owned,"reserved_lanes":skipped,"variants":variants,
        "translated":translated,
        "unsupported":unsupported,"native_defaults":native_defaults,"status":status,
        "limits":"A socket offers the mods its socket type accepts and the weapon's own mods. Chips with a Shadowkeep counterpart run that perk under their source name. Other mods become private plugs when a stat delta has a native effect: a matched effect, or a stat Shadowkeep shares by identity moved by its share of the source range, which is an approximation. Deltas without either, and magazine, rate of fire and zoom deltas the import holds fixed, are left off. Scripted chips without a counterpart are not translated. Gameplay is unverified."});
    Ok(())
}

/// Place the translated columns in a fresh import's recipe. The translation owns the donor's
/// perk lanes and any it adds, so it replaces whatever those held, and fills the native defaults
/// of other lanes only where the recipe sets none. Variants of owned lanes are replaced.
pub fn apply_perks(recipe: &mut Value, graph: &Value) -> Result<()> {
    let perks = &graph["source_perks"];
    let Some(columns) = perks["columns"].as_array() else {
        return Ok(());
    };
    let owned = perks["owned_lanes"]
        .as_array()
        .context("Translated perk lanes")?
        .iter()
        .map(|lane| lane.as_u64().context("Perk lane"))
        .collect::<Result<BTreeSet<_>>>()?;
    let mut lanes = recipe["overrides"]["socket_columns"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if lanes.len() < columns.len() {
        lanes.resize(columns.len(), Value::Null);
    }
    for (lane, column) in columns.iter().enumerate() {
        if !column.is_null() && (owned.contains(&(lane as u64)) || lanes[lane].is_null()) {
            lanes[lane] = column.clone();
        }
    }
    let mut variants = recipe["overrides"]["socket_plug_variants"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    variants.retain(|variant| {
        variant["socket_index"]
            .as_u64()
            .is_none_or(|lane| !owned.contains(&lane))
    });
    variants.extend(perks["variants"].as_array().into_iter().flatten().cloned());
    recipe["overrides"]["socket_columns"] = json!(lanes);
    recipe["overrides"]["socket_plug_variants"] = json!(variants);
    Ok(())
}

/// Close the gap a reserved lane leaves when no camo takes it, since a recipe holds no empty lane
/// past the donor's sockets. The added lanes after it move down one, with their variants.
pub fn release_lane(recipe: &mut Value, lane: usize) -> Result<()> {
    let Some(lanes) = recipe["overrides"]["socket_columns"].as_array_mut() else {
        return Ok(());
    };
    if lanes.get(lane).is_none_or(|column| !column.is_null()) {
        return Ok(());
    }
    lanes.remove(lane);
    for variant in recipe["overrides"]["socket_plug_variants"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        if let Some(index) = variant["socket_index"]
            .as_u64()
            .filter(|&index| index > lane as u64)
        {
            variant["socket_index"] = json!(index - 1);
        }
    }
    Ok(())
}
