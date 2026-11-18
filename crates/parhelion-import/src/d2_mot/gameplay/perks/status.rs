//! The status component of an attached entity, in its plain form.
//!
//! A modern status `8080B11F` nests four records: a control (its mode), a status report (the
//! HUD name row and a count), a duration (identity, matching flags, count and priority) and a
//! timer (its seconds). Shadowkeep's `80804205` nests the same four at other offsets. On twin
//! perks whose attachment holds a status on both sides, each field read here equals its
//! Shadowkeep field on at least 211 of 219 statuses, the HUD name aside.
//!
//! The plain form has a constant count and constant seconds, the built-in remaining-time
//! scalar, no child graph, no selector and no limits. Every shipped status graph this route
//! writes into has that form too, so a plain source status becomes fixed-size edits of a
//! private copy and nothing is rebuilt.
use anyhow::{Context, Result, ensure};
use sundial::package_authoring::{
    entity::weapon_component_bindings, sandbox_perk::program::NativeAssetResourcePatch,
};

use crate::d2_mot::payload::Payload;

const EMPTY: u32 = 0x811C_9DC5;
/// The binding of a Shadowkeep status component's instance.
const BINDING: u32 = 0x9E50_D3EB;
/// A modern selector with no rows.
const NO_SELECTOR: [u8; 24] = {
    let mut bytes = [0; 24];
    bytes[1] = 1;
    bytes
};

/// The fields of a plain modern status, as the bytes its Shadowkeep records take.
pub struct Status {
    control: [u8; 8],
    /// The 12 bytes after the HUD name hash: sort order and display flags.
    report: [u8; 12],
    name: u32,
    remaining: [u8; 4],
    duration: [u8; 24],
    timer: [u8; 8],
    seconds: [u8; 16],
    count: [u8; 16],
}

impl Status {
    /// Whether the status shows on the HUD under a name.
    pub fn named(&self) -> bool {
        self.name != EMPTY
    }

    /// The hash of the name the status shows on the HUD, which keys the source's HUD table.
    pub fn name(&self) -> u32 {
        self.name
    }
}

fn code(p: &Payload, at: usize) -> Result<Vec<u8>> {
    p.array(at + 16, 1, Some(0x8080_0009))?
        .into_iter()
        .map(|at| p.u8(at))
        .collect()
}

/// The one constant of a modern scalar that only returns it, with no input.
fn constant(p: &Payload, at: usize) -> Result<[u8; 16]> {
    let constants = p.array(at + 32, 16, Some(0x8080_0090))?;
    ensure!(
        code(p, at)? == [0x42, 0, 0x4C, 0]
            && constants.len() == 1
            && p.array(at + 64, 40, Some(0x8080_9591))?.is_empty(),
        "source status scalar is not a constant"
    );
    let lanes = p.bytes::<16>(constants[0])?;
    ensure!(
        p.f32(constants[0])?.is_finite()
            && (1..4).all(|lane| lanes[lane * 4..lane * 4 + 4] == lanes[..4]),
        "source status constant is not one value in every lane"
    );
    Ok(lanes)
}

/// Read the modern status owner `p` when it has the plain form.
pub fn read(p: &Payload) -> Result<Status> {
    let si = p.pointer(16)?;
    let sd = p.pointer(24)?;
    ensure!(
        si >= 4 && sd >= 4 && p.u32(si - 4)? == 0x8080_3564 && p.u32(sd - 4)? == 0x8080_B11F,
        "source component is not a status"
    );
    // Modes 0 to 4 are the ones the control's Shadowkeep class reads the same way.
    ensure!(
        p.u32(sd + 0x58)? <= 4 && p.f32(sd + 0x5C)?.is_finite(),
        "source status control mode has no Shadowkeep reading"
    );
    ensure!(
        p.u64(sd + 0x5E8)? == u64::from(EMPTY)
            && p.bytes::<24>(sd + 0x5F0)? == NO_SELECTOR
            && p.u64(sd + 0x608)? == 0
            && p.u32(sd + 0x610)? == u32::MAX
            && p.u32(sd + 0x614)? == 1
            && p.u64(sd + 0x618)? == 0
            && p.u64(sd + 0x620)? == 0
            && p.bytes::<16>(sd + 0x628)? == [0; 16],
        "source status has a key, a selector or a child graph"
    );
    let (report_live, report) = (si + 0x30, sd + 0x100);
    ensure!(
        p.u32(report + 4)? == 0x8080_356C
            && p.u64(report_live + 24)? == 0
            && p.u64(report + 0xD8)? == 0
            && p.u64(report + 0x130)? == 0
            && p.u64(report + 0x138)? <= 1
            && p.u64(report + 0x140)? == 0,
        "source status report has labels or a reporting extension"
    );
    // The remaining-time scalar every plain twin holds: its one built-in input, negated.
    ensure!(
        code(p, report + 0x148)? == [0x4A, 0, 0x2A, 0x4C, 0]
            && p.array(report + 0x148 + 32, 16, Some(0x8080_0090))?
                .is_empty()
            && p.array(report + 0x148 + 64, 40, Some(0x8080_9591))?
                .is_empty(),
        "source status adjusts its remaining time"
    );
    let (duration_live, duration) = (report_live + 0x20, report + 0x198);
    let flags = p.bytes::<4>(duration + 20)?;
    ensure!(
        p.u32(duration + 4)? == 0x8080_2DBC
            && p.u64(duration_live + 24)? == 0
            && p.u64(duration_live + 48)? == u64::MAX
            && flags[0] <= 1
            && flags[1] <= 1
            && flags[2] <= 1
            && flags[3] == 0
            && p.u32(duration + 24)? <= i32::MAX as u32,
        "source status has live state or an unread matching policy"
    );
    let inactive = (-1.0f32).to_le_bytes();
    for limit in [duration + 0x20, duration + 0x1D8] {
        ensure!(
            constant(p, limit)?.chunks(4).all(|lane| lane == inactive),
            "source status has a duration limit"
        );
    }
    // Shadowkeep keeps the first and third flags as bits 0 and 4. The second flag selects
    // bit 2 when set and bit 3 when clear, on 192 of 219 twin statuses.
    let native = u32::from(flags[0])
        | u32::from(flags[1]) << 2
        | u32::from(flags[2]) << 4
        | if flags[1] == 0 { 8 } else { 0 };
    let mut fields = [0; 24];
    fields[..4].copy_from_slice(&native.to_le_bytes());
    fields[4..8].copy_from_slice(&p.bytes::<4>(duration + 0x10)?);
    fields[8..16].copy_from_slice(&p.bytes::<8>(duration + 0x18)?);
    let (timer_live, timer) = (duration_live + 0x80, duration + 0x80);
    ensure!(
        p.u32(timer + 4)? == 0x8080_2EA3
            && p.u64(timer_live + 24)? == 0
            && p.u64(timer + 0x90)? <= 1,
        "source status timer has live state"
    );
    Ok(Status {
        control: p.bytes(sd + 0x58)?,
        report: p.bytes(report + 0xCC)?,
        name: p.u32(report + 0xC8)?,
        remaining: p.bytes(report + 0x138)?,
        duration: fields,
        timer: p.bytes(timer + 0x90)?,
        seconds: constant(p, timer + 0x98)?,
        count: constant(p, report + 0xE0)?,
    })
}

