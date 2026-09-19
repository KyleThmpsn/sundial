use super::*;

#[derive(Debug)]
pub(super) enum Line {
    Branch {
        label: String,
        path: Vec<String>,
        depth: usize,
        expanded: bool,
        indices: Vec<u16>,
        counts: acquisition::AcquisitionCounts,
    },
    Leaf {
        position: usize,
        depth: usize,
    },
}
#[derive(Debug)]
pub(super) struct Cache {
    query: String,
    filter: CollectionStatusFilter,
    sort: TableSort,
    expansion: HashMap<Vec<String>, bool>,
    pub counts: acquisition::AcquisitionCounts,
    pub indices: Vec<u16>,
    pub lines: Vec<Line>,
}
impl Cache {
    pub fn current(&self, state: &UiState) -> bool {
        self.query == state.query.trim().to_lowercase()
            && self.filter == state.status_filter
            && self.sort == state.sort
            && self.expansion == state.expansion
            && !state.reveal_selection
    }
    pub fn build(catalog: &Catalog, state: &mut UiState, expand: Option<bool>) -> Self {
        let query = state.query.trim().to_lowercase();
        let leaves = catalog
            .collectibles()
            .iter()
            .zip(
                &state
                    .cached_status
                    .as_ref()
                    .expect("collection statuses")
                    .rows,
            )
            .filter(|(definition, _)| collection_matches(&query, definition, catalog))
            .map(|(definition, status)| CollectionLeaf {
                definition,
                status: status.clone(),
            })
            .collect::<Vec<_>>();
        let counts = acquisition_counts(&leaves);
        let visible = leaves
            .into_iter()
            .filter(|leaf| state.status_filter.matches(leaf.status.state))
            .collect::<Vec<_>>();
        let indices = visible
            .iter()
            .map(|leaf| leaf.definition.index)
            .collect::<Vec<_>>();
        let mut tree = build_hierarchy(&visible);
        if let Some(expanded) = expand {
            set_all_expansion(&tree, &mut state.expansion, expanded);
        }
        if state.reveal_selection {
            set_all_expansion(&tree, &mut state.expansion, true);
        }
        sort_hierarchy(&mut tree, state.sort);
        let positions = catalog
            .collectibles()
            .iter()
            .enumerate()
            .map(|(position, definition)| (definition.index, position))
            .collect::<HashMap<_, _>>();
        let lines = display_lines(&tree, &state.expansion, !query.is_empty())
            .into_iter()
            .map(|line| match line {
                DisplayLine::Branch {
                    branch,
                    depth,
                    expanded,
                } => Line::Branch {
                    label: branch.label.clone(),
                    path: branch.path.clone(),
                    depth,
                    expanded,
                    indices: bulk::branch_indices(branch),
                    counts: branch_counts(branch),
                },
                DisplayLine::Leaf { leaf, depth } => Line::Leaf {
                    position: positions[&leaf.definition.index],
                    depth,
                },
            })
            .collect();
        Self {
            query,
            filter: state.status_filter,
            sort: state.sort,
            expansion: state.expansion.clone(),
            counts,
            indices,
            lines,
        }
    }
}
