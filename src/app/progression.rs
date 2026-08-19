use eframe::egui;
use serde_json::{Map, Value};

use crate::catalog::{
    Catalog, ObjectiveDef, ObjectiveOwnerDef, ObjectiveOwnerKind, UnlockDefinition,
};

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
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
    tab: Tab,
    unlock_table: UnlockTable,
    investment_table: InvestmentTable,
    query: String,
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

#[derive(Clone, Copy)]
enum ObjectiveLocation<'a> {
    Path(&'a [String]),
    OwnerType(&'a str),
    NoNamedOwner,
    Unavailable,
}

impl ObjectiveLocation<'_> {
    fn text(self) -> String {
        match self {
            Self::Path(path) => path.join(" > "),
            Self::OwnerType(owner_type) => owner_type.to_owned(),
            Self::NoNamedOwner => "—".into(),
            Self::Unavailable => "—".into(),
        }
    }
}

#[derive(Clone, Copy)]
struct ObjectiveValueDisplayLine<'a> {
    value: &'a IndexedValue,
    location: ObjectiveLocation<'a>,
    primary: bool,
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

pub(super) fn draw_page(
    ui: &mut egui::Ui,
    document: &Value,
    catalog: &Catalog,
    destiny_symbol_font_error: Option<&str>,
    state: &mut UiState,
) {
    ui.horizontal(|ui| {
        ui.heading("Unlocks & investment");
        ui.label(
            egui::RichText::new("EXPERIMENTAL")
                .small()
                .color(ui.visuals().warn_fg_color),
        );
        ui.label(egui::RichText::new("READ-ONLY").small().weak());
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui
            .selectable_value(&mut state.tab, Tab::Unlocks, "Unlocks")
            .changed()
        {
            state.query.clear();
        }
        if ui
            .selectable_value(&mut state.tab, Tab::Investment, "Investment")
            .changed()
        {
            state.query.clear();
        }
    });
    ui.separator();

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
            return;
        }
    };

    match state.tab {
        Tab::Unlocks => draw_unlocks(ui, &policy.unlocks, catalog, state),
        Tab::Investment => draw_investment(ui, &policy.investment, catalog, state),
    }
}

fn draw_unlocks(ui: &mut egui::Ui, unlocks: &UnlockPolicy, catalog: &Catalog, state: &mut UiState) {
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
    draw_filter(ui, &mut state.query);

    match state.unlock_table {
        UnlockTable::AccountFlagRuns => draw_flag_runs(
            ui,
            FlagTableConfig {
                id: "account_flag_runs",
                bank: ACCOUNT_FLAG_BANK,
                capacity: ACCOUNT_FLAG_CAPACITY,
            },
            &unlocks.account_flag_runs,
            catalog,
            &state.query,
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
            &state.query,
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
            &state.query,
        ),
        UnlockTable::ObjectiveValues => draw_objective_values(
            ui,
            "objective_values",
            &unlocks.objective_values,
            OBJECTIVE_VALUE_CAPACITY,
            ACCOUNT_OBJECTIVE_BANK,
            catalog,
            &state.query,
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
            &state.query,
        ),
        UnlockTable::CharacterObjectObjectiveValues => draw_objective_values(
            ui,
            "character_object_objective_values",
            &unlocks.character_objective_values,
            CHARACTER_OBJECT_VALUE_CAPACITY,
            CHARACTER_OBJECTIVE_BANK,
            catalog,
            &state.query,
        ),
    }
}

fn draw_investment(
    ui: &mut egui::Ui,
    investment: &InvestmentPolicy,
    catalog: &Catalog,
    state: &mut UiState,
) {
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
    }
    ui.add_space(4.0);
    draw_filter(ui, &mut state.query);

    match state.investment_table {
        InvestmentTable::FlagOverrides => {
            draw_flag_overrides(ui, &investment.flag_overrides, catalog, &state.query)
        }
        InvestmentTable::ValueOverrides => {
            draw_value_overrides(ui, &investment.value_overrides, catalog, &state.query)
        }
    }
}

fn draw_filter(ui: &mut egui::Ui, query: &mut String) {
    ui.add(
        egui::TextEdit::singleline(query)
            .hint_text("Filter rows…")
            .desired_width(360.0),
    );
    ui.add_space(6.0);
}

