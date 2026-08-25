use std::{cmp::Ordering, collections::HashMap};

use crate::catalog::CollectibleDef;

use super::{
    TableSort,
    acquisition::{AcquisitionCounts, StateLine},
};

#[derive(Clone)]
pub(super) struct CollectionLeaf<'a> {
    pub(super) definition: &'a CollectibleDef,
    pub(super) status: StateLine,
}

#[derive(Default)]
pub(super) struct CollectionHierarchy<'a> {
    pub(super) branches: Vec<CollectionBranch<'a>>,
    pub(super) leaves: Vec<CollectionLeaf<'a>>,
}

pub(super) struct CollectionBranch<'a> {
    pub(super) label: String,
    pub(super) path: Vec<String>,
    pub(super) branches: Vec<CollectionBranch<'a>>,
    pub(super) leaves: Vec<CollectionLeaf<'a>>,
}

pub(super) enum DisplayLine<'tree, 'data> {
    Branch {
        branch: &'tree CollectionBranch<'data>,
        depth: usize,
        expanded: bool,
    },
    Leaf {
        leaf: &'tree CollectionLeaf<'data>,
        depth: usize,
    },
}

pub(super) fn build_hierarchy<'a>(leaves: &[CollectionLeaf<'a>]) -> CollectionHierarchy<'a> {
    let mut hierarchy = CollectionHierarchy::default();
    for leaf in leaves {
        let paths = collection_paths(&leaf.definition.paths);
        if paths.is_empty() {
            hierarchy.leaves.push(leaf.clone());
            continue;
        }
        for path in paths {
            insert_leaf(&mut hierarchy.branches, &path, leaf.clone());
        }
    }
    hierarchy
}

pub(super) fn acquisition_counts(leaves: &[CollectionLeaf<'_>]) -> AcquisitionCounts {
    let mut counts = AcquisitionCounts::default();
    for leaf in leaves {
        counts.add(leaf.status.state);
    }
    counts
}

pub(super) fn branch_counts(branch: &CollectionBranch<'_>) -> AcquisitionCounts {
    let mut counts = acquisition_counts(&branch.leaves);
    for child in &branch.branches {
        let child = branch_counts(child);
        counts.acquired += child.acquired;
        counts.missing += child.missing;
        counts.no_rule += child.no_rule;
        counts.unknown += child.unknown;
    }
    counts
}

pub(super) fn set_all_expansion(
    hierarchy: &CollectionHierarchy<'_>,
    expansion: &mut HashMap<Vec<String>, bool>,
    expanded: bool,
) {
    fn visit(
        branch: &CollectionBranch<'_>,
        expansion: &mut HashMap<Vec<String>, bool>,
        expanded: bool,
    ) {
        expansion.insert(branch.path.clone(), expanded);
        for child in &branch.branches {
            visit(child, expansion, expanded);
        }
    }
    for branch in &hierarchy.branches {
        visit(branch, expansion, expanded);
    }
}

pub(super) fn root_first_path(raw_path: &[String]) -> Vec<String> {
    raw_path
        .iter()
        .rev()
        .filter_map(|component| {
            let component = component.trim();
            (!component.is_empty()).then(|| component.to_owned())
        })
        .collect()
}

pub(super) fn collection_paths(raw_paths: &[Vec<String>]) -> Vec<Vec<String>> {
    raw_paths
        .iter()
        .map(|path| root_first_path(path))
        .filter(|path| !path.is_empty())
        .fold(Vec::<Vec<String>>::new(), |mut paths, path| {
            if !paths.contains(&path) {
                paths.push(path);
            }
            paths
        })
}

fn insert_leaf<'a>(
    branches: &mut Vec<CollectionBranch<'a>>,
    path: &[String],
    leaf: CollectionLeaf<'a>,
) {
    fn insert_at<'a>(
        branches: &mut Vec<CollectionBranch<'a>>,
        path: &[String],
        depth: usize,
        leaf: CollectionLeaf<'a>,
    ) {
        let label = &path[depth];
        let index = branches
            .iter()
            .position(|branch| branch.label == *label)
            .unwrap_or_else(|| {
                branches.push(CollectionBranch {
                    label: label.clone(),
                    path: path[..=depth].to_vec(),
                    branches: Vec::new(),
                    leaves: Vec::new(),
                });
                branches.len() - 1
            });
        if depth + 1 == path.len() {
            branches[index].leaves.push(leaf);
        } else {
            insert_at(&mut branches[index].branches, path, depth + 1, leaf);
        }
    }
    insert_at(branches, path, 0, leaf);
}

