//! Append a private parameter descendant and conditional actions without renumbering stock data.
use super::*;

const EMPTY: u32 = 0x811C9DC5;

struct Group {
    at: usize,
    names: Vec<(u32, u16)>,
    masks: BTreeMap<u32, [u8; 24]>,
}

impl Group {
    fn read(dictionary: &Payload, builtin: u8) -> Result<Self> {
        ensure!(builtin < 24, "Animation builtin is outside its table");
        let groups = dictionary.array(8, 32, Some(0x80808EE5))?;
        let index = usize::from(dictionary.u16(24 + usize::from(builtin) * 2)?);
        let at = *groups
            .get(index)
            .context("Animation builtin has no parameter group")?;
        let names = dictionary
            .array(at, 8, Some(0x80808EE9))?
            .into_iter()
            .map(|row| Ok((dictionary.u32(row)?, dictionary.u16(row + 4)?)))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            !names.is_empty() && names.len() <= 127,
            "Animation parameter group exceeds mask capacity"
        );
        let mut unique = BTreeSet::new();
        for (index, &(name, parent)) in names.iter().enumerate() {
            ensure!(
                unique.insert(name) && (parent == u16::MAX || usize::from(parent) < index),
                "Animation parameter names or ancestry are invalid"
            );
        }
        let mut masks = BTreeMap::new();
        let mut previous = None;
        for row in dictionary.array(at + 16, 24, Some(0x80808EE8))? {
            let record = dictionary.bytes::<24>(row)?;
            let name = dictionary.u32(row + 16)?;
            let index = usize::from(dictionary.u16(row + 20)?);
            ensure!(
                names.get(index).is_some_and(|entry| entry.0 == name)
                    && previous.is_none_or(|before| before < name)
                    && masks.insert(name, record).is_none(),
                "Animation parameter lookup is invalid"
            );
            previous = Some(name);
            ensure!(
                record[..16] == Self::mask(&names, index),
                "Animation parameter mask disagrees with its ancestry"
            );
        }
        ensure!(
            masks.len() == names.len(),
            "Animation parameter lookup is incomplete"
        );
        Ok(Self { at, names, masks })
    }

    fn mask(names: &[(u32, u16)], mut index: usize) -> [u8; 16] {
        let mut mask = [0; 16];
        mask[15] = 128;
        loop {
            mask[index / 8] |= 1 << (index % 8);
            let parent = names[index].1;
            if parent == u16::MAX {
                return mask;
            }
            index = usize::from(parent);
        }
    }

    fn selected(&self, name: u32) -> Result<[u8; 24]> {
        self.masks
            .get(&name)
            .copied()
            .context("Animation profile is absent from its parameter group")
    }
}

/// Every existing bit ordinal and lookup record is retained. The new name inherits its parent.
pub fn extend(dictionary: &Payload, builtin: u8, parent: u32, private: u32) -> Result<Payload> {
    let mut group = Group::read(dictionary, builtin)?;
    ensure!(
        ![0, u32::MAX, EMPTY].contains(&private) && !group.masks.contains_key(&private),
        "Private animation parameter collides with an existing name"
    );
    ensure!(
        group.names.len() < 127,
        "Native animation parameter group has no free bits"
    );
    let parent = group
        .names
        .iter()
        .position(|&(name, _)| name == parent)
        .context("Native animation parameter parent is absent")?;
    let ordinal = group.names.len();
    group.names.push((private, u16::try_from(parent)?));
    let mut names = dictionary
        .array(group.at, 8, Some(0x80808EE9))?
        .into_iter()
        .flat_map(|row| dictionary.0[row..row + 8].iter().copied())
        .collect::<Vec<_>>();
    names.extend(private.to_le_bytes());
    names.extend(u16::try_from(parent)?.to_le_bytes());
    names.extend([0; 2]);
    let mut record = [0; 24];
    record[..16].copy_from_slice(&Group::mask(&group.names, ordinal));
    record[16..20].copy_from_slice(&private.to_le_bytes());
    record[20..22].copy_from_slice(&u16::try_from(ordinal)?.to_le_bytes());
    group.masks.insert(private, record);
    let mut output = dictionary.clone();
    append(&mut output.0, group.at, 0x80808EE9, &names, 8)?;
    append(
        &mut output.0,
        group.at + 16,
        0x80808EE8,
        &group.masks.values().flatten().copied().collect::<Vec<_>>(),
        24,
    )?;
    finish(&mut output)?;
    Group::read(&output, builtin)?;
    Ok(output)
}

