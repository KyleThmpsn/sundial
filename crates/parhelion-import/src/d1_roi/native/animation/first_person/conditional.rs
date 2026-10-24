//! Specialize invariant source actions through private descriptors, preserving native aliases.
use super::*;
use crate::{presentation::append, tiger::animation::state};
mod selection;

pub(super) struct Prepared {
    pub bank: Option<Payload>,
    pub clips: Vec<Value>,
    pub files: BTreeMap<String, String>,
    pub states: Value,
    pub layers: Value,
    pub report: Value,
}

impl Default for Prepared {
    fn default() -> Self {
        Self {
            bank: None,
            clips: Vec::new(),
            files: BTreeMap::new(),
            states: json!({"status":"native"}),
            layers: Value::Null,
            report: Value::Null,
        }
    }
}

#[derive(PartialEq, Eq)]
struct Choice {
    descriptor: usize,
    tag: u32,
    weight: u32,
}
struct Action {
    conditional: bool,
    index: usize,
    choices: Vec<Choice>,
}

fn action(
    states: &Payload,
    bank: &Payload,
    node: usize,
    source: bool,
    fixed: &BTreeMap<u8, Vec<u8>>,
) -> Result<Action> {
    let at = states.pointer(node + 8)?;
    let conditional = states.u64(node)? == 2;
    let (choice, index) = match states.u64(node)? {
        1 => (at, 0),
        2 => {
            ensure!(
                states.u32(at - 4)? == if source { 0x8080200E } else { 0x80803432 },
                "Conditional state payload class differs"
            );
            // Both payloads are 120 bytes. Source array markers can occupy +0x7C,
            // so allocation padding cannot be included in the selector tail.
            ensure!(
                states
                    .0
                    .get(at + 64..at + 120)
                    .context("Conditional selector tail")?
                    .iter()
                    .all(|&b| b == 0),
                "Conditional action has unsupported trailing data"
            );
            // A shared controller can declare inputs used only by other profiles.
            // Selection rejects an unknown input only when the chosen path reaches it.
            let parameters = states.array(
                at + 16,
                1,
                Some(if source { 0x8080440D } else { 0x80808DA4 }),
            )?;
            ensure!(
                !parameters.is_empty(),
                "Conditional action has no parameters"
            );
            let index = character::selection::select_at(states, at + 16, source, fixed)?;
            (
                *states
                    .array(at, 16, Some(if source { 0x80802A9D } else { 0x80803437 }))?
                    .get(index)
                    .context("Selected action choice")?,
                index,
            )
        }
        kind => anyhow::bail!("Unsupported action state kind {kind}"),
    };
    let descriptors = bank.array(0x68, 32, Some(if source { 0x80800577 } else { 0x80809002 }))?;
    let clips = bank.array(8, 4, Some(if source { 0x8080408D } else { 0x80808F48 }))?;
    let mut choices = Vec::new();
    let mut previous = 0.;
    for row in states.array(
        choice,
        8,
        Some(if source { 0x80802831 } else { 0x80803439 }),
    )? {
        let weight = states.f32(row + 4)?;
        ensure!(
            weight.is_finite() && weight >= previous && weight <= 1.,
            "Invalid conditional action weight"
        );
        previous = weight;
        let descriptor = states.u32(row)? as usize;
        let d = *descriptors
            .get(descriptor)
            .context("Conditional descriptor")?;
        ensure!(
            bank.bytes::<16>(d)? == [0; 16],
            "Conditional descriptor has inline tracks"
        );
        let index = descriptor_index(bank, d)?
            .context("Conditional action contains an unbound descriptor")?;
        choices.push(Choice {
            descriptor,
            tag: bank.u32(*clips.get(index).context("Conditional clip")?)?,
            weight: weight.to_bits(),
        });
    }
    ensure!(
        !choices.is_empty() && previous == 1.,
        "Conditional choices do not cover the interval"
    );
    Ok(Action {
        conditional,
        index,
        choices,
    })
}

fn names(p: &Payload, source: bool) -> Result<BTreeMap<u32, usize>> {
    let nodes = p.array(24, 16, Some(if source { 0x808023C9 } else { 0x8080342F }))?;
    let mut result = BTreeMap::new();
    for row in p.array(8, 8, Some(if source { 0x80802153 } else { 0x8080342E }))? {
        ensure!(
            result
                .insert(
                    p.u32(row)?,
                    *nodes
                        .get(p.u32(row + 4)? as usize)
                        .context("Action state")?
                )
                .is_none(),
            "Duplicate action name"
        );
    }
    Ok(result)
}

fn rows(p: &Payload, field: usize, stride: usize, class: u32) -> Result<Vec<u8>> {
    Ok(p.0[p.array_range(field, stride, Some(class))?].to_vec())
}

