//! Weapon-local motion. The weapon root's own lookup animates the weapon model's bones. The arm
//! clip places the weapon root, and each absolute arm clip takes one weapon-local descriptor for
//! the bones below it: the weapon state or descriptor the source names for the native action,
//! bare or behind the weapon's profile lineage, else the choice another action made for the
//! same arm descriptor by that rule, else the closing descriptor whose clip length matches a
//! firing, chambering or magazine reload action, else the weapon's idle state. A firing or
//! chambering clip chosen by length must also end at the idle pose.
use super::plan::{Routes, Sources};
use super::*;

/// The weapon bones below the weapon root and the weapon-local descriptor each routed absolute
/// arm clip takes, with how it was chosen.
pub(super) struct WeaponLocal {
    /// The weapon root's native rig index.
    pub(super) native_handle: usize,
    /// Native control slot and source weapon bone of each bone below the root, in slot order.
    pub(super) bones: Vec<(usize, usize)>,
    pub(super) corrections: Vec<Pose>,
    /// The weapon-local descriptor by request and source arm descriptor.
    pub(super) overlays: BTreeMap<(u32, u32), u32>,
    /// How each overlay was chosen. An `idle` choice is the absence of a source mechanism.
    pub(super) chosen: BTreeMap<(u32, u32), &'static str>,
    pub(super) pairings: Vec<(u32, Value)>,
    pub(super) mechanisms: BTreeMap<u32, Mechanism>,
    pub(super) report: Value,
}

impl WeaponLocal {
    pub(super) fn pair(source: &mut Reader, sources: &Sources, routes: &Routes) -> Result<Self> {
        let (native_rig, weapon_rig) = (&sources.native_rig, &sources.weapon_rig);
        let native_handle = native_rig
            .names
            .iter()
            .position(|n| *n == weapon_rig.names[0])
            .context("Native rig lacks the source weapon root")?;
        let mut bones = Vec::new();
        for (bone, name) in weapon_rig.names.iter().enumerate().skip(1) {
            let native_bone = native_rig
                .names
                .iter()
                .position(|n| n == name)
                .with_context(|| format!("Native rig lacks source weapon bone {name:08X}"))?;
            let slot = native_rig
                .controls
                .iter()
                .position(|c| c.bone == native_bone)
                .with_context(|| format!("Native weapon bone {name:08X} has no control"))?;
            bones.push((slot, bone));
        }
        bones.sort_unstable();
        // Weapon geometry is skinned in source positions. A bone that shares its name with the
        // carrier keeps the carrier bind, so it takes the bind difference, as the arm retarget's
        // weapon placement does. Appended bones carry their source bind and take none.
        let corrections = bones
            .iter()
            .map(|&(slot, bone)| {
                let source_rest = weapon_rig.bind[0].inverse().then(weapon_rig.bind[bone]);
                let native_rest = native_rig.bind[native_handle]
                    .inverse()
                    .then(native_rig.bind[native_rig.controls[slot].bone]);
                source_rest.inverse().then(native_rest)
            })
            .collect::<Vec<_>>();
        let owner = &sources.weapon_driver.lookup;
        let mut report = json!({"lookup":owner.0,"driver":sources.weapon_driver.report,
            "bones":bones.len(),"notes":[]});
        let local = (!bones.is_empty())
            .then_some(sources.local_controller.as_ref())
            .flatten();
        if let Some(error) = &sources.local_error {
            report["notes"] = json!([error]);
        }
        let mut pairing = Self {
            native_handle,
            bones,
            corrections,
            overlays: BTreeMap::new(),
            chosen: BTreeMap::new(),
            pairings: Vec::new(),
            mechanisms: BTreeMap::new(),
            report,
        };
        if let Some(local) = local {
            pairing.choose(source, sources, routes, local)?;
        }
        Ok(pairing)
    }

