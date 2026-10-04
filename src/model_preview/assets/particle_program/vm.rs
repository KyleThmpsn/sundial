//! Evaluates the validated subset of native particle expression bytecode.
//! Unknown operations and selectors fail closed so callers cannot mistake a partial result
//! for an authored spawn or motion output.
use super::Program;
use crate::expression::{binary, math, ternary, unary};

pub(super) fn supported(opcode: u8) -> bool {
    super::coverage::instruction_len(opcode).is_some()
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Transform {
    pub translation: [f32; 4],
    pub rotation: [f32; 4],
}

/// Mutable runtime state must be supplied explicitly and commits only after a
/// complete phase. Transform indices use the owning controller's binding order.
#[derive(Clone, Default)]
pub(crate) struct Runtime {
    pub seed: Option<u32>,
    pub transforms: Vec<Transform>,
}

/// The tables the engine supplies to one evaluation. Each opcode family reads its own.
#[derive(Clone, Copy, Default)]
pub(crate) struct Sources<'a> {
    /// `0x47` runtime inputs.
    pub inputs: &'a [[f32; 4]],
    /// Recorded `0x3D` random results, in encounter order.
    pub pushes: &'a [[f32; 4]],
    /// `0x43` and `0x44` scoped inputs.
    pub scoped: &'a [[f32; 4]],
    /// `0x46` external inputs.
    pub external: &'a [[f32; 4]],
}

#[derive(Clone)]
pub(crate) struct Registers {
    banks: [Vec<[f32; 4]>; 7],
}

impl Registers {
    pub fn new(program: &Program) -> Self {
        let mut banks = std::array::from_fn(|_| vec![[0.0; 4]; 64]);
        banks[6] = program.defaults.clone();
        Self { banks }
    }

    pub fn set(&mut self, bank: u8, slot: u8, value: [f32; 4]) -> Result<(), String> {
        if value.iter().any(|v| !v.is_finite()) {
            return Err("Particle register value is not finite".into());
        }
        *self
            .banks
            .get_mut(bank as usize)
            .and_then(|rows| rows.get_mut(slot as usize))
            .ok_or("Particle register is outside its bank")? = value;
        Ok(())
    }

    pub fn get(&self, bank: u8, slot: u8) -> Option<[f32; 4]> {
        self.banks.get(bank as usize)?.get(slot as usize).copied()
    }

    #[cfg(test)]
    pub fn output(&self, program: &Program, index: usize) -> Option<f32> {
        let route = program.routes.get(index)?.as_ref()?;
        let slot = route.scalar / 4;
        let lane = usize::from(route.scalar % 4);
        Some(self.get(route.bank, slot)?[lane])
    }
}

impl Program {
    pub fn evaluate_section(
        &self,
        section: usize,
        registers: &mut Registers,
    ) -> Result<(), String> {
        self.evaluate_section_with_inputs(section, registers, &[])
    }

    /// Engine-supplied values are explicit. Their source and timing depend on the
    /// particle phase, so a missing input must never silently become zero.
    pub fn evaluate_section_with_inputs(
        &self,
        section: usize,
        registers: &mut Registers,
        inputs: &[[f32; 4]],
    ) -> Result<(), String> {
        self.evaluate_section_with_runtime(section, registers, inputs, &[])
    }

    /// Supplies recorded `0x3D` random results in encounter order. Call the state
    /// evaluator with a seed to reproduce the native random sequence instead.
    pub fn evaluate_section_with_runtime(
        &self,
        section: usize,
        registers: &mut Registers,
        inputs: &[[f32; 4]],
        pushes: &[[f32; 4]],
    ) -> Result<(), String> {
        self.evaluate_section_with_sources(section, registers, inputs, pushes, &[])
    }

