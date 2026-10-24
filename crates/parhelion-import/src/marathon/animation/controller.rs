//! Source action lookups, specialized into the actions a native state table names.
//!
//! An `80803936` lookup names a bank `80803BE9` at `+E8`, a dictionary `8080ACBF` at `+EC` and
//! states `80803950` at `+F4`. State records hold a 24-byte ancestry mask with sentinel bit 191,
//! then the name at `+18`, refinement and alternate children at `+1C` and `+1E` and a signed
//! result at `+20`. A negative result selects choice `!result` and a positive one the operation
//! `result - 1`. An operation's low byte names its builtin and its high half its root record.
//! The traversal follows the native `F67190` rules. Every builtin without a declared producer
//! takes the unset name `811C9DC5`, so an emitted route is fully specialized.
use super::*;

pub(super) const LOOKUP: u32 = 0x8080_3936;
pub(super) const UNSET: u32 = 0x811C_9DC5;

pub(super) struct Descriptor {
    pub name: u32,
    pub rate: f32,
    /// Ordinal in the bank's clip array, or `None` when the descriptor is unbound.
    pub clip: Option<usize>,
}

pub(super) struct Controller {
    pub bank_tag: u32,
    pub clips: Vec<u32>,
    pub descriptors: Vec<Descriptor>,
    builtins: [u16; 24],
    masks: Vec<BTreeMap<u32, [u8; 24]>>,
    pub names: BTreeMap<u32, usize>,
    nodes: Vec<Node>,
}

struct Node {
    choices: Vec<Vec<(u32, f32)>>,
    records: Vec<[u8; 36]>,
    operations: Vec<u32>,
}

fn weights(p: &Payload, at: usize) -> Result<Vec<(u32, f32)>> {
    let mut previous = 0.;
    p.array(at, 8, Some(0x8080_3924))?
        .into_iter()
        .map(|row| {
            let weight = p.f32(row + 4)?;
            ensure!(
                weight.is_finite() && weight >= previous && weight <= 1.,
                "Source action weights are not cumulative"
            );
            previous = weight;
            Ok((p.u32(row)?, weight))
        })
        .collect()
}

fn short(record: &[u8; 36], at: usize) -> i16 {
    i16::from_le_bytes([record[at], record[at + 1]])
}

impl Controller {
    pub fn read(reader: &mut Reader, lookup: &Payload) -> Result<Self> {
        let base = lookup.pointer(24)?;
        ensure!(
            lookup.u32(base - 4)? == LOOKUP,
            "Source action lookup class differs"
        );
        let bank_tag = lookup.u32(base + 0xE8)?;
        let bank = reader.tag(bank_tag, Some(0x8080_3BE9))?;
        let dictionary = reader.tag(lookup.u32(base + 0xEC)?, Some(0x8080_ACBF))?;
        let states = reader.tag(lookup.u32(base + 0xF4)?, Some(0x8080_3950))?;
        let clips = bank
            .array(8, 16, Some(0x8080_AE00))?
            .into_iter()
            .map(|at| reader.ref64(&bank, at))
            .collect::<Result<Vec<_>>>()?;
        let mut descriptors = Vec::new();
        for at in bank.array(0x58, 48, Some(0x8080_ADFF))? {
            ensure!(
                bank.u64(at)? == 0,
                "Source action descriptor has inline tracks"
            );
            let ordinal = bank.u32(at + 0x28)? & 0xFFFF;
            let clip = if ordinal == 0xFFFF {
                ensure!(
                    bank.u32(at + 0x18)? == u32::MAX && bank.u32(at + 0x1C)? == 1,
                    "Unbound source descriptor names a clip"
                );
                None
            } else {
                let ordinal = ordinal as usize;
                ensure!(
                    clips.get(ordinal) == Some(&reader.ref64(&bank, at + 0x18)?),
                    "Source descriptor and its clip ordinal disagree"
                );
                Some(ordinal)
            };
            descriptors.push(Descriptor {
                name: bank.u32(at + 0x10)?,
                rate: bank.f32(at + 0x14)?,
                clip,
            });
        }
        let mut builtins = [0; 24];
        for (i, builtin) in builtins.iter_mut().enumerate() {
            *builtin = dictionary.u16(0x18 + i * 2)?;
        }
        let mut masks = Vec::new();
        for group in dictionary.array(8, 32, Some(0x8080_ACC3))? {
            let mut rows = BTreeMap::new();
            for at in dictionary.array(group + 0x10, 32, Some(0x8080_ACC6))? {
                ensure!(
                    dictionary.u8(at + 23)? & 128 != 0,
                    "Source parameter mask lacks its sentinel"
                );
                rows.insert(dictionary.u32(at + 0x18)?, dictionary.bytes::<24>(at)?);
            }
            masks.push(rows);
        }
        ensure!(
            builtins
                .iter()
                .all(|&g| g == u16::MAX || usize::from(g) < masks.len()),
            "Source builtin addresses an absent dictionary group"
        );
        let mut nodes = Vec::new();
        for row in states.array(0x18, 16, Some(0x8080_391A))? {
            let at = states.pointer(row + 8)?;
            nodes.push(match states.u64(row)? {
                1 => Node {
                    choices: vec![weights(&states, at)?],
                    records: Vec::new(),
                    operations: Vec::new(),
                },
                2 => Node {
                    choices: states
                        .array(at, 16, Some(0x8080_3922))?
                        .into_iter()
                        .map(|choice| weights(&states, choice))
                        .collect::<Result<_>>()?,
                    records: states
                        .array(at + 0x20, 36, Some(0x8080_3968))?
                        .into_iter()
                        .map(|record| states.bytes::<36>(record))
                        .collect::<Result<_>>()?,
                    operations: states
                        .array(at + 0x30, 4, Some(0x8080_3967))?
                        .into_iter()
                        .map(|op| states.u32(op))
                        .collect::<Result<_>>()?,
                },
                kind => anyhow::bail!("Source action node kind {kind} is untranslated"),
            });
        }
        let mut names = BTreeMap::new();
        for row in states.array(8, 8, Some(0x8080_3919))? {
            let node = states.u32(row + 4)? as usize;
            ensure!(
                node < nodes.len() && names.insert(states.u32(row)?, node).is_none(),
                "Source action names are ambiguous"
            );
        }
        for node in &nodes {
            for &(descriptor, _) in node.choices.iter().flatten() {
                ensure!(
                    (descriptor as usize) < descriptors.len(),
                    "Source action addresses an absent descriptor"
                );
            }
        }
        Ok(Self {
            bank_tag,
            clips,
            descriptors,
            builtins,
            masks,
            names,
            nodes,
        })
    }

