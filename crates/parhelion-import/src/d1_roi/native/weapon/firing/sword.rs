//! Rise of Iron sword attacks on the base's native sword owner.
//!
//! Both games store a sword's attacks alike: light and heavy attack lists by selection kind, the
//! attack rows of each list, and combo rows that continue a row into further rows. Selection
//! takes the first row whose key is empty or held by the weapon and whose ammunition gate
//! passes, so a source row translates onto the native row selection reaches the same way. Its
//! aim limits, movement, timing, ammunition cost and combo windows replace the native row's.
//! What the row plays and how it delivers damage stay native: its action's presentation, damage
//! profiles and spawned entities have Shadowkeep versions of their own.
//!
//! A keyed source row has no row on an ordinary base. It is built on the Shadowkeep row that
//! plays the same action, with the source's key, gate and mechanics, and placed ahead of the
//! rows the base already selects. The report's edits and appended bytes are relative to the
//! base's sword resource, which the package build applies to a private owner after checking the
//! owner's digest.
//!
//! A keyed source row whose action Shadowkeep has no row for is built from the source: its
//! action becomes a private human action, the entities it spawns and reports with are
//! translated into groups of their own, and its native-only fields start from the stock keyed
//! row whose contact delivers the same way.
use super::*;
use crate::d1_roi::ability::animation;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use sundial::package_authoring::{entity::weapon_component_bindings, runtime};

mod guard;

const BINDING: u32 = 0xCD2B_CEAC;
const EMPTY: u32 = 0x811C_9DC5;
const MARKER: u32 = 0x8080_9FBD;
const ROW: usize = 0x220;

/// Stock sword owners whose heavy rows play the actions Rise of Iron keys: the Adaptive, Caster
/// and Vortex frames. A keyed source row borrows the row of its own action.
const FRAMES: [u32; 3] = [0x8153_34D6, 0x81A6_ABFB, 0x81A6_AC2D];

/// Source row fields and the native fields holding the same value: fourteen aim limits, the
/// movement block, strike time, contact mode and lunge distance.
const FIELDS: [(usize, usize); 25] = [
    (0x0C, 0x28),
    (0x10, 0x2C),
    (0x14, 0x30),
    (0x18, 0x34),
    (0x1C, 0x38),
    (0x20, 0x3C),
    (0x24, 0x40),
    (0x28, 0x44),
    (0x2C, 0x48),
    (0x30, 0x4C),
    (0x34, 0x50),
    (0x38, 0x54),
    (0x3C, 0x58),
    (0x40, 0x5C),
    (0x124, 0x14C),
    (0x12C, 0x154),
    (0x130, 0x158),
    (0x134, 0x15C),
    (0x138, 0x160),
    (0x13C, 0x164),
    (0x140, 0x174),
    (0x144, 0x178),
    (0x168, 0x1A0),
    (0x178, 0x1B0),
    (0x194, 0x204),
];

/// Code and constant descriptors of a row's three scale programs.
const SOURCE_PROGRAMS: [(usize, usize); 3] = [(0x58, 0x68), (0x90, 0xA0), (0xC8, 0xD8)];
const NATIVE_PROGRAMS: [(usize, usize); 3] = [(0x70, 0x80), (0xA8, 0xB8), (0xE0, 0xF0)];
/// Code and constant descriptors of a native combo row's three window programs.
const WINDOWS: [(usize, usize); 3] = [(0x10, 0x20), (0x48, 0x58), (0x80, 0x90)];
/// A native row's byte and constant arrays: count offset, class and stride. The pointer follows
/// each count.
const ROW_ARRAYS: [(usize, u32, usize); 8] = [
    (0x70, 0x8080_0009, 1),
    (0x80, 0x8080_0090, 16),
    (0xA8, 0x8080_0009, 1),
    (0xB8, 0x8080_0090, 16),
    (0xE0, 0x8080_0009, 1),
    (0xF0, 0x8080_0090, 16),
    (0x1C0, 0x8080_0009, 1),
    (0x1D0, 0x8080_0090, 16),
];
/// Every relative pointer word of a native row.
const ROW_POINTERS: [usize; 13] = [
    0x20, 0x78, 0x88, 0xB0, 0xC0, 0xE8, 0xF8, 0x110, 0x120, 0x1A8, 0x1C8, 0x1D8, 0x1F8,
];
/// Source trajectory block fields and their native fields: distance, time, travel and two limits.
const TRAJECTORY: [(usize, usize); 5] = [
    (0x04, 0x20),
    (0x08, 0x30),
    (0x10, 0x3C),
    (0x14, 0x44),
    (0x18, 0x48),
];

/// Bytes appended to the private owner. The build places them on a 16-byte boundary, and every
/// pointer inside them is relative to its own word.
#[derive(Default)]
struct Blob {
    bytes: Vec<u8>,
    /// Pointer words whose targets stay in the base's owner, by owner offset.
    retained: Vec<(usize, usize)>,
    /// Words holding an offset from the start of the owner, with their targets in these bytes.
    absolute: Vec<(usize, usize)>,
}

impl Blob {
    /// Reserves a native array of `count` zeroed rows and returns its header and first row. A
    /// header sits on a 16-byte boundary with its allocation marker in the four bytes before.
    fn array(&mut self, count: usize, class: u32, stride: usize) -> (usize, usize) {
        let header = (self.bytes.len() + 4).next_multiple_of(16);
        self.bytes.resize(header - 4, 0);
        self.bytes.extend_from_slice(&MARKER.to_le_bytes());
        self.bytes.extend_from_slice(&(count as u64).to_le_bytes());
        self.bytes
            .extend_from_slice(&u64::from(class).to_le_bytes());
        let rows = self.bytes.len();
        self.bytes.resize(rows + count * stride, 0);
        (header, rows)
    }

    /// Appends a typed object behind its class word, on an `align`-byte boundary.
    fn typed(&mut self, class: u32, body: &[u8], align: usize) -> usize {
        let at = (self.bytes.len() + 4).next_multiple_of(align);
        self.bytes.resize(at - 4, 0);
        self.bytes.extend_from_slice(&class.to_le_bytes());
        self.bytes.extend_from_slice(body);
        at
    }

    fn point(&mut self, word: usize, target: usize) {
        let delta = target as i64 - word as i64;
        self.bytes[word..word + 8].copy_from_slice(&delta.to_le_bytes());
    }

