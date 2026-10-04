//! First-person actions taken from another animation profile one at a time.
//!
//! The arms rig's lookup component names a parameter dictionary at +0x94 (class 80808EE1) and an
//! action state table at +0x9C (class 80803465). The table maps each state name, an FNV-1 hash of
//! a plain name such as `fire`, to a node. A node's selector holds operations, and an operation of
//! kind 3 tests the attachment profile: it walks a tree of records from its root, matching the
//! weapon row's profile key or, through the dictionary's first group, its category, and outputs a
//! choice of clips or the next operation. Every frame of a hand cannon shares the table, so the
//! profile key decides, action by action, which clip each frame plays.
//!
//! Taking one action from another profile reduces its operations to the branch that profile
//! takes, by appending a record that always gives that branch's output. The other actions keep
//! testing the weapon's own profile. Nothing else in the table changes, so every clip, bank
//! ordinal and event stays as the rig shipped it. The rules follow the importer's
//! `first_person/states/profile.rs`, which reduces imported selectors the same way.
use crate::recipe::AnimationAction;
use crate::tag_payload::{append_native_array, array_at, read_u32, read_u64, relative_target};
use crate::{AuthoringResult, error::invalid};
use std::collections::{BTreeMap, BTreeSet};

const DICTIONARY_GROUP_CLASS: u32 = 0x8080_8EE5;
const DICTIONARY_NAME_CLASS: u32 = 0x8080_8EE9;
const STATE_NAME_CLASS: u32 = 0x8080_342E;
const STATE_NODE_CLASS: u32 = 0x8080_342F;
const RECORD_CLASS: u32 = 0x8080_347D;
const OPERATION_CLASS: u32 = 0x8080_347C;
/// A record's parameter bits, then its name, child, sibling and output.
const RECORD_BITS: usize = 16;
const RECORD_SIZE: usize = RECORD_BITS + 12;
/// The operation kind that tests the attachment profile.
const PROFILE: u32 = 3;
const EMPTY_NAME: u32 = 0x811C_9DC5;

impl AnimationAction {
    /// Every action, in the order the workbench lists them.
    pub(crate) const ALL: [Self; 7] = [
        Self::Fire,
        Self::AimFire,
        Self::Holster,
        Self::Sprint,
        Self::Slide,
        Self::Alert,
        Self::Reload,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Fire => "Hip Fire",
            Self::AimFire => "Aim Fire",
            Self::Holster => "Holster",
            Self::Sprint => "Sprint",
            Self::Slide => "Slide",
            Self::Alert => "Combat Stance",
            Self::Reload => "Reload Animation",
        }
    }

    /// What the action is, behind its row's info icon.
    pub(crate) const fn hint(self) -> &'static str {
        match self {
            Self::Fire => "Firing without aiming.",
            Self::AimFire => "Firing while aiming down sights.",
            Self::Holster => "Putting the weapon away on a swap.",
            Self::Sprint => "Starting, holding and ending a sprint.",
            Self::Slide => "Sliding out of a sprint.",
            Self::Alert => "Raising the weapon into combat and lowering it after.",
            Self::Reload => "Reloading the weapon.",
        }
    }

    /// The states an action is played through. Names match FNV-1 hashes in the Shadowkeep
    /// arms-rig census. Aliases such as `fire_1` and `fire` share a node and are visited once.
    const fn states(self) -> &'static [&'static str] {
        match self {
            Self::Fire => &["fire", "fire_1", "fire_auto"],
            Self::AimFire => &["iron_sight_fire", "iron_sight_fire_auto"],
            Self::Holster => &["put_away"],
            Self::Sprint => &[
                "sprint_enter",
                "sprint_loop",
                "sprint_exit",
                "sprint_loop_airborne",
            ],
            Self::Slide => &["crouch_slide"],
            Self::Alert => &["alert_enter", "alert_exit"],
            Self::Reload => &[
                "reload_full",
                "reload_empty",
                "reload_special",
                "reload_enter",
                "iron_sight_reload_full",
                "iron_sight_reload_empty",
            ],
        }
    }
}