#[derive(Clone, Copy)]
struct FlagTableConfig {
    id: &'static str,
    bank: u8,
    capacity: usize,
}

fn draw_flag_runs(
    ui: &mut egui::Ui,
    config: FlagTableConfig,
    rows: &[FlagRun],
    catalog: &Catalog,
    query: &str,
) {
    let slots = expanded_flag_slots(rows, config.capacity);
    draw_flag_slots(
        ui,
        config,
        &slots,
        &format!("{} ranges", rows.len()),
        catalog,
        query,
    );
}

fn draw_flag_indices(
    ui: &mut egui::Ui,
    config: FlagTableConfig,
    rows: &[FlagIndex],
    catalog: &Catalog,
    query: &str,
) {
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
        catalog,
        query,
    );
}

fn draw_flag_slots(
    ui: &mut egui::Ui,
    config: FlagTableConfig,
    slots: &[usize],
    source_summary: &str,
    catalog: &Catalog,
    query: &str,
) {
    let query = query.trim().to_lowercase();
    let filtered = slots
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
    ui.label(format!(
        "{source_summary} · {} set · {mapped} mapped · {named} named",
        slots.len(),
    ))
    .on_hover_text("Definition match: bank + compact slot");

    let state_width = 64.0;
    let definition_width = 176.0;
    let name_width =
        (ui.available_width() - state_width - definition_width - TABLE_COLUMN_GAP * 2.0).max(128.0);
    ui.add_space(4.0);
    table_header(
        ui,
        &[
            (state_width, "Slot"),
            (name_width, "Unlock"),
            (definition_width, "Definition"),
        ],
    );
    ui.separator();
    if filtered.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return;
    }

    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("progression_table", config.id))
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(TABLE_ROW_STRIDE * 3.0))
            .show_rows(ui, TABLE_CELL_HEIGHT, filtered.len(), |ui, range| {
                egui::Grid::new((config.id, "rows"))
                    .num_columns(3)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for row_index in range {
                            let slot = filtered[row_index];
                            table_cell(
                                ui,
                                state_width,
                                egui::RichText::new(slot.to_string()).monospace(),
                            );
                            if let Some((definition_index, definition)) =
                                catalog.unlock_flag_for_state(config.bank, slot)
                            {
                                table_cell(
                                    ui,
                                    name_width,
                                    definition_name(definition).map_or_else(
                                        || egui::RichText::new("—").weak(),
                                        egui::RichText::new,
                                    ),
                                )
                                .on_hover_text(definition_tooltip(
                                    "flag",
                                    definition_index,
                                    definition,
                                ));
                                table_cell(
                                    ui,
                                    definition_width,
                                    egui::RichText::new(definition_identity(
                                        definition_index,
                                        definition,
                                    ))
                                    .monospace(),
                                )
                                .on_hover_text(definition_tooltip(
                                    "flag",
                                    definition_index,
                                    definition,
                                ));
                            } else {
                                table_cell(ui, name_width, egui::RichText::new("—").weak())
                                    .on_hover_text("No package definition");
                                table_cell(ui, definition_width, egui::RichText::new("—").weak())
                                    .on_hover_text("No package definition");
                            }
                            ui.end_row();
                        }
                    });
            });
    });
}

