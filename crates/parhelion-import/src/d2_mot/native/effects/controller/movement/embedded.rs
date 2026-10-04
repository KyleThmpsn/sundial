//! Embedded records: the variant rows of an expression's sub table, the category tables of a
//! curve and the category mask trailers. Each class is a field list. A modern reference is
//! 16 bytes (a tag, a form flag and a 64-bit hash) and shrinks to the native 8 (the tag and a
//! zero word). Arrays and pointed records follow their owner depth first in field order, an
//! array header at 16-byte alignment behind its marker, a record body at its own alignment
//! behind its class word. The layouts were established on the twin corpus, where every sub
//! table of a projectile present in both games rewrites byte for byte.
use super::{CODE, NATIVE_MARKER, Resources, VECTORS, native_class, procedural, put};
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, bail, ensure};

#[derive(Clone, Copy)]
enum Field {
    /// Bytes copied as they are.
    Raw(usize),
    /// A resource reference.
    Reference,
    /// An array descriptor (a count and a relative pointer) with rows of the given element.
    Array(Elem),
    /// A relative pointer to an embedded record whose class word precedes its body.
    Pointer,
    /// Modern bytes without a native counterpart.
    Dropped(usize),
    /// An inline expression: a zero pair, a code array, a constant array and two ones.
    Expression,
    /// Category masks, `n` of them.
    Masks(usize),
}

#[derive(Clone, Copy)]
enum Elem {
    /// Sequencer bytecode, lowered.
    Code,
    /// Sequencer constants, 16 bytes each.
    Constants,
    /// Category rows: a hash, zeros and the dictionary reference.
    Categories,
    /// Category selections, records of class 808029D8.
    Selections,
}

/// The modern table class of an expression's sub table.
pub(super) const SUB_TABLE: u32 = 0x808029FC;
/// The modern class of a category row and its native counterpart.
pub(super) const CATEGORY_ROW: u32 = 0x80809787;
pub(super) const MODERN_CATEGORY_ROW_SIZE: usize = 32;
const SELECTION: u32 = 0x808029D8;
const MODERN_SELECTION_SIZE: usize = 0x90;
/// Category masks are 14 words modern and 10 native, one bit per dictionary index.
const MODERN_MASK_WORDS: usize = 14;
const NATIVE_MASK_WORDS: usize = 10;

fn layout(class: u32) -> Option<&'static [Field]> {
    use Elem::*;
    use Field::*;
    const EXPRESSION_PAIR: &[Field] = &[Expression, Expression];
    Some(match class {
        0x808029E4 => &[Raw(16), Reference, Raw(32)],
        0x808029E5 => &[Pointer, Raw(8), Dropped(8)],
        0x808029EF => &[
            Raw(24),
            Expression,
            Pointer,
            Pointer,
            Expression,
            Expression,
        ],
        0x808029FB | 0x808029E0 => &[Raw(4)],
        0x808029F9 => &[Raw(16), Reference, Raw(16)],
        0x808029DF => &[Raw(0x14)],
        0x808029F7 => &[
            Raw(8),
            Reference,
            Array(Categories),
            Raw(0x38),
            Reference,
            Raw(8),
            Pointer,
        ],
        0x808029F8 => &[Raw(8), Reference],
        0x808029F6 => &[Raw(24), Reference],
        0x808029E3 | 0x808029E2 => &[],
        0x808029D3 => &[Array(Selections), Raw(24), Reference],
        0x808029DB => &[
            Raw(8),
            Reference,
            Raw(0x50),
            Reference,
            Raw(16),
            Pointer,
            Pointer,
            Pointer,
        ],
        // a hashed parameter: the hash, then the sequencer holder the third pointer names
        0x808029EA => &[Raw(4)],
        0x808029EE => &[
            Raw(24),
            Expression,
            Pointer,
            Pointer,
            Expression,
            Expression,
            Expression,
            Raw(8),
            Reference,
            Raw(8),
            Dropped(8),
        ],
        0x808029E8 | 0x808029F3 => &[Expression],
        0x808029E7 => &[Expression, Expression, Expression, Expression, Raw(8)],
        0x808029F2 => EXPRESSION_PAIR,
        SELECTION => &[
            Raw(8),
            Array(Categories),
            Raw(0x38),
            Reference,
            Raw(8),
            Pointer,
            Raw(16),
            Reference,
        ],
        0x8080976E => &[Masks(2), Raw(8)],
        0x8080976D => &[Masks(4), Raw(8)],
        0x808029DE => &[Raw(8)],
        // settings trailers: four values, an expression valued block, a referenced provider
        0x80802A08 => &[Raw(16)],
        0x808029D5 => &[Expression],
        0x808029D7 => &[Raw(40), Reference, Raw(8)],
        _ => return None,
    })
}

