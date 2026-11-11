//! Channel procedures: their declarations in a source channel bank, the check that a source
//! variable feeds the bank, the bank's internal procedure state and its allocation, and the
//! program translator shared with controllers and resources.
use super::*;
mod dependency;
mod program;
pub(crate) use program::lower as lower_program;
mod state;
pub(super) use state::interpolation_allocation;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Declaration {
    parameters: Vec<u8>,
    parameter_class: Option<u32>,
    pub(super) outputs: Vec<String>,
    code: Vec<u8>,
    constants: Vec<u8>,
    pub(super) interpolation_state: Option<[u8; 16]>,
}

fn indirect(p: &Payload, at: usize, class: u32, stride: usize) -> Result<Vec<u8>> {
    let header = p.pointer(at)?;
    ensure!(p.u32(header + 8)? == class, "procedure array class differs");
    let count = usize::try_from(p.u64(header)?)?;
    ensure!(count <= 256, "procedure table exceeds supported capacity");
    Ok(p.0
        .get(header + 16..header + 16 + count * stride)
        .context("procedure array extent")?
        .to_vec())
}

#[cfg(test)]
fn append_indirect(
    data: &mut Vec<u8>,
    at: usize,
    class: u32,
    rows: &[u8],
    stride: usize,
) -> Result<()> {
    ensure!(
        !rows.is_empty() && rows.len().is_multiple_of(stride),
        "procedure array stride differs"
    );
    let header = (data.len() + 19) & !15;
    data.resize(header - 4, 0);
    data.extend(0x80809FBDu32.to_le_bytes());
    data.extend((rows.len() as u64 / stride as u64).to_le_bytes());
    data.extend((class as u64).to_le_bytes());
    data.extend(rows);
    put(data, at, &(header as i64 - at as i64).to_le_bytes())?;
    let len = data.len() as u64;
    put(data, 0, &len.to_le_bytes())
}

impl Declaration {
    pub(super) fn read(p: &Payload, row: usize, rows: &[usize]) -> Result<Self> {
        let source_code = indirect(p, row + 16, 0x80800009, 1)?;
        ensure!(
            p.u64(row + 8)? == source_code.len() as u64 && p.u64(row + 48)? == 1,
            "channel procedure program extent differs"
        );
        let constants = p
            .array(row + 24, 16, Some(0x80800090))?
            .iter()
            .flat_map(|at| p.0[*at..*at + 16].iter().copied())
            .collect::<Vec<_>>();
        let inputs = usize::try_from(p.u64(row + 40)?)?;
        let code = program::lower(&source_code, constants.len() / 16, inputs)?;
        let mut outputs = vec![];
        let ordinal = rows
            .iter()
            .position(|r| *r == row)
            .context("source procedure ordinal")?;
        for at in p.array(row + 56, 4, Some(0x80800007))? {
            let input = p.u32(at)? as usize;
            let output = *rows.get(input).context("procedure output declaration")?;
            let deps = p.array(output + 80, 4, Some(0x8080000B))?;
            let reciprocal = deps
                .get(ordinal / 32)
                .map(|at| p.u32(*at).map(|word| word & (1u32 << (ordinal % 32)) != 0))
                .transpose()?
                .unwrap_or(false);
            // Constant channels have no writable storage or reverse dependency
            // mask. They remain valid procedure inputs without dirty propagation.
            let computed = dependency::computed(p, rows, input, ordinal)?;
            ensure!(
                (p.u64(output + 8)? == 0 && (reciprocal || constant(p, output, rows)?)) || computed,
                "procedure input {input} for declaration {ordinal} lacks a qualified dependency"
            );
            outputs.push(format!("{:08X}", p.u32(output)?));
        }
        ensure!(
            outputs.len() == inputs,
            "source procedure input count differs"
        );
        // Both runtimes also author this stateful VM procedure without a typed
        // parameter block. A null relative pointer is not a kind-zero block.
        if p.u64(row + 96)? == 0 {
            return Ok(Self {
                parameters: Vec::new(),
                parameter_class: None,
                outputs,
                code,
                constants,
                interpolation_state: None,
            });
        }
        let at = p.pointer(row + 96)?;
        let (source_class, parameter_class, size) = match p.u32(at)? {
            1 => (0x808095BA, 0x808097AF, 16),
            2 => (0x808095B9, 0x808097AE, 16),
            3 => (0x808095B8, 0x808097AD, 24),
            4 => (0x808095B7, 0x808097AC, 32),
            5 => (0x808095B6, 0x808097AB, 16),
            6 => (0x808095B5, 0x808097AA, 16),
            7 => (0x808095B4, 0x808097A9, 24),
            kind => anyhow::bail!("uninspected channel procedure parameter kind {kind}"),
        };
        ensure!(
            p.u32(at.checked_sub(4).context("procedure parameter header")?)? == source_class
                && p.0
                    .get(at + 8..at + 24)
                    .context("procedure reserved parameters")?
                    .iter()
                    .all(|v| *v == 0),
            "source procedure parameter type differs"
        );
        let mut parameters =
            p.0.get(at..at + 8)
                .context("procedure parameter prefix")?
                .to_vec();
        parameters.extend(
            p.0.get(at + 24..at + size + 16)
                .context("procedure parameter extent")?,
        );
        let index = p.u32(at + 4)?;
        let interpolation_state = if index == u32::MAX {
            None
        } else {
            let states = p.array(p.pointer(16)? + 0x60, 16, Some(0x80800090))?;
            Some(
                p.bytes::<16>(
                    *states
                        .get(index as usize)
                        .context("source procedure interpolation state is outside its bank")?,
                )?,
            )
        };
        ensure!(
            (24..size + 16)
                .step_by(4)
                .all(|offset| p.f32(at + offset).is_ok_and(f32::is_finite)),
            "nonfinite procedure parameters"
        );
        Ok(Self {
            parameters,
            parameter_class: Some(parameter_class),
            outputs,
            code,
            constants,
            interpolation_state,
        })
    }