    fn word(&mut self, at: usize, value: u32) {
        self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
}

/// Where a row's values are written: the base's owner, as edits, or appended bytes.
trait Space {
    fn view(&self) -> Payload;
    fn put(&mut self, at: usize, value: [u8; 4]) -> Result<()>;
}

struct Owner<'a> {
    payload: &'a Payload,
    edits: &'a mut BTreeMap<usize, [u8; 4]>,
}

impl Space for Owner<'_> {
    fn view(&self) -> Payload {
        self.payload.clone()
    }

    fn put(&mut self, at: usize, value: [u8; 4]) -> Result<()> {
        if self.payload.bytes::<4>(at)? == value && !self.edits.contains_key(&at) {
            return Ok(());
        }
        // Two rows sharing one object would each need a value of their own.
        ensure!(
            self.edits.insert(at, value).is_none_or(|old| old == value),
            "Two source sword rows write different values to one native field at {at:X}"
        );
        Ok(())
    }
}

impl Space for Blob {
    fn view(&self) -> Payload {
        Payload(self.bytes.clone())
    }

    fn put(&mut self, at: usize, value: [u8; 4]) -> Result<()> {
        self.bytes
            .get_mut(at..at + 4)
            .context("Sword row field outside the appended bytes")?
            .copy_from_slice(&value);
        Ok(())
    }
}

/// The one constant of a program that only returns it, with its row offset.
fn constant(p: &Payload, code: usize, constants: usize) -> Result<Option<(usize, [u8; 4])>> {
    let code = p
        .array(code, 1, Some(0x8080_0009))?
        .into_iter()
        .map(|at| p.u8(at))
        .collect::<Result<Vec<_>>>()?;
    let rows = p.array(constants, 16, Some(0x8080_0090))?;
    if code != [0x34, 0, 0x3E, 0] || rows.len() != 1 {
        return Ok(None);
    }
    let value = p.bytes::<4>(rows[0])?;
    Ok((1..4)
        .all(|lane| p.bytes::<4>(rows[0] + lane * 4).ok() == Some(value))
        .then_some((rows[0], value)))
}

/// A source row's ammunition gate: the minimum and the amount spent.
fn source_ammo(p: &Payload, row: usize) -> Result<Option<[u8; 8]>> {
    if p.u64(row + 0x100)? == 0 {
        return Ok(None);
    }
    let gate = p.pointer(row + 0x100)?;
    ensure!(
        gate >= 4 && p.u32(gate - 4)? == 0x8080_3195,
        "Source sword row has another eligibility condition"
    );
    Ok(Some(p.bytes::<8>(gate)?))
}

/// The ammunition gate among a native row's conditions.
fn native_ammo(p: &Payload, row: usize) -> Result<Option<usize>> {
    let mut found = None;
    for entry in p.array(row + 0x18, 8, Some(0x8080_2D6E))? {
        let gate = p.pointer(entry)?;
        if p.u32(gate - 4)? == 0x8080_2D70 {
            ensure!(
                found.replace(gate).is_none(),
                "Native sword row has two ammunition gates"
            );
        }
    }
    Ok(found)
}

/// The rows selection can reach. A row is never taken when an earlier row passes whenever it
/// does: one with the same or no key whose gate asks for no more ammunition.
fn reachable(p: &Payload, rows: &[usize]) -> Result<Vec<usize>> {
    let mut kept: Vec<(u32, Option<f32>)> = Vec::new();
    let mut result = Vec::new();
    for &row in rows {
        let key = p.u32(row + 8)?;
        let minimum = source_ammo(p, row)?
            .map(|gate| f32::from_le_bytes([gate[0], gate[1], gate[2], gate[3]]));
        let shadowed = kept.iter().any(|&(earlier, gate)| {
            (earlier == EMPTY || earlier == key)
                && match (gate, minimum) {
                    (None, _) => true,
                    (Some(a), Some(b)) => a <= b,
                    (Some(_), None) => false,
                }
        });
        if !shadowed {
            kept.push((key, minimum));
            result.push(row);
        }
    }
    Ok(result)
}

/// Writes a source row's mechanics over the native row at `n` and returns what stayed native.
fn transfer(space: &mut dyn Space, n: usize, source: &Payload, s: usize) -> Result<Vec<String>> {
    let view = space.view();
    let mut limits = Vec::new();
    for (from, to) in FIELDS {
        space.put(n + to, source.bytes::<4>(s + from)?)?;
    }
    for (index, (from, to)) in SOURCE_PROGRAMS.into_iter().zip(NATIVE_PROGRAMS).enumerate() {
        match (
            constant(source, s + from.0, s + from.1)?,
            constant(&view, n + to.0, n + to.1)?,
        ) {
            (Some((_, value)), Some((at, _))) => {
                for lane in 0..4 {
                    space.put(at + lane * 4, value)?;
                }
            }
            _ => limits.push(format!(
                "Scale program {index} is not one constant in both games and stays native"
            )),
        }
    }
    match (source_ammo(source, s)?, native_ammo(&view, n)?) {
        (Some(gate), Some(at)) => {
            space.put(at, [gate[0], gate[1], gate[2], gate[3]])?;
            space.put(at + 4, [gate[4], gate[5], gate[6], gate[7]])?;
        }
        (None, None) => {}
        _ => anyhow::bail!("Paired sword rows differ in ammunition gating"),
    }
    let from = source
        .array(s + 0x108, 8, Some(0x8080_28AF))?
        .into_iter()
        .map(|entry| source.pointer(entry))
        .collect::<Result<Vec<_>>>()?;
    let to = view
        .array(n + 0x118, 8, Some(0x8080_4446))?
        .into_iter()
        .map(|entry| view.pointer(entry))
        .collect::<Result<Vec<_>>>()?;
    let mut alike = from.len() == to.len();
    for (&a, &b) in from.iter().zip(&to) {
        ensure!(
            source.u32(a - 4)? == 0x8080_2975 && view.u32(b - 4)? == 0x8080_4444,
            "Sword trajectory block has another class"
        );
        alike &= source.u32(a)? == view.u32(b)?;
    }
    if alike {
        for (&a, &b) in from.iter().zip(&to) {
            for (from, to) in TRAJECTORY {
                space.put(b + to, source.bytes::<4>(a + from)?)?;
            }
        }
    } else {
        limits.push("Trajectory blocks differ in number or mode and stay native".to_owned());
    }
    let contact = source.pointer(s + 0x170)?;
    let target = view.pointer(n + 0x1A8)?;
    match (source.u32(contact - 4)?, view.u32(target - 4)?) {
        (0x8080_31A4, 0x8080_4455) => {
            // A contact without a damage profile leaves damage to a spawned entity, whose
            // geometry is its own. The capsule carries only where both rows deliver alike.
            if (source.u32(contact + 0x28)? == u32::MAX) == (view.u32(target + 0x28)? == u32::MAX) {
                for offset in [0, 4, 8, 0x18] {
                    space.put(target + offset, source.bytes::<4>(contact + offset)?)?;
                }
            } else {
                limits.push(
                    "The contact delivers damage another way, so its capsule stays native"
                        .to_owned(),
                );
            }
        }
        (0x8080_307A, 0x8080_4454) => {
            space.put(target + 0x18, source.bytes::<4>(contact + 0x18)?)?;
        }
        _ => limits.push("Contact kinds differ, so the contact stays native".to_owned()),
    }
    match (source.u64(s + 0xF8)? != 0, view.u64(n + 0x110)? != 0) {
        (true, true) => {
            let record = view.pointer(n + 0x110)?;
            ensure!(
                view.u32(record - 4)? == 0x8080_692C,
                "Native sword air hold has another class"
            );
            let body = air_hold(source, s)?;
            for offset in (0..0x48).step_by(4) {
                space.put(
                    record + offset,
                    [
                        body[offset],
                        body[offset + 1],
                        body[offset + 2],
                        body[offset + 3],
                    ],
                )?;
            }
        }
        (false, false) => {}
        (true, false) => limits.push("The source air hold has no native record here".to_owned()),
        (false, true) => limits.push("The native air hold stays on this row".to_owned()),
    }
    Ok(limits)
}