struct Tables {
    bank: Payload,
    states: Payload,
    dictionary: Payload,
    state_tag: u32,
    dictionary_tag: u32,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    source: &Source,
    runtime: &Value,
    native: &mut Reader,
    rig: &Value,
    graph: &Path,
    mapping: &controls::Mapping,
    prepared_bank: &Payload,
    library: &event::Library,
) -> Result<Prepared> {
    let (profile, binding) = character::profile(source, runtime, native, rig)?;
    let parent = u32::try_from(binding["parent"].as_u64().context("Native profile")?)?;
    let lookup = source.read(controls::owner(runtime, true, SOURCE_LOOKUP, true)?)?;
    let a = lookup.pointer(24)?;
    let from = Tables {
        bank: source.read(lookup.u32(a + 0xA8)?)?,
        states: source.read(lookup.u32(a + 0xB4)?)?,
        dictionary: source.read(lookup.u32(a + 0xAC)?)?,
        state_tag: lookup.u32(a + 0xB4)?,
        dictionary_tag: lookup.u32(a + 0xAC)?,
    };
    let lookup = native.tag(
        controls::owner(rig, true, 0x8080344B, false)?,
        Some(0x80809C36),
    )?;
    let a = lookup.pointer(24)?;
    let to = Tables {
        bank: (*native.tag(lookup.u32(a + 0x90)?, Some(0x808036F6))?).clone(),
        states: (*native.tag(lookup.u32(a + 0x9C)?, Some(0x80803465))?).clone(),
        dictionary: (*native.tag(lookup.u32(a + 0x94)?, Some(0x80808EE1))?).clone(),
        state_tag: lookup.u32(a + 0x9C)?,
        dictionary_tag: lookup.u32(a + 0x94)?,
    };
    let source_names = names(&from.states, true)?;
    let native_names = names(&to.states, false)?;
    let mut builder = Builder::new(native, rig, prepared_bank, &to.states)?;
    let mut rejected = Vec::new();
    let mut accepted = Vec::new();
    for (name, node) in source_names {
        let Some(&target) = native_names.get(&name) else {
            continue;
        };
        if from.states.u64(node)? != 2 && to.states.u64(target)? != 2 {
            continue;
        }
        let attempt = (|| -> Result<()> {
            let plan = selection::plan(&from, &to, node, target, profile, parent)?;
            let mut next = builder.clone();
            let mut descriptors = Vec::new();
            for (s, n) in plan.choices {
                ensure!(s.conditional || n.conditional, "Action is unconditional");
                ensure!(
                    n.choices.len() == 1
                        || s.choices
                            .iter()
                            .map(|c| c.weight)
                            .eq(n.choices.iter().map(|c| c.weight)),
                    "Selected action has no unambiguous native templates"
                );
                let mut clips = Vec::new();
                for (index, s) in s.choices.iter().enumerate() {
                    let n = &n.choices[if n.choices.len() == 1 { 0 } else { index }];
                    ensure!(
                        source.reference(s.tag)? == SOURCE_CLIP,
                        "Conditional source is not a clip"
                    );
                    let original = source.read(s.tag)?;
                    library.validate_source(source, &original)?;
                    let template = native.tag(n.tag, Some(0x80808F49))?;
                    let direct = convert_events(
                        &original,
                        Some((&template, &mapping.bone_map)),
                        Some(library),
                    );
                    let (payload, mut report) = if let Err(error) = direct {
                        if original.u64(0x30)? == 0 {
                            return Err(error);
                        }
                        let mut compatible = None;
                        for row in to.bank.array(8, 4, Some(0x80808F48))? {
                            let tag = to.bank.u32(row)?;
                            let witness = native.tag(tag, Some(0x80808F49))?;
                            if witness.u64(0x30)? == 0
                                || witness.u8(0x181)? != original.u8(0x165)?
                            {
                                continue;
                            }
                            if let Ok((payload, mut report)) = convert_events(
                                &original,
                                Some((&witness, &mapping.bone_map)),
                                Some(library),
                            ) {
                                report["constraint_template"] = json!(tag);
                                compatible = Some((payload, report));
                                break;
                            }
                        }
                        compatible.ok_or(error)?
                    } else {
                        direct?
                    };
                    let (payload, slots) = mapping.apply(&original, &payload)?;
                    report["control_mapping"] = slots;
                    clips.push((s, n, payload, report));
                }
                next.add(name, n.index, &from.bank, &to.bank, &clips, graph)?;
                descriptors.push(json!({"native_choice":n.index,"source_descriptors":s.choices.iter().map(|c|c.descriptor).collect::<Vec<_>>(),
                "native_descriptors":n.choices.iter().map(|c|c.descriptor).collect::<Vec<_>>()}));
            }
            builder = next;
            accepted
                .push(json!({"action":name,"source_contexts":plan.contexts,"choices":descriptors}));
            Ok(())
        })();
        if let Err(error) = attempt {
            rejected.push(json!({"action":name,"reason":format!("{error:#}")}));
        }
    }
    builder.finish(graph,&to,json!({"source_profile":profile,"native_profile":parent,"converted":accepted,"unconverted":rejected,
        "supplemental_pose_policy":"native contributions retained per original descriptor","gameplay_verified":false}))
}

