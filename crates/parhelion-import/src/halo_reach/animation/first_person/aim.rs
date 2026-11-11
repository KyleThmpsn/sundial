//! The hands that leave a scoped weapon's zoomed view once aiming completes.
//!
//! Reach draws no first-person view while zoomed. A converted scoped weapon's surfaces discard
//! at full aim and its HUD scope draws in their place, but the native arms keep drawing, so the
//! carrying hand stayed at the bottom of the zoomed view in gameplay. Raising the aiming eye moved
//! the hands out of view but lowered the weapon visibly through the whole aim, which was rejected.
//! Lowering the root weapon reference `b_utility` at full aim instead brought the arms up into the
//! camera, consistent with the sight placement following the animated weapon and lifting the whole
//! rig back.
//!
//! In the native arm skeleton `b_r_grip`, and the weapon's own bones under it, are children of
//! `b_r_hand`, so the solved right hand carries the weapon. Dropping both hand targets half a metre,
//! out of reach, hid the hands, but every zooming import then fired 2.1 to 3.3 degrees above the
//! reticle at full aim while its shots before full aim and while leaving it landed on the axis: the
//! solver straightened the carrying arm and turned the weapon whose sight aimed fire follows.
//! Keeping the carrying hand's target still put the shots back on the axis, within 0.75 degrees in
//! a scripted Magnum capture, and the carrying hand back in the zoomed view.
//!
//! Raising the grip 0.3 metres above both hand targets, which kept the targets' place in the weapon
//! reference's frame, was meant to have the sight placement lower the rig and take both hands out
//! of view. In play the carrying hand stayed in view and aimed Assault Rifle shots landed about
//! 2.5 to 4 degrees above the reticle again, so the placement does not undo a weapon offset from
//! the hands, and that offset turns the aimed fire like the hand drop did.
//!
//! Here a direct-phase layer, ramping over the last part of aiming after the weapon has
//! disappeared, drops only the supporting hand's target by [`DROP`], out of its arm's reach. A live
//! read of the Magnum at full aim found that arm hanging 0.68 metres below the weapon reference, out
//! of view, while the carrying hand, at its arm's full extension on the grip, stayed at the bottom
//! of the zoomed view.
//!
//! Every earlier move of the carrying hand left the weapon's line: a drop out of reach, or a grip
//! raised from its targets. The carrying hand's target therefore slides back along the grip's
//! forward axis, which is the weapon's bore, toward its shoulder, by up to [`RETREAT`] and no
//! closer than [`SHOULDER_CLEARANCE`] to that shoulder, so within reach.
//! The solved hand meets its target, and whatever the weapon follows, it stays on its own sight
//! line, so aimed fire keeps the reticle.
use super::{
    ammunition::{self, Clip, Input, Wiring},
    hash,
};
use crate::tiger::{animation, payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::sync::Arc;

/// Aim, as stock first-person arms bank `81579906` inherits it in the default namespace at
/// declaration `3D`. Weapon optic banks inherit the same parent as local `79FD498A`.
const AIM: u32 = 0x43a361ce;
/// Aim past which a scoped weapon's surfaces discard and its scope draws, to where the layer
/// reaches its last frame. The upper end stays below one in case aiming settles just short of it.
const RANGE: [f32; 2] = [0.99, 0.995];
/// How far the supporting hand's target drops at full aim, in native metres, beyond its arm's
/// reach and so below the zoomed view.
const DROP: f32 = 0.5;
/// The most the carrying hand's target slides back along the weapon's bore at full aim, in native
/// metres. A 0.2 metre slide left the Magnum's carrying hand in view while firing, whose recoil
/// lifts it.
const RETREAT: f32 = 0.4;
/// How close the slid target may come to its shoulder, in native metres. The slide is the carrying
/// wrist's idle distance from that shoulder less this, so the arm folds within reach on every rig.
const SHOULDER_CLEARANCE: f32 = 0.1;
const IDENTITY: [f32; 8] = [0., 0., 0., 1., 0., 0., 0., 1.];

/// Whether zooming weapons carry the layer.
pub(super) const ENABLED: bool = true;
pub(super) const DISABLED: &str = "The aim layer is off";

type Outcome<T> = std::result::Result<T, &'static str>;

/// An additive clip, on the stock additive direct-phase clip's envelope, that lowers the supporting
/// hand's target by [`DROP`] and slides the right carrying hand's target back, from nothing on its
/// first frame to the full amount on its last. `wrist` is the carrying wrist's idle distance from
/// its shoulder, as source framing measures it. Without it the carrying hand stays. Both targets are controls under the
/// grip, and additive translation adds in that parent frame, whose forward axis is the bore. Every
/// other control, the grip among them, and both targets' rotations stay at identity.
pub(super) fn clip(
    native: &mut Reader,
    bank: &Payload,
    layers: &Payload,
    rig: &animation::control::Rig,
    controls: &Payload,
    ordinal: usize,
    wrist: Option<f32>,
) -> Result<Outcome<Clip>> {
    let Some(template) = ammunition::template(native, bank, layers)? else {
        return Ok(Err("The native arms have no additive direct-phase clip"));
    };
    let envelope = native.tag(template, Some(0x80808f49))?;
    let frames = usize::from(envelope.u16(0x13c)?);
    ensure!(frames >= 2, "Native phase clip is too short");
    let bone = |name: &str| {
        rig.names
            .iter()
            .position(|&value| value == hash(name))
            .with_context(|| format!("Native arm role {name} missing"))
    };
    let grip = bone("b_r_grip")?;
    let hand = rig.slot(bone("b_l_hand")?)?;
    ensure!(
        rig.controls[hand].parent == Some(grip),
        "Native hand targets do not follow the grip"
    );
    // Only a carrying target under the grip moves in the bore's frame.
    let retreat = wrist
        .map(|wrist| (wrist - SHOULDER_CLEARANCE).clamp(0., RETREAT))
        .filter(|&retreat| retreat > 0.);
    let carrying = rig
        .slot(bone("b_r_hand")?)
        .ok()
        .filter(|&slot| rig.controls[slot].parent == Some(grip))
        .zip(retreat);
    let mut deltas = vec![vec![IDENTITY; frames]; rig.controls.len()];
    let weight = |frame| frame as f32 / (frames - 1) as f32;
    for (frame, delta) in deltas[hand].iter_mut().enumerate() {
        delta[6] = -DROP * weight(frame);
    }
    if let Some((carrying, retreat)) = carrying {
        for (frame, delta) in deltas[carrying].iter_mut().enumerate() {
            delta[4] = -retreat * weight(frame);
        }
    }
    let payload =
        animation::clip::replace_deltas_for(&envelope, &deltas, rig.bind.len(), controls)?;
    Ok(Ok(Clip {
        marker: hash("reach/arms/aim"),
        template,
        payload,
        ordinal,
        report: json!({"template":format!("{template:08X}"),"frames":frames,"hand_control":hand,"drop":DROP,
            "carrying_control":carrying.map(|(slot, _)| slot),"retreat":carrying.map(|(_, retreat)| retreat),
            "carrying_wrist":wrist}),
    }))
}

/// Wires [`clip`] to aim over [`RANGE`].
pub(super) fn wire(
    native: &mut Reader,
    clip: &Clip,
    layers: &Payload,
    consumers: &[(u32, Arc<Payload>)],
    owners: (u32, u32),
) -> Result<Outcome<Wiring>> {
    let wiring = ammunition::phase(
        native,
        clip,
        layers,
        consumers,
        owners,
        &Input {
            channel: AIM,
            names: [
                hash("reach/aim"),
                hash("reach/aim_phase"),
                hash("reach/arms/aim"),
            ],
            range: RANGE,
        },
    )?;
    Ok(wiring.map(|mut wiring| {
        wiring.report["limits"] = json!([
            "Through the last part of aiming the supporting hand drops out of the zoomed view and the carrying hand slides back along the bore.",
            "Whether the carrying hand leaves the zoomed view, and whether aimed fire keeps the reticle, remain gameplay checks."
        ]);
        wiring
    }))
}

/// The report of a phase layer, or why the arms keep their stock layers.
pub(super) fn report(result: &std::result::Result<(Clip, Wiring), String>) -> Value {
    match result {
        Ok((_, wiring)) => wiring.report.clone(),
        Err(reason) => json!({"status":"native","reason":reason}),
    }
}
