//! Keep independently requested native owners out of one another's loading indexes.
use super::{scoped::LoadingOwner, *};

/// References between resources in one authored asset graph, including entry references.
#[derive(Clone, Debug)]
pub struct LoadingResource {
    pub tag: TagHash,
    pub references: Vec<TagHash>,
}

fn collect(
    graph: &BTreeMap<TagHash, &LoadingResource>,
    boundaries: &BTreeMap<TagHash, TagHash>,
    owner: TagHash,
    seeds: impl IntoIterator<Item = TagHash>,
    result: &mut BTreeSet<TagHash>,
) -> AuthoringResult<()> {
    let mut pending = seeds.into_iter().collect::<Vec<_>>();
    while let Some(tag) = pending.pop() {
        if boundaries
            .get(&tag)
            .is_some_and(|boundary| *boundary != owner)
        {
            return Err(invalid(format!(
                "Loading owner {owner} references another owner boundary {tag}"
            )));
        }
        if result.insert(tag) {
            let resource = graph.get(&tag).ok_or_else(|| {
                invalid(format!(
                    "Loading reference {tag} is outside the resource graph"
                ))
            })?;
            pending.extend(resource.references.iter().copied());
        }
    }
    Ok(())
}

/// Partition an asset graph into independently loadable native owner closures.
/// Shared leaf resources may occur in several closures. Retained resources with
/// no incoming references belong to the primary owner so authoring drops none.
pub fn partition_loading_resources(
    resources: &[LoadingResource],
    owners: &[LoadingOwner],
    primary: TagHash,
) -> AuthoringResult<BTreeMap<TagHash, Vec<TagHash>>> {
    let mut graph = BTreeMap::new();
    for resource in resources {
        let tag = resource.tag;
        if TagHash::new(tag.pkg_id(), tag.entry_index()) != tag
            || !(0x100..=0xCFF).contains(&tag.pkg_id())
            || graph.insert(tag, resource).is_some()
        {
            return Err(invalid("Loading resources require unique canonical tags"));
        }
    }
    let mut boundaries = BTreeMap::new();
    for owner in owners {
        for tag in [owner.owner, owner.companion] {
            if !graph.contains_key(&tag) || boundaries.insert(tag, owner.owner).is_some() {
                return Err(invalid(
                    "Loading boundaries must be unique members of the resource graph",
                ));
            }
        }
    }
    if !owners.iter().any(|owner| owner.owner == primary) {
        return Err(invalid("The primary loading owner is missing"));
    }
    let mut groups = BTreeMap::new();
    let mut covered = BTreeSet::new();
    for owner in owners {
        let mut group = BTreeSet::new();
        collect(
            &graph,
            &boundaries,
            owner.owner,
            [owner.owner, owner.companion],
            &mut group,
        )?;
        covered.extend(group.iter().copied());
        groups.insert(owner.owner, group);
    }
    let retained = graph
        .keys()
        .filter(|tag| !covered.contains(tag))
        .copied()
        .collect::<Vec<_>>();
    collect(
        &graph,
        &boundaries,
        primary,
        retained,
        groups.get_mut(&primary).expect("primary checked"),
    )?;
    Ok(groups
        .into_iter()
        .map(|(owner, group)| (owner, group.into_iter().collect()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tag(i: u16) -> TagHash {
        TagHash::new(0xAA0, i)
    }
    fn resource(i: u16, refs: &[u16]) -> LoadingResource {
        LoadingResource {
            tag: tag(i),
            references: refs.iter().map(|&i| tag(i)).collect(),
        }
    }
    #[test]
    fn separates_owners_retains_shared_leaves_and_unreferenced_resources() {
        let owners = [
            LoadingOwner {
                owner: tag(0),
                companion: tag(1),
            },
            LoadingOwner {
                owner: tag(2),
                companion: tag(3),
            },
        ];
        let resources = vec![
            resource(0, &[4]),
            resource(1, &[]),
            resource(2, &[4]),
            resource(3, &[]),
            resource(4, &[4]),
            resource(5, &[]),
        ];
        let groups = partition_loading_resources(&resources, &owners, tag(0)).unwrap();
        assert_eq!(groups[&tag(0)], vec![tag(0), tag(1), tag(4), tag(5)]);
        assert_eq!(groups[&tag(2)], vec![tag(2), tag(3), tag(4)]);
        let mut crossed = resources.clone();
        crossed[4].references.push(tag(2));
        assert!(partition_loading_resources(&crossed, &owners, tag(0)).is_err());
        crossed[4].references = vec![tag(99)];
        assert!(partition_loading_resources(&crossed, &owners, tag(0)).is_err());
        assert!(partition_loading_resources(&resources, &owners, tag(99)).is_err());
        assert!(partition_loading_resources(&resources, &[owners[0], owners[0]], tag(0)).is_err());
    }
}