fn draw_objective_values(
    ui: &mut egui::Ui,
    id: &'static str,
    rows: &[IndexedValue],
    _capacity: usize,
    bank: u8,
    catalog: &Catalog,
    query: &str,
) {
    let query = query.trim().to_lowercase();
    let filtered = rows
        .iter()
        .filter(|row| objective_value_matches(&query, row, bank, catalog))
        .collect::<Vec<_>>();
    let mut display_lines = Vec::new();
    for row in &filtered {
        let locations = catalog
            .unlock_value_for_state(bank, row.index)
            .and_then(|(definition_index, _)| catalog.objective_for_unlock_value(definition_index))
            .map_or_else(|| vec![ObjectiveLocation::Unavailable], objective_locations);
        display_lines.extend(locations.into_iter().enumerate().map(
            |(location_index, location)| ObjectiveValueDisplayLine {
                value: row,
                location,
                primary: location_index == 0,
            },
        ));
    }
    let objectives = rows
        .iter()
        .filter_map(|row| catalog.unlock_value_for_state(bank, row.index))
        .filter(|(definition_index, _)| {
            catalog
                .objective_for_unlock_value(*definition_index)
                .is_some()
        })
        .count();
    let named_owners = rows
        .iter()
        .filter_map(|row| catalog.unlock_value_for_state(bank, row.index))
        .filter_map(|(definition_index, _)| catalog.objective_for_unlock_value(definition_index))
        .filter(|objective| preferred_objective_owner(objective).is_some())
        .count();
    ui.label(format!(
        "{} values · {objectives} objectives · {named_owners} owners",
        rows.len(),
    ))
    .on_hover_text(
        "Definition match: bank + compact slot\nObjective match: same hash\nLocation: package owner path",
    );
    ui.label(egui::RichText::new("Target: max/min capped · ≥/≤ uncapped").weak());

    let state_width = 64.0;
    let value_width = 68.0;
    let location_width = (ui.available_width() * 0.28).clamp(164.0, 300.0);
    let target_width = 88.0;
    let objective_width = (ui.available_width()
        - state_width
        - value_width
        - location_width
        - target_width
        - TABLE_COLUMN_GAP * 4.0)
        .max(100.0);
    ui.add_space(4.0);
    table_header(
        ui,
        &[
            (state_width, "Slot"),
            (value_width, "Value"),
            (objective_width, "Goal: Objective"),
            (target_width, "Target"),
            (location_width, "Location"),
        ],
    );
    ui.separator();
    if display_lines.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return;
    }

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
                        for row_index in range {
                            let line = display_lines[row_index];
                            let row = line.value;
                            if line.primary {
                                table_cell(
                                    ui,
                                    state_width,
                                    egui::RichText::new(row.index.to_string()).monospace(),
                                );
                                table_cell(
                                    ui,
                                    value_width,
                                    egui::RichText::new(row.value.to_string()).monospace(),
                                );
                                if let Some((definition_index, definition)) =
                                    catalog.unlock_value_for_state(bank, row.index)
                                {
                                    if let Some(objective) =
                                        catalog.objective_for_unlock_value(definition_index)
                                    {
                                        table_cell(
                                            ui,
                                            objective_width,
                                            objective_goal_text(objective),
                                        )
                                        .on_hover_text(
                                            objective_details_tooltip(
                                                objective,
                                                definition_index,
                                                definition,
                                            ),
                                        );
                                        draw_objective_target(ui, target_width, objective);
                                    } else {
                                        table_cell(
                                            ui,
                                            objective_width,
                                            egui::RichText::new("—").weak(),
                                        )
                                        .on_hover_text(
                                            definition_tooltip(
                                                "value",
                                                definition_index,
                                                definition,
                                            ),
                                        );
                                        table_cell(
                                            ui,
                                            target_width,
                                            egui::RichText::new("—").weak(),
                                        );
                                    }
                                } else {
                                    table_cell(
                                        ui,
                                        objective_width,
                                        egui::RichText::new("—").weak(),
                                    )
                                    .on_hover_text("No package definition");
                                    table_cell(ui, target_width, egui::RichText::new("—").weak());
                                }
                            } else {
                                table_cell(ui, state_width, "");
                                table_cell(ui, value_width, "");
                                table_cell(ui, objective_width, "");
                                table_cell(ui, target_width, "");
                            }
                            draw_objective_location(ui, location_width, line.location);
                            ui.end_row();
                        }
                    });
            });
    });
}

