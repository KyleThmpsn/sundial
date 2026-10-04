//! Arithmetic shared by the native Shadowkeep particle and material dialects.
//! Their dispatches agree in independent machine-code captures for this prefix.
//! Binding opcodes and format validation remain in their owning interpreters.
pub(crate) mod math;

/// A two-operand operation. `right` is the most recently pushed value.
pub(crate) fn binary(opcode: u8, left: [f32; 4], right: [f32; 4]) -> [f32; 4] {
    match opcode {
        0x01 | 0x06 => std::array::from_fn(|i| left[i] + right[i]),
        0x02 => std::array::from_fn(|i| left[i] - right[i]),
        0x03 | 0x05 => std::array::from_fn(|i| left[i] * right[i]),
        0x04 => std::array::from_fn(|i| divide(left[i], right[i])),
        0x08 => std::array::from_fn(|i| left[i].min(right[i])),
        0x09 => std::array::from_fn(|i| left[i].max(right[i])),
        // Native comparison inverts right < left, retaining equality. Inputs are finite.
        0x0A => std::array::from_fn(|i| f32::from(left[i] <= right[i])),
        0x0B => [math::sum(std::array::from_fn(|i| left[i] * right[i])); 4],
        0x0C => [left[0], right[0], right[1], right[2]],
        0x0D => [left[0], left[1], right[0], right[1]],
        0x0E => [left[0], left[1], left[2], right[0]],
        // The four coefficient lanes encode one cubic, evaluated at each input lane's time.
        0x0F => left.map(|time| polynomial(right, time)),
        _ => unreachable!(),
    }
}

/// A three-operand operation, its operands in the order they were pushed.
pub(crate) fn ternary(opcode: u8, a: [f32; 4], b: [f32; 4], c: [f32; 4]) -> [f32; 4] {
    match opcode {
        // The native interpreter consumes start, destination, then weight.
        0x10 | 0x11 => std::array::from_fn(|i| {
            let value = a[i] + c[i] * (b[i] - a[i]);
            if opcode == 0x11 {
                value.clamp(0.0, 1.0)
            } else {
                value
            }
        }),
        0x12 => std::array::from_fn(|i| a[i] * b[i] + c[i]),
        0x13 => std::array::from_fn(|i| a[i].max(b[i]).min(c[i])),
        0x14 => std::array::from_fn(|i| {
            let width = a[i] - b[i];
            let weight = if width.abs() > 0.0001 {
                ((c[i] - b[i]) / width).clamp(0.0, 1.0)
            } else {
                1.0
            };
            (3.0 - 2.0 * weight) * (weight * weight)
        }),
        _ => unreachable!(),
    }
}

/// A one-operand operation.
pub(crate) fn unary(opcode: u8, value: [f32; 4]) -> [f32; 4] {
    match opcode {
        0x07 => value.map(|v| u8::from(v == 0.0) as f32),
        0x15 => value.map(f32::abs),
        0x16 => value.map(|v| if v == 0.0 { v } else { v.signum() }),
        0x17 => value.map(f32::floor),
        0x18 => value.map(f32::ceil),
        0x19 => value.map(f32::round_ties_even),
        0x1A => value.map(|v| v - v.floor()),
        0x1B => math::normalize(value, false),
        0x1C => math::normalize(value, true),
        0x1D => value.map(|v| -v),
        0x1E => value.map(math::sine),
        0x1F => value.map(|v| math::sine(v + 0.25)),
        0x20 => [
            math::sine(value[0]),
            math::sine(value[0] + 0.25),
            math::sine(value[1]),
            math::sine(value[1] + 0.25),
        ],
        0x21 => [value[0]; 4],
        0x23 => value.map(|v| v.clamp(0.0, 1.0)),
        0x24 => value.map(math::exp2),
        0x25 => value.map(math::log2),
        0x26 => [math::length(value); 4],
        0x27 => value.map(|v| (v - v.round_ties_even()).abs() * 2.0),
        0x28 => [math::jitter(value[0]); 4],
        0x29 => [math::wander(value[0]); 4],
        0x2A => [math::noise(value[0]); 4],
        0x2B => [math::smooth_noise(value[0]); 4],
        _ => unreachable!(),
    }
}

fn polynomial(coefficients: [f32; 4], time: f32) -> f32 {
    (coefficients[0] * time + coefficients[1]) * (time * time)
        + (coefficients[2] * time + coefficients[3])
}

// The native denominator threshold intentionally produces a nonfinite result. Each
// interpreter rejects that result before committing any output.
fn divide(numerator: f32, denominator: f32) -> f32 {
    if denominator.abs() > 1e-19 {
        numerator / denominator
    } else if numerator == 0.0 {
        f32::NAN
    } else {
        f32::INFINITY.copysign(numerator)
    }
}
