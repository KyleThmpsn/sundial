//! Spatial locator declarations and their runtime storage.
//!
//! Each locator row names a range of runtime points and a range of point declarations.
//! Rise of Iron resolver `4345A0` and Shadowkeep resolver `589670` read the same fields
//! with wider native rows. Both dispatch declaration kinds 3 through 23 from one switch,
//! where native inserts a new case at 5 and a final case at 25. Source kind 5 and above
//! therefore move up by one. Native flags insert bit 4, which selects the cached-point
//! path for callers whose mode bit is clear, while bit 3 serves the other callers. Source
//! tests bit 3 for every caller, so it supplies both native bits. Higher source bits shift
//! up by one, as the native census of the same declaration populations shows.
use super::*;
use crate::{presentation::append, tiger::allocation::Row};

const OWNER: u32 = 0x808001B0;
const STATE: u32 = 0x808041D1;
const SLOT: u32 = 0x808008C5;

pub struct Locators {
    rows: Vec<[u8; 5]>,
    declarations: Vec<[u8; 8]>,
    names: Vec<[u8; 4]>,
    owners: Vec<[u8; 32]>,
    points: Vec<[u8; 32]>,
    states: Vec<u8>,
    slots: usize,
}

fn flags(source: u8) -> Result<u8> {
    ensure!(
        source & 0x80 == 0,
        "Source locator flag bit 7 has no native position"
    );
    Ok((source & 0x0F) | ((source & 0x08) << 1) | ((source & 0x70) << 1))
}

fn kind(source: u8) -> Result<u8> {
    Ok(match source {
        0..=4 => source,
        5..=23 => source + 1,
        _ => anyhow::bail!("Source locator kind {source} is outside the original switch"),
    })
}

fn rows<const N: usize>(source: &Payload, field: usize, class: u32) -> Result<Vec<[u8; N]>> {
    source
        .array(field, N, Some(class))?
        .into_iter()
        .map(|at| source.bytes::<N>(at))
        .collect()
}

fn within(start: u8, count: u8, length: usize) -> bool {
    usize::from(start) + usize::from(count) <= length
}

impl Locators {
    pub fn read(source: &Payload) -> Result<Self> {
        let instance = source.pointer(16)?;
        let definition = source.pointer(24)?;
        let result = Self {
            rows: rows(source, definition + 0x1B0, 0x80800429)?,
            declarations: rows(source, definition + 0x1C0, 0x80800819)?,
            names: rows(source, definition + 0x1D0, 0x80800696)?,
            owners: rows(source, instance + 0x150, OWNER)?,
            points: rows(source, instance + 0x160, OWNER)?,
            states: rows::<1>(source, instance + 0x170, STATE)?
                .into_iter()
                .map(|[state]| state)
                .collect(),
            slots: {
                let slots = rows::<24>(source, instance + 0x180, SLOT)?;
                let mut empty = [0; 24];
                empty[..4].fill(0xFF);
                ensure!(
                    slots.iter().all(|slot| *slot == empty),
                    "Source locator slots carry runtime state"
                );
                slots.len()
            },
        };
        // Stock sequences hold up to three owner transform rows, in the same declaration-kind
        // patterns as source sequences with two or three. Four rows have no native witness.
        ensure!(
            result.owners.len() <= 3
                && result.points.len() == result.states.len()
                && result.slots == result.declarations.len()
                && result.points.len() <= 255
                && result.declarations.len() <= 255,
            "Source locator storage dimensions differ"
        );
        for row in &result.rows {
            ensure!(
                within(row[0], row[1], result.points.len())
                    && within(row[2], row[3], result.declarations.len()),
                "Source locator range exceeds its storage"
            );
            flags(row[4])?;
        }
        for declaration in &result.declarations {
            ensure!(
                within(declaration[1], declaration[2], result.points.len()),
                "Source locator declaration exceeds its points"
            );
            kind(declaration[0])?;
            flags(declaration[3])?;
            // Stored point states repeat their declaration's flags with bit 6 clear. Each
            // stored byte is translated as written, which keeps that distinction natively.
            for point in declaration[1]..declaration[1] + declaration[2] {
                ensure!(
                    result.states[usize::from(point)] | 0x40 == declaration[3] | 0x40,
                    "Source locator point state differs from its declaration"
                );
            }
        }
        for &state in &result.states {
            flags(state)?;
        }
        Ok(result)
    }

