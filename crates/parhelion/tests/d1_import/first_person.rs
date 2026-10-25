//! Follow the staged weapon's selected actions, independently of conversion reports.
use super::*;

fn identity(v: &Value) -> Result<u32> {
    if let Some(s) = v.as_str() {
        Ok(u32::from_str_radix(s, 16)?)
    } else {
        number(v)
    }
}

fn owner(rig: &Value, class: u32, fp: bool) -> Result<u32> {
    let root = identity(&rig["runtime_entity"])?;
    let mut found = Vec::new();
    for c in rig["components"].as_array().context("Components")? {
        if identity(&c["class"])? == class && (identity(&c["entity"])? != root) == fp {
            found.push(identity(&c["owner"])?);
        }
    }
    ensure!(found.len() == 1, "Ambiguous component owner");
    Ok(found[0])
}

fn profile(p: &Payload, content: u32, source: bool) -> Result<u32> {
    let at = p.pointer(24)?;
    let rows = if source {
        let rows = p
            .array(at + 0x5D8, 0xD8, Some(0x808031DE))?
            .into_iter()
            .filter(|&r| p.u32(r).ok() == Some(content))
            .collect::<Vec<_>>();
        if rows.is_empty() {
            vec![at + 0x500]
        } else {
            rows
        }
    } else {
        parhelion_import::tiger::rig::attachment_rows(p, at, content, false)?
    };
    let names = rows
        .into_iter()
        .map(|r| p.u32(r + if source { 0x5C } else { 0x6C }))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(names.len() == 1, "Ambiguous attachment profile");
    Ok(*names.first().unwrap())
}

struct Controller {
    lookup: u32,
    bank: Payload,
    states: Payload,
    dictionary: Payload,
    profile: u32,
    source: bool,
    slots: Vec<u32>,
    groups: Vec<(u32, u32)>,
    defaults: Vec<[u32; 8]>,
}

