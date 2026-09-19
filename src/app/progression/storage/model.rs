use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct Key {
    pub family: bool,
    pub bank: usize,
    pub slot: usize,
    pub lane: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
    Unlock,
    Counter,
    RankProgress,
    RankData,
    FlagOverride,
    CounterOverride,
    Unknown,
}
impl Kind {
    pub const ALL: [Self; 7] = [
        Self::Unlock,
        Self::Counter,
        Self::RankProgress,
        Self::RankData,
        Self::FlagOverride,
        Self::CounterOverride,
        Self::Unknown,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Unlock => "Unlock",
            Self::Counter => "Counter",
            Self::RankProgress => "Rank Progress",
            Self::RankData => "Extra Rank Value",
            Self::FlagOverride => "Flag Override",
            Self::CounterOverride => "Counter Override",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Debug)]
pub(super) struct Row {
    pub key: Key,
    pub value: i64,
    pub name: String,
    pub kind: Kind,
    pub scope: &'static str,
    pub location: String,
    pub search: String,
    pub definition: Option<(usize, bool)>,
    pub hash: Option<u64>,
    pub blocked: Option<&'static str>,
}

pub(super) fn field(key: Key) -> Option<&'static str> {
    if key.family {
        return match key.bank {
            0 => Some("family5_flag_overrides"),
            1 => Some("family5_value_overrides"),
            _ => None,
        };
    }
    match key.bank {
        0 => Some("account_flag_runs"),
        1 => Some("profile_flag_runs"),
        2 => Some("character_flags"),
        3 => Some("objective_values"),
        4 => Some("character_object_flag_runs"),
        5 => Some("character_object_objective_values"),
        6 => Some("account_progressions"),
        7 => Some("character_progressions"),
        _ => None,
    }
}

fn kind_scope(key: Key) -> (Kind, &'static str) {
    match (key.family, key.bank, key.lane) {
        (true, 0, _) => (Kind::FlagOverride, "Account"),
        (true, 1, _) => (Kind::CounterOverride, "Account"),
        (false, 0, _) => (Kind::Unlock, "Account"),
        (false, 1, _) => (Kind::Unlock, "Profile"),
        (false, 2, _) => (Kind::Unlock, "Character"),
        (false, 3, _) => (Kind::Counter, "Account"),
        (false, 4, _) => (Kind::Unlock, "Character Object"),
        (false, 5, _) => (Kind::Counter, "Character Object"),
        (false, 6, 0) => (Kind::RankProgress, "Account"),
        (false, 7, 0) => (Kind::RankProgress, "Character"),
        (false, 6, _) => (Kind::RankData, "Account"),
        (false, 7, _) => (Kind::RankData, "Character"),
        _ => (Kind::Unknown, "Unknown"),
    }
}

fn definition(catalog: &Catalog, key: Key) -> Option<(usize, bool)> {
    match (key.family, key.bank) {
        (true, 0) => catalog
            .unlock_flag_definition(key.slot)
            .map(|_| (key.slot, false)),
        (true, 1) => catalog
            .unlock_value_definition(key.slot)
            .map(|_| (key.slot, true)),
        (false, 0) => catalog
            .unlock_flag_for_state(1, key.slot)
            .map(|(index, _)| (index, false)),
        (false, 1) => catalog
            .unlock_flag_for_state(2, key.slot)
            .map(|(index, _)| (index, false)),
        (false, 2) => catalog
            .unlock_flag_for_state(6, key.slot)
            .map(|(index, _)| (index, false)),
        (false, 3) => catalog
            .unlock_value_for_state(1, key.slot)
            .map(|(index, _)| (index, true)),
        (false, 4) => catalog
            .unlock_flag_for_state(3, key.slot)
            .map(|(index, _)| (index, false)),
        (false, 5) => catalog
            .unlock_value_for_state(2, key.slot)
            .map(|(index, _)| (index, true)),
        _ => None,
    }
}