    pub fn count(&self) -> usize {
        self.rows.len()
    }

    /// Append an empty-named kind-0 locator at the owner origin and return its ordinal. A
    /// presentation table event without a locator plays there. Native port `80BC57DB` of such a
    /// sequence adds this locator with one identity point and flags `3F`, the native form of
    /// source flags `1F`, and its table event names it.
    pub fn add_origin(&mut self) -> Result<u8> {
        let ordinal = u8::try_from(self.rows.len()).context("Source locator capacity")?;
        let point = u8::try_from(self.points.len()).context("Source locator point capacity")?;
        let declaration =
            u8::try_from(self.declarations.len()).context("Source locator declaration capacity")?;
        let empty = 0x811C9DC5u32.to_le_bytes();
        let mut identity = [0; 32];
        identity[12..16].copy_from_slice(&1f32.to_le_bytes());
        identity[28..32].copy_from_slice(&1f32.to_le_bytes());
        self.rows.push([point, 1, declaration, 1, 0x1F]);
        let mut row = [0, point, 1, 0x1F, 0, 0, 0, 0];
        row[4..].copy_from_slice(&empty);
        self.declarations.push(row);
        self.names.push(empty);
        self.points.push(identity);
        self.states.push(0x1F);
        self.slots += 1;
        Ok(ordinal)
    }

    /// Replace the native declarations, runtime storage and capacities. The returned row
    /// replaces the native locator allocation node.
    pub(crate) fn write(&self, target: &mut Payload) -> Result<Row> {
        let instance = target.pointer(16)?;
        let definition = target.pointer(24)?;
        let mut output = target.clone();
        let mut rows = Vec::new();
        for row in &self.rows {
            rows.extend_from_slice(&row[..4]);
            rows.extend([flags(row[4])?, 0]);
        }
        let mut declarations = Vec::new();
        for declaration in &self.declarations {
            declarations.extend([kind(declaration[0])?, declaration[1], declaration[2], 0]);
            declarations.extend(u32::from(flags(declaration[3])?).to_le_bytes());
            declarations.extend_from_slice(&declaration[4..]);
            declarations.extend([0xFF, 0, 0, 0]);
        }
        let names = self.names.concat();
        let states = self
            .states
            .iter()
            .map(|&state| Ok(u16::from(flags(state)?).to_le_bytes()))
            .collect::<Result<Vec<_>>>()?
            .concat();
        let mut slot = [0; 24];
        slot[..8].fill(0xFF);
        let slots = slot.repeat(self.slots);
        append(&mut output.0, definition + 0x1B0, 0x80808617, &rows, 6)?;
        append(
            &mut output.0,
            definition + 0x1C0,
            0x80808615,
            &declarations,
            16,
        )?;
        append(&mut output.0, definition + 0x1D0, 0x80808614, &names, 4)?;
        let storage = [
            (0x170, 0x80809F75, self.owners.concat(), 32, 0x82C28E51),
            (0x180, 0x80809F75, self.points.concat(), 32, 0x4C2E87A1),
            (0x190, 0x80808618, states, 2, 0x6E385590),
            (0x1A0, 0x80809BFF, slots, 24, 0xA6E3DD20),
        ];
        let mut children = Vec::new();
        for (index, (field, class, bytes, stride, name)) in storage.into_iter().enumerate() {
            let count = bytes.len() / stride;
            append(
                &mut output.0,
                instance + field,
                u64::from(class),
                &bytes,
                stride,
            )?;
            put(
                &mut output.0,
                definition + 0x1E0 + index * 4,
                &u32::try_from(count)?.to_le_bytes(),
            )?;
            if count > 0 {
                children.push(Row::named(name, class, count, Vec::new()));
            }
        }
        *target = output;
        Ok(Row::named(0x846FDE3F, u32::MAX, 0, children))
    }
}
