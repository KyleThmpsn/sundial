//! Ordinary CPU samples retain their birth positions while the emitter moves.
use crate::{
    presentation::put,
    tiger::{particle::Program, payload::Payload},
};
use anyhow::{Result, ensure};

pub(super) fn build(original: &Payload, life: f32, emission: f32, radius: f32) -> Result<Payload> {
    ensure!(
        original.u8(0x14b)? == 0 && original.u8(0x142)? == 8,
        "Contrail samples need the native CPU billboard envelope"
    );
    let mut program = Program::default();
    // The native age, placement and angular consumers own these mutable lanes.
    // Emitter-local origin and zero velocity are transformed once, during spawn.
    for (route, bank, scalar) in [
        (0, 1, 7),
        (2, 5, 1),
        (6, 1, 8),
        (7, 1, 4),
        (8, 2, 12),
        (9, 2, 8),
        (23, 0, 3),
        (27, 1, 11),
        (38, 0, 2),
        (42, 0, 0),
        (43, 0, 1),
    ] {
        program.route(route, bank, scalar)?;
    }
    for route in [11, 12, 13, 14, 15, 16, 18, 19, 20, 21, 22, 24, 25, 26] {
        program.default_value(route, [0.; 4])?;
    }
    program.default_value(5, [life; 4])?;
    program.default_value(10, [emission; 4])?;
    program.default_value(17, [(emission * life).ceil() + 2.; 4])?;
    // Native preparation has already published birth time and ordinal. Initialize
    // only the angular lane, then the complete appearance records consumed by draw.
    program.constant(6, [0.; 4])?;
    program.code(6, &[0x3f, 0, 0, 7])?;
    for row in 0..8 {
        program.constant(3, [0.; 4])?;
        program.code(3, &[0x3f, 1, row, 0])?;
    }
    program.constant(3, [radius, radius, 0., 0.])?;
    program.code(3, &[0x3f, 1, 3, 0])?;
    let mut payload = program.write(original, 1, 8)?;
    payload.0[0x115] = 0;
    payload.0[0x116] = 0;
    // Linear historical placement spreads a frame's births between marker samples.
    // Neither frame update nor section five rewrites their positions afterwards.
    let flags = (payload.u32(0x44)? & !(1 << 10)) | (1 << 9);
    put(&mut payload.0, 0x44, &flags.to_le_bytes())?;
    put(&mut payload.0, 0x120, &(life + 0.05).to_le_bytes())?;
    Ok(payload)
}
