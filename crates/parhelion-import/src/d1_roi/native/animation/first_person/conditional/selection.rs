//! Pair complete source and native raw-name domains without flattening dynamic routes.
use super::*;

type Domains = BTreeMap<u8, BTreeMap<u32, Vec<u8>>>;

pub(super) struct Plan {
    pub contexts: usize,
    /// Each emitted choice ordinal with its source action and the native action supplying its
    /// clip templates and supporting pose dispatch.
    pub choices: Vec<(usize, Action, Action)>,
    /// The translated source selector, when one native choice serves distinct source choices.
    pub selector: Option<state::Selector>,
    /// Whether the source and native dictionaries declare their selected profiles.
    pub declared: [bool; 2],
}

fn parameters(p: &Payload, node: usize, source: bool) -> Result<BTreeSet<u8>> {
    if p.u64(node)? == 1 {
        return Ok(BTreeSet::new());
    }
    ensure!(p.u64(node)? == 2, "Unsupported action selector kind");
    p.array(
        p.pointer(node + 8)? + 16,
        1,
        Some(if source { 0x8080440D } else { 0x80808DA4 }),
    )?
    .into_iter()
    .map(|r| p.u8(r))
    .collect()
}

fn native_parameter(source: u8) -> Result<u8> {
    match source {
        0..=4 => Ok(source),
        5 => Ok(10),
        6 => Ok(11),
        7 => Ok(12),
        9 => Ok(14),
        11 => Ok(20),
        12 => Ok(21),
        13 => Ok(22),
        _ => anyhow::bail!("Untranslated first-person selector input {source}"),
    }
}

fn source_parameter(native: u8) -> Result<u8> {
    match native {
        0..=4 => Ok(native),
        10 => Ok(5),
        11 => Ok(6),
        12 => Ok(7),
        14 => Ok(9),
        20 => Ok(11),
        21 => Ok(12),
        22 => Ok(13),
        _ => anyhow::bail!("Untranslated native first-person selector input {native}"),
    }
}

// A missing dictionary group yields a zero mask while preserving the raw name.
// Every record literal and one unmatched name form the complete finite domain
// for exact comparisons. Record children belong to the operation's input.
fn literals(p: &Payload, node: usize, source: bool, parameter: u8) -> Result<BTreeSet<u32>> {
    if p.u64(node)? == 1 {
        return Ok(BTreeSet::new());
    }
    let at = p.pointer(node + 8)?;
    let width = if source { 8 } else { 16 };
    let rows = p.array(
        at + 32,
        width + 12,
        Some(if source { 0x80802B27 } else { 0x8080347D }),
    )?;
    let mut pending = Vec::new();
    for row in p.array(
        at + 48,
        4,
        Some(if source { 0x808022C3 } else { 0x8080347C }),
    )? {
        let op = p.u32(row)?;
        ensure!(op & 0xFF00 == 0, "Unknown selector operation flags");
        if op as u8 == parameter {
            pending.push((op >> 16) as usize);
        }
    }
    let mut visited = BTreeSet::new();
    let mut result = BTreeSet::new();
    while let Some(index) = pending.pop() {
        if !visited.insert(index) {
            continue;
        }
        let row = *rows.get(index).context("Raw action selector record")?;
        result.insert(p.u32(row + width)?);
        for field in [width + 4, width + 6] {
            let child = p.i16(row + field)?;
            ensure!(child >= -1, "Invalid selector record child");
            if child >= 0 {
                pending.push(child as usize);
            }
        }
    }
    Ok(result)
}

fn mask(
    domains: &BTreeMap<u8, BTreeMap<u32, Vec<u8>>>,
    parameter: u8,
    name: u32,
    width: usize,
) -> Vec<u8> {
    domains
        .get(&parameter)
        .and_then(|d| d.get(&name))
        .cloned()
        .unwrap_or_else(|| {
            let mut value = vec![0; width];
            value.extend(name.to_le_bytes());
            value
        })
}

