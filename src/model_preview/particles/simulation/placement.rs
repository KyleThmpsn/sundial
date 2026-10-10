use super::{Geometry, Program, Registers, Runtime, scalar, vector};

fn random(runtime: &mut Runtime) -> Result<f32, String> {
    let seed = runtime
        .seed
        .as_mut()
        .ok_or("Particle random seed is missing")?;
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    Ok((*seed >> 16) as f32 * (1.0 / 65_535.0))
}

fn cube(runtime: &mut Runtime) -> Result<[f32; 3], String> {
    let z = random(runtime)? * 2.0 - 1.0;
    let y = random(runtime)? * 2.0 - 1.0;
    let x = random(runtime)? * 2.0 - 1.0;
    Ok([x, y, z])
}

fn normalized(v: [f32; 3]) -> [f32; 3] {
    let length = v.iter().map(|v| v * v).sum::<f32>().sqrt();
    if length == 0.0 {
        [0.0; 3]
    } else {
        v.map(|v| v / length)
    }
}

pub(super) fn position(
    program: &Program,
    registers: &Registers,
    geometry: Geometry,
    index: u32,
    runtime: &mut Runtime,
) -> Result<[f32; 3], String> {
    let point = match geometry.position {
        0 => [0.0; 3],
        1 => {
            let angle = random(runtime)? * std::f32::consts::TAU;
            let radius = scalar(program, registers, 13)?;
            [0.0, angle.sin() * radius, angle.cos() * radius]
        }
        2 => {
            let radius = scalar(program, registers, 13)?.max(0.001);
            normalized(cube(runtime)?).map(|v| v * radius)
        }
        3 => [
            0.0,
            scalar(program, registers, 14)? * (random(runtime)? - 0.5),
            0.0,
        ],
        4 => {
            let extent = scalar(program, registers, 14)? * 0.5;
            cube(runtime)?.map(|v| v * extent)
        }
        5 => grid(program, registers, geometry.random_grid, index, runtime)?,
        _ => return Err("Unsupported particle position selector".into()),
    };
    let scale = vector(program, registers, 11)?;
    Ok(std::array::from_fn(|i| point[i] * scale[i]))
}

fn grid(
    program: &Program,
    registers: &Registers,
    random_grid: bool,
    mut index: u32,
    runtime: &mut Runtime,
) -> Result<[f32; 3], String> {
    let counts = vector(program, registers, 54)?;
    let extent = vector(program, registers, 14)?;
    if counts[..3].iter().any(|v| !(0.5..=1024.0).contains(v)) {
        return Err("Particle grid dimensions are outside the supported range".into());
    }
    let dimensions = counts.map(|v| (v + 0.5).trunc() as u32);
    if random_grid {
        let product = counts[0] * counts[1] * counts[2];
        index = (((product - 0.000001) * random(runtime)?).floor() + 0.5).trunc() as u32;
    }
    let coordinate = [
        index % dimensions[0],
        index / dimensions[0] % dimensions[1],
        index / (dimensions[0] * dimensions[1]) % dimensions[2],
    ];
    Ok(std::array::from_fn(|i| {
        if counts[i] > 1.5 {
            extent[i] * (coordinate[i] as f32 / (counts[i] - 1.0) - 0.5)
        } else {
            0.0
        }
    }))
}

pub(super) fn direction(
    program: &Program,
    registers: &Registers,
    geometry: Geometry,
    position: [f32; 3],
    runtime: &mut Runtime,
) -> Result<[f32; 3], String> {
    let direction = match geometry.direction {
        0 => {
            let azimuth = random(runtime)? * std::f32::consts::TAU;
            let polar = random(runtime)? * scalar(program, registers, 12)?.to_radians();
            [
                polar.cos(),
                azimuth.sin() * polar.sin(),
                azimuth.cos() * polar.sin(),
            ]
        }
        1 => {
            let angle = scalar(program, registers, 12)?.to_radians();
            let denominator = position[1].hypot(position[2]).max(0.0001);
            [
                angle.cos(),
                position[1] / denominator * angle.sin(),
                position[2] / denominator * angle.sin(),
            ]
        }
        2 => normalized(cube(runtime)?),
        3 | 4 => normalized(position),
        _ => return Err("Unsupported particle direction selector".into()),
    };
    let magnitude = scalar(program, registers, 15)?;
    Ok(direction.map(|v| v * magnitude))
}
