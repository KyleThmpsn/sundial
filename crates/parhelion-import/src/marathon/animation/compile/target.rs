//! Named equipment targets used by the native hand postpass, after weapon-local motion.
use super::*;

struct Binding {
    name: u32,
    bone: usize,
    local: Pose,
}

pub(super) struct Targets {
    groups: BTreeMap<(usize, usize), Vec<usize>>,
    bindings: Vec<Binding>,
    weights: Vec<Vec<f32>>,
    auxiliary: Vec<Vec<[f32; 8]>>,
}

impl Targets {
    pub(super) fn read(
        clip: &Payload,
        sources: &plan::Sources,
        used: &BTreeSet<u32>,
    ) -> Result<Self> {
        let rows = if clip.u64(0x30)? == 0 {
            Vec::new()
        } else {
            clip.array(clip.pointer(0x30)?, 64, Some(0x80808F5C))?
        };
        let mut result = Self {
            groups: BTreeMap::new(),
            bindings: Vec::new(),
            weights: Vec::new(),
            auxiliary: Vec::new(),
        };
        if rows.is_empty() {
            return Ok(result);
        }
        let equipment = &sources.extension.equipment_skeleton;
        let names = native::names(equipment)?;
        let markers = native::marker_targets(
            &sources.carrier,
            &sources.placement,
            &sources.weapon_rig,
            equipment,
            used,
        )?;
        for (index, row) in rows.iter().copied().enumerate() {
            let bone = |at| -> Result<usize> {
                Ok(sources
                    .native_rig
                    .controls
                    .get(usize::from(clip.u16(at)?))
                    .context("Constraint control slot")?
                    .bone)
            };
            let pair = (bone(row)?, bone(row + 2)?);
            let name = clip.u32(row + 20)?;
            let matches = markers
                .iter()
                .filter(|marker| marker.name == name)
                .collect::<Vec<_>>();
            let [marker] = matches.as_slice() else {
                anyhow::bail!("Constraint marker {name:08X} is absent or ambiguous");
            };
            ensure!(
                marker.binding[0] == 0,
                "Constraint marker has an unsupported body group"
            );
            let marker_name = names
                .get(marker.binding[1] as usize)
                .context("Constraint marker bone")?
                .0;
            let bone = sources
                .native_rig
                .names
                .iter()
                .position(|&name| name == marker_name)
                .context("Constraint marker bone is absent from the first-person rig")?;
            independent(&sources.native_rig, bone)?;
            let mut local = [0.; 8];
            local[..4].copy_from_slice(&marker.orientation);
            local[4..].copy_from_slice(&marker.position);
            result.groups.entry(pair).or_default().push(index);
            result.bindings.push(Binding {
                name,
                bone,
                local: Pose::checked(local)?,
            });
        }
        result.weights = native_clip::constraint_weights(clip, 0x38, rows.len())?;
        result.auxiliary = native_clip::constraint_transforms(clip, rows.len())?;
        Ok(result)
    }

    /// Each arm's final wrist target under one absolute layer at full control influence.
    ///
    /// A lone named row mixes the FK wrist toward its target by its primary weight. When several
    /// rows share the arm, C83F50 gives them one influence, the sum of all their primaries
    /// limited to one, and C84330 takes the rows above 0.0001 in ascending name order. Each
    /// blends its target into the running target and that influence into the running group
    /// weight, both by its own primary and both starting from the procedural row, the FK wrist
    /// at influence one. The group then mixes into the FK wrist by its weight. Rows at zero
    /// therefore still select the grouped route.
    pub(super) fn frame(
        &self,
        objects: &[Pose],
        frame: usize,
    ) -> Result<BTreeMap<(usize, usize), Pose>> {
        self.groups
            .iter()
            .map(|(&pair, rows)| {
                let wrist = objects[pair.1];
                let requested = |row: usize| -> Result<Pose> {
                    let binding = &self.bindings[row];
                    Ok(objects[binding.bone]
                        .then(binding.local)
                        .then(Pose::checked(self.auxiliary[frame][row])?))
                };
                let weight = |row: usize| self.weights[row][frame];
                if let [row] = rows.as_slice() {
                    let target = match weight(*row) {
                        primary if primary > 0. => wrist.mix(requested(*row)?, primary)?,
                        _ => wrist,
                    };
                    return Ok((pair, target));
                }
                let shared = rows.iter().map(|&row| weight(row)).sum::<f32>().min(1.);
                let mut active = rows
                    .iter()
                    .copied()
                    .filter(|&row| weight(row) > 0.0001)
                    .collect::<Vec<_>>();
                active.sort_by_key(|&row| self.bindings[row].name);
                // Rows of one name have no established order between them.
                ensure!(
                    active
                        .windows(2)
                        .all(|pair| self.bindings[pair[0]].name != self.bindings[pair[1]].name),
                    "Simultaneous hand targets of one arm share a name"
                );
                let (mut target, mut influence) = (wrist, 1.);
                for row in active {
                    let primary = weight(row);
                    target = target.mix(requested(row)?, primary)?;
                    influence = influence * (1. - primary) + shared * primary;
                }
                Ok((pair, wrist.mix(target, influence)?))
            })
            .collect()
    }
}

/// The reach pass changes shoulders, so its equipment targets must use independent frames.
fn independent(rig: &control::Rig, bone: usize) -> Result<()> {
    let displaced = ["b_l_upperarm", "b_r_upperarm", "b_l_forearm", "b_r_forearm"].map(source::fnv);
    let mut next = Some(bone);
    let mut visited = BTreeSet::new();
    while let Some(index) = next {
        ensure!(
            !displaced.contains(&rig.names[index]) && visited.insert(index),
            "Equipment target depends on a displaced arm frame"
        );
        next = match rig.controls.iter().find(|control| control.bone == index) {
            Some(control) if control.root => (index != 0).then_some(0),
            Some(control) => control.parent,
            None => rig.parents[index],
        };
    }
    Ok(())
}
