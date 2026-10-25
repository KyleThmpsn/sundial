//! Source arm and weapon motion on the native first-person controls.
//!
//! Bones correspond by name. The native rig keeps its own segment lengths, control parents and
//! limb solver. Each native control takes one rule:
//!
//! - The `utility` role is the view reference. It takes its source frame relative to its control
//!   frame, not a change from its rest. The native rig rests it at the origin and every stock
//!   clip lifts it to eye height, while the source rig rests it at eye height.
//!   With native framing it also takes the framing offset, described at [`Framing`].
//! - The source `right_grip` role follows the source relative to the `utility` role, so the
//!   weapon keeps its source place in the view reference.
//! - The weapon root and its parts follow the source relative to their weapon parent. A part
//!   whose native bind differs from its source bind receives the bind difference, so geometry
//!   skinned with source object positions follows its source deformation.
//! - Limb-solved targets follow the source relative to the grip, with the bind rotation
//!   difference, so the hand reaches the source wrist target.
//! - Every other shared bone takes an object rotation equal to the source's times its bind
//!   rotation alignment, solved against its native frame as placed in the same pass, so its skin
//!   deforms from bind as the source bone's does. A frame left to the native limb solver stands
//!   in as the source frame carried through its own bind alignment. Applying
//!   the source change from rest in the native bone's own axes instead turned fingers up to 29
//!   degrees away from the source motion on the examined rifle, because the two rigs' bone axes
//!   differ by 13 to 20 degrees, mostly in roll. Translation keeps the native segment with the
//!   source change from rest. Source bones between a native parent and child, such as
//!   source-only metacarpals, are composed into the child.
//! - A native control without a source bone keeps its default.
//!
//! Additive clips carry deltas. Rotations change frame by the bind alignment of the two bones,
//! and translations by the rest rotation between the two control parent frames.
use super::source::Rig as SourceRig;
use super::*;
use crate::tiger::animation::{control::Rig, pose::Pose};

enum Rule {
    Default,
    Relative {
        bone: usize,
        parent: usize,
        rest: Pose,
        /// Bind rotation alignments, source object to native object, of the bone and its frame.
        align: Pose,
        frame_align: Pose,
        /// The native frame bone, when a native control places it in this pass.
        solved_frame: Option<usize>,
    },
    Placement {
        bone: usize,
        anchor: usize,
        native_anchor: usize,
        correction: Pose,
        rotation_only: bool,
    },
}

pub(super) struct Retarget<'a> {
    source: &'a SourceRig,
    native: &'a Rig,
    rules: Vec<Rule>,
    /// Native slot of each source control slot that shares its bone name.
    pub slots: Vec<Option<usize>>,
    /// Additive frame changes: bone rotation and parent-frame rotation per native slot.
    additive: Vec<Option<(usize, Pose, Pose)>>,
    /// The view reference's native slot and the source utility and grip bones.
    view: (usize, usize, usize),
    framing: Option<Framing>,
    pub report: Value,
}

/// Native framing of the held weapon. The source rig holds its grip closer to the eye than the
/// native first-person view frames it, so the view reference takes a constant offset that puts
/// the source idle grip where the carrier's idle holds its own. The offset's lateral and vertical
/// parts scale with the grip's distance from the view axis, between the aimed and the idle
/// distance, so sights the source centres on the axis stay centred.
#[derive(Clone, Copy)]
pub(super) struct Framing {
    pub offset: [f32; 3],
    pub idle: f32,
    pub aimed: f32,
}

fn rotation(p: Pose) -> Pose {
    Pose([p.0[0], p.0[1], p.0[2], p.0[3], 0., 0., 0., 1.])
}

fn relative(bind: &[Pose], parent: usize, bone: usize) -> Pose {
    bind[parent].inverse().then(bind[bone])
}

/// The rotation that carries a source bone's bind orientation onto the native bone's, both in
/// object space. The arm meshes are skinned at bind, so a native bone whose object rotation is
/// the source's times this alignment deforms its skin as the source bone does.
fn align(source: &SourceRig, native: &Rig, source_bone: usize, native_bone: usize) -> Pose {
    rotation(
        source.bind[source_bone]
            .inverse()
            .then(native.bind[native_bone]),
    )
}

