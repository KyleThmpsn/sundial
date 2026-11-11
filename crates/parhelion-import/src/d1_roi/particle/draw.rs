//! Draw-owned definition fields. Native queues, bindings and simulation remain separate.
use super::material::Material;
use crate::{d1_roi::Source, presentation::put, tiger::payload::Payload};
use anyhow::{Result, ensure};

/// Geometries two and three draw ribbons from paired vertex records.
pub(super) fn ribbon(geometry: u8) -> bool {
    matches!(geometry, 2 | 3)
}

pub struct DrawControls {
    appearance: u8,
    secondary: u8,
    priority: u8,
    repeats: u8,
    geometry: u8,
    orientation: u8,
    sort_category: u8,
    sort_offset: [u8; 4],
}

impl Source {
    pub fn particle_draw_controls(&self, definition: u32) -> Result<DrawControls> {
        ensure!(
            self.reference(definition)? == 0x80801A6A,
            "Particle draw controls have another definition class"
        );
        let source = self.read(definition)?;
        ensure!(
            source.0.len() >= 0x138 && source.u64(0)? == source.0.len() as u64,
            "Particle draw definition extent differs"
        );
        let geometry = source.u8(0x114)?;
        let appearance = source.u8(0x10A)?;
        let secondary = source.u8(0x10C)?;
        let copies = source.u8(0x10F)?;
        // Geometry 5 is a straight trail of copied records, which both builds key alike and draw
        // per instance as they do ordinary particles. Shadowkeep's selector 5 expansion reads the
        // same routes 33 to 35 and the stored multiplier the definition writer carries.
        ensure!(
            matches!(geometry, 0..=3 | 5) && appearance > 0 && secondary > 0,
            "Particle draw controls require another geometry route"
        );
        ensure!(
            copies == if ribbon(geometry) { 2 } else { 1 }
                && u16::from(source.u8(0x10B)?) == u16::from(appearance) * u16::from(copies),
            "Particle draw appearance stride differs"
        );
        let layout = |rows| u32::from(geometry) << 28 | u32::from(rows);
        ensure!(
            source.u32(0x1C)? == layout(appearance) && source.u32(0x20)? == layout(secondary),
            "Source particle layout keys disagree with their draw controls"
        );
        source.f32(0xF0)?;
        let result = DrawControls {
            appearance,
            secondary,
            priority: source.u8(0x10D)?,
            repeats: source.u8(0x10E)?,
            geometry,
            orientation: source.u8(0x115)?,
            sort_category: source.u8(0x111)?,
            sort_offset: source.bytes(0xF0)?,
        };
        self.unchanged()?;
        Ok(result)
    }
}

impl DrawControls {
    /// The paired material must consume the base appearance stride. Ribbon adaptation fetches
    /// each record twice in its vertex shader, so native storage keeps only one copy. No source
    /// copy count is written to native +145, which selects a separate submission queue.
    /// This fragment preserves the target's routes, native bindings and simulation fields.
    pub fn write(&self, target: &mut Payload, material: &Material) -> Result<()> {
        ensure!(
            target.0.len() >= 0x150 && target.u64(0)? == target.0.len() as u64,
            "Native particle draw definition extent differs"
        );
        ensure!(
            material.appearance_rows == Some(self.appearance)
                && material.ribbon == ribbon(self.geometry),
            "Particle material does not consume the translated draw layout"
        );
        let mut result = target.clone();
        for (offset, rows) in [(0x48, self.appearance), (0x4C, self.secondary)] {
            let key = u32::from(self.geometry) << 28 | u32::from(rows);
            put(&mut result.0, offset, &key.to_le_bytes())?;
        }
        put(&mut result.0, 0x128, &self.sort_offset)?;
        for (offset, value) in [
            (0x142, self.appearance),
            (0x143, self.secondary),
            (0x144, self.priority),
            (0x146, self.repeats),
            (0x148, self.sort_category),
            (0x14B, self.geometry),
            (0x14C, self.orientation),
        ] {
            result.0[offset] = value;
        }
        *target = result;
        Ok(())
    }
}
