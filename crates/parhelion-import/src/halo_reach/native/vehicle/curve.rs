//! Bounded source torque splines expressed as native four-lane value programs.
use crate::{
    presentation::{append, put},
    tiger::payload::Payload,
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;

#[derive(Default)]
pub(super) struct Program {
    code: Vec<u8>,
    constants: Vec<[f32; 4]>,
    inputs: u64,
}

impl Program {
    fn vector(&mut self, value: [f32; 4]) -> Result<()> {
        ensure!(
            value.iter().all(|n| n.is_finite()),
            "Nonfinite vehicle curve constant"
        );
        let index = u8::try_from(self.constants.len())?;
        self.constants.push(value);
        self.code.extend([0x34, index]);
        Ok(())
    }

    fn scalar(&mut self, value: f32) -> Result<()> {
        self.vector([value; 4])
    }

    pub(super) fn constant(value: f32) -> Result<Self> {
        let mut result = Self::default();
        result.scalar(value)?;
        Ok(result)
    }

    pub(super) fn identity() -> Self {
        Self {
            code: vec![0x3c, 0],
            constants: Vec::new(),
            inputs: 1,
        }
    }

    pub(super) fn flight_pitch(limit_degrees: f32) -> Result<Self> {
        ensure!(limit_degrees > 0., "Flight pitch limit must be positive");
        let mut result = Self {
            code: vec![0x3c, 1, 0x22, 0x55],
            inputs: 2,
            ..Self::default()
        };
        // The native unit's relative aim getter returns pitch in Y, in radians,
        // positive when the body is above the aim. The flight curve uses degrees.
        result.scalar(-180. / std::f32::consts::PI)?;
        result.code.push(0x03);
        result.scalar(-limit_degrees)?;
        result.scalar(limit_degrees)?;
        result.code.push(0x13);
        Ok(result)
    }

    pub(super) fn flight_throttle(cruise: f32, reverse: f32, boost: f32) -> Result<Self> {
        ensure!(
            (0.0..=1.0).contains(&cruise)
                && reverse.is_finite()
                && reverse >= 0.
                && boost.is_finite()
                && boost > cruise,
            "Invalid flight thrust range"
        );
        let mut result = Self {
            code: vec![0x3c, 1, 0x22, 0x55, 0x23],
            inputs: 2,
            ..Self::default()
        };
        // Keep the unit movement getter's signed Y input. The native throttle
        // accumulator and force construction both retain negative demand.
        result.scalar(cruise)?;
        result.code.extend([0x03, 0x3c, 1, 0x22, 0x55]);
        result.scalar(-1.)?;
        result.code.extend([0x03, 0x23]);
        result.scalar(reverse)?;
        result.code.extend([0x03, 0x02]);
        // Boost replaces either travel direction. A maximum with zero boost
        // would discard reverse thrust even after preserving the signed input.
        result.scalar(1.)?;
        result
            .code
            .extend([0x3c, 0, 0x23, 0x02, 0x03, 0x3c, 0, 0x23]);
        result.scalar(boost)?;
        result.code.extend([0x03, 0x01]);
        Ok(result)
    }

    pub(super) fn roll_surface(right: bool) -> Result<Self> {
        let mut result = Self::ramp([-1., 1.], if right { [1., -1.] } else { [-1., 1.] })?;
        // Inputs one and two are the native left and right maneuver states.
        // While a maneuver is active, its direction replaces level restoration.
        result.scalar(1.)?;
        result.code.extend([0x3c, 1, 0x3c, 2, 0x09, 0x02, 0x03]);
        result.code.extend([0x3c, 2, 0x3c, 1, 0x02]);
        if right {
            result.scalar(-1.)?;
            result.code.push(0x03);
        }
        result.code.extend([0x3c, 1, 0x3c, 2, 0x09, 0x03, 0x01]);
        result.inputs = 7;
        Ok(result)
    }

    pub(super) fn damping_lift(gain: f32) -> Result<Self> {
        ensure!(gain.is_finite() && gain > 0., "Invalid flight damping gain");
        let mut result = Self::identity();
        // EA3270 supplies signed incidence in degrees. The native sine opcode
        // consumes turns. Its full-circle response also bounds reverse flow.
        result.scalar(1. / 360.)?;
        result.code.extend([0x03, 0x1e]);
        result.scalar(gain)?;
        result.code.push(0x03);
        Ok(result)
    }

    pub(super) fn airspeed(mut self, reference: f32, squared: bool) -> Result<Self> {
        ensure!(
            reference.is_finite() && reference > 0.,
            "Invalid flight reference speed"
        );
        // The body speed getter is input one. Bound compensation at one fifth
        // of reference speed and leave the original response above reference.
        // Authority scales with speed squared, rotational damping with speed.
        for _ in 0..if squared { 2 } else { 1 } {
            self.scalar(reference)?;
            self.code.extend([0x3c, 1]);
            self.scalar(reference * 0.2)?;
            self.code.extend([0x09, 0x04]);
            self.scalar(1.)?;
            self.code.extend([0x09, 0x03]);
        }
        self.inputs |= 2;
        Ok(self)
    }

    pub(super) fn ramp(input: [f32; 2], output: [f32; 2]) -> Result<Self> {
        ensure!(
            input.iter().chain(&output).all(|v| v.is_finite()) && input[0] < input[1],
            "Invalid bounded vehicle response"
        );
        let mut result = Self::identity();
        result.scalar(input[0])?;
        result.code.push(0x02);
        result.scalar(input[1] - input[0])?;
        result.code.extend([0x04, 0x23]);
        result.scalar(output[1] - output[0])?;
        result.code.push(0x03);
        result.scalar(output[0])?;
        result.code.push(0x01);
        Ok(result)
    }

    pub(super) fn source(source: &Value) -> Result<Self> {
        let p = Payload(hex::decode(
            source["bytes"]
                .as_str()
                .context("Source torque function bytes")?,
        )?);
        ensure!(
            p.0.len() >= 32 && p.u8(1)? == 0x34 && p.u16(2)? == 0,
            "Unsupported source torque function header"
        );
        let low = p.f32(4)?;
        let high = p.f32(8)?;
        ensure!(
            p.u32(28)? as usize == p.0.len() - 32,
            "Source torque function size differs"
        );
        if p.u8(0)? == 0 {
            ensure!(p.0.len() == 32, "Source linear function has a body");
            let mut result = Self::identity();
            result.code.push(0x23);
            result.scalar(high - low)?;
            result.code.push(0x03);
            result.scalar(low)?;
            result.code.push(0x01);
            return Ok(result);
        }
        if p.u8(0)? == 1 {
            ensure!(
                p.0.len() == 32 && low == high,
                "Interpolated torque constant requires another input"
            );
            return Self::constant(low);
        }
        ensure!(
            p.u8(0)? == 8 && low <= high,
            "Unsupported source torque function kind"
        );
        let count = usize::try_from(p.u32(32)?)?;
        ensure!(
            (1..=16).contains(&count) && p.0.len() == 36 + count * 36,
            "Unsupported source torque spline extent"
        );
        let mut result = Self::default();
        let mut previous_end = 0f32;
        let mut previous_value: Option<f32> = None;
        for index in 0..count {
            let at = 36 + index * 36;
            ensure!(p.u8(at)? == 10, "Unsupported source torque spline segment");
            let end = p.f32(at + 4)?;
            let coefficients = [
                p.f32(at + 8)?,
                p.f32(at + 12)?,
                p.f32(at + 16)?,
                p.f32(at + 20)?,
            ];
            let start = p.f32(at + 24)?;
            let width = p.f32(at + 28)?;
            // The trailing source spline value is retained in the source export.
            // Its use outside this bounded polynomial form is not established.
            p.f32(at + 32)?;
            ensure!(
                width > 0. && (start - previous_end).abs() < 0.00001,
                "Source torque spline has a gap or reversed segment"
            );
            if index + 1 == count {
                ensure!(
                    end == f32::MAX && (start + width - 1.).abs() < 0.00001,
                    "Source torque spline lacks its final bounded segment"
                );
            } else {
                ensure!(
                    (start + width - end).abs() < 0.00001 && end < 1.,
                    "Source torque spline threshold differs from its interval"
                );
            }
            if let Some(value) = previous_value {
                ensure!(
                    (coefficients[3] - value).abs() < 0.0001,
                    "Discontinuous source torque splines require explicit branch translation"
                );
            }
            result.code.extend([0x3c, 0]);
            result.scalar(start)?;
            result.code.push(0x02);
            result.scalar(width)?;
            result.code.extend([0x04, 0x23]);
            result.vector(coefficients)?;
            result.code.push(0x0f);
            if index != 0 {
                result.scalar(coefficients[3])?;
                result.code.extend([0x02, 0x01]);
            }
            previous_end = start + width;
            previous_value = Some(coefficients.iter().sum::<f32>());
        }
        result.scalar(high - low)?;
        result.code.push(0x03);
        result.scalar(low)?;
        result.code.push(0x01);
        Ok(result)
    }

    /// `at` is the native 80808E76 program, without an object-reference prefix.
    pub(super) fn write(mut self, p: &mut Payload, at: usize) -> Result<()> {
        self.code.extend([0x3e, 0]);
        append(&mut p.0, at, 0x80800009, &self.code, 1)?;
        let constants = self
            .constants
            .iter()
            .flatten()
            .flat_map(|n| n.to_le_bytes())
            .collect::<Vec<_>>();
        append(&mut p.0, at + 16, 0x80800090, &constants, 16)?;
        // The wrappers supply input zero, including for constants. Bound channel
        // values follow it in the native component expression input vector.
        put(&mut p.0, at + 32, &self.inputs.max(1).to_le_bytes())?;
        put(&mut p.0, at + 40, &1u64.to_le_bytes())?;
        Ok(())
    }
}
