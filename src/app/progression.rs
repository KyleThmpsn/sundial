use std::{
    cmp::{Ordering, Reverse},
    collections::{HashMap, HashSet},
};

use eframe::egui;
use serde_json::{Map, Value};

use crate::catalog::{
    Catalog, CollectibleDef, CollectionConditionDef, InventoryMetadata, ItemDef,
    ItemMaterialRequirementSetIndices, MaterialRequirementDef, MaterialRequirementSetDef,
    ObjectiveDef, ObjectiveOwnerDef, ObjectiveOwnerKind, ObjectiveOwnerTraitDef,
    ProgressionContextDef, ProgressionContextKind, ProgressionScope, UnlockDefinition,
};

use super::{
    glyphs::Glyph,
    ui::{
        TABLE_CELL_HEIGHT, TABLE_COLUMN_GAP, back_button,
        hierarchy_branch_cell as draw_hierarchy_branch_cell,
        hierarchy_leaf_cell as draw_hierarchy_leaf_cell, inspector_heading, sortable_header_cell,
        table_cell, toolbar as progression_toolbar,
    },
};

const ACCOUNT_FLAG_CAPACITY: usize = 12_300;
const PROFILE_FLAG_CAPACITY: usize = 512;
const CHARACTER_FLAG_CAPACITY: usize = 256;
const OBJECTIVE_VALUE_CAPACITY: usize = 6_200;
const CHARACTER_OBJECT_FLAG_CAPACITY: usize = 4_096;
const CHARACTER_OBJECT_VALUE_CAPACITY: usize = 768;
const PROGRESSION_DEFINITION_CAPACITY: usize = 256;
const FAMILY5_OVERRIDE_CAPACITY: usize = 100;
const FAMILY5_FLAG_SLOT_MAXIMUM: usize = 23_499;
const FAMILY5_VALUE_SLOT_MAXIMUM: usize = 15_499;
const FAMILY5_FLAG_VALUE_MAXIMUM: u8 = 2;
const ACCOUNT_FLAG_BANK: u8 = 1;
const PROFILE_FLAG_BANK: u8 = 2;
const CHARACTER_OBJECT_FLAG_BANK: u8 = 3;
const CHARACTER_FLAG_BANK: u8 = 6;
const ACCOUNT_OBJECTIVE_BANK: u8 = 1;
const CHARACTER_OBJECTIVE_BANK: u8 = 2;
const TABLE_ROW_GAP: f32 = 2.0;
const TABLE_ROW_STRIDE: f32 = TABLE_CELL_HEIGHT + TABLE_ROW_GAP;
const TABLE_ACTION_WIDTH: f32 = 24.0;
const CANONICAL_ROOTS: [&str; 5] = ["Items", "Triumphs", "Metrics", "Activities", "Presentation"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum View {
    Unlocks,
    Investment,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum UnlockTable {
    #[default]
    AccountFlagRuns,
    ProfileFlagRuns,
    CharacterFlags,
    ObjectiveValues,
    CharacterObjectFlagRuns,
    CharacterObjectObjectiveValues,
    AccountProgressions,
    CharacterProgressions,
}

impl UnlockTable {
    const ALL: [Self; 8] = [
        Self::AccountFlagRuns,
        Self::ProfileFlagRuns,
        Self::CharacterFlags,
        Self::ObjectiveValues,
        Self::CharacterObjectFlagRuns,
        Self::CharacterObjectObjectiveValues,
        Self::AccountProgressions,
        Self::CharacterProgressions,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::AccountFlagRuns => "Account acquired flags",
            Self::ProfileFlagRuns => "Profile unlock flags",
            Self::CharacterFlags => "Character flags",
            Self::ObjectiveValues => "Account objective values",
            Self::CharacterObjectFlagRuns => "Character object acquired flags",
            Self::CharacterObjectObjectiveValues => "Character object objective values",
            Self::AccountProgressions => "Account progressions",
            Self::CharacterProgressions => "Character progressions",
        }
    }

    const fn field_name(self) -> &'static str {
        match self {
            Self::AccountFlagRuns => "account_flag_runs",
            Self::ProfileFlagRuns => "profile_flag_runs",
            Self::CharacterFlags => "character_flags",
            Self::ObjectiveValues => "objective_values",
            Self::CharacterObjectFlagRuns => "character_flag_runs",
            Self::CharacterObjectObjectiveValues => "character_objective_values",
            Self::AccountProgressions => "account_progressions",
            Self::CharacterProgressions => "character_progressions",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum InvestmentTable {
    #[default]
    FlagOverrides,
    ValueOverrides,
}

impl InvestmentTable {
    const ALL: [Self; 2] = [Self::FlagOverrides, Self::ValueOverrides];

    const fn label(self) -> &'static str {
        match self {
            Self::FlagOverrides => "Unlock flag overrides",
            Self::ValueOverrides => "Unlock value overrides",
        }
    }

    const fn explanation(self) -> &'static str {
        match self {
            Self::FlagOverrides => {
                "Supplies the selected logical unlock-flag value when Family 5 data is rebuilt."
            }
            Self::ValueOverrides => "Supplies the selected value when Family 5 data is rebuilt.",
        }
    }

    const fn field_name(self) -> &'static str {
        match self {
            Self::FlagOverrides => "family5_flag_overrides",
            Self::ValueOverrides => "family5_value_overrides",
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct UiState {
    unlock_table: UnlockTable,
    investment_table: InvestmentTable,
    query: String,
    table_sorts: HashMap<&'static str, TableSort>,
    objective_expansion: HashMap<ObjectiveBranchKey, bool>,
    add_open: bool,
    add_query: String,
    add_value: i32,
    add_progression_lanes: [i32; 3],
    cached_progression: Option<Result<Progression, String>>,
    metadata_selection: Option<MetadataSelection>,
    metadata_history: Vec<MetadataSelection>,
    hash_inspection: HashInspectionState,
    override_filter: OverrideFilter,
    last_investment_change: Option<InvestmentUndo>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum OverrideFilter {
    #[default]
    All,
    Unmapped,
    NoResolvedReaders,
    PartiallyDecoded,
}

impl OverrideFilter {
    const ALL: [Self; 4] = [
        Self::All,
        Self::Unmapped,
        Self::NoResolvedReaders,
        Self::PartiallyDecoded,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::All => "All coverage",
            Self::Unmapped => "Not in package table",
            Self::NoResolvedReaders => "No package references",
            Self::PartiallyDecoded => "Partially decoded",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InvestmentUndo {
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
    fn label(self) -> String {
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
enum MetadataSelection {
    FlagDefinition(usize),
    ValueDefinition(usize),
    FlagOverride(usize, u8),
    ValueOverride(usize, i32),
}

impl MetadataSelection {
    const fn definition_index(self) -> usize {
        match self {
            Self::FlagDefinition(index)
            | Self::ValueDefinition(index)
            | Self::FlagOverride(index, _)
            | Self::ValueOverride(index, _) => index,
        }
    }

    const fn is_value(self) -> bool {
        matches!(self, Self::ValueDefinition(_) | Self::ValueOverride(_, _))
    }
}

impl UiState {
    pub(super) fn reset_navigation(&mut self) {
        self.query.clear();
        self.add_open = false;
        self.add_query.clear();
        self.metadata_selection = None;
        self.metadata_history.clear();
        self.hash_inspection.close();
    }

    pub(super) fn invalidate_document(&mut self) {
        self.cached_progression = None;
    }

    fn open_metadata(&mut self, selection: MetadataSelection) {
        if self.metadata_selection == Some(selection) {
            return;
        }
        if let Some(current) = self.metadata_selection {
            self.metadata_history.push(current);
        }
        self.metadata_selection = Some(selection);
    }

    fn metadata_back(&mut self) {
        self.metadata_selection = self.metadata_history.pop();
    }

    fn close_metadata(&mut self) {
        self.metadata_selection = None;
        self.metadata_history.clear();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TableSort {
    column: usize,
    descending: bool,
}

impl TableSort {
    const fn ascending(column: usize) -> Self {
        Self {
            column,
            descending: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ObjectiveBranchKey {
    table: &'static str,
    path: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FlagRun {
    start: usize,
    length: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FlagIndex {
    index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IndexedValue {
    index: usize,
    value: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProgressionValue {
    definition_index: usize,
    lanes: [i32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FlagOverride {
    definition_index: usize,
    value: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ValueOverride {
    definition_index: usize,
    value: i32,
}

#[derive(Clone)]
struct ContextDisplayLine<'a> {
    name: String,
    path: Vec<String>,
    contexts: Vec<&'a ProgressionContextDef>,
}

impl ContextDisplayLine<'_> {
    fn text(&self) -> String {
        match (self.name.is_empty(), self.path.is_empty()) {
            (false, false) => format!("{}: {}", self.path.join(" > "), self.name),
            (false, true) => self.name.clone(),
            (true, false) => self.path.join(" > "),
            (true, true) => "—".into(),
        }
    }
}

struct DefinitionContextDisplayLine<'a> {
    row_index: usize,
    definition_index: Option<usize>,
    definition: Option<&'a UnlockDefinition>,
    context: Option<ContextDisplayLine<'a>>,
    primary: bool,
}

#[derive(Clone, Copy)]
struct ObjectiveHierarchyLeaf<'a> {
    row: &'a IndexedValue,
    definition_index: Option<usize>,
    definition: Option<&'a UnlockDefinition>,
    objective_index: Option<usize>,
    objective: Option<&'a ObjectiveDef>,
}

#[derive(Default)]
struct ObjectiveHierarchy<'a> {
    branches: Vec<ObjectiveHierarchyBranch<'a>>,
    leaves: Vec<ObjectiveHierarchyLeaf<'a>>,
}

struct ObjectiveHierarchyBranch<'a> {
    label: String,
    path: Vec<String>,
    children: Vec<ObjectiveHierarchyBranch<'a>>,
    leaves: Vec<ObjectiveHierarchyLeaf<'a>>,
}

impl ObjectiveHierarchyBranch<'_> {
    fn new(label: String, path: Vec<String>) -> Self {
        Self {
            label,
            path,
            children: Vec::new(),
            leaves: Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
enum ObjectiveMatrixLine<'tree, 'data> {
    Branch {
        branch: &'tree ObjectiveHierarchyBranch<'data>,
        depth: usize,
        expanded: bool,
    },
    Leaf {
        leaf: &'tree ObjectiveHierarchyLeaf<'data>,
        depth: usize,
    },
}

#[derive(Debug, Default, PartialEq, Eq)]
struct UnlockPolicy {
    account_flag_runs: Vec<FlagRun>,
    profile_flag_runs: Vec<FlagRun>,
    character_flags: Vec<FlagIndex>,
    objective_values: Vec<IndexedValue>,
    character_object_flag_runs: Vec<FlagRun>,
    character_objective_values: Vec<IndexedValue>,
    account_progressions: Vec<ProgressionValue>,
    character_progressions: Vec<ProgressionValue>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct InvestmentPolicy {
    flag_overrides: Vec<FlagOverride>,
    value_overrides: Vec<ValueOverride>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Progression {
    unlocks: UnlockPolicy,
    investment: InvestmentPolicy,
}

pub(super) struct CollectionStateSnapshot {
    flags: HashSet<(u8, usize)>,
    values: HashMap<(u8, usize), i32>,
    flag_overrides: HashMap<usize, u8>,
    value_overrides: HashMap<usize, i32>,
}

impl CollectionStateSnapshot {
    pub(super) fn flag_value(
        &self,
        definition_index: usize,
        definition: &UnlockDefinition,
    ) -> Option<bool> {
        let Some(slot) = definition.compact_slot.map(usize::from) else {
            return match self.flag_overrides.get(&definition_index).copied() {
                Some(0) => Some(false),
                Some(2) => Some(true),
                _ => None,
            };
        };
        matches!(
            definition.bank(),
            ACCOUNT_FLAG_BANK
                | PROFILE_FLAG_BANK
                | CHARACTER_OBJECT_FLAG_BANK
                | CHARACTER_FLAG_BANK
        )
        .then(|| self.flags.contains(&(definition.bank(), slot)))
    }

    pub(super) fn value(
        &self,
        definition_index: usize,
        definition: &UnlockDefinition,
    ) -> Option<i32> {
        let Some(slot) = definition.compact_slot.map(usize::from) else {
            return self.value_overrides.get(&definition_index).copied();
        };
        matches!(
            definition.bank(),
            ACCOUNT_OBJECTIVE_BANK | CHARACTER_OBJECTIVE_BANK
        )
        .then(|| {
            self.values
                .get(&(definition.bank(), slot))
                .copied()
                .unwrap_or_default()
        })
    }

    pub(super) fn flag_text(
        &self,
        definition_index: usize,
        definition: &UnlockDefinition,
    ) -> String {
        let Some(slot) = definition.compact_slot.map(usize::from) else {
            return self
                .flag_overrides
                .get(&definition_index)
                .map_or_else(|| "No override".into(), |value| format!("Override {value}"));
        };
        if !matches!(
            definition.bank(),
            ACCOUNT_FLAG_BANK
                | PROFILE_FLAG_BANK
                | CHARACTER_OBJECT_FLAG_BANK
                | CHARACTER_FLAG_BANK
        ) {
            return "Unavailable".into();
        }
        if self.flags.contains(&(definition.bank(), slot)) {
            "Set".into()
        } else {
            "Unset".into()
        }
    }

    pub(super) fn value_text(
        &self,
        definition_index: usize,
        definition: &UnlockDefinition,
    ) -> String {
        let Some(slot) = definition.compact_slot.map(usize::from) else {
            return self
                .value_overrides
                .get(&definition_index)
                .map_or_else(|| "No override".into(), |value| format!("Override {value}"));
        };
        if !matches!(
            definition.bank(),
            ACCOUNT_OBJECTIVE_BANK | CHARACTER_OBJECTIVE_BANK
        ) {
            return "Unavailable".into();
        }
        self.values
            .get(&(definition.bank(), slot))
            .map_or_else(|| "Not listed".into(), i32::to_string)
    }
}

pub(super) fn collection_state_snapshot(document: &Value) -> Option<CollectionStateSnapshot> {
    let policy = parse(document).ok()?;
    let mut flags = HashSet::new();
    flags.extend(
        expanded_flag_slots(&policy.unlocks.account_flag_runs, ACCOUNT_FLAG_CAPACITY)
            .into_iter()
            .map(|slot| (ACCOUNT_FLAG_BANK, slot)),
    );
    flags.extend(
        expanded_flag_slots(&policy.unlocks.profile_flag_runs, PROFILE_FLAG_CAPACITY)
            .into_iter()
            .map(|slot| (PROFILE_FLAG_BANK, slot)),
    );
    flags.extend(
        expanded_flag_slots(
            &policy.unlocks.character_object_flag_runs,
            CHARACTER_OBJECT_FLAG_CAPACITY,
        )
        .into_iter()
        .map(|slot| (CHARACTER_OBJECT_FLAG_BANK, slot)),
    );
    flags.extend(
        policy
            .unlocks
            .character_flags
            .iter()
            .map(|row| (CHARACTER_FLAG_BANK, row.index)),
    );
    let values = policy
        .unlocks
        .objective_values
        .iter()
        .map(|row| ((ACCOUNT_OBJECTIVE_BANK, row.index), row.value))
        .chain(
            policy
                .unlocks
                .character_objective_values
                .iter()
                .map(|row| ((CHARACTER_OBJECTIVE_BANK, row.index), row.value)),
        )
        .collect();
    Some(CollectionStateSnapshot {
        flags,
        values,
        flag_overrides: policy
            .investment
            .flag_overrides
            .iter()
            .map(|row| (row.definition_index, row.value))
            .collect(),
        value_overrides: policy
            .investment
            .value_overrides
            .iter()
            .map(|row| (row.definition_index, row.value))
            .collect(),
    })
}

pub(super) fn collection_flag_state_text(
    state: &CollectionStateSnapshot,
    definition_index: usize,
    definition: &UnlockDefinition,
) -> String {
    state.flag_text(definition_index, definition)
}

pub(super) fn collection_value_state_text(
    state: &CollectionStateSnapshot,
    definition_index: usize,
    definition: &UnlockDefinition,
) -> String {
    state.value_text(definition_index, definition)
}

pub(super) fn validate(document: &Value) -> Result<(), String> {
    parse(document).map(|_| ())
}

fn parse(document: &Value) -> Result<Progression, String> {
    Ok(Progression {
        unlocks: parse_unlocks(document.pointer("/state/unlocks"))?,
        investment: parse_investment(document.pointer("/state/investment"))?,
    })
}

fn parse_unlocks(value: Option<&Value>) -> Result<UnlockPolicy, String> {
    let Some(object) = optional_object(value, "state.unlocks")? else {
        return Ok(UnlockPolicy::default());
    };

    Ok(UnlockPolicy {
        account_flag_runs: parse_flag_runs(
            object.get("account_flag_runs"),
            "state.unlocks.account_flag_runs",
            ACCOUNT_FLAG_CAPACITY,
        )?,
        profile_flag_runs: parse_flag_runs(
            object.get("profile_flag_runs"),
            "state.unlocks.profile_flag_runs",
            PROFILE_FLAG_CAPACITY,
        )?,
        character_flags: parse_flag_indices(
            object.get("character_flags"),
            "state.unlocks.character_flags",
            CHARACTER_FLAG_CAPACITY,
        )?,
        objective_values: parse_indexed_values(
            object.get("objective_values"),
            "state.unlocks.objective_values",
            OBJECTIVE_VALUE_CAPACITY,
        )?,
        character_object_flag_runs: parse_flag_runs(
            object.get("character_flag_runs"),
            "state.unlocks.character_flag_runs",
            CHARACTER_OBJECT_FLAG_CAPACITY,
        )?,
        character_objective_values: parse_indexed_values(
            object.get("character_objective_values"),
            "state.unlocks.character_objective_values",
            CHARACTER_OBJECT_VALUE_CAPACITY,
        )?,
        account_progressions: parse_progression_values(
            object.get("account_progressions"),
            "state.unlocks.account_progressions",
        )?,
        character_progressions: parse_progression_values(
            object.get("character_progressions"),
            "state.unlocks.character_progressions",
        )?,
    })
}

fn parse_investment(value: Option<&Value>) -> Result<InvestmentPolicy, String> {
    let Some(object) = optional_object(value, "state.investment")? else {
        return Ok(InvestmentPolicy::default());
    };

    Ok(InvestmentPolicy {
        flag_overrides: parse_flag_overrides(
            object.get("family5_flag_overrides"),
            "state.investment.family5_flag_overrides",
        )?,
        value_overrides: parse_value_overrides(
            object.get("family5_value_overrides"),
            "state.investment.family5_value_overrides",
        )?,
    })
}

fn optional_object<'a>(
    value: Option<&'a Value>,
    path: &str,
) -> Result<Option<&'a Map<String, Value>>, String> {
    value
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| format!("{path} must be an object"))
        })
        .transpose()
}

fn optional_array<'a>(value: Option<&'a Value>, path: &str) -> Result<&'a [Value], String> {
    value.map_or(Ok(&[]), |value| {
        value
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| format!("{path} must be an array"))
    })
}

fn pair<'a>(row: &'a Value, path: &str, row_index: usize) -> Result<[&'a Value; 2], String> {
    let values = row
        .as_array()
        .ok_or_else(|| format!("{path}[{row_index}] must be a two-value array"))?;
    let [first, second] = values.as_slice() else {
        return Err(format!(
            "{path}[{row_index}] must contain exactly two values"
        ));
    };
    Ok([first, second])
}

fn unsigned(value: &Value, path: &str) -> Result<usize, String> {
    let value = value
        .as_u64()
        .ok_or_else(|| format!("{path} must be an unsigned integer"))?;
    usize::try_from(value).map_err(|_| format!("{path} is too large"))
}

fn signed_32(value: &Value, path: &str) -> Result<i32, String> {
    let value = value
        .as_i64()
        .ok_or_else(|| format!("{path} must be a signed integer"))?;
    i32::try_from(value).map_err(|_| format!("{path} must fit a signed 32-bit integer"))
}

fn parse_flag_runs(
    value: Option<&Value>,
    path: &str,
    capacity: usize,
) -> Result<Vec<FlagRun>, String> {
    optional_array(value, path)?
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            let [start, length] = pair(row, path, row_index)?;
            let start = unsigned(start, &format!("{path}[{row_index}][0]"))?;
            let length = unsigned(length, &format!("{path}[{row_index}][1]"))?;
            if length == 0 {
                return Err(format!("{path}[{row_index}] must have a positive length"));
            }
            if start > capacity || length > capacity.saturating_sub(start) {
                return Err(format!(
                    "{path}[{row_index}] exceeds its {capacity}-slot bank"
                ));
            }
            Ok(FlagRun { start, length })
        })
        .collect()
}

fn parse_flag_indices(
    value: Option<&Value>,
    path: &str,
    capacity: usize,
) -> Result<Vec<FlagIndex>, String> {
    optional_array(value, path)?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let value = unsigned(value, &format!("{path}[{index}]"))?;
            if value >= capacity {
                return Err(format!(
                    "{path}[{index}] must be below the {capacity}-slot bank capacity"
                ));
            }
            Ok(FlagIndex { index: value })
        })
        .collect()
}

fn parse_indexed_values(
    value: Option<&Value>,
    path: &str,
    capacity: usize,
) -> Result<Vec<IndexedValue>, String> {
    optional_array(value, path)?
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            let [index, value] = pair(row, path, row_index)?;
            let index = unsigned(index, &format!("{path}[{row_index}][0]"))?;
            if index >= capacity {
                return Err(format!(
                    "{path}[{row_index}][0] must be below the {capacity}-slot bank capacity"
                ));
            }
            let value = signed_32(value, &format!("{path}[{row_index}][1]"))?;
            Ok(IndexedValue { index, value })
        })
        .collect()
}

fn parse_progression_values(
    value: Option<&Value>,
    path: &str,
) -> Result<Vec<ProgressionValue>, String> {
    let mut authored = vec![None::<[i32; 3]>; PROGRESSION_DEFINITION_CAPACITY];
    for (row_index, row) in optional_array(value, path)?.iter().enumerate() {
        let values = row
            .as_array()
            .ok_or_else(|| format!("{path}[{row_index}] must be a four-value array"))?;
        let [definition_index, lane_0, lane_1, lane_2] = values.as_slice() else {
            return Err(format!(
                "{path}[{row_index}] must contain exactly four values"
            ));
        };
        let definition_index = unsigned(definition_index, &format!("{path}[{row_index}][0]"))?;
        if definition_index >= PROGRESSION_DEFINITION_CAPACITY {
            return Err(format!(
                "{path}[{row_index}][0] must be below the {PROGRESSION_DEFINITION_CAPACITY}-definition capacity"
            ));
        }
        let lanes = [
            signed_32(lane_0, &format!("{path}[{row_index}][1]"))?,
            signed_32(lane_1, &format!("{path}[{row_index}][2]"))?,
            signed_32(lane_2, &format!("{path}[{row_index}][3]"))?,
        ];
        if let Some(current) = authored[definition_index].as_mut() {
            for lane in 0..3 {
                current[lane] = current[lane].max(lanes[lane]);
            }
        } else {
            authored[definition_index] = Some(lanes);
        }
    }
    Ok(authored
        .into_iter()
        .enumerate()
        .filter_map(|(definition_index, lanes)| {
            lanes.map(|lanes| ProgressionValue {
                definition_index,
                lanes,
            })
        })
        .collect())
}

fn parse_flag_overrides(value: Option<&Value>, path: &str) -> Result<Vec<FlagOverride>, String> {
    let rows = optional_array(value, path)?;
    if rows.len() > FAMILY5_OVERRIDE_CAPACITY {
        return Err(format!(
            "{path} cannot contain more than {FAMILY5_OVERRIDE_CAPACITY} rows"
        ));
    }
    rows.iter()
        .enumerate()
        .map(|(row_index, row)| {
            let [slot, value] = pair(row, path, row_index)?;
            let slot = unsigned(slot, &format!("{path}[{row_index}][0]"))?;
            if slot > FAMILY5_FLAG_SLOT_MAXIMUM {
                return Err(format!(
                    "{path}[{row_index}][0] cannot exceed {FAMILY5_FLAG_SLOT_MAXIMUM}"
                ));
            }
            let value = unsigned(value, &format!("{path}[{row_index}][1]"))?;
            let value = u8::try_from(value)
                .ok()
                .filter(|value| *value <= FAMILY5_FLAG_VALUE_MAXIMUM)
                .ok_or_else(|| {
                    format!("{path}[{row_index}][1] cannot exceed {FAMILY5_FLAG_VALUE_MAXIMUM}")
                })?;
            Ok(FlagOverride {
                definition_index: slot,
                value,
            })
        })
        .collect()
}

fn parse_value_overrides(value: Option<&Value>, path: &str) -> Result<Vec<ValueOverride>, String> {
    let rows = optional_array(value, path)?;
    if rows.len() > FAMILY5_OVERRIDE_CAPACITY {
        return Err(format!(
            "{path} cannot contain more than {FAMILY5_OVERRIDE_CAPACITY} rows"
        ));
    }
    rows.iter()
        .enumerate()
        .map(|(row_index, row)| {
            let [slot, value] = pair(row, path, row_index)?;
            let slot = unsigned(slot, &format!("{path}[{row_index}][0]"))?;
            if slot > FAMILY5_VALUE_SLOT_MAXIMUM {
                return Err(format!(
                    "{path}[{row_index}][0] cannot exceed {FAMILY5_VALUE_SLOT_MAXIMUM}"
                ));
            }
            let value = signed_32(value, &format!("{path}[{row_index}][1]"))?;
            Ok(ValueOverride {
                definition_index: slot,
                value,
            })
        })
        .collect()
}

pub(super) fn draw_content(
    ui: &mut egui::Ui,
    document: &mut Value,
    catalog: &Catalog,
    destiny_symbol_font_error: Option<&str>,
    state: &mut UiState,
    view: View,
) -> bool {
    if let Some(error) = catalog.progression_package_error() {
        ui.colored_label(ui.visuals().warn_fg_color, "Package scan incomplete")
            .on_hover_text(error);
        ui.add_space(4.0);
    }

    if let Some(error) = destiny_symbol_font_error {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "Destiny symbol fonts unavailable",
        )
        .on_hover_text(error);
        ui.add_space(4.0);
    }

    let cached = state
        .cached_progression
        .take()
        .unwrap_or_else(|| parse(document));
    let policy = match cached {
        Ok(policy) => policy,
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, "Invalid progression settings");
            ui.label(&error);
            state.cached_progression = Some(Err(error));
            return false;
        }
    };

    draw_metadata_workspace(ui, catalog, state);

    let changed = match view {
        View::Unlocks => draw_unlocks(ui, document, &policy.unlocks, catalog, state),
        View::Investment => draw_investment(ui, document, &policy.investment, catalog, state),
    };
    if let Some(hash) = take_hash_inspection_request(ui.ctx()) {
        state.hash_inspection.open(hash);
    }
    draw_catalog_hash_window(ui.ctx(), catalog, &mut state.hash_inspection);
    if !changed {
        state.cached_progression = Some(Ok(policy));
    }
    changed
}

