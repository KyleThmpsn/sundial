use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum View {
    Unlocks,
    Investment,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum UnlockTable {
    #[default]
    AccountFlagRuns,
    ProfileFlagRuns,
    CharacterFlags,
    ObjectiveValues,
    CharacterObjectFlagRuns,
    CharacterObjectObjectiveValues,
    AccountProgressions,
    CharacterProgressions,
    UnreplicatedProgressions,
}

impl UnlockTable {
    pub(super) const ALL: [Self; 9] = [
        Self::AccountFlagRuns,
        Self::ProfileFlagRuns,
        Self::CharacterFlags,
        Self::ObjectiveValues,
        Self::CharacterObjectFlagRuns,
        Self::CharacterObjectObjectiveValues,
        Self::AccountProgressions,
        Self::CharacterProgressions,
        Self::UnreplicatedProgressions,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::AccountFlagRuns => "Account Acquired Flags",
            Self::ProfileFlagRuns => "Profile Unlock Flags",
            Self::CharacterFlags => "Character Flags",
            Self::ObjectiveValues => "Account Objective Values",
            Self::CharacterObjectFlagRuns => "Character Object Acquired Flags",
            Self::CharacterObjectObjectiveValues => "Character Object Objective Values",
            Self::AccountProgressions => "Account Progressions",
            Self::CharacterProgressions => "Character Progressions",
            Self::UnreplicatedProgressions => "Unreplicated Progressions",
        }
    }

    pub(super) const fn field_name(self) -> Option<&'static str> {
        match self {
            Self::AccountFlagRuns => Some("account_flag_runs"),
            Self::ProfileFlagRuns => Some("profile_flag_runs"),
            Self::CharacterFlags => Some("character_flags"),
            Self::ObjectiveValues => Some("objective_values"),
            Self::CharacterObjectFlagRuns => Some("character_flag_runs"),
            Self::CharacterObjectObjectiveValues => Some("character_objective_values"),
            Self::AccountProgressions => Some("account_progressions"),
            Self::CharacterProgressions => Some("character_progressions"),
            Self::UnreplicatedProgressions => None,
        }
    }

    pub(super) const fn is_progression(self) -> bool {
        matches!(
            self,
            Self::AccountProgressions | Self::CharacterProgressions
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum InvestmentTable {
    #[default]
    FlagOverrides,
    ValueOverrides,
}

impl InvestmentTable {
    pub(super) const ALL: [Self; 2] = [Self::FlagOverrides, Self::ValueOverrides];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::FlagOverrides => "Unlock Flag Overrides",
            Self::ValueOverrides => "Unlock Value Overrides",
        }
    }

    pub(super) const fn explanation(self) -> &'static str {
        match self {
            Self::FlagOverrides => {
                "Overrides the unlock flag used by the linked activities and objectives."
            }
            Self::ValueOverrides => {
                "Overrides the value used by the linked activities and objectives."
            }
        }
    }

    pub(super) const fn field_name(self) -> &'static str {
        match self {
            Self::FlagOverrides => "family5_flag_overrides",
            Self::ValueOverrides => "family5_value_overrides",
        }
    }
}

#[derive(Debug, Default)]
pub(in crate::app) struct UiState {
    pub(in crate::app) read_only: bool,
    pub(super) unlock_table: UnlockTable,
    pub(super) investment_table: InvestmentTable,
    pub(super) query: String,
    pub(super) native_query: String,
    pub(super) definition_query: String,
    pub(super) browse_values: bool,
    pub(super) table_sorts: HashMap<&'static str, TableSort>,
    pub(super) objective_expansion: HashMap<ObjectiveBranchKey, bool>,
    pub(super) add_open: bool,
    pub(super) add_query: String,
    pub(super) add_value: i32,
    pub(super) add_progression_lanes: [i32; 3],
    pub(super) cached_progression: Option<Result<Progression, String>>,
    pub(super) metadata_inspector: ProgressionInspectorState,
    pub(super) hash_inspection: HashInspectionState,
    pub(super) override_filter: OverrideFilter,
    pub(super) last_investment_change: Option<InvestmentUndo>,
    pub(super) edit_progression_lanes: bool,
    pub(super) progression_baselines: HashMap<(&'static str, usize), Option<[i32; 3]>>,
    pub(super) last_progression_change: Option<ProgressionUndo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InvestmentUndo {
    Flag {
        definition_index: usize,
        previous: Option<u8>,
    },
    Value {
        definition_index: usize,
        previous: Option<i32>,
    },
}

impl InvestmentUndo {
    pub(super) fn label(self) -> String {
        match self {
            Self::Flag {
                definition_index,
                previous,
            } => previous.map_or_else(
                || format!("Remove newly added flag #{definition_index}"),
                |value| format!("Restore flag #{definition_index} to {value}"),
            ),
            Self::Value {
                definition_index,
                previous,
            } => previous.map_or_else(
                || format!("Remove newly added value #{definition_index}"),
                |value| format!("Restore value #{definition_index} to {value}"),
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ProgressionUndo {
    pub(super) table: &'static str,
    pub(super) definition_index: usize,
    pub(super) previous: Option<[i32; 3]>,
}

impl ProgressionUndo {
    pub(super) fn label(self) -> String {
        let action = if self.previous.is_some() {
            "Restore"
        } else {
            "Remove newly added"
        };
        format!("{action} progression #{}", self.definition_index)
    }
}

impl UiState {
    pub(in crate::app) fn reset_navigation(&mut self) {
        self.query.clear();
        self.add_open = false;
        self.add_query.clear();
        self.metadata_inspector.reset();

        self.hash_inspection.close();
    }

    pub(in crate::app) fn invalidate_document(&mut self) {
        self.cached_progression = None;
        self.progression_baselines.clear();
        self.last_progression_change = None;
    }

    pub(in crate::app) fn mark_saved(&mut self) {
        self.progression_baselines.clear();
        self.last_progression_change = None;
    }

    pub(super) fn record_progression_change(
        &mut self,
        table: &'static str,
        definition_index: usize,
        previous: Option<[i32; 3]>,
        current: Option<[i32; 3]>,
    ) {
        let key = (table, definition_index);
        let baseline = *self.progression_baselines.entry(key).or_insert(previous);
        if current == baseline {
            self.progression_baselines.remove(&key);
        }

        let keep_previous = self.last_progression_change.is_some_and(|change| {
            change.table == table && change.definition_index == definition_index
        });
        if !keep_previous {
            self.last_progression_change = Some(ProgressionUndo {
                table,
                definition_index,
                previous,
            });
        }
    }

    pub(super) fn progression_changed(&self, table: &'static str, definition_index: usize) -> bool {
        self.progression_baselines
            .contains_key(&(table, definition_index))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TableSort {
    pub(super) column: usize,
    pub(super) descending: bool,
}

impl TableSort {
    pub(super) const fn ascending(column: usize) -> Self {
        Self {
            column,
            descending: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ObjectiveBranchKey {
    pub(super) table: &'static str,
    pub(super) path: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ProgressionDisplayRow {
    pub(super) definition_index: usize,
    pub(super) lanes: Option<[i32; 3]>,
}
