//! Independent selection from source and staged attachment profiles into real clip payloads.
use super::*;

fn owner(rig: &Value, class: &str, first_person: bool) -> Result<u32> {
    let owners = rig["components"]
        .as_array()
        .context("Rig components")?
        .iter()
        .filter(|c| c["class"] == class && (c["entity"] != rig["runtime_entity"]) == first_person)
        .map(|c| number(&c["owner"]))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        owners.len() == 1,
        "Required rig owner is missing or ambiguous"
    );
    Ok(owners[0])
}

struct Controller {
    bank_tag: u32,
    states_tag: u32,
    bank: Payload,
    states: Payload,
    dictionary: Payload,
    profile: u32,
    modern: bool,
    clips: Vec<u32>,
    descriptors: Vec<usize>,
}

impl Controller {
    fn read(reader: &mut Reader, rig: &Value, modern: bool) -> Result<Self> {
        let lookup = reader.tag(
            owner(rig, if modern { "808025F8" } else { "8080344B" }, true)?,
            None,
        )?;
        let at = lookup.pointer(24)? + if modern { 0xA8 } else { 0x90 };
        let bank_tag = lookup.u32(at)?;
        let states_tag = lookup.u32(at + 12)?;
        let bank = (*reader.tag(bank_tag, None)?).clone();
        let attachment = reader.tag(
            owner(rig, if modern { "8080356E" } else { "80804221" }, false)?,
            None,
        )?;
        let rows = rig::attachment_rows(
            &attachment,
            attachment.pointer(24)?,
            number(&rig["content_key"])?,
            modern,
        )?;
        ensure!(rows.len() == 1, "Attachment profile is ambiguous");
        let profile = attachment.u32(rows[0] + if modern { 0x94 } else { 0x6C })?;
        let clips = bank
            .array(8, if modern { 16 } else { 4 }, None)?
            .into_iter()
            .map(|row| {
                if modern {
                    reader.ref64(&bank, row)
                } else {
                    bank.u32(row)
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let descriptors = bank.array(
            if modern { 0x58 } else { 0x68 },
            if modern { 48 } else { 32 },
            None,
        )?;
        Ok(Self {
            bank_tag,
            states_tag,
            bank,
            clips,
            descriptors,
            profile,
            modern,
            states: (*reader.tag(states_tag, None)?).clone(),
            dictionary: (*reader.tag(lookup.u32(at + 4)?, None)?).clone(),
        })
    }

    fn contexts(&self, name: &str) -> Result<Vec<BTreeMap<u8, u32>>> {
        let hash = name.bytes().fold(0x811C9DC5u32, |h, b| {
            h.wrapping_mul(0x1000193) ^ u32::from(b)
        });
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
        let width = if self.modern { 24 } else { 16 };
        let mut contexts = vec![BTreeMap::new()];
        for row in self.states.array(at + 16, 1, None)? {
            let parameter = self.states.u8(row)?;
            if parameter == 3 {
                continue;
            }
            let group_index = self.dictionary.u16(24 + usize::from(parameter) * 2)?;
            let names = if group_index == u16::MAX {
                // Unbound inputs retain their raw name with a zero mask. Every
                // literal in this input's record trees plus one unmatched value
                // covers all possible exact-name outcomes without guessing the
                // runtime producer's default. Original native lookup and selector
                // replay establishes this contract independently of conversion.
                let records = self.states.array(at + 32, width + 12, None)?;
                let mut pending = self
                    .states
                    .array(at + 48, 4, None)?
                    .into_iter()
                    .map(|r| self.states.u32(r))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .filter(|op| *op as u8 == parameter)
                    .map(|op| (op >> 16) as usize)
                    .collect::<Vec<_>>();
                let mut seen = BTreeSet::new();
                let mut names = BTreeSet::new();
                while let Some(index) = pending.pop() {
                    if !seen.insert(index) {
                        continue;
                    }
                    let record = *records.get(index).context("Unbound selector record")?;
                    names.insert(self.states.u32(record + width)?);
                    for field in [width + 4, width + 6] {
                        let child = self.states.i16(record + field)?;
                        ensure!(child >= -1, "Invalid unbound selector child");
                        if child >= 0 {
                            pending.push(child as usize);
                        }
                    }
                }
                let unmatched = (0..=u32::MAX)
                    .find(|v| !names.contains(v))
                    .context("No unmatched selector name")?;
                names.insert(unmatched);
                names
            } else {
                let group = *groups
                    .get(usize::from(group_index))
                    .context("Dynamic action domain")?;
                self.dictionary
                    .array(group + 16, width + 8, None)?
                    .into_iter()
                    .map(|r| self.dictionary.u32(r + width))
                    .collect::<Result<BTreeSet<_>>>()?
            };
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

    fn selected(&self, name: &str, context: &BTreeMap<u8, u32>) -> Result<Vec<(u32, u32)>> {
        let hash = name.bytes().fold(0x811C9DC5u32, |h, b| {
            h.wrapping_mul(0x1000193) ^ u32::from(b)
        });
        let s = &self.states;
        let entry = s
            .array(8, 8, None)?
            .into_iter()
            .find(|&r| s.u32(r).ok() == Some(hash))
            .with_context(|| format!("Missing action {name}"))?;
        let nodes = s.array(24, 16, None)?;
        let node = *nodes
            .get(s.u32(entry + 4)? as usize)
            .context("Action node")?;
        let at = s.pointer(node + 8)?;
        let choice = match s.u64(node)? {
            1 => at,
            2 => {
                let width = if self.modern { 24 } else { 16 };
                let groups = self.dictionary.array(8, 32, None)?;
                let records = s.array(at + 32, width + 12, None)?;
                let operations = s.array(at + 48, 4, None)?;
                let mut index = 0;
                let mut seen = BTreeSet::new();
                let selected = loop {
                    ensure!(seen.insert(index), "Selector operation cycle");
                    let op = s.u32(*operations.get(index).context("Selector operation")?)?;
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
                            .find(|&r| self.dictionary.u32(r + width).ok() == Some(value_name))
                            .context("Action input identity")?;
                        mask.copy_from_slice(&self.dictionary.0[row..row + width + 4]);
                    }
                    let root = (op >> 16) as usize;
                    let value = select(s, &records, &mask, root, &mut BTreeSet::new())?.unwrap_or(
                        s.i16(*records.get(root).context("Selector root")? + width + 8)?,
                    );
                    if value < 0 {
                        break (!value) as usize;
                    }
                    ensure!(value > 0, "Selector result is empty");
                    index = value as usize - 1;
                };
                *s.array(at, 16, None)?
                    .get(selected)
                    .context("Selected choice")?
            }
            _ => anyhow::bail!("Unsupported acceptance action"),
        };
        s.array(choice, 8, None)?
            .into_iter()
            .map(|row| {
                let descriptor = *self
                    .descriptors
                    .get(s.u32(row)? as usize)
                    .context("Clip descriptor")?;
                let slot = if self.modern {
                    self.bank.u32(descriptor + 40)? as usize
                } else {
                    usize::from(self.bank.u16(descriptor + 24)?)
                };
                Ok((
                    *self.clips.get(slot).context("Descriptor clip slot")?,
                    s.u32(row + 4)?,
                ))
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
    let at = *rows.get(index).context("Selector record")?;
    let exact = p.0[at + width..at + width + 4] == mask[width..];
    let contains = p.u8(at + width - 1)? & mask[width - 1] & 128 != 0
        && (0..width).all(|i| p.0[at + i] & !mask[i] == 0);
    let result = if exact {
        Some(p.i16(at + width + 8)?)
    } else {
        let child = p.i16(at + width + 4)?;
        let next = p.i16(at + width + 6)?;
        let refined = if contains && child >= 0 {
            select(p, rows, mask, child as usize, seen)?
        } else {
            None
        };
        if contains && refined.is_none() {
            Some(p.i16(at + width + 8)?)
        } else if next >= 0 {
            select(p, rows, mask, next as usize, seen)?.or(refined)
        } else {
            refined
        }
    };
    seen.remove(&index);
    Ok(result)
}

fn motion(source: &Payload, emitted: &Payload) -> Result<Value> {
    let a = source.pointer(0x18)?;
    let b = emitted.pointer(0x18)?;
    let codec = source.u16(a)?;
    ensure!(emitted.u16(b)? == codec, "Selected motion codec changed");
    let fields: &[(usize, usize)] = match codec {
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
        _ => anyhow::bail!("Configure a captured encoded-motion oracle for codec {codec}"),
    };
    let mut bytes = Vec::new();
    for &(field, stride) in fields {
        let expected = &source.0[source.array_range(a + field, stride, None)?];
        let actual = &emitted.0[emitted.array_range(b + field, stride, None)?];
        ensure!(
            actual == expected,
            "Selected action lost source motion samples"
        );
        bytes.extend_from_slice(actual);
    }
    ensure!(
        !bytes.is_empty(),
        "Selected action has no dynamic source motion"
    );
    Ok(json!({"codec":codec,"sample_sha256":hex::encode(Sha256::digest(bytes))}))
}

fn events(
    source: &Payload,
    emitted: &Payload,
    stock: &mut Reader,
    staged: &mut Reader,
    case: &Value,
) -> Result<Value> {
    let a = source.array(0x160, 8, Some(0x80808C08))?;
    let b = emitted.array(0x160, 8, Some(0x8080902B))?;
    ensure!(a.len() == b.len(), "Selected action lost source events");
    let mut events = Vec::new();
    for (a, b) in a.into_iter().zip(b) {
        let a = source.pointer(a)?;
        let b = emitted.pointer(b)?;
        let kind = source.u32(a)?;
        ensure!(
            kind == emitted.u32(b)?,
            "Selected action changed event kind, frame or order"
        );
        let class = source.u32(a - 4)?;
        let expected = match class {
            0x80808C11 => 0x80809031,
            0x80808C12 => 0x80809032,
            0x80808C13 => 0x80809033,
            0x80808C1A => 0x8080903A,
            _ => anyhow::bail!("Required source event has no captured class mapping"),
        };
        ensure!(
            emitted.u32(b - 4)? == expected,
            "Selected action changed event class"
        );
        if class == 0x80808C11 {
            ensure!(
                source.u32(a + 8)? == emitted.u32(b + 8)?,
                "Selected action changed named event"
            );
        }
        if class == 0x80808C1A && source.u32(a + 4)? == 6 {
            ensure!(
                emitted.u16(b + 4)? == 6,
                "Captured selector mapping changed"
            );
        }
        if class == 0x80808C13 {
            let text = |p: &Payload, at| -> Result<Vec<u8>> {
                let start = p.pointer(at)?;
                let tail = p.0.get(start..).context("Event path outside payload")?;
                let size = tail
                    .iter()
                    .position(|b| *b == 0)
                    .context("Event path terminator")?;
                Ok(tail[..size].to_vec())
            };
            ensure!(
                text(source, a + 16)? == text(emitted, b + 16)?,
                "Selected action changed event resource path"
            );
            for witness in case["event_resource_witnesses"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let expected_path = witness["path"].as_str().context("Witness path")?.as_bytes();
                if text(source, a + 16)? != expected_path {
                    continue;
                }
                let original = stock.tag(number(&witness["native_clip"])?, Some(0x80808F49))?;
                let matches = original
                    .array(0x160, 8, Some(0x8080902B))?
                    .into_iter()
                    .map(|r| original.pointer(r))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .filter(|&r| {
                        original.u32(r - 4).ok() == Some(0x80809033)
                            && text(&original, r + 16).ok().as_deref() == Some(expected_path)
                    })
                    .map(|r| original.u32(r + 24))
                    .collect::<Result<BTreeSet<_>>>()?;
                ensure!(
                    matches.len() == 1 && matches.contains(&emitted.u32(b + 24)?),
                    "Event resource differs from its independent native witness"
                );
                let tag = emitted.u32(b + 24)?;
                ensure!(
                    stock.tag(tag, None)?.0 == staged.tag(tag, None)?.0,
                    "Resolved stock event resource changed"
                );
            }
        }
        events.push(json!({"class":class,"kind":kind >> 16,"frame":kind & 65535}));
    }
    Ok(json!({"ordered_events":events,"resource_execution_verified":false}))
}

pub(super) fn check(
    source: &mut Reader,
    stock: &mut Reader,
    staged: &mut Reader,
    source_rig: &Value,
    donor_rig: &Value,
    emitted_rig: &Value,
    case: &Value,
) -> Result<Value> {
    let from = Controller::read(source, source_rig, true)?;
    let donor = Controller::read(stock, donor_rig, false)?;
    let to = Controller::read(staged, emitted_rig, false)?;
    ensure!(
        to.states_tag != donor.states_tag && to.bank_tag != donor.bank_tag,
        "Authored animation still uses stock routing"
    );
    ensure!(
        staged.tag(donor.states_tag, None)?.0 == donor.states.0
            && staged.tag(donor.bank_tag, None)?.0 == donor.bank.0,
        "Stock donor animation changed"
    );
    let mut actions = Vec::new();
    for action in case["actions"].as_array().context("Required actions")? {
        let name = action.as_str().context("Action name")?;
        for context in from.contexts(name)? {
            let expected = from.selected(name, &context)?;
            let actual = to.selected(name, &context)?;
            ensure!(
                !expected.is_empty() && expected.len() == actual.len(),
                "Source action alternatives changed"
            );
            for ((source_tag, source_weight), (emitted_tag, weight)) in
                expected.into_iter().zip(actual)
            {
                ensure!(
                    source_weight == weight,
                    "Source action probabilities changed"
                );
                let a = source.tag(source_tag, Some(0x80808BE0))?;
                let b = staged.tag(emitted_tag, Some(0x80808F49))?;
                ensure!(
                    a.u32(0x120)? == b.u32(0x120)?,
                    "Selected source clip identity was lost for {name}"
                );
                ensure!(
                    a.u16(0x140)? == b.u16(0x13C)?,
                    "Selected source duration was lost for {name}"
                );
                let samples = motion(&a, &b).with_context(|| format!("Source action {name}"))?;
                let events = events(&a, &b, stock, staged, case)
                    .with_context(|| format!("Source action {name}"))?;
                actions.push(
                json!({"action":name,"context":context,"source_clip":source_tag,"emitted_clip":emitted_tag,
                "frames":b.u16(0x13C)?,"motion":samples,"events":events}),
            );
            }
        }
    }
    let mut retained = Vec::new();
    if let Some(names) = case["retained_actions"].as_array() {
        for name in names {
            let name = name.as_str().context("Retained action name")?;
            for context in donor.contexts(name)? {
                let expected = donor.selected(name, &context)?;
                let actual = to.selected(name, &context)?;
                ensure!(
                    expected.len() == actual.len() && !expected.is_empty(),
                    "Native fallback alternatives changed for {name}"
                );
                for ((stock_tag, expected_weight), (emitted_tag, weight)) in
                    expected.into_iter().zip(actual)
                {
                    let a = stock.tag(stock_tag, Some(0x80808F49))?;
                    let b = staged.tag(emitted_tag, Some(0x80808F49))?;
                    ensure!(
                        expected_weight == weight
                            && a.u32(0x120)? == b.u32(0x120)?
                            && a.u16(0x13C)? == b.u16(0x13C)?,
                        "Native fallback selected another clip for {name}"
                    );
                    let headers = |p: &Payload| -> Result<Vec<u32>> {
                        p.array(0x160, 8, Some(0x8080902B))?
                            .into_iter()
                            .map(|at| p.u32(p.pointer(at)?))
                            .collect()
                    };
                    ensure!(
                        headers(&a)? == headers(&b)?,
                        "Native fallback events changed"
                    );
                    retained.push(
                        json!({"action":name,"context":context,"stock_clip":stock_tag,
                    "emitted_clip":emitted_tag,"clip_name":a.u32(0x120)?,
                    "frames":a.u16(0x13C)?,"weight":weight}),
                    );
                }
            }
        }
    }
    Ok(
        json!({"name":case["name"],"source_profile":from.profile,"emitted_profile":to.profile,
        "actions":actions,"retained_actions":retained,"stock_unchanged":true}),
    )
}
