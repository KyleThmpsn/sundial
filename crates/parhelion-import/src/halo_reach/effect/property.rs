//! MCC scalar and color curves used by source effect properties.
mod color;
mod expression;
mod periodic;
use crate::halo_reach::cache::Cache;
use anyhow::{Context, Result, ensure};
pub(in crate::halo_reach) use expression::Expr;
use serde::Serialize;

#[derive(Clone, Serialize)]
pub(in crate::halo_reach) struct Property {
    inputs: [u8; 4],
    function: Vec<u8>,
    compiled: Option<f32>,
}

#[derive(Clone, Copy, PartialEq)]
enum Evaluation {
    Cpu,
    Gpu,
}

impl Property {
    /// Material overlays carry the same mapping payload as effect properties,
    /// but their caller supplies named inputs and does not carry a compiled scalar.
    pub fn mapping(data: &[u8]) -> Result<Expr> {
        ensure!(
            (32..=4096).contains(&data.len()),
            "Unsupported material mapping size"
        );
        let length = 32usize
            .checked_add(u32::from_le_bytes(data[28..32].try_into()?) as usize)
            .context("Material mapping body size overflow")?;
        ensure!(
            length <= data.len(),
            "Material mapping body exceeds its payload"
        );
        let property = Self {
            inputs: [0, 1, 0, u8::MAX],
            function: data[..length].to_vec(),
            compiled: None,
        };
        property.formula()
    }

