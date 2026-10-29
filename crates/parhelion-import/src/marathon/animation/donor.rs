//! The native donor whose first-person animation best fits the source weapon.
//!
//! Native first-person states differ in how they reload. Every examined Shadowkeep weapon type
//! requests `reload_full` and `reload_empty`, except shotguns: 61 of 63 request `reload_enter`, a
//! shell insert and `chamber_1` instead. A source weapon that reloads by magazine on a shell-fed
//! donor never plays its reload. An exotic donor's runtime keeps its own effects, such as a
//! shotgun's flames, so ordinary donors come first. With source animation, the first ordinary
//! donor in the importer's order that requests every magazine reload the source plays is chosen,
//! else the first such exotic, else the first ordinary donor.
//!
//! Without source animation the donor's own clips play, and each source weapon bone follows the
//! donor equipment bone of its name, else its nearest named ancestor's. Every row of one native
//! attachment owner names the same arms rig, and the row's animation profile decides which clips
//! each action plays. Donors are compared on what both games' data carry, in order: whether the
//! row's hold category is the source's hold class, whether its states request every source
//! magazine reload, how many source parts that move in a source reload follow a donor bone that the
//! donor's own clips for that reload move, and how few still source parts they move. The earliest
//! donor of the best fit wins, so the importer's order decides ties. Shared bone names alone are
//! not compared: a bone no donor clip moves leaves the source part where its ancestor puts it.
//! Exotics are compared only for a type with no ordinary donor.
use super::compile::{PROFILE, RELOAD, RELOADS};
use super::*;
use crate::tiger::animation::state;
use std::{cmp::Reverse, sync::Arc};
mod exotic;
pub(in crate::marathon) use exotic::plain;

/// A weapon part moves when it travels this far from its first-frame place in the weapon root
/// frame, the distance the controller readback requires of a reload, or when it hides or shows.
/// Source clips hide parts at scale 0.01.
const TRAVEL: f32 = 0.02;
const HIDDEN: f32 = 0.5;

/// The native selector operation that tests the attachment row's animation profile.
const ROW_PROFILE: u8 = 3;

/// The weapon parts that move across sampled frames, as indices into each frame.
fn moving(frames: &[Vec<Pose>]) -> BTreeSet<usize> {
    let Some(first) = frames.first() else {
        return BTreeSet::new();
    };
    (0..first.len())
        .filter(|&part| {
            frames.iter().any(|frame| {
                (first[part].0[7] < HIDDEN) != (frame[part].0[7] < HIDDEN)
                    || (0..3)
                        .map(|i| (frame[part].0[4 + i] - first[part].0[4 + i]).powi(2))
                        .sum::<f32>()
                        .sqrt()
                        >= TRAVEL
            })
        })
        .collect()
}

/// The source reloads that carry their own weapon motion, each with the weapon-local descriptors
/// that show it: `reload_empty` and `reload_full` whose weapon lookup names a descriptor after
/// them, or whose first absolute arm clip, under the weapon's profile and reload value, has
/// weapon-local clips within one frame of its length.
fn magazine_reloads(
    source: &mut Reader,
    weapon: &Payload,
    base: usize,
    local: &controller::Controller,
) -> Result<BTreeMap<u32, BTreeSet<u32>>> {
    let profile = weapon.u32(base + 0x314)?;
    let selection = source.tag(source.ref64(weapon, base + 0x338)?, Some(0x808025AA))?;
    let arms = source.ref64(&selection, 0x10)?;
    let lookups = components(source, arms)?
        .into_iter()
        .filter(|o| o.3 == controller::LOOKUP)
        .collect::<Vec<_>>();
    let [lookup] = lookups.as_slice() else {
        return Ok(BTreeMap::new());
    };
    let arm = controller::Controller::read(source, &lookup.1)?;
    let mut lengths = Vec::new();
    for (descriptor, entry) in local.descriptors.iter().enumerate() {
        if let Some(ordinal) = entry.clip
            && entry.rate == 1.
        {
            let clip = source.tag(local.clips[ordinal], Some(0x8080AE01))?;
            lengths.push((u32::try_from(descriptor)?, clip.u16(0x140)?));
        }
    }
    let mut reloads = BTreeMap::new();
    for (action, value) in RELOADS {
        let name = source::fnv(action);
        if !arm.names.contains_key(&name) {
            continue;
        }
        let named = local
            .descriptors
            .iter()
            .enumerate()
            .filter(|(_, d)| d.name == name && d.clip.is_some())
            .map(|(descriptor, _)| u32::try_from(descriptor))
            .collect::<std::result::Result<BTreeSet<_>, _>>()?;
        if !named.is_empty() {
            reloads.insert(name, named);
            continue;
        }
        let mut inputs = BTreeMap::from([(PROFILE, profile)]);
        if arm.declares(RELOAD, source::fnv(value)) {
            inputs.insert(RELOAD, source::fnv(value));
        }
        for (descriptor, _) in arm.route(name, &inputs)? {
            let Some(ordinal) = arm.descriptors[descriptor as usize].clip else {
                continue;
            };
            let clip = source.tag(arm.clips[ordinal], Some(0x8080AE01))?;
            if clip.u8(0x181)? != 0 {
                continue;
            }
            let frames = clip.u16(0x140)?;
            let matched = lengths
                .iter()
                .filter(|(_, f)| f.abs_diff(frames) <= 1)
                .map(|(d, _)| *d)
                .collect::<BTreeSet<_>>();
            if !matched.is_empty() {
                reloads.insert(name, matched);
            }
            break;
        }
    }
    Ok(reloads)
}

