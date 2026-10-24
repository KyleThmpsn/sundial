//! A constant status host, its HUD packet and its separately timed Duration component.
use super::*;
mod conditional;
mod hud;

fn pair(
    p: &Payload,
    owner: u32,
    instance: usize,
    definition: usize,
    ic: u32,
    dc: u32,
) -> Result<()> {
    ensure!(
        p.u32(instance)? == owner
            && p.u32(instance + 4)? == 0
            && p.u64(instance + 8)? == definition as u64
            && p.u32(instance + 16)? == dc
            && p.u32(instance + 20)? == 0
            && p.u32(definition)? == owner
            && p.u32(definition + 4)? == 0
            && p.u64(definition + 8)? == instance as u64
            && p.u32(definition + 16)? == ic
            && p.u32(definition + 20)? == 0,
        "Source status has a different embedded pair"
    );
    Ok(())
}

fn constant(p: &Payload, at: usize) -> Result<[u8; 16]> {
    let code = p
        .array(at + 24, 1, Some(0x80800009))?
        .into_iter()
        .map(|a| p.u8(a))
        .collect::<Result<Vec<_>>>()?;
    let values = p.array(at + 40, 16, Some(0x80800090))?;
    ensure!(
        code == [0x34, 0, 0x3E, 0]
            && values.len() == 1
            && p.u64(at + 56)? == 1
            && p.u64(at + 64)? == 1
            && p.bytes::<24>(at + 72)? == [0; 24]
            && [0, 0x811C9DC5].contains(&p.u64(at + 96)?),
        "Source status needs expression translation"
    );
    let bits = p.u32(values[0])?;
    ensure!(
        f32::from_bits(bits).is_finite()
            && f32::from_bits(bits) >= 0.0
            && (1..4).all(|n| p.u32(values[0] + n * 4).ok() == Some(bits)),
        "Source status needs a finite constant scalar"
    );
    p.bytes::<16>(values[0])
}

struct Status {
    hud: [u8; 12],
    duration: [u8; 20],
    timer: [u8; 8],
    count: [u8; 16],
    seconds: [u8; 16],
}

