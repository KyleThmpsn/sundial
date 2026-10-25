//! Translate the source table's angle-independent emissive material profiles.
use super::*;

pub(crate) struct Profile {
    first: u8,
    last: u8,
    bias: f32,
    scale: f32,
    unlit: bool,
}

fn linear(value: u8) -> f32 {
    let value = f32::from(value) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

pub(crate) fn read(source: &Source, library: Option<&Path>) -> Result<Vec<Profile>> {
    let bootstrap = source.read(source.named("client_bootstrap_patchable")?)?;
    let globals = source.read(bootstrap.u32(0x3C)?)?;
    let tag = globals.u32(0x30)?;
    ensure!(
        source.reference(tag)? == 0x80801B84,
        "Source lighting table class differs"
    );
    let lighting = source.read(tag)?;
    let table = source.linear_texture(lighting.u32(8)?, library)?;
    ensure!(
        table.format == 29
            && table.width == 64
            && table.height == 512
            && table.layers == 1
            && table.mips == 1
            && table.bytes.len() == 64 * 512 * 4,
        "Source material response table layout differs"
    );
    let mut profiles: Vec<Profile> = Vec::new();
    // The original deferred consumer treats codes 240..255 as emissive. Each
    // code selects two rows. Preserve only constant rows with unit diffuse
    // color and either enabled or disabled lighting. View-dependent profiles
    // require a separate translation of their view and AO inputs.
    for code in 240..=255u8 {
        let start = usize::from(code) * 2 * 64 * 4;
        let rows = &table.bytes[start..start + 2 * 64 * 4];
        let first = &rows[..4];
        if first[3] != 0
            || !matches!(first[2], 0 | 255)
            || !rows[..64 * 4].chunks_exact(4).all(|pixel| pixel == first)
            || !rows[64 * 4..]
                .chunks_exact(4)
                .all(|pixel| pixel == [0, 0, 0, 255])
        {
            continue;
        }
        let bias = linear(first[0]).powi(2);
        let scale = linear(first[1]).powi(2);
        let unlit = first[2] == 0;
        if let Some(previous) = profiles.last_mut()
            && previous.last.checked_add(1) == Some(code)
            && previous.bias == bias
            && previous.scale == scale
            && previous.unlit == unlit
        {
            previous.last = code;
        } else {
            profiles.push(Profile {
                first: code,
                last: code,
                bias,
                scale,
                unlit,
            });
        }
    }
    Ok(profiles)
}

fn instruction(out: &mut Vec<u32>, opcode: u32, operands: &[&[u32]]) {
    let length = 1 + operands.iter().map(|operand| operand.len()).sum::<usize>();
    out.push(opcode | ((length as u32) << 24));
    for operand in operands {
        out.extend_from_slice(operand);
    }
}

fn write(register: u32, mask: u32) -> [u32; 2] {
    [0x00100002 | (mask << 4), register]
}

fn read_lane(register: u32, lane: u32) -> [u32; 2] {
    [0x00100006 | ((lane * 0x55) << 4), register]
}

fn literal(value: f32) -> [u32; 2] {
    [0x00004001, value.to_bits()]
}

pub(crate) fn append(out: &mut Vec<u32>, first: u32, profiles: &[Profile]) {
    if profiles.is_empty() {
        return;
    }
    let normal = [0x00100E46, first + 1];
    let vector = [0x00100E46, first + 2];
    let roughness = read_lane(first + 2, 3);
    let code = read_lane(first + 1, 3);
    let predicate = read_lane(first + 3, 0);
    let upper = read_lane(first + 3, 1);
    let emission = read_lane(first + 3, 2);
    // The original normal length carries roughness before the lighting pass.
    instruction(
        out,
        0x32,
        &[&write(first + 2, 7), &normal, &literal(2.0), &literal(-1.0)],
    );
    instruction(out, 0x10, &[&write(first + 2, 8), &vector, &vector]);
    instruction(out, 0x4B, &[&write(first + 2, 8), &roughness]);
    instruction(
        out,
        0x32 | (1 << 13),
        &[
            &write(first + 2, 8),
            &roughness,
            &literal(4.0),
            &literal(-3.0),
        ],
    );
    instruction(out, 0x38, &[&write(first + 2, 8), &roughness, &roughness]);
    for profile in profiles {
        let low = (f32::from(profile.first) - 0.5) / 255.0;
        let high = (f32::from(profile.last) + 0.5) / 255.0;
        instruction(out, 0x1D, &[&write(first + 3, 1), &code, &literal(low)]);
        instruction(out, 0x31, &[&write(first + 3, 2), &code, &literal(high)]);
        instruction(out, 1, &[&write(first + 3, 1), &predicate, &upper]);
        instruction(out, 0x1F | (1 << 18), &[&[0x0010000A, first + 3]]);
        instruction(
            out,
            0x32,
            &[
                &write(first + 3, 4),
                &roughness,
                &literal(profile.scale),
                &literal(profile.bias + 1.0 / 128.0),
            ],
        );
        instruction(out, 0x2F, &[&write(first + 3, 4), &emission]);
        // Inverse of native emission: exp2(13*saturate(2*y-257/255)-7)-1/128.
        instruction(
            out,
            0x32,
            &[
                &[0x00102022, 2],
                &emission,
                &literal(1.0 / 26.0),
                &literal((7.0 / 13.0 + 257.0 / 255.0) / 2.0),
            ],
        );
        instruction(
            out,
            0x36,
            &[
                &[0x00102082, 0],
                &literal(if profile.unlit { 1.0 } else { 0.0 }),
            ],
        );
        instruction(out, 0x15, &[]);
    }
}