impl Controller {
    fn read(
        rig: &Value,
        source: bool,
        mut read: impl FnMut(u32) -> Result<Payload>,
    ) -> Result<Self> {
        let lookup = owner(rig, if source { 0x808020BB } else { 0x8080344B }, true)?;
        let p = read(lookup)?;
        let at = p.pointer(24)? + if source { 0xA8 } else { 0x90 };
        let attachment = read(owner(
            rig,
            if source { 0x80802580 } else { 0x80804221 },
            false,
        )?)?;
        let controls = read(owner(
            rig,
            if source { 0x8080099B } else { 0x80808F8F },
            true,
        )?)?;
        let skeletons = rig["skeletons"].as_array().context("Skeletons")?;
        let selected = skeletons
            .iter()
            .filter(|s| identity(&s["entity"]).ok() != identity(&rig["runtime_entity"]).ok())
            .collect::<Vec<_>>();
        ensure!(selected.len() == 1, "Ambiguous first-person skeleton");
        let skeleton = read(identity(&selected[0]["owner"])?)?;
        let bones = skeleton.array(
            skeleton.pointer(24)? + if source { 0x98 } else { 0x80 },
            16,
            None,
        )?;
        let slots = controls
            .array(controls.pointer(24)? + 0xE8, 2, Some(0x80800006))?
            .into_iter()
            .map(|r| {
                skeleton.u32(
                    *bones
                        .get(usize::from(controls.u16(r)?))
                        .context("Control bone")?,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            lookup,
            bank: read(p.u32(at)?)?,
            states: read(p.u32(at + 12)?)?,
            dictionary: read(p.u32(at + 4)?)?,
            source,
            profile: profile(&attachment, identity(&rig["content_key"])?, source)?,
            slots,
            defaults: controls
                .array(controls.pointer(24)? + 0xB0, 32, None)?
                .into_iter()
                .map(|r| {
                    let mut words = [0; 8];
                    for (i, word) in words.iter_mut().enumerate() {
                        *word = controls.u32(r + i * 4)?;
                    }
                    Ok(words)
                })
                .collect::<Result<_>>()?,
            groups: controls
                .array(controls.pointer(24)? + 0x90, 8, None)?
                .into_iter()
                .map(|r| Ok((controls.u32(r)?, controls.u32(r + 4)?)))
                .collect::<Result<_>>()?,
        })
    }

    fn contexts(&self, hash: u32) -> Result<Vec<BTreeMap<u8, u32>>> {
        let entry = self
            .states
            .array(8, 8, None)?
            .into_iter()
            .find(|&r| self.states.u32(r).ok() == Some(hash))
            .context("Action name")?;
        let nodes = self.states.array(24, 16, None)?;
        let node = *nodes
            .get(self.states.u32(entry + 4)? as usize)
            .context("Action node")?;
        if self.states.u64(node)? == 1 {
            return Ok(vec![BTreeMap::new()]);
        }
        ensure!(
            self.states.u64(node)? == 2,
            "Unsupported action context kind"
        );
        let at = self.states.pointer(node + 8)?;
        let groups = self.dictionary.array(8, 32, None)?;
        let width = if self.source { 8 } else { 16 };
        let mut contexts = vec![BTreeMap::new()];
        for row in self.states.array(at + 16, 1, None)? {
            let parameter = self.states.u8(row)?;
            if parameter == 3 {
                continue;
            }
            let group_index = self.dictionary.u16(24 + usize::from(parameter) * 2)?;
            if group_index == u16::MAX {
                // Exact-name comparisons precede ancestry masks in the original
                // selector. All record literals plus one unmatched name cover
                // the outcomes of a raw input with a zero dictionary mask.
                let mut names = self
                    .states
                    .array(at + 32, width + 12, None)?
                    .into_iter()
                    .map(|r| self.states.u32(r + width))
                    .collect::<Result<BTreeSet<_>>>()?;
                let unmatched = (0..=u32::MAX)
                    .find(|n| !names.contains(n))
                    .context("No unmatched selector name")?;
                names.insert(unmatched);
                ensure!(
                    contexts.len().saturating_mul(names.len()) <= 4096,
                    "Raw action context corpus exceeds its bound"
                );
                contexts = contexts
                    .into_iter()
                    .flat_map(|values| {
                        names.iter().map(move |&name| {
                            let mut v = values.clone();
                            v.insert(parameter, name);
                            v
                        })
                    })
                    .collect();
                continue;
            }
            let group = *groups
                .get(usize::from(group_index))
                .context("Dynamic action domain")?;
            let names = self
                .dictionary
                .array(group + 16, width + 8, None)?
                .into_iter()
                .map(|r| self.dictionary.u32(r + width))
                .collect::<Result<BTreeSet<_>>>()?;
            ensure!(
                !names.is_empty() && contexts.len().saturating_mul(names.len()) <= 4096,
                "Dynamic action corpus is empty or exceeds its explicit context bound"
            );
            contexts = contexts
                .into_iter()
                .flat_map(|values| {
                    names.iter().map(move |&name| {
                        let mut v = values.clone();
                        v.insert(parameter, name);
                        v
                    })
                })
                .collect();
        }
        Ok(contexts)
    }

    fn clips(&self, name: u32, context: &BTreeMap<u8, u32>) -> Result<Vec<(u32, u32, usize)>> {
        let s = &self.states;
        let names = s.array(8, 8, None)?;
        let entry = names
            .into_iter()
            .find(|&r| s.u32(r).ok() == Some(name))
            .context("Action name")?;
        let nodes = s.array(24, 16, None)?;
        let node = *nodes
            .get(s.u32(entry + 4)? as usize)
            .context("Action node")?;
        let at = s.pointer(node + 8)?;
        let choice = match s.u64(node)? {
            1 => at,
            2 => {
                let width = if self.source { 8 } else { 16 };
                let records = s.array(at + 32, width + 12, None)?;
                let ops = s.array(at + 48, 4, None)?;
                let groups = self.dictionary.array(8, 32, None)?;
                let mut op_index = 0;
                let mut visited = BTreeSet::new();
                let selected = loop {
                    ensure!(visited.insert(op_index), "Selector operation cycle");
                    let op = s.u32(*ops.get(op_index).context("Selector operation")?)?;
                    ensure!(op & 0xFF00 == 0, "Unknown selector operation flags");
                    let parameter = op as u8;
                    let value_name = if parameter == 3 {
                        self.profile
                    } else {
                        *context
                            .get(&parameter)
                            .context("Missing dynamic action input")?
                    };
                    let group_index = self.dictionary.u16(24 + usize::from(parameter) * 2)?;
                    let mut mask = vec![0; width + 4];
                    mask[width..].copy_from_slice(&value_name.to_le_bytes());
                    if group_index != u16::MAX {
                        let group = *groups
                            .get(usize::from(group_index))
                            .context("Action domain")?;
                        let row = self
                            .dictionary
                            .array(group + 16, width + 8, None)?
                            .into_iter()
                            .find(|&r| self.dictionary.u32(r + width).ok() == Some(value_name));
                        if let Some(row) = row {
                            mask.copy_from_slice(&self.dictionary.0[row..row + width + 4]);
                        }
                    }
                    let value = select(
                        s,
                        &records,
                        &mask,
                        (op >> 16) as usize,
                        &mut BTreeSet::new(),
                    )?
                    .unwrap_or(s.i16(
                        *records.get((op >> 16) as usize).context("Selector root")? + width + 8,
                    )?);
                    if value < 0 {
                        break (!value) as usize;
                    }
                    ensure!(value > 0, "Selector result is empty");
                    op_index = value as usize - 1;
                };
                *s.array(at, 16, None)?
                    .get(selected)
                    .context("Selected choice")?
            }
            _ => anyhow::bail!("Unsupported acceptance action"),
        };
        let descriptors = self.bank.array(0x68, 32, None)?;
        let clips = self.bank.array(8, 4, None)?;
        s.array(choice, 8, None)?
            .into_iter()
            .map(|r| {
                let index = s.u32(r)? as usize;
                let d = *descriptors.get(index).context("Descriptor")?;
                let c = *clips
                    .get(usize::from(self.bank.u16(d + 24)?))
                    .context("Clip")?;
                Ok((self.bank.u32(c)?, s.u32(r + 4)?, index))
            })
            .collect()
    }
}

fn select(
    p: &Payload,
    rows: &[usize],
    mask: &[u8],
    index: usize,
    seen: &mut BTreeSet<usize>,
) -> Result<Option<i16>> {
    ensure!(seen.insert(index), "Selector record cycle");
    let width = mask.len() - 4;
    let r = *rows.get(index).context("Selector record")?;
    let exact = p.0[r + width..r + width + 4] == mask[width..];
    let includes = p.u8(r + width - 1)? & mask[width - 1] & 128 != 0
        && (0..width).all(|i| p.0[r + i] & !mask[i] == 0);
    let result = if exact {
        Some(p.i16(r + width + 8)?)
    } else {
        let child = p.i16(r + width + 4)?;
        let alternative = p.i16(r + width + 6)?;
        let refined = if includes && child >= 0 {
            select(p, rows, mask, child as usize, seen)?
        } else {
            None
        };
        if includes && refined.is_none() {
            Some(p.i16(r + width + 8)?)
        } else if alternative >= 0 {
            select(p, rows, mask, alternative as usize, seen)?.or(refined)
        } else {
            refined
        }
    };
    seen.remove(&index);
    Ok(result)
}

fn events(
    p: &Payload,
    source: bool,
    mut resource: impl FnMut(u32) -> Result<()>,
) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    for row in p.array(if source { 0x140 } else { 0x160 }, 8, None)? {
        let at = p.pointer(row)?;
        let mut e = json!({"frame":p.i16(at)?,"kind":p.u16(at + 2)?});
        match p.u32(at - 4)? {
            0x8080060E | 0x80809033 => {
                let text = p.pointer(at + 16)?;
                let end = p.0[text..]
                    .iter()
                    .position(|&b| b == 0)
                    .context("Event path")?;
                e["path"] = json!(std::str::from_utf8(&p.0[text..text + end])?);
                e["dispatch"] = json!(p.bytes::<12>(at + 4)?);
                resource(p.u32(at + 24)?)?;
            }
            0x80800607 | 0x80809031 => e["data"] = json!(p.bytes::<8>(at + 4)?),
            0x80800436 | 0x8080903A => e["data"] = json!(p.bytes::<2>(at + 4)?),
            other => anyhow::bail!("Unverified event class {other:08X}"),
        }
        result.push(e);
    }
    Ok(result)
}

