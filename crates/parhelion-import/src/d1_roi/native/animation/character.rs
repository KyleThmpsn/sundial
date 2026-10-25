//! Source human idle motion and constant holding layers on compatible native control frames.
use super::*;
use crate::tiger::animation as target;
pub(super) mod selection;

const SOURCE_LOOKUP_OWNER: u32 = 0x80A7E159;
const SOURCE_POSE_OWNER: u32 = 0x80A7E157;
const SOURCE_CONTROLS_OWNER: u32 = 0x80A7F7F6;

pub(super) fn profile(
    source: &Source,
    runtime: &Value,
    native: &mut Reader,
    rig: &Value,
) -> Result<(u32, Value)> {
    let owner = controls::owner(runtime, false, 0x80802580, true)?;
    let data = source.read(owner)?;
    let at = data.pointer(24)?;
    let content = u32::try_from(
        runtime["content_key"]
            .as_u64()
            .context("Source weapon content")?,
    )?;
    ensure!(
        data.u32(at - 4)? == 0x80802580,
        "Source attachment class differs"
    );
    let mut rows = data
        .array(at + 0x5D8, 0xD8, Some(0x808031DE))?
        .into_iter()
        .filter(|&r| data.u32(r).ok() == Some(content))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        rows.push(at + 0x500);
    }
    let names = rows
        .into_iter()
        .map(|r| data.u32(r + 0x5C))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        names.len() == 1,
        "Source weapon has ambiguous holding profiles"
    );
    let source_profile = *names.first().unwrap();
    let owner = controls::owner(rig, false, 0x80804221, false)?;
    let data = native.tag(owner, Some(0x80809C36))?;
    let at = data.pointer(24)?;
    let content = u32::from_str_radix(
        rig["content_key"]
            .as_str()
            .context("Native weapon content")?,
        16,
    )?;
    let rows = crate::tiger::rig::attachment_rows(&data, at, content, false)?;
    let names = rows
        .into_iter()
        .map(|r| data.u32(r + 0x6C))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        names.len() == 1,
        "Native weapon has ambiguous holding profiles"
    );
    Ok((
        source_profile,
        json!({"builtin":3,"parent":names.first().unwrap(),"owner":owner,"content":content,"alias":true}),
    ))
}

fn compatible_controls(
    source: &Source,
    native: &Payload,
    rig: &target::control::Rig,
) -> Result<Vec<[f32; 8]>> {
    let from = source.read(SOURCE_CONTROLS_OWNER)?;
    control_defaults(&from, native, rig)
}