    pub(super) fn write(
        &self,
        data: &mut Vec<u8>,
        row: usize,
        order: &[String],
        state: Option<u32>,
    ) -> Result<()> {
        ensure!(
            state.is_some() == self.interpolation_state.is_some(),
            "procedure interpolation state has not been allocated"
        );
        // Sequencer ordinals stay local. The bank's declaration references
        // below are relocated separately. Parameter blocks omit a reserved float4.
        append_array(data, row + 8, 0x80800009, &self.code, 1)?;
        append_array(data, row + 24, 0x80800090, &self.constants, 16)?;
        let outputs = self
            .outputs
            .iter()
            .map(|h| {
                Ok(u32::try_from(
                    order
                        .iter()
                        .position(|v| v == h)
                        .context("native procedure output")?,
                )?)
            })
            .collect::<Result<Vec<_>>>()?;
        let bytes = outputs
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        append_array(data, row + 56, 0x80800007, &bytes, 4)?;
        let Some(parameter_class) = self.parameter_class else {
            put(data, row + 96, &0u64.to_le_bytes())?;
            return Ok(());
        };
        let at = (data.len() + 7) & !3;
        data.resize(at - 4, 0);
        data.extend(parameter_class.to_le_bytes());
        data.extend(&self.parameters);
        put(data, at + 4, &state.unwrap_or(u32::MAX).to_le_bytes())?;
        put(
            data,
            row + 96,
            &(at as i64 - (row + 96) as i64).to_le_bytes(),
        )?;
        let len = data.len() as u64;
        put(data, 0, &len.to_le_bytes())
    }
}

