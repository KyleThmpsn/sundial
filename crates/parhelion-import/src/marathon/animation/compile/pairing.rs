//! Weapon-local motion. The weapon root's own lookup animates the weapon model's bones. The arm
//! clip places the weapon root, and each absolute arm clip takes one weapon-local descriptor for
//! the bones below it: the weapon state named like the native action, else the descriptor so
//! named, else the choice another action made for the same arm descriptor by either rule, else
//! the closing descriptor whose clip length matches the arm clip, else the weapon's idle state.
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
    /// The weapon-local descriptor by action and source arm descriptor.
    pub(super) overlays: BTreeMap<(u32, u32), u32>,
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
        let local_lookups = sources
            .owners
            .iter()
            .filter(|o| o.3 == controller::LOOKUP)
            .collect::<Vec<_>>();
        let mut report = json!({"lookup":Value::Null,"bones":bones.len(),"notes":[]});
        let local = match local_lookups.as_slice() {
            _ if bones.is_empty() => None,
            [owner] => {
                report["lookup"] = json!(owner.0);
                match controller::Controller::read(source, &owner.1) {
                    Ok(local) => Some(local),
                    Err(error) => {
                        report["notes"] = json!([format!("Weapon lookup: {error:#}")]);
                        None
                    }
                }
            }
            [] => None,
            _ => anyhow::bail!("Source weapon has several action lookups"),
        };
        let mut pairing = Self {
            native_handle,
            bones,
            corrections,
            overlays: BTreeMap::new(),
            pairings: Vec::new(),
            mechanisms: BTreeMap::new(),
            report,
        };
        if let Some(local) = &local {
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
        let (weapon_rig, profile) = (&sources.weapon_rig, sources.profile);
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
            let Ok(sampled) = mechanism(source, local, descriptor, weapon_rig, &self.bones) else {
                continue;
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
        let movement = MOVEMENT
            .iter()
            .map(|a| source::fnv(a))
            .collect::<BTreeSet<_>>();
        let reload_order = |action: u32| {
            [source::fnv("reload_empty"), source::fnv("reload_full")]
                .iter()
                .position(|r| *r == action)
        };
        // Actions that route one arm descriptor share its best-supported weapon descriptor.
        let rank = |how: &str| {
            ["state", "name", "length", "idle"]
                .iter()
                .position(|h| *h == how)
        };
        let mut shared = BTreeMap::<u32, (u32, &'static str)>::new();
        for (&action, route) in &routes.routes {
            let mut inputs = BTreeMap::new();
            if local.declares(PROFILE, profile) {
                inputs.insert(PROFILE, profile);
            }
            for &(descriptor, _) in route {
                let found = if let Ok(Some(d)) = weapon_state(local, action, &inputs) {
                    Some((d, "state"))
                } else {
                    weapon_named(local, &timed, action).map(|d| (d, "name"))
                };
                if let Some(found) = found
                    && shared
                        .get(&descriptor)
                        .is_none_or(|s| rank(found.1) < rank(s.1))
                {
                    shared.insert(descriptor, found);
                }
            }
        }
        let mut notes = Vec::new();
        for (&action, route) in &routes.routes {
            let mut inputs = BTreeMap::new();
            if local.declares(PROFILE, profile) {
                inputs.insert(PROFILE, profile);
            }
            if let Some(&value) = routes.reloads.get(&action)
                && local.declares(RELOAD, value)
            {
                inputs.insert(RELOAD, value);
            }
            for &(descriptor, _) in route {
                let Some(ordinal) = sources.controller.descriptors[descriptor as usize].clip else {
                    continue;
                };
                let arm = source.tag(sources.controller.clips[ordinal], Some(0x8080AE01))?;
                if arm.u8(0x181)? != 0 {
                    continue;
                }
                let frames = arm.u16(0x140)?;
                let chosen = (|| -> Result<Option<(u32, &'static str)>> {
                    if let Some(d) = weapon_state(local, action, &inputs)? {
                        return Ok(Some((d, "state")));
                    }
                    if let Some(d) = weapon_named(local, &timed, action) {
                        return Ok(Some((d, "name")));
                    }
                    if let Some(&(d, _)) = shared.get(&descriptor) {
                        return Ok(Some((d, "shared")));
                    }
                    if !movement.contains(&action)
                        && let Some(d) = paired(&timed, frames, reload_order(action))
                    {
                        return Ok(Some((d, "length")));
                    }
                    Ok(weapon_state(local, idle, &inputs)?.map(|d| (d, "idle")))
                })()
                .and_then(|chosen| {
                    let Some((local_descriptor, how)) = chosen else {
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
                    Ok(Some((local_descriptor, how)))
                });
                match chosen {
                    Ok(Some((local_descriptor, how))) => {
                        self.overlays.insert((action, descriptor), local_descriptor);
                        self.pairings.push((action, json!({"action":format!("{action:08X}"),"source_descriptor":descriptor,
                            "arm_frames":frames,"weapon_descriptor":format!("{:08X}", self.mechanisms[&local_descriptor].name),
                            "weapon_frames":self.mechanisms[&local_descriptor].frames.len(),"chosen_by":how})));
                    }
                    Ok(None) => {}
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