fn draw_unlocks(
    ui: &mut egui::Ui,
    document: &mut Value,
    unlocks: &UnlockPolicy,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    let mut table_changed = false;
    progression_toolbar(ui, |ui| {
        ui.label(egui::RichText::new("Table").strong());
        let table_picker = egui::ComboBox::from_id_salt("progression_unlock_table")
            .selected_text(state.unlock_table.label())
            .width(220.0)
            .show_ui(ui, |ui| {
                for table in UnlockTable::ALL {
                    table_changed |= ui
                        .selectable_value(&mut state.unlock_table, table, table.label())
                        .changed();
                }
            });
        table_picker.response.on_hover_text(format!(
            "Settings field: {}",
            state.unlock_table.field_name()
        ));
        ui.add_space(8.0);
        draw_filter(ui, &mut state.query);
        if ui.button("+ Add").clicked() {
            state.add_open = true;
            state.add_query.clear();
            state.add_value = 0;
            state.add_progression_lanes = [0; 3];
        }
    });
    if table_changed {
        state.query.clear();
    }
    let query = state.query.clone();

    let mut changed = match state.unlock_table {
        UnlockTable::AccountFlagRuns => draw_flag_runs(
            ui,
            FlagTableConfig {
                id: "account_flag_runs",
                bank: ACCOUNT_FLAG_BANK,
                capacity: ACCOUNT_FLAG_CAPACITY,
            },
            &unlocks.account_flag_runs,
            catalog,
            &query,
            state,
            document,
        ),
        UnlockTable::ProfileFlagRuns => draw_flag_runs(
            ui,
            FlagTableConfig {
                id: "profile_flag_runs",
                bank: PROFILE_FLAG_BANK,
                capacity: PROFILE_FLAG_CAPACITY,
            },
            &unlocks.profile_flag_runs,
            catalog,
            &query,
            state,
            document,
        ),
        UnlockTable::CharacterFlags => draw_flag_indices(
            ui,
            FlagTableConfig {
                id: "character_flags",
                bank: CHARACTER_FLAG_BANK,
                capacity: CHARACTER_FLAG_CAPACITY,
            },
            &unlocks.character_flags,
            catalog,
            &query,
            state,
            document,
        ),
        UnlockTable::ObjectiveValues => draw_objective_values(
            ui,
            "objective_values",
            &unlocks.objective_values,
            ACCOUNT_OBJECTIVE_BANK,
            TableDrawContext {
                catalog,
                query: &query,
                state,
                document,
            },
        ),
        UnlockTable::CharacterObjectFlagRuns => draw_flag_runs(
            ui,
            FlagTableConfig {
                id: "character_object_flag_runs",
                bank: CHARACTER_OBJECT_FLAG_BANK,
                capacity: CHARACTER_OBJECT_FLAG_CAPACITY,
            },
            &unlocks.character_object_flag_runs,
            catalog,
            &query,
            state,
            document,
        ),
        UnlockTable::CharacterObjectObjectiveValues => draw_objective_values(
            ui,
            "character_object_objective_values",
            &unlocks.character_objective_values,
            CHARACTER_OBJECTIVE_BANK,
            TableDrawContext {
                catalog,
                query: &query,
                state,
                document,
            },
        ),
        UnlockTable::AccountProgressions => draw_progression_values(
            ui,
            "account_progressions",
            &unlocks.account_progressions,
            ProgressionScope::Account,
            catalog,
            &query,
            state,
            document,
        ),
        UnlockTable::CharacterProgressions => draw_progression_values(
            ui,
            "character_progressions",
            &unlocks.character_progressions,
            ProgressionScope::Character,
            catalog,
            &query,
            state,
            document,
        ),
    };
    changed |= draw_add_unlock_window(ui.ctx(), document, unlocks, catalog, state);
    changed
}

fn draw_investment(
    ui: &mut egui::Ui,
    document: &mut Value,
    investment: &InvestmentPolicy,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    let mut table_changed = false;
    let mut undo_requested = false;
    let row_count = match state.investment_table {
        InvestmentTable::FlagOverrides => investment.flag_overrides.len(),
        InvestmentTable::ValueOverrides => investment.value_overrides.len(),
    };
    let can_add = row_count < FAMILY5_OVERRIDE_CAPACITY;
    progression_toolbar(ui, |ui| {
        ui.label(egui::RichText::new("Table").strong());
        let table_picker = egui::ComboBox::from_id_salt("progression_investment_table")
            .selected_text(state.investment_table.label())
            .width(220.0)
            .show_ui(ui, |ui| {
                for table in InvestmentTable::ALL {
                    table_changed |= ui
                        .selectable_value(&mut state.investment_table, table, table.label())
                        .changed();
                }
            });
        table_picker.response.on_hover_text(format!(
            "Settings field: {}",
            state.investment_table.field_name()
        ));
        ui.add_space(8.0);
        draw_filter(ui, &mut state.query);
        egui::ComboBox::from_id_salt("progression_override_coverage")
            .selected_text(state.override_filter.label())
            .width(170.0)
            .show_ui(ui, |ui| {
                for filter in OverrideFilter::ALL {
                    ui.selectable_value(&mut state.override_filter, filter, filter.label());
                }
            });
        let add = ui.add_enabled(can_add, egui::Button::new("+ Add"));
        let add = if can_add {
            add
        } else {
            add.on_disabled_hover_text("100-row settings limit")
        };
        if add.clicked() {
            state.add_open = true;
            state.add_query.clear();
            state.add_value = 1;
        }
        if let Some(last_change) = state.last_investment_change {
            if ui
                .button("Undo last override change")
                .on_hover_text(last_change.label())
                .clicked()
            {
                undo_requested = true;
            }
        }
    });
    if table_changed {
        state.query.clear();
        state.add_open = false;
    }
    if !can_add {
        state.add_open = false;
    }
    ui.add_space(4.0);
    ui.label(state.investment_table.explanation());
    ui.add_space(4.0);
    let query = state.query.clone();

    let mut changed = undo_requested && undo_investment_change(document, state);
    changed |= match state.investment_table {
        InvestmentTable::FlagOverrides => draw_flag_overrides(
            ui,
            &investment.flag_overrides,
            catalog,
            &query,
            state,
            document,
        ),
        InvestmentTable::ValueOverrides => draw_value_overrides(
            ui,
            &investment.value_overrides,
            catalog,
            &query,
            state,
            document,
        ),
    };
    changed |= draw_add_investment_window(ui.ctx(), document, investment, catalog, state);
    changed
}

fn draw_filter(ui: &mut egui::Ui, query: &mut String) {
    ui.add(
        egui::TextEdit::singleline(query)
            .hint_text("Filter rows…")
            .desired_width(300.0),
    );
}

#[derive(Clone, Copy)]
struct AddTableSpec {
    id: &'static str,
    bank: u8,
    capacity: usize,
    value: bool,
}

fn add_table_spec(table: UnlockTable) -> AddTableSpec {
    match table {
        UnlockTable::AccountFlagRuns => AddTableSpec {
            id: "account_flag_runs",
            bank: ACCOUNT_FLAG_BANK,
            capacity: ACCOUNT_FLAG_CAPACITY,
            value: false,
        },
        UnlockTable::ProfileFlagRuns => AddTableSpec {
            id: "profile_flag_runs",
            bank: PROFILE_FLAG_BANK,
            capacity: PROFILE_FLAG_CAPACITY,
            value: false,
        },
        UnlockTable::CharacterFlags => AddTableSpec {
            id: "character_flags",
            bank: CHARACTER_FLAG_BANK,
            capacity: CHARACTER_FLAG_CAPACITY,
            value: false,
        },
        UnlockTable::ObjectiveValues => AddTableSpec {
            id: "objective_values",
            bank: ACCOUNT_OBJECTIVE_BANK,
            capacity: OBJECTIVE_VALUE_CAPACITY,
            value: true,
        },
        UnlockTable::CharacterObjectFlagRuns => AddTableSpec {
            id: "character_object_flag_runs",
            bank: CHARACTER_OBJECT_FLAG_BANK,
            capacity: CHARACTER_OBJECT_FLAG_CAPACITY,
            value: false,
        },
        UnlockTable::CharacterObjectObjectiveValues => AddTableSpec {
            id: "character_object_objective_values",
            bank: CHARACTER_OBJECTIVE_BANK,
            capacity: CHARACTER_OBJECT_VALUE_CAPACITY,
            value: true,
        },
        UnlockTable::AccountProgressions | UnlockTable::CharacterProgressions => {
            unreachable!("progression tables use their package definition picker")
        }
    }
}

fn draw_add_unlock_window(
    ctx: &egui::Context,
    document: &mut Value,
    unlocks: &UnlockPolicy,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    if !state.add_open {
        return false;
    }
    if matches!(
        state.unlock_table,
        UnlockTable::AccountProgressions | UnlockTable::CharacterProgressions
    ) {
        return draw_add_progression_window(ctx, document, unlocks, catalog, state);
    }
    let spec = add_table_spec(state.unlock_table);
    let occupied = occupied_slots(unlocks, state.unlock_table, spec.capacity);
    let definitions = if spec.value {
        catalog.unlock_value_definitions()
    } else {
        catalog.unlock_flag_definitions()
    };
    let query = state.add_query.trim().to_lowercase();
    let candidates =
        definitions
            .iter()
            .enumerate()
            .filter_map(|(index, definition)| {
                let slot = usize::from(definition.compact_slot?);
                (definition.bank() == spec.bank
                    && slot < spec.capacity
                    && !occupied.get(slot).copied().unwrap_or(false)
                    && (query.is_empty()
                        || definition_matches(&query, index, definition)
                        || slot.to_string().contains(&query)
                        || (spec.value
                            && catalog.objective_for_unlock_value(index).is_some_and(
                                |objective| resolved_objective_matches(catalog, &query, objective),
                            ))))
                .then_some((index, slot, definition))
            })
            .collect::<Vec<_>>();

    let mut open = state.add_open;
    let mut selection = None;
    egui::Window::new(format!("Add {}", state.unlock_table.label()))
        .id(egui::Id::new("progression_add_unlock"))
        .open(&mut open)
        .collapsible(false)
        .default_width(560.0)
        .default_height(480.0)
        .show(ctx, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.add_query)
                    .hint_text("Filter package definitions…")
                    .desired_width(f32::INFINITY),
            );
            if spec.value {
                ui.horizontal(|ui| {
                    ui.label("Initial value");
                    ui.add(
                        egui::DragValue::new(&mut state.add_value)
                            .speed(1.0)
                            .range(i32::MIN..=i32::MAX),
                    );
                });
            }
            ui.label(egui::RichText::new(format!("{} available", candidates.len())).weak());
            ui.separator();
            if candidates.is_empty() {
                ui.label(egui::RichText::new("No matching definitions").weak());
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("progression_add_unlock_rows")
                .auto_shrink([false, false])
                .show_rows(ui, 34.0, candidates.len(), |ui, range| {
                    for row in range {
                        let (definition_index, slot, definition) = candidates[row];
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 30.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                let button_width = 44.0;
                                let slot_width = 62.0;
                                let label_width = (ui.available_width()
                                    - button_width
                                    - slot_width
                                    - ui.spacing().item_spacing.x * 2.0)
                                    .max(120.0);
                                let label = add_definition_label(
                                    catalog,
                                    definition,
                                    if spec.value {
                                        catalog.objective_for_unlock_value(definition_index)
                                    } else {
                                        None
                                    },
                                );
                                ui.add_sized(
                                    [label_width, 24.0],
                                    egui::Label::new(label).truncate(),
                                )
                                .on_hover_text(
                                    add_definition_tooltip(
                                        definition_index,
                                        definition,
                                        if spec.value {
                                            catalog.objective_for_unlock_value(definition_index)
                                        } else {
                                            None
                                        },
                                    ),
                                );
                                ui.add_sized(
                                    [slot_width, 24.0],
                                    egui::Label::new(
                                        egui::RichText::new(format!("Slot {slot}")).monospace(),
                                    ),
                                );
                                if ui.small_button("Add").clicked() {
                                    selection = Some(slot);
                                }
                            },
                        );
                    }
                });
        });
    state.add_open = open;
    let Some(slot) = selection else {
        return false;
    };
    state.add_open = false;
    if spec.value {
        set_unlock_value(document, spec.id, slot, state.add_value)
    } else {
        set_unlock_flag(document, spec.id, slot, true)
    }
}

fn draw_add_progression_window(
    ctx: &egui::Context,
    document: &mut Value,
    unlocks: &UnlockPolicy,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    let (id, scope, rows) = match state.unlock_table {
        UnlockTable::AccountProgressions => (
            "account_progressions",
            ProgressionScope::Account,
            &unlocks.account_progressions,
        ),
        UnlockTable::CharacterProgressions => (
            "character_progressions",
            ProgressionScope::Character,
            &unlocks.character_progressions,
        ),
        _ => return false,
    };
    let occupied = rows
        .iter()
        .map(|row| row.definition_index)
        .collect::<HashSet<_>>();
    let query = state.add_query.trim().to_lowercase();
    let candidates = catalog
        .progression_definitions()
        .iter()
        .filter(|definition| {
            definition.scope == scope
                && !occupied.contains(&usize::from(definition.definition_index))
                && (query.is_empty()
                    || definition.definition_index.to_string().contains(&query)
                    || definition
                        .scope_slot
                        .is_some_and(|slot| slot.to_string().contains(&query)))
        })
        .collect::<Vec<_>>();
    let mut open = state.add_open;
    let mut selection = None;
    egui::Window::new(format!("Add {}", state.unlock_table.label()))
        .id(egui::Id::new("progression_add_progression"))
        .open(&mut open)
        .collapsible(false)
        .default_width(520.0)
        .default_height(440.0)
        .show(ctx, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.add_query)
                    .hint_text("Filter progression definitions…")
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                for lane in 0..3 {
                    ui.label(format!("Lane {lane}"));
                    ui.add(
                        egui::DragValue::new(&mut state.add_progression_lanes[lane])
                            .speed(1.0)
                            .range(i32::MIN..=i32::MAX),
                    );
                }
            });
            ui.label(egui::RichText::new(format!("{} available", candidates.len())).weak());
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("progression_add_progression_rows")
                .show_rows(ui, 30.0, candidates.len(), |ui, range| {
                    for row in range {
                        let definition = candidates[row];
                        ui.horizontal(|ui| {
                            ui.monospace(format!("#{}", definition.definition_index));
                            ui.label(definition.scope_slot.map_or_else(
                                || "<unreplicated>".into(),
                                |slot| format!("Slot {slot}"),
                            ));
                            if ui.small_button("Add").clicked() {
                                selection = Some(usize::from(definition.definition_index));
                            }
                        });
                    }
                });
        });
    state.add_open = open;
    let Some(definition_index) = selection else {
        return false;
    };
    state.add_open = false;
    set_progression_value(document, id, definition_index, state.add_progression_lanes)
}

fn occupied_slots(unlocks: &UnlockPolicy, table: UnlockTable, capacity: usize) -> Vec<bool> {
    let mut occupied = vec![false; capacity];
    let slots = match table {
        UnlockTable::AccountFlagRuns => expanded_flag_slots(&unlocks.account_flag_runs, capacity),
        UnlockTable::ProfileFlagRuns => expanded_flag_slots(&unlocks.profile_flag_runs, capacity),
        UnlockTable::CharacterFlags => unlocks
            .character_flags
            .iter()
            .map(|row| row.index)
            .collect(),
        UnlockTable::ObjectiveValues => unlocks
            .objective_values
            .iter()
            .map(|row| row.index)
            .collect(),
        UnlockTable::CharacterObjectFlagRuns => {
            expanded_flag_slots(&unlocks.character_object_flag_runs, capacity)
        }
        UnlockTable::CharacterObjectObjectiveValues => unlocks
            .character_objective_values
            .iter()
            .map(|row| row.index)
            .collect(),
        UnlockTable::AccountProgressions => unlocks
            .account_progressions
            .iter()
            .map(|row| row.definition_index)
            .collect(),
        UnlockTable::CharacterProgressions => unlocks
            .character_progressions
            .iter()
            .map(|row| row.definition_index)
            .collect(),
    };
    for slot in slots {
        if let Some(value) = occupied.get_mut(slot) {
            *value = true;
        }
    }
    occupied
}

fn add_definition_label(
    catalog: &Catalog,
    definition: &UnlockDefinition,
    objective: Option<&ObjectiveDef>,
) -> String {
    if let Some(name) =
        definition_name(definition).or_else(|| catalog.display_name(definition.hash))
    {
        return name.to_owned();
    }
    if let Some(objective) = objective {
        return resolved_objective_table_text(catalog, objective, Some(definition));
    }
    if let Some(context) = definition_context_lines(definition).first() {
        return context.text();
    }
    definition_hash(definition)
}

fn add_definition_tooltip(
    definition_index: usize,
    definition: &UnlockDefinition,
    objective: Option<&ObjectiveDef>,
) -> String {
    let mut lines = vec![definition_identity(definition_index, definition)];
    if let Some(name) = definition_name(definition) {
        lines.push(format!("Name: {name}"));
    }
    if let Some(objective) = objective {
        lines.push(format!("Objective: {}", objective_description(objective)));
        lines.push(format!(
            "Completion value: {}",
            objective_target_text(objective)
        ));
    }
    lines.push(definition_metadata_tooltip(definition));
    lines.join("\n")
}

fn draw_add_investment_window(
    ctx: &egui::Context,
    document: &mut Value,
    investment: &InvestmentPolicy,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    if !state.add_open {
        return false;
    }
    let is_value = state.investment_table == InvestmentTable::ValueOverrides;
    let definitions = if is_value {
        catalog.unlock_value_definitions()
    } else {
        catalog.unlock_flag_definitions()
    };
    let occupied = if is_value {
        investment
            .value_overrides
            .iter()
            .map(|row| row.definition_index)
            .collect::<HashSet<_>>()
    } else {
        investment
            .flag_overrides
            .iter()
            .map(|row| row.definition_index)
            .collect::<HashSet<_>>()
    };
    let query = state.add_query.trim().to_lowercase();
    let candidates =
        definitions
            .iter()
            .enumerate()
            .filter(|(index, definition)| {
                !occupied.contains(index)
                    && (query.is_empty()
                        || definition_matches(&query, *index, definition)
                        || (is_value
                            && catalog.objective_for_unlock_value(*index).is_some_and(
                                |objective| resolved_objective_matches(catalog, &query, objective),
                            )))
            })
            .collect::<Vec<_>>();

    let mut open = state.add_open;
    let mut selection = None;
    egui::Window::new(format!("Add {}", state.investment_table.label()))
        .id(egui::Id::new("progression_add_investment"))
        .open(&mut open)
        .collapsible(false)
        .default_width(560.0)
        .default_height(480.0)
        .show(ctx, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.add_query)
                    .hint_text("Filter package definitions…")
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                ui.label(if is_value {
                    "Value"
                } else {
                    "Logical flag value"
                });
                let drag = egui::DragValue::new(&mut state.add_value).speed(1.0);
                ui.add(if is_value {
                    drag.range(i32::MIN..=i32::MAX)
                } else {
                    drag.range(0..=i32::from(FAMILY5_FLAG_VALUE_MAXIMUM))
                });
            });
            ui.label(egui::RichText::new(format!("{} available", candidates.len())).weak());
            ui.separator();
            if candidates.is_empty() {
                ui.label(egui::RichText::new("No matching definitions").weak());
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("progression_add_investment_rows")
                .auto_shrink([false, false])
                .show_rows(ui, 34.0, candidates.len(), |ui, range| {
                    for row in range {
                        let (definition_index, definition) = candidates[row];
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 30.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                let button_width = 44.0;
                                let index_width = 70.0;
                                let label_width = (ui.available_width()
                                    - button_width
                                    - index_width
                                    - ui.spacing().item_spacing.x * 2.0)
                                    .max(120.0);
                                let objective = if is_value {
                                    catalog.objective_for_unlock_value(definition_index)
                                } else {
                                    None
                                };
                                ui.add_sized(
                                    [label_width, 24.0],
                                    egui::Label::new(add_definition_label(
                                        catalog, definition, objective,
                                    ))
                                    .truncate(),
                                )
                                .on_hover_text(
                                    add_definition_tooltip(definition_index, definition, objective),
                                );
                                ui.add_sized(
                                    [index_width, 24.0],
                                    egui::Label::new(
                                        egui::RichText::new(format!("#{definition_index}"))
                                            .monospace(),
                                    ),
                                );
                                if ui.small_button("Add").clicked() {
                                    selection = Some(definition_index);
                                }
                            },
                        );
                    }
                });
        });
    state.add_open = open;
    let Some(definition_index) = selection else {
        return false;
    };
    state.add_open = false;
    let changed = set_investment_override(
        document,
        state.investment_table,
        definition_index,
        state.add_value,
    );
    if changed {
        state.last_investment_change = Some(match state.investment_table {
            InvestmentTable::FlagOverrides => InvestmentUndo::Flag {
                definition_index,
                previous: None,
            },
            InvestmentTable::ValueOverrides => InvestmentUndo::Value {
                definition_index,
                previous: None,
            },
        });
    }
    changed
}

#[derive(Clone, Copy)]
struct FlagTableConfig {
    id: &'static str,
    bank: u8,
    capacity: usize,
}

struct TableDrawContext<'a> {
    catalog: &'a Catalog,
    query: &'a str,
    state: &'a mut UiState,
    document: &'a mut Value,
}

fn draw_flag_runs(
    ui: &mut egui::Ui,
    config: FlagTableConfig,
    rows: &[FlagRun],
    catalog: &Catalog,
    query: &str,
    state: &mut UiState,
    document: &mut Value,
) -> bool {
    let slots = expanded_flag_slots(rows, config.capacity);
    draw_flag_slots(
        ui,
        config,
        &slots,
        Some(rows.len()),
        TableDrawContext {
            catalog,
            query,
            state,
            document,
        },
    )
}

fn draw_flag_indices(
    ui: &mut egui::Ui,
    config: FlagTableConfig,
    rows: &[FlagIndex],
    catalog: &Catalog,
    query: &str,
    state: &mut UiState,
    document: &mut Value,
) -> bool {
    let mut present = vec![false; config.capacity];
    for row in rows {
        present[row.index] = true;
    }
    let slots = present
        .into_iter()
        .enumerate()
        .filter_map(|(slot, present)| present.then_some(slot))
        .collect::<Vec<_>>();
    draw_flag_slots(
        ui,
        config,
        &slots,
        None,
        TableDrawContext {
            catalog,
            query,
            state,
            document,
        },
    )
}

fn draw_flag_slots(
    ui: &mut egui::Ui,
    config: FlagTableConfig,
    slots: &[usize],
    encoded_range_count: Option<usize>,
    context: TableDrawContext<'_>,
) -> bool {
    let TableDrawContext {
        catalog,
        query,
        state,
        document,
    } = context;
    let mut changed = false;
    let query = query.trim().to_lowercase();
    let mut filtered = slots
        .iter()
        .copied()
        .filter(|slot| flag_slot_matches(&query, *slot, config.bank, catalog))
        .collect::<Vec<_>>();
    let mapped = slots
        .iter()
        .filter(|slot| catalog.unlock_flag_for_state(config.bank, **slot).is_some())
        .count();
    let mut summary = vec![format!("{} flags", slots.len())];
    if mapped < slots.len() {
        summary.push(format!("{} unmapped", slots.len() - mapped));
    }
    let summary = ui
        .label(summary.join(" · "))
        .on_hover_text("Definition match: bank + compact slot");
    if let Some(count) = encoded_range_count {
        summary.on_hover_text(format!("Settings field: {count} flag runs"));
    }

    let index_width = 96.0;
    let hash_width = 104.0;
    let state_width = 64.0;
    let tested_by_width = (ui.available_width()
        - index_width
        - hash_width
        - state_width
        - TABLE_ACTION_WIDTH
        - TABLE_COLUMN_GAP * 4.0)
        .max(150.0);
    ui.add_space(4.0);
    let sort = sortable_table_header(
        ui,
        config.id,
        &[
            (index_width, "Index"),
            (hash_width, "Hash"),
            (tested_by_width, "Readers"),
            (state_width, "Slot"),
            (TABLE_ACTION_WIDTH, ""),
        ],
        TableSort::ascending(3),
        state,
    );
    ui.separator();
    if filtered.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return false;
    }

    match sort.column {
        2 => sort_by_optional_cached_key(&mut filtered, sort.descending, |slot| {
            catalog
                .unlock_flag_for_state(config.bank, *slot)
                .and_then(|(_, definition)| definition_context_sort_key(definition))
        }),
        _ => filtered
            .sort_by(|left, right| compare_flag_slots(*left, *right, config.bank, catalog, sort)),
    }
    let display_lines = definition_context_display_lines(
        filtered.len(),
        |row_index| catalog.unlock_flag_for_state(config.bank, filtered[row_index]),
        sort.column == 2 && sort.descending,
    );

    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("progression_table", config.id))
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(TABLE_ROW_STRIDE * 3.0))
            .show_rows(ui, TABLE_CELL_HEIGHT, display_lines.len(), |ui, range| {
                egui::Grid::new((config.id, "rows"))
                    .num_columns(5)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for line_index in range {
                            let line = &display_lines[line_index];
                            let slot = filtered[line.row_index];
                            if line.primary {
                                if let (Some(definition_index), Some(definition)) =
                                    (line.definition_index, line.definition)
                                {
                                    draw_definition_index_cell(
                                        ui,
                                        index_width,
                                        definition_index,
                                        Some(definition),
                                        MetadataSelection::FlagDefinition(definition_index),
                                        state,
                                    );
                                    draw_definition_hash_cell(ui, hash_width, Some(definition));
                                } else {
                                    table_cell(ui, index_width, egui::RichText::new("—").weak())
                                        .on_hover_text("No package definition");
                                    table_cell(ui, hash_width, egui::RichText::new("—").weak());
                                }
                            } else {
                                table_cell(ui, index_width, "");
                                table_cell(ui, hash_width, "");
                            }
                            draw_context_cell(ui, tested_by_width, line.context.as_ref());
                            if line.primary {
                                table_cell(
                                    ui,
                                    state_width,
                                    egui::RichText::new(slot.to_string()).monospace(),
                                );
                                if draw_remove_cell(ui, TABLE_ACTION_WIDTH, "Remove state entry")
                                    .clicked()
                                    && set_unlock_flag(document, config.id, slot, false)
                                {
                                    changed = true;
                                }
                            } else {
                                table_cell(ui, state_width, "");
                                table_cell(ui, TABLE_ACTION_WIDTH, "");
                            }
                            ui.end_row();
                        }
                    });
            });
    });
    changed
}

