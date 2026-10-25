//! Recover bounded varying expressions while retaining live uniform values.
use super::{
    evaluate::Context,
    program::{Instruction, Operand, Program},
};
mod nonlinear;

#[derive(Clone, Debug)]
enum Uniform {
    One,
    Operand(Operand),
    Add(Box<Self>, Box<Self>),
    Multiply(Box<Self>, Box<Self>),
    Negate(Box<Self>),
}

impl Uniform {
    fn at(&self, context: &impl Context) -> f32 {
        match self {
            Self::One => 1.0,
            Self::Operand(v) => {
                let raw = if v.kind == 4 {
                    v.literal
                } else {
                    context.constant(v.indices[0].base as usize, v.indices[1].base as usize)
                };
                f32::from_bits(raw[v.lanes[0]])
            }
            Self::Add(a, b) => a.at(context) + b.at(context),
            Self::Multiply(a, b) => a.at(context) * b.at(context),
            Self::Negate(v) => -v.at(context),
        }
    }
}

#[derive(Clone, Debug)]
struct Term {
    input: Operand,
    coefficient: Uniform,
}

struct Affine {
    uniform: Option<Uniform>,
    terms: Vec<Term>,
}

impl Affine {
    fn add(mut self, other: Self) -> Self {
        self.uniform = self
            .uniform
            .zip(other.uniform)
            .map(|(a, b)| Uniform::Add(Box::new(a), Box::new(b)));
        self.terms.extend(other.terms);
        self
    }
    fn multiply(self, other: Self) -> Option<Self> {
        let (mut varying, coefficient) = if let Some(coefficient) = self.uniform {
            (other, coefficient)
        } else {
            (self, other.uniform?)
        };
        for term in &mut varying.terms {
            term.coefficient = Uniform::Multiply(
                Box::new(term.coefficient.clone()),
                Box::new(coefficient.clone()),
            );
        }
        varying.uniform = varying
            .uniform
            .map(|v| Uniform::Multiply(Box::new(v), Box::new(coefficient)));
        Some(varying)
    }
    fn negate(mut self) -> Self {
        self.uniform = self.uniform.map(|v| Uniform::Negate(Box::new(v)));
        for term in &mut self.terms {
            term.coefficient = Uniform::Negate(Box::new(term.coefficient.clone()));
        }
        self
    }
}

fn branch_path(program: &Program, before: usize) -> Vec<(usize, bool)> {
    let mut path = Vec::new();
    for (at, instruction) in program.instructions[..before].iter().enumerate() {
        match instruction.code {
            31 => path.push((at, false)),
            18 => path.last_mut().unwrap().1 = true,
            21 => {
                path.pop();
            }
            _ => {}
        }
    }
    path
}

fn value(
    program: &Program,
    operand: &Operand,
    lane: usize,
    before: usize,
    budget: &mut usize,
) -> Option<Affine> {
    *budget = budget.checked_sub(1)?;
    if operand.modifier > 1 || operand.indices.iter().any(|i| i.relative.is_some()) {
        return None;
    }
    let lane = operand.lanes[lane];
    let mut scalar = operand.clone();
    scalar.lanes = [lane; 4];
    scalar.modifier = 0;
    let result = match operand.kind {
        1 if program
            .inputs
            .iter()
            .any(|s| s.register == operand.indices[0].base as usize && s.name == "TEXCOORD") =>
        {
            Affine {
                uniform: None,
                terms: vec![Term {
                    input: scalar,
                    coefficient: Uniform::One,
                }],
            }
        }
        4 | 8 => Affine {
            uniform: Some(Uniform::Operand(scalar)),
            terms: Vec::new(),
        },
        9 => {
            scalar.literal = *program.immediate.get(operand.indices[0].base as usize)?;
            scalar.kind = 4;
            scalar.indices.clear();
            Affine {
                uniform: Some(Uniform::Operand(scalar)),
                terms: Vec::new(),
            }
        }
        0 => written(program, operand.indices[0].base, lane, before, budget)?,
        _ => return None,
    };
    Some(if operand.modifier == 1 {
        result.negate()
    } else {
        result
    })
}

fn written(
    program: &Program,
    index: u32,
    lane: usize,
    before: usize,
    budget: &mut usize,
) -> Option<Affine> {
    let (at, instruction) = writer(program, index, lane, before)?;
    let read = |n, budget: &mut usize| value(program, &instruction.operands[n], lane, at, budget);
    match instruction.code {
        54 => read(1, budget),
        0 => Some(read(1, budget)?.add(read(2, budget)?)),
        56 => read(1, budget)?.multiply(read(2, budget)?),
        50 => Some(
            read(1, budget)?
                .multiply(read(2, budget)?)?
                .add(read(3, budget)?),
        ),
        _ => None,
    }
}

fn writer(
    program: &Program,
    index: u32,
    lane: usize,
    before: usize,
) -> Option<(usize, &Instruction)> {
    let (at, instruction) = program.instructions[..before]
        .iter()
        .enumerate()
        .rev()
        .find(|(_, i)| {
            let destinations = match i.code {
                13 | 18 | 21 | 31 | 62 => 0,
                38 | 77 => 2,
                _ => 1,
            };
            i.operands
                .iter()
                .take(destinations)
                .any(|v| v.kind == 0 && v.indices[0].base == index && v.mask & (1 << lane) != 0)
        })?;
    // A producer must dominate this use. A write in another arm or a completed
    // conditional cannot supply a derivative expression across the branch merge.
    if instruction.saturate || !branch_path(program, before).starts_with(&branch_path(program, at))
    {
        return None;
    }
    Some((at, instruction))
}

#[derive(Clone, Debug)]
enum Lane {
    Affine(Vec<Term>),
    Nonlinear(nonlinear::Expression),
}

#[derive(Clone, Debug)]
pub(super) struct Derivative {
    lanes: [Lane; 4],
}

impl Derivative {
    pub fn read(program: &Program, at: usize) -> Option<Self> {
        let input = &program.instructions[at].operands[1];
        let instruction = &program.instructions[at];
        let mask = if matches!(instruction.code, 69 | 108) {
            let slot = instruction.operands[2].indices[0].base as usize;
            if program
                .resources
                .iter()
                .any(|r| r.slot == slot && matches!(r.dimension, 5 | 6))
            {
                7
            } else {
                3
            }
        } else {
            instruction.operands[0].mask
        };
        let mut lanes = std::array::from_fn(|_| Lane::Affine(Vec::new()));
        for (lane, derivative) in lanes.iter_mut().enumerate() {
            if mask & (1 << lane) == 0 {
                continue;
            }
            *derivative = if let Some(affine) = value(program, input, lane, at, &mut 128) {
                if affine.terms.len() > 32 {
                    return None;
                }
                Lane::Affine(affine.terms)
            } else if matches!(instruction.code, 123 | 125) {
                Lane::Nonlinear(nonlinear::Expression::read(
                    program, input, lane, at, &mut 128,
                )?)
            } else {
                return None;
            };
        }
        Some(Self { lanes })
    }

    pub fn at(&self, context: &impl Context, y: bool) -> [f32; 4] {
        self.lanes.each_ref().map(|lane| match lane {
            Lane::Affine(terms) => terms
                .iter()
                .map(|term| context.derivative(&term.input, y)[0] * term.coefficient.at(context))
                .sum(),
            Lane::Nonlinear(expression) => {
                let offset = context.derivative_offset(y);
                (expression.at(context, y, offset) - expression.at(context, y, 0.0)) * offset
            }
        })
    }
}