pub(super) fn plan(
    from: &Tables,
    to: &Tables,
    source_node: usize,
    native_node: usize,
    source_profile: u32,
    native_profile: u32,
) -> Result<Plan> {
    let sd = character::selection::domains(&from.dictionary, true)?;
    let nd = character::selection::domains(&to.dictionary, false)?;
    // An undeclared profile still reaches the selector as its raw name with a zero mask.
    let declared = [
        sd.get(&3).is_some_and(|d| d.contains_key(&source_profile)),
        nd.get(&3).is_some_and(|d| d.contains_key(&native_profile)),
    ];
    let sp = parameters(&from.states, source_node, true)?;
    let np = parameters(&to.states, native_node, false)?;
    let mut parameters = sp.clone();
    for &parameter in &np {
        parameters.insert(source_parameter(parameter)?);
    }
    parameters.remove(&3);
    let mut contexts = vec![BTreeMap::new()];
    for parameter in parameters {
        let native = native_parameter(parameter)?;
        let mut names = literals(&from.states, source_node, true, parameter)?;
        names.extend(literals(&to.states, native_node, false, native)?);
        for domain in [sd.get(&parameter), nd.get(&native)].into_iter().flatten() {
            ensure!(!domain.is_empty(), "Empty declared action input domain");
            names.extend(domain.keys().copied());
        }
        let unmatched = (0..=u32::MAX)
            .find(|name| !names.contains(name))
            .context("No unmatched action input")?;
        names.insert(unmatched);
        ensure!(
            contexts.len().saturating_mul(names.len()) <= 4096,
            "Paired action context domain exceeds its bound"
        );
        contexts = contexts
            .into_iter()
            .flat_map(|values| {
                names.iter().map(move |&name| {
                    let mut values = values.clone();
                    values.insert(parameter, name);
                    values
                })
            })
            .collect();
    }
    let mut paired = BTreeMap::<usize, (Action, Action)>::new();
    let mut by_source = BTreeMap::<usize, (Action, Vec<Action>)>::new();
    let mut selections = Vec::new();
    let mut merged = false;
    for context in &contexts {
        let mut sf = BTreeMap::from([(3, mask(&sd, 3, source_profile, 8))]);
        let mut nf = BTreeMap::from([(3, mask(&nd, 3, native_profile, 16))]);
        for (&parameter, &name) in context {
            sf.insert(parameter, mask(&sd, parameter, name, 8));
            let native = native_parameter(parameter)?;
            nf.insert(native, mask(&nd, native, name, 16));
        }
        let s = action(&from.states, &from.bank, source_node, true, &sf)?;
        let n = action(&to.states, &to.bank, native_node, false, &nf)?;
        selections.push((s.index, nf));
        if let Some((old, _)) = paired.get(&n.index) {
            merged |= old.choices != s.choices;
        } else {
            paired.insert(n.index, (s.clone(), n.clone()));
        }
        let (_, natives) = by_source.entry(s.index).or_insert_with(|| (s, Vec::new()));
        if !natives.iter().any(|other| other.index == n.index) {
            natives.push(n);
        }
    }
    ensure!(!paired.is_empty(), "No selected action contexts");
    if !merged {
        return Ok(Plan {
            contexts: contexts.len(),
            choices: paired.into_iter().map(|(i, (s, n))| (i, s, n)).collect(),
            selector: None,
            declared,
        });
    }
    // One native choice serves distinct source choices, so the source selector itself decides.
    // It must reproduce the source selection in every paired context.
    let selector = translate(
        &from.states,
        source_node,
        &sd,
        &nd,
        (source_profile, native_profile),
    )?;
    let evaluated = selector.payload()?;
    for (index, fixed) in &selections {
        ensure!(
            character::selection::select_at(&evaluated, 32, false, fixed)? == *index,
            "Translated source selector differs from its source"
        );
    }
    let choices = by_source
        .into_iter()
        .map(|(index, (s, mut natives))| {
            // A single native clip or matching cutoffs can template every source alternative.
            // Otherwise the first reached native choice is kept, and preparation rejects it.
            let at = natives
                .iter()
                .position(|n| {
                    n.choices.len() == 1
                        || s.choices
                            .iter()
                            .map(|c| c.weight)
                            .eq(n.choices.iter().map(|c| c.weight))
                })
                .unwrap_or(0);
            (index, s, natives.swap_remove(at))
        })
        .collect();
    Ok(Plan {
        contexts: contexts.len(),
        choices,
        selector: Some(selector),
        declared,
    })
}

/// The source selector of conditional action `node` in native form. Inputs keep their identity,
/// record masks are looked up by name in each era's dictionary, and the selected source profile
/// is renamed to the native one. Bit positions never cross eras.
fn translate(
    p: &Payload,
    node: usize,
    sd: &Domains,
    nd: &Domains,
    (source_profile, native_profile): (u32, u32),
) -> Result<state::Selector> {
    ensure!(
        p.u64(node)? == 2,
        "Unconditional source action has no selector"
    );
    let at = p.pointer(node + 8)?;
    let records = p.array(at + 32, 20, Some(0x80802B27))?;
    // Every record belongs to the input of the operation whose tree reaches it.
    let mut inputs = vec![None; records.len()];
    let mut operations = Vec::new();
    for row in p.array(at + 48, 4, Some(0x808022C3))? {
        let op = p.u32(row)?;
        ensure!(op & 0xFF00 == 0, "Unknown selector operation flags");
        let input = op as u8;
        operations.extend((op & !0xFF | u32::from(native_parameter(input)?)).to_le_bytes());
        let mut pending = vec![(op >> 16) as usize];
        while let Some(index) = pending.pop() {
            let row = *records.get(index).context("Raw action selector record")?;
            match inputs[index] {
                Some(other) => {
                    ensure!(other == input, "Selector record is shared between inputs");
                    continue;
                }
                None => inputs[index] = Some(input),
            }
            for field in [12, 14] {
                let child = p.i16(row + field)?;
                ensure!(child >= -1, "Invalid selector record child");
                if child >= 0 {
                    pending.push(child as usize);
                }
            }
        }
    }
    let mut rows = Vec::new();
    for (&row, input) in records.iter().zip(inputs) {
        let name = p.u32(row + 8)?;
        let mut emitted = name;
        let mut mask = [0; 16];
        // Unreached records keep only their raw name.
        if let Some(input) = input {
            if input == 3 && name == source_profile {
                emitted = native_profile;
            }
            let source = &p.0[row..row + 8];
            if source.iter().any(|&b| b != 0) {
                ensure!(
                    sd.get(&input)
                        .and_then(|d| d.get(&name))
                        .is_some_and(|entry| entry[..8] == *source),
                    "Selector record mask differs from its dictionary"
                );
                if let Some(entry) = nd
                    .get(&native_parameter(input)?)
                    .and_then(|d| d.get(&emitted))
                {
                    mask.copy_from_slice(&entry[..16]);
                }
            }
        }
        rows.extend(mask);
        rows.extend(emitted.to_le_bytes());
        rows.extend(&p.0[row + 12..row + 20]);
    }
    Ok(state::Selector {
        choices: p.array(at, 16, Some(0x80802A9D))?.len(),
        parameters: p
            .array(at + 16, 1, Some(0x8080440D))?
            .into_iter()
            .map(|r| native_parameter(p.u8(r)?))
            .collect::<Result<_>>()?,
        records: rows,
        operations,
    })
}
