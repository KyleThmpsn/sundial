use super::{Sequence, controls::*, events};
use crate::d2_mot::{
    entity::links::Object, native::effects::controller::Relocation, payload::Payload,
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub struct Resource {
    pub source_class: u32,
    pub native_class: u32,
    pub tag: u32,
}

/// Resource keys are validated source field offsets, including resolved 64-bit
/// references. Compiled identities are separate from package-tag dependencies.
#[derive(Default)]
pub struct Bindings {
    pub resources: BTreeMap<usize, Resource>,
    pub identities: BTreeMap<u32, u32>,
}

pub struct Native {
    pub owner: Payload,
    pub allocation: Payload,
    pub objects: Vec<Relocation>,
    /// The modern bank invalidation input has no native sequence field. The
    /// linker must prove its provider is the bank used by these scalar inputs.
    /// Native conditions pull those inputs at evaluation time.
    pub pull_dependencies: Vec<Object>,
}

fn copy_array(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    sc: u32,
    nc: u32,
    stride: usize,
) -> Result<usize> {
    let rows = source.array(from, stride, Some(sc))?;
    let mut bytes = Vec::with_capacity(rows.len() * stride);
    for &at in &rows {
        bytes.extend_from_slice(
            source
                .0
                .get(at..at + stride)
                .context("sequence array extent")?,
        );
    }
    array(output, to, nc, &bytes, stride)?;
    Ok(rows.len())
}

#[expect(
    clippy::too_many_arguments,
    reason = "Native conversion keeps independent payload, identity and dispatch contracts explicit"
)]
pub(crate) fn inputs(
    source: &Payload,
    template: &Payload,
    output: &mut Payload,
    sd: usize,
    nd: usize,
    sp: usize,
    np: usize,
    ri: usize,
    objects: &mut Vec<Relocation>,
) -> Result<usize> {
    let source_rows = source.array(sd, 40, Some(0x80809591))?;
    let source_owner = source.u32(source.pointer(16)?)?;
    let native_owner = output.u32(output.pointer(16)?)?;
    let troot = template.pointer(24)?;
    let trow = *template
        .array(troot + 0x188, 40, Some(0x80809789))?
        .first()
        .context("native sequence lacks scalar input envelope")?;
    let ti = usize::try_from(template.u64(trow + 8)?)?;
    let default = template.bytes::<96>(ti)?;
    ensure!(
        template.u32(trow + 4)? == 0x80809788 && template.u32(ti + 4)? == 0x80809789,
        "native sequence scalar input class differs"
    );
    let ir = array(output, ri, 0x80809788, &vec![0; source_rows.len() * 96], 96)?;
    let dr = array(output, nd, 0x80809789, &vec![0; source_rows.len() * 40], 40)?;
    for (index, &from) in source_rows.iter().enumerate() {
        let si = usize::try_from(source.u64(from + 8)?)?;
        ensure!(
            source.u32(from)? == source_owner
                && source.u32(from + 4)? == 0x80809590
                && source.u32(si)? == source_owner
                && source.u32(si + 4)? == 0x80809591
                && source.u64(si + 8)? == from as u64
                && source.pointer(si + 16)? == sp,
            "source sequence scalar input pair differs"
        );
        ensure!(
            source.u64(from + 16)? == 0
                && source.u64(from + 24)? == 0x808095CE
                && source.u32(from + 36)? == 0
                && source.u64(si + 24)? == 0
                && source.u64(si + 32)? == u32::MAX as u64
                && source.u64(si + 40)? == 0,
            "source sequence scalar input is initialized or unsupported"
        );
        let instance = ir + index * 96;
        let definition = dr + index * 40;
        put(output, instance, &default)?;
        put(output, definition, &template.bytes::<40>(trow)?)?;
        pair(
            output,
            native_owner,
            instance,
            definition,
            0x80809788,
            0x80809789,
        )?;
        relative(output, instance + 16, np)?;
        put(
            output,
            definition + 32,
            &source.u32(from + 32)?.to_le_bytes(),
        )?;
        for (offset, class, target, target_class) in [
            (from, 0x80809591, definition, 0x80809789),
            (si, 0x80809590, instance, 0x80809788),
        ] {
            objects.push(Relocation {
                source: Object {
                    owner: source_owner,
                    class,
                    offset: offset as u64,
                },
                target: Object {
                    owner: native_owner,
                    class: target_class,
                    offset: target as u64,
                },
            });
        }
    }
    Ok(source_rows.len())
}