// Dynamic samples have the same codecs in both builds. Compare their streams and maps
// without invoking conversion, and require nonempty motion rather than only matching duration.
fn motion(
    source: &Payload,
    emitted: &Payload,
    source_slots: &[u32],
    native_slots: &[u32],
    source_groups: &[(u32, u32)],
    native_groups: &[(u32, u32)],
) -> Result<Value> {
    ensure!(
        source.u8(0x165)? == emitted.u8(0x181)?,
        "Clip composition mode changed"
    );
    let coverage = |p: &Payload, field, groups: &[(u32, u32)]| -> Result<usize> {
        let actual = p
            .array(field, 8, None)?
            .into_iter()
            .map(|r| Ok((p.u32(r)?, p.u32(r + 4)?)))
            .collect::<Result<Vec<_>>>()?;
        Ok(actual
            .iter()
            .zip(groups)
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| a.1 as usize)
            .sum())
    };
    let source_covered = coverage(source, 0x150, source_groups)?;
    let native_covered = coverage(emitted, 0x170, native_groups)?;
    ensure!(
        source_covered > 0 && native_covered > 0,
        "Clip has no applied control groups"
    );
    let a = source.pointer(0x18)?;
    let b = emitted.pointer(0x18)?;
    let kind = source.u16(a)?;
    ensure!(kind == emitted.u16(b)?, "Dynamic codec changed");
    let fields: &[(usize, usize)] = match kind {
        1 => &[
            (0x10, 2),
            (0x20, 2),
            (0x30, 1),
            (0x40, 1),
            (0x50, 4),
            (0x60, 4),
            (0x70, 2),
        ],
        2 => &[(0x18, 2), (0x28, 4), (0x38, 4)],
        3 => &[(0x38, 2)],
        _ => anyhow::bail!("Unsupported motion oracle codec"),
    };
    let mut samples = Vec::new();
    for &(field, stride) in fields {
        let expected = &source.0[source.array_range(a + field, stride, None)?];
        let actual = &emitted.0[emitted.array_range(b + field, stride, None)?];
        ensure!(expected == actual, "Source motion samples changed");
        samples.extend_from_slice(actual);
    }
    ensure!(!samples.is_empty(), "No motion was exercised");
    let mut targets = Vec::new();
    for channel in 0..3 {
        let expected = source
            .array(0xC8 + channel * 16, 1, Some(0x80800009))?
            .into_iter()
            .map(|r| source.u8(r).map(usize::from))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|&slot| slot < source_covered)
            .map(|slot| {
                source_slots
                    .get(slot)
                    .copied()
                    .context("Source control slot")
            })
            .collect::<Result<Vec<_>>>()?;
        let actual = emitted
            .array(0xD8 + channel * 16, 2, Some(0x8080000A))?
            .into_iter()
            .map(|r| emitted.u16(r).map(usize::from))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|&slot| slot < native_covered)
            .map(|slot| {
                native_slots
                    .get(slot)
                    .copied()
                    .context("Emitted control slot")
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(expected == actual, "Source motion targets different bones");
        targets.push(actual);
    }
    Ok(
        json!({"codec":kind,"composition_mode":emitted.u8(0x181)?,"source_applied_controls":source_covered,"native_applied_controls":native_covered,
            "target_bones":targets,"sample_sha256":hex::encode(Sha256::digest(samples))}),
    )
}

