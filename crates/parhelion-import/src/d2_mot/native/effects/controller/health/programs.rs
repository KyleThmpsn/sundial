//! Health section expressions and their external channel inputs.
use super::*;

pub(super) struct Programs {
    pub count: usize,
    pub objects: Vec<Relocation>,
    pub allocation: Vec<allocation::Row>,
}

pub(super) fn emit(
    source: &Payload,
    output: &mut Payload,
    input_envelope: &Payload,
) -> Result<Programs> {
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = output.pointer(16)?;
    let nd = output.pointer(24)?;
    let definitions = source.array(sd + 0x400, 80, Some(0x80808632))?;
    let instances = source.array(si + 0x4C8, 48, Some(0x80808631))?;
    ensure!(
        definitions.len() == instances.len(),
        "health expression pair count differs"
    );
    // Health expressions have one built-in numeric input. This converter admits
    // only programs that do not read it until its cross-version value is qualified.
    // Named external inputs retain their indices beginning at one.
    for (&d, &i) in definitions.iter().zip(&instances) {
        ensure!(
            source.u64(d + 8)? == i as u64,
            "health expression runtime order differs"
        );
        let code = array_bytes(source, d + 16, 1)?;
        // A bounded bytecode family avoids interpreting immediate bytes as opcodes.
        ensure!(
            matches!(
                code.as_slice(),
                [0x42, _, 0x4C, 0] | [0x4A, 1..=63, 0x4C, 0]
            ),
            "health expression needs a qualified built-in input or broader program translation"
        );
    }
    let mut candidate = output.clone();
    append_array(
        &mut candidate.0,
        nd + 0x358,
        0x808089F8,
        &vec![0; definitions.len() * 80],
        80,
    )?;
    append_array(
        &mut candidate.0,
        ni + 0x358,
        0x808089F7,
        &vec![0; definitions.len() * 48],
        48,
    )?;
    let targets = candidate.array(nd + 0x358, 80, Some(0x808089F8))?;
    let runtimes = candidate.array(ni + 0x358, 48, Some(0x808089F7))?;
    let mut objects = Vec::new();
    let mut rows = Vec::new();
    for ((&from, &to), &runtime) in definitions.iter().zip(&targets).zip(&runtimes) {
        pair(&mut candidate, runtime, to, 0x808089F7, 0x808089F8, ni)?;
        put(&mut candidate.0, to + 48, &1u64.to_le_bytes())?;
        put(&mut candidate.0, to + 56, &1u64.to_le_bytes())?;
        let converted = scalar::emit(source, &mut candidate, input_envelope, from, to, 1)?;
        objects.extend(converted.objects);
        rows.push(allocation::Row::inputs(converted.extra_inputs));
    }
    *output = candidate;
    Ok(Programs {
        count: definitions.len(),
        objects,
        allocation: allocation::Row::elements(rows),
    })
}