pub(super) fn control_defaults(
    from: &Payload,
    native: &Payload,
    rig: &target::control::Rig,
) -> Result<Vec<[f32; 8]>> {
    let a = from.pointer(24)?;
    let b = native.pointer(24)?;
    ensure!(
        from.u32(a - 4)? == 0x8080099B && native.u32(b - 4)? == 0x80808F8F,
        "Human control layouts differ"
    );
    for (field, stride, sc, nc) in [
        (0x90, 8, 0x80800380, 0x8080907F),
        (0xD8, 2, 0x80800006, 0x80800006),
        (0xE8, 2, 0x80800006, 0x80800006),
    ] {
        let sa = from.array(a + field, stride, Some(sc))?;
        let na = native.array(b + field, stride, Some(nc))?;
        ensure!(
            sa.len() == na.len()
                && sa
                    .iter()
                    .zip(na)
                    .all(|(&s, n)| from.0[s..s + stride] == native.0[n..n + stride]),
            "Human control groups or slot maps differ"
        );
    }
    let sa = from.array(a + 0xA0, 48, Some(0x80800638))?;
    let na = native.array(b + 0xA0, 52, Some(0x80809076))?;
    ensure!(
        sa.len() == rig.defaults.len()
            && sa.len() == na.len()
            && sa
                .iter()
                .zip(na)
                .all(|(&s, n)| from.0[s..s + 48] == native.0[n + 4..n + 52]),
        "Human control operations differ"
    );
    let defaults = from
        .array(a + 0xB0, 32, Some(0x808001B0))?
        .into_iter()
        .map(|r| {
            let mut pose = [0.; 8];
            for (i, v) in pose.iter_mut().enumerate() {
                *v = from.f32(r + i * 4)?;
            }
            Ok(target::pose::Pose::checked(pose)?.0)
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        defaults.len() == rig.defaults.len(),
        "Source human defaults disagree with its controls"
    );
    Ok(defaults)
}

fn descriptor(bank: &Payload, index: usize, source: bool) -> Result<u32> {
    let row = *bank
        .array(0x68, 32, Some(if source { 0x80800577 } else { 0x80809002 }))?
        .get(index)
        .context("Human body descriptor")?;
    ensure!(
        bank.bytes::<16>(row)? == [0; 16] && matches!(bank.u16(row + 26)?, 1 | 2),
        "Human body descriptor playback unsupported"
    );
    let clips = bank.array(8, 4, Some(if source { 0x8080408D } else { 0x80808F48 }))?;
    bank.u32(
        *clips
            .get(usize::from(bank.u16(row + 24)?))
            .context("Human body clip index")?,
    )
}

fn idle(table: &Payload, choice: usize, source: bool) -> Result<Option<usize>> {
    let row = *table
        .array(24, 72, Some(if source { 0x808022D5 } else { 0x80803378 }))?
        .get(choice)
        .context("Stance row")?;
    let rows = table.array(row, 12, Some(if source { 0x80802237 } else { 0x8080338C }))?;
    if rows.len() != 1 {
        return Ok(None);
    }
    Ok(Some(table.u32(rows[0])? as usize))
}

fn layer_choices(p: &Payload, descriptor: usize, source: bool) -> Result<BTreeMap<u32, usize>> {
    let aliases = p.array(40, 2, Some(0x80800006))?;
    let alias = p.u16(*aliases.get(descriptor).context("Human layer descriptor")?)?;
    if alias == u16::MAX {
        return Ok(BTreeMap::new());
    }
    let route = *p
        .array(56, 32, Some(if source { 0x8080254E } else { 0x808034EF }))?
        .get(usize::from(alias))
        .context("Human layer route")?;
    let groups = p.array(8, 24, Some(if source { 0x808022CE } else { 0x80803724 }))?;
    let mut result = BTreeMap::new();
    for edge in p.array(route, 4, Some(if source { 0x808028E6 } else { 0x808034F1 }))? {
        let group = *groups
            .get(usize::from(p.u16(edge + 2)?))
            .context("Human layer group")?;
        let choice = usize::from(p.u16(edge)?);
        let choices = p.array(
            group,
            if source { 48 } else { 16 },
            Some(if source { 0x80802744 } else { 0x8080372E }),
        )?;
        ensure!(
            result
                .insert(
                    p.u32(group + 16)?,
                    *choices.get(choice).context("Human layer choice")?
                )
                .is_none(),
            "Repeated human layer group"
        );
    }
    Ok(result)
}

struct Conversion<'a> {
    source: &'a Source,
    native: &'a mut Reader,
    directory: &'a Path,
    defaults: &'a [[f32; 8]],
    rig: &'a target::control::Rig,
}

impl Conversion<'_> {
    fn layer(&mut self, source: u32, templates: &BTreeSet<u32>, index: usize) -> Result<Value> {
        ensure!(
            !templates.is_empty(),
            "Native holding clip template missing"
        );
        let mut rejected = Vec::new();
        for &template in templates {
            crate::cancellation::check()?;
            match self.clip(source, template, index) {
                Ok(clip) => return Ok(clip),
                Err(error) => rejected.push(format!("{template:08X}: {error:#}")),
            }
        }
        anyhow::bail!(
            "No compatible native holding variant for {source:08X}: {}",
            rejected.join(", ")
        )
    }

    fn clip(&mut self, source_tag: u32, template: u32, index: usize) -> Result<Value> {
        ensure!(
            self.source.reference(source_tag)? == SOURCE_CLIP,
            "Holding motion is not a source clip"
        );
        let from = self.source.read(source_tag)?;
        let to = self.native.tag(template, Some(0x80808F49))?;
        ensure!(
            usize::from(from.u8(0x12A)?) == self.rig.bind.len()
                && usize::from(from.u8(0x12B)?) == self.rig.defaults.len(),
            "Holding clip uses another human rig"
        );
        ensure!(
            from.u32(0x164)? == to.u32(0x180)?,
            "Holding clip blend mode differs"
        );
        let source_groups = from.array(0x150, 8, Some(0x80800380))?;
        let target_groups = to.array(0x170, 8, Some(0x8080907F))?;
        ensure!(
            source_groups.len() == target_groups.len()
                && source_groups
                    .iter()
                    .zip(target_groups)
                    .all(|(&a, b)| from.0[a..a + 8] == to.0[b..b + 8]),
            "Holding clip control groups differ"
        );
        let bones = (0..u16::try_from(self.rig.bind.len())?).collect::<Vec<_>>();
        let (converted, report) = convert_bound(&from, Some((&to, &bones)))?;
        ensure!(
            report["audio"]["patches"]
                .as_array()
                .context("Holding cue patches")?
                .is_empty(),
            "Holding clip cue needs private audio binding"
        );
        let converted = if converted.u8(0x181)? == 0 {
            target::clip::bind_defaults(&converted, self.defaults)?
        } else {
            converted
        };
        for frame in [0, usize::from(converted.u16(0x13C)?) - 1] {
            target::clip::sample(&converted, &self.rig.defaults, frame)?;
        }
        let file = format!("character/clip-{index}.bin");
        fs::write(self.directory.join(&file), &converted.0)?;
        Ok(
            json!({"file":file,"source":source_tag,"template":template,"duration":f64::from(converted.u16(0x13C)?)/30.,"report":report}),
        )
    }
}

