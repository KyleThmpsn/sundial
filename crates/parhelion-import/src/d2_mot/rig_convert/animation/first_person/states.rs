//! Carry source state choices through named parameters and bank descriptor indices.
//! The private dictionary contains the names these states can actually address.
use super::*;
use crate::d2_mot::payload::Payload;
mod profile;

#[derive(Clone)]
struct Dictionary {
    groups: Vec<Vec<(u32, u16)>>,
    header: Vec<u8>,
}
impl Dictionary {
    fn read(p: &Payload, modern: bool) -> Result<Self> {
        let mut groups = Vec::new();
        for row in p.array(8, 32, Some(if modern { 0x80808ABA } else { 0x80808EE5 }))? {
            let mut names = Vec::new();
            for name in p.array(row, 8, Some(if modern { 0x80808ABE } else { 0x80808EE9 }))? {
                names.push((p.u32(name)?, p.u16(name + 4)?));
            }
            ensure!(
                names.len() <= if modern { 191 } else { 127 },
                "animation dictionary exceeds bit capacity"
            );
            ensure!(
                names
                    .iter()
                    .enumerate()
                    .all(|(i, (_, parent))| *parent == u16::MAX || usize::from(*parent) < i),
                "animation parameter ancestry cycles"
            );
            groups.push(names);
        }
        ensure!(
            !groups.is_empty() && groups.len() <= 24,
            "animation parameter groups"
        );
        Ok(Self {
            groups,
            header: p.0.get(24..72).context("parameter builtin map")?.to_vec(),
        })
    }
    fn group(&self, name: u32, bits: &[u8]) -> Result<usize> {
        let candidates = self
            .groups
            .iter()
            .enumerate()
            .filter(|(_, names)| names.iter().any(|(n, _)| *n == name))
            .collect::<Vec<_>>();
        let (index, names) = *candidates
            .first()
            .context("selector parameter name absent from dictionary")?;
        ensure!(
            candidates.iter().all(|(_, other)| {
                (0..bits.len() * 8 - 1)
                    .filter(|i| bits[i / 8] & (1 << (i % 8)) != 0)
                    .all(|i| other.get(i).map(|entry| entry.0) == names.get(i).map(|entry| entry.0))
            }),
            "selector parameter belongs to different groups"
        );
        ensure!(
            (0..bits.len() * 8 - 1)
                .filter(|i| bits[i / 8] & (1 << (i % 8)) != 0)
                .all(|i| i < names.len()),
            "selector mask addresses absent parameter"
        );
        Ok(index)
    }
}

