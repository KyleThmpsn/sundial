//! Explicit source character actions selected through a private native parameter.
mod binding;
mod grip;
mod motion;
mod rider;
mod source;
mod swing;
mod timing;
mod weapon;
use super::first_person::Action;
use crate::{
    halo_reach::{cache::Cache, native::ImportRequest, resource::Pages},
    tiger::{animation, payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::{fs, path::Path};

pub(in crate::halo_reach) fn defaults(
    cache: &Cache,
    scene: &crate::halo_reach::Scene,
    native: &mut Reader,
    donor: u32,
    carrier: Option<u32>,
) -> Result<Option<Options>> {
    if scene.object.tag.group == "vehi" {
        rider::defaults(cache, scene, native, carrier)
    } else {
        weapon::defaults(cache, scene, native, donor)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub graph: String,
    pub model: String,
    /// Absolute source stance supplying omitted replacement channels.
    pub reference: String,
    /// Native action name to an absolute source action or ordered sequence.
    #[serde(default)]
    pub actions: BTreeMap<String, Action>,
    /// Native stance name to a source idle, such as stand or crouch.
    #[serde(default)]
    pub stance: BTreeMap<String, Action>,
    /// Source entry, mounted pose and exit for the native driver transition table.
    #[serde(default)]
    pub rider: Option<Rider>,
    /// Source seat roles apart from the driver's, each with its entry, mounted pose and exit,
    /// under a private rider role of its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passengers: Vec<Passenger>,
    /// Source movement cycles by native stance name, then by direction (`front`, `back`,
    /// `left` or `right`). Each speed tier of that stance's native walking and running rows
    /// plays the candidate of its direction whose stride best matches the tier.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub locomotion: BTreeMap<String, BTreeMap<String, Vec<Action>>>,
    /// Additional source bone names mapped to native names.
    #[serde(default)]
    pub bone_aliases: BTreeMap<String, String>,
    pub native: Controller,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rider {
    pub enter: Action,
    pub idle: Action,
    pub exit: Action,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Passenger {
    /// The source seat's animation role, such as `warthog_p`.
    pub role: String,
    pub rider: Rider,
}

/// The stock seated turret's seat role, which the player's vehicle role group names. A seated
/// child's private role inherits it.
const SEATED_ROLE: u32 = 0xD639_5ACA;

/// The private rider role of source seat role `role` on the vehicle imported as `slug`.
pub(in crate::halo_reach) fn passenger_role(slug: &str, role: &str) -> u32 {
    name(&format!("reach/character/{slug}/{role}"))
}

impl Options {
    fn motions(&self) -> impl Iterator<Item = &Action> {
        self.actions
            .values()
            .chain(self.stance.values())
            .chain(
                self.rider
                    .iter()
                    .chain(self.passengers.iter().map(|passenger| &passenger.rider))
                    .flat_map(|rider| [&rider.enter, &rider.idle, &rider.exit]),
            )
            .chain(self.locomotion.values().flatten().flat_map(|(_, v)| v))
    }
}

/// How a translated clip samples its source action.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Action,
    Idle,
    Gait,
}

/// The source direction name of a native locomotion unit vector.
fn direction(vector: [f32; 2]) -> Result<&'static str> {
    Ok(match vector {
        [x, _] if x > 0.99 => "front",
        [x, _] if x < -0.99 => "back",
        [_, y] if y > 0.99 => "left",
        [_, y] if y < -0.99 => "right",
        _ => anyhow::bail!("Locomotion direction is not axis aligned"),
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Controller {
    pub skeleton: u32,
    pub controls: u32,
    pub lookup: u32,
    pub pose: u32,
}

fn name(text: &str) -> u32 {
    text.bytes()
        .fold(0x811C9DC5, |h, b| h.wrapping_mul(0x01000193) ^ u32::from(b))
}

fn action_name(text: &str) -> Result<u32> {
    if let Some(hex) = text.strip_prefix("0x") {
        ensure!(
            hex.len() == 8,
            "Native character action hash requires eight hexadecimal digits"
        );
        Ok(u32::from_str_radix(hex, 16)?)
    } else {
        Ok(name(text))
    }
}

pub(in crate::halo_reach) fn prepare(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    entry: &ImportRequest,
    graph: &mut Value,
    recipe: &mut Value,
    root: &Path,
) -> Result<Value> {
    let options = entry
        .character
        .as_ref()
        .context("Character import options are absent")?;
    let source_root = root
        .parent()
        .context("Character import root")?
        .join("source");
    let source = source::Source::read(cache, pages, options, &source_root)?;
    let skeleton = native.tag(options.native.skeleton, Some(0x80809C36))?;
    let controls = native.tag(options.native.controls, Some(0x80809C36))?;
    let rig = animation::control::Rig::read_character(&skeleton, &controls)?;
    let calibration = motion::Calibration::new(
        &source,
        &rig,
        &skeleton,
        &controls,
        options,
        (recipe["kind"].as_str().unwrap_or("weapon") == "weapon").then_some(entry.scale),
    )?;
    let lookup = native.tag(options.native.lookup, Some(0x80809C36))?;
    let lookup_at = lookup.pointer(24)?;
    ensure!(
        lookup.u32(lookup_at - 4)? == 0x8080344B,
        "Character lookup has an unsupported native class"
    );
    let bank_tag = lookup.u32(lookup_at + 0x90)?;
    let dictionary_tag = lookup.u32(lookup_at + 0x94)?;
    let states_tag = lookup.u32(lookup_at + 0x9C)?;
    let bank = native.tag(bank_tag, Some(0x808036F6))?;
    let dictionary = native.tag(dictionary_tag, Some(0x80808EE1))?;
    let states = native.tag(states_tag, Some(0x80803465))?;
    let pose = native.tag(options.native.pose, Some(0x80809C36))?;
    let pose_at = pose.pointer(24)?;
    ensure!(
        pose.u32(pose_at - 4)? == 0x80803640 && pose.u32(pose_at + 0x108)? == bank_tag,
        "Character pose and lookup must share one native bank"
    );
    let layers_tag = pose.u32(pose_at + 0x11C)?;
    let layers = native.tag(layers_tag, None)?;
    let directions_tag = pose.u32(pose_at + 0x114)?;
    let directions = native.tag(directions_tag, Some(0x8080348E))?;
    let descriptors = bank.array(0x68, 32, Some(0x80809002))?;
    let clips = bank.array(8, 4, Some(0x80808F48))?;
    ensure!(
        bank.array(24, 4, Some(0x80800007))?.len() == clips.len()
            && layers.array(40, 2, Some(0x80800006))?.len() == descriptors.len()
            && directions.array(8, 2, Some(0x80800006))?.len() == descriptors.len()
            && directions.array(0x38, 2, Some(0x80800006))?.len() == descriptors.len(),
        "Character bank arrays disagree"
    );
    let private = name(&format!("reach/character/{}", entry.slug));
    let binding = binding::prepare(native, entry, graph, recipe, root, private)?;
    let builtin = u8::try_from(binding["builtin"].as_u64().context("Character parameter")?)?;
    let parent = u32::try_from(
        binding["parent"]
            .as_u64()
            .context("Character parent profile")?,
    )?;
    // A weapon profile publishes its private name as an alias of the donor's profile. The
    // weapon profile group has 123 of its 127 hierarchy bits in use, while an alias takes none
    // and matches every stock selector as its parent does. Private branches name it exactly.
    if binding["alias"] == true {
        animation::state::parameter::alias(&dictionary, builtin, parent, private)?;
    } else {
        animation::state::parameter::extend(&dictionary, builtin, parent, private)?;
    }
    fs::create_dir_all(root.join("character"))?;
    let mut files = BTreeMap::new();
    for (key, payload) in [
        ("skeleton", &*skeleton),
        ("controls", &*controls),
        ("bank", &*bank),
        ("parameters", &*dictionary),
        ("states", &*states),
        ("layers", &*layers),
        ("pose", &*pose),
        ("directions", &*directions),
    ] {
        let file = format!("character/{key}.bin");
        fs::write(root.join(&file), &payload.0)?;
        files.insert(key.to_owned(), file);
    }
    // The weapon's first-person dictionary takes the private profile as an alias of the
    // donor's, so first-person selection keeps the donor's choices.
    let first_person_parameters = binding["first_person_parameters"]
        .as_u64()
        .map(u32::try_from)
        .transpose()?;
    if let Some(tag) = first_person_parameters {
        let parameters = native.tag(tag, Some(0x80808EE1))?;
        ensure!(
            tag != dictionary_tag,
            "First-person and character dictionaries are the same tag"
        );
        animation::state::parameter::alias(&parameters, 3, parent, private)?;
        let file = "character/first_person_parameters.bin".to_owned();
        fs::write(root.join(&file), &parameters.0)?;
        files.insert("first_person_parameters".into(), file);
    }
    let mut converted = Vec::new();
    let mut translated = BTreeMap::<(usize, String, u8), usize>::new();
    let mut translate = |native: &mut Reader,
                         index: usize,
                         action: &Action,
                         role: Role|
     -> Result<usize> {
        let key = (index, serde_json::to_string(action)?, role as u8);
        if let Some(&ordinal) = translated.get(&key) {
            return Ok(ordinal);
        }
        let row = *descriptors
            .get(index)
            .context("Character descriptor is outside its bank")?;
        let playback = bank.u16(row + 26)?;
        ensure!(
            bank.bytes::<16>(row)? == [0; 16] && matches!(playback, 1 | 2),
            "Character descriptor {index} requires unsupported playback {playback}"
        );
        let clip = *clips
            .get(usize::from(bank.u16(row + 24)?))
            .context("Character clip is outside its bank")?;
        let template = bank.u32(clip)?;
        let original = native.tag(template, Some(0x80808F49))?;
        let composition = original.u8(0x181)?;
        ensure!(
            matches!((playback, composition), (1, 0) | (2, 1 | 2)),
            "Character descriptor {index} and clip composition disagree"
        );
        // A source overlay on an additive native overlay stays additive, so the
        // character's current stance and movement remain underneath it.
        let additive = playback == 2 && composition == 1 && source.overlay(action)?;
        let mut envelope = original.as_ref().clone();
        if playback == 2 && !additive {
            // The imported action already contains a complete absolute pose.
            // Retain overlay playback using native absolute replacement, so an
            // additive donor cannot apply the character's stance a second time.
            crate::presentation::put(&mut envelope.0, 0x181, &[2])?;
        }
        let mut frames = usize::from(original.u16(0x13C)?);
        let mut duration = bank.f32(row + 20)?;
        if role == Role::Idle && frames == 2 {
            ensure!(
                envelope.u64(0x160)? == 0 && duration > 0.,
                "Static rider pose has events or an invalid duration"
            );
            frames = source.frames(action)?;
            ensure!(
                (2..=3600).contains(&frames),
                "Character idle exceeds native frame capacity"
            );
            duration *= (frames - 1) as f32;
            envelope = animation::clip::grow_static_envelope(&envelope, frames)?;
        }
        let (samples, receipt) = if role == Role::Gait {
            ensure!(
                playback == 1 && composition == 0,
                "Locomotion tier {index} requires an absolute native clip"
            );
            // A tier repeats every duration. Its last frame either repeats the first
            // or is the last distinct frame of the pass.
            let spans = (duration * 30.).round() as usize;
            let period = if spans + 1 == frames {
                spans
            } else if spans == frames {
                frames
            } else {
                anyhow::bail!("Locomotion tier {index} duration disagrees with its frames")
            };
            (
                source.cycle(action, frames, period)?,
                json!({"mode":"cycle","period":period,"retained":"Native duration, events, speed tiers and actor movement"}),
            )
        } else {
            let timing =
                if role == Role::Action && builtin == 3 && playback == 1 && composition == 0 {
                    timing::contact(&source.graph, action, &original)?
                } else {
                    timing::Fit::linear(
                        "Native stance, rider or overlay keeps its original motion fit",
                    )
                };
            (
                source.action(action, frames, timing.contact)?,
                timing.receipt,
            )
        };
        let motion = calibration.translate(&samples, &rig)?;
        let result = if additive {
            // Native mode one multiplies the current rotation by the supplied one and adds
            // the supplied translation in control space, so each delta is taken against
            // the translated stance the source overlay composes onto.
            let base = calibration.translate(&[source.base()], &rig)?;
            let deltas = motion
                .iter()
                .zip(&base)
                .map(|(track, base)| {
                    let base = animation::pose::Pose(base[0]);
                    track
                        .iter()
                        .map(|&value| {
                            let rotation = base.inverse().then(animation::pose::Pose(value));
                            Ok(animation::pose::Pose::checked([
                                rotation.0[0],
                                rotation.0[1],
                                rotation.0[2],
                                rotation.0[3],
                                value[4] - base.0[4],
                                value[5] - base.0[5],
                                value[6] - base.0[6],
                                value[7] / base.0[7],
                            ])?
                            .0)
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .collect::<Result<Vec<_>>>()?;
            animation::clip::replace_deltas_for(&envelope, &deltas, rig.bind.len(), &controls)?
        } else {
            animation::clip::replace_for(&envelope, &motion, rig.bind.len(), &controls)?
        };
        let ordinal = converted.len();
        let file = format!("character/clip-{ordinal}-{index}.bin");
        fs::write(root.join(&file), &result.0)?;
        converted.push(json!({"template":template,"descriptor":index,"file":file,"frames":frames,"duration":duration,"source":action,"timing":receipt,"additive":additive}));
        translated.insert(key, ordinal);
        Ok(ordinal)
    };
    let mut actions = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for (label, action) in &options.actions {
        let name = action_name(label)?;
        ensure!(
            names.insert(name),
            "Requested character action names collide"
        );
        let mut mapped = BTreeMap::new();
        for index in
            animation::state::parameter::descriptors(&states, &dictionary, name, builtin, parent)?
        {
            mapped.insert(index, translate(native, index, action, Role::Action)?);
        }
        // The resolved clip names the action for swing audio, whichever way it was selected.
        let clip = match action.names(&source.graph)?.as_slice() {
            [clip] => json!(clip),
            _ => Value::Null,
        };
        actions.push(json!({"name":name,"label":label,"source":action,"clip":clip,"clips":mapped}));
    }
    ensure!(
        options.locomotion.is_empty() || builtin == 3,
        "Locomotion requires a weapon attachment profile"
    );
    for (stance, directions) in &options.locomotion {
        ensure!(
            options.stance.contains_key(stance),
            "Locomotion names a stance without an idle"
        );
        for direction in directions.keys() {
            ensure!(
                matches!(direction.as_str(), "front" | "back" | "left" | "right"),
                "Unknown locomotion direction {direction}"
            );
        }
    }
    let height = calibration.scale();
    let MotionTables { gaits, mut tables } = motion_tables(
        native,
        (options, &source, height),
        (builtin, parent, &dictionary),
        (&lookup, lookup_at),
        (&mut files, root),
        &mut translate,
    )?;
    let roles = passenger_tables(
        native,
        entry,
        options,
        (builtin, &dictionary),
        (&lookup, lookup_at),
        (&mut files, root, &mut tables),
        &mut translate,
    )?;
    let swings = if builtin == 3 && entry.source.audio.is_some() {
        swing::prepare(
            native,
            options,
            &source.graph,
            &actions,
            &mut converted,
            graph,
            root,
            private,
        )?
    } else {
        json!({"status":"native","reason":"Swing recordings need a weapon profile and configured source audio"})
    };
    let mut retained = vec![
        "Character geometry and segment lengths",
        "Native action timing, and events other than replaced swing presentation",
        "Native dynamic aim parents",
        "Native limb and torso constraints",
        "Native turning in place",
        "Native actor speed and direction. Source root movement only selects each tier's stride",
    ];
    if !gaits.iter().any(|gait| gait.get("status").is_none()) {
        retained.push("Native walking and running");
    }
    let mut result = json!({"schema":2,"bank":bank_tag,"parameters":dictionary_tag,"states":states_tag,
        "layers":layers_tag,"pose":options.native.pose,"directions":directions_tag,
        "skeleton":options.native.skeleton,"controls":options.native.controls,
        "files":files,"clips":converted,"actions":actions,"tables":tables,"binding":binding,"private":private,"roles":roles,
        "source_graph":source.graph["tag"],"source_reference":options.reference,
        "weapon_grip":recipe["kind"] == "weapon","swing_audio":swings,"locomotion":gaits,
        "retained_native_behavior":retained,
        "gameplay_verified":false});
    if let Some(tag) = first_person_parameters {
        result["first_person_parameters"] = json!(tag);
    }
    Ok(result)
}

type TranslateClip<'a> = dyn FnMut(&mut Reader, usize, &Action, Role) -> Result<usize> + 'a;

struct MotionTables {
    gaits: Vec<Value>,
    tables: Vec<Value>,
}

fn motion_tables(
    native: &mut Reader,
    (options, source, height): (&Options, &source::Source, f32),
    (builtin, parent, dictionary): (u8, u32, &Payload),
    (lookup, lookup_at): (&Payload, usize),
    (files, root): (&mut BTreeMap<String, String>, &Path),
    translate: &mut TranslateClip<'_>,
) -> Result<MotionTables> {
    let mut gaits = Vec::new();
    let mut tables = Vec::new();
    let mut requested = Vec::new();
    if !options.stance.is_empty() {
        ensure!(
            builtin == 3,
            "A weapon stance requires its attachment profile"
        );
        let stances = options
            .stance
            .iter()
            .map(|(name, action)| Ok((Some((name, action_name(name)?)), vec![(0, action)])))
            .collect::<Result<Vec<_>>>()?;
        requested.push((
            "stance",
            0xB4,
            0x8080345F,
            animation::motion_table::Kind::Stance,
            stances,
        ));
    }
    if let Some(rider) = &options.rider {
        ensure!(
            builtin == 2,
            "Rider transitions require a private driver role"
        );
        requested.push((
            "rider",
            0x10C,
            0x8080344D,
            animation::motion_table::Kind::Rider,
            vec![(
                None,
                vec![(0, &rider.enter), (16, &rider.idle), (32, &rider.exit)],
            )],
        ));
    }
    for (label, field, class, kind, conditions) in requested {
        let table_tag = lookup.u32(lookup_at + field)?;
        let selector_tag = lookup.u32(lookup_at + field + 4)?;
        let table = native.tag(table_tag, Some(class))?;
        let selector = native.tag(selector_tag, Some(0x80803478))?;
        let mut branches = Vec::new();
        for (stance, fields) in conditions {
            let mut parameters = BTreeMap::from([(builtin, parent)]);
            let mut condition = BTreeMap::new();
            if let Some((_, stance)) = stance {
                parameters.insert(1, stance);
                condition.insert(1u8, stance);
            }
            let movement = stance.and_then(|(name, _)| options.locomotion.get(name));
            let mut routes = BTreeMap::new();
            for choice in
                animation::state::parameter::table_choices_for(&selector, dictionary, &parameters)?
            {
                let mut mappings = BTreeMap::new();
                for &(field, action) in &fields {
                    let mut mapping = BTreeMap::new();
                    for index in animation::motion_table::descriptors(&table, kind, choice, field)?
                    {
                        let role = if label == "stance" || field == 16 {
                            Role::Idle
                        } else {
                            Role::Action
                        };
                        mapping.insert(index, translate(native, index, action, role)?);
                    }
                    mappings.insert(field, mapping);
                }
                if let Some(movement) = movement
                    && let Some(mapping) = map_gaits(
                        native,
                        &table,
                        choice,
                        movement,
                        (source, height),
                        translate,
                        &mut gaits,
                    )?
                {
                    mappings.insert(animation::motion_table::GAITS, mapping);
                }
                routes.insert(choice, mappings);
            }
            branches.push(json!({"conditions":condition,"routes":routes}));
        }
        for (suffix, payload) in [("table", &*table), ("selector", &*selector)] {
            let key = format!("{label}_{suffix}");
            let file = format!("character/{key}.bin");
            fs::write(root.join(&file), &payload.0)?;
            files.insert(key, file);
        }
        tables.push(json!({"kind":label,"table":table_tag,"selector":selector_tag,"class":class,"branches":branches}));
    }
    Ok(MotionTables { gaits, tables })
}

type PassengerTables<'a> = (
    &'a mut BTreeMap<String, String>,
    &'a Path,
    &'a mut Vec<Value>,
);

fn passenger_tables(
    native: &mut Reader,
    entry: &ImportRequest,
    options: &Options,
    (builtin, dictionary): (u8, &Payload),
    (lookup, lookup_at): (&Payload, usize),
    (files, root, tables): PassengerTables<'_>,
    translate: &mut TranslateClip<'_>,
) -> Result<Vec<Value>> {
    // Each seated child's private rider role descends from the stock seated turret's role, and
    // its branch of the rider table names it. The vehicle writes the same name onto that seat.
    let mut roles = Vec::new();
    if !options.passengers.is_empty() {
        ensure!(
            builtin == 2
                && animation::state::parameter::contains(dictionary, builtin, SEATED_ROLE)?,
            "Passenger transitions require the seated turret role in the vehicle role group"
        );
        let table_tag = lookup.u32(lookup_at + 0x10C)?;
        let selector_tag = lookup.u32(lookup_at + 0x110)?;
        let table = native.tag(table_tag, Some(0x8080344D))?;
        let selector = native.tag(selector_tag, Some(0x80803478))?;
        let mut branches = Vec::new();
        for passenger in &options.passengers {
            let role = passenger_role(&entry.slug, &passenger.role);
            animation::state::parameter::extend(dictionary, builtin, SEATED_ROLE, role)?;
            let parameters = BTreeMap::from([(builtin, SEATED_ROLE)]);
            let mut routes = BTreeMap::new();
            for choice in
                animation::state::parameter::table_choices_for(&selector, dictionary, &parameters)?
            {
                let mut mappings = BTreeMap::new();
                for (field, action) in [
                    (0, &passenger.rider.enter),
                    (16, &passenger.rider.idle),
                    (32, &passenger.rider.exit),
                ] {
                    let kind = if field == 16 {
                        Role::Idle
                    } else {
                        Role::Action
                    };
                    let mut mapping = BTreeMap::new();
                    for index in animation::motion_table::descriptors(
                        &table,
                        animation::motion_table::Kind::Rider,
                        choice,
                        field,
                    )? {
                        mapping.insert(index, translate(native, index, action, kind)?);
                    }
                    if !mapping.is_empty() {
                        mappings.insert(field, mapping);
                    }
                }
                if !mappings.is_empty() {
                    routes.insert(choice, mappings);
                }
            }
            roles.push(
                json!({"role":passenger.role,"private":role,"parent":SEATED_ROLE,
                "translated":!routes.is_empty()}),
            );
            if !routes.is_empty() {
                branches.push(json!({"conditions":{},"routes":routes,"private":role}));
            }
        }
        if !branches.is_empty() {
            if let Some(rider) = tables.iter_mut().find(|table| table["kind"] == "rider") {
                rider["branches"]
                    .as_array_mut()
                    .context("Character rider branches")?
                    .extend(branches);
            } else {
                for (suffix, payload) in [("table", &*table), ("selector", &*selector)] {
                    let key = format!("rider_{suffix}");
                    let file = format!("character/{key}.bin");
                    fs::write(root.join(&file), &payload.0)?;
                    files.insert(key, file);
                }
                tables.push(
                    json!({"kind":"rider","table":table_tag,"selector":selector_tag,
                    "class":0x8080344Du32,"branches":branches}),
                );
            }
        }
    }
    Ok(roles)
}

fn map_gaits(
    native: &mut Reader,
    table: &Payload,
    choice: usize,
    movement: &BTreeMap<String, Vec<Action>>,
    (source, height): (&source::Source, f32),
    translate: &mut TranslateClip<'_>,
    gaits: &mut Vec<Value>,
) -> Result<Option<BTreeMap<usize, usize>>> {
    // Every tier of the row takes the source cycle of its direction whose
    // stride, at the character's height, best matches the distance the native
    // tier covers in one pass. A direction without candidates keeps the row's
    // native movement whole.
    let tiers = animation::motion_table::gaits(table, choice)?;
    let mut selected = Vec::new();
    for tier in &tiers {
        let name = direction(tier.direction)?;
        let target = tier.speed * tier.duration;
        let mut best = None::<(&Action, f32, f32)>;
        for candidate in movement.get(name).into_iter().flatten() {
            let Some(stride) = source.stride(candidate)? else {
                continue;
            };
            let length = stride * height;
            if target <= 0. || length <= 0. {
                continue;
            }
            let cost = (length / target).ln().abs();
            if best.is_none_or(|(_, _, best)| cost < best) {
                best = Some((candidate, length, cost));
            }
        }
        selected.push((tier, name, best));
    }
    if selected.iter().all(|(_, _, best)| best.is_some()) {
        let mut mapping = BTreeMap::new();
        for (tier, name, best) in selected {
            let (action, length, _) = best.unwrap();
            let ordinal = translate(native, tier.descriptor, action, Role::Gait)?;
            ensure!(
                mapping
                    .insert(tier.descriptor, ordinal)
                    .is_none_or(|old| old == ordinal),
                "Locomotion descriptor {} plays different source cycles",
                tier.descriptor
            );
            gaits.push(
                json!({"choice":choice,"descriptor":tier.descriptor,"direction":name,
                    "speed":tier.speed,"duration":tier.duration,"source":action,
                    "source_stride":length,"stride_ratio":length / (tier.speed * tier.duration)}),
            );
        }
        Ok(Some(mapping))
    } else {
        gaits.push(json!({"choice":choice,"status":"native",
                "reason":"A direction of this row has no source cycle with planar root movement"}));
        Ok(None)
    }
}