fn locators(source: &Payload, output: &mut Payload) -> Result<(usize, Allocation)> {
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = output.pointer(16)?;
    let nd = output.pointer(24)?;
    ensure!(
        source.u32(si + 0x264)? == 0x80809531 && source.u32(sd + 0x224)? == 0x80809530,
        "source sequence locator pool differs"
    );
    let mut allocation = Allocation::array(0x846FDE3F, u32::MAX, 0);
    for (from, to, sc, nc, stride, name) in [
        (0x280, 0x170, 0x80809F4F, 0x80809F75, 32, 0x82C28E51),
        (0x290, 0x180, 0x80809F4F, 0x80809F75, 32, 0x4C2E87A1),
        (0x2A0, 0x190, 0x8080953C, 0x80808618, 2, 0x6E385590),
        (0x2B0, 0x1A0, 0x80809AC8, 0x80809BFF, 24, 0xA6E3DD20),
    ] {
        let count = copy_array(source, si + from, output, ni + to, sc, nc, stride)?;
        if count > 0 {
            allocation.children.push(Allocation::array(name, nc, count));
        }
    }
    let counts = [
        copy_array(
            source,
            sd + 0x230,
            output,
            nd + 0x1B0,
            0x8080953B,
            0x80808617,
            6,
        )?,
        copy_array(
            source,
            sd + 0x240,
            output,
            nd + 0x1C0,
            0x80809539,
            0x80808615,
            16,
        )?,
        copy_array(
            source,
            sd + 0x250,
            output,
            nd + 0x1D0,
            0x80809538,
            0x80808614,
            4,
        )?,
    ];
    ensure!(
        counts.iter().all(|n| *n == counts[0]),
        "sequence locator declaration dimensions differ"
    );
    ensure!(
        source.u64(si + 0x290)? == counts[0] as u64
            && source.u64(si + 0x2A0)? == counts[0] as u64
            && source.u64(si + 0x2B0)? == counts[0] as u64,
        "sequence locator runtime dimensions differ"
    );
    put(output, nd + 0x1E0, &source.bytes::<16>(sd + 0x260)?)?;
    Ok((counts[0], allocation))
}

