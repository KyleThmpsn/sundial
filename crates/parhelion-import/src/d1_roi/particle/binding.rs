//! Two ordered sequence locators and explicitly translated numeric-pool indices.
use crate::{
    d1_roi::Source,
    presentation::{append, put},
    tiger::payload::Payload,
};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

pub struct Bindings {
    inputs: Vec<u32>,
}

impl Source {
    pub fn particle_bindings(&self, definition: u32) -> Result<Bindings> {
        ensure!(
            self.reference(definition)? == 0x80801A6A,
            "Particle bindings have another definition class"
        );
        let source = self.read(definition)?;
        ensure!(
            source.0.len() >= 0x138 && source.u64(0)? == source.0.len() as u64,
            "Particle binding definition extent differs"
        );
        ensure!(
            source.u64(0xC0)? == 0,
            "Source particle optional initialization needs a separate binding policy"
        );
        let inputs = source
            .array(0xC8, 4, Some(0x80800007))?
            .into_iter()
            .map(|row| {
                let value = source.u32(row)?;
                ensure!(
                    value <= i32::MAX as u32,
                    "Negative source particle pool index"
                );
                Ok(value)
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            inputs.len() <= 256,
            "Particle input bindings exceed their byte operand"
        );
        self.unchanged()?;
        Ok(Bindings { inputs })
    }
}

impl Bindings {
    /// Original ordered rows, including repeated numeric-pool indices.
    pub fn inputs(&self) -> &[u32] {
        &self.inputs
    }

    /// The containing sequence supplies exactly two event locator entries. Definition ranges
    /// select the first and second entry separately, even when both name the same locator.
    /// Numeric indices are resolved against the containing sequence's translated output pool.
    pub fn write(
        &self,
        target: &mut Payload,
        inputs: &BTreeMap<u32, u32>,
        pool_rows: usize,
    ) -> Result<()> {
        ensure!(
            target.0.len() >= 0x150 && target.u64(0)? == target.0.len() as u64,
            "Native particle binding definition extent differs"
        );
        ensure!(
            self.inputs.iter().copied().collect::<BTreeSet<_>>()
                == inputs.keys().copied().collect(),
            "Particle input mappings do not cover exactly the source pool indices"
        );
        let mut rows = Vec::with_capacity(self.inputs.len() * 4);
        for index in &self.inputs {
            let value = *inputs
                .get(index)
                .context("Particle input mapping is absent")?;
            ensure!(
                value <= i32::MAX as u32 && usize::try_from(value)? < pool_rows,
                "Particle input mapping exceeds its native numeric pool"
            );
            rows.extend_from_slice(&value.to_le_bytes());
        }
        let mut result = target.clone();
        append(&mut result.0, 0x18, 0x80800009, &[0, 1], 1)?;
        append(&mut result.0, 0x28, 0x80806E2D, &[], 2)?;
        append(&mut result.0, 0x100, 0x80800007, &rows, 4)?;
        // Primary and secondary ranges have one entry each. The two optional ranges are
        // empty, with no extra combined transforms. +47 counts runtime transform rows.
        put(&mut result.0, 0x38, &[0, 1, 1, 1, 0, 0, 0, 0])?;
        put(&mut result.0, 0x40, &[0, 1, 0xFF, 0xFF])?;
        result.0[0x47] = 2;
        put(&mut result.0, 0xF0, &0u64.to_le_bytes())?;
        *target = result;
        Ok(())
    }
}
