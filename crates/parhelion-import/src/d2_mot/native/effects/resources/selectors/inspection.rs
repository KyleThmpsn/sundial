//! Discover dependencies through the same typed tree that is later emitted.
use super::*;

#[derive(Default)]
pub struct Inspection {
    pub selectors: BTreeSet<u32>,
    pub dictionaries: BTreeSet<u32>,
    pub categories: BTreeSet<u32>,
    pub union_categories: Vec<BTreeSet<u32>>,
    pub pinned_categories: BTreeSet<u32>,
    pub category_groups: BTreeSet<u32>,
    pub named_inputs: BTreeSet<u32>,
    /// Numeric context lookups leave their initialized zero vector intact when
    /// no host or publication exists in either runtime.
    pub optional_inputs: BTreeSet<u32>,
}

fn mask(mask: &Mask, names: &[u32], out: &mut Inspection) -> Result<()> {
    for words in mask.words.chunks_exact(14) {
        let mut bank = BTreeSet::new();
        for (word, &bits) in words.iter().enumerate() {
            for bit in 0..32 {
                if bits & (1 << bit) != 0 {
                    bank.insert(
                        *names
                            .get(word * 32 + bit)
                            .context("predicate exceeds its source dictionary")?,
                    );
                }
            }
        }
        out.categories.extend(&bank);
        if mask.words.len() == 28 {
            out.union_categories.push(bank);
        } else {
            // Keep every four-operation operand distinct. In particular, an
            // all-members test cannot use the union projection.
            out.pinned_categories.extend(bank);
        }
    }
    Ok(())
}

pub fn inspect_mask(source: &Payload, field: usize, names: &[u32]) -> Result<BTreeSet<u32>> {
    Ok(inspect_mask_usage(source, field, names)?.categories)
}

pub fn inspect_mask_usage(source: &Payload, field: usize, names: &[u32]) -> Result<Inspection> {
    let mut output = Inspection::default();
    if let Some(value) = read::Read::new(source, None).masks(field)? {
        mask(&value, names, &mut output)?;
    }
    Ok(output)
}

fn category_rows(
    rows: &[Vec<Row>; 4],
    names: &[u32],
    groups: &GroupBindings,
    out: &mut Inspection,
) -> Result<()> {
    for (operation, rows) in rows.iter().enumerate() {
        let mut bank = BTreeSet::new();
        for row in rows {
            out.dictionaries.insert(row.dictionary);
            if names.contains(&row.name) {
                bank.insert(row.name);
            } else {
                out.category_groups.insert(row.name);
                bank.extend(
                    groups
                        .source
                        .get(&row.name)
                        .context("category group is absent from its source dictionary")?,
                );
            }
        }
        out.categories.extend(&bank);
        if operation == 0 || operation == 2 {
            out.union_categories.push(bank);
        } else {
            out.pinned_categories.extend(bank);
        }
    }
    Ok(())
}

fn visit(node: &Node, names: &[u32], groups: &GroupBindings, out: &mut Inspection) -> Result<()> {
    match node {
        Node::External { source, .. } => {
            out.selectors.insert(*source);
        }
        Node::Selector { rows, .. } => {
            for (_, node) in rows {
                visit(node, names, groups, out)?;
            }
        }
        Node::Numeric { programs, .. } => {
            for program in programs {
                for &[_, name] in &program.bindings {
                    out.optional_inputs.insert(name);
                }
            }
        }
        Node::Category {
            rows,
            dictionary,
            masks,
            ..
        } => {
            out.dictionaries.insert(*dictionary);
            category_rows(rows, names, groups, out)?;
            if let Some(value) = masks {
                mask(value, names, out)?;
            }
        }
        Node::Raw {
            class: 0x80804D75,
            data,
        } => {
            out.named_inputs
                .insert(u32::from_le_bytes(data.as_slice().try_into()?));
        }
        Node::Raw { .. } | Node::Factions(_) => {}
    }
    Ok(())
}

/// Resource discovery precedes category allocation. It still parses the full
/// selector with the emitter's reader and checks external entry classes.
pub fn inspect_resources(
    source: &Payload,
    field: usize,
    resolver: &ResourceResolver,
) -> Result<BTreeSet<u32>> {
    fn collect(node: &Node, out: &mut BTreeSet<u32>) {
        match node {
            Node::External { source, .. } => {
                out.insert(*source);
            }
            Node::Selector { rows, .. } => {
                for (_, node) in rows {
                    collect(node, out);
                }
            }
            Node::Category {
                rows, dictionary, ..
            } => {
                out.insert(*dictionary);
                out.extend(rows.iter().flatten().map(|row| row.dictionary));
            }
            Node::Raw { .. } | Node::Factions(_) | Node::Numeric { .. } => {}
        }
    }
    let root = read::Read::new(source, Some(resolver)).selector(field, 0)?;
    let mut output = BTreeSet::new();
    collect(&root, &mut output);
    Ok(output)
}

pub fn inspect_selector(
    source: &Payload,
    field: usize,
    names: &[u32],
    groups: &GroupBindings,
    resolver: &ResourceResolver,
) -> Result<Inspection> {
    let node = read::Read::new(source, Some(resolver)).selector(field, 0)?;
    let mut output = Inspection::default();
    visit(&node, names, groups, &mut output)?;
    Ok(output)
}

pub fn inspect_labels(
    source: &Payload,
    fields: (usize, usize),
    names: &[u32],
    groups: &GroupBindings,
) -> Result<Inspection> {
    let node = read::Read::new(source, None).labels(fields.0, fields.1)?;
    let mut output = Inspection::default();
    visit(&node, names, groups, &mut output)?;
    Ok(output)
}