#[allow(clippy::cognitive_complexity)]
pub(in crate::d1_roi::native) fn prepare(
    source: &Source,
    runtime: &Value,
    native: &mut Reader,
    native_rig: &Value,
    directory: &Path,
    private: u32,
) -> Result<Value> {
    let rig_evidence = super::super::rig::validate(source, native)?;
    let (source_profile, mut binding) = profile(source, runtime, native, native_rig)?;
    binding["private"] = json!(private);
    let parent = u32::try_from(binding["parent"].as_u64().context("Native profile")?)?;
    // The attachment name drives both views. Its native first-person dictionary
    // needs the same private alias even when no first-person source clip converts.
    // A third-person-only weapon has no first-person lookup to alias.
    let fp_parameters =
        if let Some(owner) = controls::optional_owner(native_rig, true, 0x8080344B, false)? {
            let lookup = native.tag(owner, Some(0x80809C36))?;
            let parameters = lookup.u32(lookup.pointer(24)? + 0x94)?;
            target::state::parameter::alias(
                native.tag(parameters, Some(0x80808EE1))?.as_ref(),
                3,
                parent,
                private,
            )?;
            Some(parameters)
        } else {
            None
        };
    let lookup = native.tag(0x80EF62E5, Some(0x80809C36))?;
    let lp = lookup.pointer(24)?;
    let pose = native.tag(0x8161D5F7, Some(0x80809C36))?;
    let pp = pose.pointer(24)?;
    ensure!(
        lookup.u32(lp - 4)? == 0x8080344B
            && pose.u32(pp - 4)? == 0x80803640
            && lookup.u32(lp + 0x90)? == pose.u32(pp + 0x108)?,
        "Native human controller differs"
    );
    let source_lookup = source.read(SOURCE_LOOKUP_OWNER)?;
    let sl = source_lookup.pointer(24)?;
    let source_pose = source.read(SOURCE_POSE_OWNER)?;
    let sp = source_pose.pointer(24)?;
    ensure!(
        source_lookup.u32(sl - 4)? == SOURCE_LOOKUP
            && source_pose.u32(sp - 4)? == 0x80802790
            && source_lookup.u32(sl + 0xA8)? == source_pose.u32(sp + 0x108)?,
        "Source playable human controller differs"
    );
    let source_tag = |tag, class| -> Result<Payload> {
        ensure!(
            source.reference(tag)? == class,
            "Source human table class differs"
        );
        source.read(tag)
    };
    let sb = source_tag(source_lookup.u32(sl + 0xA8)?, SOURCE_BANK)?;
    let sd = source_tag(source_lookup.u32(sl + 0xAC)?, 0x80800368)?;
    let st = source_tag(source_lookup.u32(sl + 0xB8)?, 0x8080287E)?;
    let ss = source_tag(source_lookup.u32(sl + 0xBC)?, 0x808025D6)?;
    let layers = source_tag(source_pose.u32(sp + 0x118)?, 0x80802919)?;
    fs::create_dir_all(directory.join("character"))?;
    let mut files = BTreeMap::new();
    let mut tags = BTreeMap::new();
    let mut payloads = BTreeMap::new();
    for (key, tag, class) in [
        ("skeleton", 0x80C22410, 0x80809C36),
        ("controls", 0x80BFCB38, 0x80809C36),
        ("pose", 0x8161D5F7, 0x80809C36),
        ("bank", lookup.u32(lp + 0x90)?, 0x808036F6),
        ("parameters", lookup.u32(lp + 0x94)?, 0x80808EE1),
        ("states", lookup.u32(lp + 0x9C)?, 0x80803465),
        ("layers", pose.u32(pp + 0x11C)?, 0x808034EB),
        ("directions", pose.u32(pp + 0x114)?, 0x8080348E),
        ("stance_table", lookup.u32(lp + 0xB4)?, 0x8080345F),
        ("stance_selector", lookup.u32(lp + 0xB8)?, 0x80803478),
    ] {
        let data = native.tag(tag, Some(class))?;
        let file = format!("character/{key}.bin");
        fs::write(directory.join(&file), &data.0)?;
        files.insert(key.to_owned(), file);
        tags.insert(key, tag);
        payloads.insert(key, data);
    }
    if let Some(tag) = fp_parameters {
        let data = native.tag(tag, Some(0x80808EE1))?;
        let key = "first_person_parameters";
        let file = format!("character/{key}.bin");
        fs::write(directory.join(&file), &data.0)?;
        files.insert(key.to_owned(), file);
        tags.insert(key, tag);
        payloads.insert(key, data);
    }
    let rig = target::control::Rig::read_character(&payloads["skeleton"], &payloads["controls"])?;
    let defaults = compatible_controls(source, &payloads["controls"], &rig)?;
    target::state::parameter::alias(&payloads["parameters"], 3, parent, private)?;
    let pairs = selection::pairs(
        &sd,
        &payloads["parameters"],
        &ss,
        &payloads["stance_selector"],
        source_profile,
        parent,
    )?;
    let mut conversion = Conversion {
        source,
        native,
        directory,
        defaults: &defaults,
        rig: &rig,
    };
    let mut clips = Vec::new();
    let mut extras = Vec::new();
    let mut routes = BTreeMap::new();
    let mut rejected = Vec::new();
    let mut bodies = BTreeMap::<(usize, usize), usize>::new();
    let source_clips = sb.array(8, 4, Some(0x8080408D))?;
    let native_clips = payloads["bank"].array(8, 4, Some(0x80808F48))?;
    for (native_choice, source_choices) in pairs {
        let attempt = (|| -> Result<(usize, usize, Value, Vec<Value>)> {
            ensure!(
                source_choices.len() == 1,
                "Shared contexts select different source stances"
            );
            let source_choice = *source_choices.first().unwrap();
            let si = idle(&st, source_choice, true)?.context("Source idle has multiple clips")?;
            let ni = idle(&payloads["stance_table"], native_choice, false)?
                .context("Native idle has multiple clips")?;
            let source_tag = descriptor(&sb, si, true)?;
            let native_tag = descriptor(&payloads["bank"], ni, false)?;
            // The complete shared parameter domain pairs this stance with one source
            // stance. Clip names can change between eras. The selected source body
            // still has to satisfy the human rig, control and constraint contracts.
            let source_layers = layer_choices(&layers, si, true)?;
            let native_layers = layer_choices(&payloads["layers"], ni, false)?;
            let mut holds = Vec::new();
            let mut added = Vec::new();
            let mut has_grip = false;
            for (name, option) in source_layers {
                // Only constant drivers have the same parameter semantics in both games.
                if layers.u32(option + 16)? != 1 || layers.f32(option + 20)? != 1. {
                    continue;
                }
                let Some(&choice) = native_layers.get(&name) else {
                    continue;
                };
                let native_options = payloads["layers"].array(choice, 64, Some(0x80803730))?;
                ensure!(!native_options.is_empty(), "Native hold has no options");
                let mut templates = BTreeSet::new();
                for &o in &native_options {
                    ensure!(
                        payloads["layers"].u32(o + 32)? == 1
                            && payloads["layers"].f32(o + 36)? == 1.,
                        "Native holding driver is dynamic"
                    );
                    for weight in payloads["layers"].array(o, 8, Some(0x80803737))? {
                        templates.insert(
                            payloads["bank"].u32(
                                *native_clips
                                    .get(payloads["layers"].u32(weight)? as usize)
                                    .context("Native holding clip")?,
                            )?,
                        );
                    }
                }
                let mut weights = Vec::new();
                for row in layers.array(option, 8, Some(0x80802687))? {
                    let tag = sb.u32(
                        *source_clips
                            .get(layers.u32(row)? as usize)
                            .context("Source holding clip")?,
                    )?;
                    let source_clip = source.read(tag)?;
                    has_grip |= source_clip.u8(0x165)? == 2;
                    let weight = layers.f32(row + 4)?;
                    ensure!(weight >= 0., "Negative source holding weight");
                    // A donor layer can contain several blend modes. One fully
                    // compatible template is sufficient for this source choice.
                    let clip =
                        conversion.layer(tag, &templates, 10000 + extras.len() + added.len())?;
                    weights.push(json!({"clip":extras.len()+added.len(),"weight":weight}));
                    added.push(clip);
                }
                ensure!(!weights.is_empty(), "Source holding layer is empty");
                holds.push(json!({"name":name,"clips":weights}));
            }
            ensure!(
                has_grip,
                "Stance has no compatible source replacement grip layer"
            );
            let mut clip = conversion.clip(source_tag, native_tag, clips.len())?;
            let source_descriptor = sb.array(0x68, 32, Some(0x80800577))?[si];
            clip["duration"] = json!(sb.f32(source_descriptor + 20)?);
            clip["descriptor"] = json!(ni);
            clip["source_descriptor"] = json!(si);
            clip["holding_layers"] = json!(holds);
            Ok((ni, si, clip, added))
        })();
        match attempt {
            Ok((ni,si,clip,added))=>{
                let index=if let Some(&i)=bodies.get(&(ni,si)) {i} else {
                    let i=clips.len(); clips.push(clip);extras.extend(added);bodies.insert((ni,si),i);i
                };
                routes.insert(native_choice,BTreeMap::from([(0,BTreeMap::from([(ni,index)]))]));
            }
            Err(error)=>rejected.push(json!({"native_choice":native_choice,"source_choices":source_choices,"reason":format!("{error:#}")})),
        }
    }
    ensure!(
        !clips.is_empty(),
        "No compatible D1 standing or crouched grip converted: {}",
        serde_json::to_string(&rejected)?
    );
    let body_count = clips.len();
    for clip in &mut clips {
        for layer in clip["holding_layers"].as_array_mut().unwrap() {
            for weight in layer["clips"].as_array_mut().unwrap() {
                weight["clip"] = json!(weight["clip"].as_u64().unwrap() + body_count as u64);
            }
        }
    }
    clips.extend(extras);
    let mut result = json!({"schema":2,"files":files,"clips":clips,"actions":[],"tables":[{"kind":"stance","table":tags["stance_table"],"selector":tags["stance_selector"],"class":0x8080345Fu32,"branches":[{"conditions":{},"routes":routes}]}],"binding":binding,"private":private,"source_profiles":{"source":source_profile,"native_parent":parent,"lookup":SOURCE_LOOKUP_OWNER,"pose":SOURCE_POSE_OWNER,"rig":rig_evidence},"unconverted":rejected,"retained_native_behavior":["Dynamic aim and layer weights","Directional motion","Walking and running","Combat actions and events"],"gameplay_verified":false});
    for (key, tag) in tags {
        result[key] = json!(tag);
    }
    Ok(result)
}