fn constraints(source: &Payload, emitted: &Payload) -> Result<Value> {
    if source.u64(0x30)? == 0 {
        return Ok(Value::Null);
    }
    let a = source.array_range(source.pointer(0x30)?, 64, Some(0x8080081F))?;
    let b = emitted.array_range(emitted.pointer(0x30)?, 64, Some(0x80808F5C))?;
    ensure!(
        source.0[a.clone()] == emitted.0[b],
        "Source constraint bindings changed"
    );
    let mut bytes = source.0[a.clone()].to_vec();
    for (sf, nf, scalar) in [(0x38, 0x38, true), (0x40, 0x48, false)] {
        let s = source.pointer(sf)?;
        let n = emitted.pointer(nf)?;
        let kind = source.u16(s)?;
        ensure!(kind == emitted.u16(n)?, "Constraint codec changed");
        let fields: &[(usize, usize)] = match (scalar, kind) {
            (true, 1) => &[(16, 2), (32, 2), (48, 1), (64, 1), (80, 2)],
            (true, 2) => &[(16, 2), (32, 2), (48, 4), (64, 4)],
            (false, 1) => &[
                (16, 2),
                (32, 2),
                (48, 1),
                (64, 1),
                (80, 4),
                (96, 4),
                (112, 2),
            ],
            (false, 2) => &[(24, 2), (40, 4), (56, 4)],
            (false, 3) => &[(56, 2)],
            _ => anyhow::bail!("Unsupported constraint readback codec"),
        };
        for &(field, stride) in fields {
            let expected = &source.0[source.array_range(s + field, stride, None)?];
            let actual = &emitted.0[emitted.array_range(n + field, stride, None)?];
            ensure!(expected == actual, "Source constraint samples changed");
            bytes.extend_from_slice(actual);
        }
    }
    Ok(
        json!({"bindings":a.len()/64,"source_binding_and_sample_sha256":hex::encode(Sha256::digest(bytes)),
        "secondary_stream_present":emitted.u64(0x40)? != 0}),
    )
}

