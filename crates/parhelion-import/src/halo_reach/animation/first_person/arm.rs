//! Source elbow placement with the native arm's segment lengths and IK convention.
use super::{hash, palm, source::Source};
use crate::{
    halo_reach::rig::normalize,
    tiger::{
        animation::{control::Rig, pose::Pose},
        payload::Payload,
    },
};
use anyhow::{Context, Result, ensure};

pub(super) struct Arm {
    pub slot: usize,
    upper: usize,
    elbow: usize,
    hand: usize,
    reference: [f32; 3],
    shoulder: [f32; 3],
    minimum: f32,
    maximum: f32,
    sign: f32,
    scale: f32,
    fallback: [f32; 3],
}

fn position(pose: Pose) -> [f32; 3] {
    pose.0[4..7].try_into().unwrap()
}

fn subtract(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}

fn length(v: [f32; 3]) -> f32 {
    v.iter().map(|v| v * v).sum::<f32>().sqrt()
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

impl Arm {
    pub fn new(
        side: &str,
        source: &Source,
        rig: &Rig,
        controls: &Payload,
        baseline: &[Pose],
        reference: &[Pose],
        align: Pose,
    ) -> Result<Self> {
        let native = |role: &str| {
            rig.names
                .iter()
                .position(|&h| h == hash(&format!("b_{side}_{role}")))
                .context("Native arm joint missing")
        };
        let source_index = |role: &str| {
            source
                .names
                .get(&format!("{side}_{role}"))
                .copied()
                .context("Source arm joint missing")
        };
        let upper = native("upperarm")?;
        let elbow = native("forearm")?;
        let hand = native("hand")?;
        let hand_row = controls.array(controls.pointer(24)? + 0xA0, 52, None)?[rig.slot(hand)?];
        ensure!(
            controls.u16(hand_row + 46)? as usize == upper
                && controls.u8(hand_row + 49)? == 0
                && controls.u8(hand_row + 50)? == 3,
            "Source elbows require the native upper-arm Z-axis IK convention"
        );
        let local_elbow = position(rig.bind[upper].inverse().then(rig.bind[elbow]));
        ensure!(
            local_elbow[0].abs() > 1e-4
                && local_elbow[1].abs() < 1e-4
                && local_elbow[2].abs() < 1e-4,
            "Native upper-arm segment is not aligned with its X axis"
        );
        let scale = baseline[upper].0[7];
        let upper_length = length(subtract(
            position(rig.bind[elbow]),
            position(rig.bind[upper]),
        )) * scale
            / rig.bind[upper].0[7];
        let lower_length = length(subtract(
            position(rig.bind[hand]),
            position(rig.bind[elbow]),
        )) * scale
            / rig.bind[upper].0[7];
        let margin = (upper_length + lower_length) * 0.001;
        let maximum = upper_length + lower_length - margin;
        let minimum = (upper_length - lower_length).abs() + margin;
        ensure!(
            minimum > 0. && maximum > minimum,
            "Native arm lengths are invalid"
        );
        let source_upper = source_index("upperarm")?;
        let z = Pose([0., 0., 0., 1., 0., 0., 1., 1.]);
        Ok(Self {
            slot: rig.slot(upper)?,
            upper: source_upper,
            elbow: source_index("forearm")?,
            hand: source_index("hand")?,
            reference: position(align.then(reference[source_upper])),
            shoulder: position(baseline[upper]),
            minimum,
            maximum,
            sign: local_elbow[0].signum(),
            scale,
            fallback: normalize(subtract(
                position(baseline[upper].then(z)),
                position(baseline[upper]),
            ))?,
        })
    }

    pub fn target(&self, align: Pose, source: &[Pose]) -> Result<(Pose, f32)> {
        let upper = position(align.then(source[self.upper]));
        let elbow = position(align.then(source[self.elbow]));
        let hand = position(align.then(source[self.hand]));
        let mut shoulder = std::array::from_fn(|i| self.shoulder[i] + upper[i] - self.reference[i]);
        let toward_hand = subtract(hand, shoulder);
        let distance = length(toward_hand);
        let direction = normalize(toward_hand)?;
        let adjustment = distance - distance.clamp(self.minimum, self.maximum);
        for i in 0..3 {
            shoulder[i] += direction[i] * adjustment;
        }
        // The nearest reachable elbow lies in the plane containing the source
        // elbow and the native shoulder-to-wrist line. Keep a small bend at the
        // reach boundary so its side remains numerically defined.
        let toward_elbow = subtract(elbow, shoulder);
        let mut normal = cross(toward_elbow, direction);
        if length(normal) < 1e-6 {
            normal = cross(subtract(elbow, upper), subtract(hand, elbow));
            let along = normal
                .iter()
                .zip(direction)
                .map(|(a, b)| a * b)
                .sum::<f32>();
            normal = std::array::from_fn(|i| normal[i] - direction[i] * along);
        }
        if length(normal) < 1e-6 {
            let along = self
                .fallback
                .iter()
                .zip(direction)
                .map(|(a, b)| a * b)
                .sum::<f32>();
            normal = std::array::from_fn(|i| self.fallback[i] - direction[i] * along);
        }
        let z = normalize(normal)?;
        let elbow_direction = normalize(toward_elbow).unwrap_or(direction);
        let x = std::array::from_fn(|i| elbow_direction[i] * self.sign);
        let y = normalize(cross(z, x))?;
        let x = normalize(cross(y, z))?;
        let mut pose = palm::axes(x, y, z)?;
        pose.0[4..7].copy_from_slice(&shoulder);
        pose.0[7] = self.scale;
        Ok((pose, adjustment.abs()))
    }
}
