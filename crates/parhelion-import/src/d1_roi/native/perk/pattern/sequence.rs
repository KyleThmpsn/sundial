//! A standalone HUD sequence owns its scheduler privately and shares only an equal immutable resource.
use super::*;
use crate::d1_roi::{
    lifecycle,
    sequence::{
        Topology,
        hud::{Hud, Keys},
    },
};
pub(super) mod audio;

// Definition offsets, interface pairs, instance-interface pairs and method ordinals.
// Root sequence methods shift by one. The embedded callback keeps methods zero/one.
type Provider = (
    usize,
    usize,
    u32,
    u32,
    u32,
    u32,
    &'static [u32],
    &'static [u32],
);
const PROVIDERS: [Provider; 12] = [
    (
        0x60,
        0x48,
        0x80800581,
        0x80809ADE,
        0x808008FE,
        0x80809ADD,
        &[2],
        &[1],
    ),
    (
        0x78,
        0x60,
        0x808007E9,
        0x808097BB,
        0x808004CD,
        0x808097BA,
        &[6],
        &[5],
    ),
    (
        0x90,
        0x78,
        0x80800935,
        0x80809ADA,
        0x808005D5,
        0x80809AD9,
        &[7],
        &[6],
    ),
    (
        0xA8,
        0x90,
        0x80800988,
        0x808092F8,
        0x808003E3,
        0x808092F7,
        &[4],
        &[3],
    ),
    (
        0xC0,
        0xA8,
        0x808004D1,
        0x8080972E,
        0x808007B1,
        0x8080972D,
        &[8],
        &[7],
    ),
    (
        0xD8,
        0xC0,
        0x80800797,
        0x80809ADC,
        0x80800481,
        0x80809ADB,
        &[11, 11],
        &[10, 10],
    ),
    (
        0xF0,
        0xD8,
        0x80801A97,
        0x80807319,
        0x80801BDF,
        0x80807318,
        &[5],
        &[4],
    ),
    (
        0x108,
        0xF0,
        0x8080063F,
        0x80809497,
        0x80800753,
        0x80809496,
        &[12],
        &[11],
    ),
    (
        0x120,
        0x108,
        0x80800813,
        0x8080974B,
        0x808009A1,
        0x8080974A,
        &[9],
        &[8],
    ),
    (
        0x140,
        0x128,
        0x80800813,
        0x8080974B,
        0x808009A1,
        0x8080974A,
        &[10],
        &[9],
    ),
    (
        0x278,
        0x280,
        0x80801C05,
        0x808072CD,
        0x80801AC3,
        0x808072CC,
        &[0, 1],
        &[0, 1],
    ),
    (
        0x290,
        0x298,
        0x80800581,
        0x80809ADE,
        0x808008FE,
        0x80809ADD,
        &[3],
        &[2],
    ),
];

#[allow(clippy::cognitive_complexity)]
fn providers(
    source: &Source,
    native: &mut Reader,
    graph: &Payload,
    p: &Payload,
    from: bool,
    ordinal: usize,
    exclusion: u32,
) -> Result<()> {
    let d = p.pointer(24)?;
    let i = p.pointer(16)?;
    let tag = p.u32(i)?;
    let exports = graph.array(if from { 0x68 } else { 0x58 }, 40, None)?;
    let references = graph.array(if from { 0x78 } else { 0x68 }, 24, None)?;
    ensure!(
        references.len() == exports.len(),
        "Sequence provider references differ"
    );
    let mut seen = std::collections::BTreeSet::new();
    for (export, reference) in exports.into_iter().zip(references) {
        if graph.u32(export + 16)? != tag {
            continue;
        }
        let at = usize::try_from(graph.u64(export + 24)?)?;
        let offset = at
            .checked_sub(d)
            .context("HUD export precedes its definition")?;
        ensure!(
            seen.insert(offset),
            "HUD sequence repeats a provider export"
        );
        let &(sf, _nf, si, ni, sc, nc, sm, nm) = PROVIDERS
            .iter()
            .find(|row| offset == if from { row.0 } else { row.1 })
            .context("HUD sequence exports an unqualified provider")?;
        let embedded = sf >= 0x278;
        let implementation = match (from, embedded) {
            (true, false) => 0x8080079A,
            (true, true) => 0x80801C3B,
            (false, false) => 0x808084D7,
            (false, true) => 0x808072CB,
        };
        let target = if embedded {
            d + if from { 0x260 } else { 0x270 }
        } else {
            d
        };
        let live = if embedded {
            i + if from { 0x1F8 } else { 0x200 }
        } else {
            i
        };
        let selector = match sf {
            0x60 => 0xFFFFFFA3,
            0x78 => 0xFFFFFFF6,
            _ => 0,
        };
        ensure!(
            p.pointer(at)? == target
                && p.u32(at + 12)? == 0
                && p.u64(at + 16)? == u64::from(selector as u16)
                && graph.u32(export)? == if sf == 0x78 { exclusion << 16 } else { 0 }
                && graph.u32(export + 4)? as usize == live - i
                && graph.u32(export + 8)? == selector
                && graph.u32(export + 12)? == implementation
                && graph.u32(export + 16)? == tag
                && graph.u32(export + 20)? == if from { 0 } else { 0x80809C50 }
                && graph.u64(export + 32)? == u64::from(p.u32(at + 8)?)
                && graph.u32(reference)? == tag
                && graph.u32(reference + 4)? == if from { 0 } else { implementation }
                && graph.u64(reference + 8)? == live as u64
                && graph.u64(reference + 16)? == ordinal as u64,
            "HUD sequence provider binding differs at {offset:X}"
        );
        let metadata = if from {
            source.read(p.u32(at + 8)?)?
        } else {
            (*native.tag(p.u32(at + 8)?, None)?).clone()
        };
        ensure!(
            metadata.u32(8)? == if from { si } else { ni }
                && metadata.u32(12)? == if from { sc } else { nc },
            "HUD sequence provider interface differs"
        );
        let methods = metadata.array(16, 24, Some(if from { 0x808005C0 } else { 0x80809C56 }))?;
        let expected = if from { sm } else { nm };
        ensure!(
            methods.len() == expected.len(),
            "HUD sequence provider method count differs"
        );
        for (row, method) in methods.into_iter().zip(expected) {
            ensure!(
                metadata.u32(row)? == implementation
                    && metadata.u32(row + 4)? == *method
                    && metadata.bytes::<16>(row + 8)? == [0; 16],
                "HUD sequence provider method differs"
            );
        }
    }
    ensure!(
        seen.len() == PROVIDERS.len(),
        "Incomplete sequence providers"
    );
    Ok(())
}

