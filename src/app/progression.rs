use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};

use eframe::egui;
use serde_json::{Map, Value};

use crate::catalog::{
    Catalog, ObjectiveDef, ObjectiveOwnerDef, ObjectiveOwnerKind, ObjectiveOwnerTraitDef,
    ProgressionContextDef, ProgressionContextKind, UnlockDefinition,
};

use super::glyphs::{self, Glyph};

const ACCOUNT_FLAG_CAPACITY: usize = 12_300;
const PROFILE_FLAG_CAPACITY: usize = 512;
const CHARACTER_FLAG_CAPACITY: usize = 256;
const OBJECTIVE_VALUE_CAPACITY: usize = 6_200;
const CHARACTER_OBJECT_FLAG_CAPACITY: usize = 4_096;
const CHARACTER_OBJECT_VALUE_CAPACITY: usize = 768;
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
const TABLE_CELL_HEIGHT: f32 = 20.0;
const TABLE_ROW_GAP: f32 = 4.0;
const TABLE_ROW_STRIDE: f32 = TABLE_CELL_HEIGHT + TABLE_ROW_GAP;
const TABLE_COLUMN_GAP: f32 = 12.0;
const HIERARCHY_INDENT: f32 = 14.0;
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
}

impl UnlockTable {
    const ALL: [Self; 6] = [
        Self::AccountFlagRuns,
        Self::ProfileFlagRuns,
        Self::CharacterFlags,
        Self::ObjectiveValues,
        Self::CharacterObjectFlagRuns,
        Self::CharacterObjectObjectiveValues,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::AccountFlagRuns => "Account flags",
            Self::ProfileFlagRuns => "Profile flags",
            Self::CharacterFlags => "Per-character flags",
            Self::ObjectiveValues => "Account objective values",
            Self::CharacterObjectFlagRuns => "Selected-character flags",
            Self::CharacterObjectObjectiveValues => "Selected-character objective values",
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
            Self::FlagOverrides => "Family 5 flag overrides",
            Self::ValueOverrides => "Family 5 value overrides",
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
}

impl UiState {
    pub(super) fn reset_navigation(&mut self) {
        self.query.clear();
        self.add_open = false;
        self.add_query.clear();
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
    definition: Option<&'a UnlockDefinition>,
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
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Progression package data unavailable",
            );
            ui.label(egui::RichText::new(error).small().weak());
        });
        ui.add_space(8.0);
    }

    if let Some(error) = destiny_symbol_font_error {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Destiny symbol fonts unavailable",
            );
            ui.label(egui::RichText::new(error).small().weak());
        });
        ui.add_space(8.0);
    }

    let policy = match parse(document) {
        Ok(policy) => policy,
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, "Invalid progression settings");
            ui.label(error);
            return false;
        }
    };

    match view {
        View::Unlocks => draw_unlocks(ui, document, &policy.unlocks, catalog, state),
        View::Investment => draw_investment(ui, document, &policy.investment, catalog, state),
    }
}

fn draw_unlocks(
    ui: &mut egui::Ui,
    document: &mut Value,
    unlocks: &UnlockPolicy,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    let mut table_changed = false;
    ui.horizontal(|ui| {
        ui.label("Table");
        egui::ComboBox::from_id_salt("progression_unlock_table")
            .selected_text(state.unlock_table.label())
            .width(250.0)
            .show_ui(ui, |ui| {
                for table in UnlockTable::ALL {
                    table_changed |= ui
                        .selectable_value(&mut state.unlock_table, table, table.label())
                        .changed();
                }
            });
    });
    if table_changed {
        state.query.clear();
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        draw_filter(ui, &mut state.query);
        if ui.button("+ Add").clicked() {
            state.add_open = true;
            state.add_query.clear();
            state.add_value = 0;
        }
    });
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
    ui.horizontal(|ui| {
        ui.label("Table");
        egui::ComboBox::from_id_salt("progression_investment_table")
            .selected_text(state.investment_table.label())
            .width(250.0)
            .show_ui(ui, |ui| {
                for table in InvestmentTable::ALL {
                    table_changed |= ui
                        .selectable_value(&mut state.investment_table, table, table.label())
                        .changed();
                }
            });
    });
    if table_changed {
        state.query.clear();
        state.add_open = false;
    }
    ui.add_space(4.0);
    let row_count = match state.investment_table {
        InvestmentTable::FlagOverrides => investment.flag_overrides.len(),
        InvestmentTable::ValueOverrides => investment.value_overrides.len(),
    };
    let can_add = row_count < FAMILY5_OVERRIDE_CAPACITY;
    ui.horizontal(|ui| {
        draw_filter(ui, &mut state.query);
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
    });
    if !can_add {
        state.add_open = false;
    }
    let query = state.query.clone();

    let mut changed = match state.investment_table {
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
            .desired_width(360.0),
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
    let spec = add_table_spec(state.unlock_table);
    let occupied = occupied_slots(unlocks, state.unlock_table, spec.capacity);
    let definitions = if spec.value {
        catalog.unlock_value_definitions()
    } else {
        catalog.unlock_flag_definitions()
    };
    let query = state.add_query.trim().to_lowercase();
    let candidates = definitions
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
                        && catalog
                            .objective_for_unlock_value(index)
                            .is_some_and(|objective| objective_matches(&query, objective)))))
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
    };
    for slot in slots {
        if let Some(value) = occupied.get_mut(slot) {
            *value = true;
        }
    }
    occupied
}

