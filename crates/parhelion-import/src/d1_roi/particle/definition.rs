//! Compose a CPU definition without inheriting stock simulation or routing fields.
use super::material::Material;
use crate::{d1_roi::Source, presentation::put, tiger::payload::Payload};
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

impl Source {
    /// Compose qualified source controls with the native two-locator binding topology.
    /// Shader and geometry resources remain obligations of the containing particle system.
    pub fn particle_definition(
        &self,
        tag: u32,
        material: &Material,
        inputs: &BTreeMap<u32, u32>,
        pool_rows: usize,
        identity: u32,
    ) -> Result<Payload> {
        ensure!(
            material.definition == Some(tag),
            "Particle material was not paired with this definition"
        );
        ensure!(
            identity & 0xFF80_0000 == 0x8280_0000,
            "Particle compiled identity is outside private storage"
        );
        let source = self.read(tag)?;
        let program = self.particle_program(tag)?;
        let draw = self.particle_draw_controls(tag)?;
        let simulation = self.particle_simulation_controls(tag)?;
        let bindings = self.particle_bindings(tag)?;
        // No source optional initialization or trailing object is silently discarded. The
        // embedded 3528 identity is the explicit absent-resource form in this CPU family.
        // The scalar block `+D8..+107` moves to native `+110` in field order. Byte `+DC` lands at
        // native `+114`, which stock definitions hold as 0 or 1 (2,968 of 26,250 hold 1) and the
        // native frame copies into its update context. Float `+E4` lands at native `+11C`, the
        // multiplier of the selector 5 record count, nonzero in exactly the 550 stock selector 5
        // definitions. Rise of Iron geometry 5 definitions likewise store it, so it is admitted
        // there alone.
        let geometry = source.u8(0x114)?;
        ensure!(
            source.u32(0x24)? == 0
                && source.u16(0xBE)? == 0
                && source.u32(0xD8)? == 0
                && source.u8(0xDC)? <= 1
                && source.u8(0xDF)? == 0
                && source.u32(0xE0)? == 0x811C9DC5
                && (geometry == 5 || source.u32(0xE4)? == 0)
                && source.u16(0x116)? == 0
                && source.bytes::<32>(0x118)? == [0; 32],
            "Particle definition requires additional control or resource translation"
        );
        // Selector 5 expansion publishes each record's normalized distance through route 35, as it
        // does its position through route 6, so the route must name writable particle storage.
        ensure!(
            geometry != 5 || matches!(source.u8(0x60 + 35 * 2)?, 0 | 1 | 0xFF),
            "Particle trail distance route requires writable private storage"
        );
        let state = source.u8(0x109)?;
        let appearance = source.u8(0x10A)?;
        let workspace = source.u8(0x110)?;
        ensure!(
            material.emitter_bytes <= usize::from(workspace),
            "Particle material reads beyond the emitter workspace"
        );
        program.validate_storage(state, appearance, workspace)?;
        let defaults = source.array(8, 16, Some(0x80800090))?.len();
        for offset in [0xE4, 0xF8, 0x100] {
            source.f32(offset)?;
        }
        let mut target = Payload(vec![0; 0x150]);
        put(&mut target.0, 0, &0x150u64.to_le_bytes())?;
        // The stored scalar block keeps its field order. Individual initialized controls are
        // also written by the simulation adapter, which preserves seconds rather than ticks.
        put(&mut target.0, 0x110, &source.bytes::<48>(0xD8)?)?;
        for index in 0..56 {
            let [bank, scalar] = if index < 47 {
                source.bytes::<2>(0x60 + index * 2)?
            } else {
                [0xFF, 0]
            };
            let rows = match bank {
                0 => Some(usize::from(state)),
                1 => Some(usize::from(appearance)),
                2 => None,
                4 => Some(16),
                5 => Some(usize::from(workspace) / 16),
                6 => Some(defaults),
                0xFF => {
                    ensure!(scalar == 0, "Absent particle route retains a lane");
                    None
                }
                _ => anyhow::bail!("Particle route requires another caller bank"),
            };
            let width = if matches!(index, 6..=9 | 11 | 18 | 20) {
                3
            } else {
                1
            };
            if let Some(rows) = rows {
                ensure!(
                    usize::from(scalar) + width <= rows * 4,
                    "Particle route {index} exceeds its value storage"
                );
            }
            put(&mut target.0, 0x80 + index * 2, &[bank, scalar])?;
        }
        for index in [0, 6, 7, 23, 27, 42] {
            ensure!(
                source.u8(0x60 + index * 2)? <= 1,
                "Particle route {index} requires writable private storage"
            );
        }
        put(&mut target.0, 0xF8, &identity.to_le_bytes())?;
        // Queue two is the native CPU submission policy. D1's appearance-copy byte is not
        // a queue selector and must never be forwarded here. Native queue budgets differ.
        target.0[0x145] = 2;
        program.write(&mut target)?;
        simulation.write(&mut target)?;
        draw.write(&mut target, material)?;
        bindings.write(&mut target, inputs, pool_rows)?;
        self.unchanged()?;
        Ok(target)
    }
}
