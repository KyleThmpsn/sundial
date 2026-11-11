//! Native contact placement and a beam from the projectile's retained creation source.
//!
//! Shadowkeep's `CE8860` reads `388F +D0`, class `37D7`, and supplies the source
//! retained by `CF2380` together with the projectile to `3B78` creation parameters.
//! `CF5550 -> CE3580` attaches the projectile at contact and zeros its pool velocity.
//! This composes existing consumers. It does not implement anchor-relative actor motion.
use crate::tiger::payload::{Payload, write_array};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub mod pull;

pub struct Prepared {
    pub projectile: Payload,
    pub beam: Payload,
    pub evidence: Value,
}

fn put(p: &mut Payload, at: usize, bytes: &[u8]) -> Result<()> {
    let end = at
        .checked_add(bytes.len())
        .context("anchor write overflow")?;
    p.0.get_mut(at..end)
        .context("anchor write outside payload")?
        .copy_from_slice(bytes);
    Ok(())
}

fn relative(p: &mut Payload, at: usize, target: usize) -> Result<()> {
    put(
        p,
        at,
        &(i64::try_from(target)? - i64::try_from(at)?).to_le_bytes(),
    )
}

fn object(p: &mut Payload, class: u32, bytes: &[u8]) -> usize {
    let start = (p.0.len() + 19) & !15;
    p.0.resize(start - 4, 0);
    p.0.extend(class.to_le_bytes());
    p.0.extend(bytes);
    start
}

fn pair(p: &Payload, instance: u32, definition: u32) -> Result<(usize, usize)> {
    ensure!(
        p.u64(0)? == p.0.len() as u64,
        "native anchor owner size differs"
    );
    let i = p.pointer(16)?;
    let d = p.pointer(24)?;
    ensure!(
        i >= 4 && d >= 4 && p.u32(i - 4)? == instance && p.u32(d - 4)? == definition,
        "native anchor component class differs"
    );
    ensure!(
        p.u32(i)? == p.u32(d)?
            && p.u32(i + 4)? == definition
            && p.u32(d + 4)? == instance
            && p.u64(i + 8)? == d as u64
            && p.u64(d + 8)? == i as u64,
        "native anchor component pair differs"
    );
    Ok((i, d))
}