impl Source {
    /// Refresh only character motion in an editable prepared graph. Callers stage a copy first.
    pub fn refresh_weapon_character(
        &self,
        directory: &Path,
        native: &Path,
    ) -> Result<crate::GraphReference> {
        self.unchanged()?;
        let directory = crate::io::outside(
            directory,
            self.report.packages.parent().context("Source root")?,
        )?;
        crate::io::outside(&directory, native.parent().context("Native root")?)?;
        let path = directory.join("asset-graph.json");
        let mut graph: Value = serde_json::from_slice(&fs::read(&path)?)?;
        ensure!(
            graph["source"] == "d1_roi" && graph["kind"] == "weapon",
            "Character refresh requires a Rise of Iron weapon graph"
        );
        let item = crate::graph::hash(&graph, "item_hash")?;
        let private = super::super::weapon::key(&format!("d1/character/{item:08X}"));
        let mut reader = Reader::discovery(native, &directory.join("character-readback"), false)?;
        let character = prepare(
            self,
            &graph["rendering"][0]["source_runtime"],
            &mut reader,
            &graph["rendering"][0]["native_runtime"],
            &directory,
            private,
        )?;
        if !graph["animation"].is_object() {
            graph["animation"] = json!({});
        }
        graph["animation"]["character"] = character;
        graph["animation"]["character_status"] = json!("linked");
        graph["animation"]
            .as_object_mut()
            .unwrap()
            .remove("character_reason");
        crate::io::write_json(&path, &graph)?;
        reader.finish()?;
        self.unchanged()?;
        crate::GraphReference::new(&directory, item)
    }
}