fn draw_objective_values(
    ui: &mut egui::Ui,
    id: &'static str,
    rows: &[IndexedValue],
    bank: u8,
    context: TableDrawContext<'_>,
) -> bool {
    let TableDrawContext {
        catalog,
        query,
        state,
        document,
    } = context;
    let mut changed = false;
    let query = query.trim().to_lowercase();
    let filtered = rows
        .iter()
        .filter(|row| objective_hierarchy_row_matches(&query, row, bank, catalog))
        .collect::<Vec<_>>();
    let objectives = rows
        .iter()
        .filter_map(|row| catalog.unlock_value_for_state(bank, row.index))
        .filter(|(definition_index, _)| {
            catalog
                .objective_for_unlock_value(*definition_index)
                .is_some()
        })
        .count();
    let mut summary = vec![format!("{} objective values", rows.len())];
    if objectives < rows.len() {
        summary.push(format!(
            "{} without a resolved objective",
            rows.len() - objectives
        ));
    }
    ui.label(summary.join(" · "))
        .on_hover_text("Definition: bank + compact slot\nHierarchy: package owner paths");
    let state_width = 72.0;
    let value_width = 76.0;
    let index_width = 118.0;
    let hash_width = 104.0;
    let objective_width = (ui.available_width()
        - index_width
        - hash_width
        - state_width
        - value_width
        - TABLE_ACTION_WIDTH
        - TABLE_COLUMN_GAP * 5.0)
        .max(150.0);
    ui.add_space(4.0);
    let sort = sortable_table_header(
        ui,
        id,
        &[
            (objective_width, "Objective"),
            (index_width, "Index"),
            (hash_width, "Hash"),
            (value_width, "Value"),
            (state_width, "Objective index"),
            (TABLE_ACTION_WIDTH, ""),
        ],
        TableSort::ascending(0),
        state,
    );
    ui.separator();
    if filtered.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return false;
    }

    let mut hierarchy = build_objective_hierarchy(&filtered, bank, catalog);
    sort_objective_hierarchy(&mut hierarchy, sort);
    let auto_expand = !query.is_empty();
    let display_lines = objective_matrix_lines(&hierarchy, id, state, auto_expand);

    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("progression_table", id))
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(TABLE_ROW_STRIDE * 3.0))
            .show_rows(ui, TABLE_CELL_HEIGHT, display_lines.len(), |ui, range| {
                egui::Grid::new((id, "rows"))
                    .num_columns(6)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for line_index in range {
                            match display_lines[line_index] {
                                ObjectiveMatrixLine::Branch {
                                    branch,
                                    depth,
                                    expanded,
                                } => {
                                    let response = draw_hierarchy_branch_cell(
                                        ui,
                                        objective_width,
                                        depth,
                                        &branch.label,
                                        expanded,
                                        !auto_expand,
                                    );
                                    let response = response.on_hover_text(branch.path.join(" > "));
                                    if !auto_expand && response.clicked() {
                                        state.objective_expansion.insert(
                                            ObjectiveBranchKey {
                                                table: id,
                                                path: branch.path.clone(),
                                            },
                                            !expanded,
                                        );
                                    }
                                    table_cell(ui, index_width, "");
                                    table_cell(ui, hash_width, "");
                                    table_cell(ui, value_width, "");
                                    table_cell(ui, state_width, "");
                                    table_cell(ui, TABLE_ACTION_WIDTH, "");
                                }
                                ObjectiveMatrixLine::Leaf { leaf, depth } => {
                                    if let Some(objective) = leaf.objective {
                                        let response = draw_hierarchy_leaf_cell(
                                            ui,
                                            objective_width,
                                            depth,
                                            resolved_objective_table_text(
                                                catalog,
                                                objective,
                                                leaf.definition,
                                            ),
                                        );
                                        if let Some(definition_index) = leaf.definition_index {
                                            if metadata_click(response, "Open objective definition")
                                                .on_hover_text(objective_details_tooltip(objective))
                                                .clicked()
                                            {
                                                state.open_metadata(
                                                    MetadataSelection::ValueDefinition(
                                                        definition_index,
                                                    ),
                                                );
                                            }
                                        } else {
                                            response.on_hover_text(objective_details_tooltip(
                                                objective,
                                            ));
                                        }
                                    } else {
                                        let response = draw_hierarchy_leaf_cell(
                                            ui,
                                            objective_width,
                                            depth,
                                            egui::RichText::new("—").weak(),
                                        );
                                        let response =
                                            response.on_hover_text(if leaf.definition.is_some() {
                                                "No same-hash objective"
                                            } else {
                                                "No package definition"
                                            });
                                        if let Some(definition_index) = leaf.definition_index
                                            && metadata_click(response, "Open objective definition")
                                                .clicked()
                                        {
                                            state.open_metadata(
                                                MetadataSelection::ValueDefinition(
                                                    definition_index,
                                                ),
                                            );
                                        }
                                    }
                                    let objective_index_text = leaf.objective_index.map_or_else(
                                        || egui::RichText::new("—").weak(),
                                        |objective_index| {
                                            egui::RichText::new(format!("#{objective_index}"))
                                                .monospace()
                                        },
                                    );
                                    let objective_index_response =
                                        if leaf.definition_index.is_some()
                                            && leaf.objective_index.is_some()
                                        {
                                            table_link(ui, index_width, objective_index_text)
                                                .on_hover_text("Open objective definition")
                                        } else {
                                            table_cell(ui, index_width, objective_index_text)
                                        };
                                    if let Some(definition_index) = leaf.definition_index
                                        && objective_index_response.clicked()
                                    {
                                        state.open_metadata(MetadataSelection::ValueDefinition(
                                            definition_index,
                                        ));
                                    }
                                    draw_hash_cell(
                                        ui,
                                        hash_width,
                                        leaf.objective.map(|objective| objective.hash),
                                    );
                                    let row = leaf.row;
                                    let mut value = row.value;
                                    if table_drag_value(ui, value_width, &mut value).changed()
                                        && set_unlock_value(document, id, row.index, value)
                                    {
                                        changed = true;
                                    }
                                    table_cell(
                                        ui,
                                        state_width,
                                        egui::RichText::new(row.index.to_string()).monospace(),
                                    );
                                    if draw_remove_cell(
                                        ui,
                                        TABLE_ACTION_WIDTH,
                                        "Remove objective value",
                                    )
                                    .clicked()
                                        && remove_unlock_value(document, id, row.index)
                                    {
                                        changed = true;
                                    }
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
    });
    changed
}

#[allow(clippy::too_many_arguments)]
fn draw_progression_values(
    ui: &mut egui::Ui,
    id: &'static str,
    rows: &[ProgressionValue],
    scope: ProgressionScope,
    catalog: &Catalog,
    query: &str,
    state: &mut UiState,
    document: &mut Value,
) -> bool {
    let query = query.trim().to_lowercase();
    let mut filtered = rows
        .iter()
        .copied()
        .filter(|row| {
            let definition = catalog.progression_definition(row.definition_index);
            query.is_empty()
                || row.definition_index.to_string().contains(&query)
                || row
                    .lanes
                    .iter()
                    .any(|lane| lane.to_string().contains(&query))
                || definition
                    .and_then(|definition| definition.scope_slot)
                    .is_some_and(|slot| slot.to_string().contains(&query))
        })
        .collect::<Vec<_>>();
    let definition_width = 118.0;
    let slot_width = 72.0;
    let lane_width = 92.0;
    let sort = sortable_table_header(
        ui,
        id,
        &[
            (definition_width, "Index"),
            (slot_width, "Slot"),
            (lane_width, "Lane 0"),
            (lane_width, "Lane 1"),
            (lane_width, "Lane 2"),
            (TABLE_ACTION_WIDTH, ""),
        ],
        TableSort::ascending(0),
        state,
    );
    filtered.sort_by(|left, right| {
        let left_definition = catalog.progression_definition(left.definition_index);
        let right_definition = catalog.progression_definition(right.definition_index);
        let order = match sort.column {
            0 => left.definition_index.cmp(&right.definition_index),
            1 => left_definition
                .and_then(|definition| definition.scope_slot)
                .cmp(&right_definition.and_then(|definition| definition.scope_slot)),
            2..=4 => left.lanes[sort.column - 2].cmp(&right.lanes[sort.column - 2]),
            _ => Ordering::Equal,
        };
        if sort.descending {
            order.reverse()
        } else {
            order
        }
    });
    ui.separator();
    if filtered.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return false;
    }
    let mut changed = false;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("progression_table", id))
            .auto_shrink([false, false])
            .show_rows(ui, TABLE_CELL_HEIGHT, filtered.len(), |ui, range| {
                egui::Grid::new((id, "rows"))
                    .num_columns(6)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for row_index in range {
                            let row = filtered[row_index];
                            let definition = catalog.progression_definition(row.definition_index);
                            table_cell(
                                ui,
                                definition_width,
                                egui::RichText::new(format!("#{}", row.definition_index))
                                    .monospace(),
                            );
                            let scope_slot = definition
                                .filter(|definition| definition.scope == scope)
                                .and_then(|definition| definition.scope_slot);
                            table_cell(
                                ui,
                                slot_width,
                                egui::RichText::new(
                                    scope_slot.map_or_else(|| "—".into(), |slot| slot.to_string()),
                                )
                                .monospace(),
                            );
                            let mut lanes = row.lanes;
                            let mut row_changed = false;
                            for lane in &mut lanes {
                                row_changed |= table_drag_value(ui, lane_width, lane).changed();
                            }
                            if row_changed {
                                changed |= set_progression_value(
                                    document,
                                    id,
                                    row.definition_index,
                                    lanes,
                                );
                            }
                            if draw_remove_cell(ui, TABLE_ACTION_WIDTH, "Remove progression")
                                .clicked()
                            {
                                changed |=
                                    remove_progression_value(document, id, row.definition_index);
                            }
                            ui.end_row();
                        }
                    });
            });
    });
    changed
}

fn draw_flag_overrides(
    ui: &mut egui::Ui,
    rows: &[FlagOverride],
    catalog: &Catalog,
    query: &str,
    state: &mut UiState,
    document: &mut Value,
) -> bool {
    let mut changed = false;
    let query = query.trim().to_lowercase();
    let mut filtered = rows
        .iter()
        .filter(|row| {
            let definition = catalog.unlock_flag_definition(row.definition_index);
            family5_flag_matches(&query, row, catalog)
                && override_filter_matches(state.override_filter, definition)
        })
        .collect::<Vec<_>>();
    let mapped = rows
        .iter()
        .filter(|row| {
            catalog
                .unlock_flag_definition(row.definition_index)
                .is_some()
        })
        .count();
    let mut summary = vec![format!("{} flag overrides", rows.len())];
    if mapped < rows.len() {
        summary.push(format!("{} unmapped", rows.len() - mapped));
    }
    let unresolved_readers = rows
        .iter()
        .filter_map(|row| catalog.unlock_flag_definition(row.definition_index))
        .filter(|definition| definition.tested_by.is_empty())
        .count();
    let partially_decoded = rows
        .iter()
        .filter_map(|row| catalog.unlock_flag_definition(row.definition_index))
        .filter(|definition| definition_has_undecoded_opcodes(definition))
        .count();
    draw_override_coverage_summary(
        ui,
        &summary.join(" · "),
        unresolved_readers,
        partially_decoded,
        "Each row stores a package definition index and the logical state used by account-wide progression checks.",
    );

    let index_width = 96.0;
    let hash_width = 104.0;
    let value_width = 124.0;
    let action_width = TABLE_ACTION_WIDTH;
    let meaning_width = (ui.available_width()
        - index_width
        - hash_width
        - value_width
        - action_width
        - TABLE_COLUMN_GAP * 4.0)
        .max(180.0);
    ui.add_space(4.0);
    let sort = sortable_table_header(
        ui,
        "family5_flag_overrides",
        &[
            (index_width, "Index"),
            (hash_width, "Hash"),
            (value_width, "Logical flag value"),
            (meaning_width, "Readers"),
            (action_width, ""),
        ],
        TableSort::ascending(0),
        state,
    );
    ui.separator();
    if filtered.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return false;
    }
    filtered.sort_by(|left, right| compare_flag_overrides(left, right, catalog, sort));
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("progression_table", "family5_flag_overrides"))
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(TABLE_ROW_STRIDE * 3.0))
            .show_rows(ui, TABLE_CELL_HEIGHT, filtered.len(), |ui, range| {
                egui::Grid::new("family5_flag_override_rows")
                    .num_columns(5)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for row_index in range {
                            let row = filtered[row_index];
                            let definition = catalog.unlock_flag_definition(row.definition_index);
                            draw_definition_index_cell(
                                ui,
                                index_width,
                                row.definition_index,
                                definition,
                                MetadataSelection::FlagOverride(row.definition_index, row.value),
                                state,
                            );
                            draw_definition_hash_cell(ui, hash_width, definition);
                            let mut value = row.value;
                            let prior_value = value;
                            ui.allocate_ui_with_layout(
                                egui::vec2(value_width, TABLE_CELL_HEIGHT),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    egui::ComboBox::from_id_salt((
                                        "family5_flag_override_value",
                                        row.definition_index,
                                    ))
                                    .selected_text(flag_override_state_label(value))
                                    .width(value_width - 12.0)
                                    .show_ui(ui, |ui| {
                                        for candidate in 0..=FAMILY5_FLAG_VALUE_MAXIMUM {
                                            ui.selectable_value(
                                                &mut value,
                                                candidate,
                                                flag_override_state_label(candidate),
                                            );
                                        }
                                    })
                                    .response
                                    .on_hover_text(flag_override_state_help());
                                },
                            );
                            if value != prior_value
                                && set_investment_override(
                                    document,
                                    InvestmentTable::FlagOverrides,
                                    row.definition_index,
                                    i32::from(value),
                                )
                            {
                                state.last_investment_change = Some(InvestmentUndo::Flag {
                                    definition_index: row.definition_index,
                                    previous: Some(prior_value),
                                });
                                changed = true;
                            }
                            draw_override_meaning(
                                ui,
                                meaning_width,
                                definition,
                                MetadataSelection::FlagOverride(row.definition_index, row.value),
                                state,
                            );
                            if draw_remove_cell(ui, action_width, "Remove flag override").clicked()
                                && remove_investment_override(
                                    document,
                                    InvestmentTable::FlagOverrides,
                                    row.definition_index,
                                )
                            {
                                state.last_investment_change = Some(InvestmentUndo::Flag {
                                    definition_index: row.definition_index,
                                    previous: Some(row.value),
                                });
                                changed = true;
                            }
                            ui.end_row();
                        }
                    });
            });
    });
    changed
}

fn draw_value_overrides(
    ui: &mut egui::Ui,
    rows: &[ValueOverride],
    catalog: &Catalog,
    query: &str,
    state: &mut UiState,
    document: &mut Value,
) -> bool {
    let mut changed = false;
    let query = query.trim().to_lowercase();
    let mut filtered = rows
        .iter()
        .filter(|row| {
            let definition = catalog.unlock_value_definition(row.definition_index);
            family5_value_matches(&query, row, catalog)
                && override_filter_matches(state.override_filter, definition)
        })
        .collect::<Vec<_>>();
    let mapped = rows
        .iter()
        .filter(|row| {
            catalog
                .unlock_value_definition(row.definition_index)
                .is_some()
        })
        .count();
    let mut summary = vec![format!("{} value overrides", rows.len())];
    if mapped < rows.len() {
        summary.push(format!("{} unmapped", rows.len() - mapped));
    }
    let unresolved_readers = rows
        .iter()
        .filter_map(|row| catalog.unlock_value_definition(row.definition_index))
        .filter(|definition| definition.tested_by.is_empty())
        .count();
    let partially_decoded = rows
        .iter()
        .filter_map(|row| catalog.unlock_value_definition(row.definition_index))
        .filter(|definition| definition_has_undecoded_opcodes(definition))
        .count();
    draw_override_coverage_summary(
        ui,
        &summary.join(" · "),
        unresolved_readers,
        partially_decoded,
        "Each row stores a package definition index and the signed number used by account-wide progression checks.",
    );

    let index_width = 96.0;
    let hash_width = 104.0;
    let value_width = 110.0;
    let action_width = TABLE_ACTION_WIDTH;
    let meaning_width = (ui.available_width()
        - index_width
        - hash_width
        - value_width
        - action_width
        - TABLE_COLUMN_GAP * 4.0)
        .max(180.0);
    ui.add_space(4.0);
    let sort = sortable_table_header(
        ui,
        "family5_value_overrides",
        &[
            (index_width, "Index"),
            (hash_width, "Hash"),
            (value_width, "Value"),
            (meaning_width, "Readers"),
            (action_width, ""),
        ],
        TableSort::ascending(0),
        state,
    );
    ui.separator();
    if filtered.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return false;
    }
    filtered.sort_by(|left, right| compare_value_overrides(left, right, catalog, sort));
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("progression_table", "family5_value_overrides"))
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(TABLE_ROW_STRIDE * 3.0))
            .show_rows(ui, TABLE_CELL_HEIGHT, filtered.len(), |ui, range| {
                egui::Grid::new("family5_value_override_rows")
                    .num_columns(5)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for row_index in range {
                            let row = filtered[row_index];
                            let definition = catalog.unlock_value_definition(row.definition_index);
                            let selection =
                                MetadataSelection::ValueOverride(row.definition_index, row.value);
                            draw_definition_index_cell(
                                ui,
                                index_width,
                                row.definition_index,
                                definition,
                                selection,
                                state,
                            );
                            draw_definition_hash_cell(ui, hash_width, definition);
                            let mut value = row.value;
                            let prior_value = value;
                            if table_drag_value(ui, value_width, &mut value).changed()
                                && set_investment_override(
                                    document,
                                    InvestmentTable::ValueOverrides,
                                    row.definition_index,
                                    value,
                                )
                            {
                                state.last_investment_change = Some(InvestmentUndo::Value {
                                    definition_index: row.definition_index,
                                    previous: Some(prior_value),
                                });
                                changed = true;
                            }
                            draw_override_meaning(ui, meaning_width, definition, selection, state);
                            if draw_remove_cell(ui, action_width, "Remove value override").clicked()
                                && remove_investment_override(
                                    document,
                                    InvestmentTable::ValueOverrides,
                                    row.definition_index,
                                )
                            {
                                state.last_investment_change = Some(InvestmentUndo::Value {
                                    definition_index: row.definition_index,
                                    previous: Some(row.value),
                                });
                                changed = true;
                            }
                            ui.end_row();
                        }
                    });
            });
    });
    changed
}

fn draw_override_meaning(
    ui: &mut egui::Ui,
    width: f32,
    definition: Option<&UnlockDefinition>,
    selection: MetadataSelection,
    state: &mut UiState,
) {
    let Some(definition) = definition else {
        table_cell(
            ui,
            width,
            egui::RichText::new("Not in package table").weak(),
        );
        return;
    };
    let meaning = override_meaning(definition);
    let text = if override_meaning_contexts(definition).is_empty()
        && definition_name(definition).is_none()
    {
        egui::RichText::new(meaning).weak().underline()
    } else {
        egui::RichText::new(meaning).underline()
    };
    let response = table_link(ui, width, text).on_hover_text(format!(
        "{} reader{}",
        definition.tested_by.len(),
        if definition.tested_by.len() == 1 {
            ""
        } else {
            "s"
        }
    ));
    if response.clicked() {
        state.open_metadata(selection);
    }
}

fn draw_override_coverage_summary(
    ui: &mut egui::Ui,
    summary: &str,
    unresolved_readers: usize,
    partially_decoded: usize,
    tooltip: &str,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(summary).on_hover_text(tooltip);
        if unresolved_readers > 0 {
            ui.label(
                egui::RichText::new(format!("· {unresolved_readers} with no resolved reader"))
                    .weak(),
            )
            .on_hover_text("No package reader relationship was found for this definition.");
        }
        if partially_decoded > 0 {
            ui.label(
                egui::RichText::new(format!("· {partially_decoded} partially decoded")).weak(),
            )
            .on_hover_text("One or more condition programs contain undecoded opcodes.");
        }
    });
}

fn sortable_table_header(
    ui: &mut egui::Ui,
    id: &'static str,
    columns: &[(f32, &str)],
    default: TableSort,
    state: &mut UiState,
) -> TableSort {
    let mut sort = state.table_sorts.get(id).copied().unwrap_or(default);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TABLE_COLUMN_GAP;
        for (column, (width, label)) in columns.iter().enumerate() {
            if label.is_empty() {
                ui.allocate_space(egui::vec2(*width, TABLE_CELL_HEIGHT));
                continue;
            }
            let marker = if sort.column == column {
                if sort.descending {
                    Some(Glyph::ChevronDown)
                } else {
                    Some(Glyph::ChevronUp)
                }
            } else {
                None
            };
            let response = sortable_header_cell(ui, *width, label, marker).on_hover_text("Sort");
            if response.clicked() {
                if sort.column == column {
                    sort.descending = !sort.descending;
                } else {
                    sort = TableSort::ascending(column);
                }
            }
        }
    });
    state.table_sorts.insert(id, sort);
    sort
}

fn table_link(ui: &mut egui::Ui, width: f32, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.add(
                egui::Button::new(text)
                    .frame(false)
                    .truncate()
                    .min_size(egui::vec2(width, TABLE_CELL_HEIGHT)),
            )
        },
    )
    .inner
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn table_drag_value(ui: &mut egui::Ui, width: f32, value: &mut i32) -> egui::Response {
    table_drag_value_ranged(ui, width, value, i32::MIN..=i32::MAX)
}

fn table_drag_value_ranged(
    ui: &mut egui::Ui,
    width: f32,
    value: &mut i32,
    range: std::ops::RangeInclusive<i32>,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.add(egui::DragValue::new(value).speed(1.0).range(range))
        },
    )
    .inner
}

const fn flag_override_state_label(value: u8) -> &'static str {
    match value {
        0 => "0 · clear",
        1 => "1 · logical value 1",
        2 => "2 · set",
        _ => "Invalid",
    }
}

const fn flag_override_state_help() -> &'static str {
    "Sunrise logical unlock-flag value: 0 clear, 1 logical value 1, 2 set."
}

fn draw_definition_index_cell(
    ui: &mut egui::Ui,
    width: f32,
    definition_index: usize,
    definition: Option<&UnlockDefinition>,
    metadata_selection: MetadataSelection,
    state: &mut UiState,
) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            let label = egui::RichText::new(format!("#{definition_index}")).monospace();
            if let Some(definition) = definition {
                let response = ui
                    .add(
                        egui::Button::new(label)
                            .frame(false)
                            .truncate()
                            .min_size(egui::vec2(width, TABLE_CELL_HEIGHT)),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(definition_metadata_tooltip(definition));
                if response.clicked() {
                    state.open_metadata(metadata_selection);
                }
            } else {
                ui.add(egui::Label::new(label.weak()).truncate());
            }
        },
    );
}

fn draw_definition_hash_cell(ui: &mut egui::Ui, width: f32, definition: Option<&UnlockDefinition>) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            if let Some(definition) = definition {
                draw_hash_link(ui, definition.hash, definition_hash(definition));
            } else {
                ui.label(egui::RichText::new("—").weak());
            }
        },
    );
}

fn draw_hash_cell(ui: &mut egui::Ui, width: f32, hash: Option<u64>) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            if let Some(hash) = hash.filter(|hash| *hash != 0) {
                draw_hash_link(ui, hash, format!("0x{hash:08X}"));
            } else {
                ui.label(egui::RichText::new("—").weak());
            }
        },
    );
}

fn draw_remove_cell(ui: &mut egui::Ui, width: f32, accessible_label: &str) -> egui::Response {
    let cell = ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| super::item_editor::draw_trash_button(ui, true, accessible_label),
    );
    cell.inner
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(accessible_label)
}

fn definition_hash(definition: &UnlockDefinition) -> String {
    format!("0x{:08X}", definition.hash)
}

fn definition_name(definition: &UnlockDefinition) -> Option<&str> {
    definition
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
}

fn definition_identity(index: usize, definition: &UnlockDefinition) -> String {
    format!("#{index}: {}", definition_hash(definition))
}

fn definition_name_tooltip(definition: &UnlockDefinition) -> Option<String> {
    let mut lines = Vec::new();
    if let Some(name) = definition_name(definition) {
        lines.push(format!("Name: {name}"));
    }
    if let Some(description) = definition
        .description
        .as_deref()
        .filter(|description| !description.trim().is_empty())
    {
        lines.push(format!("Description: {description}"));
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn definition_metadata_tooltip(definition: &UnlockDefinition) -> String {
    let mut lines: Vec<String> = definition_name_tooltip(definition)
        .map(|tooltip| tooltip.lines().map(str::to_owned).collect())
        .unwrap_or_default();
    lines.push(format!("Code: 0x{:04X}", definition.code));
    if let Some(slot) = definition.compact_slot {
        lines.push(format!("Compact slot: {slot}"));
    }
    lines.join("\n")
}

fn metadata_click(response: egui::Response, accessible_label: &'static str) -> egui::Response {
    let response = response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, accessible_label)
    });
    response
}

fn draw_metadata_workspace(ui: &mut egui::Ui, catalog: &Catalog, state: &mut UiState) {
    if state.metadata_selection.is_none() {
        return;
    }
    if !state.hash_inspection.is_open()
        && ui.ctx().input(|input| input.key_pressed(egui::Key::Escape))
    {
        state.close_metadata();
        return;
    }

    if ui.available_width() >= 980.0 {
        egui::SidePanel::right("progression_inspection_workspace")
            .resizable(true)
            .default_width(540.0)
            .width_range(420.0..=760.0)
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show_inside(ui, |ui| draw_metadata_panel(ui, catalog, state));
    } else {
        let maximum_height = (ui.available_height() * 0.7).max(260.0);
        egui::TopBottomPanel::bottom("progression_inspection_workspace_compact")
            .resizable(true)
            .default_height(maximum_height.min(380.0))
            .height_range(240.0..=maximum_height)
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show_inside(ui, |ui| draw_metadata_panel(ui, catalog, state));
    }
}

fn draw_metadata_panel(ui: &mut egui::Ui, catalog: &Catalog, state: &mut UiState) {
    let Some(selection) = state.metadata_selection else {
        return;
    };
    let (title, definition, objectives) = match selection {
        MetadataSelection::FlagDefinition(index) | MetadataSelection::FlagOverride(index, _) => (
            format!("Unlock flag definition #{index}"),
            catalog.unlock_flag_definition(index),
            Vec::new(),
        ),
        MetadataSelection::ValueDefinition(index) | MetadataSelection::ValueOverride(index, _) => (
            format!("Unlock value definition #{index}"),
            catalog.unlock_value_definition(index),
            catalog.objectives_for_unlock_value(index),
        ),
    };
    let semantic_name = definition
        .and_then(definition_name)
        .map(str::to_owned)
        .or_else(|| {
            objectives.first().and_then(|objective| {
                (!objective.name.trim().is_empty())
                    .then(|| objective.name.trim().to_owned())
                    .or_else(|| {
                        preferred_objective_owner(objective).and_then(objective_owner_display_label)
                    })
            })
        });
    let title = semantic_name.map_or(title.clone(), |name| format!("{title} · {name}"));
    let mut navigate_back = false;
    let close = inspector_heading(ui, title);
    ui.add_space(6.0);
    if let Some(previous) = state.metadata_history.last().copied() {
        let previous_label = metadata_selection_short_label(previous, catalog);
        if back_button(ui, &previous_label).clicked() {
            navigate_back = true;
        }
    }
    ui.separator();
    egui::ScrollArea::both()
        .id_salt("progression_metadata_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            match definition {
                Some(definition) => {
                    let index = selection.definition_index();
                    if matches!(
                        selection,
                        MetadataSelection::FlagOverride(_, _)
                            | MetadataSelection::ValueOverride(_, _)
                    ) {
                        metadata_section(ui, "Account override", |ui| {
                            draw_override_metadata(ui, selection, definition);
                        });
                        ui.add_space(8.0);
                    }
                    metadata_section(ui, "Unlock definition", |ui| {
                        draw_unlock_definition_metadata(ui, index, definition, catalog, state);
                    });
                    for (objective_index, objective) in objectives.iter().enumerate() {
                        ui.add_space(8.0);
                        let heading = if objectives.len() == 1 {
                            "Related objective".to_owned()
                        } else {
                            format!("Related objective {}", objective_index + 1)
                        };
                        metadata_section(ui, &heading, |ui| {
                            draw_objective_metadata(ui, objective, definition, catalog, state);
                        });
                    }
                    if objectives.is_empty() && selection.is_value() {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("No related objective definition").weak());
                    }
                }
                None => {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "Definition index is not present in the scanned package table",
                    );
                }
            }
        });
    if navigate_back {
        state.metadata_back();
    } else if close {
        state.close_metadata();
    }
}

fn metadata_selection_short_label(selection: MetadataSelection, catalog: &Catalog) -> String {
    let (kind, definition) = if selection.is_value() {
        (
            "Value",
            catalog.unlock_value_definition(selection.definition_index()),
        )
    } else {
        (
            "Flag",
            catalog.unlock_flag_definition(selection.definition_index()),
        )
    };
    let index = selection.definition_index();
    definition
        .and_then(|definition| {
            definition_name(definition).or_else(|| catalog.display_name(definition.hash))
        })
        .map_or_else(
            || format!("{kind} #{index}"),
            |name| format!("{kind} #{index} · {name}"),
        )
}

#[derive(Debug, Default)]
pub(super) struct HashInspectionState {
    current: Option<u64>,
    history: Vec<u64>,
}

impl HashInspectionState {
    pub(super) fn open(&mut self, hash: u64) {
        if hash == 0 || self.current == Some(hash) {
            return;
        }
        if let Some(current) = self.current {
            self.history.push(current);
        }
        self.current = Some(hash);
    }

    pub(super) const fn is_open(&self) -> bool {
        self.current.is_some()
    }

    fn back(&mut self) {
        if let Some(previous) = self.history.pop() {
            self.current = Some(previous);
        }
    }

    pub(super) fn close(&mut self) {
        self.current = None;
        self.history.clear();
    }
}

