//! Embedded numeric expressions and their graph-linked scalar inputs.
use super::*;
use crate::d2_mot::entity::sequence::scalar_inputs;

pub struct Converted {
    pub objects: Vec<Relocation>,
    /// The containing controller must include these in its allocation tree.
    pub extra_inputs: usize,
    pub inputs: usize,
    pub source_code: Vec<u8>,
    pub native_code: Vec<u8>,
    pub constants: Vec<u8>,
}

#[derive(Clone, Copy)]
enum Kind {
    Scalar,
    Reference,
}

impl Kind {
    fn classes(self, modern: bool) -> (u32, u32) {
        match (self, modern) {
            (Self::Scalar, true) => (0x80808631, 0x80808632),
            (Self::Scalar, false) => (0x808089F7, 0x808089F8),
            (Self::Reference, true) => (0x8080862F, 0x80808630),
            (Self::Reference, false) => (0x808089F5, 0x808089F6),
        }
    }
}

/// Parent locations are supplied by the containing controller converter. Nested
/// expressions must retain their enclosing runtime object, not the owner root.
pub struct Contract {
    pub source_parent: usize,
    pub native_parent: usize,
    pub builtins: usize,
}

fn instance(
    p: &Payload,
    definition: usize,
    parent: usize,
    kind: Kind,
    modern: bool,
) -> Result<usize> {
    let i = usize::try_from(p.u64(definition + 8)?)?;
    let owner = p.u32(p.pointer(16)?)?;
    let (ic, dc) = kind.classes(modern);
    let parent_definition = usize::try_from(p.u64(parent + 8)?)?;
    ensure!(
        p.u32(definition)? == owner
            && p.u32(definition + 4)? == ic
            && p.u32(i)? == owner
            && p.u32(i + 4)? == dc
            && p.u64(i + 8)? == definition as u64
            && p.pointer(i + 16)? == parent
            && p.u32(parent)? == owner
            && p.u32(parent_definition)? == owner
            && p.u64(parent_definition + 8)? == parent as u64
            && p.u64(i + 24)? == 0,
        "embedded scalar pair or parent differs"
    );
    Ok(i)
}

/// Replace one scalar expression inside an already allocated native owner.
/// `builtins` is the input contract established by the containing controller's
/// converter, not a guessed count derived from bytecode. Both envelopes must
/// agree after excluding their declared extra inputs. The caller seals the copy
/// span and builds its allocation metadata after every embedded edit is done.
pub fn emit(
    source: &Payload,
    output: &mut Payload,
    input_envelope: &Payload,
    from: usize,
    to: usize,
    builtins: usize,
) -> Result<Converted> {
    emit_nested(
        source,
        output,
        input_envelope,
        from,
        to,
        Contract {
            source_parent: source.pointer(16)?,
            native_parent: output.pointer(16)?,
            builtins,
        },
    )
}

/// Translate a scalar expression, including the reference-bearing derived
/// class used inside projectile movement controllers. Active reference fields
/// require separate entity linkage and are rejected here. Existing native
/// input arrays are validated and replaced, never retained as source behavior.
pub fn emit_nested(
    source: &Payload,
    output: &mut Payload,
    input_envelope: &Payload,
    from: usize,
    to: usize,
    contract: Contract,
) -> Result<Converted> {
    let kind = match source.u32(from + 4)? {
        0x80808631 => Kind::Scalar,
        0x8080862F => Kind::Reference,
        class => anyhow::bail!("unsupported embedded expression instance {class:08X}"),
    };
    let si = instance(source, from, contract.source_parent, kind, true)?;
    let ni = instance(output, to, contract.native_parent, kind, false)?;
    if matches!(kind, Kind::Reference) {
        ensure!(
            source.u64(from + 80)? == 0
                && source.u64(from + 88)? == 0x811C9DC5
                && source.bytes::<16>(from + 80)? == output.bytes::<16>(to + 80)?,
            "embedded expression reference requires entity linkage"
        );
    }
    let declarations = source.array(from + 64, 40, Some(0x80809591))?;
    let runtime = source.array(si + 32, 48, Some(0x80809590))?;
    ensure!(
        declarations.len() == runtime.len(),
        "scalar input pair count differs"
    );
    for (&d, &i) in declarations.iter().zip(&runtime) {
        ensure!(
            source.u64(d + 8)? == i as u64,
            "scalar input runtime order differs"
        );
    }
    let native_declarations = output.array(to + 64, 40, Some(0x80809789))?;
    let native_runtime = output.array(ni + 32, 96, Some(0x80809788))?;
    ensure!(
        native_declarations.len() == native_runtime.len(),
        "native scalar input pair count differs"
    );
    let native_owner = output.u32(ni)?;
    for (&d, &i) in native_declarations.iter().zip(&native_runtime) {
        ensure!(
            output.u32(d)? == native_owner
                && output.u32(i)? == native_owner
                && output.u32(d + 4)? == 0x80809788
                && output.u32(i + 4)? == 0x80809789
                && output.u64(d + 8)? == i as u64
                && output.u64(i + 8)? == d as u64
                && output.pointer(i + 16)? == ni,
            "native scalar input pair or parent differs"
        );
    }
    let builtins = contract.builtins;
    let inputs = builtins
        .checked_add(declarations.len())
        .context("scalar input count overflow")?;
    ensure!(
        builtins <= 64
            && inputs <= 64
            && source.u64(from + 48)? == inputs as u64
            && source.u64(from + 56)? == 1
            && output.u64(to + 48)? == (builtins + native_declarations.len()) as u64
            && output.u64(to + 56)? == 1
            && native_declarations.len() <= 64 - builtins,
        "embedded scalar input or output contract differs"
    );
    source.array(from + 16, 1, Some(0x80800009))?;
    let constants_rows = source.array(from + 32, 16, Some(0x80800090))?;
    let source_code = array_bytes(source, from + 16, 1)?;
    let mut constants = array_bytes(source, from + 32, 16)?;
    let native_code = procedural::lower_program(&source_code, constants_rows.len(), inputs)?;
    super::constants::broadcast(&source_code, &mut constants);
    let mut candidate = output.clone();
    let mut objects = Vec::new();
    let extra_inputs = scalar_inputs(
        source,
        input_envelope,
        &mut candidate,
        from + 64,
        to + 64,
        si,
        ni,
        ni + 32,
        &mut objects,
    )?;
    append_array(&mut candidate.0, to + 16, 0x80800009, &native_code, 1)?;
    append_array(&mut candidate.0, to + 32, 0x80800090, &constants, 16)?;
    put(&mut candidate.0, to + 48, &(inputs as u64).to_le_bytes())?;
    let source_owner = source.u32(from)?;
    let target_owner = candidate.u32(to)?;
    let (source_instance, source_definition) = kind.classes(true);
    let (native_instance, native_definition) = kind.classes(false);
    for (offset, class, target, target_class) in [
        (from, source_definition, to, native_definition),
        (si, source_instance, ni, native_instance),
    ] {
        objects.push(Relocation {
            source: Object {
                owner: source_owner,
                class,
                offset: offset as u64,
            },
            target: Object {
                owner: target_owner,
                class: target_class,
                offset: target as u64,
            },
        });
    }
    *output = candidate;
    Ok(Converted {
        objects,
        extra_inputs,
        inputs,
        source_code,
        native_code,
        constants,
    })
}