fn draw_flag_overrides(ui: &mut egui::Ui, rows: &[FlagOverride], catalog: &Catalog, query: &str) {
    let query = query.trim().to_lowercase();
    let filtered = rows
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
    ui.label(format!(
        "{} rows · {mapped} mapped · {named} named",
        rows.len()
    ))
    .on_hover_text("Definition index: first value");

    let index_width = 88.0;
    let value_width = 68.0;
    let hash_width = 136.0;
    let name_width =
        (ui.available_width() - index_width - value_width - hash_width - TABLE_COLUMN_GAP * 3.0)
            .max(128.0);
    ui.add_space(4.0);
    table_header(
        ui,
        &[
            (index_width, "Index"),
            (value_width, "Override"),
            (name_width, "Unlock"),
            (hash_width, "Hash"),
        ],
    );
    ui.separator();
    if filtered.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return;
    }
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("progression_table", "family5_flag_overrides"))
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(TABLE_ROW_STRIDE * 3.0))
            .show_rows(ui, TABLE_CELL_HEIGHT, filtered.len(), |ui, range| {
                egui::Grid::new("family5_flag_override_rows")
                    .num_columns(4)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for row_index in range {
                            let row = filtered[row_index];
                            table_cell(
                                ui,
                                index_width,
                                egui::RichText::new(row.definition_index.to_string()).monospace(),
                            );
                            table_cell(
                                ui,
                                value_width,
                                egui::RichText::new(row.value.to_string()).monospace(),
                            );
                            if let Some(definition) =
                                catalog.unlock_flag_definition(row.definition_index)
                            {
                                table_cell(
                                    ui,
                                    name_width,
                                    definition_name(definition).map_or_else(
                                        || egui::RichText::new("—").weak(),
                                        egui::RichText::new,
                                    ),
                                )
                                .on_hover_text(definition_tooltip(
                                    "flag",
                                    row.definition_index,
                                    definition,
                                ));
                                table_cell(
                                    ui,
                                    hash_width,
                                    egui::RichText::new(definition_hash(definition)).monospace(),
                                )
                                .on_hover_text(definition_tooltip(
                                    "flag",
                                    row.definition_index,
                                    definition,
                                ));
                            } else {
                                table_cell(ui, name_width, egui::RichText::new("—").weak())
                                    .on_hover_text("Index not in package table");
                                table_cell(ui, hash_width, egui::RichText::new("—").weak())
                                    .on_hover_text("Index not in package table");
                            }
                            ui.end_row();
                        }
                    });
            });
    });
}

fn draw_value_overrides(ui: &mut egui::Ui, rows: &[ValueOverride], catalog: &Catalog, query: &str) {
    let query = query.trim().to_lowercase();
    let filtered = rows
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
    let objective_matches = rows
        .iter()
        .filter(|row| {
            catalog
                .objective_for_unlock_value(row.definition_index)
                .is_some()
        })
        .count();
    ui.label(format!(
        "{} rows · {mapped} definitions · {objective_matches} objectives",
        rows.len()
    ))
    .on_hover_text("Definition index: first value\nObjective match: same hash");

    let definition_width = 164.0;
    let value_width = 68.0;
    let target_width = 88.0;
    let objective_width = (ui.available_width()
        - definition_width
        - value_width
        - target_width
        - TABLE_COLUMN_GAP * 3.0)
        .max(140.0);
    ui.add_space(4.0);
    table_header(
        ui,
        &[
            (definition_width, "Definition"),
            (value_width, "Override"),
            (objective_width, "Goal: Objective"),
            (target_width, "Target"),
        ],
    );
    ui.separator();
    if filtered.is_empty() {
        ui.label(egui::RichText::new("No matching rows").weak());
        return;
    }
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = TABLE_ROW_GAP;
        egui::ScrollArea::vertical()
            .id_salt(("progression_table", "family5_value_overrides"))
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(TABLE_ROW_STRIDE * 3.0))
            .show_rows(ui, TABLE_CELL_HEIGHT, filtered.len(), |ui, range| {
                egui::Grid::new("family5_value_override_rows")
                    .num_columns(4)
                    .striped(true)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .show(ui, |ui| {
                        for row_index in range {
                            let row = filtered[row_index];
                            table_cell(
                                ui,
                                definition_width,
                                if let Some(definition) =
                                    catalog.unlock_value_definition(row.definition_index)
                                {
                                    egui::RichText::new(definition_identity(
                                        row.definition_index,
                                        definition,
                                    ))
                                    .monospace()
                                } else {
                                    egui::RichText::new("—").weak()
                                },
                            )
                            .on_hover_text(
                                catalog
                                    .unlock_value_definition(row.definition_index)
                                    .map_or_else(
                                        || "Index not in package table".to_owned(),
                                        |definition| {
                                            definition_tooltip(
                                                "value",
                                                row.definition_index,
                                                definition,
                                            )
                                        },
                                    ),
                            );
                            table_cell(
                                ui,
                                value_width,
                                egui::RichText::new(row.value.to_string()).monospace(),
                            );
                            if let Some(definition) =
                                catalog.unlock_value_definition(row.definition_index)
                            {
                                if let Some(objective) =
                                    catalog.objective_for_unlock_value(row.definition_index)
                                {
                                    table_cell(ui, objective_width, objective_goal_text(objective))
                                        .on_hover_text(objective_details_tooltip(
                                            objective,
                                            row.definition_index,
                                            definition,
                                        ));
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
                                table_cell(ui, objective_width, egui::RichText::new("—").weak())
                                    .on_hover_text("Index not in package table");
                                table_cell(ui, target_width, egui::RichText::new("—").weak());
                            }
                            ui.end_row();
                        }
                    });
            });
    });
}