/// Records made only of 32-bit fields sit at 4-byte alignment, the referenced provider at 16,
/// the rest begin with 64-bit fields.
pub(super) fn alignment(class: u32) -> usize {
    match class {
        0x808029FB | 0x808029DF | 0x808029E0 | 0x808029E3 | 0x808029E2 | 0x8080976E
        | 0x8080976D | 0x808029DE | 0x80802A08 | 0x808029EA => 4,
        0x808029D7 => 16,
        _ => 8,
    }
}

/// The number of masks in a trailer of `class`.
pub(super) fn mask_count(class: u32) -> Result<usize> {
    match class {
        0x8080976E => Ok(2),
        0x8080976D => Ok(4),
        other => bail!("movement category trailer class {other:08X} is not translated"),
    }
}

/// A record placed after its owner: its native position and what fills it.
enum Deferred {
    Array {
        descriptor: usize,
        elem: Elem,
        header: usize,
        count: usize,
    },
    Record {
        pointer: usize,
        body: usize,
    },
}

/// The native bytes of one sub table or trailer in the making. Alignment is absolute: `base` is
/// where the bytes will sit in the owner.
pub(super) struct Blob {
    pub out: Vec<u8>,
    base: usize,
    /// The least end a record body may have: a record reserves four body bytes.
    floor: usize,
    /// The scalar constants of the code array written last, for the constants that follow it.
    scalars: Vec<u8>,
}

impl Blob {
    fn new(len: usize, base: usize) -> Self {
        Blob {
            out: vec![0; len],
            base,
            floor: 0,
            scalars: Vec::new(),
        }
    }

    fn array_header(&mut self, count: usize, class: u32) -> usize {
        let at = ((self.base + self.out.len() + 19) & !15) - self.base;
        self.out.resize(at, 0);
        self.out[at - 4..at].copy_from_slice(&NATIVE_MARKER.to_le_bytes());
        self.out.extend((count as u64).to_le_bytes());
        self.out.extend(u64::from(class).to_le_bytes());
        at
    }

    /// Place an embedded record: the class word, then its body at the record's alignment.
    fn prefix(&mut self, class: u32) -> Result<usize> {
        let native = native_class(class);
        ensure!(
            native != class,
            "movement embedded record class {class:08X} has no native counterpart"
        );
        let align = alignment(class);
        let body = ((self.base + self.out.len() + 4 + align - 1) & !(align - 1)) - self.base;
        self.out.resize(body, 0);
        self.out[body - 4..body].copy_from_slice(&native.to_le_bytes());
        self.floor = body + 4;
        Ok(body)
    }

    fn close(&mut self) {
        if self.out.len() < self.floor {
            self.out.resize(self.floor, 0);
        }
    }

    fn pointer(&mut self, at: usize, target: usize) -> Result<()> {
        put(
            &mut self.out,
            at,
            &(target as i64 - at as i64).to_le_bytes(),
        )
    }
}

pub(super) struct Rewriter<'a> {
    p: &'a Payload,
    resources: &'a Resources,
    /// The input count of the expression that owns the records, for its inline sequencers.
    inputs: usize,
}

impl<'a> Rewriter<'a> {
    pub(super) fn new(p: &'a Payload, resources: &'a Resources, inputs: usize) -> Self {
        Rewriter {
            p,
            resources,
            inputs,
        }
    }

    /// The native 8 bytes of the modern reference at `at`: a tag with a form flag of 1 or 2 is
    /// the tag itself, else the 64-bit hash names it.
    pub(super) fn reference(&self, at: usize) -> Result<[u8; 8]> {
        let p = self.p;
        let (tag, flag, hash) = (p.u32(at)?, p.u32(at + 4)?, p.u64(at + 8)?);
        let modern = if tag == u32::MAX && flag == 0 && hash != 0 {
            self.resources.hash(hash)?
        } else {
            ensure!(
                flag <= 2 && hash == 0,
                "movement reference at {at:X} has an unknown form"
            );
            tag
        };
        let native = self.resources.tag(modern)?;
        let mut out = [0; 8];
        out[..4].copy_from_slice(&native.to_le_bytes());
        Ok(out)
    }