/// Build native control/event instances, declarations and allocation metadata.
/// The caller must link returned objects and discharge pull_dependencies before
/// this component is usable by an entity. No partially linked install is implied.
pub fn emit(
    source: &Payload,
    template: &Payload,
    allocation_template: &Payload,
    bindings: &Bindings,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Native> {
    let sequence = Sequence::read(source)?;
    for payload in [template, allocation_template] {
        ensure!(
            payload.u64(0)? == payload.0.len() as u64,
            "native sequence envelope size differs"
        );
    }
    ensure!(
        (0x80800001..=0x81FFFFFF).contains(&owner_tag)
            && (0x80800001..=0x81FFFFFF).contains(&allocation_tag)
            && owner_tag != allocation_tag,
        "invalid native sequence owner tags"
    );
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = template.pointer(16)?;
    let nd = template.pointer(24)?;
    ensure!(
        template.u32(ni - 4)? == 0x808084D7 && template.u32(nd - 4)? == 0x808084E9,
        "native sequence root classes differ"
    );
    for field in [0x1B8, 0x1E8, 0x1F8] {
        ensure!(
            source.u64(sd + field)? == 0,
            "sequence extension at {field:X} requires translation"
        );
    }
    ensure!(
        source.u64(sd + 0x3A0)? == 0,
        "sequence root predicate requires translation"
    );
    // Native 0058E260 dispatches the packed initial node at definition +2D0.
    // This lowering currently supports the inspected default control entry.
    // Never inherit a template's alternate starting event or control.
    ensure!(
        source.u64(sd + 0x390)? == 0
            && sequence
                .controls
                .first()
                .is_some_and(|control| control.parent.is_none()),
        "sequence initial control requires translation"
    );
    let source_owner = source.u32(si)?;
    let old_owner = template.u32(ni)?;
    let mut output = template.clone();
    // All native envelope self-references remain paired and retain their native
    // method descriptors. Source ordinals are never substituted for native code.
    for at in (0..output.0.len().saturating_sub(15)).step_by(4) {
        if output.u32(at)? == old_owner {
            ensure!(
                output.u32(at + 4)? & 0xFFFF0000 == 0x80800000
                    && output.u64(at + 8)? < output.0.len() as u64,
                "untyped sequence envelope self-reference"
            );
            put(&mut output, at, &owner_tag.to_le_bytes())?;
        }
    }
    put(&mut output, 0x44, &allocation_tag.to_le_bytes())?;
    let mut objects = vec![];
    let main_count = inputs(
        source,
        template,
        &mut output,
        sd + 0x208,
        nd + 0x188,
        si,
        ni,
        ni + 0xD0,
        &mut objects,
    )?;
    let aux_count = inputs(
        source,
        template,
        &mut output,
        sd + 0x380,
        nd + 0x2B8,
        si + 0x1A0,
        ni + 0x200,
        ni + 0x220,
        &mut objects,
    )?;
    let (locator_count, locator_allocation) = locators(source, &mut output)?;
    let presentation_allocation = super::presentation::emit(source, &mut output, bindings)?;
    let input_map = (0..main_count as u32).map(|i| (i, i)).collect();
    let control_allocation =
        super::controls::write(source, &sequence, &mut output, &input_map, main_count)?;
    let event_allocation = events::write(
        source,
        &sequence,
        template,
        &mut output,
        bindings,
        &input_map,
        locator_count,
    )?;
    let mut allocations = vec![control_allocation, event_allocation];
    if main_count > 0 {
        allocations.push(Allocation::array(0x86374C3C, 0x80809788, main_count));
    }
    allocations.push(Allocation::array(
        0xE3EA46ED,
        0x8080000A,
        sequence.controls.len(),
    ));
    allocations.push(Allocation::array(
        0x5C5BD59A,
        0x8080000A,
        sequence.event_count,
    ));
    allocations.push(locator_allocation);
    allocations.extend(presentation_allocation);
    if aux_count > 0 {
        let mut aux = Allocation::array(0xCA95F655, u32::MAX, 0);
        aux.children
            .push(Allocation::array(0xFC3956AD, 0x80809788, aux_count));
        allocations.push(aux);
    }
    // Native root flags consume these same four bytes in 0058FC90. The modern
    // compiled identity before the predicate is not part of this native field.
    put(&mut output, nd + 0x2D0, &0u64.to_le_bytes())?;
    put(&mut output, nd + 0x2D8, &0u64.to_le_bytes())?;
    put(&mut output, nd + 0x2E0, &source.bytes::<8>(sd + 0x3A8)?)?;
    let identity = source.u32(sd + 0x30C)?;
    put(
        &mut output,
        nd + 0x26C,
        &bindings
            .identities
            .get(&identity)
            .with_context(|| format!("untranslated sequence identity {identity:08X}"))?
            .to_le_bytes(),
    )?;
    let mut allocation = Payload(
        allocation_template
            .0
            .get(..48)
            .context("sequence allocation header")?
            .to_vec(),
    );
    Allocation::write(&allocations, &mut allocation, 0x20)?;
    for payload in [&mut output, &mut allocation] {
        let size = payload.0.len() as u64;
        put(payload, 0, &size.to_le_bytes())?;
    }
    let generic = sd + 0x360;
    ensure!(
        source.u32(generic)? == source_owner
            && source.u32(generic + 4)? == 0x80809A9E
            && source.u64(generic + 16)? == 1
            && source.u64(generic + 24)? == 0x808095B2,
        "source sequence pull dependency differs"
    );
    crate::d2_mot::native::effects::controller::seal(&mut output)?;
    Ok(Native {
        owner: output,
        allocation,
        objects,
        pull_dependencies: vec![Object {
            owner: source_owner,
            class: 0x80809A9F,
            offset: generic as u64,
        }],
    })
}
