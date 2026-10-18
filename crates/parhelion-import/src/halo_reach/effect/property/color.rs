//! MCC CPU colors pass through packed bytes before returning normalized RGB.
use super::{Evaluation, Expr, Property};
use anyhow::{Context, Result};

impl Property {
    pub(super) fn color(&self, count: u8, weight: Expr, evaluation: Evaluation) -> Result<Expr> {
        let cpu = evaluation == Evaluation::Cpu;
        // MCC 0017563F returns the first packed color for an unranged constant,
        // even if its header retains more colors or curve-output flags.
        let count = if cpu && self.function[0] == 1 && self.function[1] & 1 == 0 {
            1
        } else {
            count
        };
        let rgb = |at| -> Result<Expr> {
            let color = self.word(at)?;
            let values = [
                (color >> 16 & 255) as f32,
                (color >> 8 & 255) as f32,
                (color & 255) as f32,
                255.,
            ];
            Ok(Expr::Value(if cpu {
                values
            } else {
                values.map(|value| value / 255.)
            }))
        };
        let mut colors = (0..count.saturating_sub(1))
            .map(|i| rgb(4 + usize::from(i) * 4))
            .collect::<Result<Vec<_>>>()?;
        colors.push(rgb(if count == 1 { 4 } else { 16 })?);
        // MCC 00175658 clamps the color coordinate independently of the scalar
        // function's clamp flag. GPU curves retain their separate contract.
        let weight = if cpu { weight.unary(0x23) } else { weight };
        let coordinate = weight.mul(Expr::scalar(f32::from(count - 1)));
        let mut result = colors.last().context("Effect color")?.clone();
        for index in (0..colors.len().saturating_sub(1)).rev() {
            let fraction = coordinate.clone().binary(2, Expr::scalar(index as f32));
            let segment = if cpu {
                // Keep byte-domain weighted addition and rounding before
                // normalization, as in 00175965 and its three/four-color paths.
                // Normalized lerp can choose a different byte at half steps.
                colors[index]
                    .clone()
                    .mul(Expr::scalar(1.).binary(2, fraction.clone()))
                    .add(colors[index + 1].clone().mul(fraction))
                    .add(Expr::scalar(0.5))
                    .unary(0x17)
            } else {
                colors[index]
                    .clone()
                    .lerp(colors[index + 1].clone(), fraction)
            };
            result = Expr::select(
                coordinate
                    .clone()
                    .binary(0x0a, Expr::scalar(index as f32 + 1.)),
                segment,
                result,
            );
        }
        Ok(if cpu {
            // 001755AC converts the packed RGB bytes using this reciprocal.
            result.mul(Expr::scalar(1. / 255.))
        } else {
            result
        })
    }
}
