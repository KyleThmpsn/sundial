//! Recovered ordinary CPU particle rules under an explicit deterministic studio host.
//! Scene bindings, attachment transforms, trails and the engine allocator are not inferred.
use super::super::assets::{Program, Registers, Runtime, Sources};
mod placement;

const STEP: f32 = 1.0 / 60.0;
const MAX_PARTICLES: usize = 128;
const MAX_RECORDS: usize = 16_384;

pub(crate) struct Instance {
    pub attributes: [[f32; 4]; 7],
}

pub(crate) struct Timeline {
    frames: Vec<Vec<Instance>>,
    pub duration: f32,
    pub peak: usize,
}

#[derive(Clone, Copy)]
struct Geometry {
    position: u8,
    direction: u8,
    random_grid: bool,
}

struct State<'a> {
    program: &'a Program,
    geometry: Geometry,
    runtime: Runtime,
    global: Registers,
    active: Vec<Registers>,
    burst: usize,
    fraction: f32,
    counter: u32,
}

impl Timeline {
    pub(crate) fn build(program: &Program, bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 0x150 || bytes[0x141] != 4 || bytes[0x142] != 7 {
            return Err(
                "Particle simulation requires the recovered state and appearance layout".into(),
            );
        }
        if bytes[0x14B] > 4 || bytes[0x115] > 5 || bytes[0x116] > 4 {
            return Err(
                "Particle geometry requires an unsupported expansion or placement rule".into(),
            );
        }
        if program.state_routes().is_none() || program.routes[31].is_some() {
            return Err(
                "Particle simulation requires a compatible state layout and explicit motion inputs"
                    .into(),
            );
        }
        if program.bytecode.len().saturating_mul(MAX_RECORDS) > 32 * 1024 * 1024 {
            return Err("Particle simulation exceeds the preview instruction budget".into());
        }
        let lifetime = program
            .lifetime_default()
            .filter(|v| (0.01..=5.0).contains(v))
            .ok_or("Particle simulation requires a bounded stored lifetime")?;
        let geometry = Geometry {
            position: bytes[0x115],
            direction: bytes[0x116],
            random_grid: u16::from_le_bytes([bytes[0x44], bytes[0x45]]) & 8192 != 0,
        };
        let mut state = State {
            program,
            geometry,
            runtime: Runtime {
                seed: Some(17),
                ..Default::default()
            },
            global: Registers::new(program),
            active: Vec::new(),
            burst: 0,
            fraction: 0.0,
            counter: 0,
        };
        for phase in [7, 0] {
            evaluate(program, phase, &mut state.global, &mut state.runtime)?;
        }
        state.burst = count(scalar(program, &state.global, 16)?, "burst")?;
        // This is a preview activation window, not a recovered engine lifetime schedule.
        let duration = (lifetime * 2.0).clamp(1.0, 10.0);
        let ticks = (duration / STEP).ceil() as usize;
        let mut frames = Vec::with_capacity(ticks + 1);
        let mut records = 0;
        for tick in 0..=ticks {
            state.step(if tick == 0 { 0.0 } else { STEP })?;
            records += state.active.len();
            if records > MAX_RECORDS {
                return Err("Particle simulation exceeds the preview record budget".into());
            }
            frames.push(
                state
                    .active
                    .iter()
                    .map(|registers| Instance {
                        attributes: std::array::from_fn(|slot| {
                            registers.get(1, slot as u8).unwrap()
                        }),
                    })
                    .collect(),
            );
        }
        let peak = frames.iter().map(Vec::len).max().unwrap_or(0);
        Ok(Self {
            frames,
            duration,
            peak,
        })
    }

    pub(crate) fn at(&self, seconds: f32) -> &[Instance] {
        let tick = ((seconds.max(0.0) / STEP) + 0.00001).floor() as usize;
        self.frames
            .get(tick.min(self.frames.len() - 1))
            .map_or(&[], Vec::as_slice)
    }
}