#[derive(Clone, PartialEq)]
struct Record {
    group: Option<usize>,
    names: BTreeSet<u32>,
    sentinel: bool,
    tail: [u8; 12],
}
#[derive(Clone, PartialEq)]
struct Selector {
    parameters: Vec<u8>,
    records: Vec<Record>,
    operations: Vec<u32>,
}
#[derive(Clone, PartialEq)]
struct Node {
    choices: Vec<Vec<(u32, f32)>>,
    selector: Option<Selector>,
}
struct States {
    names: BTreeMap<u32, usize>,
    nodes: Vec<Node>,
}
impl States {
    fn read(p: &Payload, dict: &Dictionary, modern: bool) -> Result<Self> {
        let mut names = BTreeMap::new();
        for row in p.array(8, 8, Some(if modern { 0x808025DB } else { 0x8080342E }))? {
            ensure!(
                names
                    .insert(p.u32(row)?, p.u32(row + 4)? as usize)
                    .is_none(),
                "duplicate animation state name"
            );
        }
        let weighted = |at| -> Result<Vec<(u32, f32)>> {
            p.array(at, 8, Some(if modern { 0x808025E6 } else { 0x80803439 }))?
                .into_iter()
                .map(|r| Ok((p.u32(r)?, p.f32(r + 4)?)))
                .collect()
        };
        let mut nodes = Vec::new();
        for row in p.array(24, 16, Some(if modern { 0x808025DC } else { 0x8080342F }))? {
            let at = p.pointer(row + 8)?;
            let kind = p.u64(row)?;
            if kind == 1 {
                nodes.push(Node {
                    choices: vec![weighted(at)?],
                    selector: None,
                });
                continue;
            }
            ensure!(kind == 2, "unsupported animation state kind {kind}");
            let choices = p
                .array(at, 16, Some(if modern { 0x808025E4 } else { 0x80803437 }))?
                .into_iter()
                .map(weighted)
                .collect::<Result<Vec<_>>>()?;
            let parameters = p
                .array(
                    at + 16,
                    1,
                    Some(if modern { 0x80808912 } else { 0x80808DA4 }),
                )?
                .into_iter()
                .map(|r| p.u8(r))
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                parameters.iter().all(|i| *i < 24),
                "source selector uses a new builtin parameter"
            );
            let mut records = Vec::new();
            let width = if modern { 24 } else { 16 };
            for record in p.array(
                at + 32,
                width + 12,
                Some(if modern { 0x8080262A } else { 0x8080347D }),
            )? {
                let bits = &p.0[record..record + width];
                let tail = p.bytes::<12>(record + width)?;
                let name = u32::from_le_bytes(tail[..4].try_into()?);
                let sentinel = bits[width - 1] & 128 != 0;
                let active = (0..width * 8 - 1)
                    .filter(|i| bits[i / 8] & (1 << (i % 8)) != 0)
                    .collect::<Vec<_>>();
                let group = if active.is_empty() {
                    None
                } else {
                    Some(dict.group(name, bits)?)
                };
                let names = if let Some(group) = group {
                    active.iter().map(|i| dict.groups[group][*i].0).collect()
                } else {
                    BTreeSet::new()
                };
                records.push(Record {
                    group,
                    names,
                    sentinel,
                    tail,
                });
            }
            let operations = p
                .array(
                    at + 48,
                    4,
                    Some(if modern { 0x80802629 } else { 0x8080347C }),
                )?
                .into_iter()
                .map(|r| p.u32(r))
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                p.0.get(at + 64..at + 120) == Some(&[0u8; 56]),
                "animation selector has unsupported extensions"
            );
            nodes.push(Node {
                choices,
                selector: Some(Selector {
                    parameters,
                    records,
                    operations,
                }),
            });
        }
        ensure!(
            names.values().all(|i| *i < nodes.len()),
            "state names reference missing nodes"
        );
        Ok(Self { names, nodes })
    }
}

/// A named action supplies playback-mode evidence independently of event data.
/// Only unanimous native choices can establish a mode for the source choices.
/// A source descriptor shared by actions with conflicting modes is excluded.
pub(super) fn playback_modes(
    source_parameters: &Payload,
    native_parameters: &Payload,
    source_states: &Payload,
    native_states: &Payload,
    native_bank: &Payload,
) -> Result<BTreeMap<u32, u16>> {
    let source = States::read(
        source_states,
        &Dictionary::read(source_parameters, true)?,
        true,
    )?;
    let native = States::read(
        native_states,
        &Dictionary::read(native_parameters, false)?,
        false,
    )?;
    let descriptors = native_bank.array(0x68, 32, Some(0x80809002))?;
    let mut evidence = BTreeMap::<u32, BTreeSet<u16>>::new();
    for (name, source_index) in &source.names {
        let Some(native_index) = native.names.get(name) else {
            continue;
        };
        let modes = native.nodes[*native_index]
            .choices
            .iter()
            .flatten()
            .map(|(index, _)| {
                native_bank.u16(
                    *descriptors
                        .get(*index as usize)
                        .context("state playback descriptor is outside the native bank")?
                        + 26,
                )
            })
            .collect::<Result<BTreeSet<_>>>()?;
        if modes.len() != 1 {
            continue;
        }
        for (index, _) in source.nodes[*source_index].choices.iter().flatten() {
            evidence.entry(*index).or_default().extend(&modes);
        }
    }
    Ok(evidence
        .into_iter()
        .filter_map(|(index, modes)| (modes.len() == 1).then(|| (index, *modes.first().unwrap())))
        .collect())
}