    pub fn periodic_tables() -> &'static [u8] {
        periodic::TABLES
    }

    pub fn transition_tables() -> &'static [u8] {
        include_bytes!("property/periodic/transitions.bin")
    }

    pub fn read(cache: &Cache, at: usize) -> Result<Self> {
        let size = usize::try_from(cache.i32(at + 4)?)?;
        ensure!(
            (32..=4096).contains(&size),
            "Unsupported effect mapping size"
        );
        let result = Self {
            inputs: cache.bytes(at, 4)?.try_into()?,
            function: cache
                .meta(cache.expand(cache.u32(at + 16)?)?, size)?
                .to_vec(),
            compiled: (cache.u16(at + 0x1c)? & 0x20 != 0)
                .then(|| cache.f32(at + 0x18))
                .transpose()?,
        };
        ensure!(
            result.function.len() == 32 + result.word(28)? as usize,
            "Effect mapping body size differs"
        );
        Ok(result)
    }

    fn word(&self, at: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.function
                .get(at..at + 4)
                .context("Effect mapping boundary")?
                .try_into()?,
        ))
    }

    fn scalar(&self, at: usize) -> Result<f32> {
        let value = f32::from_bits(self.word(at)?);
        ensure!(value.is_finite(), "Nonfinite effect mapping value");
        Ok(value)
    }

    fn polynomial(
        &self,
        at: &mut usize,
        kind: u8,
        mut input: Expr,
        evaluation: Evaluation,
    ) -> Result<Expr> {
        let result = match kind {
            4 => {
                let result = input
                    .mul(Expr::scalar(self.scalar(*at)?))
                    .add(Expr::scalar(self.scalar(*at + 4)?));
                *at += 8;
                result
            }
            7 | 10 => {
                let mut coefficients = [0.; 4];
                for (index, value) in coefficients.iter_mut().enumerate() {
                    *value = self.scalar(*at + index * 4)?;
                }
                *at += 16;
                if kind == 10 {
                    let origin = self.scalar(*at)?;
                    let width = self.scalar(*at + 4)?;
                    let power = self.scalar(*at + 8)?;
                    if evaluation == Evaluation::Cpu {
                        // MCC 001751DB evaluates the signed power of the
                        // transformed coordinate before the cubic polynomial.
                        ensure!(width != 0. && power > 0., "Invalid effect Spline2 domain");
                        input = input
                            .binary(2, Expr::scalar(origin))
                            .mul(Expr::scalar(width.recip()));
                        if power != 1. {
                            let magnitude = input.clone().binary(9, input.clone().unary(0x1d));
                            let raised = magnitude
                                .clone()
                                .binary(9, Expr::scalar(f32::MIN_POSITIVE))
                                .unary(0x25)
                                .mul(Expr::scalar(power))
                                .unary(0x24);
                            let signed = Expr::select(
                                input.clone().binary(0x0a, Expr::scalar(0.)),
                                raised.clone().unary(0x1d),
                                raised,
                            );
                            input = Expr::select(
                                magnitude.binary(0x0a, Expr::scalar(0.)),
                                Expr::scalar(0.),
                                signed,
                            );
                        }
                    } else {
                        ensure!(
                            origin == 0. && width == 1.,
                            "Effect GPU Spline2 has a transformed input domain"
                        );
                    }
                    *at += 12;
                }
                input.binary(0x0f, Expr::Value(coefficients))
            }
            _ => anyhow::bail!("Unsupported effect polynomial {kind}"),
        };
        Ok(result)
    }

    fn curve(&self, at: &mut usize, kind: u8, input: Expr, evaluation: Evaluation) -> Result<Expr> {
        let result = match kind {
            0 => input,
            1 => Expr::scalar(0.),
            2 if evaluation == Evaluation::Cpu => {
                let transition = *self.function.get(*at).context("Transition selector")?;
                ensure!(
                    transition < 8,
                    "Unsupported transition selector {transition}"
                );
                let low = self.scalar(*at + 4)?;
                let high = self.scalar(*at + 8)?;
                *at += 12;
                Expr::scalar(low).lerp(
                    Expr::scalar(high),
                    Expr::Transition(transition, Box::new(input)),
                )
            }
            9 if evaluation == Evaluation::Cpu => {
                let low = self.scalar(*at)?;
                let high = self.scalar(*at + 4)?;
                let power = self.scalar(*at + 8)?;
                *at += 12;
                Expr::Exponent(Box::new(input), low, high, power)
            }
            3 => {
                // MCC 00174FB8 and 00174FF2 read one byte for each periodic
                // branch. The following three bytes are padding, including
                // nonzero fill in shipped source effects.
                let wave = *self.function.get(*at).context("Effect waveform selector")?;
                let time = input
                    .mul(Expr::scalar(self.scalar(*at + 4)?))
                    .add(Expr::scalar(self.scalar(*at + 8)?));
                let a = self.scalar(*at + 12)?;
                let b = self.scalar(*at + 16)?;
                *at += 20;
                let wave = if evaluation == Evaluation::Cpu {
                    ensure!(wave < 12, "Unsupported source periodic waveform {wave}");
                    Expr::Periodic(wave, Box::new(time))
                } else {
                    let time = time.unary(0x1a);
                    match wave {
                        0 => Expr::scalar(1.),
                        1 => Expr::scalar(0.),
                        2 => time
                            .unary(0x1f)
                            .add(Expr::scalar(1.))
                            .mul(Expr::scalar(0.5)),
                        4 => Expr::select(
                            time.clone().binary(0x0a, Expr::scalar(0.5)),
                            time.clone().mul(Expr::scalar(2.)),
                            Expr::scalar(2.).binary(2, time.mul(Expr::scalar(2.))),
                        ),
                        6 => time,
                        9 | 10 => {
                            // The source GPU reference expresses jitter and wander
                            // as sums of three trigonometric products, in turns.
                            let term = |divisor: f32, op| {
                                time.clone().mul(Expr::scalar(1. / divisor)).unary(op)
                            };
                            let first =
                                term(if wave == 9 { 4. } else { 1. }, 0x1f).mul(term(2., 0x1e));
                            let second = term(7., 0x1f).mul(term(0.25, 0x1f));
                            let third = term(4., 0x1e).mul(term(1. / 7., 0x1f));
                            first
                                .add(second.add(third).mul(Expr::scalar(if wave == 9 {
                                    1.
                                } else {
                                    0.5
                                })))
                                .mul(Expr::scalar(0.5))
                                .add(Expr::scalar(1.))
                        }
                        _ => anyhow::bail!("Unsupported source periodic waveform {wave}"),
                    }
                };
                Expr::scalar(a).lerp(Expr::scalar(b), wave)
            }
            4 | 7 | 10 => self.polynomial(at, kind, input, evaluation)?,
            8 => {
                let count = usize::try_from(self.word(*at)?)?;
                *at += 4;
                ensure!((1..=32).contains(&count), "Unsupported effect spline size");
                let mut pieces = Vec::new();
                let mut previous = 0.;
                for _ in 0..count {
                    let segment = *self.function.get(*at).context("Effect spline kind")?;
                    let end = self.scalar(*at + 4)?;
                    ensure!(end > previous, "Effect spline knots are not increasing");
                    *at += 8;
                    pieces.push((
                        end,
                        self.polynomial(at, segment, input.clone(), evaluation)?,
                    ));
                    previous = end;
                }
                ensure!(previous >= 1., "Incomplete effect spline domain");
                let (_, mut result) = pieces.pop().context("Effect spline")?;
                for (end, segment) in pieces.into_iter().rev() {
                    result = Expr::select(
                        input.clone().binary(0x0a, Expr::scalar(end)),
                        segment,
                        result,
                    );
                }
                result
            }
            _ => anyhow::bail!("Unsupported effect mapping kind {kind}"),
        };
        if evaluation == Evaluation::Gpu {
            self.modify(result)
        } else {
            Ok(result)
        }
    }

    fn modify(&self, mut value: Expr) -> Result<Expr> {
        if self.function[1] & 8 != 0 {
            let a = self.scalar(20)?;
            let b = self.scalar(24)?;
            ensure!(a <= b, "Inverted effect output exclusion");
            let condition = value.clone().binary(0x0a, Expr::scalar(a));
            // MCC leaves values at or below the threshold unchanged. Selecting
            // between two translated values through lerp introduces cancellation.
            value = value.add(Expr::scalar(b - a).mul(Expr::scalar(1.).binary(2, condition)));
        }
        Ok(if self.function[1] & 4 != 0 {
            value.unary(0x23)
        } else {
            value
        })
    }

    pub fn formula(&self) -> Result<Expr> {
        self.evaluate(Evaluation::Cpu)
    }

    fn evaluate(&self, evaluation: Evaluation) -> Result<Expr> {
        // MCC 00240EA8 returns this value before reading any inputs when the
        // compiled scalar constant bit is set. Color properties have a separate
        // output path and must retain their packed color table.
        if evaluation == Evaluation::Cpu
            && self.function[2] == 0
            && let Some(value) = self.compiled
        {
            ensure!(value.is_finite(), "Nonfinite compiled effect constant");
            return Ok(Expr::scalar(value));
        }
        let kind = self.function[0];
        let ranged = self.function[1] & 1 != 0;
        let mut at = 32;
        let input = if kind == 1 {
            Expr::scalar(0.)
        } else if kind == 3 || evaluation == Evaluation::Cpu {
            Expr::Input(self.inputs[0])
        } else {
            Expr::Input(self.inputs[0]).unary(0x23)
        };
        let mut weight = self.curve(&mut at, kind, input.clone(), evaluation)?;
        if ranged {
            let upper = if kind == 1 {
                if evaluation == Evaluation::Gpu {
                    self.modify(Expr::scalar(1.))?
                } else {
                    Expr::scalar(1.)
                }
            } else {
                self.curve(&mut at, kind, input, evaluation)?
            };
            weight = weight.lerp(upper, Expr::Input(self.inputs[1]));
        }
        if evaluation == Evaluation::Cpu {
            weight = self.modify(weight)?;
        }
        ensure!(
            at == self.function.len(),
            "Effect mapping has trailing function data"
        );
        let mut value = match self.function[2] {
            0 => Expr::scalar(self.scalar(4)?).lerp(Expr::scalar(self.scalar(8)?), weight),
            count @ 1..=4 => self.color(count, weight, evaluation)?,
            other => anyhow::bail!("Unsupported effect color count {other}"),
        };
        value = match if self.inputs[3] == u8::MAX {
            0
        } else {
            self.inputs[2]
        } {
            0 => value,
            1 => value.add(Expr::Input(self.inputs[3])),
            2 => value.mul(Expr::Input(self.inputs[3])),
            other => anyhow::bail!("Unsupported effect output modifier {other}"),
        };
        Ok(value)
    }

    /// The primary source input ordinal the mapping reads.
    pub fn input(&self) -> u8 {
        self.inputs[0]
    }

    pub fn constant(&self) -> Result<f32> {
        ensure!(
            self.inputs[2] == 0
                && self.function[0] == 1
                && self.function[1] & 1 == 0
                && self.function[2] == 0,
            "Effect property is not a fixed scalar"
        );
        let value = self.scalar(4)?;
        ensure!(
            value == self.scalar(8)?,
            "Fixed effect scalar has different bounds"
        );
        Ok(value)
    }

    pub fn emission(&self, speed: f32) -> Result<f32> {
        let mut fixed = self.clone();
        fixed.inputs[2] = 0;
        let value = fixed.constant()?;
        match (self.inputs[2], self.inputs[3]) {
            (0, _) => Ok(value),
            (2, 11) => Ok(value * speed),
            _ => anyhow::bail!("Contrail emission needs another runtime input"),
        }
    }

    pub fn bounds(&self) -> Result<[f32; 2]> {
        ensure!(self.function[2] == 0, "Effect property is not scalar");
        Ok([self.scalar(4)?, self.scalar(8)?])
    }

    pub fn expression(&self) -> Result<String> {
        let formula = self.evaluate(Evaluation::Gpu)?;
        let value = formula.hlsl(&|index| {
            ensure!(
                index == 0,
                "Effect curve needs another profile input {index}"
            );
            Ok("age".into())
        })?;
        Ok(format!(
            "({value}).{}",
            if self.function[2] == 0 { "x" } else { "xyz" }
        ))
    }
}