    /// Whether a name occurs in the dictionary group a builtin reads.
    pub fn declares(&self, builtin: u8, name: u32) -> bool {
        let group = self.builtins[usize::from(builtin)];
        group != u16::MAX && self.masks[usize::from(group)].contains_key(&name)
    }

    fn input(&self, builtin: u8, name: u32) -> ([u8; 24], u32) {
        let group = self.builtins[usize::from(builtin)];
        let mask = if group == u16::MAX {
            [0; 24]
        } else {
            self.masks[usize::from(group)]
                .get(&name)
                .copied()
                .unwrap_or([0; 24])
        };
        (mask, name)
    }

    fn chosen(
        node: &Node,
        index: usize,
        input: &([u8; 24], u32),
        active: &mut BTreeSet<usize>,
    ) -> Result<Option<i16>> {
        ensure!(active.insert(index), "Source selector records form a cycle");
        let record = node
            .records
            .get(index)
            .context("Source selector record is absent")?;
        let child = |at| -> Result<Option<usize>> {
            let value = short(record, at);
            ensure!(
                value >= -1 && (value < 0 || (value as usize) < node.records.len()),
                "Source selector child is outside its records"
            );
            Ok((value >= 0).then_some(value as usize))
        };
        let (refinement, alternate) = (child(28)?, child(30)?);
        let output = if u32::from_le_bytes(record[24..28].try_into()?) == input.1 {
            Some(short(record, 32))
        } else {
            let matches = record[23] & input.0[23] & 128 != 0
                && record[..24]
                    .iter()
                    .zip(&input.0)
                    .all(|(&required, &value)| required & !value == 0);
            let refined = if matches {
                refinement
                    .map(|c| Self::chosen(node, c, input, active))
                    .transpose()?
                    .flatten()
            } else {
                None
            };
            if matches && refined.is_none() {
                Some(short(record, 32))
            } else {
                alternate
                    .map(|c| Self::chosen(node, c, input, active))
                    .transpose()?
                    .flatten()
                    .or(refined)
            }
        };
        active.remove(&index);
        Ok(output)
    }

    /// The weighted descriptors a named action selects for fixed builtin names.
    pub fn route(&self, action: u32, fixed: &BTreeMap<u8, u32>) -> Result<Vec<(u32, f32)>> {
        let node = &self.nodes[*self.names.get(&action).context("Source action is absent")?];
        if node.operations.is_empty() {
            return Ok(node.choices[0].clone());
        }
        let mut operation = 0;
        let mut visited = BTreeSet::new();
        loop {
            ensure!(
                visited.insert(operation),
                "Source selector operations form a cycle"
            );
            let op = *node
                .operations
                .get(operation)
                .context("Source selector operation is absent")?;
            ensure!(
                op & 0xFF00 == 0 && (op & 0xFF) < 24,
                "Source selector operation is untranslated"
            );
            let builtin = op as u8;
            let root = (op >> 16) as usize;
            let input = self.input(builtin, fixed.get(&builtin).copied().unwrap_or(UNSET));
            let result = match Self::chosen(node, root, &input, &mut BTreeSet::new())? {
                Some(result) => result,
                None => short(
                    node.records
                        .get(root)
                        .context("Source selector root is absent")?,
                    32,
                ),
            };
            if result < 0 {
                let choice = node
                    .choices
                    .get((!result) as usize)
                    .context("Source selector chose an absent choice")?;
                ensure!(
                    !choice.is_empty() && choice.last().map(|c| c.1) == Some(1.),
                    "Source action choice does not cover its interval"
                );
                return Ok(choice.clone());
            }
            ensure!(result > 0, "Source selector has a zero result");
            operation = (result - 1) as usize;
        }
    }
}
