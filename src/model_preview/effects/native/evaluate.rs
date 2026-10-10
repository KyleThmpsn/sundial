//! Raw register evaluation keeps integer masks distinct from floating point values.
use super::program::{Operand, Program};
type Bits = [u32; 4];

#[derive(Clone, Copy)]
pub(super) enum Sampling {
    Implicit,
    Level(f32),
    Gradients { dx: [f32; 4], dy: [f32; 4] },
}

pub(super) trait Context {
    fn size(&self, _resource: usize, _mip: u32) -> [u32; 4] {
        [0; 4]
    }
    fn input(&self, register: usize) -> Bits;
    fn constant(&self, buffer: usize, index: usize) -> Bits;
    fn sample(
        &self,
        resource: usize,
        sampler: usize,
        uv: [f32; 4],
        sampling: Sampling,
        offset: [i32; 3],
    ) -> Bits;
    fn load(&self, resource: usize, position: [i32; 4], offset: [i32; 3]) -> Bits;
    fn lod(&self, resource: usize, uv: [f32; 4]) -> [f32; 4];
    fn derivative(&self, input: &Operand, y: bool) -> [f32; 4];
}

struct Registers<'a, C> {
    context: &'a C,
    immediate: &'a [[u32; 4]],
    temps: [Bits; 32],
    outputs: [Bits; 16],
}

impl<C: Context> Registers<'_, C> {
    fn index(&self, v: &Operand, axis: usize) -> usize {
        let index = &v.indices[axis];
        index
            .base
            .wrapping_add(index.relative.as_ref().map_or(0, |v| self.read(v)[0])) as usize
    }
    fn read(&self, v: &Operand) -> Bits {
        let raw = match v.kind {
            0 => self.temps.get(self.index(v, 0)).copied().unwrap_or([0; 4]),
            1 => self.context.input(self.index(v, 0)),
            2 => self
                .outputs
                .get(self.index(v, 0))
                .copied()
                .unwrap_or([0; 4]),
            4 => v.literal,
            8 => self.context.constant(self.index(v, 0), self.index(v, 1)),
            9 => self
                .immediate
                .get(self.index(v, 0))
                .copied()
                .unwrap_or([0; 4]),
            _ => [0; 4],
        };
        std::array::from_fn(|i| {
            let bits = raw[v.lanes[i]];
            match v.modifier {
                1 => bits ^ 0x8000_0000,
                2 => bits & 0x7FFF_FFFF,
                3 => bits | 0x8000_0000,
                _ => bits,
            }
        })
    }
    fn write(&mut self, v: &Operand, value: Bits, saturate: bool) {
        let index = if v.kind == 13 {
            return;
        } else {
            self.index(v, 0)
        };
        let target = match v.kind {
            0 => self.temps.get_mut(index),
            2 => self.outputs.get_mut(index),
            _ => None,
        };
        if let Some(target) = target {
            for i in 0..4 {
                if v.mask & (1 << i) != 0 {
                    target[i] = if saturate {
                        saturated(f32::from_bits(value[i])).to_bits()
                    } else {
                        value[i]
                    };
                }
            }
        }
    }
}

impl Program {
    pub(super) fn evaluate(&self, context: &impl Context) -> Option<[[f32; 4]; 16]> {
        let result = self.run(context, false)?;
        result.coordinates.is_none().then_some(result.outputs)
    }

    pub(super) fn lod_coordinates(&self, context: &impl Context) -> Option<[f32; 4]> {
        self.run(context, true)?.coordinates
    }