fn add_definition_label(definition: &UnlockDefinition, objective: Option<&ObjectiveDef>) -> String {
    if let Some(name) = definition_name(definition) {
        return name.to_owned();
    }
    if let Some(objective) = objective {
        return objective_goal_text(objective);
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
        lines.push(format!("Target: {}", objective_target_text(objective)));
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
    let candidates = definitions
        .iter()
        .enumerate()
        .filter(|(index, definition)| {
            !occupied.contains(index)
                && (query.is_empty()
                    || definition_matches(&query, *index, definition)
                    || (is_value
                        && catalog
                            .objective_for_unlock_value(*index)
                            .is_some_and(|objective| objective_matches(&query, objective))))
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
                ui.label("Initial override");
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
                                    egui::Label::new(add_definition_label(definition, objective))
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
    set_investment_override(
        document,
        state.investment_table,
        definition_index,
        state.add_value,
    )
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
        &format!("{} ranges", rows.len()),
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
        &format!("{} indices", rows.len()),
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
    source_summary: &str,
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
    let named = slots
        .iter()
        .filter_map(|slot| catalog.unlock_flag_for_state(config.bank, *slot))
        .filter(|(_, definition)| definition_name(definition).is_some())
        .count();
    let contextualized = slots
        .iter()
        .filter_map(|slot| catalog.unlock_flag_for_state(config.bank, *slot))
        .filter(|(_, definition)| definition_has_context(definition))
        .count();
    ui.label(format!(
        "{source_summary} · {} set · {mapped} mapped · {named} named · {contextualized} with context",
        slots.len(),
    ))
    .on_hover_text("Definition match: bank + compact slot");

    let state_width = 76.0;
    let definition_width = 152.0;
    let name_width = (ui.available_width() * 0.2).clamp(104.0, 220.0);
    let tested_by_width = (ui.available_width()
        - state_width
        - definition_width
        - name_width
        - TABLE_COLUMN_GAP * 3.0)
        .max(150.0);
    ui.add_space(4.0);
    let sort = sortable_table_header(
        ui,
        config.id,
        &[
            (name_width, "Package name"),
            (definition_width, "Definition"),
            (tested_by_width, "Used by"),
            (state_width, "Slot"),
        ],
        TableSort::ascending(3),
        state,
    );
    ui.separator();
    if filtered.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return false;
    }

    filtered.sort_by(|left, right| compare_flag_slots(*left, *right, config.bank, catalog, sort));
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
                    .num_columns(4)
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
                                    draw_definition_name(ui, name_width, definition);
                                    draw_definition_identity(
                                        ui,
                                        definition_width,
                                        definition_index,
                                        definition,
                                    );
                                } else {
                                    table_cell(ui, name_width, egui::RichText::new("—").weak());
                                    table_cell(
                                        ui,
                                        definition_width,
                                        egui::RichText::new("—").weak(),
                                    )
                                    .on_hover_text("No package definition");
                                }
                            } else {
                                table_cell(ui, name_width, "");
                                table_cell(ui, definition_width, "");
                            }
                            draw_context_cell(ui, tested_by_width, line.context.as_ref());
                            if line.primary {
                                let response = draw_slot_with_remove(ui, state_width, slot);
                                if response.clicked()
                                    && set_unlock_flag(document, config.id, slot, false)
                                {
                                    changed = true;
                                }
                            } else {
                                table_cell(ui, state_width, "");
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
    let readable_owners = rows
        .iter()
        .filter_map(|row| catalog.unlock_value_for_state(bank, row.index))
        .filter_map(|(definition_index, _)| catalog.objective_for_unlock_value(definition_index))
        .filter(|objective| preferred_objective_owner(objective).is_some())
        .count();
    ui.label(format!(
        "{} values · {objectives} objectives · {readable_owners} owners",
        rows.len(),
    ))
    .on_hover_text("Definition: bank + compact slot\nHierarchy: package owner paths");
    ui.label(egui::RichText::new("Target: max/min capped · ≥/≤ uncapped").weak());

    let state_width = 76.0;
    let value_width = 64.0;
    let target_width = 82.0;
    let traits_width = (ui.available_width() * 0.15).clamp(96.0, 180.0);
    let objective_width = (ui.available_width()
        - traits_width
        - state_width
        - value_width
        - target_width
        - TABLE_COLUMN_GAP * 4.0)
        .max(150.0);
    ui.add_space(4.0);
    let sort = sortable_table_header(
        ui,
        id,
        &[
            (objective_width, "Goal: Objective"),
            (traits_width, "Traits"),
            (value_width, "Value"),
            (target_width, "Target"),
            (state_width, "Slot"),
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
                    .num_columns(5)
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
                                    table_cell(ui, traits_width, "");
                                    table_cell(ui, value_width, "");
                                    table_cell(ui, target_width, "");
                                    table_cell(ui, state_width, "");
                                }
                                ObjectiveMatrixLine::Leaf { leaf, depth } => {
                                    if let Some(objective) = leaf.objective {
                                        draw_hierarchy_leaf_cell(
                                            ui,
                                            objective_width,
                                            depth,
                                            objective_goal_text(objective),
                                        )
                                        .on_hover_text(objective_details_tooltip(objective));
                                    } else {
                                        let response = draw_hierarchy_leaf_cell(
                                            ui,
                                            objective_width,
                                            depth,
                                            egui::RichText::new("—").weak(),
                                        );
                                        response.on_hover_text(if leaf.definition.is_some() {
                                            "No same-hash objective"
                                        } else {
                                            "No package definition"
                                        });
                                    }
                                    if let Some(objective) = leaf.objective {
                                        table_cell(
                                            ui,
                                            traits_width,
                                            objective_traits_text(objective)
                                                .map(egui::RichText::new)
                                                .unwrap_or_else(|| egui::RichText::new("—").weak()),
                                        )
                                        .on_hover_text(objective_traits_tooltip(objective));
                                    } else {
                                        table_cell(
                                            ui,
                                            traits_width,
                                            egui::RichText::new("—").weak(),
                                        );
                                    }
                                    let row = leaf.row;
                                    let mut value = row.value;
                                    if table_drag_value(ui, value_width, &mut value).changed()
                                        && set_unlock_value(document, id, row.index, value)
                                    {
                                        changed = true;
                                    }
                                    if let Some(objective) = leaf.objective {
                                        draw_objective_target(ui, target_width, objective);
                                    } else {
                                        table_cell(
                                            ui,
                                            target_width,
                                            egui::RichText::new("—").weak(),
                                        );
                                    }
                                    let response =
                                        draw_slot_with_remove(ui, state_width, row.index);
                                    if response.clicked()
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
        .filter(|row| family5_flag_matches(&query, row, catalog))
        .collect::<Vec<_>>();
    let mapped = rows
        .iter()
        .filter(|row| {
            catalog
                .unlock_flag_definition(row.definition_index)
                .is_some()
        })
        .count();
    let named = rows
        .iter()
        .filter_map(|row| catalog.unlock_flag_definition(row.definition_index))
        .filter(|definition| definition_name(definition).is_some())
        .count();
    let contextualized = rows
        .iter()
        .filter_map(|row| catalog.unlock_flag_definition(row.definition_index))
        .filter(|definition| definition_has_context(definition))
        .count();
    ui.label(format!(
        "{} rows · {mapped} mapped · {named} named · {contextualized} with context",
        rows.len()
    ))
    .on_hover_text("Definition index: first value");

    let index_width = 68.0;
    let value_width = 64.0;
    let hash_width = 96.0;
    let name_width = (ui.available_width() * 0.18).clamp(96.0, 180.0);
    let tested_by_width = (ui.available_width()
        - index_width
        - value_width
        - hash_width
        - name_width
        - TABLE_COLUMN_GAP * 4.0)
        .max(145.0);
    ui.add_space(4.0);
    let sort = sortable_table_header(
        ui,
        "family5_flag_overrides",
        &[
            (index_width, "Index"),
            (value_width, "Override"),
            (name_width, "Package name"),
            (hash_width, "Hash"),
            (tested_by_width, "Used by"),
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
    let display_lines = definition_context_display_lines(
        filtered.len(),
        |row_index| {
            let row = filtered[row_index];
            catalog
                .unlock_flag_definition(row.definition_index)
                .map(|definition| (row.definition_index, definition))
        },
        sort.column == 4 && sort.descending,
    );
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("progression_table", "family5_flag_overrides"))
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(TABLE_ROW_STRIDE * 3.0))
            .show_rows(ui, TABLE_CELL_HEIGHT, display_lines.len(), |ui, range| {
                egui::Grid::new("family5_flag_override_rows")
                    .num_columns(5)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for line_index in range {
                            let line = &display_lines[line_index];
                            let row = filtered[line.row_index];
                            if line.primary {
                                let response =
                                    draw_slot_with_remove(ui, index_width, row.definition_index);
                                if response.clicked()
                                    && remove_investment_override(
                                        document,
                                        InvestmentTable::FlagOverrides,
                                        row.definition_index,
                                    )
                                {
                                    changed = true;
                                }
                                let mut value = i32::from(row.value);
                                if table_drag_value_ranged(
                                    ui,
                                    value_width,
                                    &mut value,
                                    0..=i32::from(FAMILY5_FLAG_VALUE_MAXIMUM),
                                )
                                .changed()
                                    && set_investment_override(
                                        document,
                                        InvestmentTable::FlagOverrides,
                                        row.definition_index,
                                        value,
                                    )
                                {
                                    changed = true;
                                }
                                if let Some(definition) = line.definition {
                                    draw_definition_name(ui, name_width, definition);
                                    draw_definition_hash(ui, hash_width, definition);
                                } else {
                                    table_cell(ui, name_width, egui::RichText::new("—").weak());
                                    table_cell(ui, hash_width, egui::RichText::new("—").weak())
                                        .on_hover_text("Index not in package table");
                                }
                            } else {
                                table_cell(ui, index_width, "");
                                table_cell(ui, value_width, "");
                                table_cell(ui, name_width, "");
                                table_cell(ui, hash_width, "");
                            }
                            draw_context_cell(ui, tested_by_width, line.context.as_ref());
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
        .filter(|row| family5_value_matches(&query, row, catalog))
        .collect::<Vec<_>>();
    let mapped = rows
        .iter()
        .filter(|row| {
            catalog
                .unlock_value_definition(row.definition_index)
                .is_some()
        })
        .count();
    let named = rows
        .iter()
        .filter_map(|row| catalog.unlock_value_definition(row.definition_index))
        .filter(|definition| definition_name(definition).is_some())
        .count();
    let contextualized = rows
        .iter()
        .filter_map(|row| catalog.unlock_value_definition(row.definition_index))
        .filter(|definition| definition_has_context(definition))
        .count();
    ui.label(format!(
        "{} rows · {mapped} mapped · {named} named · {contextualized} with context",
        rows.len()
    ))
    .on_hover_text("Definition index: first value");

    let definition_width = 112.0;
    let value_width = 64.0;
    let target_width = 72.0;
    let tested_by_width = (ui.available_width() * 0.26).clamp(124.0, 260.0);
    let objective_width = (ui.available_width()
        - definition_width
        - value_width
        - target_width
        - tested_by_width
        - TABLE_COLUMN_GAP * 4.0)
        .max(96.0);
    ui.add_space(4.0);
    let sort = sortable_table_header(
        ui,
        "family5_value_overrides",
        &[
            (definition_width, "Definition"),
            (value_width, "Override"),
            (objective_width, "Goal: Objective"),
            (target_width, "Target"),
            (tested_by_width, "Used by"),
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
    let display_lines = definition_context_display_lines(
        filtered.len(),
        |row_index| {
            let row = filtered[row_index];
            catalog
                .unlock_value_definition(row.definition_index)
                .map(|definition| (row.definition_index, definition))
        },
        sort.column == 4 && sort.descending,
    );
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("progression_table", "family5_value_overrides"))
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(TABLE_ROW_STRIDE * 3.0))
            .show_rows(ui, TABLE_CELL_HEIGHT, display_lines.len(), |ui, range| {
                egui::Grid::new("family5_value_override_rows")
                    .num_columns(5)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for line_index in range {
                            let line = &display_lines[line_index];
                            let row = filtered[line.row_index];
                            if line.primary {
                                let response = draw_definition_with_remove(
                                    ui,
                                    definition_width,
                                    row.definition_index,
                                    line.definition,
                                );
                                if response.clicked()
                                    && remove_investment_override(
                                        document,
                                        InvestmentTable::ValueOverrides,
                                        row.definition_index,
                                    )
                                {
                                    changed = true;
                                }
                                let mut value = row.value;
                                if table_drag_value(ui, value_width, &mut value).changed()
                                    && set_investment_override(
                                        document,
                                        InvestmentTable::ValueOverrides,
                                        row.definition_index,
                                        value,
                                    )
                                {
                                    changed = true;
                                }
                                if let Some(objective) =
                                    catalog.objective_for_unlock_value(row.definition_index)
                                {
                                    table_cell(ui, objective_width, objective_goal_text(objective))
                                        .on_hover_text(objective_details_tooltip(objective));
                                    draw_objective_target(ui, target_width, objective);
                                } else {
                                    table_cell(
                                        ui,
                                        objective_width,
                                        egui::RichText::new("—").weak(),
                                    )
                                    .on_hover_text("No same-hash objective");
                                    table_cell(ui, target_width, egui::RichText::new("—").weak());
                                }
                            } else {
                                table_cell(ui, definition_width, "");
                                table_cell(ui, value_width, "");
                                table_cell(ui, objective_width, "");
                                table_cell(ui, target_width, "");
                            }
                            draw_context_cell(ui, tested_by_width, line.context.as_ref());
                            ui.end_row();
                        }
                    });
            });
    });
    changed
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

fn sortable_header_cell(
    ui: &mut egui::Ui,
    width: f32,
    label: &str,
    marker: Option<Glyph>,
) -> egui::Response {
    let cell = ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.spacing_mut().item_spacing.x = 3.0;
            if let Some(direction) = marker {
                let (rect, _) = ui
                    .allocate_exact_size(egui::vec2(10.0, TABLE_CELL_HEIGHT), egui::Sense::hover());
                glyphs::paint(ui, rect, direction);
            }
            ui.add(egui::Label::new(egui::RichText::new(label).strong()).truncate());
        },
    );
    cell.response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn table_cell(ui: &mut egui::Ui, width: f32, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.add(egui::Label::new(text).truncate())
        },
    )
    .inner
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

fn draw_definition_with_remove(
    ui: &mut egui::Ui,
    width: f32,
    definition_index: usize,
    definition: Option<&UnlockDefinition>,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            let label = definition.map_or_else(
                || egui::RichText::new(format!("#{definition_index}")).weak(),
                |definition| {
                    egui::RichText::new(definition_identity(definition_index, definition))
                        .monospace()
                },
            );
            ui.add(egui::Label::new(label).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                super::item_editor::draw_trash_button(ui, true, "Remove state entry")
                    .on_hover_text("Remove")
            })
            .inner
        },
    )
    .inner
}