struct Record {
    name: u32,
    child: Option<usize>,
    sibling: Option<usize>,
    output: i16,
    /// Marks a record that matches a profile hierarchy rather than one name.
    sentinel: bool,
    /// The dictionary group its parameter bits address, and the names they set.
    group: Option<usize>,
    names: BTreeSet<u32>,
}

/// One arms rig's action states, read from its state table and parameter dictionary.
pub(crate) struct Machine {
    states: Vec<u8>,
    /// Every dictionary group's names with each name's parent in the same group.
    groups: Vec<Vec<(u32, u16)>>,
    /// State name to node, and each node's selector, when it has one.
    names: BTreeMap<u32, usize>,
    selectors: Vec<Option<usize>>,
}

fn short(bytes: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn index(value: i16) -> AuthoringResult<Option<usize>> {
    if value < -1 {
        return Err(invalid("An animation selector has an invalid record index"));
    }
    Ok(usize::try_from(value).ok())
}

impl Machine {
    pub(crate) fn read(states: Vec<u8>, parameters: &[u8]) -> AuthoringResult<Self> {
        // Builtin 3, the attachment profile, has to address the dictionary's first group, as
        // every surveyed native dictionary does.
        if parameters.get(24 + 6..24 + 8) != Some(&[0, 0]) {
            return Err(invalid(
                "The animation dictionary maps the profile to another group",
            ));
        }
        let mut groups = Vec::new();
        for group in rows(parameters, 8, 32, DICTIONARY_GROUP_CLASS)? {
            let mut names = Vec::new();
            for name in rows(parameters, group, 8, DICTIONARY_NAME_CLASS)? {
                let parent = u16::from_le_bytes([parameters[name + 4], parameters[name + 5]]);
                names.push((read_u32(parameters, name)?, parent));
            }
            groups.push(names);
        }
        if groups.is_empty() {
            return Err(invalid("The animation dictionary has no groups"));
        }
        let mut names = BTreeMap::new();
        for row in rows(&states, 8, 8, STATE_NAME_CLASS)? {
            let node = usize::try_from(read_u32(&states, row + 4)?)
                .map_err(|_| invalid("Animation state node overflow"))?;
            names.insert(read_u32(&states, row)?, node);
        }
        let mut selectors = Vec::new();
        for row in rows(&states, 24, 16, STATE_NODE_CLASS)? {
            let kind = read_u64(&states, row)?;
            selectors.push(
                (kind == 2)
                    .then(|| relative_target(&states, row + 8))
                    .transpose()?,
            );
        }
        if names.values().any(|node| *node >= selectors.len()) {
            return Err(invalid("An animation state names a missing node"));
        }
        Ok(Self {
            states,
            groups,
            names,
            selectors,
        })
    }

    /// The state table as edited.
    pub(crate) fn into_payload(self) -> Vec<u8> {
        self.states
    }

    /// The profile and every category above it in the dictionary, or `None` when the dictionary
    /// does not name the profile.
    fn ancestors(&self, profile: u32) -> Option<BTreeSet<u32>> {
        let group = &self.groups[0];
        let mut matches = group.iter().enumerate().filter(|(_, row)| row.0 == profile);
        let (mut current, _) = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let mut ancestors = BTreeSet::new();
        // A parent chain longer than the group is a cycle.
        for _ in 0..=group.len() {
            ancestors.insert(group[current].0);
            let parent = group[current].1;
            if parent == u16::MAX {
                return Some(ancestors);
            }
            current = usize::from(parent);
            if current >= group.len() {
                return None;
            }
        }
        None
    }

    fn records(&self, selector: usize) -> AuthoringResult<Vec<Record>> {
        rows(&self.states, selector + 32, RECORD_SIZE, RECORD_CLASS)?
            .into_iter()
            .map(|at| {
                let bits = &self.states[at..at + RECORD_BITS];
                let tail = &self.states[at + RECORD_BITS..at + RECORD_SIZE];
                let name = u32::from_le_bytes(tail[..4].try_into().expect("four bytes"));
                let active = (0..RECORD_BITS * 8 - 1)
                    .filter(|bit| bits[bit / 8] & (1 << (bit % 8)) != 0)
                    .collect::<Vec<_>>();
                // The first group that holds the record's name is the one its bits address.
                let group = (!active.is_empty())
                    .then(|| {
                        self.groups
                            .iter()
                            .position(|names| names.iter().any(|(n, _)| *n == name))
                    })
                    .flatten();
                let names = group.map_or_else(BTreeSet::new, |group| {
                    active
                        .iter()
                        .filter_map(|bit| self.groups[group].get(*bit).map(|(n, _)| *n))
                        .collect()
                });
                Ok(Record {
                    name,
                    child: index(short(tail, 4))?,
                    sibling: index(short(tail, 6))?,
                    output: short(tail, 8),
                    sentinel: bits[RECORD_BITS - 1] & 0x80 != 0,
                    group,
                    names,
                })
            })
            .collect()
    }

    fn operations(&self, selector: usize) -> AuthoringResult<Vec<(usize, u32)>> {
        rows(&self.states, selector + 48, 4, OPERATION_CLASS)?
            .into_iter()
            .map(|at| Ok((at, read_u32(&self.states, at)?)))
            .collect()
    }

    /// The output the record tree at `index` gives `profile`, as the client walks it: a record
    /// naming the profile answers, a sentinel whose names are all the profile's ancestors answers
    /// unless a child does, and siblings are tried after.
    fn chosen(
        records: &[Record],
        index: usize,
        profile: u32,
        ancestors: &BTreeSet<u32>,
        visited: &mut BTreeSet<usize>,
    ) -> AuthoringResult<Option<i16>> {
        if !visited.insert(index) {
            return Err(invalid("An animation selector contains a cycle"));
        }
        let record = records
            .get(index)
            .ok_or_else(|| invalid("An animation selector record is out of range"))?;
        if record.name == profile {
            return Ok(Some(record.output));
        }
        let mut result = None;
        if record.sentinel && record.group == Some(0) && record.names.is_subset(ancestors) {
            if let Some(child) = record.child {
                result = Self::chosen(records, child, profile, ancestors, visited)?;
            }
            if result.is_none() {
                return Ok(Some(record.output));
            }
        }
        if let Some(sibling) = record.sibling {
            result = Self::chosen(records, sibling, profile, ancestors, visited)?.or(result);
        }
        Ok(result)
    }

    /// The selectors an action plays through, each once.
    fn selectors(&self, action: AnimationAction) -> Vec<usize> {
        let mut nodes = action
            .states()
            .iter()
            .filter_map(|name| {
                self.names
                    .get(&sundial::package_authoring::fnv1_name_hash(name))
            })
            .copied()
            .collect::<Vec<_>>();
        nodes.sort_unstable();
        nodes.dedup();
        nodes
            .into_iter()
            .filter_map(|node| self.selectors[node])
            .collect()
    }

    /// What `action` plays for `profile`: the output of each of its profile tests, in order.
    /// `None` when the rig has no such action, it tests no profile, or the dictionary does not
    /// name the profile.
    pub(crate) fn outputs(
        &self,
        action: AnimationAction,
        profile: u32,
    ) -> AuthoringResult<Option<Vec<i16>>> {
        let Some(ancestors) = self.ancestors(profile) else {
            return Ok(None);
        };
        let mut outputs = Vec::new();
        for selector in self.selectors(action) {
            let records = self.records(selector)?;
            for (_, operation) in self.operations(selector)? {
                if operation & 0xFF != PROFILE {
                    continue;
                }
                let root = (operation >> 16) as usize;
                let fallback = records
                    .get(root)
                    .ok_or_else(|| invalid("An animation selector root is out of range"))?
                    .output;
                outputs.push(
                    Self::chosen(&records, root, profile, &ancestors, &mut BTreeSet::new())?
                        .unwrap_or(fallback),
                );
            }
        }
        Ok((!outputs.is_empty()).then_some(outputs))
    }

    /// Makes `action` play what `profile` plays, whatever profile the weapon's row names.
    /// Returns whether anything changed.
    pub(crate) fn take(&mut self, action: AnimationAction, profile: u32) -> AuthoringResult<bool> {
        let ancestors = self.ancestors(profile).ok_or_else(|| {
            invalid(format!(
                "This rig's animations do not name profile 0x{profile:08X}"
            ))
        })?;
        // A payload that opens with its own length keeps it current when it grows.
        let sized = read_u64(&self.states, 0)? == self.states.len() as u64;
        let mut changed = false;
        for selector in self.selectors(action) {
            let records = self.records(selector)?;
            let operations = self.operations(selector)?;
            if operations
                .iter()
                .any(|(_, operation)| operation & 0xFF00 != 0)
            {
                return Err(invalid("An animation selector operation has unknown bits"));
            }
            let (count, _, first, _) = array_at(&self.states, selector + 32)?;
            let mut rows = self.states[first..first + count * RECORD_SIZE].to_vec();
            let mut edits = Vec::new();
            for (at, operation) in operations {
                if operation & 0xFF != PROFILE {
                    continue;
                }
                let root = (operation >> 16) as usize;
                let fallback = records
                    .get(root)
                    .ok_or_else(|| invalid("An animation selector root is out of range"))?
                    .output;
                let output =
                    Self::chosen(&records, root, profile, &ancestors, &mut BTreeSet::new())?
                        .unwrap_or(fallback);
                let index = u16::try_from(rows.len() / RECORD_SIZE)
                    .ok()
                    .filter(|index| *index <= i16::MAX as u16)
                    .ok_or_else(|| invalid("An animation selector has too many records"))?;
                // A record with no parameter bits and no children always answers its output.
                let mut record = [0u8; RECORD_SIZE];
                record[RECORD_BITS..RECORD_BITS + 4].copy_from_slice(&EMPTY_NAME.to_le_bytes());
                record[RECORD_BITS + 4..RECORD_BITS + 8].fill(0xFF);
                record[RECORD_BITS + 8..RECORD_BITS + 10].copy_from_slice(&output.to_le_bytes());
                rows.extend_from_slice(&record);
                edits.push((at, u32::from(index) << 16 | PROFILE));
            }
            if edits.is_empty() {
                continue;
            }
            for (at, operation) in edits {
                self.states[at..at + 4].copy_from_slice(&operation.to_le_bytes());
            }
            let count = rows.len() / RECORD_SIZE;
            append_native_array(&mut self.states, selector + 32, RECORD_CLASS, count, &rows)?;
            changed = true;
        }
        if changed && sized {
            let length = u64::try_from(self.states.len())
                .map_err(|_| invalid("The animation state table is too large"))?;
            self.states[..8].copy_from_slice(&length.to_le_bytes());
        }
        Ok(changed)
    }
}

/// Element offsets of the native array at `descriptor`, which must hold `class`. An empty array
/// has none.
fn rows(
    payload: &[u8],
    descriptor: usize,
    stride: usize,
    class: u32,
) -> AuthoringResult<Vec<usize>> {
    if read_u64(payload, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, _, first, found) = array_at(payload, descriptor)?;
    if found != class {
        return Err(invalid(format!(
            "Animation data at 0x{descriptor:X} holds class 0x{found:08X}, expected 0x{class:08X}"
        )));
    }
    if first + count * stride > payload.len() {
        return Err(invalid("Animation data is truncated"));
    }
    Ok((0..count).map(|row| first + row * stride).collect())
}