struct CatalogHashMatches<'a> {
    flag_definitions: Vec<(usize, &'a UnlockDefinition)>,
    value_definitions: Vec<(usize, &'a UnlockDefinition)>,
    objectives: Vec<(usize, &'a ObjectiveDef)>,
    owner_matches: Vec<(usize, &'a ObjectiveDef, &'a ObjectiveOwnerDef)>,
    trait_matches: Vec<(
        usize,
        &'a ObjectiveDef,
        &'a ObjectiveOwnerDef,
        &'a ObjectiveOwnerTraitDef,
    )>,
    context_matches: Vec<(&'static str, usize, &'a ProgressionContextDef)>,
    collectible_matches: Vec<&'a CollectibleDef>,
    material_requirement_set_matches: Vec<&'a MaterialRequirementSetDef>,
    bucket_items: Vec<&'a ItemDef>,
    item: Option<&'a ItemDef>,
    inventory_metadata: Option<&'a InventoryMetadata>,
    item_material_requirement_set_indices: Option<ItemMaterialRequirementSetIndices>,
}

impl<'a> CatalogHashMatches<'a> {
    fn collect(catalog: &'a Catalog, hash: u64) -> Self {
        let flag_definitions = catalog
            .unlock_flag_definitions()
            .iter()
            .enumerate()
            .filter(|(_, definition)| definition.hash == hash)
            .collect();
        let value_definitions = catalog
            .unlock_value_definitions()
            .iter()
            .enumerate()
            .filter(|(_, definition)| definition.hash == hash)
            .collect();
        let objectives = catalog
            .objectives()
            .iter()
            .enumerate()
            .filter(|(_, objective)| objective.hash == hash)
            .collect();
        let owner_matches = catalog
            .objectives()
            .iter()
            .enumerate()
            .flat_map(|(objective_index, objective)| {
                objective
                    .owners
                    .iter()
                    .filter(move |owner| owner.hash == hash)
                    .map(move |owner| (objective_index, objective, owner))
            })
            .collect();
        let trait_matches = catalog
            .objectives()
            .iter()
            .enumerate()
            .flat_map(|(objective_index, objective)| {
                objective.owners.iter().flat_map(move |owner| {
                    owner
                        .traits
                        .iter()
                        .filter(move |trait_definition| trait_definition.hash == hash)
                        .map(move |trait_definition| {
                            (objective_index, objective, owner, trait_definition)
                        })
                })
            })
            .collect();
        let context_matches = catalog
            .unlock_flag_definitions()
            .iter()
            .enumerate()
            .flat_map(|(index, definition)| {
                definition
                    .tested_by
                    .iter()
                    .filter(move |context| context.hash == hash)
                    .map(move |context| ("Flag", index, context))
            })
            .chain(
                catalog
                    .unlock_value_definitions()
                    .iter()
                    .enumerate()
                    .flat_map(|(index, definition)| {
                        definition
                            .tested_by
                            .iter()
                            .filter(move |context| context.hash == hash)
                            .map(move |context| ("Value", index, context))
                    }),
            )
            .collect();
        let collectible_matches = catalog
            .collectibles()
            .iter()
            .filter(|definition| {
                definition.hash == hash
                    || definition.item_hash == hash
                    || definition.material_requirement_set_hash == hash
                    || definition
                        .material_requirements
                        .iter()
                        .any(|requirement| requirement.item_hash == hash)
            })
            .collect();
        let material_requirement_set_matches = catalog
            .material_requirement_sets()
            .iter()
            .filter(|set| {
                set.hash == hash
                    || set
                        .requirements
                        .iter()
                        .any(|requirement| requirement.item_hash == hash)
            })
            .collect();
        Self {
            flag_definitions,
            value_definitions,
            objectives,
            owner_matches,
            trait_matches,
            context_matches,
            collectible_matches,
            material_requirement_set_matches,
            bucket_items: catalog.items_for_bucket(hash).collect(),
            item: catalog.item(hash),
            inventory_metadata: catalog.inventory_metadata(hash),
            item_material_requirement_set_indices: catalog
                .item_material_requirement_set_indices(hash),
        }
    }

    fn count(&self) -> usize {
        usize::from(self.item.is_some())
            + usize::from(self.inventory_metadata.is_some())
            + self.flag_definitions.len()
            + self.value_definitions.len()
            + self.objectives.len()
            + self.owner_matches.len()
            + self.trait_matches.len()
            + self.context_matches.len()
            + self.collectible_matches.len()
            + self.material_requirement_set_matches.len()
            + usize::from(!self.bucket_items.is_empty())
    }
}

pub(super) fn draw_catalog_hash_window(
    ctx: &egui::Context,
    catalog: &Catalog,
    hash_inspection: &mut HashInspectionState,
) {
    let Some(hash) = hash_inspection.current else {
        return;
    };
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        hash_inspection.close();
        return;
    }

    let matches = CatalogHashMatches::collect(catalog, hash);
    let match_count = matches.count();

    let mut open = true;
    let screen_size = ctx.screen_rect().size();
    let maximum_size = egui::vec2(
        (screen_size.x - 48.0).max(360.0),
        (screen_size.y - 64.0).max(280.0),
    );
    let minimum_size = egui::vec2(maximum_size.x.min(520.0), maximum_size.y.min(320.0));
    let default_size = egui::vec2(maximum_size.x.min(720.0), maximum_size.y.min(560.0));
    let resolved_name = catalog.display_name(hash).map(str::to_owned).or_else(|| {
        matches
            .bucket_items
            .iter()
            .find_map(|item| catalog.inventory_metadata(item.hash))
            .map(|metadata| metadata.bucket_label())
    });
    let title = resolved_name.as_deref().map_or_else(
        || format!("Definition hash 0x{hash:08X}"),
        |name| format!("Definition hash 0x{hash:08X} — {name}"),
    );
    let mut navigate_back = false;
    egui::Window::new(title)
        .id(egui::Id::new("catalog_hash_metadata"))
        .open(&mut open)
        .order(egui::Order::Foreground)
        .collapsible(false)
        .movable(true)
        .resizable(true)
        .default_size(default_size)
        .min_size(minimum_size)
        .max_size(maximum_size)
        .show(ctx, |ui| {
            if let Some(previous) = hash_inspection.history.last().copied() {
                let previous_label = catalog.display_name(previous).map_or_else(
                    || format!("0x{previous:08X}"),
                    |name| format!("0x{previous:08X} · {name}"),
                );
                if back_button(ui, &previous_label)
                    .on_hover_text("Return to the previous definition hash")
                    .clicked()
                {
                    navigate_back = true;
                }
                ui.separator();
            }
            egui::ScrollArea::vertical()
                .id_salt(("catalog_hash_metadata_scroll", hash))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("catalog_hash_identity")
                        .num_columns(2)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            hash_detail_field(ui, "Definition hash", format!("0x{hash:08X}"), true);
                        });

                    draw_hash_item_matches(ui, catalog, hash, &resolved_name, &matches);
                    draw_hash_progression_matches(ui, catalog, &matches);
                    draw_hash_collection_matches(ui, catalog, hash, &matches);
                    if match_count == 0 {
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(
                                "No directly indexed package entity uses this hash.",
                            )
                            .weak(),
                        );
                    }
                });
        });
    if navigate_back {
        hash_inspection.back();
    } else if !open {
        hash_inspection.close();
    }
}

fn draw_hash_item_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    resolved_name: &Option<String>,
    matches: &CatalogHashMatches<'_>,
) {
    let item = matches.item;
    let inventory_metadata = matches.inventory_metadata;
    let bucket_items = &matches.bucket_items;
    let item_material_requirement_set_indices = matches.item_material_requirement_set_indices;
    let flag_definitions = &matches.flag_definitions;
    let value_definitions = &matches.value_definitions;

    if let Some(item) = item {
        ui.add_space(8.0);
        hash_metadata_section(ui, "Inventory item", true, |ui| {
            egui::Grid::new("hash_inventory_item")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    hash_detail_field(
                        ui,
                        "Name",
                        if item.name.trim().is_empty() {
                            resolved_name.as_deref().unwrap_or("<not resolved>")
                        } else {
                            &item.name
                        },
                        false,
                    );
                    hash_detail_field(ui, "Type", metadata_text(&item.type_name), false);
                    hash_detail_field(ui, "Bucket hash", metadata_hash(item.bucket_hash), true);
                    hash_detail_field(ui, "Class type", item.class_type.to_string(), true);
                    hash_detail_field(ui, "Sockets", item.sockets.len().to_string(), true);
                    hash_detail_field(
                        ui,
                        "Default plugs",
                        item.default_plugs.len().to_string(),
                        true,
                    );
                    if let Some(indices) = item_material_requirement_set_indices {
                        draw_item_material_requirement_set_link(
                            ui,
                            catalog,
                            "Insertion material requirement set",
                            indices.insertion,
                        );
                        draw_item_material_requirement_set_link(
                            ui,
                            catalog,
                            "Enabled material requirement set",
                            indices.enabled,
                        );
                    }
                });
            draw_hash_item_sockets(ui, catalog, item);
            draw_hash_item_abilities(ui, item);
        });
    }
    if let Some(metadata) = inventory_metadata {
        ui.add_space(8.0);
        hash_metadata_section(ui, "Inventory metadata", true, |ui| {
            egui::Grid::new("hash_inventory_metadata")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    hash_detail_field(ui, "Scope", metadata.scope.label(), false);
                    hash_detail_field(
                        ui,
                        "Native bucket",
                        metadata.native_bucket_id.to_string(),
                        true,
                    );
                    hash_detail_field(ui, "Stackability", metadata.stackability.label(), false);
                    hash_detail_field(
                        ui,
                        "Maximum stack",
                        metadata
                            .max_stack_size
                            .map_or_else(|| "<none>".into(), |value| value.to_string()),
                        true,
                    );
                    hash_detail_field(
                        ui,
                        "Bucket capacity",
                        metadata
                            .bucket_capacity
                            .map_or_else(|| "<none>".into(), |value| value.to_string()),
                        true,
                    );
                });
        });
    }

    if !bucket_items.is_empty() {
        draw_hash_inventory_bucket(ui, catalog, hash, bucket_items);
    }

    draw_hash_unlock_matches(ui, catalog, "Unlock flag definitions", flag_definitions);
    draw_hash_unlock_matches(ui, catalog, "Unlock value definitions", value_definitions);
}

fn draw_hash_progression_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    matches: &CatalogHashMatches<'_>,
) {
    let objectives = &matches.objectives;
    let owner_matches = &matches.owner_matches;
    let trait_matches = &matches.trait_matches;
    let context_matches = &matches.context_matches;

    if !objectives.is_empty() {
        ui.add_space(8.0);
        hash_metadata_section(
            ui,
            &format!("Objectives ({})", objectives.len()),
            objectives.len() <= 3,
            |ui| {
                for (index, objective) in objectives {
                    metadata_subsection(ui, &format!("Objective #{index}"), |ui| {
                        egui::Grid::new(("hash_objective", *index))
                            .num_columns(2)
                            .spacing([16.0, 4.0])
                            .show(ui, |ui| {
                                hash_detail_field(
                                    ui,
                                    "Name",
                                    if objective.name.trim().is_empty() {
                                        catalog
                                            .display_name(objective.hash)
                                            .unwrap_or("<not resolved>")
                                    } else {
                                        &objective.name
                                    },
                                    false,
                                );
                                hash_detail_field(
                                    ui,
                                    "Description",
                                    objective_description(objective),
                                    false,
                                );
                                hash_detail_field(
                                    ui,
                                    "Completion value",
                                    objective.completion_value.to_string(),
                                    true,
                                );
                                hash_detail_field(
                                    ui,
                                    "Owners",
                                    objective.owners.len().to_string(),
                                    true,
                                );
                            });
                        draw_hash_condition_programs(
                            ui,
                            egui::Id::new(("hash_objective_conditions", *index, objective.hash)),
                            &objective.condition_programs,
                            catalog,
                        );
                    });
                }
            },
        );
    }

    if !owner_matches.is_empty() {
        ui.add_space(8.0);
        hash_metadata_section(
            ui,
            &format!("Objective owners ({})", owner_matches.len()),
            false,
            |ui| {
                for (objective_index, objective, owner) in owner_matches {
                    metadata_subsection(ui, &format!("Objective #{objective_index}"), |ui| {
                        egui::Grid::new(("hash_objective_owner", *objective_index, owner.hash))
                            .num_columns(2)
                            .spacing([16.0, 4.0])
                            .show(ui, |ui| {
                                hash_detail_field(
                                    ui,
                                    "Objective",
                                    objective_description(objective),
                                    false,
                                );
                                hash_detail_field(
                                    ui,
                                    "Kind",
                                    objective_owner_kind_label(owner.kind),
                                    false,
                                );
                                hash_detail_field(
                                    ui,
                                    "Name",
                                    if owner.name.trim().is_empty() {
                                        catalog.display_name(owner.hash).unwrap_or("<not resolved>")
                                    } else {
                                        &owner.name
                                    },
                                    false,
                                );
                                hash_detail_field(
                                    ui,
                                    "Type",
                                    metadata_text(&owner.type_name),
                                    false,
                                );
                                hash_detail_field(
                                    ui,
                                    "Traits",
                                    owner.traits.len().to_string(),
                                    true,
                                );
                            });
                    });
                }
            },
        );
    }

    if !trait_matches.is_empty() {
        ui.add_space(8.0);
        hash_metadata_section(
            ui,
            &format!("Objective traits ({})", trait_matches.len()),
            false,
            |ui| {
                for (objective_index, objective, owner, trait_definition) in trait_matches {
                    metadata_subsection(ui, &format!("Objective #{objective_index}"), |ui| {
                        egui::Grid::new((
                            "hash_objective_trait",
                            *objective_index,
                            trait_definition.hash,
                        ))
                        .num_columns(2)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            hash_detail_field(
                                ui,
                                "Objective",
                                objective_description(objective),
                                false,
                            );
                            hash_detail_field(
                                ui,
                                "Owner",
                                objective_owner_display_label(owner)
                                    .or_else(|| catalog.display_name(owner.hash).map(str::to_owned))
                                    .unwrap_or_else(|| "<not resolved>".into()),
                                false,
                            );
                            hash_detail_field(
                                ui,
                                "Name",
                                if trait_definition.name.trim().is_empty() {
                                    catalog
                                        .display_name(trait_definition.hash)
                                        .unwrap_or("<not resolved>")
                                } else {
                                    &trait_definition.name
                                },
                                false,
                            );
                            hash_detail_field(
                                ui,
                                "Description",
                                metadata_text(&trait_definition.description),
                                false,
                            );
                        });
                    });
                }
            },
        );
    }

    if !context_matches.is_empty() {
        ui.add_space(8.0);
        hash_metadata_section(
            ui,
            &format!("Progression readers ({})", context_matches.len()),
            false,
            |ui| {
                for (kind, definition_index, context) in context_matches {
                    metadata_subsection(
                        ui,
                        &format!("{kind} definition #{definition_index}"),
                        |ui| {
                            egui::Grid::new((
                                "hash_context",
                                *kind,
                                *definition_index,
                                context.hash,
                            ))
                            .num_columns(2)
                            .spacing([16.0, 4.0])
                            .show(ui, |ui| {
                                hash_detail_field(
                                    ui,
                                    "Reader kind",
                                    progression_context_kind_label(context.kind),
                                    false,
                                );
                                hash_detail_field(
                                    ui,
                                    "Name",
                                    if context.name.trim().is_empty() {
                                        catalog
                                            .display_name(context.hash)
                                            .unwrap_or("<not resolved>")
                                    } else {
                                        &context.name
                                    },
                                    false,
                                );
                                hash_detail_field(
                                    ui,
                                    "Type",
                                    metadata_text(&context.type_name),
                                    false,
                                );
                                hash_detail_field(
                                    ui,
                                    "Description",
                                    metadata_text(&context.description),
                                    false,
                                );
                            });
                            let detail_id = egui::Id::new((
                                "hash_context_detail",
                                *kind,
                                *definition_index,
                                context.hash,
                            ));
                            draw_hash_package_paths(ui, detail_id, &context.paths);
                            draw_hash_condition_programs(
                                ui,
                                detail_id,
                                &context.condition_programs,
                                catalog,
                            );
                        },
                    );
                }
            },
        );
    }
}

fn draw_hash_collection_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    matches: &CatalogHashMatches<'_>,
) {
    let collectible_matches = &matches.collectible_matches;
    let material_requirement_set_matches = &matches.material_requirement_set_matches;

    if !collectible_matches.is_empty() {
        ui.add_space(8.0);
        hash_metadata_section(
            ui,
            &format!("Collectibles ({})", collectible_matches.len()),
            collectible_matches.len() <= 3,
            |ui| {
                for collectible in collectible_matches {
                    metadata_subsection(ui, &format!("Collectible #{}", collectible.index), |ui| {
                        egui::Grid::new(("hash_collectible", collectible.index))
                            .num_columns(2)
                            .spacing([16.0, 4.0])
                            .show(ui, |ui| {
                                let mut matched_as = Vec::new();
                                if collectible.hash == hash {
                                    matched_as.push("Collectible hash");
                                }
                                if collectible.item_hash == hash {
                                    matched_as.push("Definition hash");
                                }
                                if collectible.material_requirement_set_hash == hash {
                                    matched_as.push("Material requirement set hash");
                                }
                                if collectible
                                    .material_requirements
                                    .iter()
                                    .any(|requirement| requirement.item_hash == hash)
                                {
                                    matched_as.push("Material requirement definition hash");
                                }
                                hash_detail_field(ui, "Matched as", matched_as.join(" · "), false);
                                hash_detail_field(
                                    ui,
                                    "Collectible index",
                                    collectible.index.to_string(),
                                    true,
                                );
                                hash_detail_field(
                                    ui,
                                    "Collectible hash",
                                    metadata_hash(collectible.hash),
                                    true,
                                );
                                hash_detail_field(
                                    ui,
                                    "Item definition index",
                                    if collectible.item_definition_index == u16::MAX {
                                        "<unavailable>".into()
                                    } else {
                                        collectible.item_definition_index.to_string()
                                    },
                                    true,
                                );
                                hash_detail_field(
                                    ui,
                                    "Item definition hash",
                                    metadata_hash(collectible.item_hash),
                                    true,
                                );
                                hash_detail_field(
                                    ui,
                                    "Material requirement set index",
                                    collectible.material_requirement_set_index.map_or_else(
                                        || "<unavailable>".into(),
                                        |index| index.to_string(),
                                    ),
                                    true,
                                );
                                hash_detail_field(
                                    ui,
                                    "Material requirement set hash",
                                    metadata_hash(collectible.material_requirement_set_hash),
                                    true,
                                );
                                hash_detail_field(
                                    ui,
                                    "Name",
                                    if collectible.name.trim().is_empty() {
                                        catalog.display_name(hash).unwrap_or("<not resolved>")
                                    } else {
                                        &collectible.name
                                    },
                                    false,
                                );
                                hash_detail_field(
                                    ui,
                                    "Type",
                                    metadata_text(&collectible.type_name),
                                    false,
                                );
                            });
                        let detail_id = egui::Id::new((
                            "hash_collectible_detail",
                            collectible.index,
                            collectible.hash,
                        ));
                        draw_hash_package_paths(ui, detail_id, &collectible.paths);
                        draw_hash_collection_conditions(
                            ui,
                            detail_id,
                            &collectible.conditions,
                            catalog,
                        );
                        draw_hash_material_requirements(
                            ui,
                            detail_id,
                            &collectible.material_requirements,
                        );
                    });
                }
            },
        );
    }

    if !material_requirement_set_matches.is_empty() {
        ui.add_space(8.0);
        hash_metadata_section(
            ui,
            &format!(
                "Material requirement sets ({})",
                material_requirement_set_matches.len()
            ),
            material_requirement_set_matches.len() <= 3,
            |ui| {
                for set in material_requirement_set_matches {
                    draw_hash_material_requirement_set(ui, set, hash);
                }
            },
        );
    }
}

fn draw_hash_package_paths(ui: &mut egui::Ui, id: egui::Id, paths: &[Vec<String>]) {
    if paths.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("Package paths ({})", paths.len()))
        .id_salt((id, "package_paths"))
        .default_open(false)
        .show(ui, |ui| {
            for (index, path) in paths.iter().enumerate() {
                ui.label(
                    egui::RichText::new(format!("{}. {}", index + 1, metadata_path_text(path)))
                        .monospace(),
                );
            }
        });
}

fn draw_hash_condition_programs(
    ui: &mut egui::Ui,
    id: egui::Id,
    programs: &[Vec<[u32; 2]>],
    catalog: &Catalog,
) {
    if programs.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("Condition programs ({})", programs.len()))
        .id_salt((id, "condition_programs"))
        .default_open(false)
        .show(ui, |ui| {
            for (program_index, program) in programs.iter().enumerate() {
                draw_hash_condition_tokens(ui, (id, program_index), program, catalog);
            }
        });
}

fn draw_hash_collection_conditions(
    ui: &mut egui::Ui,
    id: egui::Id,
    conditions: &[CollectionConditionDef],
    catalog: &Catalog,
) {
    if conditions.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("Conditions ({})", conditions.len()))
        .id_salt((id, "collection_conditions"))
        .default_open(false)
        .show(ui, |ui| {
            for (condition_index, condition) in conditions.iter().enumerate() {
                ui.label(
                    egui::RichText::new(if condition.field == 3 {
                        "Acquisition (field 3)".to_owned()
                    } else {
                        format!("Field {}", condition.field)
                    })
                    .strong(),
                );
                let program = condition
                    .tokens
                    .iter()
                    .map(|token| [token.kind, token.operand])
                    .collect::<Vec<_>>();
                draw_hash_condition_tokens(ui, (id, condition_index), &program, catalog);
            }
        });
}

fn draw_hash_condition_tokens(
    ui: &mut egui::Ui,
    id: (egui::Id, usize),
    program: &[[u32; 2]],
    catalog: &Catalog,
) {
    egui::Grid::new(("hash_condition_tokens", id))
        .num_columns(4)
        .spacing([16.0, 3.0])
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Index");
            ui.strong("Operation");
            ui.strong("Operand");
            ui.strong("Referenced entry");
            ui.end_row();
            for (token_index, token) in program.iter().enumerate() {
                ui.monospace((token_index + 1).to_string());
                ui.label(condition_opcode_label(token[0]));
                ui.monospace(token[1].to_string());
                draw_hash_condition_reference(ui, token[0], token[1], catalog);
                ui.end_row();
            }
        });
}

fn draw_hash_condition_reference(ui: &mut egui::Ui, kind: u32, operand: u32, catalog: &Catalog) {
    let index = operand as usize;
    let hash = match kind {
        1 => catalog
            .unlock_flag_definition(index)
            .map(|definition| definition.hash),
        10 => catalog
            .unlock_value_definition(index)
            .map(|definition| definition.hash),
        12 => catalog
            .objective_definition(index)
            .map(|objective| objective.hash),
        _ => None,
    };
    let text = condition_token_resolution(kind, operand, catalog);
    if let Some(hash) = hash {
        draw_hash_link(ui, hash, text);
    } else {
        ui.label(text);
    }
}

fn draw_hash_material_requirements(
    ui: &mut egui::Ui,
    id: egui::Id,
    requirements: &[MaterialRequirementDef],
) {
    if requirements.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("Material requirements ({})", requirements.len()))
        .id_salt((id, "material_requirements"))
        .default_open(false)
        .show(ui, |ui| {
            egui::Grid::new(("hash_material_requirements", id))
                .num_columns(6)
                .spacing([16.0, 3.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Index");
                    ui.strong("Hash");
                    ui.strong("Quantity");
                    ui.strong("Delete on action");
                    ui.strong("Omit from requirements");
                    ui.strong("Condition");
                    ui.end_row();
                    for requirement in requirements {
                        ui.monospace(requirement.item_definition_index.to_string());
                        draw_hash_link(
                            ui,
                            requirement.item_hash,
                            metadata_hash(requirement.item_hash),
                        );
                        ui.monospace(requirement.quantity.to_string());
                        ui.label(yes_no(requirement.delete_on_action));
                        ui.label(yes_no(requirement.omit_from_requirements));
                        ui.monospace(format!("0x{:04X}", requirement.condition));
                        ui.end_row();
                    }
                });
        });
}

fn draw_item_material_requirement_set_link(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    label: &str,
    index: Option<u16>,
) {
    let Some(index) = index else {
        return;
    };
    hash_detail_field(ui, &format!("{label} index"), index.to_string(), true);
    if let Some(set) = catalog.material_requirement_set(usize::from(index)) {
        hash_detail_field(ui, &format!("{label} hash"), metadata_hash(set.hash), true);
    }
}

fn draw_hash_item_sockets(ui: &mut egui::Ui, catalog: &Catalog, item: &crate::catalog::ItemDef) {
    let socket_count = item.sockets.len().max(item.default_plugs.len());
    if socket_count == 0 {
        return;
    }

    ui.add_space(8.0);
    egui::CollapsingHeader::new(format!("Sockets ({socket_count})"))
        .id_salt(("hash_item_sockets", item.hash))
        .default_open(socket_count <= 4)
        .show(ui, |ui| {
            egui::Grid::new(("hash_item_socket_rows", item.hash))
                .num_columns(5)
                .spacing([16.0, 3.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Index");
                    ui.strong("Socket");
                    ui.strong("Socket type");
                    ui.strong("Hash");
                    ui.strong("Plugs");
                    ui.end_row();
                    for socket_index in 0..socket_count {
                        let socket = item.sockets.get(socket_index);
                        let default_hash = item
                            .default_plugs
                            .get(socket_index)
                            .and_then(Option::as_deref)
                            .and_then(parse_metadata_hash);
                        ui.monospace(socket_index.to_string());
                        ui.label(socket.map_or("<not present>".into(), |socket| {
                            socket.display_label(socket_index)
                        }));
                        ui.monospace(
                            socket.map_or_else(
                                || "—".into(),
                                |socket| socket.socket_type.to_string(),
                            ),
                        );
                        if let Some(default_hash) = default_hash {
                            draw_hash_link(ui, default_hash, metadata_hash(default_hash));
                        } else {
                            ui.label(egui::RichText::new("—").weak());
                        }
                        ui.monospace(
                            socket
                                .map_or(0, |socket| catalog.socket_options(socket).len())
                                .to_string(),
                        );
                        ui.end_row();
                    }
                });

            for (socket_index, socket) in item.sockets.iter().enumerate() {
                let options = catalog.socket_options(socket);
                if options.is_empty() {
                    continue;
                }
                egui::CollapsingHeader::new(format!(
                    "Socket {socket_index} plugs ({})",
                    options.len()
                ))
                .id_salt(("hash_item_socket_plugs", item.hash, socket_index))
                .show(ui, |ui| {
                    draw_hash_item_rows(
                        ui,
                        catalog,
                        ("socket_plugs", item.hash, socket_index),
                        options.iter().copied(),
                    );
                });
            }
        });
}

fn draw_hash_item_abilities(ui: &mut egui::Ui, item: &crate::catalog::ItemDef) {
    let abilities = &item.abilities;
    let choice_count = abilities.movement.len()
        + abilities.grenade.len()
        + abilities.super_ability.len()
        + abilities.melee.len()
        + abilities.class_ability.len()
        + abilities
            .attunements
            .iter()
            .map(|attunement| {
                attunement.super_abilities.len()
                    + attunement.perks.len()
                    + usize::from(attunement.melee.entry != 0)
            })
            .sum::<usize>();
    if choice_count == 0 {
        return;
    }

    ui.add_space(8.0);
    egui::CollapsingHeader::new(format!("Abilities ({choice_count})"))
        .id_salt(("hash_item_abilities", item.hash))
        .show(ui, |ui| {
            egui::Grid::new(("hash_item_ability_rows", item.hash))
                .num_columns(3)
                .spacing([16.0, 3.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Ability");
                    ui.strong("Hash");
                    ui.strong("Name");
                    ui.end_row();
                    for (label, choices) in [
                        ("Movement", abilities.movement.as_slice()),
                        ("Grenade", abilities.grenade.as_slice()),
                        ("Super ability", abilities.super_ability.as_slice()),
                        ("Melee", abilities.melee.as_slice()),
                        ("Class ability", abilities.class_ability.as_slice()),
                    ] {
                        for choice in choices {
                            draw_hash_ability_row(ui, label, choice);
                        }
                    }
                    for attunement in &abilities.attunements {
                        for choice in &attunement.super_abilities {
                            draw_hash_ability_row(
                                ui,
                                &format!("{} · Super ability", attunement.name),
                                choice,
                            );
                        }
                        if attunement.melee.entry != 0 {
                            draw_hash_ability_row(
                                ui,
                                &format!("{} · Melee", attunement.name),
                                &attunement.melee,
                            );
                        }
                        for choice in &attunement.perks {
                            draw_hash_ability_row(
                                ui,
                                &format!("{} · Perk", attunement.name),
                                choice,
                            );
                        }
                    }
                });
        });
}

fn draw_hash_ability_row(ui: &mut egui::Ui, label: &str, choice: &crate::catalog::AbilityChoice) {
    ui.label(label);
    draw_hash_link(ui, choice.entry, metadata_hash(choice.entry));
    ui.label(metadata_text(&choice.name));
    ui.end_row();
}

