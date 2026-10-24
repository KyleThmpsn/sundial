//! Explicit source character actions selected through a private native parameter.
mod binding;
mod grip;
mod motion;
mod rider;
mod source;
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
) -> Result<Option<Options>> {
    if scene.object.tag.group == "vehi" {
        rider::defaults(cache, scene)
    } else {
        weapon::defaults(scene, native, donor)
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

impl Options {
    fn motions(&self) -> impl Iterator<Item = &Action> {
        self.actions.values().chain(self.stance.values()).chain(
            self.rider
                .iter()
                .flat_map(|rider| [&rider.enter, &rider.idle, &rider.exit]),
        )
    }
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
        bank.array(24, 2, Some(0x80800007))?.len() == clips.len()
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
    animation::state::parameter::extend(&dictionary, builtin, parent, private)?;
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
    let mut converted = Vec::new();
    let mut translate = |native: &mut Reader,
                         index: usize,
                         action: &Action,
                         idle: bool|
     -> Result<usize> {
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
        ensure!(
            matches!((playback, original.u8(0x181)?), (1, 0) | (2, 1 | 2)),
            "Character descriptor {index} and clip composition disagree"
        );
        let mut envelope = original.as_ref().clone();
        if playback == 2 {
            // The imported action already contains a complete absolute pose.
            // Retain overlay playback using native absolute replacement, so an
            // additive donor cannot apply the character's stance a second time.
            crate::presentation::put(&mut envelope.0, 0x181, &[2])?;
        }
        let mut frames = usize::from(original.u16(0x13C)?);
        let mut duration = bank.f32(row + 20)?;
        if idle && frames == 2 {
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
        let motion = calibration.translate(&source.action(action, frames)?, &rig)?;
        let result = animation::clip::replace_for(&envelope, &motion, rig.bind.len(), &controls)?;
        let ordinal = converted.len();
        let file = format!("character/clip-{ordinal}-{index}.bin");
        fs::write(root.join(&file), &result.0)?;
        converted.push(json!({"template":template,"descriptor":index,"file":file,"frames":frames,"duration":duration,"source":action}));
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
            mapped.insert(index, translate(native, index, action, false)?);
        }
        actions.push(json!({"name":name,"label":label,"source":action,"clips":mapped}));
    }
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
            .map(|(name, action)| Ok((Some(action_name(name)?), vec![(0, action)])))
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
            if let Some(stance) = stance {
                parameters.insert(1, stance);
                condition.insert(1u8, stance);
            }
            let mut routes = BTreeMap::new();
            for choice in
                animation::state::parameter::table_choices_for(&selector, &dictionary, &parameters)?
            {
                let mut mappings = BTreeMap::new();
                for &(field, action) in &fields {
                    let mut mapping = BTreeMap::new();
                    for index in animation::motion_table::descriptors(&table, kind, choice, field)?
                    {
                        mapping.insert(
                            index,
                            translate(native, index, action, label == "stance" || field == 16)?,
                        );
                    }
                    mappings.insert(field, mapping);
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
    Ok(
        json!({"schema":2,"bank":bank_tag,"parameters":dictionary_tag,"states":states_tag,
        "layers":layers_tag,"pose":options.native.pose,"directions":directions_tag,
        "skeleton":options.native.skeleton,"controls":options.native.controls,
        "files":files,"clips":converted,"actions":actions,"tables":tables,"binding":binding,"private":private,
        "source_graph":source.graph["tag"],"source_reference":options.reference,
        "weapon_grip":recipe["kind"] == "weapon",
        "retained_native_behavior":["Character geometry and segment lengths", "Native action timing and events", "Native dynamic aim parents", "Native limb and torso constraints", "Native walking and running"],
        "gameplay_verified":false}),
    )
}