fn draw_slot_with_remove(ui: &mut egui::Ui, width: f32, slot: usize) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.label(egui::RichText::new(slot.to_string()).monospace());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                super::item_editor::draw_trash_button(ui, true, "Remove state entry")
                    .on_hover_text("Remove")
            })
            .inner
        },
    )
    .inner
}

fn draw_hierarchy_branch_cell(
    ui: &mut egui::Ui,
    width: f32,
    depth: usize,
    label: &str,
    expanded: bool,
    interactive: bool,
) -> egui::Response {
    let cell = ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.add_space(depth as f32 * HIERARCHY_INDENT);
            ui.spacing_mut().item_spacing.x = 4.0;
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(10.0, TABLE_CELL_HEIGHT), egui::Sense::hover());
            glyphs::paint(
                ui,
                rect,
                if expanded {
                    Glyph::ChevronDown
                } else {
                    Glyph::ChevronRight
                },
            );
            ui.add(egui::Label::new(egui::RichText::new(label).strong()).truncate());
        },
    );
    let response = cell.response.interact(if interactive {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    });
    if interactive {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

fn draw_hierarchy_leaf_cell(
    ui: &mut egui::Ui,
    width: f32,
    depth: usize,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TABLE_CELL_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, TABLE_CELL_HEIGHT));
            ui.add_space((depth as f32 + 1.0) * HIERARCHY_INDENT);
            ui.add(egui::Label::new(text).truncate())
        },
    )
    .inner
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