    /// Chooses the weapon-local descriptor of every routed absolute arm clip from the weapon's
    /// own lookup `local`, keeping each choice's failure as a note.
    fn choose(
        &mut self,
        source: &mut Reader,
        sources: &Sources,
        routes: &Routes,
        local: &controller::Controller,
    ) -> Result<()> {
        let weapon_rig = &sources.weapon_rig;
        // Both pairing passes use the same supported inputs as the arm request. A reload's
        // explicit state cannot seed a shared fallback with a different ammunition state.
        let inputs_for = |request: u32| {
            routes.requests[&request]
                .inputs
                .iter()
                .filter(|(builtin, value)| {
                    ![PROFILE, OPTIC].contains(builtin) || local.declares(**builtin, **value)
                })
                .map(|(&builtin, &value)| (builtin, value))
                .collect::<BTreeMap<_, _>>()
        };
        let mut timed = Vec::new();
        for (index, entry) in local.descriptors.iter().enumerate() {
            let Some(ordinal) = entry.clip else {
                continue;
            };
            if entry.rate != 1. {
                continue;
            }
            let clip = source.tag(local.clips[ordinal], Some(0x8080AE01))?;
            let descriptor = u32::try_from(index)?;
            // A clip the weapon rig cannot sample is left out, as choosing it would fail.
            let sampled = match mechanism(source, local, descriptor, weapon_rig, &self.bones) {
                Ok(sampled) => sampled,
                Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
                Err(_) => continue,
            };
            timed.push(Timed {
                descriptor,
                ordinal,
                frames: clip.u16(0x140)?,
                closes: closes(&sampled),
            });
            self.mechanisms.insert(descriptor, sampled);
        }
        let idle = source::fnv("idle");
        // Timing correspondence is only qualified for these mechanism actions. A transition,
        // draw or locomotion clip can have the same span as a reload or firing clip without
        // requesting it. Unknown action names retain explicit/shared selection or idle.
        let timed_actions = [
            "fire",
            "fire_1",
            "iron_sight_fire",
            "chamber_1",
            "reload_full",
            "reload_empty",
        ]
        .iter()
        .map(|a| source::fnv(a))
        .collect::<BTreeSet<_>>();
        let reload_order = |action: u32| {
            [source::fnv("reload_empty"), source::fnv("reload_full")]
                .iter()
                .position(|r| *r == action)
        };
        // A shot or a chambering stroke returns the weapon to rest. Reloads are left out: an
        // empty reload starts and can end away from the idle pose.
        let resting_actions = ["fire", "fire_1", "iron_sight_fire", "chamber_1"]
            .iter()
            .map(|a| source::fnv(a))
            .collect::<BTreeSet<_>>();
        // The clips that end at the idle pose, by the idle descriptor the request selects.
        let mut resting = BTreeMap::<u32, BTreeSet<u32>>::new();
        // The weapon's profile and its dictionary ancestors prefix its descriptor names.
        let lineage = sources.controller.lineage(PROFILE, sources.profile);
        // Actions that route one arm descriptor share its best-supported weapon descriptor.
        let rank = |how: &str| {
            ["state", "name", "length", "idle"]
                .iter()
                .position(|h| *h == how)
        };
        let mut shared = BTreeMap::<(u32, BTreeMap<u8, u32>), (&'static str, BTreeSet<u32>)>::new();
        for (&request, route) in &routes.routes {
            let action = routes.requests[&request].action;
            let inputs = inputs_for(request);
            let found = explicit(local, &timed, action, &inputs, &lineage)
                .ok()
                .flatten();
            for &(descriptor, _) in route {
                if let Some((local_descriptor, how, _)) = found {
                    let (best, choices) = shared
                        .entry((descriptor, inputs.clone()))
                        .or_insert_with(|| (how, BTreeSet::new()));
                    if rank(how) < rank(best) {
                        *best = how;
                        choices.clear();
                    }
                    if rank(how) == rank(best) {
                        choices.insert(local_descriptor);
                    }
                }
            }
        }
        self.report["ambiguous_shared_descriptors"] = json!(
            shared
                .iter()
                .filter(|(_, (_, choices))| choices.len() > 1)
                .map(
                    |((descriptor, inputs), (how, choices))| json!({"source_descriptor":descriptor,"inputs":inputs,
                "chosen_by":how,"weapon_descriptors":choices})
                )
                .collect::<Vec<_>>()
        );
        let mut notes = Vec::new();
        for (&request, route) in &routes.routes {
            let action = routes.requests[&request].action;
            let inputs = inputs_for(request);
            for &(descriptor, _) in route {
                let Some(ordinal) = sources.controller.descriptors[descriptor as usize].clip else {
                    continue;
                };
                let arm = source.tag(sources.controller.clips[ordinal], Some(0x8080AE01))?;
                if arm.u8(0x181)? != 0 {
                    continue;
                }
                let frames = arm.u16(0x140)?;
                let mechanisms = &self.mechanisms;
                let selected = (|| -> Result<Option<(u32, &'static str, Option<&'static str>)>> {
                    if let Some(found) = explicit(local, &timed, action, &inputs, &lineage)? {
                        return Ok(Some(found));
                    }
                    if let Some((_, choices)) = shared.get(&(descriptor, inputs.clone()))
                        && choices.len() == 1
                    {
                        return Ok(Some((*choices.first().unwrap(), "shared", None)));
                    }
                    let rest = weapon_state(local, idle, &inputs);
                    if timed_actions.contains(&action) {
                        let limit = if resting_actions.contains(&action) {
                            // Without a sampled idle pose nothing is known to return to it.
                            let rest = rest.as_ref().ok().copied().flatten();
                            let idle_pose = rest
                                .and_then(|d| mechanisms.get(&d))
                                .and_then(|m| m.frames.first());
                            Some(
                                &*resting.entry(rest.unwrap_or(u32::MAX)).or_insert_with(|| {
                                    timed
                                        .iter()
                                        .filter(|t| {
                                            idle_pose.is_some_and(|pose| {
                                                rests(&mechanisms[&t.descriptor], pose)
                                            })
                                        })
                                        .map(|t| t.descriptor)
                                        .collect()
                                }),
                            )
                        } else {
                            None
                        };
                        if let Some(d) = paired(&timed, frames, reload_order(action), limit) {
                            return Ok(Some((d, "length", None)));
                        }
                    }
                    Ok(rest?.map(|d| (d, "idle", None)))
                })();
                let chosen = selected.and_then(|chosen| {
                    let Some((local_descriptor, how, named)) = chosen else {
                        return Ok(None);
                    };
                    if let std::collections::btree_map::Entry::Vacant(slot) =
                        self.mechanisms.entry(local_descriptor)
                    {
                        slot.insert(mechanism(
                            source,
                            local,
                            local_descriptor,
                            weapon_rig,
                            &self.bones,
                        )?);
                    }
                    Ok(Some((local_descriptor, how, named)))
                });
                match chosen {
                    Ok(Some((local_descriptor, how, named))) => {
                        self.overlays
                            .insert((request, descriptor), local_descriptor);
                        self.chosen.insert((request, descriptor), how);
                        self.pairings.push((request, json!({"action":format!("{action:08X}"),"request":request,"source_descriptor":descriptor,
                            "inputs":inputs.iter().map(|(b, v)| (b.to_string(), format!("{v:08X}"))).collect::<BTreeMap<_, _>>(),
                            "arm_frames":frames,"weapon_descriptor":format!("{:08X}", self.mechanisms[&local_descriptor].name),
                            "weapon_frames":self.mechanisms[&local_descriptor].frames.len(),"chosen_by":how,
                            "source_name":named})));
                    }
                    Ok(None) => {}
                    Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
                    Err(error) => notes.push(format!("{action:08X}: {error:#}")),
                }
            }
        }
        let reached = self.overlays.values().collect::<BTreeSet<_>>();
        self.report["unreached_descriptors"] = json!(
            local
                .descriptors
                .iter()
                .enumerate()
                .filter(|(i, d)| d.clip.is_some() && !reached.contains(&(*i as u32)))
                .map(|(_, d)| format!("{:08X}", d.name))
                .collect::<Vec<_>>()
        );
        self.report["notes"] = json!(notes);
        Ok(())
    }

    /// The overlay that adds the weapon-local `descriptor`'s motion to an arm clip.
    pub(super) fn overlay(&self, descriptor: u32) -> Overlay<'_> {
        Overlay {
            bones: &self.bones,
            corrections: &self.corrections,
            mechanism: &self.mechanisms[&descriptor],
            native_handle: self.native_handle,
        }
    }
}