fn table_header(ui: &mut egui::Ui, columns: &[(f32, &str)]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TABLE_COLUMN_GAP;
        for (width, label) in columns {
            table_cell(ui, *width, egui::RichText::new(*label).strong());
        }
    });
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
    format!("#{index} · {}", definition_hash(definition))
}

fn definition_tooltip(kind: &str, index: usize, definition: &UnlockDefinition) -> String {
    let mut lines = vec![format!("{kind} definition #{index}")];
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
    lines.push(format!("Hash: {}", definition_hash(definition)));
    lines.push(format!("Code: 0x{:04X}", definition.code));
    if let Some(slot) = definition.compact_slot {
        lines.push(format!("Compact slot: {slot}"));
    }
    lines.join("\n")
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
        .filter(|owner| !owner.name.trim().is_empty())
        .min_by_key(|owner| objective_owner_priority(owner.kind))
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
            ObjectiveOwnerKind::Record => "Triumph / record",
            ObjectiveOwnerKind::PresentationNode => "Presentation node",
        }
    }
}

fn objective_goal_text(objective: &ObjectiveDef) -> String {
    let description = objective_description(objective);
    let Some(owner) = preferred_objective_owner(objective) else {
        return description;
    };
    if objective.description.trim().is_empty()
        || owner.name.trim().eq_ignore_ascii_case(description.trim())
    {
        owner.name.clone()
    } else {
        format!("{}: {description}", owner.name)
    }
}

