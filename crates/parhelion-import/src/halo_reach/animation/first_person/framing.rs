//! Place the source view in the native weapon reference frame.
use super::*;
use crate::{
    halo_reach::rig::METRES_PER_UNIT,
    tiger::animation::{control::Rig, pose::Pose},
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Framing {
    #[default]
    Native,
    SourceCentered,
    SourceLowered,
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| a[i] * b[i]).sum()
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = std::array::from_fn(|i| a[i] - b[i]);
    dot(d, d).sqrt()
}

/// Largest scale of `source`, at most one, for which `scale * source + offset`
/// stays within `limit` of the origin. The squared distance is quadratic in the
/// scale, so its larger root bounds the reachable range.
fn reach_scale(source: [f32; 3], offset: [f32; 3], limit: f32) -> Result<f32> {
    let a = dot(source, source);
    let b = 2. * dot(source, offset);
    let c = dot(offset, offset) - limit * limit;
    if a <= 0. || a + b + c <= 0. {
        return Ok(1.);
    }
    let discriminant = b * b - 4. * a * c;
    let scale = if discriminant < 0. {
        -b / (2. * a)
    } else {
        (-b + discriminant.sqrt()) / (2. * a)
    };
    ensure!(
        scale.is_finite() && scale > 0.,
        "Source idle grip is beyond native reach at every distance"
    );
    Ok(scale.min(1.))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve(
    cache: &Cache,
    scene: &Scene,
    source: &source::Source,
    rig: &Rig,
    reference: &[[f32; 8]],
    source_reference: &[Pose],
    options: &Options,
) -> Result<([f32; 3], Value)> {
    let field = match options.framing {
        Framing::Native => return Ok((options.held_offset, json!({"mode":"native"}))),
        Framing::SourceCentered => 0x484,
        Framing::SourceLowered => 0x478,
    };
    let bone = |name: &str| {
        rig.names
            .iter()
            .position(|&value| value == hash(name))
            .with_context(|| format!("Native framing role {name} missing"))
    };
    let source_bone = |name: &str| {
        source
            .names
            .get(name)
            .copied()
            .with_context(|| format!("Source framing role {name} missing"))
    };
    let grip = bone("b_r_grip")?;
    let grip_slot = rig.slot(grip)?;
    let parent = rig.controls[grip_slot]
        .parent
        .context("Native grip reference frame missing")?;
    ensure!(
        parent == bone("b_utility")?
            && !rig.controls[grip_slot].root
            && rig.controls[rig.slot(parent)?].root,
        "Source framing requires the native utility-parented grip"
    );
    for name in ["b_l_hand", "b_r_hand"] {
        ensure!(
            rig.controls[rig.slot(bone(name)?)?].parent == Some(grip),
            "Source framing requires both hands to follow the weapon grip"
        );
    }
    let native = rig.objects(reference)?;
    let utility = native[parent].inverse();
    let native_relative = utility.then(native[bone("b_handle")?]);
    let source_relative = source_reference[source_bone("camera_control")?]
        .inverse()
        .then(source_reference[source.root]);
    let source_offset = cache
        .floats::<3>(scene.object.tag.address()? + field)?
        .map(|value| value * METRES_PER_UNIT * scene.scale);
    let requested: [f32; 3] =
        std::array::from_fn(|axis| source_relative.0[4 + axis] + source_offset[axis]);
    // The skeleton hangs the grip, and with it the weapon, from one hand. Native
    // shoulders stay with the body, so a source position at arm's length leaves
    // that arm straight and the hand short of the grip. Keep the source's lateral
    // and vertical placement and shorten only the distance along the reference's
    // +X view axis until the carrying wrist extends no further than the donor
    // idle's most extended wrist does, relative to arm length. Shortening toward
    // the reference origin instead also draws the weapon toward the line of sight,
    // which put a pistol held at arm's length in the middle of the view. The other
    // arm keeps the per-frame shoulder reach adjustment.
    let carrier = rig.parents[grip].context("Native grip has no hierarchy parent")?;
    let side = ["l", "r"]
        .into_iter()
        .find(|side| bone(&format!("b_{side}_hand")).ok() == Some(carrier))
        .context("Native grip does not follow a hand")?;
    let mut envelope = 0f32;
    let mut arm = None;
    for name in ["l", "r"] {
        let [upper, lower] = super::arm::segments(rig, name, &native)?;
        let length = upper + lower;
        let position = |role: &str| -> Result<[f32; 3]> {
            let object = utility.then(native[bone(&format!("b_{name}_{role}"))?]);
            Ok(object.0[4..7].try_into()?)
        };
        let shoulder = position("upperarm")?;
        envelope = envelope.max(distance(position("hand")?, shoulder) / length);
        if name == side {
            arm = Some((shoulder, length));
        }
    }
    let (shoulder, length) = arm.context("Native carrying arm missing")?;
    // The arm solver keeps a 0.1 percent margin below full extension.
    let envelope = envelope.min(0.999);
    ensure!(
        envelope.is_finite() && envelope > 0.,
        "Native idle reach is invalid"
    );
    let mut turn = native_relative;
    turn.0[4..7].fill(0.);
    turn.0[7] = 1.;
    let wrist = turn
        .then(
            source_reference[source.root]
                .inverse()
                .then(source_reference[source_bone(&format!("{side}_hand"))?]),
        )
        .0;
    let forward = [requested[0], 0., 0.];
    let fixed: [f32; 3] =
        std::array::from_fn(|i| requested[i] - forward[i] + options.held_offset[i]);
    let from_shoulder = std::array::from_fn(|i| fixed[i] + wrist[4 + i] - shoulder[i]);
    let limit = envelope * length;
    let scale = reach_scale(forward, from_shoulder, limit)?;
    let target: [f32; 3] = std::array::from_fn(|axis| scale * forward[axis] + fixed[axis]);
    let delta = std::array::from_fn(|axis| target[axis] - native_relative.0[4 + axis]);
    let reached = distance(std::array::from_fn(|i| target[i] + wrist[4 + i]), shoulder);
    // This is a position mapping into the weapon's existing parent frame. Keep
    // the native gun orientation and bind correction. The renderer's final
    // camera, view projection and later pose layers are separate consumers.
    offset::grip(rig, delta)?;
    Ok((
        delta,
        json!({
            "mode":options.framing,"native_reference":"b_utility","source_reference":"camera_control",
            "source_offset_metres":source_offset,"source_gun_relative":&source_relative.0[4..7],
            "native_gun_relative":&native_relative.0[4..7],"requested_gun_relative":requested,
            "reach":{"side":side,"scale":scale,"envelope":envelope,"wrist_distance":reached,"limit":limit,"arm_length":length},
            "target_gun_relative":target,"applied_offset":delta,
            "limit":"Source position mapped into the native weapon reference, with the forward distance shortened to the donor idle's arm reach. Final camera projection and gameplay remain unverified."
        }),
    ))
}
