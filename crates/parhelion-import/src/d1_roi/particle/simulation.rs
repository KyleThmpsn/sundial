//! Simulation-owned controls, separate from programs, route tables and submission queues.
use crate::{d1_roi::Source, presentation::put, tiger::payload::Payload};
use anyhow::{Result, ensure};

pub struct SimulationControls {
    flags: u8,
    seed: u8,
    position: u8,
    direction: u8,
    duration: [u8; 4],
    bounds: [u8; 4],
    scalar_6c: [u8; 4],
    scalar_70: [u8; 4],
    quantized: [u8; 2],
    spatial: u8,
    state: u8,
    workspace: u8,
    force_space: [u8; 2],
}

impl Source {
    pub fn particle_simulation_controls(&self, definition: u32) -> Result<SimulationControls> {
        ensure!(
            self.reference(definition)? == 0x80801A6A,
            "Particle simulation controls have another definition class"
        );
        let source = self.read(definition)?;
        ensure!(
            source.0.len() >= 0x138 && source.u64(0)? == source.0.len() as u64,
            "Particle simulation definition extent differs"
        );
        let flags = source.u8(0x18)?;
        ensure!(
            flags & !0x0F == 0 && source.u16(0x1A)? == 0,
            "Source particle flags require another simulation path"
        );
        let position = source.u8(0xDD)?;
        let direction = source.u8(0xDE)?;
        ensure!(
            position <= 4 && direction <= 4,
            "Particle spawn selectors require another producer"
        );
        let state = source.u8(0x109)?;
        let workspace = source.u8(0x110)?;
        ensure!(
            (1..=16).contains(&state) && workspace % 16 == 0 && workspace <= 160,
            "Particle state or material workspace exceeds native storage"
        );
        let duration = source.f32(0xE8)?;
        ensure!(duration >= 0.0, "Particle lifetime ceiling is negative");
        for offset in [0xEC, 0xF4, 0xFC] {
            source.f32(offset)?;
        }
        let result = SimulationControls {
            flags,
            seed: source.u8(0x19)?,
            position,
            direction,
            duration: source.bytes(0xE8)?,
            bounds: source.bytes(0xEC)?,
            scalar_6c: source.bytes(0xFC)?,
            scalar_70: source.bytes(0xF4)?,
            quantized: source.bytes(0x104)?,
            spatial: source.u8(0x108)?,
            state,
            workspace,
            force_space: source.bytes(0x112)?,
        };
        self.unchanged()?;
        Ok(result)
    }
}

impl SimulationControls {
    /// Stored seconds remain Float32. Each original initializer converts them to its own tick
    /// representation. Copying the source owner's UInt32 ticks would corrupt native UInt64 time.
    /// Fields outside these qualified consumers, including the queue and routes, stay untouched.
    pub fn write(&self, target: &mut Payload) -> Result<()> {
        ensure!(
            target.0.len() >= 0x150 && target.u64(0)? == target.0.len() as u64,
            "Native particle simulation definition extent differs"
        );
        let mut result = target.clone();
        put(&mut result.0, 0x44, &[self.flags, 0, self.seed])?;
        put(&mut result.0, 0x115, &[self.position, self.direction])?;
        for (offset, bytes) in [
            (0x120, &self.duration),
            (0x124, &self.bounds),
            (0x12C, &self.scalar_70),
            (0x134, &self.scalar_6c),
        ] {
            put(&mut result.0, offset, bytes)?;
        }
        put(&mut result.0, 0x13C, &self.quantized)?;
        put(&mut result.0, 0x140, &[self.spatial, self.state])?;
        put(&mut result.0, 0x147, &[self.workspace])?;
        put(&mut result.0, 0x149, &self.force_space)?;
        *target = result;
        Ok(())
    }
}