fn draw_hash_inventory_bucket(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: u64,
    items: &[&crate::catalog::ItemDef],
) {
    let mut native_buckets = items
        .iter()
        .filter_map(|item| catalog.inventory_metadata(item.hash))
        .map(|metadata| metadata.bucket_label())
        .collect::<Vec<_>>();
    native_buckets.sort();
    native_buckets.dedup();

    ui.add_space(8.0);
    hash_metadata_section(ui, "Inventory bucket", false, |ui| {
        egui::Grid::new(("hash_inventory_bucket", hash))
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                hash_detail_field(ui, "Definition hash", metadata_hash(hash), true);
                hash_detail_field(ui, "Items", items.len().to_string(), true);
                hash_detail_field(
                    ui,
                    "Native buckets",
                    if native_buckets.is_empty() {
                        "<not resolved>".into()
                    } else {
                        native_buckets.join(" · ")
                    },
                    false,
                );
            });
        egui::CollapsingHeader::new(format!("Items ({})", items.len()))
            .id_salt(("hash_inventory_bucket_items", hash))
            .default_open(items.len() <= 20)
            .show(ui, |ui| {
                draw_hash_item_rows(
                    ui,
                    catalog,
                    ("bucket_items", hash, 0_usize),
                    items.iter().map(|item| item.hash),
                );
            });
    });
}

fn draw_hash_item_rows(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    id: (&'static str, u64, usize),
    hashes: impl IntoIterator<Item = u64>,
) {
    let hashes = hashes.into_iter().collect::<Vec<_>>();
    let hash_width = 126.0;
    let name_width = 240.0;
    let type_width = 180.0;
    ui.horizontal(|ui| {
        table_cell(ui, hash_width, egui::RichText::new("Hash").strong());
        table_cell(ui, name_width, egui::RichText::new("Name").strong());
        table_cell(ui, type_width, egui::RichText::new("Type").strong());
    });
    egui::ScrollArea::vertical()
        .id_salt(("hash_item_rows", id))
        .auto_shrink([false, false])
        .max_height(240.0)
        .show_rows(ui, TABLE_CELL_HEIGHT, hashes.len(), |ui, range| {
            egui::Grid::new(("hash_item_row_grid", id))
                .num_columns(3)
                .striped(true)
                .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                .show(ui, |ui| {
                    for row in range {
                        let hash = hashes[row];
                        draw_hash_cell(ui, hash_width, Some(hash));
                        table_cell(
                            ui,
                            name_width,
                            catalog.display_name(hash).unwrap_or("<not resolved>"),
                        );
                        table_cell(
                            ui,
                            type_width,
                            catalog
                                .item(hash)
                                .map_or("<not resolved>", |item| metadata_text(&item.type_name)),
                        );
                        ui.end_row();
                    }
                });
        });
}

fn draw_hash_material_requirement_set(
    ui: &mut egui::Ui,
    set: &crate::catalog::MaterialRequirementSetDef,
    inspected_hash: u64,
) {
    metadata_subsection(
        ui,
        &format!("Material requirement set #{}", set.index),
        |ui| {
            egui::Grid::new(("hash_material_requirement_set", set.index))
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    hash_detail_field(
                        ui,
                        "Matched as",
                        if set.hash == inspected_hash {
                            "Material requirement set hash"
                        } else {
                            "Item definition hash"
                        },
                        false,
                    );
                    hash_detail_field(
                        ui,
                        "Material requirement set index",
                        set.index.to_string(),
                        true,
                    );
                    hash_detail_field(
                        ui,
                        "Material requirement set hash",
                        metadata_hash(set.hash),
                        true,
                    );
                });
            draw_hash_material_requirements(
                ui,
                egui::Id::new(("hash_material_requirement_set_detail", set.index, set.hash)),
                &set.requirements,
            );
        },
    );
}

fn draw_hash_unlock_matches(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    title: &str,
    matches: &[(usize, &UnlockDefinition)],
) {
    if matches.is_empty() {
        return;
    }
    ui.add_space(8.0);
    hash_metadata_section(
        ui,
        &format!("{title} ({})", matches.len()),
        matches.len() <= 3,
        |ui| {
            for (index, definition) in matches {
                metadata_subsection(ui, &format!("Definition #{index}"), |ui| {
                    egui::Grid::new(("hash_unlock_definition", title, *index))
                        .num_columns(2)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            let name = definition
                                .name
                                .as_deref()
                                .filter(|name| !name.trim().is_empty())
                                .or_else(|| catalog.display_name(definition.hash));
                            hash_detail_field(ui, "Name", name.unwrap_or("<not present>"), false);
                            hash_detail_field(
                                ui,
                                "Description",
                                definition
                                    .description
                                    .as_deref()
                                    .filter(|description| !description.trim().is_empty())
                                    .unwrap_or("<not present>"),
                                false,
                            );
                            hash_detail_field(
                                ui,
                                "Code",
                                format!("0x{:04X}", definition.code),
                                true,
                            );
                            hash_detail_field(ui, "Bank", definition.bank().to_string(), true);
                            hash_detail_field(
                                ui,
                                "Compact slot",
                                definition
                                    .compact_slot
                                    .map_or_else(|| "<none>".into(), |slot| slot.to_string()),
                                true,
                            );
                            hash_detail_field(
                                ui,
                                "Readers",
                                definition.tested_by.len().to_string(),
                                true,
                            );
                        });
                });
            }
        },
    );
}

fn hash_detail_field(ui: &mut egui::Ui, label: &str, value: impl Into<String>, monospace: bool) {
    ui.label(egui::RichText::new(label).weak());
    let value = value.into();
    let absent = value.starts_with('<') && value.ends_with('>');
    let parsed_hash = label
        .to_ascii_lowercase()
        .contains("hash")
        .then(|| parse_metadata_hash(metadata_hash_hex_text(&value)))
        .flatten()
        .filter(|hash| *hash != 0);
    let text = egui::RichText::new(&value);
    let text = if absent { text.weak().italics() } else { text };
    let text = if monospace { text.monospace() } else { text };
    if let Some(parsed_hash) = parsed_hash {
        let response = ui
            .add(egui::Button::new(text).frame(false))
            .on_hover_text(format!("Open details for 0x{parsed_hash:08X}"));
        if response.clicked() {
            request_hash_inspection(ui.ctx(), parsed_hash);
        }
    } else {
        ui.label(text);
    }
    ui.end_row();
}

fn draw_override_metadata(
    ui: &mut egui::Ui,
    selection: MetadataSelection,
    definition: &UnlockDefinition,
) {
    let index = selection.definition_index();
    let usage = override_usage_summary(selection, definition);
    let condition_program_decode = if definition.tested_by.is_empty() {
        "No condition programs"
    } else if definition_has_undecoded_opcodes(definition) {
        "Contains undecoded opcodes"
    } else {
        "All opcodes decoded"
    };
    egui::Grid::new("progression_override_metadata")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            metadata_field(ui, "Definition index", format!("#{index}"), true);
            match selection {
                MetadataSelection::FlagOverride(_, value) => {
                    metadata_field(
                        ui,
                        "Logical flag value",
                        flag_override_state_label(value),
                        false,
                    );
                    metadata_field(
                        ui,
                        "Settings field",
                        "state.investment.family5_flag_overrides",
                        true,
                    );
                }
                MetadataSelection::ValueOverride(_, value) => {
                    metadata_field(ui, "Value", value.to_string(), true);
                    metadata_field(
                        ui,
                        "Settings field",
                        "state.investment.family5_value_overrides",
                        true,
                    );
                }
                MetadataSelection::FlagDefinition(_) | MetadataSelection::ValueDefinition(_) => {}
            }
            metadata_field(
                ui,
                "Condition program decode",
                condition_program_decode,
                false,
            );
            metadata_field(ui, "Readers", usage.readers, false);
            metadata_field(ui, "Reader kinds", usage.reader_types, false);
            metadata_field(ui, "Condition usage", usage.condition_usage, false);
            if let Some(impact) = usage.forced_impact {
                metadata_field(ui, "Result", impact, false);
            }
            if let Some(opcodes) = usage.undecoded_opcodes {
                metadata_field(ui, "Undecoded opcodes", opcodes, true);
            }
        });
}

struct OverrideUsageSummary {
    readers: String,
    reader_types: String,
    condition_usage: String,
    forced_impact: Option<String>,
    undecoded_opcodes: Option<String>,
}

fn definition_has_undecoded_opcodes(definition: &UnlockDefinition) -> bool {
    definition
        .tested_by
        .iter()
        .flat_map(|context| context.condition_programs.iter())
        .flatten()
        .any(|token| !decoded_condition_opcode(token[0]))
}

const fn decoded_condition_opcode(opcode: u32) -> bool {
    matches!(
        opcode,
        1 | 2 | 3 | 4 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 22
    )
}

fn override_filter_matches(filter: OverrideFilter, definition: Option<&UnlockDefinition>) -> bool {
    match filter {
        OverrideFilter::All => true,
        OverrideFilter::Unmapped => definition.is_none(),
        OverrideFilter::NoResolvedReaders => {
            definition.is_some_and(|definition| definition.tested_by.is_empty())
        }
        OverrideFilter::PartiallyDecoded => {
            definition.is_some_and(definition_has_undecoded_opcodes)
        }
    }
}

fn override_usage_summary(
    selection: MetadataSelection,
    definition: &UnlockDefinition,
) -> OverrideUsageSummary {
    let mut reader_types = Vec::<(ProgressionContextKind, usize)>::new();
    let mut programs = Vec::<Vec<[u32; 2]>>::new();
    for context in &definition.tested_by {
        if let Some((_, count)) = reader_types
            .iter_mut()
            .find(|(kind, _)| *kind == context.kind)
        {
            *count += 1;
        } else {
            reader_types.push((context.kind, 1));
        }
        programs.extend(context.condition_programs.iter().cloned());
    }
    let program_count = programs.len();
    programs.sort();
    programs.dedup();

    let readers = if definition.tested_by.is_empty() {
        "No package reference found".to_owned()
    } else {
        format!(
            "{} exact package {}",
            definition.tested_by.len(),
            if definition.tested_by.len() == 1 {
                "relationship"
            } else {
                "relationships"
            }
        )
    };
    let reader_types = if reader_types.is_empty() {
        "None resolved".to_owned()
    } else {
        reader_types
            .into_iter()
            .map(|(kind, count)| format!("{} {count}", progression_context_kind_label(kind)))
            .collect::<Vec<_>>()
            .join(" · ")
    };

    let mut undecoded = programs
        .iter()
        .flat_map(|program| program.iter().map(|token| token[0]))
        .filter(|opcode| !decoded_condition_opcode(*opcode))
        .collect::<Vec<_>>();
    undecoded.sort_unstable();
    undecoded.dedup();
    let undecoded_opcodes = (!undecoded.is_empty()).then(|| {
        format!(
            "{} · preserved raw in each condition program",
            undecoded
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    });

    let (condition_usage, forced_impact) = match selection {
        MetadataSelection::FlagOverride(index, value) => {
            let direct = programs
                .iter()
                .filter(|program| program.as_slice() == [[1, index as u32]])
                .count();
            let negated = programs
                .iter()
                .filter(|program| {
                    program.len() == 2 && program[0] == [1, index as u32] && program[1][0] == 2
                })
                .count();
            let composite = programs.len().saturating_sub(direct + negated);
            let usage = format!(
                "{program_count} program {}, {} unique · {direct} direct · {negated} negated · {composite} composite/other",
                if program_count == 1 {
                    "occurrence"
                } else {
                    "occurrences"
                },
                programs.len()
            );
            let active = value == FAMILY5_FLAG_VALUE_MAXIMUM;
            let impact = Some(format!(
                "Direct checks read {}; negated checks read {}",
                if active { "true" } else { "false" },
                if active { "false" } else { "true" }
            ));
            (usage, impact)
        }
        MetadataSelection::ValueOverride(index, value) => {
            let mut comparisons = programs
                .iter()
                .filter_map(|program| direct_value_comparison(program, index, value))
                .collect::<Vec<_>>();
            comparisons.sort_by(|left, right| left.0.cmp(&right.0));
            comparisons.dedup_by(|left, right| left.0 == right.0);
            let composite = programs.len().saturating_sub(comparisons.len());
            let mut labels = comparisons
                .iter()
                .map(|(label, _)| label.clone())
                .take(10)
                .collect::<Vec<_>>();
            if comparisons.len() > 10 {
                labels.push(format!("+{} more", comparisons.len() - 10));
            }
            let direct_text = if labels.is_empty() {
                "no standalone decoded comparison".to_owned()
            } else {
                format!("direct {}", labels.join(", "))
            };
            let usage = format!(
                "{program_count} program {}, {} unique · {direct_text} · {composite} composite/other",
                if program_count == 1 {
                    "occurrence"
                } else {
                    "occurrences"
                },
                programs.len()
            );
            let passed = comparisons.iter().filter(|(_, result)| *result).count();
            let impact = (!comparisons.is_empty()).then(|| {
                format!(
                    "At {value}: {passed} decoded direct {} pass, {} fail",
                    if passed == 1 {
                        "comparison"
                    } else {
                        "comparisons"
                    },
                    comparisons.len() - passed
                )
            });
            (usage, impact)
        }
        MetadataSelection::FlagDefinition(_) | MetadataSelection::ValueDefinition(_) => (
            format!(
                "{program_count} program {}, {} unique",
                if program_count == 1 {
                    "occurrence"
                } else {
                    "occurrences"
                },
                programs.len()
            ),
            None,
        ),
    };

    OverrideUsageSummary {
        readers,
        reader_types,
        condition_usage,
        forced_impact,
        undecoded_opcodes,
    }
}

fn direct_value_comparison(
    program: &[[u32; 2]],
    definition_index: usize,
    forced_value: i32,
) -> Option<(String, bool)> {
    let (left, right, operator) = match program {
        [left, right, operator] => (*left, *right, *operator),
        [left, right, encoding, operator] if *encoding == [22, 0] => (*left, *right, *operator),
        _ => return None,
    };
    let index = u32::try_from(definition_index).ok()?;
    let (literal, reference_first) = match (left[0], right[0]) {
        (10, 11) if left[1] == index => (right[1] as i32, true),
        (11, 10) if right[1] == index => (left[1] as i32, false),
        _ => return None,
    };
    let (label, result) = match (operator[0], reference_first) {
        (8, _) => (format!("= {literal}"), forced_value == literal),
        (9, _) => (format!("≠ {literal}"), forced_value != literal),
        (13, true) => (format!("> {literal}"), forced_value > literal),
        (13, false) => (format!("< {literal}"), forced_value < literal),
        (14, true) => (format!("≥ {literal}"), forced_value >= literal),
        (14, false) => (format!("≤ {literal}"), forced_value <= literal),
        (15, true) => (format!("< {literal}"), forced_value < literal),
        (15, false) => (format!("> {literal}"), forced_value > literal),
        _ => return None,
    };
    Some((label, result))
}

fn draw_unlock_definition_metadata(
    ui: &mut egui::Ui,
    index: usize,
    definition: &UnlockDefinition,
    catalog: &Catalog,
    state: &mut UiState,
) {
    let [bank, code_high_byte] = definition.code.to_le_bytes();
    egui::Grid::new("progression_definition_metadata")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            metadata_field(ui, "Definition index", format!("#{index}"), true);
            metadata_hash_field(ui, "Definition hash", definition.hash);
            metadata_field(
                ui,
                "Name",
                definition
                    .name
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                    .or_else(|| catalog.display_name(definition.hash))
                    .unwrap_or("<not present>"),
                false,
            );
            metadata_field(
                ui,
                "Description",
                definition.description.as_deref().unwrap_or("<not present>"),
                false,
            );
            metadata_field(
                ui,
                "Condition references",
                definition.tested_by.len().to_string(),
                true,
            );
        });
    egui::CollapsingHeader::new("Technical package fields")
        .id_salt(("progression_definition_technical", index))
        .show(ui, |ui| {
            egui::Grid::new(("progression_definition_technical_fields", index))
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    metadata_field(
                        ui,
                        "Code",
                        format!("0x{:04X} · {}", definition.code, definition.code),
                        true,
                    );
                    metadata_field(ui, "Bank / code low byte", bank.to_string(), true);
                    metadata_field(ui, "Code high byte", code_high_byte.to_string(), true);
                    metadata_field(
                        ui,
                        "Compact slot",
                        definition.compact_slot.map_or_else(
                            || "Unbanked".into(),
                            |slot| format!("{slot} · 0x{slot:04X}"),
                        ),
                        true,
                    );
                });
        });
    for (context_index, context) in definition.tested_by.iter().enumerate() {
        let label = format!(
            "{}. {} · 0x{:08X}",
            context_index + 1,
            progression_context_kind_label(context.kind),
            context.hash
        );
        egui::CollapsingHeader::new(label)
            .id_salt(("progression_context_metadata", context_index))
            .default_open(definition.tested_by.len() <= 3)
            .show(ui, |ui| draw_context_metadata(ui, context, catalog, state));
    }
}

fn draw_context_metadata(
    ui: &mut egui::Ui,
    context: &ProgressionContextDef,
    catalog: &Catalog,
    state: &mut UiState,
) {
    egui::Grid::new((
        "progression_context_fields",
        context.hash,
        context.kind as u8,
    ))
    .num_columns(2)
    .spacing([16.0, 4.0])
    .show(ui, |ui| {
        metadata_field(
            ui,
            "Kind",
            progression_context_kind_label(context.kind),
            false,
        );
        metadata_hash_field(ui, "Definition hash", context.hash);
        metadata_field(ui, "Name", metadata_text(&context.name), false);
        metadata_field(ui, "Type", metadata_text(&context.type_name), false);
        metadata_field(
            ui,
            "Description",
            metadata_text(&context.description),
            false,
        );
        metadata_field(
            ui,
            "Condition programs",
            context.condition_programs.len().to_string(),
            true,
        );
    });
    draw_metadata_paths(ui, &context.paths);
    draw_condition_programs(
        ui,
        "context",
        context.hash,
        &context.condition_programs,
        catalog,
        state,
    );
}

fn draw_condition_programs(
    ui: &mut egui::Ui,
    id_source: &'static str,
    owner_hash: u64,
    programs: &[Vec<[u32; 2]>],
    catalog: &Catalog,
    state: &mut UiState,
) {
    for (program_index, program) in programs.iter().enumerate() {
        egui::CollapsingHeader::new(format!("Condition program {}", program_index + 1))
            .id_salt((id_source, owner_hash, program_index))
            .show(ui, |ui| {
                egui::Grid::new((id_source, "condition_tokens", owner_hash, program_index))
                    .num_columns(4)
                    .spacing([16.0, 3.0])
                    .show(ui, |ui| {
                        ui.strong("#");
                        ui.strong("Operation");
                        ui.strong("Operand");
                        ui.strong("Referenced entry");
                        ui.end_row();
                        for (token_index, token) in program.iter().enumerate() {
                            ui.monospace((token_index + 1).to_string());
                            ui.label(condition_opcode_label(token[0]));
                            ui.monospace(token[1].to_string());
                            let resolution =
                                condition_token_resolution(token[0], token[1], catalog);
                            if let Some(selection) =
                                condition_token_selection(token[0], token[1], catalog)
                            {
                                if ui
                                    .add(egui::Button::new(resolution).frame(false))
                                    .on_hover_text("Open referenced metadata")
                                    .clicked()
                                {
                                    state.open_metadata(selection);
                                }
                            } else {
                                ui.label(resolution);
                            }
                            ui.end_row();
                        }
                    });
                egui::CollapsingHeader::new("Raw opcodes")
                    .id_salt((id_source, "raw_condition", owner_hash, program_index))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(
                                    program
                                        .iter()
                                        .map(|token| format!("{}:{}", token[0], token[1]))
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                )
                                .monospace(),
                            )
                            .wrap(),
                        );
                    });
            });
    }
}

fn condition_token_selection(
    kind: u32,
    operand: u32,
    catalog: &Catalog,
) -> Option<MetadataSelection> {
    let index = operand as usize;
    match kind {
        1 => catalog
            .unlock_flag_definition(index)
            .map(|_| MetadataSelection::FlagDefinition(index)),
        10 => catalog
            .unlock_value_definition(index)
            .map(|_| MetadataSelection::ValueDefinition(index)),
        12 => catalog
            .objective_definition(index)
            .and_then(|objective| objective.related_unlock_value_definition_index)
            .map(usize::from)
            .and_then(|definition_index| {
                catalog
                    .unlock_value_definition(definition_index)
                    .map(|_| MetadataSelection::ValueDefinition(definition_index))
            }),
        _ => None,
    }
}

fn condition_opcode_label(kind: u32) -> String {
    match kind {
        1 => "Flag reference (1)".into(),
        2 => "Not (2)".into(),
        3 => "Or (3)".into(),
        4 => "And (4)".into(),
        8 => "Equal (8)".into(),
        9 => "Not equal (9)".into(),
        10 => "Value reference (10)".into(),
        11 => "Literal (11)".into(),
        12 => "Objective reference (12)".into(),
        13 => "Greater than (13)".into(),
        14 => "Greater than or equal (14)".into(),
        15 => "Less than (15)".into(),
        22 => "Literal encoding (22)".into(),
        _ => format!("Undecoded ({kind})"),
    }
}

fn condition_token_resolution(kind: u32, operand: u32, catalog: &Catalog) -> String {
    let index = operand as usize;
    if kind == 12 {
        let Some(objective) = catalog.objective_definition(index) else {
            return format!("Objective #{index} unavailable");
        };
        return format!(
            "Objective #{index} · {} · 0x{:08X}",
            resolved_objective_table_text(catalog, objective, None),
            objective.hash
        );
    }
    let definition = match kind {
        1 => catalog.unlock_flag_definition(index),
        10 => catalog.unlock_value_definition(index),
        _ => return "—".into(),
    };
    let Some(definition) = definition else {
        return format!("Definition #{index} unavailable");
    };
    let identity = definition_name(definition)
        .or_else(|| catalog.display_name(definition.hash))
        .unwrap_or("<not resolved>");
    let slot = definition.compact_slot.map_or_else(
        || "unbanked".into(),
        |slot| format!("bank {} · slot {slot}", definition.bank()),
    );
    format!("#{index} · {identity} · 0x{:08X} · {slot}", definition.hash)
}

fn draw_objective_metadata(
    ui: &mut egui::Ui,
    objective: &ObjectiveDef,
    definition: &UnlockDefinition,
    catalog: &Catalog,
    state: &mut UiState,
) {
    let external_context_count = meaningful_definition_contexts(definition).len();
    let ownership_source = if !objective.owners.is_empty() {
        format!(
            "{} direct package {}",
            objective.owners.len(),
            if objective.owners.len() == 1 {
                "owner"
            } else {
                "owners"
            }
        )
    } else if !objective.referenced_objective_indices.is_empty() {
        format!(
            "{} linked {}",
            objective.referenced_objective_indices.len(),
            if objective.referenced_objective_indices.len() == 1 {
                "objective"
            } else {
                "objectives"
            }
        )
    } else if external_context_count > 0 {
        format!(
            "{external_context_count} reverse package {}",
            if external_context_count == 1 {
                "reference"
            } else {
                "references"
            }
        )
    } else if let Some(index) = objective.related_unlock_value_definition_index {
        format!("Unlock value definition #{index}")
    } else {
        "Objective definition".to_owned()
    };
    egui::Grid::new("progression_objective_metadata")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            metadata_field(
                ui,
                "Resolved label",
                resolved_objective_table_text(catalog, objective, Some(definition)),
                false,
            );
            metadata_field(ui, "Ownership source", ownership_source, false);
            metadata_hash_field(ui, "Definition hash", objective.hash);
            metadata_hash_field(ui, "Unlock definition hash", definition.hash);
            metadata_field(ui, "Name", metadata_text(&objective.name), false);
            metadata_field(
                ui,
                "Display description",
                metadata_text(&objective.display_description),
                false,
            );
            metadata_field(
                ui,
                "Progress description",
                metadata_text(&objective.progress_description),
                false,
            );
            metadata_field(
                ui,
                "Completion value",
                objective.completion_value.to_string(),
                true,
            );
            metadata_field(
                ui,
                "Related unlock definition",
                objective.related_unlock_value_definition_index.map_or_else(
                    || "<not present>".into(),
                    |index| format!("#{index} · current account value"),
                ),
                true,
            );
            metadata_field(ui, "Unlock bank", definition.bank().to_string(), true);
            metadata_field(
                ui,
                "Account slot",
                definition
                    .compact_slot
                    .map_or_else(|| "<unbanked>".into(), |slot| slot.to_string()),
                true,
            );
            metadata_field(
                ui,
                "Allows over-completion",
                yes_no(objective.allow_overcompletion),
                false,
            );
            metadata_field(
                ui,
                "Allows negative values",
                yes_no(objective.allow_negative_value),
                false,
            );
            metadata_field(
                ui,
                "Allows changes after completion",
                yes_no(objective.allow_value_change_when_completed),
                false,
            );
            metadata_field(
                ui,
                "Counts downward",
                yes_no(objective.is_counting_downward),
                false,
            );
            metadata_field(
                ui,
                "Condition programs",
                objective.condition_programs.len().to_string(),
                true,
            );
            metadata_field(
                ui,
                "Referenced objectives",
                objective.referenced_objective_indices.len().to_string(),
                true,
            );
            metadata_field(
                ui,
                "Intrinsic perk flags",
                objective
                    .intrinsic_perk_flag_definition_indices
                    .len()
                    .to_string(),
                true,
            );
            metadata_field(ui, "Owners", objective.owners.len().to_string(), true);
        });
    draw_condition_programs(
        ui,
        "objective",
        objective.hash,
        &objective.condition_programs,
        catalog,
        state,
    );
    draw_objective_references(ui, objective, catalog);
    draw_objective_intrinsic_perks(ui, objective, catalog);
    for (owner_index, owner) in objective.owners.iter().enumerate() {
        let label = format!(
            "{}. {} · 0x{:08X}",
            owner_index + 1,
            objective_owner_kind_label(owner.kind),
            owner.hash
        );
        egui::CollapsingHeader::new(label)
            .id_salt(("progression_owner_metadata", owner_index))
            .default_open(objective.owners.len() <= 3)
            .show(ui, |ui| draw_objective_owner_metadata(ui, owner));
    }
}

fn draw_objective_references(ui: &mut egui::Ui, objective: &ObjectiveDef, catalog: &Catalog) {
    if objective.referenced_objective_indices.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!(
        "Referenced objectives ({})",
        objective.referenced_objective_indices.len()
    ))
    .id_salt(("objective_references", objective.hash))
    .default_open(true)
    .show(ui, |ui| {
        egui::Grid::new(("objective_reference_rows", objective.hash))
            .num_columns(4)
            .spacing([16.0, 3.0])
            .show(ui, |ui| {
                ui.strong("Index");
                ui.strong("Label");
                ui.strong("Hash");
                ui.strong("Relationship");
                ui.end_row();
                for &raw_index in &objective.referenced_objective_indices {
                    let index = usize::from(raw_index);
                    let target = catalog.objective_definition(index);
                    ui.monospace(format!("#{index}"));
                    ui.label(
                        target
                            .map(|target| resolved_objective_table_text(catalog, target, None))
                            .unwrap_or_else(|| "<unavailable>".into()),
                    );
                    if let Some(target) = target {
                        draw_hash_link(ui, target.hash, metadata_hash(target.hash));
                    } else {
                        ui.label(egui::RichText::new("<unavailable>").weak().italics());
                    }
                    ui.label("Condition program reference (opcode 12)");
                    ui.end_row();
                }
            });
    });
}

fn draw_objective_intrinsic_perks(ui: &mut egui::Ui, objective: &ObjectiveDef, catalog: &Catalog) {
    if objective.intrinsic_perk_flag_definition_indices.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!(
        "Intrinsic perks ({})",
        objective.intrinsic_perk_flag_definition_indices.len()
    ))
    .id_salt(("objective_intrinsic_perks", objective.hash))
    .default_open(true)
    .show(ui, |ui| {
        egui::Grid::new(("objective_intrinsic_perk_rows", objective.hash))
            .num_columns(4)
            .spacing([16.0, 3.0])
            .show(ui, |ui| {
                ui.strong("Index");
                ui.strong("Perk");
                ui.strong("Hash");
                ui.strong("Effect");
                ui.end_row();
                for &raw_index in &objective.intrinsic_perk_flag_definition_indices {
                    let index = usize::from(raw_index);
                    let definition = catalog.unlock_flag_definition(index);
                    ui.monospace(format!("#{index}"));
                    ui.label(
                        definition
                            .and_then(|definition| {
                                definition_name(definition)
                                    .or_else(|| catalog.display_name(definition.hash))
                            })
                            .unwrap_or("Package perk name not resolved"),
                    );
                    if let Some(definition) = definition {
                        draw_hash_link(ui, definition.hash, metadata_hash(definition.hash));
                    } else {
                        ui.label(egui::RichText::new("<unavailable>").weak().italics());
                    }
                    ui.label("Enabled when this objective completes");
                    ui.end_row();
                }
            });
    });
}

