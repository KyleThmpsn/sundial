//! Encode local controls for the carrier clip's native composition operation.
use super::super::pose::Pose;
use crate::tiger::payload::Payload;
use anyhow::{Context, Result, ensure};

pub(super) struct Local {
    additive: bool,
    defaults: Vec<Pose>,
}

impl Local {
    pub(super) fn read(clip: &Payload, controls: &Payload, total: usize) -> Result<Self> {
        // 01044FE0 dispatches this byte. Mode 1 multiplies scale and rotation,
        // and adds translation through 0103EF10 or its weighted counterpart.
        // Mode 2 replaces sparse absolute channels, with 0103F310 for weighting.
        let mode = clip.u8(0x181)?;
        ensure!(mode <= 2, "Native clip composition {mode} is unsupported");
        let definition = controls.pointer(24)?;
        ensure!(
            controls.u32(definition - 4)? == 0x80808F8F,
            "Local clip motion requires native rig controls"
        );
        let rows = controls.array(definition + 0xB0, 32, Some(0x80809F75))?;
        ensure!(rows.len() == total, "Clip default control count differs");
        let defaults = rows
            .into_iter()
            .map(|row| {
                let mut values = [0.; 8];
                for (axis, value) in values.iter_mut().enumerate() {
                    *value = controls.f32(row + axis * 4)?;
                }
                Pose::checked(values)
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            additive: mode == 1,
            defaults,
        })
    }

    pub(super) fn encode(&self, slot: usize, value: [f32; 8]) -> Result<[f32; 8]> {
        let pose = Pose::checked(value)?;
        if !self.additive {
            return Ok(pose.0);
        }
        let rest = self
            .defaults
            .get(slot)
            .context("Clip default control slot")?;
        let mut delta = rest.inverse().then(pose).0;
        // Native additive translation stays in control space. It does not
        // rotate or scale the position delta through the rest transform.
        for ((value, pose), rest) in delta[4..7].iter_mut().zip(&pose.0[4..7]).zip(&rest.0[4..7]) {
            *value = pose - rest;
        }
        if delta[3] < 0. {
            delta[..4].iter_mut().for_each(|v| *v = -*v);
        }
        Ok(Pose::checked(delta)?.0)
    }
}