    /// Write the native fixed part of the record of `class` at modern `at`; the deferred
    /// targets come back in field order with the modern size consumed.
    fn fixed(&self, blob: &mut Blob, class: u32, at: usize) -> Result<(Vec<Deferred>, usize)> {
        let p = self.p;
        let fields = layout(class).with_context(|| {
            format!("movement embedded record class {class:08X} is not translated")
        })?;
        let mut deferred = Vec::new();
        let mut off = at;
        for field in fields {
            match *field {
                Field::Raw(n) => {
                    blob.out.extend(
                        p.0.get(off..off + n)
                            .context("movement embedded record extent")?,
                    );
                    off += n;
                }
                Field::Dropped(n) => off += n,
                Field::Reference => {
                    blob.out.extend(self.reference(off)?);
                    off += 16;
                }
                Field::Array(elem) => {
                    self.descriptor(blob, &mut deferred, off, elem)?;
                    off += 16;
                }
                Field::Pointer => {
                    let pointer = blob.out.len();
                    blob.out.extend([0; 8]);
                    if p.u64(off)? != 0 {
                        deferred.push(Deferred::Record {
                            pointer,
                            body: p.pointer(off)?,
                        });
                    }
                    off += 8;
                }
                Field::Expression => {
                    blob.out.extend(p.bytes::<8>(off)?);
                    self.descriptor(blob, &mut deferred, off + 8, Elem::Code)?;
                    self.descriptor(blob, &mut deferred, off + 24, Elem::Constants)?;
                    blob.out.extend(p.bytes::<16>(off + 40)?);
                    off += 0x38;
                }
                Field::Masks(n) => {
                    let words = n * MODERN_MASK_WORDS;
                    let modern: Vec<u32> = (0..words)
                        .map(|k| p.u32(off + k * 4))
                        .collect::<Result<_>>()?;
                    for word in self.masks(&modern, n) {
                        blob.out.extend(word.to_le_bytes());
                    }
                    off += words * 4;
                }
            }
        }
        Ok((deferred, off - at))
    }

    fn descriptor(
        &self,
        blob: &mut Blob,
        deferred: &mut Vec<Deferred>,
        at: usize,
        elem: Elem,
    ) -> Result<()> {
        let p = self.p;
        let count = usize::try_from(p.u64(at)?)?;
        let descriptor = blob.out.len();
        blob.out.extend((count as u64).to_le_bytes());
        blob.out.extend([0; 8]);
        if p.u64(at + 8)? != 0 {
            deferred.push(Deferred::Array {
                descriptor,
                elem,
                header: p.pointer(at + 8)?,
                count,
            });
        } else {
            ensure!(
                count == 0,
                "movement embedded array has a count without rows"
            );
        }
        Ok(())
    }

    fn deferred(&self, blob: &mut Blob, list: Vec<Deferred>) -> Result<()> {
        let p = self.p;
        for entry in list {
            match entry {
                Deferred::Record { pointer, body } => {
                    let class = p.u32(body - 4)?;
                    let at = blob.prefix(class)?;
                    blob.pointer(pointer, at)?;
                    self.record(blob, class, body)?;
                }
                Deferred::Array {
                    descriptor,
                    elem,
                    header,
                    count,
                } => {
                    let class = p.u32(header + 8)?;
                    let rows = header + 16;
                    let at = match elem {
                        Elem::Code => {
                            ensure!(class == CODE, "movement inline sequencer class differs");
                            let code = p.0.get(rows..rows + count).context("sequencer extent")?;
                            let native = procedural::lower_program(code, 256, self.inputs.max(1))
                                .with_context(|| {
                                format!("movement inline sequencer at {rows:X}")
                            })?;
                            ensure!(
                                native.len() == count,
                                "sequencer lowering changed its length"
                            );
                            blob.scalars = code.to_vec();
                            let at = blob.array_header(count, CODE);
                            blob.out.extend(native);
                            at
                        }
                        Elem::Constants => {
                            ensure!(class == VECTORS, "movement inline constant class differs");
                            let at = blob.array_header(count, VECTORS);
                            let mut table =
                                p.0.get(rows..rows + count * 16)
                                    .context("constant extent")?
                                    .to_vec();
                            super::super::constants::broadcast(&blob.scalars, &mut table);
                            blob.out.extend(table);
                            at
                        }
                        Elem::Categories => {
                            ensure!(class == CATEGORY_ROW, "movement category row class differs");
                            let at = blob.array_header(count, native_class(CATEGORY_ROW));
                            blob.out.extend(self.category_rows(rows, count)?);
                            at
                        }
                        Elem::Selections => {
                            ensure!(class == SELECTION, "movement selection class differs");
                            let at = blob.array_header(count, native_class(SELECTION));
                            let mut nested = Vec::new();
                            for index in 0..count {
                                let (mut inner, size) = self.fixed(
                                    blob,
                                    SELECTION,
                                    rows + index * MODERN_SELECTION_SIZE,
                                )?;
                                ensure!(size == MODERN_SELECTION_SIZE, "selection size");
                                nested.append(&mut inner);
                            }
                            blob.pointer(descriptor + 8, at)?;
                            self.deferred(blob, nested)?;
                            continue;
                        }
                    };
                    blob.pointer(descriptor + 8, at)?;
                }
            }
        }
        Ok(())
    }

