//! Specialize a private state selector for its fixed attachment profile.
use super::*;

fn short(bytes: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn child(record: &Record, at: usize) -> Result<Option<usize>> {
    let index = short(&record.tail, at);
    ensure!(index >= -1, "selector has an invalid child index");
    Ok((index >= 0).then_some(index as usize))
}

fn chosen(
    records: &[Record],
    index: usize,
    name: u32,
    ancestors: &BTreeSet<u32>,
    visited: &mut BTreeSet<usize>,
) -> Result<Option<i16>> {
    ensure!(visited.insert(index), "profile selector contains a cycle");
    let record = records
        .get(index)
        .context("profile selector child is out of range")?;
    let hash = u32::from_le_bytes(record.tail[..4].try_into()?);
    if hash == name {
        return Ok(Some(short(&record.tail, 8)));
    }
    let mut result = None;
    if record.sentinel && record.group == Some(0) && record.names.is_subset(ancestors) {
        if let Some(index) = child(record, 4)? {
            result = chosen(records, index, name, ancestors, visited)?;
        }
        if result.is_none() {
            return Ok(Some(short(&record.tail, 8)));
        }
    }
    if let Some(index) = child(record, 6)? {
        result = chosen(records, index, name, ancestors, visited)?.or(result);
    }
    Ok(result)
}

fn constant(output: i16) -> Record {
    let mut tail = [0; 12];
    tail[..4].copy_from_slice(&0x811C9DC5u32.to_le_bytes());
    tail[4..8].fill(0xFF);
    tail[8..10].copy_from_slice(&output.to_le_bytes());
    Record {
        group: None,
        names: BTreeSet::new(),
        sentinel: false,
        tail,
    }
}

fn acyclic(
    edges: &BTreeMap<usize, BTreeSet<usize>>,
    index: usize,
    active: &mut BTreeSet<usize>,
    done: &mut BTreeSet<usize>,
) -> Result<()> {
    if done.contains(&index) {
        return Ok(());
    }
    ensure!(active.insert(index), "selector operations contain a cycle");
    for target in edges.get(&index).into_iter().flatten() {
        acyclic(edges, *target, active, done)?;
    }
    active.remove(&index);
    done.insert(index);
    Ok(())
}

/// Native F67190/F67330 establish the hierarchy, branch and result contracts.
/// Every unknown dynamic parameter keeps all of its possible branches.
pub(super) fn specialize(
    node: &mut Node,
    dictionary: &Dictionary,
    profile: u32,
) -> Result<Vec<usize>> {
    let Some(selector) = node.selector.as_mut() else {
        return Ok((0..node.choices.len()).collect());
    };
    ensure!(
        dictionary.header.get(6..8) == Some(&[0, 0]),
        "profile builtin maps to a different group"
    );
    let group = &dictionary.groups[0];
    let matches = group
        .iter()
        .enumerate()
        .filter(|(_, row)| row.0 == profile)
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "authored profile is absent or ambiguous"
    );
    let mut ancestors = BTreeSet::new();
    let mut current = matches[0].0;
    loop {
        ancestors.insert(group[current].0);
        if group[current].1 == u16::MAX {
            break;
        }
        current = usize::from(group[current].1);
    }
    for operation in &mut selector.operations {
        ensure!(
            *operation & 0xFF00 == 0,
            "selector operation padding differs"
        );
        if *operation & 255 != 3 {
            continue;
        }
        let root = (*operation >> 16) as usize;
        let record = selector
            .records
            .get(root)
            .context("profile selector root is out of range")?;
        let output = chosen(
            &selector.records,
            root,
            profile,
            &ancestors,
            &mut BTreeSet::new(),
        )?
        .unwrap_or(short(&record.tail, 8));
        let index = u16::try_from(selector.records.len())?;
        ensure!(
            index <= i16::MAX as u16,
            "specialized selector exceeds native index range"
        );
        selector.records.push(constant(output));
        *operation = u32::from(index) << 16 | 3;
    }
    let mut choices = BTreeSet::new();
    let mut records = BTreeSet::new();
    let mut pending = vec![0usize];
    let mut operations = BTreeSet::new();
    let mut edges = BTreeMap::<usize, BTreeSet<usize>>::new();
    while let Some(index) = pending.pop() {
        if !operations.insert(index) {
            continue;
        }
        let operation_index = index;
        let operation = *selector
            .operations
            .get(index)
            .context("selector result operation is out of range")?;
        let mut queue = vec![(operation >> 16) as usize];
        let mut tree = BTreeSet::new();
        while let Some(index) = queue.pop() {
            ensure!(tree.insert(index), "selector tree cycles or aliases a node");
            let record = selector
                .records
                .get(index)
                .context("selector child is out of range")?;
            records.insert(index);
            for at in [4, 6] {
                if let Some(index) = child(record, at)? {
                    queue.push(index);
                }
            }
            let output = short(&record.tail, 8);
            if output < 0 {
                let choice = (!output) as usize;
                ensure!(
                    choice < node.choices.len(),
                    "selector result choice is out of range"
                );
                choices.insert(choice);
            } else {
                ensure!(output > 0, "selector result operation is zero");
                pending.push(output as usize - 1);
                edges
                    .entry(operation_index)
                    .or_default()
                    .insert(output as usize - 1);
            }
        }
    }
    acyclic(&edges, 0, &mut BTreeSet::new(), &mut BTreeSet::new())?;
    ensure!(
        !choices.is_empty(),
        "authored profile cannot reach any animation choice"
    );
    for (i, record) in selector.records.iter_mut().enumerate() {
        if !records.contains(&i) {
            *record = constant(-1);
        }
    }
    for (i, choice) in node.choices.iter_mut().enumerate() {
        if !choices.contains(&i) {
            choice.clear();
        }
    }
    Ok(choices.into_iter().collect())
}
