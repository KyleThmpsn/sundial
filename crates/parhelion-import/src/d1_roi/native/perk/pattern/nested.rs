//! A native component owns a separate private controller for the attachment's lifetime.
use super::*;
use sundial::package_authoring::sandbox_perk::program::NativeAssetProgram;

pub(super) fn source_action(source: &Source, graph: &Payload) -> Result<u32> {
    let owners = graph.array(0x20, 12, Some(0x80800715))?;
    ensure!(
        owners.len() == 1 && graph.bytes::<32>(0x30)? == [0; 32] && graph.u64(0x88)? == 0,
        "Nested controller graph has additional owners, connections or conditions"
    );
    let owner = graph.u32(owners[0])?;
    ensure!(
        source.reference(owner)? == 0x80800861,
        "Nested controller owner class differs"
    );
    let p = source.read(owner)?;
    let (live, settings) = (p.pointer(16)?, p.pointer(24)?);
    ensure!(
        p.u32(live - 4)? == 0x808026AD && p.u32(settings - 4)? == 0x80802A94,
        "Nested controller component pair differs"
    );
    status::pair(&p, owner, live, settings, 0x808026AD, 0x80802A94)?;
    for (offset, value) in [
        (0x18, 0),
        (0x20, u32::MAX as u64),
        (0x28, 0),
        (0x30, u32::MAX as u64),
        (0x38, u32::MAX as u64),
        (0x40, 0),
        (0x48, u32::MAX as u64),
        (0x50, 0),
        (0x58, 0),
        (0x60, 0),
        (0x80, 0),
    ] {
        ensure!(
            p.u64(settings + offset)? == value,
            "Nested controller has additional host state"
        );
    }
    ensure!(
        p.pointer(settings + 0x70)? == settings
            && p.u64(settings + 0x78)? == 0x80AAE896
            && p.u32(settings + 0x6C)? == 0
            && p.bytes::<40>(live + 0x18)?
                == [
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
                ],
        "Nested controller has a live recipient or another input driver"
    );
    let bindings = graph.array(0x58, 8, Some(0x80800361))?;
    let exports = graph.array(0x68, 40, Some(0x80800370))?;
    let references = graph.array(0x78, 24, Some(0x80800790))?;
    ensure!(
        bindings.len() == 1 && exports.len() == 1 && references.len() == 1,
        "Nested controller exports additional behavior"
    );
    let (binding, export, reference) = (bindings[0], exports[0], references[0]);
    ensure!(
        graph.u32(binding)? == 0x0FFD6163
            && graph.bytes::<4>(binding + 4)? == [0, 0, 1, 0]
            && graph.bytes::<12>(export)? == [0; 12]
            && graph.u32(export + 12)? == 0x808026AD
            && graph.u32(export + 16)? == owner
            && graph.u32(export + 20)? == 0
            && graph.u64(export + 24)? == (settings + 0x70) as u64
            && graph.u64(export + 32)? == 0x80AAE896
            && graph.u32(reference)? == owner
            && graph.u32(reference + 4)? == 0
            && graph.u64(reference + 8)? == live as u64
            && graph.u64(reference + 16)? == 0,
        "Nested controller input export differs"
    );
    let resources = p.array(0x30, 16, Some(0x80800930))?;
    ensure!(
        resources.len() == 1
            && p.pointer(resources[0])? == live
            && p.u32(resources[0] + 8)? == 0x808026AD
            && p.u32(resources[0] + 12)? == 0x1FFFF,
        "Nested controller runtime allocation differs"
    );
    let action = p.u32(settings + 0x68)?;
    ensure!(
        source.reference(action)? == 0x80802711,
        "Nested reference is not a source controller"
    );
    Ok(action)
}

pub(super) fn read(
    source: &Source,
    native: &mut Reader,
    graph: &Payload,
    ancestors: &mut Vec<u32>,
) -> Result<(u32, NativeAssetProgram, usize)> {
    let action = source_action(source, graph)?;
    // Original source methods 111B2E0/111B430 and native EFA440/EFA010 retain
    // the recipient and register/remove the referenced controller. Source definition
    // +68 becomes native +50. The child keeps its own events and retained effects.
    let (program, omitted_feedback) = super::super::program_inner(
        source,
        action,
        "Nested Source Effect",
        Some(native),
        ancestors,
    )?;
    let tag = 0x80C191F5;
    let graph = native.tag(tag, Some(0x80809C0F))?;
    let owners = graph.array(0x10, 12, Some(0x80809C04))?;
    ensure!(
        owners.len() == 1
            && graph.bytes::<32>(0x20)? == [0; 32]
            && graph.u64(0x78)? == 0
            && graph.u64(0x48)? == 1
            && graph.u64(0x58)? == 1
            && graph.u64(0x68)? == 1,
        "Native nested controller envelope has additional topology"
    );
    let owner = graph.u32(owners[0])?;
    let p = native.tag(owner, Some(0x80809C36))?;
    let (live, settings) = (p.pointer(16)?, p.pointer(24)?);
    ensure!(
        p.u32(live - 4)? == 0x80803E51
            && p.u32(settings - 4)? == 0x80803E52
            && p.u32(live)? == owner
            && p.u32(live + 4)? == 0x80803E52
            && p.u64(live + 8)? == settings as u64
            && p.u32(settings)? == owner
            && p.u32(settings + 4)? == 0x80803E51
            && p.u64(settings + 8)? == live as u64,
        "Native nested controller pair differs"
    );
    for (offset, value) in [
        (0x10, 0),
        (0x18, u64::MAX),
        (0x20, 0),
        (0x28, u64::MAX),
        (0x30, 0),
        (0x38, 0),
        (0x40, 0),
        (0x48, 0),
        (0x68, 0),
    ] {
        ensure!(
            p.u64(settings + offset)? == value,
            "Native nested controller has additional host state"
        );
    }
    ensure!(
        p.pointer(settings + 0x58)? == settings
            && p.u64(settings + 0x60)? == 0x80BFD2C6
            && p.u32(settings + 0x54)? == 0,
        "Native nested controller has another input driver"
    );
    let bindings = weapon_component_bindings(&graph.0, 0x95A60F29).map_err(anyhow::Error::msg)?;
    ensure!(
        bindings.len() == 1
            && bindings[0].owner_tag == owner
            && bindings[0].concrete_class == 0x80803E51
            && bindings[0].resource_offset == live as u64,
        "Native nested controller binding differs"
    );
    let expected_action = p.u32(settings + 0x50)?;
    native.tag(expected_action, Some(0x808040B5))?;
    Ok((
        tag,
        NativeAssetProgram {
            binding_hash: bindings[0].binding_hash,
            resource_index: u16::try_from(bindings[0].resource_index)?,
            offset: u32::try_from(
                (settings + 0x50)
                    .checked_sub(live)
                    .context("Nested controller definition precedes its runtime resource")?,
            )?,
            expected_action,
            program: Box::new(program),
        },
        omitted_feedback,
    ))
}
