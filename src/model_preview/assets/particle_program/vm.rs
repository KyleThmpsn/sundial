//! Evaluates the validated subset of native particle expression bytecode.
//! Unknown operations and selectors fail closed so callers cannot mistake a partial result
//! for an authored spawn or motion output.
use super::Program;

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

    /// Supplies unindexed `0x3D` values in encounter order. The engine source of
    /// these values is not encoded in the bytecode, so the caller must provide it.
    pub fn evaluate_section_with_runtime(
        &self,
        section: usize,
        registers: &mut Registers,
        inputs: &[[f32; 4]],
        pushes: &[[f32; 4]],
    ) -> Result<(), String> {
        self.evaluate_section_with_sources(section, registers, inputs, pushes, &[])
    }

    /// `0x43` and `0x47` index distinct native input families. Their engine
    /// bindings remain unresolved, so they have separate caller-supplied tables.
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
        let code = self.section(section).ok_or("Particle section is missing")?;
        let mut next = registers.clone();
        let mut stack = Vec::<[f32; 4]>::new();
        let mut at = 0;
        let mut pushed = 0;
        while at < code.len() {
            let opcode = code[at];
            at += 1;
            match opcode {
                0x01 | 0x02 | 0x03 | 0x04 | 0x05 | 0x06 | 0x08 | 0x09 | 0x0A | 0x0B | 0x0C
                | 0x0D | 0x0E | 0x0F => {
                    let right = pop(&mut stack)?;
                    let left = pop(&mut stack)?;
                    let result = match opcode {
                        0x01 | 0x06 => std::array::from_fn(|i| left[i] + right[i]),
                        0x02 => std::array::from_fn(|i| left[i] - right[i]),
                        0x03 | 0x05 => std::array::from_fn(|i| left[i] * right[i]),
                        0x04 => std::array::from_fn(|i| left[i] / right[i]),
                        0x08 => std::array::from_fn(|i| left[i].min(right[i])),
                        0x09 => std::array::from_fn(|i| left[i].max(right[i])),
                        // Shadowkeep compares the most recently pushed value with the
                        // preceding one, in that order.
                        0x0A => std::array::from_fn(|i| u8::from(right[i] < left[i]) as f32),
                        0x0B => [left.into_iter().zip(right).map(|(a, b)| a * b).sum(); 4],
                        0x0C => [left[0], right[0], right[1], right[2]],
                        0x0D => [left[0], left[1], right[0], right[1]],
                        0x0E => [left[0], left[1], left[2], right[0]],
                        // The four coefficient lanes encode one cubic, evaluated at each
                        // input lane's time.
                        0x0F => left.map(|time| polynomial(right, time)),
                        _ => unreachable!(),
                    };
                    stack.push(result);
                }
                0x10 | 0x12 | 0x13 => {
                    let c = pop(&mut stack)?;
                    let b = pop(&mut stack)?;
                    let a = pop(&mut stack)?;
                    stack.push(match opcode {
                        // The bytecode pushes the destination, start, and weight in that order.
                        0x10 => std::array::from_fn(|i| b[i] + c[i] * (a[i] - b[i])),
                        0x12 => std::array::from_fn(|i| a[i] * b[i] + c[i]),
                        0x13 => std::array::from_fn(|i| a[i].max(b[i]).min(c[i])),
                        _ => unreachable!(),
                    });
                }
                0x07 | 0x15 | 0x16 | 0x17 | 0x18 | 0x19 | 0x1A | 0x1D | 0x21 | 0x23 | 0x27 => {
                    let value = pop(&mut stack)?;
                    stack.push(match opcode {
                        0x07 => value.map(|v| u8::from(v == 0.0) as f32),
                        0x15 => value.map(f32::abs),
                        0x16 => value.map(f32::signum),
                        0x17 => value.map(f32::floor),
                        0x18 => value.map(f32::ceil),
                        0x19 => value.map(f32::round),
                        0x1A => value.map(|v| v - v.floor()),
                        0x1D => value.map(|v| -v),
                        0x21 => [value[0]; 4],
                        0x23 => value.map(|v| v.clamp(0.0, 1.0)),
                        0x27 => value.map(|v| (v - v.round()).abs() * 2.0),
                        _ => unreachable!(),
                    });
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
                    stack.push(
                        *pushes
                            .get(pushed)
                            .ok_or_else(|| format!("Particle runtime push {pushed} is missing"))?,
                    );
                    pushed += 1;
                }
                0x43 => {
                    let index = operand(code, &mut at)? as usize;
                    stack.push(
                        *scoped_inputs
                            .get(index)
                            .ok_or_else(|| format!("Particle scoped input {index} is missing"))?,
                    );
                }
                0x46 => {
                    let index = operand(code, &mut at)? as usize;
                    stack.push(
                        *external_inputs
                            .get(index)
                            .ok_or_else(|| format!("Particle external input {index} is missing"))?,
                    );
                }
                0x47 => {
                    let index = operand(code, &mut at)? as usize;
                    stack.push(
                        *inputs
                            .get(index)
                            .ok_or_else(|| format!("Particle runtime input {index} is missing"))?,
                    );
                }
                0x35 => {
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
                        start[i] + (end[i] - start[i]) * time[i]
                    }));
                }
                0x37 => {
                    let index = operand(code, &mut at)? as usize;
                    let curve = self
                        .constants
                        .get(index..index + 5)
                        .ok_or("Particle cubic curve is missing")?;
                    let time = pop(&mut stack)?[0];
                    let knots = curve[4];
                    if knots.windows(2).any(|pair| pair[0] > pair[1]) {
                        return Err("Particle cubic curve has unordered knots".into());
                    }
                    let segment = (1..4).take_while(|&i| time >= knots[i]).count();
                    let coefficients = std::array::from_fn(|row| curve[row][segment]);
                    stack.push([polynomial(coefficients, time); 4]);
                }
                0x38 | 0x39 => {
                    let index = operand(code, &mut at)? as usize;
                    let curve = self
                        .constants
                        .get(index..index + 10)
                        .ok_or("Particle eight-segment curve is missing")?;
                    let time = pop(&mut stack)?[0];
                    let fallback = if opcode == 0x39 {
                        Some(pop(&mut stack)?[0])
                    } else {
                        None
                    };
                    let first = curve[8];
                    let second = curve[9];
                    if first.windows(2).any(|pair| pair[0] > pair[1])
                        || second.windows(2).any(|pair| pair[0] > pair[1])
                        || first[3] > second[0]
                    {
                        return Err("Particle eight-segment curve has unordered knots".into());
                    }
                    let value = if time < first[0] {
                        fallback.unwrap_or(0.0)
                    } else {
                        let (base, knots) = if time >= second[0] {
                            (4, second)
                        } else {
                            (0, first)
                        };
                        let segment = (1..4).take_while(|&i| time >= knots[i]).count();
                        let coefficients = std::array::from_fn(|row| curve[base + row][segment]);
                        polynomial(coefficients, time)
                    };
                    stack.push([value; 4]);
                }
                0x3A => {
                    let index = operand(code, &mut at)? as usize;
                    let gradient = self
                        .constants
                        .get(index..index + 6)
                        .ok_or("Particle gradient is missing")?;
                    let bounds = gradient[5];
                    if bounds.windows(2).any(|pair| pair[0] > pair[1]) {
                        return Err("Particle gradient has unordered bounds".into());
                    }
                    let input = pop(&mut stack)?;
                    let weights: [f32; 4] = std::array::from_fn(|i| {
                        let end = if i == 3 { 1.0 } else { bounds[i + 1] };
                        let width = end - bounds[i];
                        if width.abs() < 1e-6 {
                            u8::from(input[i] > bounds[i]) as f32
                        } else {
                            ((input[i] - bounds[i]) / width).clamp(0.0, 1.0)
                        }
                    });
                    stack.push(std::array::from_fn(|lane| {
                        gradient[0][lane]
                            + (0..4)
                                .map(|segment| gradient[segment + 1][lane] * weights[segment])
                                .sum::<f32>()
                    }));
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
                                0 | 1 => value,
                                2 => [value[0], value[1], 0.0, 0.0],
                                3 => [value[2], value[3], 0.0, 0.0],
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
                                0 | 1 => destination = value,
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

fn polynomial(coefficients: [f32; 4], time: f32) -> f32 {
    ((coefficients[0] * time + coefficients[1]) * time + coefficients[2]) * time + coefficients[3]
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
            vec![0x47, 0, 0x34, 0, 0x0F, 0x3F, 1, 0, 1],
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
            vec![0x47, 0, 0x1A, 0x47, 1, 0x0E, 0x3F, 1, 0, 1],
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
            vec![0x47, 0, 0x3A, 0, 0x3F, 1, 0, 1],
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
            vec![0x47, 0, 0x38, 0, 0x3F, 1, 0, 1],
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
        let program = program(vec![0x34, 10, 0x47, 0, 0x39, 0, 0x3F, 1, 0, 1], constants);
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
        let program = program(vec![0x34, 0, 0x3F, 1, 0, 1, 0x47, 0], vec![[1.0; 4]]);
        let mut registers = Registers::new(&program);
        assert!(program.evaluate_section(0, &mut registers).is_err());
        assert_eq!(registers.get(1, 0), Some([0.0; 4]));
    }

    #[test]
    fn runtime_inputs_are_required_and_injected_by_index() {
        let program = program(vec![0x47, 1, 0x3F, 1, 0, 1], Vec::new());
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
        let program = program(vec![0x43, 0, 0x47, 0, 0x02, 0x3F, 1, 0, 1], Vec::new());
        let mut registers = Registers::new(&program);
        program
            .evaluate_section_with_sources(0, &mut registers, &[[1.0; 4]], &[], &[[3.0; 4]])
            .unwrap();
        assert_eq!(registers.get(1, 0), Some([2.0; 4]));
    }

    #[test]
    fn external_inputs_keep_their_own_index_space() {
        let program = program(vec![0x46, 1, 0x47, 1, 0x02, 0x3F, 1, 0, 1], Vec::new());
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
            vec![0x34, 0, 0x34, 1, 0x34, 2, 0x10, 0x3F, 1, 0, 1],
            vec![[10.0; 4], [2.0; 4], [0.25; 4]],
        );
        let mut registers = Registers::new(&program);
        program.evaluate_section(0, &mut registers).unwrap();
        assert_eq!(registers.get(1, 0), Some([4.0; 4]));
    }

    #[test]
    fn less_than_uses_the_native_stack_order() {
        let program = program(
            vec![0x34, 0, 0x34, 1, 0x0A, 0x3F, 1, 0, 1],
            vec![[2.0, 2.0, 2.0, 2.0], [1.0, 3.0, 2.0, -1.0]],
        );
        let mut registers = Registers::new(&program);
        program.evaluate_section(0, &mut registers).unwrap();
        assert_eq!(registers.get(1, 0), Some([1.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn clamp_uses_value_then_lower_and_upper_bounds() {
        let program = program(
            vec![0x34, 0, 0x34, 1, 0x34, 2, 0x13, 0x3F, 1, 0, 1],
            vec![[-2.0, 0.5, 3.0, 1.5], [0.0; 4], [1.0; 4]],
        );
        let mut registers = Registers::new(&program);
        program.evaluate_section(0, &mut registers).unwrap();
        assert_eq!(registers.get(1, 0), Some([0.0, 0.5, 1.0, 1.0]));
    }
}