fn short(record: &[u8; 28], at: usize) -> i16 {
    i16::from_le_bytes(record[at..at + 2].try_into().unwrap())
}

fn set_short(record: &mut [u8; 28], at: usize, value: i16) {
    record[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

struct Selector {
    parameters: Vec<u8>,
    records: Vec<[u8; 28]>,
    operations: Vec<u32>,
}

impl Selector {
    fn at(p: &Payload, at: usize) -> Result<Self> {
        let result = Self {
            parameters: p
                .array(at, 1, Some(0x80808DA4))?
                .into_iter()
                .map(|at| p.u8(at))
                .collect::<Result<_>>()?,
            records: p
                .array(at + 16, 28, Some(0x8080347D))?
                .into_iter()
                .map(|at| p.bytes(at))
                .collect::<Result<_>>()?,
            operations: p
                .array(at + 32, 4, Some(0x8080347C))?
                .into_iter()
                .map(|at| p.u32(at))
                .collect::<Result<_>>()?,
        };
        ensure!(
            !result.operations.is_empty()
                && result.parameters.iter().all(|&p| p < 24)
                && result.operations.iter().all(|&op| op & 0xFF00 == 0
                    && result.parameters.contains(&(op as u8))
                    && (op >> 16) < result.records.len() as u32),
            "Invalid native animation selector operations"
        );
        Ok(result)
    }

    fn read(p: &Payload, row: usize, builtin: u8) -> Result<Self> {
        let at = p.pointer(row + 8)?;
        if p.u64(row)? == 1 {
            let mut record = [0; 28];
            record[15] = 128;
            record[16..20].copy_from_slice(&EMPTY.to_le_bytes());
            record[20..26].fill(255);
            return Ok(Self {
                parameters: vec![builtin],
                records: vec![record],
                operations: vec![u32::from(builtin)],
            });
        }
        // choices checks the complete native selector envelope, including its tail.
        choices(p, row)?;
        Self::at(p, at + 16)
    }

    fn children(&self, index: usize) -> Result<[Option<usize>; 2]> {
        let record = self
            .records
            .get(index)
            .context("Animation selector child is outside its records")?;
        let mut output = [None; 2];
        for (slot, at) in [20, 22].into_iter().enumerate() {
            let child = short(record, at);
            ensure!(
                child >= -1 && (child < 0 || (child as usize) < self.records.len()),
                "Invalid animation selector child"
            );
            output[slot] = (child >= 0).then_some(child as usize);
        }
        Ok(output)
    }

    fn chosen(
        &self,
        index: usize,
        selected: &[u8; 24],
        active: &mut BTreeSet<usize>,
    ) -> Result<Option<i16>> {
        ensure!(
            active.insert(index),
            "Animation selector contains a record cycle"
        );
        let record = self
            .records
            .get(index)
            .context("Animation selector record is absent")?;
        let [child, alternate] = self.children(index)?;
        let output = if record[16..20] == selected[16..20] {
            Some(short(record, 24))
        } else {
            let matches = record[15] & selected[15] & 128 != 0
                && record[..16]
                    .iter()
                    .zip(selected)
                    .all(|(&required, &value)| required & !value == 0);
            let refined = if matches {
                child
                    .map(|child| self.chosen(child, selected, active))
                    .transpose()?
                    .flatten()
            } else {
                None
            };
            if matches && refined.is_none() {
                Some(short(record, 24))
            } else {
                alternate
                    .map(|child| self.chosen(child, selected, active))
                    .transpose()?
                    .flatten()
                    .or(refined)
            }
        };
        active.remove(&index);
        Ok(output)
    }

    fn outputs(
        &self,
        index: usize,
        active: &mut BTreeSet<usize>,
        values: &mut BTreeSet<i16>,
    ) -> Result<()> {
        ensure!(
            active.insert(index),
            "Animation selector contains a record cycle"
        );
        values.insert(short(
            self.records
                .get(index)
                .context("Animation selector record is absent")?,
            24,
        ));
        for child in self.children(index)?.into_iter().flatten() {
            self.outputs(child, active, values)?;
        }
        active.remove(&index);
        Ok(())
    }
}

/// Resolve the fixed parameter and retain every branch controlled by other runtime inputs.
pub fn descriptors(
    p: &Payload,
    dictionary: &Payload,
    action: u32,
    builtin: u8,
    name: u32,
) -> Result<BTreeSet<usize>> {
    let group = Group::read(dictionary, builtin)?;
    let selected = group.selected(name)?;
    let row = node(p, action)?;
    let weights = choices(p, row)?;
    let selector = Selector::read(p, row, builtin)?;
    let selected_choices = selected_choices(&selector, &BTreeMap::from([(builtin, selected)]))?;
    let mut result = BTreeSet::new();
    for choice in selected_choices {
        let choice = *weights
            .get(choice)
            .context("Animation choice is outside its table")?;
        for weight in p.array(choice, 8, Some(0x80803439))? {
            ensure!(p.f32(weight + 4)? >= 0., "Invalid character action weight");
            result.insert(p.u32(weight)? as usize);
        }
    }
    ensure!(
        !result.is_empty(),
        "Native parameter has no action descriptors"
    );
    Ok(result)
}

fn selected_choices(
    selector: &Selector,
    fixed: &BTreeMap<u8, [u8; 24]>,
) -> Result<BTreeSet<usize>> {
    let mut pending = vec![0usize];
    let mut operations = BTreeSet::new();
    let mut selected_choices = BTreeSet::new();
    let mut edges = BTreeMap::<usize, BTreeSet<usize>>::new();
    while let Some(index) = pending.pop() {
        if !operations.insert(index) {
            continue;
        }
        let op = *selector
            .operations
            .get(index)
            .context("Animation result operation is absent")?;
        let mut outputs = BTreeSet::new();
        if let Some(selected) = fixed.get(&(op as u8)) {
            let root = (op >> 16) as usize;
            outputs.insert(
                selector
                    .chosen(root, selected, &mut BTreeSet::new())?
                    .unwrap_or(short(&selector.records[root], 24)),
            );
        } else {
            selector.outputs((op >> 16) as usize, &mut BTreeSet::new(), &mut outputs)?;
        }
        for output in outputs {
            if output < 0 {
                let choice = (!output) as usize;
                selected_choices.insert(choice);
            } else {
                ensure!(output > 0, "Animation selector has a zero result");
                let next = output as usize - 1;
                edges.entry(index).or_default().insert(next);
                pending.push(next);
            }
        }
    }
    fn acyclic(
        index: usize,
        edges: &BTreeMap<usize, BTreeSet<usize>>,
        active: &mut BTreeSet<usize>,
        done: &mut BTreeSet<usize>,
    ) -> Result<()> {
        if done.contains(&index) {
            return Ok(());
        }
        ensure!(
            active.insert(index),
            "Animation selector contains an operation cycle"
        );
        for &next in edges.get(&index).into_iter().flatten() {
            acyclic(next, edges, active, done)?;
        }
        active.remove(&index);
        done.insert(index);
        Ok(())
    }
    acyclic(0, &edges, &mut BTreeSet::new(), &mut BTreeSet::new())?;
    ensure!(
        !selected_choices.is_empty(),
        "Native parameter has no motion choices"
    );
    Ok(selected_choices)
}

/// Standalone 3478 selectors return motion-table row ordinals directly.
pub fn table_choices(
    p: &Payload,
    dictionary: &Payload,
    builtin: u8,
    name: u32,
) -> Result<BTreeSet<usize>> {
    table_choices_for(p, dictionary, &BTreeMap::from([(builtin, name)]))
}

pub fn table_choices_for(
    p: &Payload,
    dictionary: &Payload,
    parameters: &BTreeMap<u8, u32>,
) -> Result<BTreeSet<usize>> {
    let fixed = parameters
        .iter()
        .map(|(&builtin, &name)| Ok((builtin, Group::read(dictionary, builtin)?.selected(name)?)))
        .collect::<Result<_>>()?;
    selected_choices(&Selector::at(p, 8)?, &fixed)
}

/// Retain the complete stock decision tree and remap only the private descendant.
pub fn table_branch(
    p: &Payload,
    dictionary: &Payload,
    builtin: u8,
    private: u32,
    mapping: &BTreeMap<usize, usize>,
    conditions: &BTreeMap<u8, u32>,
) -> Result<Payload> {
    ensure!(!mapping.is_empty(), "Empty private motion table mapping");
    ensure!(
        !conditions.contains_key(&builtin),
        "Private motion condition repeats its profile parameter"
    );
    let selector = Selector::at(p, 8)?;
    let gates = std::iter::once((builtin, private))
        .chain(
            conditions
                .iter()
                .map(|(&parameter, &name)| (parameter, name)),
        )
        .collect::<Vec<_>>();
    let old_operation = gates.len();
    let new_operation = old_operation + selector.operations.len();
    let mut records = Vec::new();
    let mut operations = Vec::new();
    for (index, &(parameter, name)) in gates.iter().enumerate() {
        let mask = Group::read(dictionary, parameter)?.selected(name)?;
        let mut root = [0; 28];
        root[15] = 128;
        root[16..20].copy_from_slice(&EMPTY.to_le_bytes());
        set_short(&mut root, 20, i16::try_from(records.len() + 1)?);
        set_short(&mut root, 22, -1);
        set_short(&mut root, 24, i16::try_from(old_operation + 1)?);
        let mut selected = [0; 28];
        selected[..20].copy_from_slice(&mask[..20]);
        set_short(&mut selected, 20, -1);
        set_short(&mut selected, 22, -1);
        let next = if index + 1 == gates.len() {
            new_operation
        } else {
            index + 1
        };
        set_short(&mut selected, 24, i16::try_from(next + 1)?);
        operations.push(u32::from(parameter) | (u32::try_from(records.len())? << 16));
        records.extend([root, selected]);
    }
    let prefix = records.len();
    for (record_base, operation_base, private_branch) in [
        (prefix, old_operation, false),
        (prefix + selector.records.len(), new_operation, true),
    ] {
        let (mut copied, ops) = relocated(&selector, record_base, operation_base, 0)?;
        if private_branch {
            for record in &mut copied {
                let result = short(record, 24);
                if result < 0
                    && let Some(&choice) = mapping.get(&((!result) as usize))
                {
                    set_short(record, 24, !i16::try_from(choice)?);
                }
            }
        }
        records.extend(copied);
        operations.extend(ops);
    }
    ensure!(
        records.len() <= i16::MAX as usize && operations.len() <= i16::MAX as usize,
        "Private motion selector exceeds native capacity"
    );
    let mut parameters = selector.parameters;
    for (parameter, _) in gates {
        if !parameters.contains(&parameter) {
            parameters.push(parameter);
        }
    }
    let mut out = p.clone();
    append(&mut out.0, 8, 0x80808DA4, &parameters, 1)?;
    append(
        &mut out.0,
        24,
        0x8080347D,
        &records.iter().flatten().copied().collect::<Vec<_>>(),
        28,
    )?;
    append(
        &mut out.0,
        40,
        0x8080347C,
        &operations
            .iter()
            .flat_map(|op| op.to_le_bytes())
            .collect::<Vec<_>>(),
        4,
    )?;
    finish(&mut out)?;
    Ok(out)
}

fn finish(p: &mut Payload) -> Result<()> {
    let size = u64::try_from(p.0.len())?;
    put(&mut p.0, 0, &size.to_le_bytes())
}

fn relocated(
    selector: &Selector,
    record_base: usize,
    operation_base: usize,
    choice_base: usize,
) -> Result<(Vec<[u8; 28]>, Vec<u32>)> {
    let mut records = selector.records.clone();
    for record in &mut records {
        for at in [20, 22] {
            let child = short(record, at);
            ensure!(
                child >= -1 && (child < 0 || (child as usize) < selector.records.len()),
                "Invalid animation selector child"
            );
            if child >= 0 {
                set_short(record, at, i16::try_from(child as usize + record_base)?);
            }
        }
        let output = short(record, 24);
        ensure!(output != 0, "Invalid animation selector result");
        let output = if output < 0 {
            !i16::try_from((!output) as usize + choice_base)?
        } else {
            ensure!(
                output as usize <= selector.operations.len(),
                "Animation selector result operation is absent"
            );
            i16::try_from(output as usize + operation_base)?
        };
        set_short(record, 24, output);
    }
    let operations = selector
        .operations
        .iter()
        .map(|&op| {
            Ok(
                (u32::from(i16::try_from((op >> 16) as usize + record_base)? as u16) << 16)
                    | (op & 255),
            )
        })
        .collect::<Result<_>>()?;
    Ok((records, operations))
}

/// The new parameter selects translated copies. Every old name keeps its original selector.
pub fn branch(
    p: &Payload,
    dictionary: &Payload,
    builtin: u8,
    private: u32,
    routes: &BTreeMap<u32, BTreeMap<usize, usize>>,
) -> Result<Payload> {
    let group = Group::read(dictionary, builtin)?;
    let private_mask = group.selected(private)?;
    let mut out = p.clone();
    let mut nodes = p
        .array(24, 16, Some(0x8080342F))?
        .into_iter()
        .map(|row| {
            Ok((
                p.u64(row)?,
                if p.u64(row + 8)? == 0 {
                    None
                } else {
                    Some(p.pointer(row + 8)?)
                },
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut replacements = BTreeMap::new();
    for (&name, map) in routes {
        ensure!(!map.is_empty(), "Empty private character action mapping");
        let row = node(p, name)?;
        let original_choices = choices(p, row)?;
        let selector = Selector::read(p, row, builtin)?;
        let choice_count = original_choices.len();
        ensure!(
            choice_count * 2 <= i16::MAX as usize
                && selector.operations.len() * 2 < i16::MAX as usize,
            "Private character action exceeds native selector capacity"
        );
        let target = (out.0.len() + 19) & !15;
        out.0.resize(target - 4, 0);
        out.0.extend(0x80803432u32.to_le_bytes());
        out.0.resize(target + 120, 0);
        append(
            &mut out.0,
            target,
            0x80803437,
            &vec![0; choice_count * 32],
            16,
        )?;
        let fields = out.array(target, 16, Some(0x80803437))?;
        for (index, &choice) in original_choices.iter().enumerate() {
            let mut old = Vec::new();
            let mut new = Vec::new();
            for at in p.array(choice, 8, Some(0x80803439))? {
                old.extend(p.bytes::<8>(at)?);
                let descriptor = p.u32(at)? as usize;
                new.extend(
                    u32::try_from(*map.get(&descriptor).unwrap_or(&descriptor))?.to_le_bytes(),
                );
                new.extend(p.bytes::<4>(at + 4)?);
            }
            append(&mut out.0, fields[index], 0x80803439, &old, 8)?;
            append(
                &mut out.0,
                fields[index + choice_count],
                0x80803439,
                &new,
                8,
            )?;
        }
        let mut parameters = selector.parameters.clone();
        if !parameters.contains(&builtin) {
            parameters.push(builtin);
        }
        append(&mut out.0, target + 16, 0x80808DA4, &parameters, 1)?;
        let mut root = [0; 28];
        root[15] = 128;
        root[16..20].copy_from_slice(&EMPTY.to_le_bytes());
        set_short(&mut root, 20, 1);
        set_short(&mut root, 22, -1);
        set_short(&mut root, 24, 2);
        let mut selected = [0; 28];
        selected[..20].copy_from_slice(&private_mask[..20]);
        set_short(&mut selected, 20, -1);
        set_short(&mut selected, 22, -1);
        set_short(
            &mut selected,
            24,
            i16::try_from(selector.operations.len() + 2)?,
        );
        let mut records = vec![root, selected];
        let mut operations = vec![u32::from(builtin)];
        for (record_base, operation_base, choice_base) in [
            (2, 1, 0),
            (
                2 + selector.records.len(),
                1 + selector.operations.len(),
                choice_count,
            ),
        ] {
            let (copied_records, copied_operations) =
                relocated(&selector, record_base, operation_base, choice_base)?;
            records.extend(copied_records);
            operations.extend(copied_operations);
        }
        ensure!(
            records.len() <= i16::MAX as usize,
            "Private character action exceeds native record capacity"
        );
        append(
            &mut out.0,
            target + 32,
            0x8080347D,
            &records.iter().flatten().copied().collect::<Vec<_>>(),
            28,
        )?;
        append(
            &mut out.0,
            target + 48,
            0x8080347C,
            &operations
                .iter()
                .flat_map(|op| op.to_le_bytes())
                .collect::<Vec<_>>(),
            4,
        )?;
        replacements.insert(name, nodes.len());
        nodes.push((2, Some(target)));
    }
    append(&mut out.0, 24, 0x8080342F, &vec![0; nodes.len() * 16], 16)?;
    for (row, (kind, target)) in out.array(24, 16, Some(0x8080342F))?.into_iter().zip(nodes) {
        put(&mut out.0, row, &kind.to_le_bytes())?;
        let relative = target.map_or(0, |target| target as i64 - row as i64 - 8);
        put(&mut out.0, row + 8, &relative.to_le_bytes())?;
    }
    for row in p.array(8, 8, Some(0x8080342E))? {
        if let Some(&index) = replacements.get(&p.u32(row)?) {
            put(&mut out.0, row + 4, &u32::try_from(index)?.to_le_bytes())?;
        }
    }
    finish(&mut out)?;
    Ok(out)
}