fn implicit_pose(
    source: &Payload,
    emitted: &Payload,
    from: &Controller,
    to: &Controller,
) -> Result<usize> {
    if source.u8(0x165)? != 0 {
        return Ok(0);
    }
    let source_groups = source
        .array(0x150, 8, None)?
        .into_iter()
        .map(|r| Ok((source.u32(r)?, source.u32(r + 4)?)))
        .collect::<Result<Vec<_>>>()?;
    let covered = source_groups
        .iter()
        .zip(&from.groups)
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.1 as usize)
        .sum::<usize>();
    let mut count = 0;
    for (kind, (width, lane)) in [(1, 7), (4, 0), (3, 4)].into_iter().enumerate() {
        let explicit = [0x98, 0xC8]
            .into_iter()
            .map(|field| source.array(field + kind * 16, 1, None))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .map(|r| source.u8(r).map(usize::from))
            .collect::<Result<BTreeSet<_>>>()?;
        for slot in 0..covered {
            if explicit.contains(&slot) {
                continue;
            }
            let target = to
                .slots
                .iter()
                .position(|name| *name == from.slots[slot])
                .context("Default pose target")?;
            let maps = emitted.array(0xA8 + kind * 16, 2, None)?;
            let track = maps
                .iter()
                .position(|&r| emitted.u16(r).ok() == Some(target as u16))
                .context("Source implicit pose was left to the donor")?;
            let codec = emitted.pointer(0x10)?;
            ensure!(
                emitted.u16(codec)? == 0,
                "Explicit source defaults require float tracks"
            );
            let values = emitted.array(
                codec + 0x18 + kind * 16,
                if kind == 0 { 4 } else { 16 },
                None,
            )?;
            for i in 0..width {
                ensure!(
                    emitted.u32(values[track] + i * 4)? == from.defaults[slot][lane + i],
                    "Implicit source pose changed on a native control"
                );
            }
            count += 1;
        }
    }
    Ok(count)
}

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    staged: &mut Reader,
    item: u32,
    graph: &Value,
    cases: &Value,
) -> Result<Value> {
    let cases = cases.as_array().context("Required first-person cases")?;
    ensure!(!cases.is_empty(), "Empty required first-person cases");
    let from = Controller::read(&graph["rendering"][0]["source_runtime"], true, |t| {
        source.read(t)
    })?;
    let original_rig = &graph["rendering"][0]["native_runtime"];
    let donor = Controller::read(original_rig, false, |t| Ok((*stock.tag(t, None)?).clone()))?;
    let live = parhelion_import::tiger::rig::inspect(staged, item, false)?;
    let to = Controller::read(&live, false, |t| Ok((*staged.tag(t, None)?).clone()))?;
    ensure!(
        to.lookup != donor.lookup,
        "Authored action is using stock lookup"
    );
    let unchanged = Controller::read(original_rig, false, |t| Ok((*staged.tag(t, None)?).clone()))?;
    ensure!(
        unchanged.bank.0 == donor.bank.0 && unchanged.states.0 == donor.states.0,
        "Stock animation donor changed"
    );
    let mut report = Vec::new();
    for case in cases {
        let name = case["action"].as_str().context("Action string")?;
        let hash = name.bytes().fold(0x811C9DC5u32, |v, b| {
            v.wrapping_mul(0x01000193) ^ u32::from(b)
        });
        let contexts = if case["retain_native"] == true {
            donor.contexts(hash)?
        } else {
            let mut domains = BTreeMap::<u8, BTreeSet<u32>>::new();
            for values in from.contexts(hash)? {
                for (input, name) in values {
                    domains.entry(input).or_default().insert(name);
                }
            }
            for values in donor.contexts(hash)? {
                for (input, name) in values {
                    let input = match input {
                        10 => 5,
                        11 => 6,
                        12 => 7,
                        14 => 9,
                        20 => 11,
                        21 => 12,
                        22 => 13,
                        0..=4 => input,
                        _ => anyhow::bail!("Unverified acceptance input {input} for {name}"),
                    };
                    domains.entry(input).or_default().insert(name);
                }
            }
            let mut contexts = vec![BTreeMap::new()];
            for (input, names) in domains {
                ensure!(
                    contexts.len().saturating_mul(names.len()) <= 4096,
                    "Combined action context corpus exceeds its bound"
                );
                contexts = contexts
                    .into_iter()
                    .flat_map(|values| {
                        names.iter().map(move |&name| {
                            let mut values = values.clone();
                            values.insert(input, name);
                            values
                        })
                    })
                    .collect();
            }
            contexts
        };
        for context in contexts {
            let native_context = context
                .iter()
                .map(|(&input, &name)| {
                    let input = if case["retain_native"] == true {
                        input
                    } else {
                        match input {
                            5 => 10,
                            6 => 11,
                            7 => 12,
                            9 => 14,
                            11 => 20,
                            12 => 21,
                            13 => 22,
                            _ => input,
                        }
                    };
                    (input, name)
                })
                .collect();
            let actual = to
                .clips(hash, &native_context)
                .with_context(|| format!("Staged action {name}, context {context:?}"))?;
            if case["retain_native"] == true {
                ensure!(
                    actual == donor.clips(hash, &context)?,
                    "Unrelated action route changed"
                );
                report.push(json!({"action":name,"context":context,"retained":actual}));
                continue;
            }
            let expected = from
                .clips(hash, &context)
                .with_context(|| format!("Source action {name}, context {context:?}"))?;
            ensure!(
                !expected.is_empty() && expected.len() == actual.len(),
                "Action choice coverage changed for {name}: {} source choices, {} staged choices",
                expected.len(),
                actual.len()
            );
            for ((s, sw, _), (n, nw, descriptor)) in expected.into_iter().zip(actual) {
                ensure!(sw == nw, "Action probabilities changed");
                if let Some(oracle) = case.get("source_clip") {
                    ensure!(
                        s == identity(oracle)?,
                        "Source selection differs from the configured oracle"
                    );
                }
                let a = source.read(s)?;
                let b = staged.tag(n, Some(0x80808F49))?;
                ensure!(a.u16(0x128)? == b.u16(0x13C)?, "Source duration was lost");
                let expected_events = events(&a, true, |t| {
                    source.read(t)?;
                    Ok(())
                })?;
                let actual_events = events(&b, false, |t| {
                    staged.tag(t, None)?;
                    Ok(())
                })?;
                ensure!(
                    expected_events == actual_events,
                    "Action {name} event order, timing, names or dispatch changed in {context:?}"
                );
                let samples = motion(&a, &b, &from.slots, &to.slots, &from.groups, &to.groups)
                    .with_context(|| format!("Motion for {name}, {context:?}"))?;
                let constraints = constraints(&a, &b)
                    .with_context(|| format!("Constraints for {name}, {context:?}"))?;
                let defaults = implicit_pose(&a, &b, &from, &to)?;
                report.push(json!({"action":name,"context":context,"source":s,"emitted":n,"descriptor":descriptor,"frames":b.u16(0x13C)?,"events":actual_events,"motion":samples,"constraints":constraints,"source_default_channels":defaults,"source_sha256":hex::encode(Sha256::digest(&a.0)),"emitted_sha256":hex::encode(Sha256::digest(&b.0))}));
            }
        }
    }
    Ok(
        json!({"source_profile":from.profile,"emitted_profile":to.profile,"actions":report,"gameplay_verified":false}),
    )
}
