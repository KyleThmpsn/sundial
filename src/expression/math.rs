//! Native vector and curve math used by the preview expression evaluators.
//! Recovered from the Shadowkeep particle and material dispatches and their helpers.
//! Offline comparisons execute the captured machine code, not another VM.

type Vector = [f32; 4];

pub(crate) fn length(value: Vector) -> f32 {
    sum(value.map(|v| v * v)).sqrt()
}

pub(crate) fn normalize(value: Vector, xyz: bool) -> Vector {
    let squares = value.map(|v| v * v);
    let length = if xyz {
        ((squares[0] + squares[1]) + squares[2]).sqrt()
    } else {
        sum(squares).sqrt()
    };
    if length == 0.0 {
        [0.0; 4]
    } else {
        value.map(|v| v / length)
    }
}

pub(crate) fn rotate(value: Vector, rotation: Vector) -> Vector {
    let dot = (rotation[0] * value[0] + rotation[1] * value[1]) + rotation[2] * value[2];
    let scale = rotation[3] * rotation[3] - 0.5;
    std::array::from_fn(|i| {
        if i == 3 {
            return 0.0;
        }
        let j = (i + 1) % 3;
        let k = (i + 2) % 3;
        let cross = rotation[j] * value[k] - rotation[k] * value[j];
        ((scale * value[i] + dot * rotation[i]) + rotation[3] * cross) * 2.0
    })
}

pub(crate) fn rotate_axis(value: Vector, axis: Vector) -> Vector {
    rotate(value, axis_rotation(axis, axis[3]))
}

fn axis_rotation(axis: Vector, turns: f32) -> Vector {
    let angle = (turns * std::f32::consts::TAU) * 0.5;
    let direction = normalize(axis, true);
    let sine = angle.sin();
    [
        direction[0] * sine,
        direction[1] * sine,
        direction[2] * sine,
        angle.cos(),
    ]
}

fn multiply(a: Vector, b: Vector) -> Vector {
    // Keep the native SIMD operation order, including the scalar lane.
    [
        ((a[0] * b[3] + a[3] * b[0]) + a[1] * b[2]) - a[2] * b[1],
        ((a[1] * b[3] + a[3] * b[1]) + a[2] * b[0]) - a[0] * b[2],
        ((a[2] * b[3] + a[3] * b[2]) + a[0] * b[1]) - a[1] * b[0],
        ((-(a[0] * b[0]) + a[3] * b[3]) - a[1] * b[1]) - a[2] * b[2],
    ]
}

/// Native 4F rotates a transform around an axis in a reference frame. Native
/// 52 changes only its orientation. Stack order is translation, rotation,
/// reference translation, reference rotation, axis, angle in turns.
pub(crate) fn rotate_frame(values: [Vector; 6], move_position: bool) -> [Vector; 2] {
    let [mut position, rotation, origin, frame, axis, angle] = values;
    // The engine negates W, rather than XYZ. This represents the same inverse
    // rotation as a conjugate, but retains the engine's quaternion sign.
    let inverse = [frame[0], frame[1], frame[2], -frame[3]];
    let delta = axis_rotation(axis, angle[0]);
    let rotation = multiply(frame, multiply(delta, multiply(inverse, rotation)));
    if move_position {
        // The inverse-transform helper uses the origin's W as its scale. Its
        // position transform then applies XYZ rotation and translation with W=1.
        let inverse_origin = rotate(origin, inverse);
        let local = rotate(position, inverse);
        let reciprocal = 1.0 / origin[3];
        let local = std::array::from_fn(|i| {
            if i == 3 {
                1.0
            } else {
                local[i] + (-reciprocal) * inverse_origin[i]
            }
        });
        let local = rotate(local, delta);
        let world = rotate(local, frame);
        position = [
            world[0] + origin[0],
            world[1] + origin[1],
            world[2] + origin[2],
            1.0,
        ];
    }
    [position, rotation]
}