/// A source row's air-hold record as native record `8080692C`: the key, both admission flags,
/// then the source's two eight-float groups.
fn air_hold(source: &Payload, row: usize) -> Result<[u8; 0x48]> {
    let record = source.pointer(row + 0xF8)?;
    ensure!(
        record >= 4 && source.u32(record - 4)? == 0x8080_2AA6,
        "Source sword movement record is not the air-hold record"
    );
    let mut body = [0; 0x48];
    body[..4].copy_from_slice(&source.bytes::<4>(record)?);
    body[4..6].copy_from_slice(&[1, 1]);
    body[8..].copy_from_slice(&source.bytes::<0x40>(record + 4)?);
    Ok(body)
}

/// A native row set: a list, or the rows a combo continues into.
#[derive(Clone, Copy)]
enum Set {
    List(usize),
    Combo(usize),
}

impl Set {
    /// The descriptor of the rows the set stores.
    fn stored(self) -> usize {
        match self {
            Self::List(list) => list + 0x50,
            Self::Combo(combo) => combo + 0xB8,
        }
    }

    /// The descriptor of the row references selection walks.
    fn selected(self) -> usize {
        match self {
            Self::List(list) => list + 0x60,
            Self::Combo(combo) => combo + 0xD8,
        }
    }
}

struct Walk<'a> {
    source: &'a Payload,
    donor: &'a Payload,
    /// The base's owner first, then the stock frame owners.
    frames: &'a [(u32, Payload)],
    edits: BTreeMap<usize, [u8; 4]>,
    blob: Blob,
    /// Descriptors pointed into the appended bytes: owner offset, header and count.
    arrays: Vec<(usize, usize, u64)>,
    /// Stored rows copied into a rebuilt array, by their offset in the base's owner.
    moved: BTreeMap<usize, usize>,
    /// Appended row references to rows of the base's owner, settled once every list is rebuilt.
    references: Vec<(usize, usize)>,
    ordinal: u32,
    keys: BTreeSet<u32>,
    rows: Vec<Value>,
    unsupported: Vec<Value>,
    /// Every source action a native row carries, and the keyed ones among them in order.
    actions: BTreeSet<u32>,
    lenders: Vec<u32>,
    /// Keyed source rows no stock row plays, by action.
    missing: BTreeMap<u32, usize>,
    /// What each such row needs of its own, once prepared.
    specials: &'a BTreeMap<u32, Special>,
    /// Resource words of the appended bytes that name a translated entity, by symbol.
    symbols: Vec<(usize, String)>,
}

/// A keyed attack no stock row plays: the private human action it plays, the stock row its
/// native-only fields start from, and its translated entities by native row field.
struct Special {
    action: u32,
    template: (usize, usize),
    entities: Vec<(usize, Option<String>)>,
}

/// Source row fields naming the entity an attack reports with and the entity it spawns, with
/// the native row fields of the same roles. Rise of Iron Phoenix Uppercut names presentation
/// entity `80A7E2D3` and spawned entity `80A88C7F` there, and the stock Phoenix Uppercut row
/// names `80BC8365` and `80BC840D`, graphs of the same owners.
const ENTITIES: [(usize, usize); 2] = [(0x150, 0x188), (0x160, 0x198)];