impl State<'_> {
    fn step(&mut self, elapsed: f32) -> Result<(), String> {
        evaluate(self.program, 1, &mut self.global, &mut self.runtime)?;
        let rate = scalar(self.program, &self.global, 10)?;
        if rate < 0.0 {
            return Err("Particle emission rate is negative".into());
        }
        let accumulated = rate * elapsed + self.fraction;
        let continuous = count(accumulated, "emission")?;
        self.fraction = accumulated - continuous as f32;
        let requested = continuous + std::mem::take(&mut self.burst);
        let capacity = count(scalar(self.program, &self.global, 17)?, "capacity")?;
        let capacity = if capacity == 0 { 65_535 } else { capacity };
        let spawned = requested.min(capacity.saturating_sub(self.active.len()));
        if self.active.len() + spawned > MAX_PARTICLES {
            return Err("Particle simulation exceeds the preview live-particle budget".into());
        }
        for _ in 0..spawned {
            self.spawn()?;
        }
        for registers in &mut self.active {
            // Bank 5 is the shared frame result. Per-particle state and appearance persist.
            for slot in 0..64 {
                registers.set(5, slot, self.global.get(5, slot).unwrap())?;
            }
            let life = scalar(self.program, registers, 5)?;
            let age = (scalar(self.program, registers, 42)?
                + if life < 0.0001 { 1.0 } else { elapsed / life })
            .min(1.0);
            set_scalar(self.program, registers, 42, age)?;
            set_scalar(self.program, registers, 0, age)?;
            evaluate(self.program, 4, registers, &mut self.runtime)?;
            motion(self.program, registers, elapsed)?;
        }
        // Retirement is the studio host's explicit policy. The native draw's cutoff is
        // stricter than the host's removal boundary and is evaluated by the material.
        self.active
            .retain(|registers| scalar(self.program, registers, 42).is_ok_and(|age| age < 1.0));
        Ok(())
    }

    fn spawn(&mut self) -> Result<(), String> {
        let mut registers = self.global.clone();
        set_scalar(self.program, &mut registers, 42, 0.0)?;
        let age = scalar(self.program, &registers, 2)?;
        set_scalar(self.program, &mut registers, 38, age)?;
        set_scalar(self.program, &mut registers, 43, self.counter as f32)?;
        for phase in [6, 2] {
            evaluate(self.program, phase, &mut registers, &mut self.runtime)?;
        }
        let position = placement::position(
            self.program,
            &registers,
            self.geometry,
            self.counter,
            &mut self.runtime,
        )?;
        let direction = placement::direction(
            self.program,
            &registers,
            self.geometry,
            position,
            &mut self.runtime,
        )?;
        set_xyz(self.program, &mut registers, 8, position)?;
        set_xyz(self.program, &mut registers, 9, direction)?;
        evaluate(self.program, 3, &mut registers, &mut self.runtime)?;
        self.active.push(registers);
        self.counter = self
            .counter
            .checked_add(1)
            .ok_or("Particle counter overflow")?;
        Ok(())
    }
}

fn evaluate(
    program: &Program,
    phase: usize,
    registers: &mut Registers,
    runtime: &mut Runtime,
) -> Result<(), String> {
    program.evaluate_section_with_state(phase, registers, Sources::default(), runtime)
}

fn count(value: f32, name: &str) -> Result<usize, String> {
    if !value.is_finite() || !(0.0..=65_535.0).contains(&value) {
        return Err(format!(
            "Particle {name} is outside the supported count range"
        ));
    }
    Ok(value.trunc() as usize)
}

fn scalar(program: &Program, registers: &Registers, route: usize) -> Result<f32, String> {
    registers
        .output(program, route)
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("Particle input route {route} is missing or invalid"))
}

fn vector(program: &Program, registers: &Registers, route: usize) -> Result<[f32; 4], String> {
    let route = program.routes[route].ok_or("Particle vector route is missing")?;
    if route.scalar % 4 != 0 {
        return Err("Particle vector route is not aligned".into());
    }
    registers
        .get(route.bank, route.scalar / 4)
        .filter(|v| v.iter().all(|v| v.is_finite()))
        .ok_or("Particle vector input is missing or invalid".into())
}

fn set_scalar(
    program: &Program,
    registers: &mut Registers,
    index: usize,
    value: f32,
) -> Result<(), String> {
    let route = program.routes[index].ok_or("Particle destination route is missing")?;
    if route.bank == 6 {
        return Err("Particle destination points to the constant bank".into());
    }
    let mut row = registers
        .get(route.bank, route.scalar / 4)
        .ok_or("Particle destination is missing")?;
    row[usize::from(route.scalar % 4)] = value;
    registers.set(route.bank, route.scalar / 4, row)
}

fn set_xyz(
    program: &Program,
    registers: &mut Registers,
    index: usize,
    value: [f32; 3],
) -> Result<(), String> {
    let route = program.routes[index].ok_or("Particle destination route is missing")?;
    if route.bank == 6 {
        return Err("Particle destination points to the constant bank".into());
    }
    let mut row = vector(program, registers, index)?;
    row[..3].copy_from_slice(&value);
    registers.set(route.bank, route.scalar / 4, row)
}

fn motion(program: &Program, registers: &mut Registers, elapsed: f32) -> Result<(), String> {
    let position = vector(program, registers, 6)?;
    let velocity = vector(program, registers, 7)?;
    // The studio attachment is identity, so both force-space gate values preserve XYZ.
    let first = vector(program, registers, 18)?;
    let second = vector(program, registers, 20)?;
    let a = scalar(program, registers, 19)?;
    let b = scalar(program, registers, 21)?;
    let damping = 1.0 - (scalar(program, registers, 22)? * elapsed).clamp(0.0, 1.0);
    let velocity =
        std::array::from_fn(|i| (velocity[i] + elapsed * (first[i] * a + second[i] * b)) * damping);
    let position = std::array::from_fn(|i| position[i] + elapsed * velocity[i]);
    set_xyz(program, registers, 7, velocity)?;
    set_xyz(program, registers, 6, position)
}
