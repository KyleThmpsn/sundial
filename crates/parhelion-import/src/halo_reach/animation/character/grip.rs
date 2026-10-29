//! Preserve the source hold and grip spacing with native hands and reachable arm lengths.
use super::source::Source;
use crate::tiger::animation::{control::Rig, pose::Pose};
use anyhow::{Context, Result, ensure};

struct Hand {
    source: usize,
    grip: usize,
    hand: usize,
    upper: usize,
    elbow: usize,
    hand_to_grip: Pose,
    descendants: Vec<usize>,
    minimum: f32,
    maximum: f32,
}

pub(super) struct Grip {
    hands: [Hand; 2],
    source_pelvis: usize,
    pelvis: usize,
    character_scale: f32,
    scale: f32,
}

fn length(v: [f32; 3]) -> f32 {
    v.into_iter().map(|x| x * x).sum::<f32>().sqrt()
}

fn difference(a: Pose, b: Pose) -> [f32; 3] {
    std::array::from_fn(|axis| a.0[axis + 4] - b.0[axis + 4])
}

impl Grip {
    pub fn new(
        source: &Source,
        rig: &Rig,
        parents: &[Option<usize>],
        scale: f32,
        character_scale: f32,
        source_pelvis: usize,
    ) -> Result<Self> {
        ensure!(
            scale.is_finite() && scale > 0. && character_scale.is_finite() && character_scale > 0.,
            "Invalid character or weapon grip scale"
        );
        ensure!(
            source_pelvis < source.bind.len(),
            "Source grip pelvis is absent"
        );
        let pelvis = rig
            .names
            .iter()
            .position(|&name| name == super::name("b_pelvis"))
            .context("Native grip pelvis missing")?;
        let hand = |side: &str| -> Result<Hand> {
            let native = |role: &str| {
                let name = super::name(&format!("b_{side}_{role}"));
                rig.names
                    .iter()
                    .position(|&v| v == name)
                    .context("Native grip bone missing")
            };
            let grip = native("grip")?;
            let hand = native("hand")?;
            let upper = native("upperarm")?;
            let elbow = native("forearm")?;
            ensure!(
                parents[grip] == Some(hand),
                "Weapon grip must follow its hand"
            );
            rig.slot(grip)?;
            let mut descendants = Vec::new();
            for bone in hand + 1..parents.len() {
                let mut parent = parents[bone];
                while let Some(p) = parent {
                    if p == hand {
                        descendants.push(bone);
                        break;
                    }
                    parent = parents[p];
                }
            }
            let a = length(difference(rig.bind[elbow], rig.bind[upper]));
            let b = length(difference(rig.bind[hand], rig.bind[elbow]));
            ensure!(a > 0. && b > 0., "Invalid native arm length");
            let margin = (a + b) * 0.001;
            Ok(Hand {
                source: *source
                    .names
                    .get(&format!("b_{side}_grip"))
                    .context("Source character grip missing")?,
                grip,
                hand,
                upper,
                elbow,
                hand_to_grip: rig.bind[hand].inverse().then(rig.bind[grip]),
                descendants,
                minimum: (a - b).abs() + margin,
                maximum: a + b - margin,
            })
        };
        Ok(Self {
            hands: [hand("r")?, hand("l")?],
            source_pelvis,
            pelvis,
            character_scale,
            scale,
        })
    }

    pub fn apply(&self, source: &[Pose], targets: &mut [Pose]) -> Result<()> {
        let right = &self.hands[0];
        let mut anchor = source[right.source];
        // Keep the source hold relative to the translated body. Retargeted arm
        // rotations alone put the carrying grip at a different body-relative
        // position because the native segment lengths and hand axes differ.
        // Both poses use the same object axes, so the existing character-height
        // ratio applies to this displacement without changing the source rotation.
        let relative = difference(anchor, source[self.source_pelvis]);
        for (axis, value) in relative.into_iter().enumerate() {
            anchor.0[4 + axis] = targets[self.pelvis].0[4 + axis] + value * self.character_scale;
        }
        for hand in &self.hands {
            // Hand spacing follows weapon size independently of character height.
            let mut relative = source[right.source].inverse().then(source[hand.source]);
            relative.0[4..7].iter_mut().for_each(|v| *v *= self.scale);
            let grip = anchor.then(relative);
            let wrist = grip.then(hand.hand_to_grip.inverse());
            let delta = wrist.then(targets[hand.hand].inverse());
            for &bone in &hand.descendants {
                targets[bone] = Pose::checked(delta.then(targets[bone]).0)?;
            }
            targets[hand.hand] = wrist;
            targets[hand.grip] = grip;
            let toward = difference(wrist, targets[hand.upper]);
            let distance = length(toward);
            ensure!(
                distance.is_finite() && distance > 1e-6,
                "Degenerate weapon arm target"
            );
            let adjustment = distance - distance.clamp(hand.minimum, hand.maximum);
            for (axis, value) in toward.into_iter().enumerate() {
                let offset = value * adjustment / distance;
                targets[hand.upper].0[4 + axis] += offset;
                targets[hand.elbow].0[4 + axis] += offset;
            }
        }
        Ok(())
    }
}
