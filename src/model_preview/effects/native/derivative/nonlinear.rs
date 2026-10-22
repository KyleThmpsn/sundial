//! Finite differences of pure arithmetic, including normalized interpolated normals.
use super::{Context, Operand, Program};

#[derive(Clone, Debug)]
pub(super) enum Expression {
    Zero,
    Operand(Operand),
    Add(Box<Self>, Box<Self>),
    Multiply(Box<Self>, Box<Self>),
    ReciprocalSqrt(Box<Self>),
    Negate(Box<Self>),
}

impl Expression {
    pub fn read(
        program: &Program,
        operand: &Operand,
        lane: usize,
        before: usize,
        budget: &mut usize,
    ) -> Option<Self> {
        *budget = budget.checked_sub(1)?;
        if operand.modifier > 1 || operand.indices.iter().any(|i| i.relative.is_some()) {
            return None;
        }
        let lane = operand.lanes[lane];
        let mut scalar = operand.clone();
        scalar.lanes = [lane; 4];
        scalar.modifier = 0;
        let result = match operand.kind {
            1 if program.inputs.iter().any(|s| {
                s.register == operand.indices[0].base as usize && s.name == "TEXCOORD"
            }) =>
            {
                Self::Operand(scalar)
            }
            4 | 8 => Self::Operand(scalar),
            9 => {
                scalar.literal = *program.immediate.get(operand.indices[0].base as usize)?;
                scalar.kind = 4;
                scalar.indices.clear();
                Self::Operand(scalar)
            }
            0 => {
                let (at, instruction) =
                    super::writer(program, operand.indices[0].base, lane, before)?;
                let read = |n, lane, budget: &mut usize| {
                    Self::read(program, &instruction.operands[n], lane, at, budget)
                };
                match instruction.code {
                    54 => read(1, lane, budget)?,
                    0 => Self::Add(
                        Box::new(read(1, lane, budget)?),
                        Box::new(read(2, lane, budget)?),
                    ),
                    56 => Self::Multiply(
                        Box::new(read(1, lane, budget)?),
                        Box::new(read(2, lane, budget)?),
                    ),
                    50 => Self::Add(
                        Box::new(Self::Multiply(
                            Box::new(read(1, lane, budget)?),
                            Box::new(read(2, lane, budget)?),
                        )),
                        Box::new(read(3, lane, budget)?),
                    ),
                    code @ 15..=17 => {
                        let mut sum = Self::Zero;
                        for component in 0..(code - 13) as usize {
                            sum = Self::Add(
                                Box::new(sum),
                                Box::new(Self::Multiply(
                                    Box::new(read(1, component, budget)?),
                                    Box::new(read(2, component, budget)?),
                                )),
                            );
                        }
                        sum
                    }
                    68 => Self::ReciprocalSqrt(Box::new(read(1, lane, budget)?)),
                    _ => return None,
                }
            }
            _ => return None,
        };
        Some(if operand.modifier == 1 {
            Self::Negate(Box::new(result))
        } else {
            result
        })
    }

    pub fn at(&self, context: &impl Context, y: bool, offset: f32) -> f32 {
        match self {
            Self::Zero => 0.0,
            Self::Operand(v) => {
                let raw = match v.kind {
                    1 => context.input(v.indices[0].base as usize),
                    4 => v.literal,
                    _ => context.constant(v.indices[0].base as usize, v.indices[1].base as usize),
                };
                let value = f32::from_bits(raw[v.lanes[0]]);
                if v.kind == 1 && offset != 0.0 {
                    value + context.derivative(v, y)[0] * offset
                } else {
                    value
                }
            }
            Self::Add(a, b) => a.at(context, y, offset) + b.at(context, y, offset),
            Self::Multiply(a, b) => a.at(context, y, offset) * b.at(context, y, offset),
            Self::ReciprocalSqrt(value) => value.at(context, y, offset).sqrt().recip(),
            Self::Negate(value) => -value.at(context, y, offset),
        }
    }
}
