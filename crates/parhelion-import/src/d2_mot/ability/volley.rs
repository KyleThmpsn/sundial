//! Numeric profiles shared by the modern and native projectile-volley controller families.
//!
//! This is a partial adaptation into an existing native controller. Its energy, event helpers,
//! animation and shared projectile stay native. Modern per-window projectile overrides have
//! no corresponding field in the native firing consumer and are reported rather than copied.
use crate::{
    d2_mot::native::effects::controller::scalar, presentation::put, tiger::payload::Payload,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};

/// Corresponding embedded expressions, identified by the same reflected member in both builds.
pub const SCALARS: [(usize, usize); 7] = [
    (0x8F8, 0x880),
    (0x958, 0x8E0),
    (0xB50, 0xAD0),
    (0xBB0, 0xB30),
    (0xC10, 0xB90),
    (0xD08, 0xC88),
    (0xD68, 0xCE8),
];

pub struct Profile {
    pub owner: Payload,
    pub evidence: Value,
}

fn root(p: &Payload, classes: [u32; 2]) -> Result<(usize, usize)> {
    let instance = p.pointer(16)?;
    let definition = p.pointer(24)?;
    let owner = p.u32(instance)?;
    ensure!(
        p.u64(0)? == p.0.len() as u64
            && instance >= 4
            && definition >= 4
            && p.u32(instance - 4)? == classes[0]
            && p.u32(definition - 4)? == classes[1]
            && p.u32(definition)? == owner
            && p.u32(instance + 4)? == classes[1]
            && p.u32(definition + 4)? == classes[0]
            && p.u64(instance + 8)? == definition as u64
            && p.u64(definition + 8)? == instance as u64,
        "Volley controller has another root pair"
    );
    Ok((instance, definition))
}

/// Lower only builtin-only programs and the common launch-window prefix. The enclosing graph
/// retains its native registrations and repeated component occurrences. This does not convert
/// the modern controller, its projectile graphs or the meaning of its incoming builtin values.
pub fn adapt_profile(source: &Payload, native: &Payload) -> Result<Profile> {
    let (_, sd) = root(source, [0x808034A1, 0x8080360D])?;
    let (_, nd) = root(native, [0x80804198, 0x808042BF])?;
    let source_rows = source.array(sd + 0xC78, 0x50, Some(0x808034A5))?;
    let native_rows = native.array(nd + 0xBF8, 0x40, Some(0x80802D56))?;
    ensure!(
        !source_rows.is_empty()
            && source_rows.len() <= 32
            && source_rows.len() == native_rows.len(),
        "Volley windows need matching nonempty native storage"
    );
    let mut output = native.clone();
    let mut programs = Vec::new();
    for (from, to) in SCALARS {
        ensure!(
            source.u64(sd + from + 64)? == 0 && native.u64(nd + to + 64)? == 0,
            "Volley profile needs an external numeric provider"
        );
        let converted = scalar::emit(source, &mut output, native, sd + from, nd + to, 1)?;
        ensure!(
            converted.extra_inputs == 0,
            "Volley profile added external inputs"
        );
        ensure!(
            converted.constants.chunks_exact(4).all(|bytes| {
                f32::from_le_bytes(bytes.try_into().expect("Four-byte lane")).is_finite()
            }),
            "Volley profile has non-finite constants"
        );
        programs.push(json!({"source_field":from,"native_field":to,
            "source_code":hex::encode(converted.source_code),
            "native_code":hex::encode(converted.native_code),
            "constants":hex::encode(converted.constants),"builtin_inputs":1}));
    }
    let mut windows = Vec::new();
    for (from, to) in source_rows.into_iter().zip(native_rows) {
        // +38 begins the source's 24-byte reference view. The native row ends at +40 and
        // supplies no per-window projectile override. Preserve its reserved native tail.
        ensure!(
            source.u64(from + 0x38)? == 0,
            "Volley window has an authored reference path"
        );
        let prefix = source.bytes::<0x38>(from)?;
        ensure!(
            prefix.chunks_exact(4).all(|bytes| {
                f32::from_le_bytes(bytes.try_into().expect("Four-byte lane")).is_finite()
            }),
            "Volley window has non-finite launch values"
        );
        put(&mut output.0, to, &prefix)?;
        windows.push(json!({"source_offset":from,"native_offset":to,
            "prefix":hex::encode(prefix),
            "unmapped_projectile":hex::encode(source.bytes::<16>(from + 0x40)?)}));
    }
    let size = output.0.len() as u64;
    put(&mut output.0, 0, &size.to_le_bytes())?;
    Ok(Profile {
        owner: output,
        evidence: json!({"programs":programs,"windows":windows,
            "projectile_selection":"native_shared_reference",
            "energy":"native","event_helpers":"native","animations":"native",
            "builtin_producer_equivalence_verified":false,"gameplay_verified":false}),
    })
}