fn draw_objective_owner_metadata(ui: &mut egui::Ui, owner: &ObjectiveOwnerDef) {
    egui::Grid::new(("progression_owner_fields", owner.hash, owner.kind as u8))
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            metadata_field(ui, "Kind", objective_owner_kind_label(owner.kind), false);
            metadata_hash_field(ui, "Definition hash", owner.hash);
            metadata_field(ui, "Name", metadata_text(&owner.name), false);
            metadata_field(ui, "Type", metadata_text(&owner.type_name), false);
            metadata_field(ui, "Description", metadata_text(&owner.description), false);
            metadata_field(ui, "Traits", owner.traits.len().to_string(), true);
        });
    draw_metadata_paths(ui, &owner.paths);
    for (trait_index, trait_definition) in owner.traits.iter().enumerate() {
        ui.add_space(6.0);
        metadata_subsection(ui, &format!("Trait {}", trait_index + 1), |ui| {
            egui::Grid::new((
                "progression_trait_fields",
                owner.hash,
                trait_index,
                trait_definition.hash,
            ))
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                metadata_hash_field(ui, "Definition hash", trait_definition.hash);
                metadata_field(ui, "Name", metadata_text(&trait_definition.name), false);
                metadata_field(
                    ui,
                    "Description",
                    metadata_text(&trait_definition.description),
                    false,
                );
            });
        });
    }
}

fn draw_metadata_paths(ui: &mut egui::Ui, paths: &[Vec<String>]) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(format!("Package paths ({})", paths.len()))
            .strong()
            .small(),
    );
    if paths.is_empty() {
        ui.label(egui::RichText::new("<none>").weak().monospace());
        return;
    }
    for (index, path) in paths.iter().enumerate() {
        let path = metadata_path_text(path);
        ui.label(egui::RichText::new(format!("{}. {path}", index + 1)).monospace());
    }
}

fn metadata_path_text(path: &[String]) -> String {
    if path.is_empty() {
        "<empty path>".into()
    } else {
        path.iter()
            .rev()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" > ")
    }
}

fn metadata_section<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.label(egui::RichText::new(title).heading().strong());
    ui.add_space(4.0);
    add_contents(ui)
}

fn hash_metadata_section(
    ui: &mut egui::Ui,
    title: &str,
    default_open: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    ui.add_space(6.0);
    egui::CollapsingHeader::new(egui::RichText::new(title).strong())
        .id_salt(("hash_metadata_section", title))
        .default_open(default_open)
        .show(ui, add_contents);
}

fn metadata_subsection<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.label(egui::RichText::new(title).strong().small());
    ui.add_space(2.0);
    add_contents(ui)
}

fn metadata_field(
    ui: &mut egui::Ui,
    label: &'static str,
    value: impl Into<String>,
    monospace: bool,
) {
    ui.label(egui::RichText::new(label).weak());
    let value = value.into();
    let absent = value.starts_with('<') && value.ends_with('>');
    let text = egui::RichText::new(&value);
    let text = if absent { text.weak().italics() } else { text };
    let text = if monospace { text.monospace() } else { text };
    ui.label(text);
    ui.end_row();
}

fn metadata_hash_field(ui: &mut egui::Ui, label: &'static str, hash: u64) {
    ui.label(egui::RichText::new(label).weak());
    draw_hash_link(ui, hash, metadata_hash(hash));
    ui.end_row();
}

fn draw_hash_link(ui: &mut egui::Ui, hash: u64, text: impl Into<String>) -> egui::Response {
    let response = ui
        .add(egui::Button::new(egui::RichText::new(text.into()).monospace()).frame(false))
        .on_hover_text(format!("Open details for 0x{hash:08X}"));
    if response.clicked() {
        request_hash_inspection(ui.ctx(), hash);
    }
    response
}

fn metadata_hash_hex_text(value: &str) -> &str {
    value.split_once(" · ").map_or(value, |(hex, _)| hex)
}

fn parse_metadata_hash(value: &str) -> Option<u64> {
    u64::from_str_radix(value.trim().strip_prefix("0x")?, 16).ok()
}

const HASH_INSPECTION_REQUEST_ID: &str = "catalog_hash_inspection_request";

pub(super) fn request_hash_inspection(ctx: &egui::Context, hash: u64) {
    if hash != 0 {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new(HASH_INSPECTION_REQUEST_ID), hash));
    }
}

pub(super) fn take_hash_inspection_request(ctx: &egui::Context) -> Option<u64> {
    ctx.data_mut(|data| data.remove_temp(egui::Id::new(HASH_INSPECTION_REQUEST_ID)))
}

fn metadata_text(value: &str) -> &str {
    if value.is_empty() { "<empty>" } else { value }
}

fn metadata_hash(hash: u64) -> String {
    format!("0x{hash:08X} · {hash}")
}

const fn yes_no(value: bool) -> &'static str {
    if value { "Yes" } else { "No" }
}

const fn objective_owner_kind_label(kind: ObjectiveOwnerKind) -> &'static str {
    match kind {
        ObjectiveOwnerKind::InventoryItem => "Inventory item",
        ObjectiveOwnerKind::Milestone => "Milestone",
        ObjectiveOwnerKind::Metric => "Metric",
        ObjectiveOwnerKind::Record => "Record",
        ObjectiveOwnerKind::PresentationNode => "Presentation node",
    }
}

fn draw_context_cell(ui: &mut egui::Ui, width: f32, context: Option<&ContextDisplayLine<'_>>) {
    let Some(context) = context else {
        table_cell(ui, width, egui::RichText::new("—").weak());
        return;
    };
    let text = context.text();
    let rich_text = if text == "—" {
        egui::RichText::new(&text).weak()
    } else {
        egui::RichText::new(&text)
    };
    table_cell(ui, width, rich_text).on_hover_text(context_relation_tooltip(context));
}

fn definition_context_display_lines<'a>(
    row_count: usize,
    mut definition_for_row: impl FnMut(usize) -> Option<(usize, &'a UnlockDefinition)>,
    reverse_contexts: bool,
) -> Vec<DefinitionContextDisplayLine<'a>> {
    let mut display_lines = Vec::new();
    for row_index in 0..row_count {
        let Some((definition_index, definition)) = definition_for_row(row_index) else {
            display_lines.push(DefinitionContextDisplayLine {
                row_index,
                definition_index: None,
                definition: None,
                context: None,
                primary: true,
            });
            continue;
        };
        let mut contexts = definition_context_lines(definition);
        if reverse_contexts {
            contexts.reverse();
        }
        if contexts.is_empty() {
            display_lines.push(DefinitionContextDisplayLine {
                row_index,
                definition_index: Some(definition_index),
                definition: Some(definition),
                context: None,
                primary: true,
            });
            continue;
        }
        display_lines.extend(
            contexts
                .into_iter()
                .enumerate()
                .map(|(context_index, context)| DefinitionContextDisplayLine {
                    row_index,
                    definition_index: Some(definition_index),
                    definition: Some(definition),
                    context: Some(context),
                    primary: context_index == 0,
                }),
        );
    }
    display_lines
}

fn definition_context_lines(definition: &UnlockDefinition) -> Vec<ContextDisplayLine<'_>> {
    let mut lines = Vec::<ContextDisplayLine<'_>>::new();
    for context in &definition.tested_by {
        let paths = if context.paths.is_empty() {
            vec![Vec::new()]
        } else {
            let mut paths = Vec::new();
            for raw_path in &context.paths {
                let path = normalize_context_path(raw_path);
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
            paths
        };
        let name = if context.name.trim().is_empty() {
            progression_type_label(&context.type_name).to_owned()
        } else {
            context.name.trim().to_owned()
        };
        if name.is_empty() && paths.iter().all(Vec::is_empty) {
            continue;
        }
        for path in paths {
            if let Some(line) = lines
                .iter_mut()
                .find(|line| line.name == name && line.path == path)
            {
                if !line
                    .contexts
                    .iter()
                    .any(|existing| existing.kind == context.kind && existing.hash == context.hash)
                {
                    line.contexts.push(context);
                }
            } else {
                lines.push(ContextDisplayLine {
                    name: name.clone(),
                    path,
                    contexts: vec![context],
                });
            }
        }
    }
    lines.sort_by_cached_key(|line| line.text().to_lowercase());
    lines
}

fn progression_type_label(type_name: &str) -> &str {
    let type_name = type_name.trim();
    if type_name.eq_ignore_ascii_case("General inventory") {
        ""
    } else {
        type_name
    }
}

fn context_relation_tooltip(context: &ContextDisplayLine<'_>) -> String {
    let mut lines = vec!["Package condition reference".to_owned()];
    if !context.name.is_empty() {
        lines.push(format!("Name: {}", context.name));
    }
    if !context.path.is_empty() {
        lines.push(format!("Path: {}", context.path.join(" > ")));
    }
    let mut type_names = Vec::new();
    let mut descriptions = Vec::new();
    for reference in &context.contexts {
        lines.push(format!(
            "{}: 0x{:08X}",
            progression_context_kind_label(reference.kind),
            reference.hash
        ));
        let type_name = reference.type_name.trim();
        if !type_name.is_empty() && !type_names.contains(&type_name) {
            type_names.push(type_name);
        }
        let description = reference.description.trim();
        if !description.is_empty() && !descriptions.contains(&description) {
            descriptions.push(description);
        }
    }
    lines.extend(type_names.into_iter().map(|value| format!("Type: {value}")));
    lines.extend(
        descriptions
            .into_iter()
            .map(|value| format!("Description: {value}")),
    );
    lines.join("\n")
}

const fn progression_context_kind_label(kind: ProgressionContextKind) -> &'static str {
    match kind {
        ProgressionContextKind::InventoryItem => "Inventory item",
        ProgressionContextKind::Collectible => "Collectible",
        ProgressionContextKind::Record => "Record",
        ProgressionContextKind::Objective => "Objective",
        ProgressionContextKind::PresentationNode => "Presentation node",
        ProgressionContextKind::Activity => "Activity",
        ProgressionContextKind::ActivityAvailability => "Activity availability",
        ProgressionContextKind::Location => "Location",
        ProgressionContextKind::LocationRelease => "Location release",
        ProgressionContextKind::ExpressionMapping => "Expression mapping",
    }
}

fn canonical_root(component: &str) -> Option<&'static str> {
    CANONICAL_ROOTS
        .into_iter()
        .find(|root| component.trim().eq_ignore_ascii_case(root))
}

fn normalize_hierarchy_path(raw_path: &[String], fallback_root: &str) -> Vec<String> {
    let root = raw_path
        .iter()
        .rev()
        .find_map(|component| canonical_root(component))
        .unwrap_or(fallback_root);
    let mut path = raw_path
        .iter()
        .filter_map(|component| {
            let component = component.trim();
            (!component.is_empty() && !component.eq_ignore_ascii_case(root))
                .then(|| component.to_owned())
        })
        .collect::<Vec<_>>();
    path.reverse();
    path.insert(0, root.to_owned());
    path
}

fn normalize_context_path(raw_path: &[String]) -> Vec<String> {
    if let Some(root) = raw_path
        .iter()
        .rev()
        .find_map(|component| canonical_root(component))
    {
        normalize_hierarchy_path(raw_path, root)
    } else {
        raw_path
            .iter()
            .filter_map(|component| {
                let component = component.trim();
                (!component.is_empty()).then(|| component.to_owned())
            })
            .collect()
    }
}

fn objective_hierarchy_paths(objective: &ObjectiveDef) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    for owner in &objective.owners {
        for raw_path in &owner.paths {
            let path = raw_path
                .iter()
                .rev()
                .filter_map(|component| {
                    let component = component.trim();
                    (!component.is_empty()).then(|| component.to_owned())
                })
                .collect::<Vec<_>>();
            if !path.is_empty() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}

fn definition_hierarchy_paths(definition: &UnlockDefinition) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    for context in meaningful_definition_contexts(definition) {
        for raw_path in &context.paths {
            let path = normalize_context_path(raw_path);
            if !path.is_empty() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}

fn build_objective_hierarchy<'a>(
    rows: &[&'a IndexedValue],
    bank: u8,
    catalog: &'a Catalog,
) -> ObjectiveHierarchy<'a> {
    let mut hierarchy = ObjectiveHierarchy::default();
    for &row in rows {
        let definition = catalog.unlock_value_for_state(bank, row.index);
        let objective = definition.and_then(|(definition_index, _)| {
            catalog.objective_with_index_for_unlock_value(definition_index)
        });
        let leaf = ObjectiveHierarchyLeaf {
            row,
            definition_index: definition.map(|(definition_index, _)| definition_index),
            definition: definition.map(|(_, definition)| definition),
            objective_index: objective.map(|(objective_index, _)| objective_index),
            objective: objective.map(|(_, objective)| objective),
        };
        let mut paths = objective
            .map(|(_, objective)| objective_hierarchy_paths(objective))
            .unwrap_or_default();
        if paths.is_empty() {
            paths = definition
                .map(|(_, definition)| definition_hierarchy_paths(definition))
                .unwrap_or_default();
        }
        if paths.is_empty() {
            hierarchy.leaves.push(leaf);
        } else {
            for path in paths {
                insert_objective_leaf(&mut hierarchy.branches, &path, leaf);
            }
        }
    }
    hierarchy
}

fn insert_objective_leaf<'a>(
    branches: &mut Vec<ObjectiveHierarchyBranch<'a>>,
    path: &[String],
    leaf: ObjectiveHierarchyLeaf<'a>,
) {
    fn insert_at<'a>(
        branches: &mut Vec<ObjectiveHierarchyBranch<'a>>,
        path: &[String],
        depth: usize,
        leaf: ObjectiveHierarchyLeaf<'a>,
    ) {
        let label = &path[depth];
        let position = branches
            .iter()
            .position(|branch| branch.label == *label)
            .unwrap_or_else(|| {
                let branch_path = path[..=depth].to_vec();
                branches.push(ObjectiveHierarchyBranch::new(label.clone(), branch_path));
                branches.len() - 1
            });
        let branch = &mut branches[position];
        if depth + 1 == path.len() {
            branch.leaves.push(leaf);
        } else {
            insert_at(&mut branch.children, path, depth + 1, leaf);
        }
    }

    if !path.is_empty() {
        insert_at(branches, path, 0, leaf);
    }
}

fn canonical_root_position(label: &str) -> usize {
    CANONICAL_ROOTS
        .iter()
        .position(|root| label.eq_ignore_ascii_case(root))
        .unwrap_or(CANONICAL_ROOTS.len())
}

fn sort_objective_hierarchy(hierarchy: &mut ObjectiveHierarchy<'_>, sort: TableSort) {
    fn sort_branch(branch: &mut ObjectiveHierarchyBranch<'_>, sort: TableSort) {
        branch
            .children
            .sort_by_cached_key(|child| child.label.to_lowercase());
        sort_objective_leaves(&mut branch.leaves, sort);
        for child in &mut branch.children {
            sort_branch(child, sort);
        }
    }

    sort_objective_leaves(&mut hierarchy.leaves, sort);
    hierarchy.branches.sort_by(|left, right| {
        canonical_root_position(&left.label)
            .cmp(&canonical_root_position(&right.label))
            .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
    });
    for branch in &mut hierarchy.branches {
        sort_branch(branch, sort);
    }
}

fn objective_matrix_lines<'tree, 'data>(
    hierarchy: &'tree ObjectiveHierarchy<'data>,
    table: &'static str,
    state: &UiState,
    auto_expand: bool,
) -> Vec<ObjectiveMatrixLine<'tree, 'data>> {
    fn append<'tree, 'data>(
        output: &mut Vec<ObjectiveMatrixLine<'tree, 'data>>,
        branch: &'tree ObjectiveHierarchyBranch<'data>,
        table: &'static str,
        state: &UiState,
        auto_expand: bool,
        depth: usize,
    ) {
        let key = ObjectiveBranchKey {
            table,
            path: branch.path.clone(),
        };
        let expanded = auto_expand
            || state
                .objective_expansion
                .get(&key)
                .copied()
                .unwrap_or(depth == 0);
        output.push(ObjectiveMatrixLine::Branch {
            branch,
            depth,
            expanded,
        });
        if !expanded {
            return;
        }
        output.extend(branch.leaves.iter().map(|leaf| ObjectiveMatrixLine::Leaf {
            leaf,
            depth: depth + 1,
        }));
        for child in &branch.children {
            append(output, child, table, state, auto_expand, depth + 1);
        }
    }

    let mut output = Vec::new();
    for branch in &hierarchy.branches {
        append(&mut output, branch, table, state, auto_expand, 0);
    }
    output.extend(
        hierarchy
            .leaves
            .iter()
            .map(|leaf| ObjectiveMatrixLine::Leaf { leaf, depth: 0 }),
    );
    output
}

fn compare_ordering(ordering: Ordering, descending: bool) -> Ordering {
    if descending {
        ordering.reverse()
    } else {
        ordering
    }
}

fn compare_optional<T: Ord>(left: Option<T>, right: Option<T>, descending: bool) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => compare_ordering(left.cmp(&right), descending),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn sort_by_optional_cached_key<T, K: Ord>(
    rows: &mut [T],
    descending: bool,
    mut key: impl FnMut(&T) -> Option<K>,
) {
    if descending {
        rows.sort_by_cached_key(|row| {
            let key = key(row);
            (key.is_none(), Reverse(key))
        });
    } else {
        rows.sort_by_cached_key(|row| {
            let key = key(row);
            (key.is_none(), key)
        });
    }
}