/// Native 50 derives its local axis from the transform's forward direction and
/// position relative to the reference frame, then rotates position and orientation.
pub(crate) fn orbit_frame(values: [Vector; 5]) -> [Vector; 2] {
    let [position, rotation, origin, frame, angle] = values;
    let inverse = [frame[0], frame[1], frame[2], -frame[3]];
    let inverse_origin = rotate(origin, inverse);
    let local = rotate(position, inverse);
    let reciprocal = 1.0 / origin[3];
    let local: Vector = std::array::from_fn(|i| local[i] + (-reciprocal) * inverse_origin[i]);
    let forward = rotate([1.0, 0.0, 0.0, 0.0], multiply(inverse, rotation));
    let axis = std::array::from_fn(|i| {
        if i == 3 {
            0.0
        } else {
            let j = (i + 1) % 3;
            let k = (i + 2) % 3;
            forward[j] * local[k] - forward[k] * local[j]
        }
    });
    rotate_frame([position, rotation, origin, frame, axis, angle], true)
}

fn pseudo_sine(value: f32) -> f32 {
    let phase = value - value.round_ties_even();
    phase * (-16.0 * phase.abs() + 8.0)
}

pub(crate) fn sine(value: f32) -> f32 {
    let wave = pseudo_sine(value);
    wave * (0.225 * wave.abs() + 0.775)
}

pub(crate) fn sum(value: Vector) -> f32 {
    (value[0] + value[2]) + (value[1] + value[3])
}

pub(crate) fn jitter(value: f32) -> f32 {
    let frequency = [4.67, 2.99, 1.08, 1.35];
    let phase = [0.52, 0.37, 0.16, 0.79];
    let value = sum(std::array::from_fn(|i| {
        pseudo_sine(value * frequency[i] + phase[i]) * 0.25
    })) + 0.5;
    value * value * (3.0 - 2.0 * value)
}

pub(crate) fn wander(value: f32) -> f32 {
    // These are the stored f32 coefficients, not recomputed rational values.
    let a = [4.08, 1.02, 0.558_659_26, 0.310_237_85];
    let b = [0.92, 0.33, 0.26, 0.54];
    let c = [1.83, 3.09, 0.39, 0.87];
    let d = [0.12, 0.37, 0.16, 0.79];
    let weight = [0.02, 0.02, 0.28, 0.28];
    0.5 + sum(std::array::from_fn(|i| {
        (pseudo_sine(value * c[i] + d[i]) * weight[i]) * pseudo_sine(value * a[i] + b[i])
    }))
}

/// The native estimate uses stored polynomial coefficients, not the platform exp2 function.
pub(crate) fn exp2(value: f32) -> f32 {
    let rounded = value.round_ties_even();
    let integer = if (-2_147_483_648.0..2_147_483_648.0).contains(&rounded) {
        rounded as i32
    } else {
        i32::MIN
    };
    let fraction = value - integer as f32;
    let exponent = f32::from_bits((integer as u32).wrapping_add(127).wrapping_shl(23));
    (fraction * fraction * f32::from_bits(0x3E70_F0F1)
        + (fraction * f32::from_bits(0x3F34_B4B5) + 1.0))
        * exponent
}

pub(crate) fn log2(value: f32) -> f32 {
    let bits = value.to_bits();
    let fraction = (bits & 0x7F_FFFF) as f32 * f32::from_bits(0x3400_0000);
    let exponent = ((bits & 0x7F80_0000) >> 23) as i32 - 127;
    ((f32::from_bits(0xBF15_DE28) + f32::from_bits(0x3E26_0F04) * fraction) * (fraction * fraction)
        + f32::from_bits(0x3FB6_2D34) * fraction)
        + exponent as f32
}

/// Scalar pseudo-random noise keyed by floor(input.x), matching the native SIMD reduction.
pub(crate) fn noise(value: f32) -> f32 {
    let cell = value.floor();
    let frequency = [0x3F75_53F7, 0x3FA1_1D0E, 0x410C_A03F, 0x4284_710A];
    let phase = sum(frequency.map(|bits| cell * f32::from_bits(bits)));
    let fraction = phase - phase.floor();
    let value = (fraction * fraction) * 251.0;
    value - value.floor()
}