    /// `0x43` reads a scalar from the flattened scoped input vectors, while
    /// `0x47` reads a named channel. Their tables are independent.
    pub fn evaluate_section_with_sources(
        &self,
        section: usize,
        registers: &mut Registers,
        inputs: &[[f32; 4]],
        pushes: &[[f32; 4]],
        scoped_inputs: &[[f32; 4]],
    ) -> Result<(), String> {
        self.evaluate_section_with_external(section, registers, inputs, pushes, scoped_inputs, &[])
    }

    /// `0x46` is a further indexed input family in the particle dialect. Its
    /// operand is kept separate from the other input tables until the engine
    /// source of each family is identified.
    pub fn evaluate_section_with_external(
        &self,
        section: usize,
        registers: &mut Registers,
        inputs: &[[f32; 4]],
        pushes: &[[f32; 4]],
        scoped_inputs: &[[f32; 4]],
        external_inputs: &[[f32; 4]],
    ) -> Result<(), String> {
        self.evaluate_section_with_state(
            section,
            registers,
            Sources {
                inputs,
                pushes,
                scoped: scoped_inputs,
                external: external_inputs,
            },
            &mut Runtime::default(),
        )
    }

    pub fn evaluate_section_with_state(
        &self,
        section: usize,
        registers: &mut Registers,
        sources: Sources<'_>,
        runtime: &mut Runtime,
    ) -> Result<(), String> {
        let code = self.section(section).ok_or("Particle section is missing")?;
        let mut next = registers.clone();
        let mut state = runtime.clone();
        let mut stack = Vec::<[f32; 4]>::new();
        let mut at = 0;
        let mut pushed = 0;
        while at < code.len() {
            let opcode = code[at];
            if !supported(opcode) {
                return Err(format!(
                    "Unsupported particle opcode 0x{opcode:02X} in section {section} at byte {at}"
                ));
            }
            at += 1;
            match opcode {
                0x01 | 0x02 | 0x03 | 0x04 | 0x05 | 0x06 | 0x08 | 0x09 | 0x0A | 0x0B | 0x0C
                | 0x0D | 0x0E | 0x0F => {
                    let right = pop(&mut stack)?;
                    let left = pop(&mut stack)?;
                    stack.push(binary(opcode, left, right));
                }
                0x10..=0x14 => {
                    let c = pop(&mut stack)?;
                    let b = pop(&mut stack)?;
                    let a = pop(&mut stack)?;
                    stack.push(ternary(opcode, a, b, c));
                }
                0x07
                | 0x15
                | 0x16
                | 0x17
                | 0x18
                | 0x19
                | 0x1A
                | 0x1B
                | 0x1C
                | 0x1D
                | 0x1E
                | 0x1F
                | 0x20
                | 0x21
                | 0x23..=0x2B => {
                    let value = pop(&mut stack)?;
                    stack.push(unary(opcode, value));
                }
                0x22 => {
                    let mask = operand(code, &mut at)?;
                    let value = pop(&mut stack)?;
                    stack.push(std::array::from_fn(|i| {
                        value[((mask >> (6 - 2 * i)) & 3) as usize]
                    }));
                }
                0x34 => {
                    let index = operand(code, &mut at)? as usize;
                    stack.push(
                        *self
                            .constants
                            .get(index)
                            .ok_or("Particle constant is missing")?,
                    );
                }
                0x3D => {
                    let value = if let Some(seed) = &mut state.seed {
                        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        [((*seed >> 16) as f32) * (1.0 / 65_535.0); 4]
                    } else {
                        *sources
                            .pushes
                            .get(pushed)
                            .ok_or_else(|| format!("Particle random result {pushed} is missing"))?
                    };
                    stack.push(value);
                    pushed += 1;
                }
                0x43 => {
                    let index = operand(code, &mut at)? as usize;
                    let vector = sources
                        .scoped
                        .get(index / 4)
                        .ok_or_else(|| format!("Particle scalar input {index} is missing"))?;
                    stack.push([vector[index % 4]; 4]);
                }
                0x44 => {
                    let index = operand(code, &mut at)? as usize;
                    stack.push(
                        *sources
                            .scoped
                            .get(index)
                            .ok_or_else(|| format!("Particle vector input {index} is missing"))?,
                    );
                }
                0x45 => {
                    let index = operand(code, &mut at)? as usize;
                    let rows = sources
                        .scoped
                        .get(index..index + 4)
                        .ok_or_else(|| format!("Particle matrix input {index} is missing"))?;
                    stack.extend_from_slice(rows);
                }
                0x2C => {
                    let axis = pop(&mut stack)?;
                    let value = pop(&mut stack)?;
                    stack.push(math::rotate_axis(value, axis));
                }
                0x2D => {
                    let right = pop_values::<4>(&mut stack)?;
                    let left = pop_values::<4>(&mut stack)?;
                    stack.extend(right.map(|row| math::matrix(left, row)));
                }
                0x2E => {
                    let value = pop(&mut stack)?;
                    let matrix = pop_values::<4>(&mut stack)?;
                    stack.push(math::matrix(matrix, value));
                }
                0x2F..=0x33 => {
                    let input = pop(&mut stack)?;
                    let count = match opcode {
                        0x2F => 5,
                        0x30 | 0x31 => 10,
                        0x32 => 6,
                        _ => 11,
                    };
                    let start = stack
                        .len()
                        .checked_sub(count)
                        .ok_or("Particle expression stack is empty")?;
                    let constants = stack.split_off(start);
                    let value = if opcode <= 0x31 {
                        let fallback = if opcode == 0x31 {
                            Some(pop(&mut stack)?)
                        } else {
                            None
                        };
                        math::spline(input, &constants, fallback)?
                    } else {
                        math::gradient(input, &constants)?
                    };
                    stack.push(value);
                }
                0x49..=0x4B => {
                    let index = pop(&mut stack)?[0];
                    if !index.is_finite() || index < 0.0 || index >= state.transforms.len() as f32 {
                        return Err("Particle transform input is outside its bindings".into());
                    }
                    let transform = state.transforms[index as usize];
                    if opcode != 0x4B {
                        stack.push(transform.translation);
                    }
                    if opcode != 0x4A {
                        stack.push(transform.rotation);
                    }
                }
                0x4C | 0x4D => {
                    let offset = pop(&mut stack)?;
                    let rotation = pop(&mut stack)?;
                    let mut translation = pop(&mut stack)?;
                    let offset = if opcode == 0x4D {
                        math::rotate(offset, rotation)
                    } else {
                        offset
                    };
                    for i in 0..3 {
                        translation[i] += offset[i];
                    }
                    stack.extend([translation, rotation]);
                }
                0x4E => {
                    let values = pop_values::<5>(&mut stack)?;
                    stack.extend(math::interpolate_transform(values));
                }
                0x50 => {
                    let values = pop_values::<5>(&mut stack)?;
                    stack.extend(math::orbit_frame(values));
                }
                0x4F | 0x51 | 0x52 => {
                    let angle = pop(&mut stack)?;
                    let axis = pop(&mut stack)?;
                    let frame_rotation = pop(&mut stack)?;
                    let frame_translation = pop(&mut stack)?;
                    let rotation = pop(&mut stack)?;
                    let translation = pop(&mut stack)?;
                    let mut result = math::rotate_frame(
                        [
                            translation,
                            rotation,
                            frame_translation,
                            frame_rotation,
                            axis,
                            angle,
                        ],
                        opcode != 0x52,
                    );
                    if opcode == 0x51 {
                        result[1] = rotation;
                    }
                    stack.extend(result);
                }
                0x40..=0x42 => {
                    let index = operand(code, &mut at)? as usize;
                    let transform = state
                        .transforms
                        .get_mut(index)
                        .ok_or("Particle transform output is outside its bindings")?;
                    match opcode {
                        0x40 => {
                            transform.rotation = pop(&mut stack)?;
                            transform.translation = pop(&mut stack)?;
                        }
                        0x41 => transform.translation = pop(&mut stack)?,
                        0x42 => transform.rotation = pop(&mut stack)?,
                        _ => unreachable!(),
                    }
                }
                0x46 => {
                    let index = operand(code, &mut at)? as usize;
                    stack.push(
                        *sources
                            .external
                            .get(index)
                            .ok_or_else(|| format!("Particle external input {index} is missing"))?,
                    );
                }
                0x47 => {
                    let index = operand(code, &mut at)? as usize;
                    stack.push(
                        *sources
                            .inputs
                            .get(index)
                            .ok_or_else(|| format!("Particle runtime input {index} is missing"))?,
                    );
                }
                0x35 | 0x36 => {
                    let index = operand(code, &mut at)? as usize;
                    let start = self
                        .constants
                        .get(index)
                        .ok_or("Particle curve start is missing")?;
                    let end = self
                        .constants
                        .get(index + 1)
                        .ok_or("Particle curve end is missing")?;
                    let time = pop(&mut stack)?;
                    stack.push(std::array::from_fn(|i| {
                        let value = start[i] + (end[i] - start[i]) * time[i];
                        if opcode == 0x36 {
                            value.clamp(0.0, 1.0)
                        } else {
                            value
                        }
                    }));
                }
                0x37 => {
                    let index = operand(code, &mut at)? as usize;
                    let curve = self
                        .constants
                        .get(index..index + 5)
                        .ok_or("Particle cubic curve is missing")?;
                    let time = pop(&mut stack)?;
                    stack.push(math::spline(time, curve, None)?);
                }
                0x38 | 0x39 => {
                    let index = operand(code, &mut at)? as usize;
                    let curve = self
                        .constants
                        .get(index..index + 10)
                        .ok_or("Particle eight-segment curve is missing")?;
                    let time = pop(&mut stack)?;
                    let fallback = if opcode == 0x39 {
                        Some(pop(&mut stack)?)
                    } else {
                        None
                    };
                    stack.push(math::spline(time, curve, fallback)?);
                }
                0x3A | 0x3B => {
                    let index = operand(code, &mut at)? as usize;
                    let count = if opcode == 0x3A { 6 } else { 11 };
                    let gradient = self
                        .constants
                        .get(index..index + count)
                        .ok_or("Particle gradient is missing")?;
                    let input = pop(&mut stack)?;
                    stack.push(math::gradient(input, gradient)?);
                }
                0x3E | 0x3F => {
                    let bank = operand(code, &mut at)?;
                    let slot = operand(code, &mut at)?;
                    let selector = operand(code, &mut at)?;
                    match opcode {
                        0x3E => {
                            let value = next
                                .get(bank, slot)
                                .ok_or("Particle register read is outside its bank")?;
                            stack.push(match selector {
                                0 => value,
                                1 => [value[0], value[1], value[2], value[2]],
                                2 => [value[0], value[1], value[0], value[1]],
                                3 => [value[2], value[3], value[2], value[3]],
                                4..=7 => [value[(selector - 4) as usize]; 4],
                                _ => {
                                    return Err(format!(
                                        "Unsupported particle read selector {selector}"
                                    ));
                                }
                            });
                        }
                        0x3F => {
                            let value = pop(&mut stack)?;
                            let mut destination = next
                                .get(bank, slot)
                                .ok_or("Particle register write is outside its bank")?;
                            match selector {
                                0 => destination = value,
                                1 => destination[..3].copy_from_slice(&value[..3]),
                                2 => destination[..2].copy_from_slice(&value[..2]),
                                3 => destination[2..].copy_from_slice(&value[..2]),
                                4..=7 => destination[(selector - 4) as usize] = value[0],
                                _ => {
                                    return Err(format!(
                                        "Unsupported particle write selector {selector}"
                                    ));
                                }
                            }
                            next.set(bank, slot, destination)?;
                        }
                        _ => unreachable!(),
                    }
                }
                other => {
                    return Err(format!(
                        "Unsupported particle expression opcode 0x{other:02X}"
                    ));
                }
            }
            if stack.len() > 256 {
                return Err("Particle expression stack exceeds its limit".into());
            }
            if stack.iter().flatten().any(|value| !value.is_finite()) {
                return Err("Particle expression produced a nonfinite value".into());
            }
        }
        if !stack.is_empty() {
            return Err("Particle expression left values on its stack".into());
        }
        *registers = next;
        *runtime = state;
        Ok(())
    }
}

