//! Registration condition words, laid out as stock entities hold them.
//!
//! A resource map row names an instance condition word at `+00` and the bits it tests at `+02`.
//! Across the 46,235 entities of the clean Shadowkeep set the words form one dense bitset: an
//! entity without conditioned rows has none, and otherwise two words per 32 bits in use, filled
//! from bit 0 of word 0. No stock entity uses a word past 4 or gives each component words of its
//! own. Rows taken from the entity an output is built on keep their word and bits, so they share
//! words with that entity's other components as in stock. A row from another source entity, or an
//! authored provider's condition, takes the next free bit after the template's, one bit for each
//! condition of its own source.
use crate::tiger::payload::Payload;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    /// Another source entity, by the order rows first name it.
    Entity(usize),
    /// An authored provider's condition, by the order providers are placed. Its source word is
    /// unknown, so no two providers share a bit.
    Authored(usize),
}

pub(in crate::tiger::entity) struct Conditions<'a> {
    template: &'a Payload,
    sources: Vec<&'a Payload>,
    authored: usize,
    /// Bit positions, word times 16 plus bit, given to each source's original positions.
    given: BTreeMap<(Source, u32), u32>,
    /// The first position past the template's own conditions and every position given since.
    next: u32,
    /// The first position past every position a written row uses.
    end: u32,
}

/// The first position past the highest bit of `bits` in `word`.
fn past(word: u32, bits: u16) -> u32 {
    word * 16 + 16 - bits.leading_zeros()
}

impl<'a> Conditions<'a> {
    pub fn new(template: &'a Payload) -> Result<Self> {
        let mut next = 0;
        for row in template.array(88, 40, Some(0x80809C22))? {
            let bits = template.u16(row + 2)?;
            if bits != 0 {
                let word =
                    u32::try_from(template.i16(row)?).context("Negative condition word index")?;
                next = next.max(past(word, bits));
            }
        }
        Ok(Self {
            template,
            sources: Vec::new(),
            authored: 0,
            given: BTreeMap::new(),
            next,
            end: 0,
        })
    }

    /// The word and bits for a row of `entity` that tests `bits` of its condition word `word`.
    pub fn place(&mut self, entity: &'a Payload, word: usize, bits: u16) -> Result<(i16, u16)> {
        let word = u32::try_from(word)?;
        if entity.0 == self.template.0 {
            self.end = self.end.max(past(word, bits));
            return Ok((i16::try_from(word)?, bits));
        }
        let source = match self.sources.iter().position(|source| source.0 == entity.0) {
            Some(index) => index,
            None => {
                self.sources.push(entity);
                self.sources.len() - 1
            }
        };
        self.give(Source::Entity(source), word, bits)
    }

    /// The word and bits for an authored provider's condition.
    pub fn authored(&mut self, bits: u16) -> Result<(i16, u16)> {
        self.authored += 1;
        self.give(Source::Authored(self.authored), 0, bits)
    }

    fn give(&mut self, source: Source, word: u32, bits: u16) -> Result<(i16, u16)> {
        let positions = (0..16)
            .filter(|bit| bits >> bit & 1 != 0)
            .map(|bit| word * 16 + bit)
            .collect::<Vec<_>>();
        let fresh = positions
            .iter()
            .filter(|&&position| !self.given.contains_key(&(source, position)))
            .count() as u32;
        // A row tests one word, so its new bits start a word when they would cross one.
        if fresh > 0 && self.next % 16 + fresh > 16 {
            self.next = self.next.next_multiple_of(16);
        }
        let mut placed = Vec::new();
        for position in positions {
            let new = match self.given.get(&(source, position)) {
                Some(&new) => new,
                None => {
                    let new = self.next;
                    self.next += 1;
                    self.given.insert((source, position), new);
                    new
                }
            };
            placed.push(new);
        }
        let first = *placed.first().context("Condition row tests no bit")?;
        ensure!(
            placed.iter().all(|position| position / 16 == first / 16),
            "A condition row's bits span words"
        );
        let new_bits = placed
            .iter()
            .fold(0u16, |bits, position| bits | 1 << (position % 16));
        self.end = self.end.max(past(first / 16, new_bits));
        Ok((i16::try_from(first / 16)?, new_bits))
    }

    /// Zeroed words covering every written row, two per 32 bits as in stock.
    pub fn words(&self) -> Result<Vec<Vec<u8>>> {
        let count = usize::try_from(self.end.div_ceil(32) * 2)?;
        ensure!(
            count <= i16::MAX as usize,
            "compiled condition mask capacity exceeded"
        );
        Ok(vec![vec![0; 2]; count])
    }
}