pub(super) fn read(
    source: &Source,
    native: &mut Reader,
    graph: &Payload,
) -> Result<(u32, Vec<NativeAssetResourcePatch>)> {
    let owners = graph.array(32, 12, Some(0x80800715))?;
    ensure!(
        owners.len() == 1
            && graph.bytes::<32>(48)? == [0; 32]
            && graph.array(0x68, 40, Some(0x80800370))?.len() == PROVIDERS.len(),
        "HUD sequence has companions or connections"
    );
    let source_tag = graph.u32(owners[0])?;
    let p = source.read(source_tag)?;
    let (i, s) = (p.pointer(16)?, p.pointer(24)?);
    status::pair(&p, source_tag, i, s, 0x8080079A, 0x80800610)?;
    let tree = Topology::read(&p)?;
    ensure!(
        tree.control_count() == 1 && tree.events().len() == 1 && tree.input_count() == 0,
        "HUD sequence needs a standalone single-event tree"
    );
    for (field, value) in [
        (0x18, 0),
        (0x20, u32::MAX as u64),
        (0x28, 0),
        (0x30, u32::MAX as u64),
        (0x38, u32::MAX as u64),
        (0x40, 0),
        (0x48, u32::MAX as u64),
        (0x50, 0),
        (0x58, 0),
        (0x170, 0),
        (0x178, 0),
        (0x2C0, 0x100000000),
        (0x2C8, 0),
        (0x2D0, 0x100FFFF00),
        (0x2D8, 0),
    ] {
        ensure!(
            p.u64(s + field)? == value,
            "HUD sequence root state differs at {field:X}"
        );
    }
    for (start, end) in [(0x190, 0x1E0), (0x1F8, 0x248), (0x2A8, 0x2C0)] {
        ensure!(
            (start..end)
                .step_by(8)
                .all(|at| p.u64(s + at).ok() == Some(0)),
            "HUD sequence has additional inputs, outputs, locators or callback state"
        );
    }
    providers(source, native, graph, &p, true, 0, 1)?;
    let event = tree.events()[0].offset;
    let hud = Hud::read(source, &p, event)?;
    let translated = hud.translate("perk-hud", &Keys::read(native)?)?;

    let tag = 0x80BC57BD;
    let target_graph = native.tag(tag, Some(0x80809C0F))?;
    let owners = target_graph.array(16, 12, Some(0x80809C04))?;
    ensure!(
        owners.len() == 1
            && target_graph.bytes::<32>(32)? == [0; 32]
            && target_graph.array(0x58, 40, Some(0x80809C22))?.len() == PROVIDERS.len(),
        "Native HUD sequence has companions or connections"
    );
    let target_tag = target_graph.u32(owners[0])?;
    for (graph, field) in [(graph, 0x88), (target_graph.as_ref(), 0x78)] {
        let masks = graph.array(field, 2, Some(0x80800006))?;
        ensure!(
            masks.len() == 2 && masks.iter().all(|&at| graph.u16(at).ok() == Some(0)),
            "HUD sequence condition state differs"
        );
    }
    let target = native.tag(target_tag, Some(0x80809C36))?;
    let n = target.pointer(24)?;
    ensure!(
        target.u32(n - 4)? == 0x808084E9
            && target.u64(n + 0x2D0)? == 0
            && target.u64(n + 0x2D8)? == 0
            && target.u64(n + 0x2E0)? == 0xFFFF09,
        "Native HUD sequence root state differs"
    );
    // Original constructors read source +2C4 and native flag bit 0. Root completion
    // reads source +2D0 and native bit 1, both clear. Keep the native carrier's other defaults.
    let mut lifecycle_target = (*target).clone();
    lifecycle::sequence(&p, &mut lifecycle_target)?;
    providers(source, native, &target_graph, &target, false, 0, 1)?;
    for field in [
        0x148, 0x178, 0x188, 0x1B0, 0x1C0, 0x1D0, 0x1E0, 0x200, 0x210, 0x220, 0x230, 0x240, 0x2B0,
        0x2C0,
    ] {
        ensure!(
            target.bytes::<16>(n + field)? == [0; 16],
            "Native HUD sequence has additional state"
        );
    }
    let controls = p.array(s + 0x160, 8, Some(0x80800629))?;
    let control = p.pointer(controls[0])?;
    let native_controls = target.array(n + 0x158, 24, Some(0x808093E6))?;
    let native_events = target.array(n + 0x168, 24, Some(0x808093E6))?;
    ensure!(
        native_controls.len() == 1 && native_events.len() == 1,
        "Native HUD sequence has additional nodes"
    );
    let nc = target.pointer(native_controls[0] + 16)?;
    let ne = target.pointer(native_events[0] + 16)?;
    ensure!(
        p.u32(control - 4)? == 0x80800691
            && p.u32(control + 28)? == 1
            && p.bytes::<16>(control + 48)? == [0; 16]
            && target.u32(nc - 4)? == 0x808093D7
            && target.u32(nc + 48)? == 1
            && target.bytes::<16>(nc + 72)? == [0; 16]
            && target.u32(ne - 4)? == 0x80803F24
            && target.u32(ne + 48)? == 13,
        "HUD sequence node kinds or conditions differ"
    );
    for (from, to) in [(control, nc), (event, ne)] {
        ensure!(
            p.bytes::<28>(from)? == target.bytes::<28>(to + 16)?,
            "HUD sequence timing, flags, name or parent differs"
        );
        let live = usize::try_from(target.u64(to + 8)?)?;
        ensure!(
            target.u32(to)? == target_tag
                && target.u32(live)? == target_tag
                && target.u64(live + 8)? == to as u64
                && target.u32(live + 4)? == target.u32(to - 4)?,
            "Native HUD sequence node pair differs"
        );
    }
    let children = p.array(control + 32, 4, Some(0x8080052C))?;
    let native_children = target.array(nc + 56, 4, Some(0x808093FB))?;
    ensure!(
        children.len() == 1
            && native_children.len() == 1
            && p.u32(children[0])? == 2
            && target.u32(native_children[0])? == 1,
        "HUD sequence child selector differs"
    );
    let resource = native.tag(target.u32(ne + 56)?, Some(0x80803F26))?;
    let values = |p: &Payload| -> Result<Vec<[u8; 4]>> {
        p.array(8, 4, Some(0x8080619B))?
            .into_iter()
            .map(|a| p.bytes::<4>(a))
            .collect()
    };
    ensure!(
        resource.bytes::<8>(24)? == translated.payload.bytes::<8>(24)?
            && values(&resource)? == values(&translated.payload)?,
        "HUD sequence has no equal native named override resource"
    );
    let bindings =
        weapon_component_bindings(&target_graph.0, 0x1C0F1EBB).map_err(anyhow::Error::msg)?;
    ensure!(
        bindings.len() == 1
            && bindings[0].owner_tag == target_tag
            && bindings[0].concrete_class == 0x808084D7
            && bindings[0].resource_offset == target.pointer(16)? as u64,
        "Native HUD sequence root binding differs"
    );
    // The allocator clones only owners touched by a patch. Retaining the checked root
    // flags therefore also requests private ownership of this complete scheduler.
    // Only the immutable HUD resource stays shared, after comparison by translated names.
    let flags = target.bytes::<1>(n + 0x2E0)?.to_vec();
    Ok((
        tag,
        vec![NativeAssetResourcePatch {
            binding_hash: bindings[0].binding_hash,
            resource_index: u16::try_from(bindings[0].resource_index)?,
            offset: u32::try_from(
                (n + 0x2E0)
                    .checked_sub(target.pointer(16)?)
                    .context("HUD definition precedes its runtime")?,
            )?,
            expected: flags.clone(),
            bytes: flags,
            imported_particle: None,
        }],
    ))
}