    fn run(&self, context: &impl Context, coordinates: bool) -> Option<ResultValue> {
        let mut r = Registers {
            context,
            immediate: &self.immediate,
            temps: [[0; 4]; 32],
            outputs: [[0; 4]; 16],
        };
        let mut branches = [(false, false); 8];
        let mut depth = 0;
        let mut active = true;
        for (at, instruction) in self.instructions.iter().enumerate() {
            let v = &instruction.operands;
            match instruction.code {
                31 => {
                    let condition = active && ((r.read(&v[0])[0] != 0) == instruction.nonzero);
                    branches[depth] = (active, condition);
                    depth += 1;
                    active = condition;
                    continue;
                }
                18 => {
                    let (parent, condition) = branches[depth - 1];
                    active = parent && !condition;
                    continue;
                }
                21 => {
                    depth -= 1;
                    active = branches[depth].0;
                    continue;
                }
                _ if !active => continue,
                62 => break,
                13 => {
                    if (r.read(&v[0])[0] != 0) == instruction.nonzero {
                        return None;
                    }
                    continue;
                }
                _ => {}
            }
            let source = |i| r.read(&v[i]);
            let float = |i| source(i).map(f32::from_bits);
            if coordinates && instruction.code == 108 {
                return Some(ResultValue {
                    outputs: [[0.0; 4]; 16],
                    coordinates: Some(float(1)),
                });
            }
            let unary = |f: fn(f32) -> f32| float(1).map(|v| f(v).to_bits());
            let binary = |f: fn(f32, f32) -> f32| {
                let a = float(1);
                let b = float(2);
                std::array::from_fn(|i| f(a[i], b[i]).to_bits())
            };
            let value = match instruction.code {
                0 => binary(|a, b| a + b),
                1 => {
                    let a = source(1);
                    let b = source(2);
                    std::array::from_fn(|i| a[i] & b[i])
                }
                14 => binary(|a, b| a / b),
                code @ 15..=17 => {
                    let a = float(1);
                    let b = float(2);
                    [(0..(code - 13) as usize)
                        .map(|i| a[i] * b[i])
                        .sum::<f32>()
                        .to_bits(); 4]
                }
                25 => unary(f32::exp2),
                26 => unary(|a| a - a.floor()),
                27 => float(1).map(|v| (v as i32) as u32),
                28 => float(1).map(|v| v as u32),
                code @ (29 | 49 | 57) => {
                    let a = float(1);
                    let b = float(2);
                    std::array::from_fn(|i| {
                        if match code {
                            29 => a[i] >= b[i],
                            49 => a[i] < b[i],
                            _ => a[i] != b[i],
                        } {
                            u32::MAX
                        } else {
                            0
                        }
                    })
                }
                30 => {
                    let a = source(1);
                    let b = source(2);
                    std::array::from_fn(|i| a[i].wrapping_add(b[i]))
                }
                35 => {
                    let a = source(1);
                    let b = source(2);
                    let c = source(3);
                    std::array::from_fn(|i| a[i].wrapping_mul(b[i]).wrapping_add(c[i]))
                }
                38 => {
                    let a = source(2);
                    let b = source(3);
                    let product: [i64; 4] =
                        std::array::from_fn(|i| i64::from(a[i] as i32) * i64::from(b[i] as i32));
                    r.write(&v[0], product.map(|v| (v >> 32) as u32), false);
                    r.write(&v[1], product.map(|v| v as u32), false);
                    continue;
                }
                32 => {
                    let a = source(1);
                    let b = source(2);
                    std::array::from_fn(|i| if a[i] == b[i] { u32::MAX } else { 0 })
                }
                41 => {
                    let a = source(1);
                    let b = source(2);
                    std::array::from_fn(|i| a[i].wrapping_shl(b[i] & 31))
                }
                43 => source(1).map(|v| ((v as i32) as f32).to_bits()),
                45 => {
                    let resource = r.index(&v[2], 0);
                    let raw =
                        context.load(resource, source(1).map(|v| v as i32), instruction.offset);
                    std::array::from_fn(|i| raw[v[2].lanes[i]])
                }
                47 => unary(f32::log2),
                50 => {
                    let a = float(1);
                    let b = float(2);
                    let c = float(3);
                    std::array::from_fn(|i| (a[i] * b[i] + c[i]).to_bits())
                }
                51 => binary(f32::min),
                52 => binary(f32::max),
                54 => source(1),
                55 => {
                    let a = source(1);
                    let b = source(2);
                    let c = source(3);
                    std::array::from_fn(|i| if a[i] != 0 { b[i] } else { c[i] })
                }
                56 => binary(|a, b| a * b),
                64 => unary(f32::round_ties_even),
                65 => unary(f32::floor),
                66 => unary(f32::ceil),
                67 => unary(f32::trunc),
                68 => unary(|a| a.sqrt().recip()),
                69 | 72 | 73 => {
                    let raw = context.sample(
                        r.index(&v[2], 0),
                        r.index(&v[3], 0),
                        float(1),
                        match instruction.code {
                            72 => Sampling::Level(float(4)[0]),
                            73 => Sampling::Gradients {
                                dx: float(4),
                                dy: float(5),
                            },
                            _ => self.derivatives[at]
                                .as_ref()
                                .map_or(Sampling::Implicit, |d| Sampling::Gradients {
                                    dx: d.at(context, false),
                                    dy: d.at(context, true),
                                }),
                        },
                        instruction.offset,
                    );
                    std::array::from_fn(|i| raw[v[2].lanes[i]])
                }
                60 => {
                    let a = source(1);
                    let b = source(2);
                    std::array::from_fn(|i| a[i] | b[i])
                }
                78 => {
                    let a = source(2);
                    let b = source(3);
                    let quotient =
                        std::array::from_fn(|i| a[i].checked_div(b[i]).unwrap_or(u32::MAX));
                    let remainder =
                        std::array::from_fn(|i| a[i].checked_rem(b[i]).unwrap_or(u32::MAX));
                    r.write(&v[0], quotient, false);
                    r.write(&v[1], remainder, false);
                    continue;
                }
                80 => {
                    let a = source(1);
                    let b = source(2);
                    std::array::from_fn(|i| if a[i] >= b[i] { u32::MAX } else { 0 })
                }
                61 => {
                    let raw = context.size(r.index(&v[2], 0), source(1)[0]);
                    std::array::from_fn(|i| raw[v[2].lanes[i]])
                }
                75 => unary(f32::sqrt),
                77 => {
                    let input = float(2);
                    r.write(
                        &v[0],
                        input.map(|v| v.sin().to_bits()),
                        instruction.saturate,
                    );
                    r.write(
                        &v[1],
                        input.map(|v| v.cos().to_bits()),
                        instruction.saturate,
                    );
                    continue;
                }
                86 => source(1).map(|v| (v as f32).to_bits()),
                108 => {
                    let raw = context.lod(r.index(&v[2], 0), float(1));
                    std::array::from_fn(|i| raw[v[2].lanes[i]].to_bits())
                }
                122 | 124 => self.derivatives[at]
                    .as_ref()
                    .map_or_else(
                        || context.derivative(&v[1], instruction.code == 124),
                        |derivative| derivative.at(context, instruction.code == 124),
                    )
                    .map(f32::to_bits),
                140 => {
                    let width = source(1);
                    let offset = source(2);
                    let insert = source(3);
                    let base = source(4);
                    std::array::from_fn(|i| {
                        let w = width[i] & 31;
                        let o = offset[i] & 31;
                        let mask = ((1u32 << w) - 1).wrapping_shl(o);
                        (insert[i].wrapping_shl(o) & mask) | (base[i] & !mask)
                    })
                }
                _ => unreachable!("validated shader instruction"),
            };
            r.write(&v[0], value, instruction.saturate);
        }
        Some(ResultValue {
            outputs: r.outputs.map(|v| v.map(f32::from_bits)),
            coordinates: None,
        })
    }
}

struct ResultValue {
    outputs: [[f32; 4]; 16],
    coordinates: Option<[f32; 4]>,
}

fn saturated(value: f32) -> f32 {
    // Retain max's NaN and signed-zero handling before clamping the upper bound.
    value.max(0.0).clamp(0.0, 1.0)
}
