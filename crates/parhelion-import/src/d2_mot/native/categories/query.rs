//! Exact union-query projection with fixed stock positions and no dynamic writes.
use super::*;

#[derive(Default)]
pub struct Queries {
    pub required: BTreeSet<u32>,
    pub pinned: BTreeSet<u32>,
    pub unions: Vec<BTreeSet<u32>>,
    pub groups: BTreeSet<u32>,
}

/// Every union bank must be included. All-members predicates and every other
/// category use must be pinned. The caller must exclude mutable publishers.
/// A projected set bit means any source member of its equivalence class is set.
/// Identical membership in all union banks then preserves their intersections.
pub fn emit_queries(source: &Payload, stock: &Payload, queries: &Queries) -> Result<Namespace> {
    let original = Dictionary::read(source, true)?;
    let native = Dictionary::read(stock, false)?;
    let native_names = native.names.iter().copied().collect::<BTreeSet<_>>();
    let mut buckets = BTreeMap::<(Option<u32>, Vec<bool>), Vec<u32>>::new();
    for &name in &original.names {
        if !queries.required.contains(&name) || native_names.contains(&name) {
            continue;
        }
        let signature = queries
            .unions
            .iter()
            .map(|bank| bank.contains(&name))
            .collect::<Vec<_>>();
        let pinned = queries.pinned.contains(&name) || !signature.iter().any(|&bit| bit);
        buckets
            .entry((pinned.then_some(name), signature))
            .or_default()
            .push(name);
    }
    let mut requested = queries
        .required
        .intersection(&native_names)
        .copied()
        .collect::<BTreeSet<_>>();
    requested.extend(buckets.values().map(|bucket| bucket[0]));
    // Validate unknown requested identities even if a malformed caller supplied
    // them outside every query bank.
    let source_names = original.names.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        queries.required.is_subset(&source_names),
        "query category is absent from the source registry"
    );
    let mut namespace = emit_static(source, stock, &requested)?;
    let mut dictionary = Dictionary::read(&namespace.payload, false)?;
    let mut occupied = dictionary
        .names
        .iter()
        .copied()
        .chain(dictionary.groups.iter().map(|group| group.name))
        .collect::<BTreeSet<_>>();
    for bucket in buckets.into_values() {
        let representative = bucket[0];
        let index = namespace
            .names
            .iter()
            .position(|&name| name == representative)
            .context("query representative was not allocated")?;
        for name in bucket.into_iter().skip(1) {
            ensure!(
                occupied.insert(name),
                "query atom alias collides with a native name or group"
            );
            ensure!(
                dictionary.groups.len() < 4096,
                "private category group capacity exceeded"
            );
            dictionary.groups.push(Group {
                name,
                bits: BTreeSet::from([index]),
            });
            let source_index = namespace
                .source_names
                .iter()
                .position(|&source| source == name)
                .context("query source atom disappeared")?;
            namespace.source_indices[source_index] = Some(u16::try_from(index)?);
            namespace.query_aliases.insert(name, representative);
        }
    }
    namespace.payload = dictionary.emit()?;
    namespace.groups = dictionary.named_groups();
    namespace.with_group_aliases(&queries.groups)
}
