//! A small vector expression shared by source curves and native particle code.
use anyhow::{Result, bail, ensure};

#[derive(Clone)]
pub(in crate::halo_reach::effect) enum Expr {
    Value([f32; 4]),
    Input(u8),
    Register(u8, u8, u8),
    Unary(u8, Box<Expr>),
    Binary(u8, Box<Expr>, Box<Expr>),
    Ternary(u8, Box<Expr>, Box<Expr>, Box<Expr>),
    Periodic(u8, Box<Expr>),
}

impl Expr {
    pub fn scalar(value: f32) -> Self {
        Self::Value([value; 4])
    }
    pub fn unary(self, opcode: u8) -> Self {
        Self::Unary(opcode, Box::new(self))
    }
    pub fn binary(self, opcode: u8, right: Self) -> Self {
        Self::Binary(opcode, Box::new(self), Box::new(right))
    }
    pub fn lerp(self, target: Self, weight: Self) -> Self {
        if let (Self::Value(a), Self::Value(b)) = (&self, &target)
            && a.map(f32::to_bits) == b.map(f32::to_bits)
        {
            return self;
        }
        Self::Ternary(0x10, Box::new(self), Box::new(target), Box::new(weight))
    }
    pub fn add(self, value: Self) -> Self {
        self.binary(1, value)
    }
    pub fn mul(self, value: Self) -> Self {
        self.binary(3, value)
    }
    pub fn select(condition: Self, yes: Self, no: Self) -> Self {
        no.lerp(yes, condition)
    }

    pub fn hlsl(&self, input: &impl Fn(u8) -> Result<String>) -> Result<String> {
        Ok(match self {
            Self::Value(values) => {
                ensure!(
                    values.iter().all(|v| v.is_finite()),
                    "Nonfinite curve constant"
                );
                format!(
                    "float4({:.9},{:.9},{:.9},{:.9})",
                    values[0], values[1], values[2], values[3]
                )
            }
            Self::Input(index) => format!("float4({0},{0},{0},{0})", input(*index)?),
            Self::Register(..) => bail!("Native particle register has no source shader expression"),
            Self::Periodic(..) => bail!("CPU periodic table has no source shader expression"),
            Self::Unary(op, a) => {
                let a = a.hlsl(input)?;
                match op {
                    0x17 => format!("floor({a})"),
                    0x1a => format!("frac({a})"),
                    0x1d => format!("(-{a})"),
                    0x1e => format!("sin(6.28318530718*{a})"),
                    0x1f => format!("cos(6.28318530718*{a})"),
                    0x23 => format!("saturate({a})"),
                    _ => bail!("Unsupported curve unary operation {op}"),
                }
            }
            Self::Binary(op, a, b) => {
                let (a, b) = (a.hlsl(input)?, b.hlsl(input)?);
                match op {
                    1 => format!("({a}+{b})"),
                    2 => format!("({a}-{b})"),
                    3 => format!("({a}*{b})"),
                    4 => format!("({a}/{b})"),
                    0x0a => format!("float4({a}<={b})"),
                    0x0f => format!("((({b}.xxxx*{a}+{b}.yyyy)*{a}+{b}.zzzz)*{a}+{b}.wwww)"),
                    _ => bail!("Unsupported curve binary operation {op}"),
                }
            }
            Self::Ternary(0x10, a, b, c) => format!(
                "lerp({},{},{})",
                a.hlsl(input)?,
                b.hlsl(input)?,
                c.hlsl(input)?
            ),
            Self::Ternary(op, _, _, _) => bail!("Unsupported curve ternary operation {op}"),
        })
    }

    pub fn emit(
        &self,
        program: &mut crate::tiger::particle::Program,
        phase: usize,
        input: &impl Fn(&mut crate::tiger::particle::Program, usize, u8) -> Result<()>,
    ) -> Result<()> {
        match self {
            Self::Value(value) => program.constant(phase, *value)?,
            Self::Input(index) => input(program, phase, *index)?,
            Self::Register(bank, slot, selector) => {
                program.code(phase, &[0x3e, *bank, *slot, *selector])?
            }
            Self::Unary(op, a) => {
                a.emit(program, phase, input)?;
                program.code(phase, &[*op])?;
            }
            Self::Binary(op, a, b) => {
                a.emit(program, phase, input)?;
                b.emit(program, phase, input)?;
                program.code(phase, &[*op])?;
            }
            Self::Ternary(op, a, b, c) => {
                a.emit(program, phase, input)?;
                b.emit(program, phase, input)?;
                c.emit(program, phase, input)?;
                program.code(phase, &[*op])?;
            }
            Self::Periodic(wave, time) => {
                super::periodic::emit(*wave, time, program, phase, input)?;
            }
        }
        Ok(())
    }
}
