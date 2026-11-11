//! First-person hand targets and the native arm postpass.
//!
//! A first-person clip's binding resource at `+0x30` names hand targets the postpass moves the
//! wrists to. They resolve against the weapon's markers by name. A clip converted onto a carrier
//! clip whose bindings came from the donor would put the hands on the donor's contact points
//! instead of its own wrists, and an action without them followed by one with them jumps between
//! the two, so [`release`] clears them. A source clip whose own bindings name the source weapon's
//! markers keeps them, and [`targets`] lists the names those markers must carry.
use crate::{
    presentation::put,
    tiger::{ornament::fnv, payload::Payload},
};
use anyhow::{Context, Result, ensure};

/// The upper-arm and hand control slots of each side, from rig controls `controls` and the
/// skeleton's bone names. Controls map bones to slots at definition `+0xD8`.
fn arms(controls: &Payload, names: &[u32]) -> Result<Vec<(u16, u16)>> {
    let definition = controls.pointer(24)?;
    ensure!(
        controls.u32(definition - 4)? == 0x80808f8f,
        "Arm constraints require native rig controls"
    );
    let slots = controls.array(definition + 0xd8, 2, None)?;
    let slot = |name: &str| -> Result<u16> {
        let bone = names
            .iter()
            .position(|&n| n == fnv(name))
            .context("Native arm constraint joint missing")?;
        controls.u16(*slots.get(bone).context("Arm joint has no control slot")?)
    };
    ["l", "r"]
        .into_iter()
        .map(|side| {
            Ok((
                slot(&format!("b_{side}_upperarm"))?,
                slot(&format!("b_{side}_hand"))?,
            ))
        })
        .collect()
}

/// The marker names clip `clip`'s hand targets resolve to, in row order. A row targets a named
/// marker through its target tuple at `+0x10`: no secondary bone (`FFFF`), mode 3, zero flags and
/// the name at `+0x14`. Rows of other modes name no marker and are not listed.
pub fn targets(clip: &Payload) -> Result<Vec<u32>> {
    if clip.u64(0x30)? == 0 {
        return Ok(Vec::new());
    }
    let resource = clip.pointer(0x30)?;
    ensure!(
        clip.u32(resource.checked_sub(4).context("Arm constraint resource")?)? == 0x80808f59,
        "Unsupported native arm constraint resource"
    );
    let mut names = Vec::new();
    for row in clip.array(resource, 64, Some(0x80808f5c))? {
        if clip.u16(row + 0x10)? == u16::MAX
            && clip.u8(row + 0x12)? == 3
            && clip.u8(row + 0x13)? == 0
        {
            names.push(clip.u32(row + 0x14)?);
        }
    }
    Ok(names)
}

/// Clears the hand targets of complete absolute clip `clip` and returns how many it held. Every
/// target must be one of the native upper-arm and hand pairs. The control rows still request
/// guided two-bone solving from the clip's own wrists and upper-arm planes, and auxiliary streams
/// and action events stay.
pub fn release(clip: &mut Payload, controls: &Payload, names: &[u32]) -> Result<usize> {
    ensure!(
        clip.u8(0x181)? == 0,
        "Source hand targets require a complete absolute clip"
    );
    if clip.u64(0x30)? == 0 {
        return Ok(0);
    }
    let resource = clip.pointer(0x30)?;
    ensure!(
        clip.u32(resource.checked_sub(4).context("Arm constraint resource")?)? == 0x80808f59,
        "Unsupported native arm constraint resource"
    );
    let rows = clip.array(resource, 64, Some(0x80808f5c))?;
    if rows.is_empty() {
        return Ok(0);
    }
    let pairs = arms(controls, names)?;
    for &row in &rows {
        ensure!(
            pairs.contains(&(clip.u16(row)?, clip.u16(row + 2)?))
                && clip.u16(row + 0x10)? == u16::MAX
                && clip.u8(row + 0x12)? == 3
                && clip.u8(row + 0x13)? == 0,
            "Native clip has constraints outside the source hand targets"
        );
    }
    put(&mut clip.0, 0x30, &0u64.to_le_bytes())?;
    Ok(rows.len())
}
