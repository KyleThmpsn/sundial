//! Embedded predicates retain actual source ownership and destination placement.
use super::*;

/// Retain an action's inline source label lists and compiled predicate using
/// the same private registry as its selectors. Header flags between them stay
/// owned by the action converter.
pub fn append_labels(
    source: &Payload,
    from: (usize, usize),
    destination: &mut Payload,
    to: (usize, usize),
    resources: &Resources,
    groups: &GroupBindings,
) -> Result<()> {
    let node = read::Read::new(source, None).labels(from.0, from.1)?;
    let (mut native, root) = write::Write::new(resources, None)
        .with_groups(groups)
        .fragment(node, destination)?;
    ensure!(
        native.gates.is_empty()
            && native
                .references
                .iter()
                .all(|reference| reference.target.is_some()),
        "event label filter has unbound categories or dictionary references"
    );
    let lists = native.payload.bytes::<80>(root)?;
    let property = native.payload.bytes::<16>(root + 80)?;
    put(&mut native.payload.0, to.0, &lists)?;
    put(&mut native.payload.0, to.1, &property)?;
    for (field, target) in [
        (root + 8, to.0 + 8),
        (root + 24, to.0 + 24),
        (root + 40, to.0 + 40),
        (root + 56, to.0 + 56),
        (root + 64, to.0 + 64),
        (root + 88, to.1 + 8),
    ] {
        let delta = if native.payload.u64(field)? == 0 {
            0
        } else {
            native.payload.pointer(field)? as i64 - target as i64
        };
        put(&mut native.payload.0, target, &delta.to_le_bytes())?;
    }
    *destination = native.payload;
    Ok(())
}

/// Translate a selector embedded in a component definition. Its containing
/// field supplies ownership, so it need not have a standalone class prefix.
pub fn append_selector(
    source: &Payload,
    from: usize,
    destination: &mut Payload,
    to: usize,
    resources: &Resources,
    groups: &GroupBindings,
    resolver: &ResourceResolver,
) -> Result<Fragment> {
    let mut read = read::Read::new(source, Some(resolver));
    let node = read.selector(from, 0)?;
    let (source_spans, source_references) = read.ownership();
    let (native, root) = write::Write::new(resources, Some(resolver))
        .with_groups(groups)
        .fragment(node, destination)?;
    let mut payload = native.payload;
    let flags = payload.bytes::<8>(root)?;
    let count = payload.u64(root + 8)?;
    let target = if count == 0 {
        None
    } else {
        Some(payload.pointer(root + 16)?)
    };
    put(&mut payload.0, to, &flags)?;
    put(&mut payload.0, to + 8, &count.to_le_bytes())?;
    let delta = target.map_or(0, |target| target as i64 - (to + 16) as i64);
    put(&mut payload.0, to + 16, &delta.to_le_bytes())?;
    *destination = payload;
    Ok(Fragment {
        root: to,
        references: native.references,
        reference_classes: native.external_classes,
        gates: native.gates,
        source_spans,
        source_references,
        named_conditions: native.named_conditions,
        group_aliases: native.group_aliases,
    })
}

/// Translate a compiled category predicate at a declared typed-pointer field.
/// Its flags and both/four mask banks are retained. Missing category bindings
/// remain explicit gates for the containing resource converter.
pub fn append_mask(
    source: &Payload,
    from: usize,
    destination: &mut Payload,
    to: usize,
    resources: &Resources,
) -> Result<Vec<Gate>> {
    ensure!(
        resources.names.len() == resources.categories.len() && resources.names.len() <= 448,
        "selector category correspondence differs"
    );
    let mask = read::Read::new(source, None).masks(from)?;
    let (payload, gates) =
        write::Write::new(resources, None).mask_fragment(mask, destination, to)?;
    *destination = payload;
    Ok(gates)
}

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
#[derive(Clone, Default)]
pub struct GroupBindings {
    pub source: BTreeMap<u32, BTreeSet<u32>>,
    pub native: BTreeMap<u32, BTreeSet<u32>>,
    pub aliases: BTreeMap<u32, u32>,
    pub canonical: BTreeMap<u32, u32>,
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
            canonical: namespace.query_aliases.clone(),
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
                mapped.insert(self.canonical.get(member).copied().unwrap_or(*member));
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

/// Preserve the target predicate of a modern damage-event condition. The caller
/// owns the containing condition, while this fragment owns only its selector tree.
pub fn append_hit_selector(
    source: &Payload,
    condition: usize,
    destination: &mut Payload,
    resources: &Resources,
    groups: &GroupBindings,
    resolver: &ResourceResolver,
) -> Result<Fragment> {
    ensure!(
        source.u32(condition.checked_sub(4).context("hit condition prefix")?)? == 0x8080B7B3
            && source.0.get(condition..condition + 360).is_some()
            && destination.0.len() >= 208,
        "damage-event selector contract differs"
    );
    let mut read = read::Read::new(source, Some(resolver));
    let node = read.selector(condition + 0x138, 0)?;
    let (source_spans, source_references) = read.ownership();
    let native = write::Write::new(resources, Some(resolver))
        .with_groups(groups)
        .inline(node, destination, 0xA8)?;
    let reference_classes = native
        .references
        .iter()
        .map(|reference| {
            (
                reference.offset,
                native
                    .external_classes
                    .get(&reference.offset)
                    .copied()
                    .unwrap_or(crate::d2_mot::native::categories::NATIVE_CLASS),
            )
        })
        .collect();
    *destination = native.payload;
    Ok(Fragment {
        root: 0xA8,
        references: native.references,
        reference_classes,
        gates: native.gates,
        source_spans,
        source_references,
        named_conditions: native.named_conditions,
        group_aliases: native.group_aliases,
    })
}