/// The one constant of a Shadowkeep scalar that only returns it.
fn native_constant(p: &Payload, at: usize) -> Result<usize> {
    let code = p
        .array(at + 16, 1, Some(0x8080_0009))?
        .into_iter()
        .map(|at| p.u8(at))
        .collect::<Result<Vec<_>>>()?;
    let constants = p.array(at + 32, 16, Some(0x8080_0090))?;
    ensure!(
        code == [0x34, 0, 0x3E, 0] && constants.len() == 1,
        "the Shadowkeep status scalar is not a constant"
    );
    Ok(constants[0])
}

/// The edits that write `status` over the status owner `owner` of the shipped graph `graph`.
/// The HUD name hash stays the graph's own: a named status takes a HUD status of the
/// project's own on its action, which the build writes there.
pub fn patches(
    graph: &Payload,
    owner_tag: u32,
    owner: &Payload,
    status: &Status,
) -> Result<Vec<NativeAssetResourcePatch>> {
    let bindings = weapon_component_bindings(&graph.0, BINDING).map_err(anyhow::Error::msg)?;
    let [binding] = bindings.as_slice() else {
        anyhow::bail!(
            "the Shadowkeep status graph binds {} statuses",
            bindings.len()
        );
    };
    ensure!(
        binding.owner_tag == owner_tag && binding.concrete_class == 0x8080_4209,
        "the Shadowkeep status binding names another component"
    );
    let root = owner.pointer(24)?;
    let report = root + 0xE0;
    let duration = report + 0x148;
    let timer = duration + 0xB0;
    let remaining = owner
        .array(report + 0xF8 + 16, 1, Some(0x8080_0009))?
        .into_iter()
        .map(|at| owner.u8(at))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        root >= 4
            && owner.u32(root - 4)? == 0x8080_4205
            && owner.u32(report + 4)? == 0x8080_4211
            && owner.u32(duration + 4)? == 0x8080_3B32
            && owner.u32(timer + 4)? == 0x8080_3C50
            && owner.u64(root + 0x400)? == u64::from(EMPTY)
            && owner.bytes::<32>(root + 0x408)? == [0; 32]
            && owner.u32(root + 0x428)? == u32::MAX
            && owner.u32(report + 0x80)? == EMPTY
            && owner.u64(report + 0xE8)? == u64::from(EMPTY)
            && remaining == [0x3C, 0, 0x23, 0x3E, 0],
        "the Shadowkeep status graph is not the plain form"
    );
    let instance = usize::try_from(binding.resource_offset)?;
    let mut patches = Vec::new();
    let mut replace = |at: usize, bytes: &[u8]| -> Result<()> {
        let expected = owner
            .0
            .get(at..at + bytes.len())
            .context("the Shadowkeep status is truncated")?;
        if expected != bytes {
            patches.push(NativeAssetResourcePatch {
                binding_hash: binding.binding_hash,
                resource_index: u16::try_from(binding.resource_index)?,
                offset: u32::try_from(
                    at.checked_sub(instance)
                        .context("a Shadowkeep status field lies before its instance")?,
                )?,
                expected: expected.to_vec(),
                bytes: bytes.to_vec(),
                imported_particle: None,
            });
        }
        Ok(())
    };
    replace(root + 0x58, &status.control)?;
    replace(report + 0x84, &status.report)?;
    replace(report + 0xEC, &status.remaining)?;
    replace(duration + 0x98, &status.duration)?;
    replace(timer + 0x70, &status.timer)?;
    replace(native_constant(owner, timer + 0x78)?, &status.seconds)?;
    replace(native_constant(owner, report + 0x98)?, &status.count)?;
    Ok(patches)
}
