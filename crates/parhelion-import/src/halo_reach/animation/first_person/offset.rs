//! Move the shared grip ancestor so both native hands follow the weapon.
use super::*;

pub(super) fn grip(rig: &animation::control::Rig, delta: [f32; 3]) -> Result<Option<usize>> {
    ensure!(
        delta.iter().all(|v| v.is_finite() && v.abs() <= 0.2),
        "Held offset exceeds 20 centimetres"
    );
    if delta == [0.; 3] {
        return Ok(None);
    }
    let bone = |name: &str| {
        rig.names
            .iter()
            .position(|&n| n == hash(name))
            .context("Native grip hierarchy is missing")
    };
    let grip = bone("b_r_grip")?;
    let slot = rig.slot(grip)?;
    for name in ["b_l_hand", "b_r_hand"] {
        ensure!(
            rig.controls[rig.slot(bone(name)?)?].parent == Some(grip),
            "Both native hands must follow the adjusted grip"
        );
    }
    ensure!(
        !rig.controls[slot].root && rig.controls[slot].parent.is_some(),
        "Native grip has no local parent frame"
    );
    Ok(Some(slot))
}

pub(super) fn apply(
    p: &Payload,
    rig: &animation::control::Rig,
    grip: Option<usize>,
    delta: [f32; 3],
) -> Result<Payload> {
    match grip {
        Some(slot) => {
            animation::clip::translate(p, slot, rig.defaults[slot][4..7].try_into()?, delta)
        }
        None => Ok(p.clone()),
    }
}

pub(super) fn clips(
    native: &mut Reader,
    bank: &Payload,
    rig: &animation::control::Rig,
    grip: Option<usize>,
    delta: [f32; 3],
    files: &mut Files<'_>,
) -> Result<Vec<Value>> {
    if grip.is_none() {
        return Ok(Vec::new());
    }
    let tags = bank.array(8, 4, Some(0x80808F48))?;
    let mut modes = BTreeMap::<u32, BTreeSet<u16>>::new();
    for row in bank.array(0x68, 32, Some(0x80809002))? {
        let slot = usize::from(bank.u16(row + 24)?);
        let tag = bank.u32(
            *tags
                .get(slot)
                .context("Native clip descriptor exceeds its bank")?,
        )?;
        modes.entry(tag).or_default().insert(bank.u16(row + 26)?);
    }
    let mut clips = Vec::new();
    for (tag, modes) in modes {
        if !modes.contains(&1) {
            continue;
        }
        ensure!(
            modes.len() == 1,
            "Native clip shares absolute and additive playback"
        );
        let original = native.tag(tag, Some(0x80808F49))?;
        let converted = apply(&original, rig, grip, delta)?;
        let file = files.add(&format!("held-{tag:08X}"), &converted)?;
        clips.push(json!({"native":tag,"file":file}));
    }
    ensure!(
        !clips.is_empty(),
        "Native arm controller has no absolute clips"
    );
    Ok(clips)
}