fn row(key: Key, value: i64, catalog: &Catalog, native: bool) -> Row {
    let (kind, scope) = kind_scope(key);
    let definition = definition(catalog, key);
    let progression = matches!(kind, Kind::RankProgress | Kind::RankData)
        .then(|| catalog.progression_definition(key.slot))
        .flatten();
    let (name, hash) = if let Some((index, value)) = definition {
        let label = labels::unlock(catalog, index, value);
        let hash = if value {
            catalog.unlock_value_definition(index)
        } else {
            catalog.unlock_flag_definition(index)
        }
        .map(|definition| definition.hash);
        (format!("{} · {}", label.text, label.purpose), hash)
    } else if let Some(progression) = progression {
        (
            progression_display_name(progression)
                .unwrap_or_else(|| format!("Unnamed Rank #{}", key.slot)),
            Some(progression.hash),
        )
    } else {
        (format!("Unmapped {}", kind.label()), None)
    };
    let location = if key.family {
        format!("Definition #{}", key.slot)
    } else if matches!(kind, Kind::RankProgress | Kind::RankData) {
        format!("Rank #{} · Value {}", key.slot, key.lane)
    } else {
        format!("Slot {}", key.slot)
    };
    let blocked = blocked(key, value, kind, definition, native);
    let search = format!(
        "{name} {location} {scope} {} {value} {} {}",
        kind.label(),
        field(key).unwrap_or("unknown"),
        hash.map_or_else(String::new, |hash| format!(
            "{hash} {}",
            format_hash_hex(hash)
        ))
    )
    .to_lowercase();
    Row {
        key,
        value,
        name,
        kind,
        scope,
        location,
        search,
        definition,
        hash,
        blocked,
    }
}

fn blocked(
    key: Key,
    value: i64,
    kind: Kind,
    definition: Option<(usize, bool)>,
    native: bool,
) -> Option<&'static str> {
    if field(key).is_none() {
        Some("This saved bank has no supported editor")
    } else if !key.family && key.bank < 6 && key.lane != 0 {
        Some("This saved lane has no supported editor")
    } else if i32::try_from(value).is_err() {
        Some("This raw value exceeds the editor's integer range")
    } else if (kind == Kind::Unlock && ![0, 2].contains(&value))
        || (kind == Kind::FlagOverride && !(0..=2).contains(&value))
    {
        Some("This raw flag value is preserved as stored")
    } else if matches!(kind, Kind::RankProgress | Kind::RankData) && !(0..=2).contains(&key.lane) {
        Some("This rank field has no supported editor")
    } else if key.family
        && key.slot
            > if key.bank == 0 {
                FAMILY5_FLAG_SLOT_MAXIMUM
            } else {
                FAMILY5_VALUE_SLOT_MAXIMUM
            }
    {
        Some("This definition is outside the editable override range")
    } else if native
        && ((key.bank == 6 && !key.family && (38..=41).contains(&key.slot))
            || definition.is_some_and(|(index, value)| {
                value && super::super::seasonal::is_derived_value(index)
            }))
    {
        Some("Edit this value in Seasonal")
    } else if !native
        && key.bank == 5
        && !key.family
        && RESERVED_CHARACTER_OBJECTIVE_VALUES
            .iter()
            .any(|(slot, _)| *slot == key.slot)
    {
        Some("This slot is reserved by the JSON account format")
    } else {
        None
    }
}

fn stored_values(document: &Value, catalog: &Catalog) -> Result<BTreeMap<Key, i64>, String> {
    let mut stored = BTreeMap::new();
    let native = document.get("_native_progression").is_some();
    if native {
        for (family, source) in [(false, "unlocks"), (true, "family")] {
            for entry in document["_native_progression"][source]
                .as_array()
                .into_iter()
                .flatten()
            {
                let bank = entry[0]
                    .as_u64()
                    .and_then(|bank| usize::try_from(bank).ok())
                    .ok_or("Invalid saved bank")?;
                let slot = entry[1]
                    .as_u64()
                    .and_then(|slot| usize::try_from(slot).ok())
                    .ok_or("Invalid saved slot")?;
                let lane = if family {
                    0
                } else {
                    entry[2].as_i64().ok_or("Invalid saved lane")?
                };
                let value = entry[if family { 2 } else { 3 }]
                    .as_i64()
                    .ok_or("Invalid saved value")?;
                stored.insert(
                    Key {
                        family,
                        bank,
                        slot,
                        lane,
                    },
                    value,
                );
            }
        }
    }
    // The editable projection can contain changes newer than the raw native snapshot.
    // Keep unprojected bytes and explicit zero rows, then overlay the current projection.
    if native {
        stored.retain(|key, value| {
            *value == 0
                || blocked(
                    *key,
                    *value,
                    kind_scope(*key).0,
                    definition(catalog, *key),
                    true,
                )
                .is_some()
        });
    }
    stored.extend(projected(document)?);
    Ok(stored)
}