impl Walk<'_> {
    /// The stock row that plays `action` with no key and no combos of its own.
    fn twin(&self, action: u32) -> Result<Option<(usize, usize)>> {
        for (index, (_, frame)) in self.frames.iter().enumerate() {
            let definition = frame.pointer(24)?;
            for lists in [0x240, 0x230] {
                for list in frame.array(definition + lists, 0x78, Some(0x8080_43D7))? {
                    for entry in frame.array(list + 0x60, 8, Some(0x8080_2EAE))? {
                        let row = frame.pointer(entry)?;
                        if frame.u32(row)? == action
                            && frame.u32(row + 4)? == action
                            && frame.u32(row + 0xC)? == EMPTY
                            && frame.u64(row + 0x1F0)? == 0
                        {
                            return Ok(Some((index, row)));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// A stock trajectory block of `mode`, from the twin's own rows first, then from the rows
    /// their combos store.
    fn trajectory_block(&self, first: usize, mode: u32) -> Result<Option<[u8; 0x50]>> {
        fn among(
            frame: &Payload,
            rows: Vec<usize>,
            mode: u32,
            depth: usize,
        ) -> Result<Option<[u8; 0x50]>> {
            ensure!(depth < 8, "Native sword combos nest too deeply");
            for &row in &rows {
                for entry in frame.array(row + 0x118, 8, Some(0x8080_4446))? {
                    let block = frame.pointer(entry)?;
                    if frame.u32(block - 4)? == 0x8080_4444 && frame.u32(block)? == mode {
                        return Ok(Some(frame.bytes::<0x50>(block)?));
                    }
                }
            }
            for row in rows {
                for combo in frame.array(row + 0x1F0, 0xE8, Some(0x8080_2D6D))? {
                    let nested = frame.array(combo + 0xB8, ROW, Some(0x8080_2D7B))?;
                    if let Some(block) = among(frame, nested, mode, depth + 1)? {
                        return Ok(Some(block));
                    }
                }
            }
            Ok(None)
        }
        let order = std::iter::once(first).chain((0..self.frames.len()).filter(|&i| i != first));
        for index in order {
            let frame = &self.frames[index].1;
            let definition = frame.pointer(24)?;
            for lists in [0x240, 0x230] {
                for list in frame.array(definition + lists, 0x78, Some(0x8080_43D7))? {
                    let rows = frame.array(list + 0x50, ROW, Some(0x8080_2D7B))?;
                    if let Some(block) = among(frame, rows, mode, 0)? {
                        return Ok(Some(block));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Builds a keyed row at `dest` from the stock row `(frame, twin)` and the source row `s`:
    /// the stock row's presentation and delivery with the source's identity, gate and mechanics.
    fn build(
        &mut self,
        dest: usize,
        (frame, twin): (usize, usize),
        s: usize,
        identity: u32,
    ) -> Result<Vec<String>> {
        let (source, frames) = (self.source, self.frames);
        let template = &frames[frame].1;
        let mut limits = Vec::new();
        self.blob.bytes[dest..dest + ROW].copy_from_slice(&template.0[twin..twin + ROW]);
        for (count, class, stride) in ROW_ARRAYS {
            let rows = template.array_range(twin + count, stride, Some(class))?;
            if rows.is_empty() {
                continue;
            }
            let length = rows.len();
            let (header, at) = self.blob.array(length / stride, class, stride);
            self.blob.bytes[at..at + length].copy_from_slice(&template.0[rows]);
            self.blob.point(dest + count + 8, header);
        }
        // The gate is the source's own. A stock frame row also asks for sword energy, which
        // Rise of Iron has no version of.
        self.blob.bytes[dest + 0x18..dest + 0x28].fill(0);
        if let Some(gate) = source_ammo(source, s)? {
            let (header, at) = self.blob.array(1, 0x8080_2D6E, 8);
            let mut body = [0; 0xC];
            body[..8].copy_from_slice(&gate);
            let object = self.blob.typed(0x8080_2D70, &body, 4);
            self.blob.point(at, object);
            self.blob.bytes[dest + 0x18..dest + 0x20].copy_from_slice(&1u64.to_le_bytes());
            self.blob.point(dest + 0x20, header);
        }
        // The trajectory holds the source's blocks on stock blocks of the same mode.
        self.blob.bytes[dest + 0x118..dest + 0x128].fill(0);
        let blocks = source.array(s + 0x108, 8, Some(0x8080_28AF))?;
        if !blocks.is_empty() {
            let (header, at) = self.blob.array(blocks.len(), 0x8080_4446, 8);
            for (index, entry) in blocks.iter().enumerate() {
                let mode = source.u32(source.pointer(*entry)?)?;
                let body = self.trajectory_block(frame, mode)?.with_context(|| {
                    format!("Shadowkeep sword rows hold no trajectory block of mode {mode}")
                })?;
                let block = self.blob.typed(0x8080_4444, &body, 16);
                self.blob.point(at + index * 8, block);
            }
            self.blob.bytes[dest + 0x118..dest + 0x120]
                .copy_from_slice(&(blocks.len() as u64).to_le_bytes());
            self.blob.point(dest + 0x120, header);
        }
        let contact = template.pointer(twin + 0x1A8)?;
        let (class, size) = match template.u32(contact - 4)? {
            0x8080_4455 => (0x8080_4455, 0x50),
            0x8080_4454 => (0x8080_4454, 0x20),
            other => anyhow::bail!("Stock sword row has contact class {other:08X}"),
        };
        let body = template
            .0
            .get(contact..contact + size)
            .context("Stock sword contact")?;
        let object = self.blob.typed(class, body, 16);
        self.blob.point(dest + 0x1A8, object);
        // The air hold is the source's, or none.
        self.blob.bytes[dest + 0x110..dest + 0x118].fill(0);
        if source.u64(s + 0xF8)? != 0 {
            let object = self.blob.typed(0x8080_692C, &air_hold(source, s)?, 4);
            self.blob.point(dest + 0x110, object);
        } else if template.u64(twin + 0x110)? != 0 {
            limits.push("The stock row's air hold is left out, as the source row has none".into());
        }
        ensure!(
            template.u64(twin + 0x1F0)? == 0,
            "Stock sword row continues into combos"
        );
        self.blob.word(dest, identity);
        self.blob.word(dest + 0xC, source.u32(s + 8)?);
        self.blob.word(dest + 0x200, self.ordinal);
        self.ordinal += 1;
        limits.extend(transfer(&mut self.blob, dest, source, s)?);
        Ok(limits)
    }

    /// Zeroes the array descriptor at `descriptor` in the base's owner.
    fn clear(&mut self, descriptor: usize) -> Result<()> {
        let mut owner = Owner {
            payload: self.donor,
            edits: &mut self.edits,
        };
        for word in 0..4 {
            owner.put(descriptor + word * 4, [0; 4])?;
        }
        Ok(())
    }

    /// Builds the keyed source row `s` at `dest`: on the stock row `twin` that plays its action,
    /// or with no such row on its prepared [`Special`], whose private action and translated
    /// entities it then names. A row's identity is its action, as on stock rows, or its key
    /// where the set already holds a row of that action.
    fn keyed(
        &mut self,
        dest: usize,
        s: usize,
        twin: Option<(usize, usize)>,
        identities: &mut BTreeSet<u32>,
        place: &str,
    ) -> Result<()> {
        let (source, specials) = (self.source, self.specials);
        let action = source.u32(s)?;
        let key = source.u32(s + 8)?;
        let identity = if identities.contains(&action) {
            key
        } else {
            action
        };
        ensure!(
            identities.insert(identity),
            "Keyed sword row {action:08X} has no free identity"
        );
        let mut row = json!({"attack":format!("{place}/{action:08X}"),"key":key,
            "identity":identity});
        let template = match twin {
            Some(twin) => twin,
            None => {
                specials
                    .get(&action)
                    .context("Sword special was not prepared")?
                    .template
            }
        };
        row["limits"] = json!(self.build(dest, template, s, identity)?);
        if twin.is_some() {
            row["stock_owner"] = json!(self.frames[template.0].0);
        } else if let Some(special) = specials.get(&action) {
            self.blob.word(dest + 4, special.action);
            for (field, symbol) in &special.entities {
                self.blob.word(dest + field, u32::MAX);
                if let Some(symbol) = symbol {
                    self.symbols.push((dest + field, symbol.clone()));
                }
            }
            row["private_action"] = json!(special.action);
            row["template_owner"] = json!(self.frames[template.0].0);
        }
        self.keys.insert(key);
        self.rows.push(row);
        Ok(())
    }

    /// Gives `set` the rows of `order` and nothing else: its stored rows that the source keeps,
    /// copied with their own parts, and the keyed rows built for it, in the source's order. A
    /// row the base stores elsewhere stays there and is only referenced.
    fn rebuild(&mut self, set: Set, order: &[Entry], place: &str) -> Result<()> {
        let donor = self.donor;
        let stored = donor.array(set.stored(), ROW, Some(0x8080_2D7B))?;
        let own = |entry: &Entry| match *entry {
            Entry::Keyed(..) | Entry::Special(_) => true,
            Entry::Existing(row) => stored.contains(&row),
        };
        if matches!(set, Set::List(_)) {
            ensure!(
                order.iter().all(own),
                "A base sword list selects rows outside its own storage"
            );
        }
        let count = order.iter().filter(|&entry| own(entry)).count();
        let mut identities = BTreeSet::new();
        for entry in order {
            if let Entry::Existing(row) = *entry {
                identities.insert(donor.u32(row)?);
            }
        }
        // Rows first, so every reference below has its target.
        let mut placed = Vec::new();
        if count == 0 {
            if donor.u64(set.stored())? != 0 {
                self.clear(set.stored())?;
            }
        } else {
            let (header, rows) = self.blob.array(count, 0x8080_2D7B, ROW);
            let mut next = rows;
            for entry in order.iter().filter(|&entry| own(entry)) {
                match *entry {
                    Entry::Keyed(s, twin) => {
                        self.keyed(next, s, Some(twin), &mut identities, place)?;
                    }
                    Entry::Special(s) => self.keyed(next, s, None, &mut identities, place)?,
                    Entry::Existing(old) => {
                        self.blob.bytes[next..next + ROW].copy_from_slice(&donor.0[old..old + ROW]);
                        for offset in ROW_POINTERS {
                            if donor.u64(old + offset)? != 0 {
                                self.blob
                                    .retained
                                    .push((next + offset, donor.pointer(old + offset)?));
                            }
                        }
                        self.moved.insert(old, next);
                    }
                }
                placed.push(next);
                next += ROW;
            }
            self.arrays.push((set.stored(), header, count as u64));
        }
        // A combo's row names stand for rows stored elsewhere, which the references below now
        // settle on their own.
        if let Set::Combo(combo) = set
            && donor.u64(combo + 0xC8)? != 0
        {
            self.clear(combo + 0xC8)?;
        }
        if order.is_empty() {
            return self.clear(set.selected());
        }
        let (header, entries) = self.blob.array(order.len(), 0x8080_2EAE, 8);
        let mut placed = placed.into_iter();
        for (index, entry) in order.iter().enumerate() {
            let word = entries + index * 8;
            match *entry {
                Entry::Existing(row) if !stored.contains(&row) => {
                    self.references.push((word, row));
                }
                _ => {
                    let row = placed.next().context("Rebuilt sword row")?;
                    self.blob.point(word, row);
                }
            }
        }
        self.arrays
            .push((set.selected(), header, order.len() as u64));
        Ok(())
    }

    /// Translates the reachable source `rows` onto the rows `set` selects, and through their
    /// combos onto the rows those continue into. The set then selects those rows alone, so no
    /// attack of the base's that the source lacks stays reachable.
    fn pair(&mut self, rows: &[usize], set: Set, place: &str, depth: usize) -> Result<()> {
        ensure!(depth < 6, "Source sword combos nest too deeply");
        let (source, donor) = (self.source, self.donor);
        let selected = donor
            .array(set.selected(), 8, Some(0x8080_2EAE))?
            .into_iter()
            .map(|entry| donor.pointer(entry))
            .collect::<Result<Vec<_>>>()?;
        let mut order = Vec::new();
        for s in reachable(source, rows)? {
            let action = source.u32(s)?;
            let key = source.u32(s + 8)?;
            let name = format!("{place}/{action:08X}");
            if key != EMPTY {
                match self.twin(action)? {
                    Some(twin) => {
                        self.actions.insert(action);
                        if !self.lenders.contains(&action) {
                            self.lenders.push(action);
                        }
                        order.push(Entry::Keyed(s, twin));
                    }
                    None if self.specials.contains_key(&action) => {
                        order.push(Entry::Special(s));
                    }
                    None => {
                        // The list's own row is the last one walked for an action.
                        self.missing.insert(action, s);
                        self.unsupported
                            .push(json!({"attack":name,"action":action,"key":key,
                        "reason":"Shadowkeep has no sword attack row that plays this action"}));
                    }
                }
                continue;
            }
            let gated = source_ammo(source, s)?.is_some();
            let mut partner = None;
            for &row in &selected {
                if !order
                    .iter()
                    .any(|entry| matches!(entry, Entry::Existing(used) if *used == row))
                    && donor.u32(row + 0xC)? == EMPTY
                    && donor.u32(row + 4)? == action
                    && native_ammo(donor, row)?.is_some() == gated
                {
                    partner = Some(row);
                    break;
                }
            }
            // A base without the source's own attack is not this sword's lineage, and leaving
            // the attack out would change what every later row means.
            let n = partner.with_context(|| {
                format!("The base's sword owner has no row for source attack {name}")
            })?;
            order.push(Entry::Existing(n));
            self.actions.insert(action);
            let limits = transfer(
                &mut Owner {
                    payload: donor,
                    edits: &mut self.edits,
                },
                n,
                source,
                s,
            )?;
            self.rows
                .push(json!({"attack":name,"native_row":n,"limits":limits}));
            let combos = donor.array(n + 0x1F0, 0xE8, Some(0x8080_2D6D))?;
            let mut kinds = BTreeSet::new();
            for combo in source.array(s + 0x180, 0x20, Some(0x8080_26AC))? {
                let kind = source.u8(combo)?;
                kinds.insert(kind);
                let Some(&target) = combos.iter().find(|&&t| donor.u8(t).ok() == Some(kind)) else {
                    self.unsupported.push(json!({"attack":name,"combo":kind,
                        "reason":"The base's row has no combo of this kind, so the source's continuations of it are left out"}));
                    continue;
                };
                for (index, (code, constants)) in WINDOWS.into_iter().enumerate() {
                    let (at, _) = constant(donor, target + code, target + constants)?
                        .context("Native sword combo window is not one constant")?;
                    let value = source.bytes::<4>(combo + 4 + index * 4)?;
                    let mut owner = Owner {
                        payload: donor,
                        edits: &mut self.edits,
                    };
                    for lane in 0..4 {
                        owner.put(at + lane * 4, value)?;
                    }
                }
                let nested = source.array(combo + 0x10, 0x198, Some(0x8080_2A15))?;
                self.pair(
                    &nested,
                    Set::Combo(target),
                    &format!("{name}:{kind}"),
                    depth + 1,
                )?;
            }
            // A combo kind the source row lacks continues into nothing.
            for &target in &combos {
                if !kinds.contains(&donor.u8(target)?) && donor.u64(target + 0xD8)? != 0 {
                    self.clear(target + 0xD8)?;
                    self.rows
                        .push(json!({"attack":name,"combo":donor.u8(target)?,
                        "left_out":"The source row has no combo of this kind"}));
                }
            }
        }
        let unchanged = order.len() == selected.len()
            && order
                .iter()
                .zip(&selected)
                .all(|(entry, row)| matches!(entry, Entry::Existing(kept) if kept == row));
        if !unchanged {
            self.rebuild(set, &order, place)?;
        }
        Ok(())
    }
}

/// A row of a rebuilt set: one the base already holds, by its offset, or a keyed row built
/// from a source row and the stock row of its action.
#[derive(Clone, Copy)]
enum Entry {
    Existing(usize),
    Keyed(usize, (usize, usize)),
    /// A keyed source row built on its prepared [`Special`].
    Special(usize),
}

/// A special's translated entities by native row field, and their attachment groups.
type Spawned = (Vec<(usize, Option<String>)>, Vec<Value>);

/// What the keyed attacks without a stock row add: each prepared special by action, their
/// reports, their attachment groups and their private human actions.
type Specials = (
    BTreeMap<u32, Special>,
    Vec<Value>,
    Vec<Value>,
    Option<animation::Human>,
);

/// Prepares every keyed attack the first walk found no stock row for. One whose preparation is
/// refused stays out, with the reason.
fn specials(
    reader: &Source,
    native: &mut Reader,
    folder: &Path,
    walk: &Walk<'_>,
) -> Result<Specials> {
    let mut prepared = BTreeMap::new();
    let mut reports = Vec::new();
    let mut attachments = Vec::new();
    let mut human: Option<animation::Human> = None;
    for (&action, &row) in &walk.missing {
        match special(reader, native, folder, walk, action, row) {
            Ok((special, actions, groups, report)) => {
                match human.as_mut() {
                    Some(held) => held.absorb(actions)?,
                    None => human = Some(actions),
                }
                prepared.insert(action, special);
                attachments.extend(groups);
                reports.push(report);
            }
            Err(error) => {
                crate::cancellation::check()?;
                reports.push(json!({"action":action,"key":walk.source.u32(row + 8)?,
                    "reason":format!("{error:#}")}));
            }
        }
    }
    Ok((prepared, reports, attachments, human))
}

/// Prepares what a keyed attack with no stock row needs: a stock row to start from, its private
/// human action and its translated entities, saved as groups in the item's own folder.
fn special(
    reader: &Source,
    native: &mut Reader,
    folder: &Path,
    walk: &Walk<'_>,
    action: u32,
    row: usize,
) -> Result<(Special, animation::Human, Vec<Value>, Value)> {
    let source = walk.source;
    let contact = source.pointer(row + 0x170)?;
    let melee = match source.u32(contact - 4)? {
        0x8080_31A4 => true,
        0x8080_307A => false,
        other => anyhow::bail!("Source sword contact has class {other:08X}"),
    };
    // The first stock keyed row whose contact delivers as the source's does.
    let mut template = None;
    for &lender in &walk.lenders {
        let Some((frame, twin)) = walk.twin(lender)? else {
            continue;
        };
        let stock = &walk.frames[frame].1;
        let target = stock.pointer(twin + 0x1A8)?;
        let alike = match (melee, stock.u32(target - 4)?) {
            (true, 0x8080_4455) => {
                (source.u32(contact + 0x28)? == u32::MAX) == (stock.u32(target + 0x28)? == u32::MAX)
            }
            (false, 0x8080_4454) => true,
            _ => false,
        };
        if alike {
            template = Some((frame, twin));
            break;
        }
    }
    let template =
        template.context("No stock keyed sword row delivers its contact as this attack does")?;
    let animated = (|| -> Result<(u32, animation::Prepared)> {
        let plan = animation::read(
            reader,
            animation::SOURCE_HUMAN_LOOKUP,
            &BTreeSet::from([action]),
        )?;
        let mut refusals = Vec::new();
        for &lender in &walk.lenders {
            match animation::prepare_lent(
                reader,
                native,
                &plan,
                &animation::HUMAN,
                &format!("sword-{action:08x}"),
                &BTreeMap::new(),
                &[],
                (&BTreeMap::from([(action, lender)]), &walk.actions),
            ) {
                Ok(found) => return Ok((lender, found)),
                Err(error) => {
                    crate::cancellation::check()?;
                    refusals.push(format!("{lender:08X}: {error:#}"));
                }
            }
        }
        anyhow::bail!("No keyed action lends its node: {}", refusals.join(" | "))
    })();
    // The attack's groups share a folder of their own, which leaves again when the attack does
    // not translate.
    let home = format!("sword/{action:08X}");
    let spawned = (|| -> Result<Spawned> {
        let mut entities = Vec::new();
        let mut groups = Vec::new();
        for (field, target) in ENTITIES {
            let tag = source.u32(row + field)?;
            if tag == u32::MAX {
                entities.push((target, None));
                continue;
            }
            let name = format!("d1-closure-{tag:08X}");
            let group = crate::d1_roi::native::perk::pattern::closure::prepare_at(
                reader,
                native,
                tag,
                &folder.join(&home).join(&name),
            )
            .with_context(|| format!("Sword attack entity {tag:08X}"))?;
            groups.push(
                json!({"directory":format!("{home}/{name}"),"sha256":group.sha256,
                "symbol":group.symbol,"source":tag}),
            );
            entities.push((target, Some(group.symbol)));
        }
        Ok((entities, groups))
    })();
    // Both halves are tried, so a refusal names everything the attack still lacks.
    let ((lender, prepared), (entities, groups)) = match (animated, spawned) {
        (Ok(animated), Ok(spawned)) => (animated, spawned),
        (animated, spawned) => {
            // A folder that cannot be removed holds groups nothing names.
            let _ = fs::remove_dir_all(folder.join(&home));
            let _ = fs::remove_dir(folder.join("sword"));
            crate::cancellation::check()?;
            let reasons = [
                animated.err().map(|error| format!("action: {error:#}")),
                spawned.err().map(|error| format!("entities: {error:#}")),
            ];
            anyhow::bail!(
                "{}",
                reasons
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" | ")
            );
        }
    };
    let private = prepared.actions[&action];
    let human = match animation::Human::new(native, prepared) {
        Ok(human) => human,
        Err(error) => {
            let _ = fs::remove_dir_all(folder.join(&home));
            let _ = fs::remove_dir(folder.join("sword"));
            return Err(error);
        }
    };
    let report = json!({"action":action,"key":source.u32(row + 8)?,"lender":lender,
        "private_action":private,"template_owner":walk.frames[template.0].0,
        "entities":groups.clone()});
    Ok((
        Special {
            action: private,
            template,
            entities,
        },
        human,
        groups,
        report,
    ))
}

/// Walks every source list onto the base's list of the same selection kind.
fn translate<'a>(
    (source, source_definition): (&'a Payload, usize),
    (payload, definition): (&'a Payload, usize),
    frames: &'a [(u32, Payload)],
    highest: u32,
    specials: &'a BTreeMap<u32, Special>,
) -> Result<Walk<'a>> {
    let mut walk = Walk {
        source,
        donor: payload,
        frames,
        edits: BTreeMap::new(),
        blob: Blob::default(),
        arrays: Vec::new(),
        moved: BTreeMap::new(),
        references: Vec::new(),
        ordinal: highest + 1,
        keys: BTreeSet::new(),
        rows: Vec::new(),
        unsupported: Vec::new(),
        actions: BTreeSet::new(),
        lenders: Vec::new(),
        missing: BTreeMap::new(),
        specials,
        symbols: Vec::new(),
    };
    for (name, from, to) in [("light", 0x268, 0x230), ("heavy", 0x278, 0x240)] {
        let targets = payload.array(definition + to, 0x78, Some(0x8080_43D7))?;
        for list in source.array(source_definition + from, 0x68, Some(0x8080_21A6))? {
            let kind = source.u8(list)?;
            let rows = source.array(list + 0x50, 0x198, Some(0x8080_2A15))?;
            let matching = targets
                .iter()
                .copied()
                .filter(|&target| payload.u8(target).ok() == Some(kind))
                .collect::<Vec<_>>();
            let &[target] = matching.as_slice() else {
                anyhow::bail!(
                    "The base's {name} attacks hold {} lists of selection kind {kind}",
                    matching.len()
                );
            };
            walk.pair(&rows, Set::List(target), &format!("{name}[{kind}]"), 0)?;
        }
    }
    ensure!(!walk.rows.is_empty(), "No source sword attack has a row");
    Ok(walk)
}
/// The highest row ordinal the owner's lists and combos hold.
fn ordinals(p: &Payload, rows: &[usize], depth: usize, highest: &mut u32) -> Result<()> {
    ensure!(depth < 8, "Native sword combos nest too deeply");
    for &row in rows {
        *highest = (*highest).max(p.u32(row + 0x200)?);
        for combo in p.array(row + 0x1F0, 0xE8, Some(0x8080_2D6D))? {
            let nested = p.array(combo + 0xB8, ROW, Some(0x8080_2D7B))?;
            ordinals(p, &nested, depth + 1, highest)?;
        }
    }
    Ok(())
}

/// The base's owner first, then the stock frame owners. A frame owner this build lacks only
/// narrows the rows keyed attacks can borrow.
fn frame_owners(native: &mut Reader, base: u32, payload: &Payload) -> Result<Vec<(u32, Payload)>> {
    let mut frames = vec![(base, payload.clone())];
    for tag in FRAMES {
        if tag == base {
            continue;
        }
        let Ok(frame) = native.tag(tag, Some(0x8080_9C36)) else {
            crate::cancellation::check()?;
            continue;
        };
        let at = frame.pointer(24)?;
        if at >= 4 && frame.u32(at - 4)? == 0x8080_2D66 {
            frames.push((tag, (*frame).clone()));
        }
    }
    Ok(frames)
}

/// Grows the base's empty key set at array descriptor `field` to the source's slots when the
/// weapon holds a name, and returns the slot count.
fn key_set(
    payload: &Payload,
    field: usize,
    (source_slots, held): (usize, usize),
    blob: &mut Blob,
    arrays: &mut Vec<(usize, usize, u64)>,
) -> Result<usize> {
    let stock_slots = payload.array(field, 8, Some(0x8080_43D5))?;
    let slots = stock_slots.len().max(source_slots);
    if held != 0 && slots > stock_slots.len() {
        let mut empty = [0; 8];
        empty[..4].copy_from_slice(&EMPTY.to_le_bytes());
        for &slot in &stock_slots {
            ensure!(
                payload.bytes::<8>(slot)? == empty,
                "The base's sword key set is not empty"
            );
        }
        let (header, at) = blob.array(slots, 0x8080_43D5, 8);
        for index in 0..slots {
            blob.bytes[at + index * 8..at + index * 8 + 8].copy_from_slice(&empty);
        }
        arrays.push((field, header, slots as u64));
    }
    ensure!(
        held == 0 || slots > 0,
        "The base's sword key set has no slot"
    );
    Ok(slots)
}

pub(super) fn prepare(assembly: &mut Assembly<'_>, export: &Value, donor: u32) -> Result<Value> {
    assembly.sword_keys.clear();
    assembly.sword_key_slots = 0;
    let root = u32::from_str_radix(
        export["runtime"]["runtime_entity"]
            .as_str()
            .context("Source sword runtime")?,
        16,
    )?;
    let entity = assembly.source.read(root)?;
    let mut owners = Vec::new();
    for r in entity.array(32, 12, Some(0x8080_0715))? {
        let tag = entity.u32(r)?;
        let payload = assembly.source.read(tag)?;
        let definition = payload.pointer(24)?;
        if definition >= 4 && payload.u32(definition - 4)? == 0x8080_2056 {
            owners.push((tag, payload, definition));
        }
    }
    if owners.is_empty() {
        return Ok(Value::Null);
    }
    ensure!(owners.len() == 1, "Source sword owner is ambiguous");
    let (source_tag, source, source_definition) = owners.pop().context("Source sword owner")?;
    let native = &mut assembly.native;
    let manager = sundial::package_authoring::PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )
    .map_err(anyhow::Error::msg)?;
    let runtime = runtime::load_runtime_entity_for_item_with_manager(&manager, donor)
        .map_err(anyhow::Error::msg)?;
    let bindings =
        weapon_component_bindings(&runtime.payload, BINDING).map_err(anyhow::Error::msg)?;
    ensure!(bindings.len() == 1, "The base has no single sword owner");
    let binding = &bindings[0];
    let payload = (*native.tag(binding.owner_tag, Some(0x8080_9C36))?).clone();
    let resource = usize::try_from(binding.resource_offset)?;
    ensure!(
        resource >= 4
            && payload.u32(resource - 4)? == 0x8080_43D2
            && payload.u32(resource)? == binding.owner_tag,
        "The base's sword resource has another class"
    );
    let definition = usize::try_from(payload.u64(resource + 8)?)?;
    ensure!(
        definition == payload.pointer(24)? && payload.u32(definition - 4)? == 0x8080_2D66,
        "The base's sword definition has another class"
    );
    // Guards are owners of their own. A refusal leaves every guard the base's and carries no
    // name, while the attacks still translate.
    let (guards, guard_keys, unsupported_guards) =
        match guard::prepare(assembly.source, native, &entity, &runtime.payload) {
            Ok(prepared) => prepared,
            Err(error) => {
                crate::cancellation::check()?;
                (
                    Vec::new(),
                    BTreeSet::new(),
                    vec![json!({"reason":format!("{error:#}")})],
                )
            }
        };
    let frames = frame_owners(native, binding.owner_tag, &payload)?;
    let mut highest = 0;
    for lists in [0x230, 0x240] {
        for list in payload.array(definition + lists, 0x78, Some(0x8080_43D7))? {
            let rows = payload.array(list + 0x50, ROW, Some(0x8080_2D7B))?;
            ordinals(&payload, &rows, 0, &mut highest)?;
        }
    }
    // A keyed attack no stock row plays is prepared from what a first walk finds, and a second
    // walk then places it. An attack whose preparation is refused stays out, with the reason.
    let none = BTreeMap::new();
    let first = translate(
        (&source, source_definition),
        (&payload, definition),
        &frames,
        highest,
        &none,
    )?;
    let (prepared, specials, attachments, human) =
        specials(assembly.source, native, &assembly.folder, &first)?;
    let walk = if prepared.is_empty() {
        first
    } else {
        drop(first);
        translate(
            (&source, source_definition),
            (&payload, definition),
            &frames,
            highest,
            &prepared,
        )?
    };
    let Walk {
        edits,
        mut blob,
        mut arrays,
        moved,
        references,
        keys,
        rows,
        unsupported,
        symbols,
        ..
    } = walk;
    // A reference to a row that moved follows it. Any other stays on the base's row.
    for (word, old) in references {
        match moved.get(&old) {
            Some(&new) => blob.point(word, new),
            None => blob.retained.push((word, old)),
        }
    }
    // A moved row carries the values its original takes.
    for (&old, &new) in &moved {
        for (&at, value) in edits.range(old..old + ROW) {
            blob.bytes[new + at - old..new + at - old + 4].copy_from_slice(value);
        }
    }
    // Each held name takes a slot of the weapon's key set, which the source sizes for its perks.
    // The set is instance data, so the build seals the instance span around the appended rows.
    let source_instance = source.pointer(16)?;
    let source_slots = source
        .array(source_instance + 0x1E0, 8, Some(0x8080_2401))?
        .len();
    let held = keys.len() + guard_keys.len();
    let slots = key_set(
        &payload,
        resource + 0x1F0,
        (source_slots, held),
        &mut blob,
        &mut arrays,
    )?;
    let relative = |at: usize| -> Result<u32> {
        Ok(u32::try_from(
            at.checked_sub(resource)
                .context("Sword edit precedes its resource")?,
        )?)
    };
    let edits = edits
        .iter()
        .map(|(&at, after)| {
            Ok(json!({"offset":relative(at)?,"before":payload.bytes::<4>(at)?,"after":after}))
        })
        .collect::<Result<Vec<_>>>()?;
    let appends = if blob.bytes.is_empty() {
        Vec::new()
    } else {
        vec![json!({"bytes":hex::encode(&blob.bytes),
            "arrays":arrays.iter().map(|&(descriptor, header, count)| Ok(json!([relative(descriptor)?, header, count]))).collect::<Result<Vec<_>>>()?,
            "symbols":symbols,
            "retained_pointers":blob.retained.iter().map(|&(word, target)| Ok(json!([word, relative(target)?]))).collect::<Result<Vec<_>>>()?})]
    };
    assembly.sword_keys = keys.union(&guard_keys).copied().collect();
    assembly.sword_key_slots = slots;
    assembly.human = human;
    Ok(
        json!({"status":"translated","schema":1,"source_owner":source_tag,
        "native_owner":binding.owner_tag,"native_resource":binding.resource_offset,
        "native_sha256":hex::encode(Sha256::digest(&payload.0)),
        "edits":edits,"appends":appends,"keys":keys,"key_slots":slots,"source_key_slots":source_slots,
        "rows":rows,"unsupported":unsupported,
        "guards":guards,"guard_keys":guard_keys,"unsupported_guards":unsupported_guards,
        "specials":specials,"attachments":attachments,
        "scope":"Attack selection, aim limits, movement, strike timing, ammunition cost, combo windows, keyed heavy attacks and keyed guards. A row with a native action keeps that action's presentation, damage profiles and spawned entities. A keyed row without one plays a private action and spawns its translated source entities. Sword energy and definition programs stay the base's.",
        "gameplay_verified":false}),
    )
}