    fn record(&self, blob: &mut Blob, class: u32, at: usize) -> Result<()> {
        let (deferred, _) = self.fixed(blob, class, at)?;
        blob.close();
        self.deferred(blob, deferred)
    }

    /// The native rows of a category table: the hash and zeros, then the dictionary reference.
    pub(super) fn category_rows(&self, rows: usize, count: usize) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(count * 24);
        for index in 0..count {
            let row = rows + index * MODERN_CATEGORY_ROW_SIZE;
            out.extend(self.p.bytes::<16>(row)?);
            out.extend(self.reference(row + 16)?);
        }
        Ok(out)
    }

    /// The native words of `count` masks: every set modern bit re-indexed through the category
    /// correspondence, bits of categories the native dictionary lacks dropped.
    pub(super) fn masks(&self, modern: &[u32], count: usize) -> Vec<u32> {
        let mut out = Vec::with_capacity(count * NATIVE_MASK_WORDS);
        for mask in modern.chunks_exact(MODERN_MASK_WORDS).take(count) {
            let mut native = [0u32; NATIVE_MASK_WORDS];
            for (word, bits) in mask.iter().enumerate() {
                for bit in 0..32 {
                    if bits >> bit & 1 == 0 {
                        continue;
                    }
                    let index = word * 32 + bit;
                    if let Some(Some(target)) = self.resources.categories.get(index) {
                        let target = usize::from(*target);
                        if target / 32 < NATIVE_MASK_WORDS {
                            native[target / 32] |= 1 << (target % 32);
                        }
                    }
                }
            }
            out.extend(native);
        }
        out
    }

    /// The trailer body of `class` at modern `body`, as native bytes: the masks, then the
    /// tail word pair the layouts share.
    pub(super) fn trailer(&self, class: u32, body: usize) -> Result<Vec<u8>> {
        let count = mask_count(class)?;
        let words = count * MODERN_MASK_WORDS;
        let modern: Vec<u32> = (0..words)
            .map(|k| self.p.u32(body + k * 4))
            .collect::<Result<_>>()?;
        let mut out: Vec<u8> = self
            .masks(&modern, count)
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        out.extend(self.p.bytes::<8>(body + words * 4)?);
        Ok(out)
    }

    /// An embedded record of `class` at modern `at` with its nested data, as the bytes to append
    /// at owner offset `base` (the class word at the record's alignment, then the body) beside
    /// the body's offset within them.
    pub(super) fn embedded(&self, class: u32, at: usize, base: usize) -> Result<(Vec<u8>, usize)> {
        let mut blob = Blob::new(0, base);
        let body = blob.prefix(class)?;
        self.record(&mut blob, class, at)?;
        Ok((blob.out, body))
    }

    /// The rewritten sub table named by the descriptor at `descriptor`: its rows, then every
    /// body behind its class word with its nested data. Returns the modern header and the
    /// bytes that follow the native header.
    pub(super) fn table(&self, descriptor: usize) -> Result<(usize, Vec<u8>, usize)> {
        let p = self.p;
        let count = usize::try_from(p.u64(descriptor)?)?;
        let header = p.pointer(descriptor + 8)?;
        ensure!(
            p.u32(header + 8)? == SUB_TABLE,
            "movement expression sub table class {:08X} is not translated",
            p.u32(header + 8)?
        );
        let mut blob = Blob::new(16 * count, 0);
        for index in 0..count {
            let row = header + 16 + index * 16;
            let body = row + usize::try_from(p.u64(row)?)?;
            let kind = p.u64(row + 8)?;
            let class = p.u32(body - 4)?;
            let at = blob.prefix(class)?;
            put(
                &mut blob.out,
                index * 16,
                &((at - index * 16) as u64).to_le_bytes(),
            )?;
            put(&mut blob.out, index * 16 + 8, &kind.to_le_bytes())?;
            self.record(&mut blob, class, body)?;
        }
        Ok((header, blob.out, count))
    }
}