fn definition_has_context(definition: &UnlockDefinition) -> bool {
    definition.tested_by.iter().any(|context| {
        !context.name.trim().is_empty()
            || !context.type_name.trim().is_empty()
            || !context.description.trim().is_empty()
            || context
                .paths
                .iter()
                .flatten()
                .any(|component| !component.trim().is_empty())
    })
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
    let mut lines = vec![format!("Code: 0x{:04X}", definition.code)];
    if let Some(slot) = definition.compact_slot {
        lines.push(format!("Compact slot: {slot}"));
    }
    lines.join("\n")
}

fn draw_definition_name(ui: &mut egui::Ui, width: f32, definition: &UnlockDefinition) {
    let response = table_cell(
        ui,
        width,
        definition_name(definition)
            .map(egui::RichText::new)
            .unwrap_or_else(|| egui::RichText::new("—").weak()),
    );
    if let Some(tooltip) = definition_name_tooltip(definition) {
        response.on_hover_text(tooltip);
    }
}

fn draw_definition_identity(
    ui: &mut egui::Ui,
    width: f32,
    index: usize,
    definition: &UnlockDefinition,
) {
    table_cell(
        ui,
        width,
        egui::RichText::new(definition_identity(index, definition)).monospace(),
    )
    .on_hover_text(definition_metadata_tooltip(definition));
}