fn sort_objective_leaves(rows: &mut [ObjectiveHierarchyLeaf<'_>], sort: TableSort) {
    match sort.column {
        0 => sort_by_optional_cached_key(rows, sort.descending, |leaf| {
            leaf.objective
                .map(|objective| objective_table_text(objective, leaf.definition).to_lowercase())
        }),
        1 => sort_by_optional_cached_key(rows, sort.descending, |leaf| {
            leaf.objective
                .and_then(objective_traits_text)
                .map(|traits| traits.to_lowercase())
        }),
        _ => rows.sort_by(|left, right| compare_objective_leaves(left, right, sort)),
    }
}

fn definition_context_sort_key(definition: &UnlockDefinition) -> Option<String> {
    definition_context_lines(definition)
        .into_iter()
        .map(|line| line.text().to_lowercase())
        .next()
}

fn compare_flag_slots(
    left: usize,
    right: usize,
    bank: u8,
    catalog: &Catalog,
    sort: TableSort,
) -> Ordering {
    let left_definition = catalog.unlock_flag_for_state(bank, left);
    let right_definition = catalog.unlock_flag_for_state(bank, right);
    match sort.column {
        0 => compare_optional(
            left_definition.map(|(index, _)| index),
            right_definition.map(|(index, _)| index),
            sort.descending,
        ),
        1 => compare_optional(
            left_definition.map(|(_, definition)| definition.hash),
            right_definition.map(|(_, definition)| definition.hash),
            sort.descending,
        ),
        2 => compare_optional(
            left_definition.and_then(|(_, definition)| definition_context_sort_key(definition)),
            right_definition.and_then(|(_, definition)| definition_context_sort_key(definition)),
            sort.descending,
        ),
        3 => compare_ordering(left.cmp(&right), sort.descending),
        _ => Ordering::Equal,
    }
}

fn compare_flag_overrides(
    left: &FlagOverride,
    right: &FlagOverride,
    catalog: &Catalog,
    sort: TableSort,
) -> Ordering {
    let left_definition = catalog.unlock_flag_definition(left.definition_index);
    let right_definition = catalog.unlock_flag_definition(right.definition_index);
    match sort.column {
        0 => compare_ordering(
            left.definition_index.cmp(&right.definition_index),
            sort.descending,
        ),
        1 => compare_optional(
            left_definition.map(|definition| definition.hash),
            right_definition.map(|definition| definition.hash),
            sort.descending,
        ),
        2 => compare_ordering(left.value.cmp(&right.value), sort.descending),
        3 => compare_optional(
            left_definition.map(|definition| override_meaning(definition).to_lowercase()),
            right_definition.map(|definition| override_meaning(definition).to_lowercase()),
            sort.descending,
        ),
        _ => Ordering::Equal,
    }
}

fn compare_value_overrides(
    left: &ValueOverride,
    right: &ValueOverride,
    catalog: &Catalog,
    sort: TableSort,
) -> Ordering {
    let left_definition = catalog.unlock_value_definition(left.definition_index);
    let right_definition = catalog.unlock_value_definition(right.definition_index);
    match sort.column {
        0 => compare_ordering(
            left.definition_index.cmp(&right.definition_index),
            sort.descending,
        ),
        1 => compare_optional(
            left_definition.map(|definition| definition.hash),
            right_definition.map(|definition| definition.hash),
            sort.descending,
        ),
        2 => compare_ordering(left.value.cmp(&right.value), sort.descending),
        3 => compare_optional(
            left_definition.map(|definition| override_meaning(definition).to_lowercase()),
            right_definition.map(|definition| override_meaning(definition).to_lowercase()),
            sort.descending,
        ),
        _ => Ordering::Equal,
    }
}

fn compare_objective_leaves(
    left: &ObjectiveHierarchyLeaf<'_>,
    right: &ObjectiveHierarchyLeaf<'_>,
    sort: TableSort,
) -> Ordering {
    match sort.column {
        0 => compare_optional(
            left.objective
                .map(|objective| objective_table_text(objective, left.definition).to_lowercase()),
            right
                .objective
                .map(|objective| objective_table_text(objective, right.definition).to_lowercase()),
            sort.descending,
        ),
        1 => compare_optional(left.objective_index, right.objective_index, sort.descending),
        2 => compare_optional(
            left.objective.map(|objective| objective.hash),
            right.objective.map(|objective| objective.hash),
            sort.descending,
        ),
        3 => compare_ordering(left.row.value.cmp(&right.row.value), sort.descending),
        4 => compare_ordering(left.row.index.cmp(&right.row.index), sort.descending),
        _ => Ordering::Equal,
    }
}

fn objective_description(objective: &crate::catalog::ObjectiveDef) -> String {
    if objective.description.trim().is_empty() {
        format!("0x{:08X}", objective.hash)
    } else {
        objective.description.clone()
    }
}

fn preferred_objective_owner(objective: &ObjectiveDef) -> Option<&ObjectiveOwnerDef> {
    objective
        .owners
        .iter()
        .filter(|owner| objective_owner_label(owner).is_some())
        .min_by_key(|owner| objective_owner_priority(owner.kind))
}

fn objective_owner_label(owner: &ObjectiveOwnerDef) -> Option<&str> {
    let name = owner.name.trim();
    if !name.is_empty() {
        return Some(name);
    }
    let type_name = progression_type_label(&owner.type_name);
    (!type_name.is_empty()).then_some(type_name)
}

fn objective_owner_trait_label(trait_definition: &ObjectiveOwnerTraitDef) -> String {
    let name = trait_definition.name.trim();
    if name.is_empty() {
        format!("0x{:08X}", trait_definition.hash)
    } else {
        name.to_owned()
    }
}

fn objective_owner_display_label(owner: &ObjectiveOwnerDef) -> Option<String> {
    let label = objective_owner_label(owner)?;
    Some(label.to_owned())
}

fn objective_traits_text(objective: &ObjectiveDef) -> Option<String> {
    let owner = preferred_objective_owner(objective)?;
    (!owner.traits.is_empty()).then(|| {
        owner
            .traits
            .iter()
            .map(objective_owner_trait_label)
            .collect::<Vec<_>>()
            .join(", ")
    })
}

const fn objective_owner_priority(kind: ObjectiveOwnerKind) -> u8 {
    match kind {
        ObjectiveOwnerKind::Milestone => 0,
        ObjectiveOwnerKind::Metric => 1,
        ObjectiveOwnerKind::Record => 2,
        ObjectiveOwnerKind::PresentationNode => 3,
        ObjectiveOwnerKind::InventoryItem => 4,
    }
}

fn objective_owner_type(owner: &ObjectiveOwnerDef) -> &str {
    if !owner.type_name.trim().is_empty() {
        owner.type_name.as_str()
    } else {
        match owner.kind {
            ObjectiveOwnerKind::InventoryItem => "Item",
            ObjectiveOwnerKind::Milestone => "Milestone",
            ObjectiveOwnerKind::Metric => "Metric",
            ObjectiveOwnerKind::Record => "Record",
            ObjectiveOwnerKind::PresentationNode => "Presentation node",
        }
    }
}

fn objective_goal_text(objective: &ObjectiveDef) -> String {
    let description = objective_description(objective);
    let Some(owner) = preferred_objective_owner(objective) else {
        return description;
    };
    let Some(owner_label) = objective_owner_display_label(owner) else {
        return description;
    };
    if owner_label.eq_ignore_ascii_case(description.trim()) {
        return description;
    }
    if objective.description.trim().is_empty() && !owner.name.trim().is_empty() {
        owner_label
    } else {
        format!("{owner_label}: {description}")
    }
}

fn meaningful_definition_contexts(definition: &UnlockDefinition) -> Vec<&ProgressionContextDef> {
    definition
        .tested_by
        .iter()
        .filter(|context| {
            context.kind != ProgressionContextKind::Objective
                && (!context.name.trim().is_empty()
                    || !progression_type_label(&context.type_name).is_empty()
                    || !context.description.trim().is_empty()
                    || context
                        .paths
                        .iter()
                        .any(|path| path.iter().any(|component| !component.trim().is_empty())))
        })
        .collect()
}

fn override_meaning_contexts(definition: &UnlockDefinition) -> Vec<&ProgressionContextDef> {
    definition
        .tested_by
        .iter()
        .filter(|context| {
            !context.name.trim().is_empty()
                || !progression_type_label(&context.type_name).is_empty()
                || !context.description.trim().is_empty()
                || context
                    .paths
                    .iter()
                    .any(|path| path.iter().any(|component| !component.trim().is_empty()))
        })
        .collect()
}

fn override_meaning(definition: &UnlockDefinition) -> String {
    if let Some(name) = definition_name(definition) {
        return name.trim().to_owned();
    }

    let contexts = override_meaning_contexts(definition);
    let mut labels = contexts
        .iter()
        .filter_map(|context| definition_context_label(context))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    labels.sort_by_key(|label| label.to_lowercase());
    labels.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    match labels.len() {
        1 => return labels.remove(0),
        2 => return labels.join(" · "),
        count if count > 2 => {
            return format!("{} · {} · +{} more", labels[0], labels[1], count - 2);
        }
        _ => {}
    }
    if contexts.is_empty() {
        return "Reader not resolved".to_owned();
    }

    let mut kinds = contexts
        .iter()
        .map(|context| context.kind)
        .collect::<Vec<_>>();
    kinds.sort_by_key(|kind| objective_context_priority(*kind));
    kinds.dedup();
    if kinds.len() == 1 {
        format!("{} conditions", progression_context_kind_label(kinds[0]))
    } else {
        let mut labels = kinds
            .iter()
            .take(2)
            .map(|kind| progression_context_kind_label(*kind).to_owned())
            .collect::<Vec<_>>();
        if kinds.len() > 2 {
            labels.push(format!("+{} more", kinds.len() - 2));
        }
        labels.join(" · ")
    }
}

const fn objective_context_priority(kind: ProgressionContextKind) -> u8 {
    match kind {
        ProgressionContextKind::PresentationNode => 0,
        ProgressionContextKind::Record => 1,
        ProgressionContextKind::Collectible => 2,
        ProgressionContextKind::InventoryItem => 3,
        ProgressionContextKind::ActivityAvailability => 4,
        ProgressionContextKind::Activity => 5,
        ProgressionContextKind::LocationRelease => 6,
        ProgressionContextKind::Location => 7,
        ProgressionContextKind::ExpressionMapping => 8,
        ProgressionContextKind::Objective => 9,
    }
}

fn definition_context_label(context: &ProgressionContextDef) -> Option<&str> {
    let name = context.name.trim();
    if !name.is_empty() {
        return Some(name);
    }
    let type_name = progression_type_label(&context.type_name);
    if !type_name.is_empty() {
        return Some(type_name);
    }
    let description = context.description.trim();
    if !description.is_empty() {
        return Some(description);
    }
    context
        .paths
        .iter()
        .flat_map(|path| path.iter())
        .map(|component| component.trim())
        .find(|component| !component.is_empty())
}

fn resolved_objective_table_text(
    catalog: &Catalog,
    objective: &ObjectiveDef,
    definition: Option<&UnlockDefinition>,
) -> String {
    if !objective.description.trim().is_empty() || preferred_objective_owner(objective).is_some() {
        return objective_goal_text(objective);
    }
    if let Some(name) = catalog.display_name(objective.hash) {
        return name.to_owned();
    }
    let mut labels = objective
        .referenced_objective_indices
        .iter()
        .filter_map(|index| catalog.objective_definition(usize::from(*index)))
        .map(|target| objective_table_text(target, None))
        .filter(|label| !label.starts_with("Objective 0x"))
        .collect::<Vec<_>>();
    labels.dedup();
    if let Some(label) = labels.first() {
        return if labels.len() == 1 {
            format!("{label} · linked objective")
        } else {
            format!("{label} · +{} linked", labels.len() - 1)
        };
    }
    objective_table_text(objective, definition)
}

fn resolved_objective_hierarchy_paths(
    catalog: &Catalog,
    objective: &ObjectiveDef,
) -> Vec<Vec<String>> {
    let direct = objective_hierarchy_paths(objective);
    if !direct.is_empty() {
        return direct;
    }
    let mut paths = objective
        .referenced_objective_indices
        .iter()
        .filter_map(|index| catalog.objective_definition(usize::from(*index)))
        .flat_map(objective_hierarchy_paths)
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

fn objective_table_text(objective: &ObjectiveDef, definition: Option<&UnlockDefinition>) -> String {
    if !objective.description.trim().is_empty() || preferred_objective_owner(objective).is_some() {
        return objective_goal_text(objective);
    }

    let context = definition.and_then(|definition| {
        meaningful_definition_contexts(definition)
            .into_iter()
            .min_by_key(|context| objective_context_priority(context.kind))
    });
    if let Some(label) = context.and_then(definition_context_label) {
        format!("{label} · objective 0x{:08X}", objective.hash)
    } else if let Some(index) = objective.related_unlock_value_definition_index {
        format!(
            "Objective 0x{:08X} · value definition #{index}",
            objective.hash
        )
    } else {
        format!("Objective 0x{:08X}", objective.hash)
    }
}

fn objective_details_tooltip(objective: &ObjectiveDef) -> String {
    let mut lines = vec![
        format!("Objective: {}", objective_description(objective)),
        format!("Objective hash: 0x{:08X}", objective.hash),
    ];
    for (label, value) in [
        ("Name", objective.name.as_str()),
        (
            "Display description",
            objective.display_description.as_str(),
        ),
        (
            "Progress description",
            objective.progress_description.as_str(),
        ),
    ] {
        let value = value.trim();
        if !value.is_empty() && !value.eq_ignore_ascii_case(objective.description.trim()) {
            lines.push(format!("{label}: {value}"));
        }
    }
    if objective
        .owners
        .iter()
        .any(|owner| objective_owner_label(owner).is_some())
    {
        lines.push("Package owners:".into());
        for owner in objective
            .owners
            .iter()
            .filter(|owner| objective_owner_label(owner).is_some())
        {
            let owner_type = objective_owner_type(owner);
            let owner_label = objective_owner_label(owner).unwrap_or(owner_type);
            if owner_type.eq_ignore_ascii_case(owner_label) {
                lines.push(owner_label.to_owned());
            } else {
                lines.push(format!("{owner_type}: {owner_label}"));
            }
            lines.push(format!("{owner_type} hash: 0x{:08X}", owner.hash));
            let description = owner.description.trim();
            if !description.is_empty() && !description.eq_ignore_ascii_case(owner_label) {
                lines.push(format!("Description: {description}"));
            }
        }
    }
    lines.join("\n")
}

#[cfg(test)]
fn objective_traits_tooltip(objective: &ObjectiveDef) -> String {
    let Some(owner) = preferred_objective_owner(objective) else {
        return "No package traits".into();
    };
    if owner.traits.is_empty() {
        return "No package traits".into();
    }
    let mut lines = Vec::new();
    for trait_definition in &owner.traits {
        let trait_label = objective_owner_trait_label(trait_definition);
        lines.push(format!("{trait_label}: 0x{:08X}", trait_definition.hash));
        let description = trait_definition.description.trim();
        if !description.is_empty() && !description.eq_ignore_ascii_case(&trait_label) {
            lines.push(format!("{trait_label}: {description}"));
        }
    }
    lines.join("\n")
}

fn objective_target_text(objective: &crate::catalog::ObjectiveDef) -> String {
    let target = objective.completion_value;
    if objective.maximum_value().is_some() {
        format!("{target} max")
    } else if objective.minimum_value().is_some() {
        format!("{target} min")
    } else if objective.is_counting_downward {
        format!("≤{target}")
    } else {
        format!("≥{target}")
    }
}

#[cfg(test)]
fn objective_target_tooltip(objective: &crate::catalog::ObjectiveDef) -> String {
    let target = objective.completion_value;
    let counts_downward = if objective.is_counting_downward {
        "yes"
    } else {
        "no"
    };
    let overcompletion = if objective.allow_overcompletion {
        "allowed"
    } else {
        "not allowed"
    };
    let negative = if objective.allow_negative_value {
        "allowed"
    } else {
        "not allowed"
    };
    let completed_changes = if objective.allow_value_change_when_completed {
        "allowed"
    } else {
        "not allowed"
    };
    format!(
        "Completion value: {target}\nCounts downward: {counts_downward}\nOver-completion: {overcompletion}\nNegative values: {negative}\nChanges after completion: {completed_changes}"
    )
}

fn flag_slot_matches(query: &str, slot: usize, bank: u8, catalog: &Catalog) -> bool {
    if query.is_empty() || slot.to_string().contains(query) {
        return true;
    }
    catalog
        .unlock_flag_for_state(bank, slot)
        .is_some_and(|(index, definition)| definition_matches(query, index, definition))
}

fn objective_value_matches(query: &str, row: &IndexedValue, bank: u8, catalog: &Catalog) -> bool {
    if query.is_empty()
        || row.index.to_string().contains(query)
        || row.value.to_string().contains(query)
    {
        return true;
    }
    let Some((definition_index, definition)) = catalog.unlock_value_for_state(bank, row.index)
    else {
        return false;
    };
    definition_matches(query, definition_index, definition)
        || catalog
            .objective_for_unlock_value(definition_index)
            .is_some_and(|objective| resolved_objective_matches(catalog, query, objective))
}

fn objective_hierarchy_row_matches(
    query: &str,
    row: &IndexedValue,
    bank: u8,
    catalog: &Catalog,
) -> bool {
    if objective_value_matches(query, row, bank, catalog) {
        return true;
    }
    let definition = catalog.unlock_value_for_state(bank, row.index);
    let objective = definition
        .and_then(|(definition_index, _)| catalog.objective_for_unlock_value(definition_index));
    match objective {
        Some(objective) => {
            let mut paths = resolved_objective_hierarchy_paths(catalog, objective);
            if paths.is_empty() {
                paths = definition
                    .map(|(_, definition)| definition_hierarchy_paths(definition))
                    .unwrap_or_default();
            }
            paths.iter().any(|path| {
                path.iter()
                    .any(|component| component.to_lowercase().contains(query))
            })
        }
        None => false,
    }
}

fn family5_flag_matches(query: &str, row: &FlagOverride, catalog: &Catalog) -> bool {
    query.is_empty()
        || row.definition_index.to_string().contains(query)
        || row.value.to_string().contains(query)
        || catalog
            .unlock_flag_definition(row.definition_index)
            .is_some_and(|definition| definition_matches(query, row.definition_index, definition))
}

fn family5_value_matches(query: &str, row: &ValueOverride, catalog: &Catalog) -> bool {
    query.is_empty()
        || row.definition_index.to_string().contains(query)
        || row.value.to_string().contains(query)
        || catalog
            .unlock_value_definition(row.definition_index)
            .is_some_and(|definition| definition_matches(query, row.definition_index, definition))
        || catalog
            .objective_for_unlock_value(row.definition_index)
            .is_some_and(|objective| resolved_objective_matches(catalog, query, objective))
}

fn resolved_objective_matches(catalog: &Catalog, query: &str, objective: &ObjectiveDef) -> bool {
    objective_matches(query, objective)
        || objective
            .referenced_objective_indices
            .iter()
            .filter_map(|index| catalog.objective_definition(usize::from(*index)))
            .any(|target| objective_matches(query, target))
}

fn objective_matches(query: &str, objective: &ObjectiveDef) -> bool {
    objective.description.to_lowercase().contains(query)
        || objective.name.to_lowercase().contains(query)
        || objective.display_description.to_lowercase().contains(query)
        || objective
            .progress_description
            .to_lowercase()
            .contains(query)
        || formatted_hash_matches(query, objective.hash)
        || objective.completion_value.to_string().contains(query)
        || (objective.maximum_value().is_some() && "maximum max capped".contains(query))
        || (objective.minimum_value().is_some() && "minimum min capped".contains(query))
        || (objective.allow_overcompletion
            && "overcompletion threshold no maximum no minimum".contains(query))
        || objective
            .intrinsic_perk_flag_definition_indices
            .iter()
            .any(|index| index.to_string().contains(query))
        || objective.condition_programs.iter().flatten().any(|token| {
            token[0].to_string().contains(query) || token[1].to_string().contains(query)
        })
        || objective.owners.iter().any(|owner| {
            owner.name.to_lowercase().contains(query)
                || owner.description.to_lowercase().contains(query)
                || objective_owner_type(owner).to_lowercase().contains(query)
                || formatted_hash_matches(query, owner.hash)
                || owner.traits.iter().any(|trait_definition| {
                    trait_definition.name.to_lowercase().contains(query)
                        || trait_definition.description.to_lowercase().contains(query)
                        || formatted_hash_matches(query, trait_definition.hash)
                })
                || owner
                    .paths
                    .iter()
                    .flatten()
                    .any(|part| part.to_lowercase().contains(query))
        })
}

fn definition_matches(query: &str, index: usize, definition: &UnlockDefinition) -> bool {
    index.to_string().contains(query)
        || format!("#{index}").contains(query)
        || formatted_hash_matches(query, definition.hash)
        || definition_name(definition).is_some_and(|name| name.to_lowercase().contains(query))
        || definition
            .description
            .as_deref()
            .is_some_and(|description| description.to_lowercase().contains(query))
        || definition
            .tested_by
            .iter()
            .any(|context| progression_context_matches(query, context))
}

fn progression_context_matches(query: &str, context: &ProgressionContextDef) -> bool {
    formatted_hash_matches(query, context.hash)
        || context.name.to_lowercase().contains(query)
        || context.type_name.to_lowercase().contains(query)
        || context.description.to_lowercase().contains(query)
        || progression_context_kind_label(context.kind)
            .to_lowercase()
            .contains(query)
        || context
            .paths
            .iter()
            .flatten()
            .any(|component| component.to_lowercase().contains(query))
        || context.condition_programs.iter().flatten().any(|token| {
            token[0].to_string().contains(query) || token[1].to_string().contains(query)
        })
}

fn formatted_hash_matches(query: &str, hash: u64) -> bool {
    format!("{hash:08x}").contains(query) || format!("0x{hash:08x}").contains(query)
}

fn expanded_flag_slots(rows: &[FlagRun], capacity: usize) -> Vec<usize> {
    let mut flags = vec![false; capacity];
    for row in rows {
        flags[row.start..row.start + row.length].fill(true);
    }
    flags
        .into_iter()
        .enumerate()
        .filter_map(|(slot, set)| set.then_some(slot))
        .collect()
}

fn unlocks_object_mut(document: &mut Value) -> Option<&mut Map<String, Value>> {
    let root = document.as_object_mut()?;
    let state = root
        .entry("state")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()?;
    state
        .entry("unlocks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
}

fn write_unlock_array(document: &mut Value, key: &str, rows: Vec<Value>) -> bool {
    let Some(unlocks) = unlocks_object_mut(document) else {
        return false;
    };
    unlocks.insert(key.to_owned(), Value::Array(rows));
    true
}

fn investment_object_mut(document: &mut Value) -> Option<&mut Map<String, Value>> {
    let root = document.as_object_mut()?;
    let state = root
        .entry("state")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()?;
    state
        .entry("investment")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
}

fn write_investment_array(document: &mut Value, key: &str, rows: Vec<Value>) -> bool {
    let Some(investment) = investment_object_mut(document) else {
        return false;
    };
    investment.insert(key.to_owned(), Value::Array(rows));
    true
}

fn undo_investment_change(document: &mut Value, state: &mut UiState) -> bool {
    let Some(change) = state.last_investment_change.take() else {
        return false;
    };
    let changed = match change {
        InvestmentUndo::Flag {
            definition_index,
            previous: Some(value),
        } => set_investment_override(
            document,
            InvestmentTable::FlagOverrides,
            definition_index,
            i32::from(value),
        ),
        InvestmentUndo::Flag {
            definition_index,
            previous: None,
        } => remove_investment_override(document, InvestmentTable::FlagOverrides, definition_index),
        InvestmentUndo::Value {
            definition_index,
            previous: Some(value),
        } => set_investment_override(
            document,
            InvestmentTable::ValueOverrides,
            definition_index,
            value,
        ),
        InvestmentUndo::Value {
            definition_index,
            previous: None,
        } => {
            remove_investment_override(document, InvestmentTable::ValueOverrides, definition_index)
        }
    };
    if !changed {
        state.last_investment_change = Some(change);
    }
    changed
}

fn set_investment_override(
    document: &mut Value,
    table: InvestmentTable,
    definition_index: usize,
    value: i32,
) -> bool {
    let Ok(policy) = parse_investment(document.pointer("/state/investment")) else {
        return false;
    };
    match table {
        InvestmentTable::FlagOverrides => {
            if definition_index > FAMILY5_FLAG_SLOT_MAXIMUM
                || !(0..=i32::from(FAMILY5_FLAG_VALUE_MAXIMUM)).contains(&value)
            {
                return false;
            }
            let mut rows = policy.flag_overrides;
            if let Some(row) = rows
                .iter_mut()
                .find(|row| row.definition_index == definition_index)
            {
                if i32::from(row.value) == value {
                    return false;
                }
                row.value = value as u8;
            } else {
                if rows.len() >= FAMILY5_OVERRIDE_CAPACITY {
                    return false;
                }
                rows.push(FlagOverride {
                    definition_index,
                    value: value as u8,
                });
            }
            rows.sort_by_key(|row| row.definition_index);
            write_investment_array(
                document,
                "family5_flag_overrides",
                rows.into_iter()
                    .map(|row| serde_json::json!([row.definition_index, row.value]))
                    .collect(),
            )
        }
        InvestmentTable::ValueOverrides => {
            if definition_index > FAMILY5_VALUE_SLOT_MAXIMUM {
                return false;
            }
            let mut rows = policy.value_overrides;
            if let Some(row) = rows
                .iter_mut()
                .find(|row| row.definition_index == definition_index)
            {
                if row.value == value {
                    return false;
                }
                row.value = value;
            } else {
                if rows.len() >= FAMILY5_OVERRIDE_CAPACITY {
                    return false;
                }
                rows.push(ValueOverride {
                    definition_index,
                    value,
                });
            }
            rows.sort_by_key(|row| row.definition_index);
            write_investment_array(
                document,
                "family5_value_overrides",
                rows.into_iter()
                    .map(|row| serde_json::json!([row.definition_index, row.value]))
                    .collect(),
            )
        }
    }
}

fn remove_investment_override(
    document: &mut Value,
    table: InvestmentTable,
    definition_index: usize,
) -> bool {
    let Ok(policy) = parse_investment(document.pointer("/state/investment")) else {
        return false;
    };
    match table {
        InvestmentTable::FlagOverrides => {
            let mut rows = policy.flag_overrides;
            let prior_len = rows.len();
            rows.retain(|row| row.definition_index != definition_index);
            if rows.len() == prior_len {
                return false;
            }
            write_investment_array(
                document,
                "family5_flag_overrides",
                rows.into_iter()
                    .map(|row| serde_json::json!([row.definition_index, row.value]))
                    .collect(),
            )
        }
        InvestmentTable::ValueOverrides => {
            let mut rows = policy.value_overrides;
            let prior_len = rows.len();
            rows.retain(|row| row.definition_index != definition_index);
            if rows.len() == prior_len {
                return false;
            }
            write_investment_array(
                document,
                "family5_value_overrides",
                rows.into_iter()
                    .map(|row| serde_json::json!([row.definition_index, row.value]))
                    .collect(),
            )
        }
    }
}

fn flag_table_key(id: &str) -> Option<(&'static str, usize, bool)> {
    match id {
        "account_flag_runs" => Some(("account_flag_runs", ACCOUNT_FLAG_CAPACITY, true)),
        "profile_flag_runs" => Some(("profile_flag_runs", PROFILE_FLAG_CAPACITY, true)),
        "character_flags" => Some(("character_flags", CHARACTER_FLAG_CAPACITY, false)),
        "character_object_flag_runs" => {
            Some(("character_flag_runs", CHARACTER_OBJECT_FLAG_CAPACITY, true))
        }
        _ => None,
    }
}

fn value_table_key(id: &str) -> Option<(&'static str, usize)> {
    match id {
        "objective_values" => Some(("objective_values", OBJECTIVE_VALUE_CAPACITY)),
        "character_object_objective_values" => Some((
            "character_objective_values",
            CHARACTER_OBJECT_VALUE_CAPACITY,
        )),
        _ => None,
    }
}

fn progression_table_key(id: &str) -> Option<&'static str> {
    match id {
        "account_progressions" => Some("account_progressions"),
        "character_progressions" => Some("character_progressions"),
        _ => None,
    }
}

fn set_progression_value(
    document: &mut Value,
    id: &str,
    definition_index: usize,
    lanes: [i32; 3],
) -> bool {
    let Some(key) = progression_table_key(id) else {
        return false;
    };
    if definition_index >= PROGRESSION_DEFINITION_CAPACITY {
        return false;
    }
    let Ok(current) = parse_unlocks(document.pointer("/state/unlocks")) else {
        return false;
    };
    let mut values = match id {
        "account_progressions" => current.account_progressions,
        "character_progressions" => current.character_progressions,
        _ => return false,
    };
    if let Some(row) = values
        .iter_mut()
        .find(|row| row.definition_index == definition_index)
    {
        if row.lanes == lanes {
            return false;
        }
        row.lanes = lanes;
    } else {
        values.push(ProgressionValue {
            definition_index,
            lanes,
        });
    }
    values.sort_by_key(|row| row.definition_index);
    write_unlock_array(
        document,
        key,
        values
            .into_iter()
            .map(|row| {
                serde_json::json!([
                    row.definition_index,
                    row.lanes[0],
                    row.lanes[1],
                    row.lanes[2]
                ])
            })
            .collect(),
    )
}

fn remove_progression_value(document: &mut Value, id: &str, definition_index: usize) -> bool {
    let Some(key) = progression_table_key(id) else {
        return false;
    };
    let Ok(current) = parse_unlocks(document.pointer("/state/unlocks")) else {
        return false;
    };
    let mut values = match id {
        "account_progressions" => current.account_progressions,
        "character_progressions" => current.character_progressions,
        _ => return false,
    };
    let prior_len = values.len();
    values.retain(|row| row.definition_index != definition_index);
    if values.len() == prior_len {
        return false;
    }
    write_unlock_array(
        document,
        key,
        values
            .into_iter()
            .map(|row| {
                serde_json::json!([
                    row.definition_index,
                    row.lanes[0],
                    row.lanes[1],
                    row.lanes[2]
                ])
            })
            .collect(),
    )
}

fn set_unlock_flag(document: &mut Value, id: &str, slot: usize, set: bool) -> bool {
    let Some((key, capacity, uses_runs)) = flag_table_key(id) else {
        return false;
    };
    if slot >= capacity {
        return false;
    }
    let Ok(current) = parse_unlocks(document.pointer("/state/unlocks")) else {
        return false;
    };
    let mut slots = match id {
        "account_flag_runs" => expanded_flag_slots(&current.account_flag_runs, capacity),
        "profile_flag_runs" => expanded_flag_slots(&current.profile_flag_runs, capacity),
        "character_flags" => current
            .character_flags
            .into_iter()
            .map(|row| row.index)
            .collect(),
        "character_object_flag_runs" => {
            expanded_flag_slots(&current.character_object_flag_runs, capacity)
        }
        _ => return false,
    };
    slots.sort_unstable();
    slots.dedup();
    match slots.binary_search(&slot) {
        Ok(index) if !set => {
            slots.remove(index);
        }
        Err(index) if set => slots.insert(index, slot),
        _ => return false,
    }
    let rows = if uses_runs {
        compress_flag_slots(&slots)
            .into_iter()
            .map(|run| serde_json::json!([run.start, run.length]))
            .collect()
    } else {
        slots.into_iter().map(Value::from).collect()
    };
    write_unlock_array(document, key, rows)
}

pub(super) fn set_collection_flag(
    document: &mut Value,
    definition_index: usize,
    definition: &UnlockDefinition,
    set: bool,
) -> bool {
    let Some(slot) = definition.compact_slot.map(usize::from) else {
        return set_investment_override(
            document,
            InvestmentTable::FlagOverrides,
            definition_index,
            if set { 2 } else { 0 },
        );
    };
    let table = match definition.bank() {
        ACCOUNT_FLAG_BANK => "account_flag_runs",
        PROFILE_FLAG_BANK => "profile_flag_runs",
        CHARACTER_OBJECT_FLAG_BANK => "character_object_flag_runs",
        CHARACTER_FLAG_BANK => "character_flags",
        _ => return false,
    };
    set_unlock_flag(document, table, slot, set)
}

fn compress_flag_slots(slots: &[usize]) -> Vec<FlagRun> {
    let mut runs = Vec::new();
    let Some(&first) = slots.first() else {
        return runs;
    };
    let mut start = first;
    let mut previous = first;
    for &slot in &slots[1..] {
        if slot == previous.saturating_add(1) {
            previous = slot;
            continue;
        }
        runs.push(FlagRun {
            start,
            length: previous - start + 1,
        });
        start = slot;
        previous = slot;
    }
    runs.push(FlagRun {
        start,
        length: previous - start + 1,
    });
    runs
}

fn set_unlock_value(document: &mut Value, id: &str, slot: usize, value: i32) -> bool {
    let Some((key, capacity)) = value_table_key(id) else {
        return false;
    };
    if slot >= capacity {
        return false;
    }
    let Ok(current) = parse_unlocks(document.pointer("/state/unlocks")) else {
        return false;
    };
    let mut values = match id {
        "objective_values" => current.objective_values,
        "character_object_objective_values" => current.character_objective_values,
        _ => return false,
    };
    if let Some(row) = values.iter_mut().find(|row| row.index == slot) {
        if row.value == value {
            return false;
        }
        row.value = value;
    } else {
        values.push(IndexedValue { index: slot, value });
    }
    values.sort_by_key(|row| row.index);
    write_unlock_array(
        document,
        key,
        values
            .into_iter()
            .map(|row| serde_json::json!([row.index, row.value]))
            .collect(),
    )
}

pub(super) fn set_collection_value(
    document: &mut Value,
    definition_index: usize,
    definition: &UnlockDefinition,
    value: i32,
) -> bool {
    let Some(slot) = definition.compact_slot.map(usize::from) else {
        return set_investment_override(
            document,
            InvestmentTable::ValueOverrides,
            definition_index,
            value,
        );
    };
    let table = match definition.bank() {
        ACCOUNT_OBJECTIVE_BANK => "objective_values",
        CHARACTER_OBJECTIVE_BANK => "character_object_objective_values",
        _ => return false,
    };
    set_unlock_value(document, table, slot, value)
}

fn remove_unlock_value(document: &mut Value, id: &str, slot: usize) -> bool {
    let Some((key, _)) = value_table_key(id) else {
        return false;
    };
    let Ok(current) = parse_unlocks(document.pointer("/state/unlocks")) else {
        return false;
    };
    let mut values = match id {
        "objective_values" => current.objective_values,
        "character_object_objective_values" => current.character_objective_values,
        _ => return false,
    };
    let prior_len = values.len();
    values.retain(|row| row.index != slot);
    if values.len() == prior_len {
        return false;
    }
    write_unlock_array(
        document,
        key,
        values
            .into_iter()
            .map(|row| serde_json::json!([row.index, row.value]))
            .collect(),
    )
}

#[cfg(test)]
fn expanded_flag_count(rows: &[FlagRun], capacity: usize) -> usize {
    expanded_flag_slots(rows, capacity).len()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn metadata_navigation_preserves_and_clears_history() {
        let mut state = UiState::default();
        let first = MetadataSelection::FlagDefinition(7);
        let second = MetadataSelection::ValueDefinition(11);

        state.open_metadata(first);
        state.open_metadata(second);
        assert_eq!(state.metadata_selection, Some(second));
        assert_eq!(state.metadata_history, [first]);

        state.metadata_back();
        assert_eq!(state.metadata_selection, Some(first));
        assert!(state.metadata_history.is_empty());

        state.close_metadata();
        assert_eq!(state.metadata_selection, None);
        assert!(state.metadata_history.is_empty());
    }

    #[test]
    fn inspector_hash_parser_accepts_the_displayed_hash_and_decimal_pair() {
        let displayed = "0x574E0A2A · 1464732202";
        assert_eq!(metadata_hash_hex_text(displayed), "0x574E0A2A");
        assert_eq!(
            parse_metadata_hash(metadata_hash_hex_text(displayed)),
            Some(0x574E_0A2A)
        );
        assert_eq!(parse_metadata_hash("1464732202"), None);
    }

    #[test]
    fn hash_inspector_keeps_a_real_navigation_history() {
        let mut inspection = HashInspectionState::default();
        inspection.open(0);
        assert_eq!(inspection.current, None);

        inspection.open(0x1111_1111);
        inspection.open(0x1111_1111);
        assert_eq!(inspection.current, Some(0x1111_1111));
        assert!(inspection.history.is_empty());

        inspection.open(0x2222_2222);
        inspection.open(0x3333_3333);
        assert_eq!(inspection.current, Some(0x3333_3333));
        assert_eq!(inspection.history, [0x1111_1111, 0x2222_2222]);

        inspection.back();
        assert_eq!(inspection.current, Some(0x2222_2222));
        assert_eq!(inspection.history, [0x1111_1111]);

        inspection.close();
        assert!(!inspection.is_open());
        assert!(inspection.history.is_empty());
    }

    #[test]
    fn zero_hash_requests_are_rejected_before_catalog_lookup() {
        let context = egui::Context::default();
        request_hash_inspection(&context, 0);
        assert_eq!(take_hash_inspection_request(&context), None);

        request_hash_inspection(&context, 0x574E_0A2A);
        assert_eq!(take_hash_inspection_request(&context), Some(0x574E_0A2A));
    }

    #[test]
    fn cached_optional_sort_builds_each_key_once_and_keeps_missing_values_last() {
        let mut rows = vec![Some("bravo"), None, Some("alpha")];
        let mut calls = 0;

        sort_by_optional_cached_key(&mut rows, false, |row| {
            calls += 1;
            row.map(str::to_owned)
        });

        assert_eq!(calls, rows.len());
        assert_eq!(rows, [Some("alpha"), Some("bravo"), None]);

        sort_by_optional_cached_key(&mut rows, true, |row| row.map(str::to_owned));
        assert_eq!(rows, [Some("bravo"), Some("alpha"), None]);
    }

    #[test]
    fn progression_parses_every_sunrise_table_shape() {
        let document = json!({
            "state": {
                "investment": {
                    "family5_flag_overrides": [[2003, 2]],
                    "family5_value_overrides": [[3510, -5]]
                },
                "unlocks": {
                    "account_flag_runs": [[26, 21], [40, 2]],
                    "profile_flag_runs": [[1, 1]],
                    "character_flags": [16, 17],
                    "objective_values": [[58, 5000]],
                    "character_flag_runs": [[61, 1]],
                    "character_objective_values": [[443, -1]],
                    "account_progressions": [[3, 1, 2, 3], [3, 4, 0, 5]],
                    "character_progressions": [[7, -1, 0, 9]],
                    "future_field": {"preserved": true}
                }
            }
        });

        let policy = parse(&document).unwrap();

        assert_eq!(policy.investment.flag_overrides.len(), 1);
        assert_eq!(policy.investment.value_overrides[0].value, -5);
        assert_eq!(policy.unlocks.account_flag_runs.len(), 2);
        assert_eq!(
            expanded_flag_count(&policy.unlocks.account_flag_runs, 64),
            21
        );
        assert_eq!(policy.unlocks.character_flags.len(), 2);
        assert_eq!(policy.unlocks.character_objective_values[0].value, -1);
        assert_eq!(
            policy.unlocks.account_progressions,
            [ProgressionValue {
                definition_index: 3,
                lanes: [4, 2, 5],
            }]
        );
        assert_eq!(
            policy.unlocks.character_progressions,
            [ProgressionValue {
                definition_index: 7,
                lanes: [-1, 0, 9],
            }]
        );
    }

    #[test]
    fn flag_mutations_split_and_rejoin_runs_without_touching_unknown_fields() {
        let mut document = json!({
            "state": {
                "unlocks": {
                    "account_flag_runs": [[1, 3]],
                    "future_field": {"preserved": true}
                }
            }
        });

        assert!(set_unlock_flag(
            &mut document,
            "account_flag_runs",
            2,
            false
        ));
        assert_eq!(
            document.pointer("/state/unlocks/account_flag_runs"),
            Some(&json!([[1, 1], [3, 1]]))
        );
        assert!(set_unlock_flag(&mut document, "account_flag_runs", 2, true));
        assert_eq!(
            document.pointer("/state/unlocks/account_flag_runs"),
            Some(&json!([[1, 3]]))
        );
        assert_eq!(
            document.pointer("/state/unlocks/future_field/preserved"),
            Some(&json!(true))
        );
    }

    #[test]
    fn indexed_flag_and_value_mutations_are_sorted_and_removable() {
        let mut document = json!({
            "state": {"unlocks": {"character_flags": [9, 3], "objective_values": [[8, 1]]}}
        });

        assert!(set_unlock_flag(&mut document, "character_flags", 5, true));
        assert_eq!(
            document.pointer("/state/unlocks/character_flags"),
            Some(&json!([3, 5, 9]))
        );
        assert!(set_unlock_value(&mut document, "objective_values", 4, -7));
        assert!(set_unlock_value(&mut document, "objective_values", 8, 12));
        assert_eq!(
            document.pointer("/state/unlocks/objective_values"),
            Some(&json!([[4, -7], [8, 12]]))
        );
        assert!(remove_unlock_value(&mut document, "objective_values", 4));
        assert_eq!(
            document.pointer("/state/unlocks/objective_values"),
            Some(&json!([[8, 12]]))
        );
    }

    #[test]
    fn progression_mutations_are_sorted_updated_and_removable() {
        let mut document = json!({"state": {"unlocks": {}}});

        assert!(set_progression_value(
            &mut document,
            "account_progressions",
            5,
            [1, 2, 3],
        ));
        assert!(set_progression_value(
            &mut document,
            "account_progressions",
            2,
            [-1, 0, 9],
        ));
        assert!(set_progression_value(
            &mut document,
            "account_progressions",
            5,
            [4, 5, 6],
        ));
        assert_eq!(
            document.pointer("/state/unlocks/account_progressions"),
            Some(&json!([[2, -1, 0, 9], [5, 4, 5, 6]]))
        );
        assert!(remove_progression_value(
            &mut document,
            "account_progressions",
            2,
        ));
        assert_eq!(
            document.pointer("/state/unlocks/account_progressions"),
            Some(&json!([[5, 4, 5, 6]]))
        );
    }

    #[test]
    fn family5_overrides_add_edit_and_remove_the_selected_definition() {
        let mut document = json!({"state": {"investment": {}}});

        assert!(set_investment_override(
            &mut document,
            InvestmentTable::FlagOverrides,
            2003,
            2
        ));
        assert!(set_investment_override(
            &mut document,
            InvestmentTable::ValueOverrides,
            3510,
            -5
        ));
        assert_eq!(
            document.pointer("/state/investment/family5_flag_overrides"),
            Some(&json!([[2003, 2]]))
        );
        assert_eq!(
            document.pointer("/state/investment/family5_value_overrides"),
            Some(&json!([[3510, -5]]))
        );
        assert!(remove_investment_override(
            &mut document,
            InvestmentTable::FlagOverrides,
            2003
        ));
        assert_eq!(
            document.pointer("/state/investment/family5_flag_overrides"),
            Some(&json!([]))
        );
    }

    #[test]
    fn investment_undo_restores_edits_and_removes_new_rows() {
        let mut document = json!({
            "state": {"investment": {
                "family5_flag_overrides": [[2003, 2]],
                "family5_value_overrides": [[3510, 9]]
            }}
        });
        let mut state = UiState {
            last_investment_change: Some(InvestmentUndo::Flag {
                definition_index: 2003,
                previous: None,
            }),
            ..UiState::default()
        };
        assert!(undo_investment_change(&mut document, &mut state));
        assert_eq!(
            document.pointer("/state/investment/family5_flag_overrides"),
            Some(&json!([]))
        );
        assert!(state.last_investment_change.is_none());

        state.last_investment_change = Some(InvestmentUndo::Value {
            definition_index: 3510,
            previous: Some(7),
        });
        assert!(undo_investment_change(&mut document, &mut state));
        assert_eq!(
            document.pointer("/state/investment/family5_value_overrides"),
            Some(&json!([[3510, 7]]))
        );
    }

    #[test]
    fn override_coverage_filters_distinguish_mapping_and_decode_confidence() {
        let unresolved = UnlockDefinition {
            hash: 1,
            code: 0,
            compact_slot: None,
            name: None,
            description: None,
            tested_by: Vec::new(),
        };
        let partial = UnlockDefinition {
            tested_by: vec![ProgressionContextDef {
                hash: 2,
                kind: ProgressionContextKind::Activity,
                name: String::new(),
                type_name: String::new(),
                description: String::new(),
                paths: Vec::new(),
                condition_programs: vec![vec![[99, 1]]],
            }],
            ..unresolved.clone()
        };

        assert!(override_filter_matches(OverrideFilter::Unmapped, None));
        assert!(override_filter_matches(
            OverrideFilter::NoResolvedReaders,
            Some(&unresolved)
        ));
        assert!(override_filter_matches(
            OverrideFilter::PartiallyDecoded,
            Some(&partial)
        ));
        assert!(!override_filter_matches(
            OverrideFilter::NoResolvedReaders,
            Some(&partial)
        ));
    }

    #[test]
    fn collection_state_snapshot_reports_encoded_and_absent_entries_without_inference() {
        let document = json!({
            "state": {
                "unlocks": {
                    "account_flag_runs": [[10, 1]],
                    "objective_values": [[20, 7]]
                },
                "investment": {
                    "family5_flag_overrides": [[30, 2]],
                    "family5_value_overrides": [[40, -1]]
                }
            }
        });
        let snapshot = collection_state_snapshot(&document).unwrap();
        let account_flag = UnlockDefinition {
            hash: 1,
            code: 1,
            compact_slot: Some(10),
            name: None,
            description: None,
            tested_by: Vec::new(),
        };
        let absent_flag = UnlockDefinition {
            compact_slot: Some(11),
            ..account_flag.clone()
        };
        let account_value = UnlockDefinition {
            hash: 2,
            code: 1,
            compact_slot: Some(20),
            name: None,
            description: None,
            tested_by: Vec::new(),
        };
        let absent_value = UnlockDefinition {
            compact_slot: Some(21),
            ..account_value.clone()
        };
        let unbanked = UnlockDefinition {
            hash: 3,
            code: 0,
            compact_slot: None,
            name: None,
            description: None,
            tested_by: Vec::new(),
        };

        assert_eq!(snapshot.flag_text(0, &account_flag), "Set");
        assert_eq!(snapshot.flag_text(0, &absent_flag), "Unset");
        assert_eq!(snapshot.value_text(0, &account_value), "7");
        assert_eq!(snapshot.value_text(0, &absent_value), "Not listed");
        assert_eq!(snapshot.flag_text(30, &unbanked), "Override 2");
        assert_eq!(snapshot.value_text(40, &unbanked), "Override -1");
    }

    #[test]
    fn progression_accepts_missing_sections_and_unknown_fields() {
        let document = json!({
            "state": {
                "investment": {"future": [1, 2, 3]},
                "unlocks": {"future": [1, 2, 3]}
            }
        });

        assert_eq!(parse(&document).unwrap(), Progression::default());
        assert_eq!(parse(&json!({})).unwrap(), Progression::default());
    }

    #[test]
    fn rendered_definition_identifiers_are_filterable() {
        let definition = UnlockDefinition {
            hash: 0x1304_C3FA,
            code: 0x0202,
            compact_slot: Some(502),
            name: Some("Sweet Business Acquired".into()),
            description: None,
            tested_by: vec![ProgressionContextDef {
                hash: 7,
                kind: ProgressionContextKind::Activity,
                name: "The Shattered Throne".into(),
                type_name: String::new(),
                description: String::new(),
                paths: Vec::new(),
                condition_programs: Vec::new(),
            }],
        };

        assert!(definition_matches("#12913", 12_913, &definition));
        assert!(definition_matches("0x1304c3fa", 12_913, &definition));
        assert!(definition_matches("1304c3fa", 12_913, &definition));
        assert!(definition_matches("sweet business", 12_913, &definition));
        assert!(definition_matches("shattered throne", 12_913, &definition));
        assert!(!definition_matches("0xdeadbeef", 12_913, &definition));
    }

    #[test]
    fn objective_summary_includes_goal_hierarchy_and_limit_semantics() {
        let mut objective = ObjectiveDef {
            hash: 0x14D6_FB47,
            description: "Arc".into(),
            completion_value: 5_000,
            allow_overcompletion: true,
            allow_value_change_when_completed: true,
            owners: vec![ObjectiveOwnerDef {
                hash: 0x5A33_50CC,
                kind: ObjectiveOwnerKind::Metric,
                name: "Arc Final Blows".into(),
                type_name: "Metric".into(),
                description: "Arc metric".into(),
                traits: vec![
                    ObjectiveOwnerTraitDef {
                        hash: 0x557C_63B3,
                        name: "All".into(),
                        description: String::new(),
                    },
                    ObjectiveOwnerTraitDef {
                        hash: 0x84EC_E10B,
                        name: "Seasonal".into(),
                        description: "Seasonal metric".into(),
                    },
                ],
                paths: vec![vec!["Account".into(), "Metrics".into()]],
            }],
            ..ObjectiveDef::default()
        };

        assert_eq!(objective_goal_text(&objective), "Arc Final Blows: Arc");
        assert_eq!(
            objective_traits_text(&objective).as_deref(),
            Some("All, Seasonal")
        );
        assert_eq!(
            objective_hierarchy_paths(&objective),
            vec![vec!["Metrics".to_owned(), "Account".to_owned()]]
        );
        assert_eq!(objective_target_text(&objective), "≥5000");
        assert!(objective_target_tooltip(&objective).contains("Over-completion: allowed"));
        assert!(objective_matches("account", &objective));
        assert!(objective_matches("metric", &objective));
        assert!(objective_matches("seasonal", &objective));
        assert!(objective_matches("84ece10b", &objective));
        let details_tooltip = objective_details_tooltip(&objective);
        assert!(!details_tooltip.contains("Seasonal"));
        let traits_tooltip = objective_traits_tooltip(&objective);
        assert!(traits_tooltip.contains("All: 0x557C63B3"));
        assert!(traits_tooltip.contains("Seasonal: 0x84ECE10B"));

        objective.allow_overcompletion = false;
        assert_eq!(objective_target_text(&objective), "5000 max");
        assert!(objective_target_tooltip(&objective).contains("Over-completion: not allowed"));
        assert!(objective_matches("capped", &objective));

        objective.is_counting_downward = true;
        assert_eq!(objective_target_text(&objective), "5000 min");
        objective.allow_overcompletion = true;
        assert_eq!(objective_target_text(&objective), "≤5000");

        objective.description.clear();
        objective.owners[0].name.clear();
        objective.owners[0].type_name = "General inventory".into();
        assert_eq!(objective_goal_text(&objective), "0x14D6FB47");
    }

    #[test]
    fn unnamed_objectives_use_real_reverse_context_without_claiming_ownership() {
        let objective = ObjectiveDef::default();
        let definition = UnlockDefinition {
            hash: 0xE2C6_8308,
            code: 1,
            compact_slot: Some(5_662),
            name: None,
            description: None,
            tested_by: vec![
                ProgressionContextDef {
                    hash: 0,
                    kind: ProgressionContextKind::Objective,
                    name: String::new(),
                    type_name: String::new(),
                    description: String::new(),
                    paths: Vec::new(),
                    condition_programs: Vec::new(),
                },
                ProgressionContextDef {
                    hash: 0xAABB_CCDD,
                    kind: ProgressionContextKind::PresentationNode,
                    name: "Menagerie".into(),
                    type_name: String::new(),
                    description: String::new(),
                    paths: vec![vec![
                        "Minor".into(),
                        "Destinations".into(),
                        "Triumphs".into(),
                    ]],
                    condition_programs: Vec::new(),
                },
            ],
        };

        assert_eq!(
            objective_table_text(&objective, Some(&definition)),
            "Menagerie · objective 0x00000000"
        );
        assert_eq!(
            definition_hierarchy_paths(&definition),
            vec![vec![
                "Triumphs".to_owned(),
                "Destinations".to_owned(),
                "Minor".to_owned(),
            ]]
        );
    }

    #[test]
    fn unnamed_objectives_surface_their_unlock_definition_reference() {
        let objective = ObjectiveDef {
            hash: 0x1234_5678,
            related_unlock_value_definition_index: Some(1_630),
            ..ObjectiveDef::default()
        };
        let definition = UnlockDefinition {
            hash: 0x32DC_3113,
            code: 1,
            compact_slot: Some(404),
            name: None,
            description: None,
            tested_by: vec![ProgressionContextDef {
                hash: objective.hash,
                kind: ProgressionContextKind::Objective,
                name: String::new(),
                type_name: String::new(),
                description: String::new(),
                paths: Vec::new(),
                condition_programs: Vec::new(),
            }],
        };

        assert_eq!(
            objective_table_text(&objective, Some(&definition)),
            "Objective 0x12345678 · value definition #1630"
        );
        assert!(definition_hierarchy_paths(&definition).is_empty());
    }

    #[test]
    fn metric_traits_distinguish_rows_with_the_same_package_name() {
        let objective = |trait_hash, trait_name: &str| ObjectiveDef {
            description: "Arc".into(),
            owners: vec![ObjectiveOwnerDef {
                hash: 1,
                kind: ObjectiveOwnerKind::Metric,
                name: "Arc Final Blows".into(),
                type_name: "Metric".into(),
                description: String::new(),
                traits: vec![ObjectiveOwnerTraitDef {
                    hash: trait_hash,
                    name: trait_name.into(),
                    description: String::new(),
                }],
                paths: vec![vec!["Account".into(), "Metrics".into()]],
            }],
            ..ObjectiveDef::default()
        };

        let seasonal = objective(0x84EC_E10B, "Seasonal");
        let weekly = objective(0x8C79_925E, "Weekly");
        assert_eq!(objective_goal_text(&seasonal), "Arc Final Blows: Arc");
        assert_eq!(objective_goal_text(&weekly), "Arc Final Blows: Arc");
        assert_eq!(
            objective_traits_text(&seasonal).as_deref(),
            Some("Seasonal")
        );
        assert_eq!(objective_traits_text(&weekly).as_deref(), Some("Weekly"));
    }

    #[test]
    fn hierarchy_normalizes_leaf_first_and_bare_paths_to_one_root_first_path() {
        let leaf_first = vec!["Destination".to_owned(), "Metrics".to_owned()];
        let bare = vec!["Destination".to_owned()];
        let repeated_root = vec![
            "Metrics".to_owned(),
            "Destination".to_owned(),
            "Metrics".to_owned(),
        ];

        let expected = vec!["Metrics".to_owned(), "Destination".to_owned()];
        assert_eq!(normalize_hierarchy_path(&leaf_first, "Metrics"), expected);
        assert_eq!(normalize_hierarchy_path(&bare, "Metrics"), expected);
        assert_eq!(
            normalize_hierarchy_path(&repeated_root, "Metrics"),
            expected
        );
    }

    #[test]
    fn metadata_package_paths_render_root_first() {
        let package_path = vec![
            "Aspirant Suit".to_owned(),
            "Leveling".to_owned(),
            "Warlock".to_owned(),
            "Armor".to_owned(),
            "Items".to_owned(),
        ];

        assert_eq!(
            metadata_path_text(&package_path),
            "Items > Armor > Warlock > Leveling > Aspirant Suit"
        );
        assert_eq!(metadata_path_text(&[]), "<empty path>");
    }

    #[test]
    fn family5_flag_states_use_sunrise_logical_value_terms() {
        assert_eq!(flag_override_state_label(0), "0 · clear");
        assert_eq!(flag_override_state_label(1), "1 · logical value 1");
        assert_eq!(flag_override_state_label(2), "2 · set");
    }

    #[test]
    fn override_meaning_uses_authored_names_then_exact_package_readers() {
        let context = |name: &str| ProgressionContextDef {
            hash: 1,
            kind: ProgressionContextKind::ActivityAvailability,
            name: name.into(),
            type_name: String::new(),
            description: String::new(),
            paths: Vec::new(),
            condition_programs: Vec::new(),
        };
        let mut definition = UnlockDefinition {
            hash: 2,
            code: 1,
            compact_slot: Some(3),
            name: Some("Authored package meaning".into()),
            description: None,
            tested_by: vec![context("The Menagerie")],
        };
        assert_eq!(override_meaning(&definition), "Authored package meaning");

        definition.name = None;
        assert_eq!(override_meaning(&definition), "The Menagerie");

        definition.tested_by.push(context("The Gauntlet"));
        assert_eq!(
            override_meaning(&definition),
            "The Gauntlet · The Menagerie"
        );

        definition.tested_by.push(context("The Mockery"));
        assert_eq!(
            override_meaning(&definition),
            "The Gauntlet · The Menagerie · +1 more"
        );

        definition.tested_by.clear();
        assert_eq!(override_meaning(&definition), "Reader not resolved");
    }

    #[test]
    fn value_override_usage_decodes_direct_comparisons_and_reports_unknown_programs() {
        let definition = UnlockDefinition {
            hash: 2,
            code: 1,
            compact_slot: None,
            name: None,
            description: None,
            tested_by: vec![ProgressionContextDef {
                hash: 3,
                kind: ProgressionContextKind::Activity,
                name: "Power-gated activity".into(),
                type_name: String::new(),
                description: String::new(),
                paths: Vec::new(),
                condition_programs: vec![
                    vec![[10, 462], [11, 900], [14, u32::MAX]],
                    vec![[1, 4], [99, u32::MAX]],
                ],
            }],
        };

        let usage =
            override_usage_summary(MetadataSelection::ValueOverride(462, 1_010), &definition);
        assert!(usage.condition_usage.contains("direct ≥ 900"));
        assert_eq!(
            usage.forced_impact.as_deref(),
            Some("At 1010: 1 decoded direct comparison pass, 0 fail")
        );
        assert_eq!(
            usage.undecoded_opcodes.as_deref(),
            Some("99 · preserved raw in each condition program")
        );
        assert_eq!(condition_opcode_label(15), "Less than (15)");
        assert_eq!(condition_opcode_label(4), "And (4)");
        assert_eq!(condition_opcode_label(9), "Not equal (9)");
    }

    #[test]
    fn objective_reference_opcode_is_not_reported_as_undecoded() {
        let definition = UnlockDefinition {
            tested_by: vec![ProgressionContextDef {
                hash: 0,
                kind: ProgressionContextKind::ExpressionMapping,
                name: String::new(),
                type_name: String::new(),
                description: String::new(),
                paths: Vec::new(),
                condition_programs: vec![vec![[12, 91]]],
            }],
            ..UnlockDefinition::default()
        };

        assert!(!definition_has_undecoded_opcodes(&definition));
        let usage = override_usage_summary(MetadataSelection::FlagDefinition(0), &definition);
        assert_eq!(usage.undecoded_opcodes, None);
    }

    #[test]
    fn objectives_without_package_paths_do_not_get_synthetic_categories() {
        let mut objective = ObjectiveDef {
            owners: vec![ObjectiveOwnerDef {
                hash: 1,
                kind: ObjectiveOwnerKind::InventoryItem,
                name: "Ace of Spades Catalyst".into(),
                type_name: "General inventory".into(),
                description: String::new(),
                traits: Vec::new(),
                paths: Vec::new(),
            }],
            ..ObjectiveDef::default()
        };

        assert!(objective_hierarchy_paths(&objective).is_empty());

        objective.owners[0].name.clear();
        objective.owners[0].type_name = "Item".into();
        assert!(objective_hierarchy_paths(&objective).is_empty());
        assert_eq!(objective_goal_text(&objective), "Item: 0x00000000");
    }

    #[test]
    fn context_paths_do_not_invent_an_uncategorized_parent() {
        assert_eq!(
            normalize_context_path(&["Werner 99-40".into()]),
            vec!["Werner 99-40"]
        );
        assert_eq!(
            normalize_context_path(&["Destination".into(), "Metrics".into()]),
            vec!["Metrics", "Destination"]
        );
    }

    #[test]
    fn objective_hierarchy_lists_every_distinct_path_without_a_cap() {
        let paths = (0..300)
            .map(|index| vec![format!("Branch {index}"), "Metrics".into()])
            .collect::<Vec<_>>();
        let objective = ObjectiveDef {
            owners: vec![
                ObjectiveOwnerDef {
                    hash: 1,
                    kind: ObjectiveOwnerKind::Metric,
                    name: "Metric".into(),
                    type_name: "Metric".into(),
                    description: String::new(),
                    traits: Vec::new(),
                    paths: paths.clone(),
                },
                ObjectiveOwnerDef {
                    hash: 2,
                    kind: ObjectiveOwnerKind::Record,
                    name: "Record".into(),
                    type_name: "Triumph / record".into(),
                    description: String::new(),
                    traits: Vec::new(),
                    paths: vec![paths[0].clone(), vec!["Account".into(), "Triumphs".into()]],
                },
            ],
            ..ObjectiveDef::default()
        };

        let locations = objective_hierarchy_paths(&objective);

        assert_eq!(locations.len(), 301);
        assert_eq!(
            locations.first().unwrap(),
            &vec!["Metrics".to_owned(), "Branch 0".to_owned()]
        );
        assert_eq!(
            locations.get(299).unwrap(),
            &vec!["Metrics".to_owned(), "Branch 299".to_owned()]
        );
        assert_eq!(
            locations.last().unwrap(),
            &vec!["Triumphs".to_owned(), "Account".to_owned()]
        );
    }

    #[test]
    fn objective_leaf_sort_is_stable_inside_a_branch() {
        let rows = [
            IndexedValue { index: 1, value: 5 },
            IndexedValue { index: 2, value: 4 },
            IndexedValue { index: 3, value: 3 },
        ];
        let objectives = [
            ObjectiveDef {
                description: "Alpha".into(),
                ..ObjectiveDef::default()
            },
            ObjectiveDef {
                description: "Alpha".into(),
                ..ObjectiveDef::default()
            },
            ObjectiveDef {
                description: "Beta".into(),
                ..ObjectiveDef::default()
            },
        ];
        let mut branch = ObjectiveHierarchyBranch::new("Metrics".into(), vec!["Metrics".into()]);
        branch.leaves = vec![
            ObjectiveHierarchyLeaf {
                row: &rows[0],
                definition_index: None,
                definition: None,
                objective_index: None,
                objective: Some(&objectives[0]),
            },
            ObjectiveHierarchyLeaf {
                row: &rows[1],
                definition_index: None,
                definition: None,
                objective_index: None,
                objective: Some(&objectives[1]),
            },
            ObjectiveHierarchyLeaf {
                row: &rows[2],
                definition_index: None,
                definition: None,
                objective_index: None,
                objective: Some(&objectives[2]),
            },
        ];

        let mut hierarchy = ObjectiveHierarchy {
            branches: vec![branch],
            leaves: Vec::new(),
        };
        sort_objective_hierarchy(&mut hierarchy, TableSort::ascending(0));

        assert_eq!(
            hierarchy.branches[0]
                .leaves
                .iter()
                .map(|leaf| leaf.row.index)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn filtered_hierarchy_auto_expands_matching_branches() {
        let row = IndexedValue { index: 1, value: 1 };
        let mut root = ObjectiveHierarchyBranch::new("Metrics".into(), vec!["Metrics".into()]);
        let mut child = ObjectiveHierarchyBranch::new(
            "Destination".into(),
            vec!["Metrics".into(), "Destination".into()],
        );
        child.leaves.push(ObjectiveHierarchyLeaf {
            row: &row,
            definition_index: None,
            definition: None,
            objective_index: None,
            objective: None,
        });
        root.children.push(child);
        let hierarchy = ObjectiveHierarchy {
            branches: vec![root],
            leaves: Vec::new(),
        };
        let state = UiState::default();

        assert_eq!(
            objective_matrix_lines(&hierarchy, "test", &state, false).len(),
            2
        );
        assert_eq!(
            objective_matrix_lines(&hierarchy, "test", &state, true).len(),
            3
        );
    }

    #[test]
    fn tested_by_rows_have_no_cap_and_merge_identical_visible_contexts() {
        let mut contexts = (0..300)
            .map(|index| ProgressionContextDef {
                hash: index,
                kind: ProgressionContextKind::Activity,
                name: format!("Activity {index}"),
                type_name: String::new(),
                description: String::new(),
                paths: Vec::new(),
                condition_programs: Vec::new(),
            })
            .collect::<Vec<_>>();
        contexts.push(ProgressionContextDef {
            hash: 999,
            kind: ProgressionContextKind::ActivityAvailability,
            name: "Activity 0".into(),
            type_name: String::new(),
            description: String::new(),
            paths: Vec::new(),
            condition_programs: Vec::new(),
        });
        let definition = UnlockDefinition {
            tested_by: contexts,
            ..UnlockDefinition::default()
        };

        let lines = definition_context_lines(&definition);
        let display_lines = definition_context_display_lines(1, |_| Some((0, &definition)), false);

        assert_eq!(lines.len(), 300);
        assert_eq!(display_lines.len(), 300);
        assert_eq!(
            lines
                .iter()
                .find(|line| line.text() == "Activity 0")
                .unwrap()
                .contexts
                .len(),
            2
        );
    }

    #[test]
    fn tested_by_rows_hide_empty_internal_refs_and_generic_inventory_buckets() {
        let definition = UnlockDefinition {
            tested_by: vec![
                ProgressionContextDef {
                    hash: 1,
                    kind: ProgressionContextKind::ExpressionMapping,
                    name: String::new(),
                    type_name: String::new(),
                    description: String::new(),
                    paths: Vec::new(),
                    condition_programs: Vec::new(),
                },
                ProgressionContextDef {
                    hash: 2,
                    kind: ProgressionContextKind::InventoryItem,
                    name: String::new(),
                    type_name: "General inventory".into(),
                    description: String::new(),
                    paths: Vec::new(),
                    condition_programs: Vec::new(),
                },
            ],
            ..UnlockDefinition::default()
        };

        let lines = definition_context_lines(&definition);
        assert!(lines.is_empty());
    }

    #[test]
    fn progression_rejects_rows_sunrise_cannot_parse() {
        let invalid_run = json!({
            "state": {"unlocks": {"profile_flag_runs": [[511, 2]]}}
        });
        assert!(parse(&invalid_run).unwrap_err().contains("512-slot bank"));

        let zero_run = json!({
            "state": {"unlocks": {"account_flag_runs": [[1, 0]]}}
        });
        assert!(parse(&zero_run).unwrap_err().contains("positive length"));

        let invalid_override = json!({
            "state": {"investment": {"family5_flag_overrides": [[23500, 2]]}}
        });
        assert!(parse(&invalid_override).unwrap_err().contains("23499"));

        let invalid_value = json!({
            "state": {"unlocks": {"objective_values": [[1, 2147483648_i64]]}}
        });
        assert!(parse(&invalid_value).unwrap_err().contains("signed 32-bit"));
    }
}