#[derive(Clone)]
struct Builder {
    bank: Payload,
    states: Payload,
    layer_owner: u32,
    layer_tag: u32,
    layers: Payload,
    original_layers: Payload,
    clips: BTreeMap<u32, (Payload, Value)>,
    routes: BTreeMap<u32, BTreeMap<usize, Vec<(usize, u32)>>>,
    native_names: BTreeSet<u32>,
}

impl Builder {
    fn new(native: &mut Reader, rig: &Value, bank: &Payload, states: &Payload) -> Result<Self> {
        let mut native_names = BTreeSet::new();
        for row in bank.array(8, 4, Some(0x80808F48))? {
            native_names.insert(native.tag(bank.u32(row)?, Some(0x80808F49))?.u32(0x120)?);
        }
        for row in bank.array(0x68, 32, Some(0x80809002))? {
            ensure!(
                bank.bytes::<16>(row)? == [0; 16],
                "Extending this bank requires inline descriptor relocation"
            );
        }
        let layer_owner = controls::owner(rig, true, 0x80803640, false)?;
        let owner = native.tag(layer_owner, Some(0x80809C36))?;
        let at = owner.pointer(24)?;
        ensure!(
            [0, u32::MAX].contains(&owner.u32(at + 0x114)?),
            "Conditional actions with directional dispatch need companion translation"
        );
        let layer_tag = owner.u32(at + 0x11C)?;
        let layers = (*native.tag(layer_tag, Some(0x808034EB))?).clone();
        ensure!(
            layers.u64(40)? == bank.u64(0x68)?,
            "Pose dispatch does not cover the native bank"
        );
        Ok(Self {
            bank: bank.clone(),
            states: states.clone(),
            layer_owner,
            layer_tag,
            original_layers: layers.clone(),
            layers,
            clips: BTreeMap::new(),
            routes: BTreeMap::new(),
            native_names,
        })
    }