fn draw_definition_hash(ui: &mut egui::Ui, width: f32, definition: &UnlockDefinition) {
    table_cell(
        ui,
        width,
        egui::RichText::new(definition_hash(definition)).monospace(),
    )
    .on_hover_text(definition_metadata_tooltip(definition));
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

fn build_objective_hierarchy<'a>(
    rows: &[&'a IndexedValue],
    bank: u8,
    catalog: &'a Catalog,
) -> ObjectiveHierarchy<'a> {
    let mut hierarchy = ObjectiveHierarchy::default();
    for &row in rows {
        let definition = catalog.unlock_value_for_state(bank, row.index);
        let objective = definition
            .and_then(|(definition_index, _)| catalog.objective_for_unlock_value(definition_index));
        let leaf = ObjectiveHierarchyLeaf {
            row,
            definition: definition.map(|(_, definition)| definition),
            objective,
        };
        let paths = objective.map(objective_hierarchy_paths).unwrap_or_default();
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
        branch
            .leaves
            .sort_by(|left, right| compare_objective_leaves(left, right, sort));
        for child in &mut branch.children {
            sort_branch(child, sort);
        }
    }

    hierarchy
        .leaves
        .sort_by(|left, right| compare_objective_leaves(left, right, sort));
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
            left_definition
                .and_then(|(_, definition)| definition_name(definition))
                .map(str::to_lowercase),
            right_definition
                .and_then(|(_, definition)| definition_name(definition))
                .map(str::to_lowercase),
            sort.descending,
        ),
        1 => compare_optional(
            left_definition.map(|(index, definition)| (index, definition.hash)),
            right_definition.map(|(index, definition)| (index, definition.hash)),
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
        1 => compare_ordering(left.value.cmp(&right.value), sort.descending),
        2 => compare_optional(
            left_definition
                .and_then(definition_name)
                .map(str::to_lowercase),
            right_definition
                .and_then(definition_name)
                .map(str::to_lowercase),
            sort.descending,
        ),
        3 => compare_optional(
            left_definition.map(|definition| definition.hash),
            right_definition.map(|definition| definition.hash),
            sort.descending,
        ),
        4 => compare_optional(
            left_definition.and_then(definition_context_sort_key),
            right_definition.and_then(definition_context_sort_key),
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
    let left_objective = catalog.objective_for_unlock_value(left.definition_index);
    let right_objective = catalog.objective_for_unlock_value(right.definition_index);
    match sort.column {
        0 => compare_ordering(
            left.definition_index.cmp(&right.definition_index),
            sort.descending,
        ),
        1 => compare_ordering(left.value.cmp(&right.value), sort.descending),
        2 => compare_optional(
            left_objective.map(|objective| objective_goal_text(objective).to_lowercase()),
            right_objective.map(|objective| objective_goal_text(objective).to_lowercase()),
            sort.descending,
        ),
        3 => compare_optional(
            left_objective.map(|objective| objective.completion_value),
            right_objective.map(|objective| objective.completion_value),
            sort.descending,
        ),
        4 => compare_optional(
            left_definition.and_then(definition_context_sort_key),
            right_definition.and_then(definition_context_sort_key),
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
                .map(|objective| objective_goal_text(objective).to_lowercase()),
            right
                .objective
                .map(|objective| objective_goal_text(objective).to_lowercase()),
            sort.descending,
        ),
        1 => compare_optional(
            left.objective
                .and_then(objective_traits_text)
                .map(|traits| traits.to_lowercase()),
            right
                .objective
                .and_then(objective_traits_text)
                .map(|traits| traits.to_lowercase()),
            sort.descending,
        ),
        2 => compare_ordering(left.row.value.cmp(&right.row.value), sort.descending),
        3 => compare_optional(
            left.objective.map(|objective| objective.completion_value),
            right.objective.map(|objective| objective.completion_value),
            sort.descending,
        ),
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
        ObjectiveOwnerKind::Metric => 0,
        ObjectiveOwnerKind::Record => 1,
        ObjectiveOwnerKind::PresentationNode => 2,
        ObjectiveOwnerKind::InventoryItem => 3,
    }
}

fn objective_owner_type(owner: &ObjectiveOwnerDef) -> &str {
    if !owner.type_name.trim().is_empty() {
        owner.type_name.as_str()
    } else {
        match owner.kind {
            ObjectiveOwnerKind::InventoryItem => "Item",
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

fn objective_details_tooltip(objective: &ObjectiveDef) -> String {
    let mut lines = vec![
        format!("Objective: {}", objective_description(objective)),
        format!("Objective hash: 0x{:08X}", objective.hash),
    ];
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

fn draw_objective_target(ui: &mut egui::Ui, width: f32, objective: &crate::catalog::ObjectiveDef) {
    table_cell(
        ui,
        width,
        egui::RichText::new(objective_target_text(objective)).monospace(),
    )
    .on_hover_text(objective_target_tooltip(objective));
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
        "Package target: {target}\nCounts downward: {counts_downward}\nOver-completion: {overcompletion}\nNegative values: {negative}\nChanges after completion: {completed_changes}"
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
            .is_some_and(|objective| objective_matches(query, objective))
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
    let objective = catalog
        .unlock_value_for_state(bank, row.index)
        .and_then(|(definition_index, _)| catalog.objective_for_unlock_value(definition_index));
    match objective {
        Some(objective) => objective_hierarchy_paths(objective).iter().any(|path| {
            path.iter()
                .any(|component| component.to_lowercase().contains(query))
        }),
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
            .is_some_and(|objective| objective_matches(query, objective))
}

fn objective_matches(query: &str, objective: &ObjectiveDef) -> bool {
    objective.description.to_lowercase().contains(query)
        || formatted_hash_matches(query, objective.hash)
        || objective.completion_value.to_string().contains(query)
        || (objective.maximum_value().is_some() && "maximum max capped".contains(query))
        || (objective.minimum_value().is_some() && "minimum min capped".contains(query))
        || (objective.allow_overcompletion
            && "overcompletion threshold no maximum no minimum".contains(query))
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
                definition: None,
                objective: Some(&objectives[0]),
            },
            ObjectiveHierarchyLeaf {
                row: &rows[1],
                definition: None,
                objective: Some(&objectives[1]),
            },
            ObjectiveHierarchyLeaf {
                row: &rows[2],
                definition: None,
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
            definition: None,
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
            })
            .collect::<Vec<_>>();
        contexts.push(ProgressionContextDef {
            hash: 999,
            kind: ProgressionContextKind::ActivityAvailability,
            name: "Activity 0".into(),
            type_name: String::new(),
            description: String::new(),
            paths: Vec::new(),
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
                },
                ProgressionContextDef {
                    hash: 2,
                    kind: ProgressionContextKind::InventoryItem,
                    name: String::new(),
                    type_name: "General inventory".into(),
                    description: String::new(),
                    paths: Vec::new(),
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
