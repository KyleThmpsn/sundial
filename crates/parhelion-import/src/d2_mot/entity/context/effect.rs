//! Checked borrowing of the complete default shared effect context.
use super::super::links::Object;
use crate::d2_mot::{native::effects::controller::Relocation, payload::Payload};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

fn root(payload: &Payload, modern: bool) -> Result<(u32, usize, usize)> {
    let (instance, definition, header, prefix, size, ic, dc) = if modern {
        (192, 296, 528, 164, 560, 0x80808158, 0x80808159)
    } else {
        (128, 232, 432, 100, 464, 0x808084CB, 0x808084CC)
    };
    ensure!(
        payload.0.len() == size,
        "effect context has extra or truncated records"
    );
    let (owner, actual_instance, actual_definition) = super::root(payload, ic, dc)?;
    ensure!(
        actual_instance == instance && actual_definition == definition,
        "effect context root layout differs"
    );
    ensure!(
        payload.u64(8)? == if modern { 160 } else { 96 },
        "effect context root envelope differs"
    );
    let spans: &[(usize, usize)] = if modern {
        &[(32, 64), (80, 128), (168, 188)]
    } else {
        &[(32, 48), (104, 124)]
    };
    ensure!(
        spans
            .iter()
            .all(|&(lo, hi)| payload.0[lo..hi].iter().all(|&v| v == 0)),
        "effect context has active unrepresented root fields"
    );
    let field = if modern { 64 } else { 48 };
    ensure!(
        payload.u64(field)? == 1
            && payload.pointer(field + 8)? == header
            && payload.u32(header - 4)? == if modern { 0x80809FB8 } else { 0x80809FBD }
            && payload.u64(header)? == 1
            && payload.u64(header + 8)? == if modern { 0x8080907C } else { 0x808091A4 }
            && payload.pointer(header + 16)? == instance
            && payload.u32(header + 24)? == ic
            && payload.u32(header + 28)? == 0x10000,
        "effect context instance allocation descriptor differs"
    );
    let lifecycle = if modern { 128 } else { 64 };
    ensure!(
        payload.u32(lifecycle)? == u32::MAX
            && payload.u64(lifecycle + 8)? == if modern { 100 } else { 104 }
            && payload.u64(lifecycle + 16)? == if modern { 20 } else { 24 }
            && payload.u64(lifecycle + 24)? == 0x170500080010
            && payload.u32(lifecycle + 32)? == 0
            && payload.u32(prefix)? == if modern { 0x80809AF3 } else { 0x80809C28 },
        "effect context lifecycle envelope differs"
    );
    let nested = if modern { 496 } else { 400 };
    ensure!(
        payload.u32(nested)? == if modern { 0x8080815D } else { 0x808084D0 }
            && payload.u32(nested + 4)? == 0
            && payload.u32(nested + 8)? == 0x811C9DC5
            && payload.u32(nested + 12)? == 0
            && payload.0[nested + 16..header - 4].iter().all(|&v| v == 0),
        "effect context nondefault composite suffix requires translation"
    );
    Ok((owner, instance, definition))
}

fn allocation(payload: &Payload) -> Result<()> {
    ensure!(
        payload.0.len() == 48
            && payload.u64(0)? == 48
            && payload.u64(8)? == 0x811C9DC5
            && payload.u64(16)? == 0
            && payload.u64(24)? == u32::MAX as u64
            && payload.u64(32)? == 0
            && payload.u64(40)? == 0,
        "effect context allocation is not the validated empty form"
    );
    Ok(())
}

fn metadata(
    payload: &Payload,
    modern: bool,
    class: u32,
    instance: u32,
    methods: &[u32],
) -> Result<()> {
    ensure!(
        payload.u64(0)? == payload.0.len() as u64
            && payload.0.len() == 64 + 24 * methods.len()
            && payload.u32(8)? == class
            && payload.u32(12)? == instance
            && payload.u64(16)? == methods.len() as u64
            && payload.pointer(24)? == 48
            && payload.bytes::<12>(32)? == [0; 12]
            && payload.u32(44)? == if modern { 0x80809FB8 } else { 0x80809FBD }
            && payload.u64(48)? == methods.len() as u64
            && payload.u64(56)? == if modern { 0x80809B25 } else { 0x80809C56 },
        "effect context provider metadata envelope differs"
    );
    for (index, method) in methods.iter().enumerate() {
        let at = 64 + index * 24;
        ensure!(
            payload.u32(at)? == if modern { 0x80808158 } else { 0x808084CB }
                && payload.u32(at + 4)? == *method
                && payload.u64(at + 8)? == 0
                && payload.u64(at + 16)? == 0,
            "effect context provider method or arguments differ"
        );
    }
    Ok(())
}