fn validate(
    graph: &Payload,
    p: &Payload,
    owner: u32,
    companion: Option<&conditional::Companion>,
) -> Result<Status> {
    let live = p.pointer(16)?;
    let settings = p.pointer(24)?;
    let hud = settings + 0xF0;
    let duration = hud + 0x160;
    let controller = duration + 0xB8;
    ensure!(
        p.u32(live - 4)? == 0x80802A75 && p.u32(settings - 4)? == 0x80802698,
        "Source status root differs"
    );
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
    ] {
        ensure!(
            p.u64(settings + offset)? == value,
            "Source status host has additional state"
        );
    }
    let indices = graph.array(0x88, 2, Some(0x80800006))?;
    ensure!(
        indices.len() == 2 && indices.iter().all(|&a| graph.u16(a).ok() == Some(0)),
        "Source status owner indices differ"
    );
    for (i, d, ic, dc) in [
        (live, settings, 0x80802A75, 0x80802698),
        (live + 0x30, hud, 0x8080278D, 0x8080254F),
        (live + 0x50, duration, 0x8080255D, 0x8080223C),
        (live + 0x80, controller, 0x808027B5, 0x80802890),
        (live + 0xA0, duration + 0x138, 0x808008AC, 0x8080038D),
        (live + 0xE0, hud + 0xA0, 0x8080084D, 0x808005A0),
        (live + 0x110, hud + 0x108, 0x8080084D, 0x808005A0),
        (live + 0x170, settings + 0xB8, 0x808008AF, 0x80800456),
    ] {
        pair(p, owner, i, d, ic, dc)?;
    }
    // The host's only output is an empty state program. No external receiver or
    // dynamic parameter driver may disappear while replacing this component.
    ensure!(
        p.u64(settings + 0xD0)? == 2
            && p.u64(settings + 0xD8)? == 0x808003E3
            && p.u64(settings + 0xE0)? == 0
            && p.u64(settings + 0xE8)? == u32::MAX as u64
            && p.u32(settings + 0xA8)? == 0x3C18FBCE
            && p.u32(settings + 0xAC)? == 0
            && p.u64(settings + 0xB0)? == 4,
        "Source status host has a state program or another owner scope"
    );
    let outputs = graph.array(0x30, 88, Some(0x8080054E))?;
    ensure!(
        outputs.len() == if companion.is_some() { 2 } else { 1 }
            && graph.bytes::<16>(0x40)? == [0; 16],
        "Source status has additional event connections"
    );
    let at = outputs[0];
    ensure!(
        graph.u32(at)? == 0x811C9DC5
            && graph.u32(at + 4)? == 0
            && graph.u32(at + 8)? == owner
            && graph.u32(at + 12)? == 0
            && graph.u64(at + 16)? == (settings + 0xB8) as u64
            && graph.u32(at + 24)? == 0x80800456
            && graph.u32(at + 28)? == 0
            && graph.u64(at + 32)? == 0
            && graph.u64(at + 40)? == 0x811C9DC5
            && graph.u64(at + 48)? == u32::MAX as u64
            && graph.u64(at + 56)? == 0
            && graph.u64(at + 64)? == u32::MAX as u64
            && graph.u64(at + 72)? == 65535
            && graph.u64(at + 80)? == 0,
        "Source status has a live output receiver"
    );

    // Each allowed parameter is a declaration, followed by its literal selector.
    // Check the complete export set and reciprocal runtime references as well.
    let declarations: [(usize, usize, u32, u32, u64, u32); 18] = [
        (settings + 0x60, live, 0x80802A75, 0x80AAC44B, 0, 0),
        (settings + 0x78, live, 0x80802A75, 0x80AAC282, 0xFFA2, 0),
        (settings + 0x90, live, 0x80802A75, 0x80AAC44C, 0, 0),
        (settings + 0x418, live, 0x80802A75, 0x80AAC44A, 0, 0),
        (settings + 0x438, live, 0x80802A75, 0x80AAC283, 0, 0),
        (hud + 0x18, live + 0x30, 0x8080278D, 0x80AAC449, 0, 0),
        (hud + 0x38, live + 0x30, 0x8080278D, 0x80AAC447, 0, 0),
        (hud + 0x58, live + 0x30, 0x8080278D, 0x80AAC281, 0, 0),
        (hud + 0x70, live + 0x30, 0x8080278D, 0x80AAC448, 0, 1),
        (duration + 0x18, live + 0x50, 0x8080255D, 0x80AAC286, 0, 0),
        (duration + 0x38, live + 0x50, 0x8080255D, 0x80AAC284, 0, 0),
        (duration + 0x58, live + 0x50, 0x8080255D, 0x80AAEDB3, 0, 0),
        (duration + 0x70, live + 0x50, 0x8080255D, 0x80AAC288, 0, 0),
        (duration + 0x88, live + 0x50, 0x8080255D, 0x80AAC444, 0, 0),
        (
            controller + 0x18,
            live + 0x80,
            0x808027B5,
            0x80AAEDB4,
            0xFF9C,
            0,
        ),
        (controller + 0x30, live + 0x80, 0x808027B5, 0x80AAC285, 0, 2),
        (controller + 0x48, live + 0x80, 0x808027B5, 0x80AAC289, 0, 0),
        (controller + 0x60, live + 0x80, 0x808027B5, 0x80AAC287, 0, 0),
    ];
    for &(a, _, class, declaration, value, _) in &declarations {
        let container = match class {
            0x80802A75 => settings,
            0x8080278D => hud,
            0x8080255D => duration,
            0x808027B5 => controller,
            _ => unreachable!(),
        };
        ensure!(
            p.pointer(a)? == container,
            "Source status parameter has another path"
        );
        ensure!(
            p.u64(a + 8)? == declaration as u64 && p.u64(a + 16)? == value,
            "Source status parameter has an unqualified driver"
        );
    }
    let exports = graph.array(0x68, 40, Some(0x80800370))?;
    let refs = graph.array(0x78, 24, Some(0x80800790))?;
    // The unused host input at +90 is not exported by this compiled form.
    ensure!(
        exports.len() == declarations.len() - 1 + if companion.is_some() { 4 } else { 0 }
            && refs.len() == exports.len(),
        "Source status export count differs"
    );
    let mut seen = BTreeSet::new();
    for (&at, &reference) in exports.iter().zip(&refs) {
        if let Some(companion) = companion
            && graph.u32(at + 16)? == companion.owner
        {
            // The companion reader validates its complete four-export set.
            continue;
        }
        let a = usize::try_from(graph.u64(at + 24)?)?;
        let &(_, instance, class, declaration, value, output) = declarations
            .iter()
            .find(|d| d.0 == a && a != settings + 0x90)
            .context("Unknown source status export")?;
        let selector = if value == 0 {
            0
        } else {
            (value as u16 as i16 as i32) as u32
        };
        ensure!(
            graph.u32(at)? == output << 16
                && graph.u32(at + 4)? as usize == instance - live
                && graph.u32(at + 8)? == selector
                && graph.u32(at + 12)? == class
                && graph.u64(at + 16)? == owner as u64
                && graph.u64(at + 32)? == declaration as u64
                && graph.u64(reference)? == owner as u64
                && graph.u64(reference + 8)? == instance as u64
                && graph.u64(reference + 16)? == 0
                && seen.insert(a),
            "Source status export targets another parameter"
        );
    }
    ensure!(
        p.u32(hud + 0x30)? == 0x80800955
            && p.u32(hud + 0x50)? == 0x80800652
            && p.u64(hud + 0x98)? == 0
            && p.u8(hud + 0xF8)? == 0
            && p.u64(duration + 0x30)? == 0x8080299F
            && p.u64(duration + 0x50)? == 0x80802B20
            && p.u32(duration + 0xA0)? == 5
            && i32::from_le_bytes(p.bytes(duration + 0xA8)?) > 0
            && p.u64(duration + 0xB0)? == u64::from(companion.is_some())
            && p.u64(controller + 0x78)? == u64::from(companion.is_none()),
        "Source status has additional presentation or duration policy"
    );
    let count = match companion {
        Some(companion) => companion.threshold,
        None => constant(p, hud + 0xA0)?,
    };
    let seconds = constant(p, duration + 0x138)?;
    ensure!(
        p.u64(hud + 0x120)? == 5
            && p.u64(hud + 0x130)? == 0
            && p.u64(hud + 0x140)? == 1
            && p.u64(hud + 0x148)? == 1
            && p.bytes::<16>(hud + 0x150)? == [0; 16],
        "Source HUD has a time transform"
    );

    Ok(Status {
        hud: p.bytes(hud + 0x88)?,
        duration: p.bytes(duration + 0xA0)?,
        timer: p.bytes(controller + 0x78)?,
        count,
        seconds,
    })
}