struct Write(Vec<u8>);
impl Write {
    fn word(&mut self, at: usize, value: u32) {
        self.0[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn wide(&mut self, at: usize, value: u64) {
        self.0[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    fn pointer(&mut self, at: usize, target: usize) {
        self.0[at..at + 8].copy_from_slice(&((target as i64) - (at as i64)).to_le_bytes());
    }
    fn allocation(&mut self, class: u32, size: usize, alignment: usize) -> usize {
        let at = (self.0.len() + 4 + alignment - 1) & !(alignment - 1);
        self.0.resize(at + size, 0);
        self.word(at - 4, class);
        at
    }
    fn array(&mut self, at: usize, count: usize, stride: usize, class: u32) -> usize {
        if count == 0 {
            self.0[at..at + 16].fill(0);
            return self.0.len();
        }
        let header = self.allocation(0x80809FBD, 16 + count * stride, 16);
        self.wide(at, count as u64);
        self.pointer(at + 8, header);
        self.wide(header, count as u64);
        self.word(header + 8, class);
        header + 16
    }
    fn finish(mut self) -> Vec<u8> {
        let len = self.0.len();
        self.wide(0, len as u64);
        self.0
    }
}

pub(super) struct Converted {
    pub parameters: Vec<u8>,
    pub states: Vec<u8>,
    pub source_states: usize,
    pub fallback_states: Vec<u32>,
    pub profile_choices: BTreeMap<u32, Vec<usize>>,
}

/// Rebuild native-callable state names with source choices wherever every descriptor
/// has converted. Unconvertible states retain an explicit native fallback.
pub(super) fn convert(
    source_parameters: &Payload,
    native_parameters: &Payload,
    source_states: &Payload,
    native_states: &Payload,
    descriptor_map: &BTreeMap<u32, u32>,
    required: &[u32],
) -> Result<Converted> {
    convert_for_profile(
        source_parameters,
        native_parameters,
        source_states,
        native_states,
        descriptor_map,
        required,
        None,
    )
}

/// A group's retained parameters: each source index with its name and parent.
type Kept = Vec<(usize, (u32, u16))>;
/// A group's retained source names mapped to their native bit ordinals.
type Ordinals = BTreeMap<u32, usize>;

/// Specialize every source node to the profile and record each name's choices.
fn profile_choices(
    ss: &mut States,
    source: &Dictionary,
    profile: Option<u32>,
) -> Result<BTreeMap<u32, Vec<usize>>> {
    let mut profile_choices = BTreeMap::new();
    if let Some(profile) = profile {
        for (index, node) in ss.nodes.iter_mut().enumerate() {
            let choices = profile::specialize(node, source, profile)?;
            for (name, target) in &ss.names {
                if *target == index {
                    profile_choices.insert(*name, choices.clone());
                }
            }
        }
    }
    Ok(profile_choices)
}

/// Rebuild each native node from its source counterpart where every name and
/// descriptor converts. Returns the nodes, the converted name count and the
/// names keeping their native fallback.
fn convert_nodes(
    ns: &States,
    ss: &States,
    descriptor_map: &BTreeMap<u32, u32>,
) -> (Vec<Node>, usize, Vec<u32>) {
    let mut nodes = Vec::new();
    let mut source_count = 0;
    let mut fallback = Vec::new();
    // Controllers outside this asset address the original node ordinals. Several
    // names may alias one node, and unnamed nodes can still be called directly.
    // Rebuilding one node per name silently changes the controller's dispatch
    // contract, including indices beyond its table. Keep every native ordinal.
    for (index, original) in ns.nodes.iter().enumerate() {
        let names = ns
            .names
            .iter()
            .filter_map(|(name, target)| (*target == index).then_some(*name))
            .collect::<Vec<_>>();
        let candidates = names
            .iter()
            .map(|name| ss.names.get(name).map(|i| &ss.nodes[*i]))
            .collect::<Option<Vec<_>>>();
        let converted = candidates
            .as_ref()
            .and_then(|candidates| candidates.first().copied())
            .filter(|node| {
                candidates
                    .as_ref()
                    .is_some_and(|candidates| candidates.iter().all(|other| *other == *node))
                    && node
                        .choices
                        .iter()
                        .flatten()
                        .all(|(descriptor, _)| descriptor_map.contains_key(descriptor))
            });
        let node = if let Some(node) = converted {
            let mut node = node.clone();
            for (descriptor, _) in node.choices.iter_mut().flatten() {
                *descriptor = descriptor_map[descriptor];
            }
            source_count += names.len();
            node
        } else {
            fallback.extend(names);
            original.clone()
        };
        nodes.push(node);
    }
    (nodes, source_count, fallback)
}

/// The source names each parameter group must keep addressable.
fn used_names(source: &Dictionary, required: &[u32], nodes: &[Node]) -> Result<Vec<BTreeSet<u32>>> {
    let mut used = vec![BTreeSet::new(); source.groups.len()];
    for (group, names) in source.groups.iter().enumerate() {
        // Non-profile groups are small and accept game-driven names beyond the
        // selector constants. Keep their complete input domain.
        if group != 0 {
            used[group].extend(names.iter().map(|(name, _)| *name));
        }
        used[group].insert(names.first().context("empty parameter group")?.0);
        for name in required {
            if names.iter().any(|(n, _)| n == name) {
                used[group].insert(*name);
            }
        }
    }
    for node in nodes {
        if let Some(selector) = &node.selector {
            for record in &selector.records {
                if let Some(group) = record.group {
                    used[group].extend(record.names.iter().copied());
                }
            }
        }
    }
    Ok(used)
}

/// Close each group's used names over their ancestry, then keep those names in
/// source order with their native bit ordinals.
fn select(source: &Dictionary, used: &mut [BTreeSet<u32>]) -> Result<(Vec<Kept>, Vec<Ordinals>)> {
    let mut kept = Vec::new();
    let mut maps = Vec::new();
    for (group, names) in source.groups.iter().enumerate() {
        for name in used[group].clone() {
            let mut index = names
                .iter()
                .position(|(n, _)| *n == name)
                .context("native selector name absent from source dictionary")?;
            loop {
                used[group].insert(names[index].0);
                let parent = names[index].1;
                if parent == u16::MAX {
                    break;
                }
                index = usize::from(parent);
            }
        }
        let names = names
            .iter()
            .enumerate()
            .filter(|(_, entry)| used[group].contains(&entry.0))
            .map(|(index, entry)| (index, *entry))
            .collect::<Vec<_>>();
        ensure!(
            names.len() <= 127,
            "selected source profiles need {} native bits",
            names.len()
        );
        maps.push(
            names
                .iter()
                .enumerate()
                .map(|(index, (_, entry))| (entry.0, index))
                .collect::<BTreeMap<_, _>>(),
        );
        kept.push(names);
    }
    Ok((kept, maps))
}

/// Write the native parameter dictionary for the kept names.
fn write_params(source: &Dictionary, kept: &[Kept], maps: &[Ordinals]) -> Result<Vec<u8>> {
    let mut params = Write(vec![0; 0x50]);
    params.0[24..72].copy_from_slice(&source.header);
    let rows = params.array(8, kept.len(), 32, 0x80808EE5);
    for (group, names) in kept.iter().enumerate() {
        let row = rows + group * 32;
        let entries = params.array(row, names.len(), 8, 0x80808EE9);
        for (index, (_, (name, parent))) in names.iter().enumerate() {
            params.word(entries + index * 8, *name);
            let parent = if *parent == u16::MAX {
                u16::MAX
            } else {
                u16::try_from(maps[group][&source.groups[group][usize::from(*parent)].0])?
            };
            params.0[entries + index * 8 + 4..entries + index * 8 + 6]
                .copy_from_slice(&parent.to_le_bytes());
        }
        let lookup = params.array(row + 16, names.len(), 24, 0x80808EE8);
        for (ordinal, (name, index)) in maps[group].iter().enumerate() {
            let at = lookup + ordinal * 24;
            let mut old = names[*index].0;
            loop {
                let new = maps[group][&source.groups[group][old].0];
                params.0[at + new / 8] |= 1 << (new % 8);
                let parent = source.groups[group][old].1;
                if parent == u16::MAX {
                    break;
                }
                old = usize::from(parent);
            }
            params.0[at + 15] |= 128;
            params.word(at + 16, *name);
            params.0[at + 20..at + 22].copy_from_slice(&(*index as u16).to_le_bytes());
        }
    }
    Ok(params.finish())
}

/// Write a selector's parameter list, records and operations at its allocation.
fn write_selector(states: &mut Write, at: usize, selector: &Selector, maps: &[Ordinals]) {
    let parameters = states.array(at + 16, selector.parameters.len(), 1, 0x80808DA4);
    states.0[parameters..parameters + selector.parameters.len()]
        .copy_from_slice(&selector.parameters);
    let records = states.array(at + 32, selector.records.len(), 28, 0x8080347D);
    for (i, record) in selector.records.iter().enumerate() {
        let pos = records + i * 28;
        if let Some(group) = record.group {
            for name in &record.names {
                let bit = maps[group][name];
                states.0[pos + bit / 8] |= 1 << (bit % 8);
            }
        }
        if record.sentinel {
            states.0[pos + 15] |= 128;
        }
        states.0[pos + 16..pos + 28].copy_from_slice(&record.tail);
    }
    let operations = states.array(at + 48, selector.operations.len(), 4, 0x8080347C);
    for (i, operation) in selector.operations.iter().enumerate() {
        states.word(operations + i * 4, *operation);
    }
}

/// Write the native state table with every native name and node ordinal.
fn write_states(ns: &States, nodes: &[Node], maps: &[Ordinals]) -> Result<Vec<u8>> {
    let mut states = Write(vec![0; 40]);
    let names = states.array(8, ns.names.len(), 8, 0x8080342E);
    let rows = states.array(24, nodes.len(), 16, 0x8080342F);
    for (index, (name, target)) in ns.names.iter().enumerate() {
        states.word(names + index * 8, *name);
        states.word(names + index * 8 + 4, u32::try_from(*target)?);
    }
    for (index, node) in nodes.iter().enumerate() {
        let kind = if node.selector.is_some() { 2 } else { 1 };
        let at = states.allocation(
            if kind == 2 { 0x80803432 } else { 0x80803434 },
            if kind == 2 { 120 } else { 16 },
            8,
        );
        states.wide(rows + index * 16, kind);
        states.pointer(rows + index * 16 + 8, at);
        let choices = if kind == 2 {
            states.array(at, node.choices.len(), 16, 0x80803437)
        } else {
            at
        };
        for (choice, clips) in node.choices.iter().enumerate() {
            let entries = states.array(choices + choice * 16, clips.len(), 8, 0x80803439);
            for (i, (clip, weight)) in clips.iter().enumerate() {
                states.word(entries + i * 8, *clip);
                states.word(entries + i * 8 + 4, weight.to_bits());
            }
        }
        if let Some(selector) = &node.selector {
            write_selector(&mut states, at, selector, maps);
        }
    }
    Ok(states.finish())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn convert_for_profile(
    source_parameters: &Payload,
    native_parameters: &Payload,
    source_states: &Payload,
    native_states: &Payload,
    descriptor_map: &BTreeMap<u32, u32>,
    required: &[u32],
    profile: Option<u32>,
) -> Result<Converted> {
    let source = Dictionary::read(source_parameters, true)?;
    let native = Dictionary::read(native_parameters, false)?;
    ensure!(
        source.header == native.header && source.groups.len() == native.groups.len(),
        "animation builtin parameter map differs"
    );
    let mut ss = States::read(source_states, &source, true)?;
    let profile_choices = profile_choices(&mut ss, &source, profile)?;
    let ns = States::read(native_states, &native, false)?;
    let (nodes, source_count, fallback) = convert_nodes(&ns, &ss, descriptor_map);
    ensure!(
        source_count > 0,
        "no source animation states have converted clips"
    );
    let mut used = used_names(&source, required, &nodes)?;
    let (kept, maps) = select(&source, &mut used)?;
    let parameters = write_params(&source, &kept, &maps)?;
    let states = write_states(&ns, &nodes, &maps)?;
    Ok(Converted {
        parameters,
        states,
        source_states: source_count,
        fallback_states: fallback,
        profile_choices,
    })
}