fn projected(document: &Value) -> Result<BTreeMap<Key, i64>, String> {
    let mut stored = BTreeMap::new();
    let policy = parse(document)?;
    for (bank, slots) in [
        (
            0,
            expanded_flag_slots(&policy.unlocks.account_flag_runs, ACCOUNT_FLAG_CAPACITY),
        ),
        (
            1,
            expanded_flag_slots(&policy.unlocks.profile_flag_runs, PROFILE_FLAG_CAPACITY),
        ),
        (
            2,
            policy
                .unlocks
                .character_flags
                .iter()
                .map(|row| row.index)
                .collect(),
        ),
        (
            4,
            expanded_flag_slots(
                &policy.unlocks.character_object_flag_runs,
                CHARACTER_OBJECT_FLAG_CAPACITY,
            ),
        ),
    ] {
        for slot in slots {
            stored.insert(
                Key {
                    family: false,
                    bank,
                    slot,
                    lane: 0,
                },
                2,
            );
        }
    }
    for (bank, values) in [
        (3, &policy.unlocks.objective_values),
        (5, &policy.unlocks.character_objective_values),
    ] {
        for value in values {
            stored.insert(
                Key {
                    family: false,
                    bank,
                    slot: value.index,
                    lane: 0,
                },
                i64::from(value.value),
            );
        }
    }
    for (bank, values) in [
        (6, &policy.unlocks.account_progressions),
        (7, &policy.unlocks.character_progressions),
    ] {
        for value in values {
            for (lane, saved) in value.lanes.iter().enumerate() {
                stored.insert(
                    Key {
                        family: false,
                        bank,
                        slot: value.definition_index,
                        lane: lane as i64,
                    },
                    i64::from(*saved),
                );
            }
        }
    }
    for value in &policy.investment.flag_overrides {
        stored.insert(
            Key {
                family: true,
                bank: 0,
                slot: value.definition_index,
                lane: 0,
            },
            i64::from(value.value),
        );
    }
    for value in &policy.investment.value_overrides {
        stored.insert(
            Key {
                family: true,
                bank: 1,
                slot: value.definition_index,
                lane: 0,
            },
            i64::from(value.value),
        );
    }
    Ok(stored)
}

pub(super) fn filtered_rows(rows: &[Row], filter: &Filter, catalog: &Catalog) -> Vec<usize> {
    let mut filtered = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            let mode = match filter.mode {
                Mode::All => true,
                Mode::Overrides(table) => {
                    row.key.family
                        && row.key.bank == usize::from(table == InvestmentTable::ValueOverrides)
                }
            };
            let definition = row.definition.and_then(|(index, value)| {
                if value {
                    catalog.unlock_value_definition(index)
                } else {
                    catalog.unlock_flag_definition(index)
                }
            });
            mode && filter.kind.is_none_or(|kind| row.kind == kind)
                && filter.scope.is_none_or(|scope| row.scope == scope)
                && (filter.mode == Mode::All
                    || override_filter_matches(filter.coverage, definition))
                && filter
                    .query
                    .split_whitespace()
                    .all(|term| row.search.contains(term))
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match filter.sort.column {
        1 => filtered.sort_by_key(|index| rows[*index].kind),
        2 => filtered.sort_by_key(|index| rows[*index].scope),
        3 => filtered.sort_by_key(|index| rows[*index].value),
        4 => filtered.sort_by_key(|index| rows[*index].key),
        _ => filtered.sort_by_cached_key(|index| rows[*index].name.to_lowercase()),
    }
    if filter.sort.descending {
        filtered.reverse();
    }
    filtered
}

#[cfg(test)]
pub(super) fn rows(document: &Value, catalog: &Catalog) -> Result<Vec<Row>, String> {
    Ok(stored_values(document, catalog)?
        .into_iter()
        .map(|(key, value)| {
            row(
                key,
                value,
                catalog,
                document.get("_native_progression").is_some(),
            )
        })
        .collect())
}

#[derive(Debug)]
pub(super) struct Preparing {
    pending: std::collections::btree_map::IntoIter<Key, i64>,
    rows: Vec<Row>,
    native: bool,
}
impl Preparing {
    pub fn new(document: &Value, catalog: &Catalog, mode: Mode) -> Result<Self, String> {
        let mut values = stored_values(document, catalog)?;
        if let Mode::Overrides(table) = mode {
            values.retain(|key, _| {
                key.family && key.bank == usize::from(table == InvestmentTable::ValueOverrides)
            });
        }
        Ok(Self {
            rows: Vec::with_capacity(values.len()),
            pending: values.into_iter(),
            native: document.get("_native_progression").is_some(),
        })
    }
    pub fn step(&mut self, catalog: &Catalog) -> bool {
        let start = std::time::Instant::now();
        for (key, value) in self.pending.by_ref() {
            self.rows.push(row(key, value, catalog, self.native));
            if start.elapsed() >= std::time::Duration::from_millis(6) {
                break;
            }
        }
        self.pending.len() == 0
    }
    pub fn progress(&self) -> (usize, usize) {
        (self.rows.len(), self.rows.len() + self.pending.len())
    }
    pub fn finish(self) -> Vec<Row> {
        self.rows
    }
}