fn objective_locations(objective: &ObjectiveDef) -> Vec<ObjectiveLocation<'_>> {
    let mut paths = Vec::<&[String]>::new();
    for priority in 0..=3 {
        for owner in objective
            .owners
            .iter()
            .filter(|owner| objective_owner_priority(owner.kind) == priority)
        {
            for path in &owner.paths {
                let path = path.as_slice();
                if !path.is_empty() && !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
    }
    if !paths.is_empty() {
        return paths.into_iter().map(ObjectiveLocation::Path).collect();
    }

    let mut owner_types = Vec::new();
    for priority in 0..=3 {
        for owner in objective.owners.iter().filter(|owner| {
            objective_owner_priority(owner.kind) == priority && !owner.name.trim().is_empty()
        }) {
            let owner_type = objective_owner_type(owner);
            if !owner_types.contains(&owner_type) {
                owner_types.push(owner_type);
            }
        }
    }
    if owner_types.is_empty() {
        vec![ObjectiveLocation::NoNamedOwner]
    } else {
        owner_types
            .into_iter()
            .map(ObjectiveLocation::OwnerType)
            .collect()
    }
}

fn draw_objective_location(ui: &mut egui::Ui, width: f32, location: ObjectiveLocation<'_>) {
    let text = location.text();
    let rich_text = match location {
        ObjectiveLocation::Path(_) => egui::RichText::new(&text),
        ObjectiveLocation::OwnerType(_)
        | ObjectiveLocation::NoNamedOwner
        | ObjectiveLocation::Unavailable => egui::RichText::new(&text).weak(),
    };
    let response = table_cell(ui, width, rich_text);
    match location {
        ObjectiveLocation::Path(_) => {
            response.on_hover_text(text);
        }
        ObjectiveLocation::OwnerType(_) => {
            response.on_hover_text("No presentation path");
        }
        ObjectiveLocation::NoNamedOwner => {
            response.on_hover_text("No named package owner");
        }
        ObjectiveLocation::Unavailable => {}
    }
}

fn objective_details_tooltip(
    objective: &ObjectiveDef,
    definition_index: usize,
    definition: &UnlockDefinition,
) -> String {
    let mut lines = vec![
        format!("Objective: {}", objective_description(objective)),
        format!("Objective hash: 0x{:08X}", objective.hash),
        "Match: same package hash".into(),
    ];
    if !objective
        .owners
        .iter()
        .any(|owner| !owner.name.trim().is_empty())
    {
        lines.push("Named package owner: not found".into());
    } else {
        lines.push("Package owners:".into());
        for owner in objective
            .owners
            .iter()
            .filter(|owner| !owner.name.trim().is_empty())
        {
            lines.push(format!("{}: {}", objective_owner_type(owner), owner.name));
            lines.extend(
                owner
                    .paths
                    .iter()
                    .map(|path| format!("  {}", path.join(" > "))),
            );
        }
    }
    lines.push(format!("Value definition #{definition_index}"));
    lines.push(format!("Value hash: {}", definition_hash(definition)));
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
                || objective_owner_type(owner).to_lowercase().contains(query)
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
        };

        assert!(definition_matches("#12913", 12_913, &definition));
        assert!(definition_matches("0x1304c3fa", 12_913, &definition));
        assert!(definition_matches("1304c3fa", 12_913, &definition));
        assert!(definition_matches("sweet business", 12_913, &definition));
        assert!(!definition_matches("0xdeadbeef", 12_913, &definition));
    }

    #[test]
    fn objective_summary_includes_goal_location_and_limit_semantics() {
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
                paths: vec![vec!["Account".into(), "Metrics".into()]],
            }],
            ..ObjectiveDef::default()
        };

        assert_eq!(objective_goal_text(&objective), "Arc Final Blows: Arc");
        assert_eq!(
            objective_locations(&objective)
                .into_iter()
                .map(ObjectiveLocation::text)
                .collect::<Vec<_>>(),
            vec!["Account > Metrics"]
        );
        assert_eq!(objective_target_text(&objective), "≥5000");
        assert!(objective_target_tooltip(&objective).contains("Over-completion: allowed"));
        assert!(objective_matches("account", &objective));
        assert!(objective_matches("metric", &objective));

        objective.allow_overcompletion = false;
        assert_eq!(objective_target_text(&objective), "5000 max");
        assert!(objective_target_tooltip(&objective).contains("Over-completion: not allowed"));
        assert!(objective_matches("capped", &objective));

        objective.is_counting_downward = true;
        assert_eq!(objective_target_text(&objective), "5000 min");
        objective.allow_overcompletion = true;
        assert_eq!(objective_target_text(&objective), "≤5000");
    }

    #[test]
    fn objective_locations_list_every_distinct_package_path_without_a_cap() {
        let paths = (0..256)
            .map(|index| vec![format!("Branch {index}"), "Metrics".into()])
            .collect::<Vec<_>>();
        let objective = ObjectiveDef {
            owners: vec![
                ObjectiveOwnerDef {
                    hash: 1,
                    kind: ObjectiveOwnerKind::Metric,
                    name: "Metric".into(),
                    type_name: "Metric".into(),
                    paths: paths.clone(),
                },
                ObjectiveOwnerDef {
                    hash: 2,
                    kind: ObjectiveOwnerKind::Record,
                    name: "Record".into(),
                    type_name: "Triumph / record".into(),
                    paths: vec![paths[0].clone(), vec!["Account".into(), "Triumphs".into()]],
                },
            ],
            ..ObjectiveDef::default()
        };

        let locations = objective_locations(&objective)
            .into_iter()
            .map(ObjectiveLocation::text)
            .collect::<Vec<_>>();

        assert_eq!(locations.len(), 257);
        assert_eq!(locations.first().unwrap(), "Branch 0 > Metrics");
        assert_eq!(locations.get(255).unwrap(), "Branch 255 > Metrics");
        assert_eq!(locations.last().unwrap(), "Account > Triumphs");
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