/// Borrow the unchanged Native default context only after complete validation.
/// Allocation payloads must be bound to the roots' +84 and +44 tags by the
/// caller. Native identity and named publication remain assembler obligations.
/// This proves no serialized channel-selection or gameplay contract.
pub fn interfaces(
    source: &Payload,
    native: &Payload,
    source_allocation: &Payload,
    native_allocation: &Payload,
    providers: &BTreeMap<u32, Payload>,
) -> Result<Vec<Relocation>> {
    let (source_owner, si, sd) = root(source, true)?;
    let (native_owner, ni, nd) = root(native, false)?;
    allocation(source_allocation)?;
    allocation(native_allocation)?;
    ensure!(
        source.bytes::<84>(si + 16)? == native.bytes::<84>(ni + 16)?
            && source.bytes::<56>(sd + 16)? == native.bytes::<56>(nd + 16)?,
        "effect context authored base or initialized state differs"
    );
    let object = |owner, class, offset: usize| Object {
        owner,
        class,
        offset: offset as u64,
    };
    let mut objects = vec![
        Relocation {
            source: object(source_owner, 0x80808158, si),
            target: object(native_owner, 0x808084CB, ni),
        },
        Relocation {
            source: object(source_owner, 0x80808159, sd),
            target: object(native_owner, 0x808084CC, nd),
        },
    ];
    let pairs: &[(u32, u32, u32, u32, &[u32])] = &[
        (
            0x8080815B,
            0x8080815A,
            0x808084CE,
            0x808084CD,
            &[4, 5, 6, 7, 8, 9],
        ),
        (0x808098CD, 0x808098CC, 0x80809ADE, 0x80809ADD, &[2]),
        (0x808091A8, 0x808091A7, 0x80809494, 0x80809493, &[3]),
        (0x8080955E, 0x8080955D, 0x8080974B, 0x8080974A, &[10]),
    ];
    for (index, &(sc, sic, nc, nic, methods)) in pairs.iter().enumerate() {
        let sa = sd + 72 + index * 32;
        let na = nd + 72 + index * 24;
        ensure!(
            source.pointer(sa)? == sd
                && native.pointer(na)? == nd
                && source.bytes::<20>(sa + 12)? == [0; 20]
                && native.bytes::<12>(na + 12)? == [0; 12],
            "effect context initialized provider differs"
        );
        metadata(
            providers
                .get(&source.u32(sa + 8)?)
                .context("effect context Source metadata is absent")?,
            true,
            sc,
            sic,
            methods,
        )?;
        metadata(
            providers
                .get(&native.u32(na + 8)?)
                .context("effect context Native metadata is absent")?,
            false,
            nc,
            nic,
            methods,
        )?;
        objects.push(Relocation {
            source: object(source_owner, sc, sa),
            target: object(native_owner, nc, na),
        });
    }
    // Reconstruct every Native byte independently, using Source authored values
    // and the validated Native identities and lifecycle envelope.
    let mut expected = vec![0u8; 464];
    fn word(out: &mut [u8], at: usize, value: u32) {
        out[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn wide(out: &mut [u8], at: usize, value: u64) {
        out[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    fn relative(out: &mut [u8], at: usize, target: usize) {
        out[at..at + 8].copy_from_slice(&((target as i64) - (at as i64)).to_le_bytes());
    }
    wide(&mut expected, 0, 464);
    wide(&mut expected, 8, 96);
    relative(&mut expected, 16, 128);
    relative(&mut expected, 24, 232);
    wide(&mut expected, 48, 1);
    relative(&mut expected, 56, 432);
    word(&mut expected, 64, source.u32(128)?);
    word(&mut expected, 68, native.u32(68)?);
    wide(&mut expected, 72, 104);
    wide(&mut expected, 80, 24);
    wide(&mut expected, 88, source.u64(152)?);
    word(&mut expected, 96, source.u32(160)?);
    word(&mut expected, 100, 0x80809C28);
    word(&mut expected, 124, 0x808084CB);
    word(&mut expected, 128, native_owner);
    word(&mut expected, 132, 0x808084CC);
    wide(&mut expected, 136, 232);
    expected[ni + 16..ni + 100].copy_from_slice(&source.0[si + 16..si + 100]);
    word(&mut expected, 228, 0x808084CC);
    word(&mut expected, 232, native_owner);
    word(&mut expected, 236, 0x808084CB);
    wide(&mut expected, 240, 128);
    expected[nd + 16..nd + 72].copy_from_slice(&source.0[sd + 16..sd + 72]);
    for index in 0..4 {
        let sa = sd + 72 + index * 32;
        let na = nd + 72 + index * 24;
        relative(&mut expected, na, nd);
        word(&mut expected, na + 8, native.u32(na + 8)?);
        expected[na + 12..na + 24].copy_from_slice(&source.0[sa + 12..sa + 24]);
    }
    word(&mut expected, 400, 0x808084D0);
    expected[404..416].copy_from_slice(&source.0[500..512]);
    word(&mut expected, 428, 0x80809FBD);
    wide(&mut expected, 432, 1);
    wide(&mut expected, 440, 0x808091A4);
    relative(&mut expected, 448, 128);
    word(&mut expected, 456, 0x808084CB);
    word(&mut expected, 460, source.u32(556)?);
    ensure!(
        expected == native.0,
        "effect context complete Native counterpart differs"
    );
    Ok(objects)
}