impl<'a> Retarget<'a> {
    pub fn new(source: &'a SourceRig, native: &'a Rig) -> Result<Self> {
        let index = |names: &[u32], name: u32| names.iter().position(|n| *n == name);
        let grip = source.role("right_grip")?;
        let utility = source.role("utility")?;
        let handle = source.role("handle")?;
        let native_of = |bone: usize| {
            index(&native.names, source.names[bone])
                .with_context(|| format!("Native rig lacks source bone {:08X}", source.names[bone]))
        };
        let native_grip = native_of(grip)?;
        let native_utility = native_of(utility)?;
        let native_handle = native_of(handle)?;
        // Weapon membership follows the native hierarchy below the weapon root.
        let weapon = descendants(native, native_handle);
        let rest = source.objects(&source.defaults.iter().map(|v| Pose(*v)).collect::<Vec<_>>())?;
        let mut rules = Vec::new();
        let mut counts = BTreeMap::<&str, usize>::new();
        for control in &native.controls {
            let name = native.names[control.bone];
            // The native control frame: the root for root-relative controls, else the parent.
            let frame = if control.root {
                Some(0)
            } else {
                control.parent
            };
            let rule = match index(&source.names, name) {
                None => Rule::Default,
                Some(bone) if control.bone == native_utility => {
                    let native_anchor = frame.context("Native view reference lacks a frame")?;
                    Rule::Placement {
                        bone,
                        anchor: index(&source.names, native.names[native_anchor]).with_context(
                            || {
                                format!(
                                    "Source rig lacks view reference frame {:08X}",
                                    native.names[native_anchor]
                                )
                            },
                        )?,
                        native_anchor,
                        correction: Pose::IDENTITY,
                        rotation_only: false,
                    }
                }
                Some(bone) if control.bone == native_grip => Rule::Placement {
                    bone,
                    anchor: utility,
                    native_anchor: native_utility,
                    correction: Pose::IDENTITY,
                    rotation_only: false,
                },
                Some(bone) if control.bone == native_handle => Rule::Placement {
                    bone,
                    anchor: grip,
                    native_anchor: native_grip,
                    correction: Pose::IDENTITY,
                    rotation_only: false,
                },
                Some(bone) if weapon.contains(&control.bone) => Rule::Placement {
                    bone,
                    anchor: handle,
                    native_anchor: native_handle,
                    correction: relative(&source.bind, handle, bone)
                        .inverse()
                        .then(relative(&native.bind, native_handle, control.bone)),
                    rotation_only: false,
                },
                Some(bone)
                    if source
                        .controls
                        .iter()
                        .any(|c| c.bone == bone && matches!(c.limb, 3 | 6)) =>
                {
                    Rule::Placement {
                        bone,
                        anchor: grip,
                        native_anchor: native_grip,
                        correction: rotation(
                            relative(&source.bind, grip, bone).inverse().then(relative(
                                &native.bind,
                                native_grip,
                                control.bone,
                            )),
                        ),
                        rotation_only: true,
                    }
                }
                Some(bone) => {
                    match frame.and_then(|p| Some((p, index(&source.names, native.names[p])?))) {
                        Some((native_frame, parent)) => Rule::Relative {
                            bone,
                            parent,
                            rest: relative(&rest, parent, bone).inverse(),
                            align: align(source, native, bone, control.bone),
                            frame_align: align(source, native, parent, native_frame),
                            solved_frame: native
                                .controls
                                .iter()
                                .any(|c| c.bone == native_frame)
                                .then_some(native_frame),
                        },
                        None => Rule::Default,
                    }
                }
            };
            *counts
                .entry(match rule {
                    Rule::Placement { .. } if control.bone == native_utility => "view_reference",
                    Rule::Default => "native_default",
                    Rule::Relative { .. } => "source_relative",
                    Rule::Placement {
                        rotation_only: true,
                        ..
                    } => "limb_target",
                    Rule::Placement { .. } => "weapon_placement",
                })
                .or_default() += 1;
            rules.push(rule);
        }
        let slots = source
            .controls
            .iter()
            .map(|c| {
                native
                    .controls
                    .iter()
                    .position(|n| native.names[n.bone] == source.names[c.bone])
            })
            .collect::<Vec<_>>();
        // Rest frames for additive deltas, evaluated on both rigs' defaults.
        let native_rest = native.objects(&native.defaults)?;
        let mut additive = Vec::new();
        for (slot, control) in native.controls.iter().enumerate() {
            let Some(source_slot) = slots.iter().position(|s| *s == Some(slot)) else {
                additive.push(None);
                continue;
            };
            let bone = source.controls[source_slot].bone;
            // A delta post-multiplied on the source bone becomes the same rotation seen through
            // the bind alignment of the two bones.
            let turn = align(source, native, bone, control.bone).inverse();
            let source_parent = source.parent_frame(source_slot, &rest);
            let native_parent = native.parent(slot, &native_rest);
            additive.push(Some((
                source_slot,
                turn,
                rotation(native_parent.inverse().then(source_parent)),
            )));
        }
        let report = json!({"rules":counts,"weapon_bones":weapon.len(),
            "anchors":{"utility":format!("{:08X}",source.names[utility]),"grip":format!("{:08X}",source.names[grip]),
                "handle":format!("{:08X}",source.names[handle])},
            "unmatched_source_controls":slots.iter().enumerate().filter(|(_, s)| s.is_none())
                .map(|(i, _)| format!("{:08X}", source.names[source.controls[i].bone])).collect::<Vec<_>>()});
        let view = native
            .controls
            .iter()
            .position(|c| c.bone == native_utility)
            .context("Native view reference has no control")?;
        Ok(Self {
            source,
            native,
            rules,
            slots,
            additive,
            view: (view, utility, grip),
            framing: None,
            report,
        })
    }