/// Turn a native projectile into a non-damaging contact anchor with a source beam.
/// The caller privately owns the containing graphs and their compiled allocations.
/// The native flight lifetime and material admission remain. The final response
/// has no additional label, target or scalar gate once its material group matches.
pub fn prepare(projectile: &Payload, beam: &Payload, beam_tag: u32) -> Result<Prepared> {
    ensure!(
        beam_tag >= 0x80800001 && ![0x811C9DC5, u32::MAX].contains(&beam_tag),
        "native anchor beam tag is absent"
    );
    let (i, d) = pair(projectile, 0x80803B73, 0x8080388F)?;
    let (_, bd) = pair(beam, 0x80803B75, 0x80803B7B)?;
    projectile.bytes::<0x5D0>(d)?;
    projectile.bytes::<0x1D0>(i)?;
    beam.bytes::<0x240>(bd)?;
    let groups = projectile.array(d + 0x218, 0x58, Some(0x808037CD))?;
    ensure!(!groups.is_empty(), "native anchor has no contact responses");
    let mut out = projectile.clone();
    let mut visual = beam.clone();
    // Mode zero uses the world transform on both ends, without a skeleton,
    // locator name, collision attachment body or an enemy-target query.
    put(&mut visual, bd + 0x230, &[0])?;
    // BAFAD0 + BB047D requires both retained endpoints when this byte is set.
    // Losing the projectile or its source then enters native beam retirement.
    put(&mut visual, bd + 0x239, &[1])?;
    let mut config = [0u8; 0x30];
    config[0x28..0x2C].copy_from_slice(&beam_tag.to_le_bytes());
    let beam_config = object(&mut out, 0x808037D7, &config);
    relative(&mut out, d + 0xD0, beam_config)?;
    let mut placement = [0u8; 16];
    placement[8] = 1;
    let placement = object(&mut out, 0x808037E2, &placement);
    // Keep the native speed. Gravity has matching initial and reset values.
    put(&mut out, i + 0x148, &0f32.to_le_bytes())?;
    put(&mut out, d + 0xD8, &0f32.to_le_bytes())?;
    let mut nodes = 0;
    let mut removed = 0;
    let last_group = *groups
        .last()
        .context("native anchor response group missing")?;
    for group in groups {
        let responses = projectile.array(group + 0x48, 0x188, Some(0x808037D0))?;
        ensure!(
            !responses.is_empty(),
            "native anchor response group is empty"
        );
        let fallback = *responses.last().context("native anchor fallback missing")?;
        // CF3BB0 rejects a group with neither material entries nor a fallback.
        // Preserve its native material ancestry instead of turning an empty list
        // into an apparent wildcard. A rejected group never reaches placement.
        ensure!(
            projectile.u64(group + 0x18)? > 0 || projectile.u32(group + 0x30)? != u32::MAX,
            "native anchor response group has no material admission"
        );
        put(&mut out, group + 0x40, &u32::MAX.to_le_bytes())?;
        for node in responses {
            removed += projectile.array(node + 0x178, 16, Some(0x808037F7))?.len();
            if group == last_group && node == fallback {
                let threshold = node + 0xF0;
                let threshold_instance = usize::try_from(projectile.u64(threshold + 8)?)?;
                ensure!(
                    projectile.u32(threshold + 4)? == 0x808089F5
                        && projectile.u32(threshold_instance + 4)? == 0x808089F6
                        && projectile.u64(threshold_instance + 8)? == threshold as u64
                        && projectile.u64(threshold + 0x40)? == 0
                        && projectile.u64(threshold_instance + 0x20)? == 0,
                    "native anchor fallback needs connected input translation"
                );
                // Selection runs last to first. Give the existing final node an
                // unconditional placement path without disconnecting other nodes'
                // compiled scalar inputs or changing their allocation counts.
                put(&mut out, node + 0x28, &[0; 0x50])?;
                // 57F880 treats mode FF as unrestricted. Mode zero with a null
                // label-set pointer is not an empty accepting filter.
                put(&mut out, node + 0x80, &[0; 0x10])?;
                put(&mut out, node + 0x80, &[u8::MAX])?;
                put(&mut out, node + 0x90, &[0; 0x30])?;
                put(&mut out, node + 0x150, &0f32.to_le_bytes())?;
                put(&mut out, node + 0x158, &[0; 0x20])?;
                for program in [node + 0xC0, threshold + 0x10] {
                    write_array(&mut out.0, program, 0x80800009, 4, &[0x34, 0, 0x3E, 0])?;
                    let constant = [1f32.to_le_bytes(); 4].concat();
                    write_array(&mut out.0, program + 16, 0x80800090, 1, &constant)?;
                }
            }
            // Selector 1 places this existing projectile. No damage, child
            // anchor, status recipient or finish-spawn action remains reachable.
            let mut action = [0u8; 16];
            action[8] = 1;
            write_array(&mut out.0, node + 0x178, 0x808037F7, 1, &action)?;
            let row = out.array(node + 0x178, 16, Some(0x808037F7))?[0];
            relative(&mut out, row, placement)?;
            nodes += 1;
        }
    }
    let size = out.0.len() as u64;
    put(&mut out, 0, &size.to_le_bytes())?;
    Ok(Prepared {
        projectile: out,
        beam: visual,
        evidence: json!({
            "native_build":"86657.20.08.23.1800.d2_rc___release",
            "response_nodes":nodes,"replaced_contact_actions":removed,
            "beam_configuration_class":"808037D7","contact_parameter_class":"808037E2",
            "contact_selector":1,"source_endpoint_mode":0,"projectile_endpoint_mode":0,
            "native_material_admission_preserved":true,"fallback_label_mode":255,
            "retire_on_endpoint_loss":true,
            "gravity":0,"native_flight_lifetime_retained":true,
            "actor_pull":false,"gameplay_verified":false
        }),
    })
}
