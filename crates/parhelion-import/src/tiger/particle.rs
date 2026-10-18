//! Native CPU particle programs with explicit phases, routes and value storage.
use super::payload::Payload;
use crate::presentation::{append, put};
use anyhow::{Context, Result, ensure};

pub(crate) struct Program {
    sections: [Vec<u8>; 8],
    constants: Vec<[f32; 4]>,
    routes: [[u8; 2]; 56],
    defaults: Vec<[f32; 4]>,
    required_state_rows: u8,
}

impl Default for Program {
    fn default() -> Self {
        Self {
            sections: std::array::from_fn(|_| Vec::new()),
            constants: Vec::new(),
            routes: [[0xff, 0]; 56],
            defaults: Vec::new(),
            required_state_rows: 0,
        }
    }
}

impl Program {
    pub fn require_state_rows(&mut self, rows: u8) -> Result<()> {
        ensure!(
            (1..=64).contains(&rows),
            "Particle temporary state exceeds native scalar addressing"
        );
        self.required_state_rows = self.required_state_rows.max(rows);
        Ok(())
    }

    pub fn code(&mut self, phase: usize, bytes: &[u8]) -> Result<()> {
        let section = self
            .sections
            .get_mut(phase)
            .context("Particle phase outside native table")?;
        ensure!(
            section
                .len()
                .checked_add(bytes.len())
                .is_some_and(|len| len <= u16::MAX as usize),
            "Particle phase exceeds native length"
        );
        section.extend_from_slice(bytes);
        Ok(())
    }

    pub fn constant(&mut self, phase: usize, value: [f32; 4]) -> Result<()> {
        ensure!(
            value.iter().all(|v| v.is_finite()),
            "Nonfinite particle constant"
        );
        let index = if let Some(index) = self
            .constants
            .iter()
            .position(|v| v.map(f32::to_bits) == value.map(f32::to_bits))
        {
            index
        } else {
            ensure!(
                self.constants.len() < 256,
                "Particle constant table exceeds its byte index"
            );
            self.constants.push(value);
            self.constants.len() - 1
        };
        self.code(phase, &[0x34, u8::try_from(index)?])
    }

    pub fn route(&mut self, index: usize, bank: u8, scalar: u8) -> Result<()> {
        ensure!(bank <= 6, "Particle route bank is unsupported");
        *self
            .routes
            .get_mut(index)
            .context("Particle route outside native table")? = [bank, scalar];
        Ok(())
    }

    pub fn default_value(&mut self, index: usize, value: [f32; 4]) -> Result<()> {
        ensure!(
            value.iter().all(|v| v.is_finite()),
            "Nonfinite particle default"
        );
        let slot = if let Some(slot) = self
            .defaults
            .iter()
            .position(|v| v.map(f32::to_bits) == value.map(f32::to_bits))
        {
            slot
        } else {
            ensure!(
                self.defaults.len() < 64,
                "Particle defaults exceed native scalar indices"
            );
            self.defaults.push(value);
            self.defaults.len() - 1
        };
        self.route(index, 6, u8::try_from(slot * 4)?)
    }

    pub fn write(&self, source: &Payload, state_rows: u8, appearance_rows: u8) -> Result<Payload> {
        ensure!(
            source.0.len() >= 0x150 && source.u64(0)? == source.0.len() as u64,
            "Particle definition envelope differs"
        );
        ensure!(
            (1..=64).contains(&state_rows) && (1..=64).contains(&appearance_rows),
            "Particle record counts are outside native scalar addressing"
        );
        let state_rows = state_rows.max(self.required_state_rows);
        // The frame consumer reads and writes angular state even when its
        // optional initialization source is absent. Authored mutable records
        // are banks zero and one. The route loop below checks their lane bounds.
        ensure!(
            self.routes[23][0] <= 1,
            "Particle angular update requires writable state"
        );
        for [bank, scalar] in self.routes {
            let count = match bank {
                0 => Some(usize::from(state_rows)),
                1 => Some(usize::from(appearance_rows)),
                6 => Some(self.defaults.len()),
                _ => None,
            };
            if let Some(count) = count {
                ensure!(
                    usize::from(scalar) < count * 4,
                    "Particle route exceeds its record"
                );
            }
        }
        let mut payload = source.clone();
        let vectors = |values: &[[f32; 4]]| {
            values
                .iter()
                .flatten()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>()
        };
        append(&mut payload.0, 8, 0x80800090, &vectors(&self.defaults), 16)?;
        let code = self.sections.iter().flatten().copied().collect::<Vec<_>>();
        ensure!(
            code.len() <= u16::MAX as usize,
            "Particle program exceeds native cumulative phase offsets"
        );
        append(&mut payload.0, 0x50, 0x80800009, &code, 1)?;
        append(
            &mut payload.0,
            0x60,
            0x80800090,
            &vectors(&self.constants),
            16,
        )?;
        for (index, section) in self.sections.iter().enumerate() {
            put(
                &mut payload.0,
                0x70 + index * 2,
                &u16::try_from(section.len())?.to_le_bytes(),
            )?;
        }
        for (index, route) in self.routes.iter().enumerate() {
            put(&mut payload.0, 0x80 + index * 2, route)?;
        }
        put(&mut payload.0, 0x100, &[0; 16])?;
        put(&mut payload.0, 0x141, &[state_rows, appearance_rows])?;
        let length = u64::try_from(payload.0.len())?;
        put(&mut payload.0, 0, &length.to_le_bytes())?;
        Ok(payload)
    }
}