/// A source declaration with no code, storage, dependency mask, parameters or
/// public name. Nothing writes its cached vector, so it keeps its initial
/// value. Native output refresh recognizes storage `FFFF` and skips zero-code
/// declarations, so the same row shape keeps that value in a native bank.
pub(super) fn constant(p: &Payload, row: usize, rows: &[usize]) -> Result<bool> {
    if p.u64(row + 8)? != 0
        || p.0[row + 16..row + 72].iter().any(|v| *v != 0)
        || p.u16(row + 72)? != u16::MAX
        || p.u16(row + 74)? != 0
        || p.u64(row + 80)? != 0
        || p.u64(row + 96)? != 0
        || p.u16(row + 106)? != u16::MAX
    {
        return Ok(false);
    }
    let definition = p.pointer(24)?;
    ensure!(
        p.u32(definition - 4)? == 0x80809597,
        "constant channel bank type differs"
    );
    let index = rows
        .iter()
        .position(|at| *at == row)
        .context("constant channel ordinal")?;
    for field in [0x168, 0x178, 0x188] {
        for name in p.array(definition + field, 12, Some(0x808095A8))? {
            if p.u32(name + 8)? as usize == index {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub(super) fn validate_source(
    source_assets: &Source,
    bank: &Payload,
    entity: &Payload,
) -> Result<String> {
    let bank_tag = bank.u32(bank.pointer(16)?)?;
    let connections = entity.array(0x18, 56, Some(0x80809A8F))?;
    let providers = connections
        .iter()
        .filter(|at| {
            entity.u32(**at).ok() == Some(bank_tag)
                && entity.u32(**at + 4).ok() == Some(0x808095B0)
                && entity.u32(**at + 28).ok() == Some(0x808098D2)
        })
        .collect::<Vec<_>>();
    if providers.len() != 1 {
        // This inspects the source entity, so every donor reaches the same
        // verdict and the loop should not keep trying.
        return Err(crate::d2_mot::source_limit(anyhow::anyhow!(
            "source bank lacks one variable provider"
        )));
    }
    let variable_tag = entity.u32(*providers[0] + 24)?;
    let variable = format!("{variable_tag:08X}");
    let source = source_assets.raw(&variable)?;
    let instance = source.pointer(16)?;
    let schema = source.pointer(24)?;
    ensure!(
        source.u32(instance - 4)? == 0x80802C62 && source.u32(schema - 4)? == 0x80802C63,
        "source variable component type differs"
    );
    ensure!(
        source.u32(schema + 0x68)? == 0x6E8F4D11 && source.u32(schema + 0xB0)? == 0x6E8F4D11,
        "source variable output name differs"
    );
    let inputs = source.array(instance + 0xA0, 48, Some(0x80809590))?;
    ensure!(inputs.len() == 1, "source variable input count differs");
    let link = source.u64(inputs[0] + 8)? as usize;
    ensure!(
        source.u32(link + 32)? == 0x64590948 && source.u64(link + 8)? == inputs[0] as u64,
        "source variable input differs"
    );
    let runtime = bank.array(bank.pointer(16)? + 0x30, 80, Some(0x808095AF))?;
    let static_inputs = bank.array(bank.pointer(24)? + 0x138, 32, Some(0x808095B0))?;
    ensure!(
        runtime.len() == 1
            && static_inputs.len() == 1
            && bank.u64(runtime[0] + 8)? == static_inputs[0] as u64,
        "source variable resource input differs"
    );
    let rows = bank.array(bank.pointer(24)? + 0x148, 112, Some(0x808095A9))?;
    let controls = rows
        .iter()
        .enumerate()
        .filter(|(_, at)| bank.u32(**at).ok() == Some(0x64590948))
        .collect::<Vec<_>>();
    let aliases = rows
        .iter()
        .filter(|at| bank.u32(**at).ok() == Some(0x6E8F4D11))
        .collect::<Vec<_>>();
    ensure!(
        controls.len() == 1 && aliases.len() == 1,
        "source variable bank declarations differ"
    );
    let control_index = controls[0].0 as u32;
    ensure!(
        connections
            .iter()
            .any(|at| entity.u32(*at).ok() == Some(variable_tag)
                && entity.u32(*at + 4).ok() == Some(0x80809591)
                && entity.u64(*at + 8).ok() == Some(link as u64)
                && entity.u32(*at + 24).ok() == Some(bank_tag)
                && entity.u32(*at + 48).ok() == Some(control_index)),
        "source variable numeric feedback is missing"
    );
    ensure!(
        connections
            .iter()
            .any(|at| entity.u32(*at).ok() == Some(bank_tag)
                && entity.u32(*at + 4).ok() == Some(0x808095B0)
                && entity.u64(*at + 8).ok() == Some(static_inputs[0] as u64)
                && entity.u32(*at + 24).ok() == Some(variable_tag)
                && entity.u32(*at + 28).ok() == Some(0x808098D2)
                && entity.u64(*at + 32).ok() == Some((schema + 0x48) as u64)),
        "source variable resource feedback is missing"
    );
    Ok(variable)
}

fn template_bank(c: &Effect) -> Result<Payload> {
    Ok(Payload(fs::read(
        c.refs.join("native-procedure-bank-02/raw/81532E86.bin"),
    )?))
}

pub(super) fn internal_resources(
    bank: &mut Payload,
    schema: usize,
    instance: usize,
    count: usize,
) -> Result<()> {
    let mut handles = array_bytes(bank, instance + 0x80, 8)?;
    ensure!(
        handles.len() <= count * 8,
        "procedure state capacity shrank"
    );
    handles.resize(count * 8, 0xFF);
    append_array(&mut bank.0, instance + 0x80, 0x8080979D, &handles, 8)?;
    put(
        &mut bank.0,
        schema + 0x13C,
        &u32::try_from(count)?.to_le_bytes(),
    )
}

pub(super) fn allocation(c: &Effect) -> Result<Payload> {
    let tag = template_bank(c)?.u32(0x44)?;
    Ok(Payload(fs::read(c.refs.join(format!(
        "native-procedure-bank-02/raw/{tag:08X}.bin"
    )))?))
}

pub(super) fn internal_allocation(c: &Effect, metadata: &mut Payload, count: usize) -> Result<()> {
    let rows = metadata.array(0x20, 40, Some(0x80808852))?;
    let found = rows
        .iter()
        .copied()
        .filter(|at| metadata.u32(*at).ok() == Some(0xBAFA25AF))
        .collect::<Vec<_>>();
    ensure!(found.len() <= 1, "duplicate procedure-state allocation");
    if let Some(at) = found.first() {
        ensure!(
            metadata.u32(at + 16)? == 0x8080979D,
            "procedure-state allocation class differs"
        );
        return put(
            &mut metadata.0,
            at + 20,
            &u32::try_from(count)?.to_le_bytes(),
        );
    }
    let template = allocation(c)?;
    let entries = template.array(0x20, 40, Some(0x80808852))?;
    let entry = entries
        .into_iter()
        .find(|at| template.u32(*at).ok() == Some(0xBAFA25AF))
        .context("native procedure-state allocation template")?;
    ensure!(
        template.u32(entry + 16)? == 0x8080979D && template.u64(entry + 24)? == 0,
        "native procedure-state allocation layout differs"
    );
    let mut bytes = Vec::new();
    for at in rows {
        ensure!(
            metadata.u64(at + 24)? == 0,
            "channel allocation has nested arrays"
        );
        bytes.extend_from_slice(&metadata.0[at..at + 40]);
    }
    let offset = bytes.len();
    bytes.extend_from_slice(&template.0[entry..entry + 40]);
    put(
        &mut bytes,
        offset + 20,
        &u32::try_from(count)?.to_le_bytes(),
    )?;
    append_array(&mut metadata.0, 0x20, 0x80808852, &bytes, 40)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn procedure(kind: u32, class: u32, size: usize) -> Payload {
        let mut data = vec![0; 0x300];
        let row = 0x80;
        let output = 0x100;
        put(&mut data, row, &0x12345678u32.to_le_bytes()).unwrap();
        put(&mut data, row + 8, &4u32.to_le_bytes()).unwrap();
        put(&mut data, row + 40, &1u64.to_le_bytes()).unwrap();
        put(&mut data, row + 48, &1u64.to_le_bytes()).unwrap();
        put(&mut data, output, &0xABCDEF01u32.to_le_bytes()).unwrap();
        append_indirect(&mut data, row + 16, 0x80800009, &[0x4A, 0, 0x4C, 0], 1).unwrap();
        append_array(&mut data, row + 56, 0x80800007, &1u32.to_le_bytes(), 4).unwrap();
        append_array(&mut data, output + 80, 0x8080000B, &1u32.to_le_bytes(), 4).unwrap();
        put(&mut data, 16, &0x170u64.to_le_bytes()).unwrap();
        append_array(&mut data, 0x1E0, 0x80800090, &[0; 16], 16).unwrap();
        let parameters = data.len() + 4;
        data.extend(class.to_le_bytes());
        data.extend(kind.to_le_bytes());
        data.extend(0u32.to_le_bytes());
        data.extend([0; 16]);
        for i in 0..(size - 8) / 4 {
            data.extend((i as f32 + 0.25).to_le_bytes());
        }
        put(
            &mut data,
            row + 96,
            &(parameters as i64 - (row + 96) as i64).to_le_bytes(),
        )
        .unwrap();
        Payload(data)
    }

    #[test]
    fn unparameterized_procedures_keep_a_null_pointer_and_their_output() {
        let mut source = procedure(2, 0x808095B9, 16);
        put(&mut source.0, 0x80 + 96, &0u64.to_le_bytes()).unwrap();
        let declaration = Declaration::read(&source, 0x80, &[0x80, 0x100]).unwrap();
        assert_eq!(declaration.parameter_class, None);
        let mut native = vec![0; 0x180];
        declaration
            .write(&mut native, 0x80, &["ABCDEF01".into()], None)
            .unwrap();
        let p = Payload(native);
        assert_eq!(p.u64(0x80 + 96).unwrap(), 0);
        assert_eq!(
            indirect(&p, 0x80 + 16, 0x80800009, 1).unwrap(),
            [0x3C, 0, 0x3E, 0]
        );
        let outputs = p.array(0x80 + 56, 4, Some(0x80800007)).unwrap();
        assert_eq!(p.u32(outputs[0]).unwrap(), 0);
    }

    #[test]
    fn internal_procedures_preserve_typed_parameters_and_relocate_dependencies() {
        for (kind, source_class, native_class, size) in [
            (1, 0x808095BA, 0x808097AF, 16),
            (2, 0x808095B9, 0x808097AE, 16),
            (3, 0x808095B8, 0x808097AD, 24),
            (4, 0x808095B7, 0x808097AC, 32),
            (5, 0x808095B6, 0x808097AB, 16),
            (6, 0x808095B5, 0x808097AA, 16),
            (7, 0x808095B4, 0x808097A9, 24),
        ] {
            let source = procedure(kind, source_class, size);
            let declaration = Declaration::read(&source, 0x80, &[0x80, 0x100]).unwrap();
            assert_eq!(declaration.outputs, ["ABCDEF01"]);
            let mut native = vec![0; 0x180];
            declaration
                .write(
                    &mut native,
                    0x80,
                    &["55555555".into(), "12345678".into(), "ABCDEF01".into()],
                    Some(7),
                )
                .unwrap();
            let p = Payload(native);
            let at = p.pointer(0x80 + 96).unwrap();
            assert_eq!(p.u32(at - 4).unwrap(), native_class);
            assert_eq!(p.u32(at).unwrap(), kind);
            assert_eq!(p.u32(at + 4).unwrap(), 7);
            assert_eq!(p.f32(at + 8).unwrap(), 0.25);
            assert_eq!(
                p.f32(at + size - 4).unwrap(),
                ((size - 8) / 4 - 1) as f32 + 0.25
            );
            let outputs = p.array(0x80 + 56, 4, Some(0x80800007)).unwrap();
            assert_eq!(p.u32(outputs[0]).unwrap(), 2);
            assert_eq!(
                indirect(&p, 0x80 + 16, 0x80800009, 1).unwrap(),
                [0x3C, 0, 0x3E, 0]
            );
            let mut invalid = source.clone();
            let at = invalid.pointer(0x80 + 96).unwrap();
            invalid.0[at + 8] = 1;
            assert!(Declaration::read(&invalid, 0x80, &[0x80, 0x100]).is_err());
            invalid.0[at + 8] = 0;
            put(&mut invalid.0, at + 24, &f32::NAN.to_le_bytes()).unwrap();
            assert!(Declaration::read(&invalid, 0x80, &[0x80, 0x100]).is_err());
            invalid.0.truncate(at + 12);
            assert!(Declaration::read(&invalid, 0x80, &[0x80, 0x100]).is_err());
        }
    }

    #[test]
    fn parameter_state_requires_a_valid_source_slot_and_private_destination() {
        let mut source = procedure(5, 0x808095B6, 16);
        let declaration = Declaration::read(&source, 0x80, &[0x80, 0x100]).unwrap();
        assert_eq!(declaration.interpolation_state, Some([0; 16]));
        assert!(
            declaration
                .write(&mut vec![0; 0x180], 0x80, &["ABCDEF01".into()], None)
                .is_err()
        );
        let params = source.pointer(0x80 + 96).unwrap();
        put(&mut source.0, params + 4, &1u32.to_le_bytes()).unwrap();
        assert!(Declaration::read(&source, 0x80, &[0x80, 0x100]).is_err());
        put(&mut source.0, params + 4, &u32::MAX.to_le_bytes()).unwrap();
        assert_eq!(
            Declaration::read(&source, 0x80, &[0x80, 0x100])
                .unwrap()
                .interpolation_state,
            None
        );
    }

    #[test]
    fn internal_state_keeps_existing_handles_and_allocates_each_new_procedure() {
        let mut p = Payload(vec![0; 0x300]);
        append_array(&mut p.0, 0x80, 0x8080979D, &7u64.to_le_bytes(), 8).unwrap();
        internal_resources(&mut p, 0x100, 0, 4).unwrap();
        let slots = p.array(0x80, 8, Some(0x8080979D)).unwrap();
        assert_eq!(slots.len(), 4);
        assert_eq!(p.u64(slots[0]).unwrap(), 7);
        assert!(slots[1..].iter().all(|at| p.u64(*at).unwrap() == u64::MAX));
        assert_eq!(p.u32(0x23C).unwrap(), 4);
        assert!(internal_resources(&mut p, 0x100, 0, 3).is_err());
    }
}