fn operand(code: &[u8], at: &mut usize) -> Result<u8, String> {
    let value = *code
        .get(*at)
        .ok_or("Particle expression ends within an instruction")?;
    *at += 1;
    Ok(value)
}

fn pop(stack: &mut Vec<[f32; 4]>) -> Result<[f32; 4], String> {
    stack
        .pop()
        .ok_or_else(|| "Particle expression stack is empty".into())
}

fn pop_values<const N: usize>(stack: &mut Vec<[f32; 4]>) -> Result<[[f32; 4]; N], String> {
    let start = stack
        .len()
        .checked_sub(N)
        .ok_or("Particle expression stack is empty")?;
    let values = std::array::from_fn(|index| stack[start + index]);
    stack.truncate(start);
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(bytecode: Vec<u8>, constants: Vec<[f32; 4]>) -> Program {
        Program {
            sections: [bytecode.len() as u16, 0, 0, 0, 0, 0, 0, 0],
            routes: [None; 56],
            defaults: Vec::new(),
            bytecode,
            constants,
            lifetime_ceiling: 0.0,
        }
    }

    #[test]
    fn evaluates_cubic_expression_into_a_native_register() {
        let program = program(
            vec![0x3E, 2, 0, 4, 0x34, 0, 0x0F, 0x3F, 1, 0, 4],
            vec![[2.0, -3.0, 0.0, 1.0]],
        );
        let mut registers = Registers::new(&program);
        registers.set(2, 0, [0.5, 0.0, 0.0, 0.0]).unwrap();
        program.evaluate_section(0, &mut registers).unwrap();
        assert_eq!(registers.get(1, 0).unwrap()[0], 0.5);
    }

    #[test]
    fn cubic_evaluates_each_input_lane() {
        let program = program(
            vec![0x47, 0, 0x34, 0, 0x0F, 0x3F, 1, 0, 0],
            vec![[0.0, 0.0, 1.0, 0.0]],
        );
        let mut registers = Registers::new(&program);
        program
            .evaluate_section_with_inputs(0, &mut registers, &[[0.0, 0.25, 0.5, 1.0]])
            .unwrap();
        assert_eq!(registers.get(1, 0), Some([0.0, 0.25, 0.5, 1.0]));
    }

    #[test]
    fn fractional_part_and_three_one_merge_match_native_vector_math() {
        let program = program(
            vec![0x47, 0, 0x1A, 0x47, 1, 0x0E, 0x3F, 1, 0, 0],
            Vec::new(),
        );
        let mut registers = Registers::new(&program);
        program
            .evaluate_section_with_inputs(0, &mut registers, &[[-0.25, 1.25, 2.5, 0.0], [9.0; 4]])
            .unwrap();
        assert_eq!(registers.get(1, 0), Some([0.75, 0.25, 0.5, 9.0]));
    }

    #[test]
    fn evaluates_continuous_and_step_piecewise_curves() {
        let program = program(
            vec![0x3E, 2, 0, 4, 0x37, 0, 0x3F, 1, 0, 4],
            vec![
                [0.0; 4],
                [0.0; 4],
                [1.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.5, 0.500001, 1.0],
            ],
        );
        let mut registers = Registers::new(&program);
        registers.set(2, 0, [0.5, 0.0, 0.0, 0.0]).unwrap();
        program.evaluate_section(0, &mut registers).unwrap();
        assert_eq!(registers.get(1, 0).unwrap()[0], 0.5);
        registers.set(2, 0, [0.500001, 0.0, 0.0, 0.0]).unwrap();
        program.evaluate_section(0, &mut registers).unwrap();
        assert_eq!(registers.get(1, 0).unwrap()[0], 1.0);
    }

    #[test]
    fn gradient_uses_four_independent_bounded_segments() {
        let program = program(
            vec![0x47, 0, 0x3A, 0, 0x3F, 1, 0, 0],
            vec![
                [0.5, 0.5, 0.5, 1.0],
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, -1.0],
                [0.0, 0.25, 0.5, 0.75],
            ],
        );
        let mut registers = Registers::new(&program);
        program
            .evaluate_section_with_inputs(0, &mut registers, &[[0.125, 0.375, 0.625, 0.875]])
            .unwrap();
        assert_eq!(registers.get(1, 0), Some([1.0, 1.0, 1.0, 0.5]));
    }

    #[test]
    fn eight_segment_curve_switches_at_the_second_knot_block() {
        let program = program(
            vec![0x47, 0, 0x38, 0, 0x3F, 1, 0, 0],
            vec![
                [0.0; 4],
                [0.0; 4],
                [0.0; 4],
                [2.0; 4],
                [0.0; 4],
                [0.0; 4],
                [0.0; 4],
                [3.0; 4],
                [0.0, 0.125, 0.25, 0.375],
                [0.5, 0.625, 0.75, 0.875],
            ],
        );
        let mut registers = Registers::new(&program);
        program
            .evaluate_section_with_inputs(0, &mut registers, &[[0.499; 4]])
            .unwrap();
        assert_eq!(registers.get(1, 0), Some([2.0; 4]));
        program
            .evaluate_section_with_inputs(0, &mut registers, &[[0.5; 4]])
            .unwrap();
        assert_eq!(registers.get(1, 0), Some([3.0; 4]));
    }

    #[test]
    fn chained_spline_uses_its_previous_result_before_the_first_knot() {
        let mut constants = vec![[0.0; 4]; 10];
        constants[3] = [2.0; 4];
        constants[7] = [3.0; 4];
        constants[8] = [0.2, 0.3, 0.4, 0.5];
        constants[9] = [0.6, 0.7, 0.8, 0.9];
        constants.push([9.0; 4]);
        let program = program(vec![0x34, 10, 0x47, 0, 0x39, 0, 0x3F, 1, 0, 0], constants);
        let mut registers = Registers::new(&program);
        program
            .evaluate_section_with_inputs(0, &mut registers, &[[0.1; 4]])
            .unwrap();
        assert_eq!(registers.get(1, 0), Some([9.0; 4]));
        program
            .evaluate_section_with_inputs(0, &mut registers, &[[0.2; 4]])
            .unwrap();
        assert_eq!(registers.get(1, 0), Some([2.0; 4]));
    }

    #[test]
    fn unsupported_instructions_do_not_commit_partial_register_writes() {
        let program = program(vec![0x34, 0, 0x3F, 1, 0, 0, 0x47, 0], vec![[1.0; 4]]);
        let mut registers = Registers::new(&program);
        assert!(program.evaluate_section(0, &mut registers).is_err());
        assert_eq!(registers.get(1, 0), Some([0.0; 4]));
    }

    #[test]
    fn runtime_inputs_are_required_and_injected_by_index() {
        let program = program(vec![0x47, 1, 0x3F, 1, 0, 0], Vec::new());
        let mut registers = Registers::new(&program);
        assert!(program.evaluate_section(0, &mut registers).is_err());
        assert_eq!(registers.get(1, 0), Some([0.0; 4]));
        program
            .evaluate_section_with_inputs(0, &mut registers, &[[0.0; 4], [1.0, 2.0, 3.0, 4.0]])
            .unwrap();
        assert_eq!(registers.get(1, 0), Some([1.0, 2.0, 3.0, 4.0]));
    }

    #[test]
    fn half_vector_selectors_preserve_the_other_lanes() {
        let program = program(
            vec![0x3E, 2, 0, 3, 0x3F, 1, 0, 2, 0x3E, 2, 0, 2, 0x3F, 1, 0, 3],
            Vec::new(),
        );
        let mut registers = Registers::new(&program);
        registers.set(2, 0, [1.0, 2.0, 3.0, 4.0]).unwrap();
        program.evaluate_section(0, &mut registers).unwrap();
        assert_eq!(registers.get(1, 0), Some([3.0, 4.0, 1.0, 2.0]));
    }

    #[test]
    fn sequential_runtime_pushes_feed_distinct_registers() {
        let program = program(vec![0x3D, 0x3F, 1, 0, 7, 0x3D, 0x3F, 1, 1, 4], Vec::new());
        let mut registers = Registers::new(&program);
        assert!(program.evaluate_section(0, &mut registers).is_err());
        program
            .evaluate_section_with_runtime(0, &mut registers, &[], &[[0.2; 4], [0.8; 4]])
            .unwrap();
        assert_eq!(registers.get(1, 0).unwrap()[3], 0.2);
        assert_eq!(registers.get(1, 1).unwrap()[0], 0.8);
    }

    #[test]
    fn scoped_inputs_are_separate_from_indexed_runtime_inputs() {
        let program = program(vec![0x43, 0, 0x47, 0, 0x02, 0x3F, 1, 0, 0], Vec::new());
        let mut registers = Registers::new(&program);
        program
            .evaluate_section_with_sources(0, &mut registers, &[[1.0; 4]], &[], &[[3.0; 4]])
            .unwrap();
        assert_eq!(registers.get(1, 0), Some([2.0; 4]));
    }

    #[test]
    fn external_inputs_keep_their_own_index_space() {
        let program = program(vec![0x46, 1, 0x47, 1, 0x02, 0x3F, 1, 0, 0], Vec::new());
        let mut registers = Registers::new(&program);
        assert!(
            program
                .evaluate_section_with_sources(0, &mut registers, &[[0.0; 4]; 2], &[], &[])
                .is_err()
        );
        program
            .evaluate_section_with_external(
                0,
                &mut registers,
                &[[0.0; 4], [2.0; 4]],
                &[],
                &[],
                &[[0.0; 4], [7.0; 4]],
            )
            .unwrap();
        assert_eq!(registers.get(1, 0), Some([5.0; 4]));
    }

    #[test]
    fn interpolation_uses_the_native_three_value_stack_order() {
        let program = program(
            vec![0x34, 0, 0x34, 1, 0x34, 2, 0x10, 0x3F, 1, 0, 0],
            vec![[10.0; 4], [2.0; 4], [0.25; 4]],
        );
        let mut registers = Registers::new(&program);
        program.evaluate_section(0, &mut registers).unwrap();
        assert_eq!(registers.get(1, 0), Some([8.0; 4]));
    }

    #[test]
    fn comparison_includes_equality_in_the_native_stack_order() {
        let program = program(
            vec![0x34, 0, 0x34, 1, 0x0A, 0x3F, 1, 0, 0],
            vec![[2.0, 2.0, 2.0, 2.0], [1.0, 3.0, 2.0, -1.0]],
        );
        let mut registers = Registers::new(&program);
        program.evaluate_section(0, &mut registers).unwrap();
        assert_eq!(registers.get(1, 0), Some([0.0, 1.0, 1.0, 0.0]));
    }

    #[test]
    fn clamp_uses_value_then_lower_and_upper_bounds() {
        let program = program(
            vec![0x34, 0, 0x34, 1, 0x34, 2, 0x13, 0x3F, 1, 0, 0],
            vec![[-2.0, 0.5, 3.0, 1.5], [0.0; 4], [1.0; 4]],
        );
        let mut registers = Registers::new(&program);
        program.evaluate_section(0, &mut registers).unwrap();
        assert_eq!(registers.get(1, 0), Some([0.0, 0.5, 1.0, 1.0]));
    }
}
