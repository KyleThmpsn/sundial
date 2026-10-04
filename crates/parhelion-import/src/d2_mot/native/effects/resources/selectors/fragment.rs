//! Embedded predicates retain actual source ownership and destination placement.
use super::*;

#[derive(Clone, Copy)]
pub struct FragmentContract {
    pub root_class: u32,
    pub field: usize,
    pub extent: usize,
}

pub struct Fragment {
    pub root: usize,
    pub references: Vec<Reference>,
    pub reference_classes: BTreeMap<usize, u32>,
    pub gates: Vec<Gate>,
    pub source_spans: Vec<(usize, usize)>,
    pub source_references: Vec<usize>,
    pub named_conditions: Vec<u32>,
    pub group_aliases: Vec<GroupAliasUse>,
}

/// Group hashes identify sets of category names, never category indices.
#[derive(Default)]
pub struct GroupBindings {
    pub source: BTreeMap<u32, BTreeSet<u32>>,
    pub native: BTreeMap<u32, BTreeSet<u32>>,
    pub aliases: BTreeMap<u32, u32>,
}
impl GroupBindings {
    pub fn from_namespace(namespace: &crate::d2_mot::native::categories::Namespace) -> Self {
        Self {
            source: namespace.source_groups.clone(),
            native: namespace.groups.clone(),
            aliases: namespace
                .group_aliases
                .iter()
                .map(|(source, alias)| (*source, alias.native))
                .collect(),
        }
    }
    pub(super) fn validate(
        &self,
        name: u32,
        offset: usize,
        resources: &Resources,
        gates: &mut Vec<Gate>,
    ) -> Result<u32> {
        let source = self
            .source
            .get(&name)
            .context("selector row absent from source names and groups")?;
        let target = self.aliases.get(&name).copied().unwrap_or(name);
        ensure!(
            target == name || !resources.names.contains(&target),
            "selector alias shadows a category name"
        );
        let native = self
            .native
            .get(&target)
            .context("native selector group missing")?;
        let mut mapped = BTreeSet::new();
        for member in source {
            let index = resources
                .names
                .iter()
                .position(|n| n == member)
                .context("selector group member absent from source dictionary")?;
            if resources.categories[index].is_some() {
                mapped.insert(*member);
            } else {
                gates.push(Gate {
                    offset,
                    index,
                    name: *member,
                });
            }
        }
        ensure!(
            &mapped == native,
            "native selector group {name:08X} membership differs"
        );
        Ok(target)
    }
}

pub fn emit_with_groups(
    source: &Payload,
    resources: &Resources,
    groups: &GroupBindings,
) -> Result<Selector> {
    ensure!(
        source.u64(0)? == source.0.len() as u64 && source.u64(32)? == 0,
        "selector envelope differs"
    );
    ensure!(
        resources.names.len() == resources.categories.len() && resources.names.len() <= 448,
        "selector category correspondence differs"
    );
    let mut read = read::Read::new(source, None);
    read.claim(0, 40)?;
    let root = read.selector(8, 0)?;
    read.finish()?;
    write::Write::new(resources, None)
        .with_groups(groups)
        .finish(root)
}

/// Append a category body without inventing a selector owner envelope.
/// Failed validation leaves the supplied destination unchanged.
pub fn append_category(
    source: &Payload,
    destination: &mut Payload,
    contract: FragmentContract,
    resources: &Resources,
    groups: &GroupBindings,
) -> Result<Fragment> {
    ensure!(
        contract.root_class == 0x808042CB && contract.extent == 104,
        "embedded category class or extent differs"
    );
    ensure!(
        resources.names.len() == resources.categories.len() && resources.names.len() <= 448,
        "selector category correspondence differs"
    );
    let prefix = contract
        .field
        .checked_sub(4)
        .context("embedded category prefix")?;
    ensure!(
        source.u32(prefix)? == contract.root_class,
        "embedded category root class differs"
    );
    let mut read = read::Read::new(source, None);
    let node = read.node(contract.field, 0)?;
    ensure!(
        source.u64(contract.field + 80)? == 0,
        "embedded category padding differs"
    );
    let (source_spans, source_references) = read.ownership();
    let (native, root) = write::Write::new(resources, None)
        .with_groups(groups)
        .fragment(node, destination)?;
    *destination = native.payload;
    Ok(Fragment {
        root,
        reference_classes: native
            .references
            .iter()
            .map(|r| (r.offset, crate::d2_mot::native::categories::NATIVE_CLASS))
            .collect(),
        references: native.references,
        gates: native.gates,
        named_conditions: native.named_conditions,
        group_aliases: native.group_aliases,
        source_spans,
        source_references,
    })
}

/// Emit the inline value selector inside a checked collision modifier body.
/// The containing converter owns the 40 byte source body and modifier array.
/// Returned ownership covers selector arrays, predicates, strings and wide references.
pub fn append_value_selector(
    source: &Payload,
    destination: &mut Payload,
    contract: FragmentContract,
    native_field: usize,
    resources: &Resources,
    groups: &GroupBindings,
    resolver: &ResourceResolver,
) -> Result<Fragment> {
    ensure!(
        contract.root_class == 0x80803F78 && contract.extent == 40,
        "collision value selector contract differs"
    );
    ensure!(
        resources.names.len() == resources.categories.len() && resources.names.len() <= 448,
        "selector category correspondence differs"
    );
    let source_prefix = contract
        .field
        .checked_sub(4)
        .context("collision selector source prefix")?;
    let native_prefix = native_field
        .checked_sub(4)
        .context("collision selector native prefix")?;
    ensure!(
        source.u32(source_prefix)? == contract.root_class
            && destination.u32(native_prefix)? == 0x80804B0A,
        "collision value selector root class differs"
    );
    source
        .0
        .get(
            contract.field
                ..contract
                    .field
                    .checked_add(40)
                    .context("collision source extent overflow")?,
        )
        .context("collision source body outside payload")?;
    destination
        .0
        .get(
            native_field
                ..native_field
                    .checked_add(40)
                    .context("collision native extent overflow")?,
        )
        .context("collision native body outside payload")?;
    let mut read = read::Read::new(source, Some(resolver));
    let node = read.selector(contract.field, 0)?;
    let (source_spans, source_references) = read.ownership();
    let native = write::Write::new(resources, Some(resolver))
        .with_groups(groups)
        .inline(node, destination, native_field)?;
    let reference_classes = native
        .references
        .iter()
        .map(|r| {
            (
                r.offset,
                native
                    .external_classes
                    .get(&r.offset)
                    .copied()
                    .unwrap_or(crate::d2_mot::native::categories::NATIVE_CLASS),
            )
        })
        .collect();
    *destination = native.payload;
    Ok(Fragment {
        root: native_field,
        references: native.references,
        reference_classes,
        gates: native.gates,
        source_spans,
        source_references,
        named_conditions: native.named_conditions,
        group_aliases: native.group_aliases,
    })
}