    /// The source grip place relative to the utility role, for framing.
    pub fn grip(&self, values: &[Pose]) -> Result<[f32; 3]> {
        let source = self.source.objects(values)?;
        let held = source[self.view.1].inverse().then(source[self.view.2]);
        Ok([held.0[4], held.0[5], held.0[6]])
    }

    pub fn frame(&mut self, framing: Framing) {
        self.framing = Some(framing);
    }

    /// The view reference offset for one frame of source objects.
    fn view_offset(&self, source: &[Pose]) -> Option<Pose> {
        let framing = self.framing?;
        let held = source[self.view.1].inverse().then(source[self.view.2]);
        let axis = held.0[5].hypot(held.0[6]);
        let span = framing.idle - framing.aimed;
        let share = if span > 0.01 {
            ((axis - framing.aimed) / span).clamp(0., 1.)
        } else {
            1.
        };
        let [x, y, z] = framing.offset;
        Some(Pose([0., 0., 0., 1., x, y * share, z * share, 1.]))
    }

    /// Native local control poses for one frame of absolute source control values.
    pub fn absolute(&self, values: &[Pose]) -> Result<Vec<[f32; 8]>> {
        let source = self.source.objects(values)?;
        let view_offset = self.view_offset(&source);
        let mut objects = self.native.bind.clone();
        let mut result = Vec::with_capacity(self.rules.len());
        for (slot, rule) in self.rules.iter().enumerate() {
            let local = match *rule {
                Rule::Default => Pose(self.native.defaults[slot]),
                Rule::Relative {
                    bone,
                    parent,
                    rest,
                    align,
                    frame_align,
                    solved_frame,
                } => {
                    let motion = source[parent].inverse().then(source[bone]);
                    // Translation and scale keep the native segment and the source change from
                    // rest. The object rotation is the source's times the bone's bind alignment,
                    // taken against the native frame this pass already placed. A frame the native
                    // limb solver moves in game is not placed here, so the source frame carried
                    // through its own bind alignment stands in for it.
                    let placed = Pose(self.native.defaults[slot]).then(rest.then(motion));
                    let turned = match solved_frame {
                        Some(frame) => rotation(objects[frame])
                            .inverse()
                            .then(rotation(source[bone]))
                            .then(align),
                        None => frame_align.inverse().then(rotation(motion)).then(align),
                    };
                    Pose([
                        turned.0[0],
                        turned.0[1],
                        turned.0[2],
                        turned.0[3],
                        placed.0[4],
                        placed.0[5],
                        placed.0[6],
                        placed.0[7],
                    ])
                }
                Rule::Placement {
                    bone,
                    anchor,
                    native_anchor,
                    correction,
                    rotation_only,
                } => {
                    let relative = source[anchor].inverse().then(source[bone]);
                    let mut target = relative.then(correction);
                    if rotation_only {
                        target.0[4..8].copy_from_slice(&relative.0[4..8]);
                    }
                    if slot == self.view.0
                        && let Some(offset) = view_offset
                    {
                        target = target.then(offset);
                    }
                    self.native
                        .local(slot, &objects, objects[native_anchor].then(target))?
                }
            };
            let local = Pose::checked(local.0)?;
            let bone = self.native.controls[slot].bone;
            objects[bone] = self.native.compose(slot, &objects, local);
            result.push(local.0);
        }
        Ok(result)
    }

    /// Native delta poses for one frame of additive source deltas.
    pub fn additive(&self, deltas: &[Pose]) -> Result<Vec<[f32; 8]>> {
        self.additive
            .iter()
            .map(|change| {
                let Some((slot, turn, frame)) = *change else {
                    return Ok(Pose::IDENTITY.0);
                };
                let delta = deltas[slot];
                // A post-multiplied delta changes frame by conjugation with the rest turn.
                let rotated = turn.then(rotation(delta)).then(turn.inverse());
                let moved = frame.then(Pose([
                    0., 0., 0., 1., delta.0[4], delta.0[5], delta.0[6], 1.,
                ]));
                Ok(Pose::checked([
                    rotated.0[0],
                    rotated.0[1],
                    rotated.0[2],
                    rotated.0[3],
                    moved.0[4],
                    moved.0[5],
                    moved.0[6],
                    delta.0[7],
                ])?
                .0)
            })
            .collect()
    }
}

/// Native bones under and including `root`, through the bind hierarchy.
fn descendants(native: &Rig, root: usize) -> BTreeSet<usize> {
    let mut result = BTreeSet::from([root]);
    for bone in root + 1..native.parents.len() {
        if native.parents[bone].is_some_and(|p| result.contains(&p)) {
            result.insert(bone);
        }
    }
    result
}