pub(crate) fn smooth_noise(value: f32) -> f32 {
    let cell = value.floor();
    let weight = value - cell;
    let weight = (3.0 - 2.0 * weight) * (weight * weight);
    let start = noise(cell);
    start + weight * (noise(cell + 1.0) - start)
}

pub(crate) fn matrix(rows: [Vector; 4], value: Vector) -> Vector {
    std::array::from_fn(|i| {
        (rows[0][i] * value[0] + rows[1][i] * value[1])
            + (rows[2][i] * value[2] + rows[3][i] * value[3])
    })
}

pub(crate) fn interpolate_transform(values: [Vector; 5]) -> [Vector; 2] {
    let [position, rotation, target_position, target_rotation, weight] = values;
    let interpolate =
        |a: Vector, b: Vector| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * weight[0]);
    [
        interpolate(position, target_position),
        normalize(interpolate(rotation, target_rotation), false),
    ]
}

/// Native spline selection XOR-reduces masked cubic lanes. Usually the input is a replicated
/// scalar, but retaining all lanes matters for nonuniform inputs and before-first-knot behavior.
pub(crate) fn spline(
    input: Vector,
    constants: &[Vector],
    fallback: Option<Vector>,
) -> Result<Vector, String> {
    let groups = match constants.len() {
        5 => 1,
        10 => 2,
        _ => return Err("Expression spline layout is unsupported".into()),
    };
    let knots = &constants[groups * 4..];
    let ordered = knots.iter().flatten().copied().collect::<Vec<_>>();
    if ordered.windows(2).any(|pair| pair[0] > pair[1]) {
        return Err("Expression spline has unordered knots".into());
    }
    let evaluate = |group: usize| {
        let base = group * 4;
        let active: [bool; 4] = std::array::from_fn(|i| knots[group][i] <= input[i]);
        let bits = (0..4).fold(0, |bits, lane| {
            if active[lane] == active.get(lane + 1).copied().unwrap_or(false) {
                return bits;
            }
            let time = input[lane];
            let high = constants[base][lane] * time + constants[base + 1][lane];
            let low = constants[base + 2][lane] * time + constants[base + 3][lane];
            bits ^ (high * (time * time) + low).to_bits()
        });
        [f32::from_bits(bits); 4]
    };
    if groups == 2 && input[0] >= knots[1][0] {
        Ok(evaluate(1))
    } else if let Some(value) = fallback.filter(|_| input[0] < knots[0][0]) {
        Ok(value)
    } else {
        Ok(evaluate(0))
    }
}

fn segment(input: f32, start: f32, end: f32) -> f32 {
    let width = end - start;
    if width.abs() > 0.0001 {
        ((input - start) / width).clamp(0.0, 1.0)
    } else {
        f32::from(input >= start)
    }
}

pub(crate) fn gradient(input: Vector, constants: &[Vector]) -> Result<Vector, String> {
    let groups = match constants.len() {
        6 => 1,
        11 => 2,
        _ => return Err("Expression gradient layout is unsupported".into()),
    };
    let bounds = &constants[1 + groups * 4..];
    let knots = bounds.iter().flatten().copied().collect::<Vec<_>>();
    if knots.windows(2).any(|pair| pair[0] > pair[1]) {
        return Err("Expression gradient has unordered bounds".into());
    }
    let mut result = constants[0];
    let weights = (0..groups)
        .map(|group| {
            std::array::from_fn::<_, 4, _>(|lane| {
                let index = group * 4 + lane;
                segment(
                    input[lane],
                    knots[index],
                    knots.get(index + 1).copied().unwrap_or(1.0),
                )
            })
        })
        .collect::<Vec<_>>();
    for channel in 0..4 {
        // Each row is one output channel with four segment coefficients.
        // It is not one segment containing four output channels.
        result[channel] += sum(std::array::from_fn(|lane| {
            let low = constants[1 + channel][lane] * weights[0][lane];
            if groups == 2 {
                low + constants[5 + channel][lane] * weights[1][lane]
            } else {
                low
            }
        }));
    }
    Ok(result)
}
