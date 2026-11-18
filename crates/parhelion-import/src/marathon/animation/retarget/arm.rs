//! Keep authored wrists within the native limb's reach without stretching its bones.
use super::*;

/// The share of its two segment lengths an arm may span. A wrist further away moves its
/// shoulder toward it. The examined stock carriers hold their support arm at 0.79 to 0.88 of
/// its length at idle and ask for at most 0.94 in magazine reloads, while a translated support
/// arm solved to the last 0.1 percent stayed locked straight through idle, firing and sprinting
/// on weapons whose foregrip lies beyond the carrier's shoulder anchor. At 0.95 the elbow keeps
/// about 36 degrees of bend, and the carrier's retained procedural layers have slack to move
/// the weapon without pulling the hand off its target.
const REACH: f32 = 0.95;

pub(super) struct Arm {
    upper: usize,
    hand: usize,
    slot: usize,
    lengths: [f32; 2],
}

fn distance(a: Pose, b: Pose) -> f32 {
    (4..7)
        .map(|i| (a.0[i] - b.0[i]).powi(2))
        .sum::<f32>()
        .sqrt()
}

impl Arm {
    pub(super) fn read(native: &Rig, controls: &Payload, side: &str) -> Result<Self> {
        let bone = |role| {
            native
                .names
                .iter()
                .position(|&name| name == source::fnv(&format!("b_{side}_{role}")))
                .context("Native arm joint missing")
        };
        let upper = bone("upperarm")?;
        let elbow = bone("forearm")?;
        let hand = bone("hand")?;
        let slot = native.slot(upper)?;
        let rows = controls.array(controls.pointer(24)? + 0xA0, 52, None)?;
        let hand_row = rows[native.slot(hand)?];
        ensure!(
            controls.u16(hand_row + 46)? as usize == upper
                && controls.u8(hand_row + 49)? == 0
                && controls.u8(hand_row + 50)? == 3
                && native.parents[hand] == Some(elbow)
                && native.parents[elbow] == Some(upper),
            "Native arm does not use the checked two-bone hand solve"
        );
        // The wrist target must stay independent of the displaced shoulder. Deformation
        // helpers may follow the shoulder or forearm and retain their native local offsets.
        let mut frame = Some(hand);
        let mut visited = BTreeSet::new();
        while let Some(index) = frame {
            ensure!(
                index != upper && index != elbow && visited.insert(index),
                "Native wrist target depends on a displaced shoulder frame"
            );
            frame = match native.controls.iter().find(|c| c.bone == index) {
                Some(c) if c.root => (index != 0).then_some(0),
                Some(c) => c.parent,
                None => native.parents[index],
            };
        }
        let lengths = [
            distance(native.bind[upper], native.bind[elbow]),
            distance(native.bind[elbow], native.bind[hand]),
        ];
        ensure!(
            lengths.iter().all(|v| v.is_finite() && *v > 1e-4),
            "Native arm segment length is invalid"
        );
        Ok(Self {
            upper,
            hand,
            slot,
            lengths,
        })
    }

    pub(super) fn place(
        &self,
        native: &Rig,
        objects: &[Pose],
        values: &mut [[f32; 8]],
        target: Option<Pose>,
    ) -> Result<f32> {
        let mut shoulder = objects[self.upper];
        let hand = target.unwrap_or(objects[self.hand]);
        let scale = shoulder.0[7] / native.bind[self.upper].0[7];
        ensure!(
            scale.is_finite() && scale > 0.,
            "Native shoulder scale is invalid"
        );
        let [a, b] = self.lengths.map(|v| v * scale);
        // The folded limit keeps only the margin that leaves the bend side defined.
        let minimum = (a - b).abs() + (a + b) * 0.001;
        let maximum = (a + b) * REACH;
        ensure!(maximum > minimum, "Native arm reach interval is empty");
        let length = distance(shoulder, hand);
        let shift = length - length.clamp(minimum, maximum);
        if shift.abs() <= 1e-7 {
            return Ok(0.);
        }
        ensure!(
            length > 1e-6,
            "Coincident wrist and shoulder have no reach direction"
        );
        for i in 4..7 {
            shoulder.0[i] += (hand.0[i] - shoulder.0[i]) * shift / length;
        }
        // Preserve the source-aligned guide rotation. Moving along the wrist line changes no
        // bend-plane direction, and the native solver retains the authored wrist orientation.
        values[self.slot] = native.local(self.slot, objects, shoulder)?.0;
        Ok(shift.abs())
    }

    pub(super) fn pair(&self) -> (usize, usize) {
        (self.upper, self.hand)
    }
}