pub(super) fn sort_hierarchy(hierarchy: &mut CollectionHierarchy<'_>, sort: TableSort) {
    fn sort_branch(branch: &mut CollectionBranch<'_>, sort: TableSort) {
        branch
            .leaves
            .sort_by(|left, right| compare_leaves(left, right, sort));
        for child in &mut branch.branches {
            sort_branch(child, sort);
        }
    }
    hierarchy
        .leaves
        .sort_by(|left, right| compare_leaves(left, right, sort));
    for branch in &mut hierarchy.branches {
        sort_branch(branch, sort);
    }
}

fn compare_leaves(
    left: &CollectionLeaf<'_>,
    right: &CollectionLeaf<'_>,
    sort: TableSort,
) -> Ordering {
    let ordering = match sort.column {
        0 => left
            .definition
            .name
            .to_lowercase()
            .cmp(&right.definition.name.to_lowercase()),
        1 => left
            .definition
            .type_name
            .to_lowercase()
            .cmp(&right.definition.type_name.to_lowercase()),
        2 => left.status.text.cmp(&right.status.text),
        3 => left.definition.index.cmp(&right.definition.index),
        4 => left.definition.hash.cmp(&right.definition.hash),
        _ => Ordering::Equal,
    };
    if sort.descending {
        ordering.reverse()
    } else {
        ordering
    }
}

pub(super) fn display_lines<'tree, 'data>(
    hierarchy: &'tree CollectionHierarchy<'data>,
    expansion: &HashMap<Vec<String>, bool>,
    auto_expand: bool,
) -> Vec<DisplayLine<'tree, 'data>> {
    fn append_leaf<'tree, 'data>(
        output: &mut Vec<DisplayLine<'tree, 'data>>,
        leaf: &'tree CollectionLeaf<'data>,
        depth: usize,
    ) {
        output.push(DisplayLine::Leaf { leaf, depth });
    }
    fn append_branch<'tree, 'data>(
        output: &mut Vec<DisplayLine<'tree, 'data>>,
        branch: &'tree CollectionBranch<'data>,
        expansion: &HashMap<Vec<String>, bool>,
        auto_expand: bool,
        depth: usize,
    ) {
        let expanded = auto_expand || expansion.get(&branch.path).copied().unwrap_or(depth == 0);
        output.push(DisplayLine::Branch {
            branch,
            depth,
            expanded,
        });
        if !expanded {
            return;
        }
        for leaf in &branch.leaves {
            append_leaf(output, leaf, depth + 1);
        }
        for child in &branch.branches {
            append_branch(output, child, expansion, auto_expand, depth + 1);
        }
    }
    let mut output = Vec::new();
    for branch in &hierarchy.branches {
        append_branch(&mut output, branch, expansion, auto_expand, 0);
    }
    for leaf in &hierarchy.leaves {
        append_leaf(&mut output, leaf, 0);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_paths_render_root_first_without_synthetic_roots() {
        assert_eq!(
            root_first_path(&["Kinetic".into(), "Weapons".into(), "Items".into()]),
            ["Items", "Weapons", "Kinetic"]
        );
    }

    #[test]
    fn collections_preserve_every_package_parent_path() {
        let paths = collection_paths(&[
            vec!["Titan".into(), "Season of the Worthy".into()],
            vec![
                "Season 10".into(),
                "Ships".into(),
                "Equipment".into(),
                "Items".into(),
            ],
            vec!["Season 10".into(), "Ships".into(), "Vehicles".into()],
        ]);
        assert_eq!(
            paths,
            [
                vec![String::from("Season of the Worthy"), String::from("Titan")],
                vec![
                    String::from("Items"),
                    String::from("Equipment"),
                    String::from("Ships"),
                    String::from("Season 10"),
                ],
                vec![
                    String::from("Vehicles"),
                    String::from("Ships"),
                    String::from("Season 10"),
                ],
            ]
        );
    }
}
