use crate::app::PlugSelectionMode;

pub(in crate::app::equipment) const STAT_NAMES: [&str; 6] = [
    "Mobility",
    "Resilience",
    "Recovery",
    "Discipline",
    "Intellect",
    "Strength",
];
pub(in crate::app::equipment) const TARGET_MAX: u16 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AllocationGroup {
    Top,
    Bottom,
}

impl AllocationGroup {
    pub(super) const ALL: [Self; 2] = [Self::Top, Self::Bottom];

    pub(super) const fn indices(self) -> [usize; 3] {
        match self {
            Self::Top => [0, 1, 2],
            Self::Bottom => [3, 4, 5],
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Top => "Top allocation",
            Self::Bottom => "Bottom allocation",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct AllocationFeedback {
    pub(super) text: String,
    pub(super) is_error: bool,
}

#[derive(Clone, Debug, Default)]
pub(in crate::app::equipment) struct State {
    pub(super) targets: [u16; 6],
    pub(super) feedback: Option<AllocationFeedback>,
}

impl State {
    pub(in crate::app::equipment) fn clear_feedback(&mut self) {
        self.feedback = None;
    }

    pub(in crate::app::equipment) fn feedback(&self) -> Option<(&str, bool)> {
        self.feedback
            .as_ref()
            .map(|feedback| (feedback.text.as_str(), feedback.is_error))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Choice {
    pub(super) hash: u64,
    pub(super) values: [u16; 3],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CrossGroupChoice {
    pub(super) hash: u64,
    pub(super) values: [u16; 6],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Plan {
    pub(super) hashes: Vec<u64>,
    pub(super) changes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GroupSolution {
    pub(super) assignments: Vec<(usize, u64)>,
    pub(super) values: [u16; 3],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GroupFailure {
    pub(super) best: Option<GroupSolution>,
    pub(super) reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CrossGroupSolution {
    pub(super) assignments: Vec<(usize, u64)>,
    pub(super) values: [u16; 6],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CrossGroupFailure {
    pub(super) best: Option<CrossGroupSolution>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Solution {
    pub(super) plugs: Vec<Option<u64>>,
    pub(super) changed: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SolveFailure {
    pub(super) plugs: Vec<Option<u64>>,
    pub(super) changed: usize,
    pub(super) best_totals: [u16; 6],
    pub(super) reason: Option<String>,
}

pub(super) const fn group_index(group: AllocationGroup) -> usize {
    match group {
        AllocationGroup::Top => 0,
        AllocationGroup::Bottom => 1,
    }
}

pub(super) const fn mode_allows_cross_group_allocations(mode: PlugSelectionMode) -> bool {
    matches!(
        mode,
        PlugSelectionMode::GearType | PlugSelectionMode::AnyPlug
    )
}