    fn add(
        &mut self,
        name: u32,
        choice: usize,
        source: &Payload,
        native: &Payload,
        clips: &[(&Choice, &Choice, Payload, Value)],
        graph: &Path,
    ) -> Result<()> {
        // Validate and build a complete weighted action before committing any of its routes.
        let mut bank = self.bank.clone();
        let mut tags = rows(&bank, 8, 4, 0x80808F48)?;
        let mut auxiliary = rows(&bank, 24, 4, 0x80800007)?;
        let mut descriptors = rows(&bank, 0x68, 32, 0x80809002)?;
        let mut aliases = rows(&self.layers, 40, 2, 0x80800006)?;
        ensure!(
            tags.len() == auxiliary.len(),
            "Clip auxiliary coverage differs"
        );
        let sd = source.array(0x68, 32, Some(0x80800577))?;
        let nd = native.array(0x68, 32, Some(0x80809002))?;
        let mut route = Vec::new();
        let mut additions: Vec<(u32, Payload, Value)> = Vec::new();
        for (s, n, payload, report) in clips {
            let mut payload = payload.clone();
            let source_name = payload.u32(0x120)?;
            let occupied = |name| {
                self.native_names.contains(&name)
                    || self
                        .clips
                        .iter()
                        .any(|(&tag, (p, _))| tag != s.tag && p.u32(0x120).ok() == Some(name))
                    || additions
                        .iter()
                        .any(|(tag, p, _)| *tag != s.tag && p.u32(0x120).ok() == Some(name))
            };
            let mut clip_name = source_name;
            if occupied(clip_name) {
                clip_name = (0..=u16::MAX)
                    .map(|salt| {
                        format!("parhelion.d1.first_person.{:08X}.{salt}", s.tag)
                            .bytes()
                            .fold(0x811C9DC5u32, |h, b| {
                                h.wrapping_mul(16777619) ^ u32::from(b)
                            })
                    })
                    .find(|name| ![0, u32::MAX, 0x811C9DC5].contains(name) && !occupied(*name))
                    .context("No private source clip name")?;
                put(&mut payload.0, 0x120, &clip_name.to_le_bytes());
            }
            let sr = *sd.get(s.descriptor).context("Source descriptor")?;
            let nr = *nd.get(n.descriptor).context("Native descriptor")?;
            ensure!(
                source.bytes::<6>(sr + 26)? == native.bytes::<6>(nr + 26)?,
                "Descriptor playback flags differ"
            );
            // Source tag words are temporary relocation symbols, never installed references.
            let slot = if let Some((previous, _)) = self.clips.get(&s.tag) {
                ensure!(
                    previous.0 == payload.0,
                    "Shared source clip converted differently"
                );
                tags.chunks_exact(4)
                    .position(|b| b == s.tag.to_le_bytes())
                    .context("Extra clip bank slot")?
            } else if let Some(slot) = tags.chunks_exact(4).position(|b| b == s.tag.to_le_bytes()) {
                let (_, previous, _) = additions
                    .iter()
                    .find(|(tag, _, _)| *tag == s.tag)
                    .context("Source clip symbol collides with a native tag")?;
                ensure!(
                    previous.0 == payload.0,
                    "Weighted source clip converted differently"
                );
                slot
            } else {
                let slot = tags.len() / 4;
                tags.extend(s.tag.to_le_bytes());
                auxiliary.extend(u32::MAX.to_le_bytes());
                let file = format!("animation/first-person-source-{:08X}.bin", s.tag);
                additions.push((s.tag,payload.clone(),json!({"source":s.tag,"template":n.tag,"file":file,"frames":payload.u16(0x13C)?,"source_name":source_name,"emitted_name":clip_name,"report":report})));
                slot
            };
            let mut descriptor = native.bytes::<32>(nr)?;
            descriptor[16..20].copy_from_slice(&source.u32(sr + 16)?.to_le_bytes());
            descriptor[20..24]
                .copy_from_slice(&(f32::from(payload.u16(0x13C)? - 1) / 30.).to_le_bytes());
            descriptor[24..26].copy_from_slice(&u16::try_from(slot)?.to_le_bytes());
            route.push((descriptors.len() / 32, s.weight));
            descriptors.extend(descriptor);
            let row = *self
                .original_layers
                .array(40, 2, Some(0x80800006))?
                .get(n.descriptor)
                .context("Original pose descriptor")?;
            aliases.extend(self.original_layers.u16(row)?.to_le_bytes());
        }
        append(&mut bank.0, 8, 0x80808F48, &tags, 4)?;
        append(&mut bank.0, 24, 0x80800007, &auxiliary, 4)?;
        append(&mut bank.0, 0x68, 0x80809002, &descriptors, 32)?;
        let mut layers = self.layers.clone();
        append(&mut layers.0, 40, 0x80800006, &aliases, 2)?;
        for p in [&mut bank, &mut layers] {
            let length = p.0.len() as u64;
            put(&mut p.0, 0, &length.to_le_bytes());
        }
        let mut routes = self.routes.clone();
        routes.entry(name).or_default().insert(choice, route);
        state::replace_selected_choices(&self.states, &routes)?;
        for (tag, payload, report) in additions {
            fs::write(
                graph.join(report["file"].as_str().context("Source clip file")?),
                &payload.0,
            )?;
            self.clips.insert(tag, (payload, report));
        }
        self.bank = bank;
        self.layers = layers;
        self.routes = routes;
        Ok(())
    }

    fn finish(self, graph: &Path, tables: &Tables, report: Value) -> Result<Prepared> {
        if self.clips.is_empty() {
            return Ok(Prepared {
                report,
                ..Prepared::default()
            });
        }
        let states = state::replace_selected_choices(&self.states, &self.routes)?;
        let mut files = BTreeMap::new();
        for (key, payload) in [
            ("states_template", &self.states),
            ("states", &states),
            ("parameters_template", &tables.dictionary),
            ("parameters", &tables.dictionary),
        ] {
            let file = format!("animation/first-person-{key}.bin");
            fs::write(graph.join(&file), &payload.0)?;
            files.insert(key.to_owned(), file);
        }
        let template_file = "animation/first-person-layers-template.bin";
        let file = "animation/first-person-layers.bin";
        fs::write(graph.join(template_file), &self.original_layers.0)?;
        fs::write(graph.join(file), &self.layers.0)?;
        Ok(Prepared {
            bank: Some(self.bank),
            clips: self.clips.into_values().map(|(_, r)| r).collect(),
            files,
            states: json!({"status":"source_states","parameters":tables.dictionary_tag,"states":tables.state_tag,"policy":"private named native routes with selected source clips"}),
            layers: json!({"owner":self.layer_owner,"tag":self.layer_tag,"template_file":template_file,"file":file}),
            report,
        })
    }
}