/// The status host's private tag, its patches and appended resources, and its private HUD entry.
pub(super) type Hosted = (
    u32,
    Vec<NativeAssetResourcePatch>,
    Vec<NativeAssetResourceAppend>,
    Option<sundial::package_authoring::sandbox_perk::program::HudStatus>,
);

pub(super) fn read(
    source: &Source,
    native: &mut Reader,
    graph: &Payload,
    p: &Payload,
    owner: u32,
) -> Result<Hosted> {
    let companion = if graph.u64(32)? == 2 {
        Some(conditional::read(source, graph, p, owner)?)
    } else {
        None
    };
    let status = validate(graph, p, owner, companion.as_ref())?;
    let name = u32::from_le_bytes(status.hud[..4].try_into()?);
    let hud_status = hud::read(source, native, name)?;
    let tag = if companion.is_some() {
        0x80BC5880
    } else {
        0x80BC31BC
    };
    let graph = native.tag(tag, Some(0x80809C0F))?;
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    ensure!(
        owners.len() == if companion.is_some() { 2 } else { 1 },
        "Native status envelope has companion components"
    );
    let target_owner = graph.u32(owners[0])?;
    let target = native.tag(target_owner, Some(0x80809C36))?;
    let root = target.pointer(24)?;
    let h = root + 0xE0;
    let d = h + 0x148;
    ensure!(
        target.u32(root - 4)? == 0x80804205
            && target.u32(h + 4)? == 0x80804211
            && target.u32(d + 4)? == 0x80803B32
            && target.u64(root + 0x58)? == 0
            && target.u64(root + 0x70)? == 2
            && target.u64(root + 0xC0)? == 0x3C18FBCE
            && target.u64(root + 0x408)? == 0
            && target.u64(root + 0x410)? == 0
            && target.u64(root + 0x418)? == 0
            && target.u64(root + 0x420)? == 0
            && target.u32(root + 0x428)? == u32::MAX
            && target.u64(h + 0xE8)? == 0x811C9DC5,
        "Native status envelope has an active sequence or display transform"
    );
    let outputs = graph.array(32, 72, Some(0x80809BC9))?;
    ensure!(
        outputs.len() == if companion.is_some() { 3 } else { 2 },
        "Native status envelope outputs differ"
    );
    for at in outputs {
        if companion.is_some() && graph.u32(at + 40)? != u32::MAX {
            // The conditional patcher validates the one channel connection.
            continue;
        }
        ensure!(
            graph.u32(at + 40)? == u32::MAX && graph.u64(at + 48)? == 0,
            "Native status envelope has an external receiver"
        );
    }
    let bindings = weapon_component_bindings(&graph.0, 0x9E50D3EB).map_err(anyhow::Error::msg)?;
    ensure!(
        bindings.len() == 1
            && bindings[0].owner_tag == target_owner
            && bindings[0].concrete_class == 0x80804209,
        "Native status binding differs"
    );
    let binding = &bindings[0];
    let mut patches = Vec::new();
    let mut replace = |at: usize, bytes: Vec<u8>| -> Result<()> {
        let expected = target
            .0
            .get(at..at + bytes.len())
            .context("Truncated native status")?
            .to_vec();
        patches.push(NativeAssetResourcePatch {
            binding_hash: binding.binding_hash,
            resource_index: u16::try_from(binding.resource_index)?,
            offset: u32::try_from(
                at.checked_sub(usize::try_from(binding.resource_offset)?)
                    .context("Native status field precedes instance")?,
            )?,
            expected,
            bytes,
            imported_particle: None,
        });
        Ok(())
    };
    if hud_status.is_some() {
        // The private HUD builder patches the name separately. Leave its expected stock
        // bytes intact and avoid overlapping that patch with priority and display flags.
        replace(h + 0x84, status.hud[4..].to_vec())?;
    } else {
        replace(h + 0x80, status.hud.to_vec())?;
    }
    replace(d + 0x98, status.duration.to_vec())?;
    replace(d + 0xB0 + 0x70, status.timer.to_vec())?;
    for (at, bytes) in [(h + 0x98, status.count), (d + 0x128, status.seconds)] {
        let values = target.array(at + 0x20, 16, Some(0x80800090))?;
        let code = target
            .array(at + 0x10, 1, Some(0x80800009))?
            .into_iter()
            .map(|a| target.u8(a))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            (code == [0x34, 0, 0x3E, 0]
                || (companion.is_some()
                    && at == h + 0x98
                    && code == [0x3C, 1, 0x34, 0, 0x08, 0x3E, 0]))
                && values.len() == 1,
            "Native status envelope has a dynamic expression"
        );
        replace(values[0], bytes.to_vec())?;
    }
    let mut appends = Vec::new();
    if let Some(companion) = companion {
        let (extra, append) =
            conditional::patch(native, &graph, &target, target_owner, &companion)?;
        patches.extend(extra);
        appends.push(append);
    }
    Ok((tag, patches, appends, hud_status))
}