/// What the source weapon's own animation shows.
#[derive(Default)]
struct Source {
    /// The hold class at weapon definition `+310`.
    hold: Option<u32>,
    /// Weapon bone names and parents. Parents precede children.
    names: Vec<u32>,
    parents: Vec<Option<usize>>,
    /// Each magazine reload with the weapon-local descriptors that show its motion.
    reloads: BTreeMap<u32, BTreeSet<u32>>,
    /// Each magazine reload whose weapon-local clips sample, with the weapon bones they move.
    moving: BTreeMap<u32, BTreeSet<usize>>,
    notes: Vec<String>,
}

impl Source {
    /// `motion` also reads the weapon hierarchy and samples each magazine reload's weapon-local
    /// clips on the weapon rig.
    fn read(source: &mut Reader, root: u32, motion: bool) -> Result<Self> {
        let owners = components(source, root)?;
        let mut wanted = Self::default();
        if motion
            && let [skeleton] = owners
                .iter()
                .filter(|o| o.3 == source::SKELETON)
                .collect::<Vec<_>>()
                .as_slice()
        {
            let rows = skeleton.1.array(skeleton.2 + 0xB0, 16, Some(0x8080AF42))?;
            for (index, row) in rows.into_iter().enumerate() {
                let parent = usize::try_from(skeleton.1.u32(row + 4)? as i32).ok();
                ensure!(
                    (index == 0) == parent.is_none() && parent.is_none_or(|p| p < index),
                    "Invalid source weapon hierarchy"
                );
                wanted.names.push(skeleton.1.u32(row)?);
                wanted.parents.push(parent);
            }
        }
        let Some((_, weapon, base, _)) = owners.iter().find(|o| o.3 == 0x80804A14) else {
            return Ok(wanted);
        };
        let (weapon, base) = (weapon.clone(), *base);
        wanted.hold = Some(weapon.u32(base + 0x310)?);
        let locals = owners
            .iter()
            .filter(|o| o.3 == controller::LOOKUP)
            .collect::<Vec<_>>();
        let [local] = locals.as_slice() else {
            return Ok(wanted);
        };
        let local = controller::Controller::read(source, &local.1.clone())?;
        wanted.reloads = magazine_reloads(source, &weapon, base, &local)?;
        if motion && !wanted.reloads.is_empty() {
            wanted.sample(source, &owners, &local);
        }
        Ok(wanted)
    }

    /// Record the weapon bones each magazine reload's weapon-local clips move. A clip the weapon
    /// rig cannot sample is noted and left out.
    fn sample(
        &mut self,
        source: &mut Reader,
        owners: &[Component],
        local: &controller::Controller,
    ) {
        let one = |class| owners.iter().find(|o| o.3 == class);
        let (Some(skeleton), Some(controls)) = (one(source::SKELETON), one(source::CONTROLS))
        else {
            self.notes.push("The source weapon rig is absent".into());
            return;
        };
        let rig = match source::Rig::read(&skeleton.1, &controls.1) {
            Ok(rig) if rig.names == self.names => rig,
            Ok(_) => {
                self.notes
                    .push("The source weapon rig differs from its skeleton".into());
                return;
            }
            Err(error) => {
                self.notes.push(format!("Source weapon rig: {error:#}"));
                return;
            }
        };
        let bones = (1..rig.names.len()).map(|b| (b, b)).collect::<Vec<_>>();
        for (&action, descriptors) in &self.reloads {
            let mut parts = None::<BTreeSet<usize>>;
            for &descriptor in descriptors {
                match compile::mechanism(source, local, descriptor, &rig, &bones) {
                    Ok(mechanism) => parts
                        .get_or_insert_default()
                        .extend(moving(&mechanism.frames).into_iter().map(|k| bones[k].1)),
                    Err(error) => self.notes.push(format!(
                        "{action:08X} weapon descriptor {descriptor}: {error:#}"
                    )),
                }
            }
            if let Some(parts) = parts {
                self.moving.insert(action, parts);
            }
        }
    }

    /// The donor bone each source weapon bone follows in donor mode: its own name, else its
    /// nearest named ancestor's, else the donor root.
    fn followed(&self, equipment: &[u32]) -> Vec<u32> {
        let mut follows: Vec<u32> = Vec::with_capacity(self.names.len());
        for (bone, name) in self.names.iter().enumerate() {
            let bone = if equipment.contains(name) {
                *name
            } else {
                self.parents[bone].map_or(equipment[0], |parent| follows[parent])
            };
            follows.push(bone);
        }
        follows
    }

    fn report(&self) -> Value {
        let reloads = self
            .reloads
            .iter()
            .map(|(action, descriptors)| {
                (
                    format!("{action:08X}"),
                    json!({"weapon_descriptors":descriptors,
                        "moving_parts":self.moving.get(action).map(|parts| parts.iter()
                            .map(|&b| format!("{:08X}", self.names[b])).collect::<Vec<_>>())}),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        json!({"hold":self.hold.map(|h| format!("{h:08X}")),"weapon_bones":self.names.len(),
            "magazine_reloads":reloads,"notes":self.notes})
    }
}

/// A native donor's own first-person animation, as its attachment row selects it.
struct Played {
    report: Value,
    entity: u32,
    /// The row's hold category and animation profile, when the content key selects one row.
    category: Option<u32>,
    profile: Option<u32>,
    actions: BTreeSet<u32>,
    states: Arc<Payload>,
    dictionary: Arc<Payload>,
    bank: Arc<Payload>,
    /// Equipment bone names in palette order, which donor-mode geometry skins to.
    equipment: Vec<u32>,
}

impl Played {
    fn read(native: &mut Reader, hash: u32) -> Result<Self> {
        let item = crate::tiger::shadowkeep::item_tag(native, hash)?;
        let (report, rows) = native::rows(native, item)?;
        ensure!(
            rows.iter()
                .map(|r| (r[0], r[1]))
                .collect::<BTreeSet<_>>()
                .len()
                == 1,
            "Native first-person attachment is ambiguous"
        );
        let entity = rows[0][1];
        let (category, profile) = match rows.as_slice() {
            [row] => (Some(row[2]), Some(row[3])),
            _ => (None, None),
        };
        let lookup = native.tag(
            native::owner(&report, "8080344B", entity)?,
            Some(0x80809C36),
        )?;
        let definition = lookup.pointer(24)?;
        let bank = native.tag(lookup.u32(definition + 0x90)?, Some(0x808036F6))?;
        let dictionary = native.tag(lookup.u32(definition + 0x94)?, Some(0x80808EE1))?;
        let states = native.tag(lookup.u32(definition + 0x9C)?, Some(0x80803465))?;
        let actions = states
            .array(8, 8, Some(0x8080342E))?
            .into_iter()
            .map(|at| states.u32(at))
            .collect::<Result<_>>()?;
        let runtime = native::hex(&report["runtime_entity"])?;
        let skeleton = native.tag(
            native::owner(&report, "80808546", runtime)?,
            Some(0x80809C36),
        )?;
        let equipment = native::names(&skeleton)?
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        ensure!(!equipment.is_empty(), "Native equipment skeleton is empty");
        Ok(Self {
            report,
            entity,
            category,
            profile,
            actions,
            states,
            dictionary,
            bank,
            equipment,
        })
    }

    /// The descriptors this row plays for `action`: those its profile selects, else every
    /// descriptor of the action when the dictionary does not resolve the profile.
    fn descriptors(&self, action: u32) -> Result<(BTreeSet<usize>, bool)> {
        if let Some(profile) = self.profile
            && let Ok(descriptors) = state::parameter::descriptors(
                &self.states,
                &self.dictionary,
                action,
                ROW_PROFILE,
                profile,
            )
        {
            return Ok((descriptors, true));
        }
        Ok((state::descriptors(&self.states, action)?, false))
    }

    /// The equipment bones the absolute clips this row plays for `action` move, by name, and
    /// whether the row's profile selected those clips. Clips are cached by arms rig and tag.
    fn moving(
        &self,
        native: &mut Reader,
        arm: &Arm,
        action: u32,
        cache: &mut BTreeMap<(u32, u32), BTreeSet<u32>>,
    ) -> Result<(BTreeSet<u32>, bool)> {
        let (descriptors, selected) = self.descriptors(action)?;
        let rows = self.bank.array(0x68, 32, Some(0x80809002))?;
        let clips = self.bank.array(8, 4, Some(0x80808F48))?;
        let mut moving = BTreeSet::new();
        for descriptor in descriptors {
            let row = *rows
                .get(descriptor)
                .context("Native state names an absent descriptor")?;
            let ordinal = self.bank.u16(row + 24)?;
            if ordinal == u16::MAX {
                continue;
            }
            let tag = self.bank.u32(
                *clips
                    .get(usize::from(ordinal))
                    .context("Native clip ordinal")?,
            )?;
            if let Some(parts) = cache.get(&(self.entity, tag)) {
                moving.extend(parts);
                continue;
            }
            let clip = native.tag(tag, Some(0x80808F49))?;
            let parts = if clip.u8(0x181)? == 0 {
                arm.moving(&clip)?
            } else {
                BTreeSet::new()
            };
            moving.extend(&parts);
            cache.insert((self.entity, tag), parts);
        }
        Ok((moving, selected))
    }
}

/// A native arms rig and the weapon parts below its weapon root.
struct Arm {
    rig: control::Rig,
    controls: Arc<Payload>,
    handle: usize,
    parts: Vec<usize>,
}

impl Arm {
    fn read(native: &mut Reader, played: &Played) -> Result<Self> {
        let skeleton = native.tag(
            native::owner(&played.report, "80808546", played.entity)?,
            Some(0x80809C36),
        )?;
        let controls = native.tag(
            native::owner(&played.report, "80808F8F", played.entity)?,
            Some(0x80809C36),
        )?;
        let rig = control::Rig::read(&skeleton, &controls)?;
        let handle = rig
            .names
            .iter()
            .position(|n| *n == played.equipment[0])
            .context("Native arms rig lacks the weapon root")?;
        let parts = super::cue::parts(&rig, handle);
        Ok(Self {
            rig,
            controls,
            handle,
            parts,
        })
    }

    /// The weapon parts an absolute clip moves, by name. Carrier clips play only the control
    /// groups they share with the rig.
    fn moving(&self, clip: &Payload) -> Result<BTreeSet<u32>> {
        let mut frames = Vec::new();
        for frame in 0..usize::from(clip.u16(0x13C)?) {
            let values =
                native_clip::sample_common(clip, &self.rig.defaults, &self.controls, frame)?;
            let objects = self.rig.objects(&values)?;
            frames.push(
                self.parts
                    .iter()
                    .map(|&part| objects[self.handle].inverse().then(objects[part]))
                    .collect::<Vec<_>>(),
            );
        }
        Ok(moving(&frames)
            .into_iter()
            .map(|k| self.rig.names[self.parts[k]])
            .collect())
    }
}

/// How well a donor's own animation fits the source weapon, compared in field order.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Fit {
    hold: bool,
    reloads: bool,
    moving: usize,
    still: Reverse<usize>,
}

impl Fit {
    /// The first criterion on which a better fit differs from `first`'s, which is named
    /// `name`, and why it favours this one.
    fn decided(&self, first: &Self, name: &str) -> (&'static str, String) {
        if self.hold != first.hold {
            (
                "hold",
                format!("It holds the weapon in the source's hold class and {name} does not."),
            )
        } else if self.reloads != first.reloads {
            (
                "reloads",
                format!("It requests every magazine reload the source plays and {name} does not."),
            )
        } else if self.moving != first.moving {
            (
                "moving_parts",
                format!(
                    "Its reloads move more of the parts the source's reloads move than {name}."
                ),
            )
        } else {
            (
                "still_parts_moved",
                format!(
                    "Its reloads move fewer of the parts the source's reloads keep still than {name}."
                ),
            )
        }
    }
}

/// For one donor, the source parts that move in a source reload and follow a donor bone the
/// donor's clips for that reload move, the still source parts such a bone moves, the donor bones
/// each reload moves, and whether the row's profile selected every clip.
fn motion(
    wanted: &Source,
    native: &mut Reader,
    played: &Played,
    arms: &mut BTreeMap<u32, Arm>,
    cache: &mut BTreeMap<(u32, u32), BTreeSet<u32>>,
) -> Result<(usize, usize, Value, bool)> {
    if let std::collections::btree_map::Entry::Vacant(slot) = arms.entry(played.entity) {
        slot.insert(Arm::read(native, played)?);
    }
    let arm = &arms[&played.entity];
    let follows = wanted.followed(&played.equipment);
    let (mut moving, mut still, mut selected) = (0, 0, true);
    let mut report = serde_json::Map::new();
    for (&action, parts) in &wanted.moving {
        if !played.actions.contains(&action) {
            continue;
        }
        let (moved, by_profile) = played.moving(native, arm, action, cache)?;
        selected &= by_profile;
        for (bone, followed) in follows.iter().enumerate().skip(1) {
            if moved.contains(followed) {
                if parts.contains(&bone) {
                    moving += 1;
                } else {
                    still += 1;
                }
            }
        }
        report.insert(
            format!("{action:08X}"),
            json!(moved.iter().map(|n| format!("{n:08X}")).collect::<Vec<_>>()),
        );
    }
    Ok((moving, still, Value::Object(report), selected))
}

/// The index of the donor to build on and the choice's report. An exotic donor keeps its own
/// runtime effects beneath the import, so ordinary donors come first. With `source_animation`,
/// the first ordinary donor whose first-person states request every magazine reload the source
/// plays, else the first such exotic, else the first ordinary donor. Otherwise the earliest
/// ordinary donor whose own animation best fits the source, and an exotic only for a type with no
/// ordinary donor.
pub(in crate::marathon) fn choose(
    source: &mut Reader,
    native: &mut Reader,
    root: u32,
    donors: &[(u32, String)],
    source_animation: bool,
) -> Result<(usize, Value)> {
    let (ordinary, exotics): (Vec<usize>, Vec<usize>) =
        (0..donors.len()).partition(|&index| !exotic::exotic(native, donors[index].0));
    if source_animation {
        let order = ordinary.iter().chain(&exotics).copied().collect::<Vec<_>>();
        return by_reloads(source, native, root, donors, &order);
    }
    let (allowed, left_out) = if ordinary.is_empty() {
        (exotics, Vec::new())
    } else {
        (ordinary, exotics)
    };
    let (index, mut report) = by_fit(source, native, root, donors, &allowed)?;
    if !left_out.is_empty() {
        let names = left_out
            .iter()
            .map(|&at| json!({"donor":format!("{:08X}", donors[at].0),"name":donors[at].1}))
            .collect::<Vec<_>>();
        report["exotics_left_out"] = json!(names);
    }
    Ok((index, report))
}

/// The earliest of the `allowed` donors whose own animation best fits the source.
fn by_fit(
    source: &mut Reader,
    native: &mut Reader,
    root: u32,
    donors: &[(u32, String)],
    allowed: &[usize],
) -> Result<(usize, Value)> {
    let first = allowed.first().copied().unwrap_or(0);
    if allowed.len() < 2 {
        return Ok((first, json!({"chosen_by":"order"})));
    }
    // Donor mode never needed the source controller, so an unreadable one keeps the order.
    let wanted = match Source::read(source, root, true) {
        Ok(wanted) => wanted,
        Err(error) => {
            return Ok((
                first,
                json!({"chosen_by":"order","reason":format!("The source animation is unreadable: {error:#}")}),
            ));
        }
    };
    let reloads = wanted.reloads.keys().copied().collect::<BTreeSet<_>>();
    let mut rejected = Vec::new();
    let mut compared = Vec::new();
    for &index in allowed {
        let (hash, name) = &donors[index];
        match Played::read(native, *hash) {
            Ok(played) => {
                let fit = Fit {
                    hold: wanted.hold.is_some() && played.category == wanted.hold,
                    reloads: reloads.is_subset(&played.actions),
                    ..Fit::default()
                };
                compared.push((index, played, fit, json!({})));
            }
            Err(error) => rejected.push(
                json!({"donor":format!("{hash:08X}"),"name":name,"reason":format!("{error:#}")}),
            ),
        }
    }
    // Clips are sampled only where motion can decide.
    let best = compared.iter().map(|c| (c.2.hold, c.2.reloads)).max();
    if !wanted.moving.is_empty() {
        let mut arms = BTreeMap::new();
        let mut cache = BTreeMap::new();
        for (index, played, fit, report) in &mut compared {
            if Some((fit.hold, fit.reloads)) != best {
                continue;
            }
            match motion(&wanted, native, played, &mut arms, &mut cache) {
                Ok((moving, still, moved, selected)) => {
                    fit.moving = moving;
                    fit.still = Reverse(still);
                    *report = json!({"moving_parts":moving,"still_parts_moved":still,
                        "donor_moving_parts":moved,"profile_selected":selected});
                }
                Err(error) => {
                    let (hash, name) = &donors[*index];
                    rejected.push(json!({"donor":format!("{hash:08X}"),"name":name,
                        "reason":format!("Reload motion: {error:#}")}));
                    *report = Value::Null;
                }
            }
        }
        compared.retain(|c| !c.3.is_null());
    }
    let mut chosen: Option<usize> = None;
    for (at, candidate) in compared.iter().enumerate() {
        if chosen.is_none_or(|best| candidate.2 > compared[best].2) {
            chosen = Some(at);
        }
    }
    let ranked = compared
        .iter()
        .map(|(index, played, fit, motion)| {
            let mut row = json!({"donor":format!("{:08X}", donors[*index].0),"name":donors[*index].1,
                "category":played.category.map(|c| format!("{c:08X}")),
                "profile":played.profile.map(|p| format!("{p:08X}")),
                "hold":fit.hold,"requests_reloads":fit.reloads});
            for (key, value) in motion.as_object().into_iter().flatten() {
                row[key] = value.clone();
            }
            row
        })
        .collect::<Vec<_>>();
    let mut report = json!({"source":wanted.report(),
        "source_magazine_reloads":reloads.iter().map(|r| format!("{r:08X}")).collect::<Vec<_>>(),
        "compared":ranked,"rejected":rejected});
    let Some(at) = chosen else {
        report["chosen_by"] = json!("order");
        report["reason"] = json!("No donor's first-person animation is readable.");
        return Ok((first, report));
    };
    if at == 0 {
        report["chosen_by"] = json!("order");
        report["reason"] = json!("No donor fits the source's animation better than the first.");
    } else {
        let (by, reason) = compared[at]
            .2
            .decided(&compared[0].2, &donors[compared[0].0].1);
        report["chosen_by"] = json!(by);
        report["reason"] = json!(reason);
    }
    Ok((compared[at].0, report))
}

/// The first donor, in `order`, whose first-person states request every magazine reload the
/// source plays, else the first in `order`.
fn by_reloads(
    source: &mut Reader,
    native: &mut Reader,
    root: u32,
    donors: &[(u32, String)],
    order: &[usize],
) -> Result<(usize, Value)> {
    let first = order.first().copied().unwrap_or(0);
    let reloads = Source::read(source, root, false)?
        .reloads
        .into_keys()
        .collect::<BTreeSet<_>>();
    let names = reloads
        .iter()
        .map(|r| format!("{r:08X}"))
        .collect::<Vec<_>>();
    if reloads.is_empty() || order.len() < 2 {
        return Ok((
            first,
            json!({"source_magazine_reloads":names,"chosen_by":"order"}),
        ));
    }
    let mut rejected = Vec::new();
    for &index in order {
        let (hash, name) = &donors[index];
        let requested = crate::tiger::shadowkeep::item_tag(native, *hash)
            .and_then(|tag| native::requested_actions(native, tag));
        match requested {
            Ok(actions) if reloads.is_subset(&actions) => {
                return Ok((
                    index,
                    json!({"source_magazine_reloads":names,"chosen_by":"reloads","rejected":rejected}),
                ));
            }
            Ok(_) => rejected.push(json!({"donor":format!("{hash:08X}"),"name":name,
                "reason":"Does not request every source magazine reload"})),
            Err(error) => rejected.push(json!({"donor":format!("{hash:08X}"),"name":name,
                "reason":format!("{error:#}")})),
        }
    }
    Ok((
        first,
        json!({"source_magazine_reloads":names,"chosen_by":"order","rejected":rejected,
            "reason":"No donor of this type requests every source magazine reload"}),
    ))
}
