//! Lossless JSON projection for profile items and dismantle rewards.

use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU64,
};

use serde_json::{Map, Value};
use sundial_account::{
    DefinitionHash, DismantleGearClass, DismantleRarity, DismantleReward, DismantleRewardCommand,
    EntityId, ProfileCapabilities, ProfileItem, ProfileItemCommand, ProfileState,
};

use crate::hash::parse_unsigned_value;

use super::JsonAccountError;

type JsonProfileError = JsonAccountError;

const MIN_SUPPORTED_JSON_SCHEMA: u64 = 2;
const MAX_SUPPORTED_JSON_SCHEMA: u64 = 8;
const LEGACY_PROFILE_ITEM_CAPACITY: usize = 32;
const PROFILE_ITEM_CAPACITY: usize = 701;
const LEGACY_DISMANTLE_REWARD_CAPACITY: usize = 8;
const FILTERED_DISMANTLE_REWARD_CAPACITY: usize = 32;

type JsonProfileResult<T> = Result<T, JsonAccountError>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ProfileFieldChanges {
    definition_hash: bool,
    quantity: bool,
}

struct LoadedRows<T> {
    entities: Vec<T>,
    order: Vec<EntityId>,
    raw: BTreeMap<EntityId, Map<String, Value>>,
}

impl<T> Default for LoadedRows<T> {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            order: Vec::new(),
            raw: BTreeMap::new(),
        }
    }
}

impl ProfileFieldChanges {
    const ALL: Self = Self {
        definition_hash: true,
        quantity: true,
    };
}

/// A loaded JSON profile projection with adapter-owned raw row sidecars.
///
/// The raw rows retain unknown members and the original representation of unchanged known fields.
/// Applying a command returns a new adapter and document, leaving both inputs untouched on error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JsonProfileAdapter {
    schema_version: u64,
    capabilities: ProfileCapabilities,
    state: ProfileState,
    next_entity_id: NonZeroU64,
    profile_order: Vec<EntityId>,
    profile_rows: BTreeMap<EntityId, Map<String, Value>>,
    profile_changes: BTreeMap<EntityId, ProfileFieldChanges>,
    dismantle_managed: bool,
    dismantle_order: Vec<EntityId>,
    dismantle_rows: BTreeMap<EntityId, Map<String, Value>>,
    rewritten_dismantle_rows: BTreeSet<EntityId>,
}

impl JsonProfileAdapter {
    pub(crate) fn load(document: &Value) -> JsonProfileResult<Self> {
        Self::load_scoped(document, true)
    }

    /// Loads only profile items, leaving dismantle data completely opaque.
    ///
    /// Profile-item edits historically do not validate or rewrite the sibling dismantle section.
    pub(crate) fn load_profile_items(document: &Value) -> JsonProfileResult<Self> {
        Self::load_scoped(document, false)
    }

    fn load_scoped(document: &Value, load_dismantle: bool) -> JsonProfileResult<Self> {
        let schema_version = schema_version(document)?;
        let capabilities = capabilities_for_schema(schema_version)?;
        let account = optional_account(document)?;
        let mut next_id = 1_u64;

        let profile = load_profile_items(account, capabilities, &mut next_id)?;
        let dismantle_managed =
            load_dismantle && (5..=MAX_SUPPORTED_JSON_SCHEMA).contains(&schema_version);
        let dismantle = if dismantle_managed {
            load_dismantle_rewards(account, capabilities, &mut next_id)?
        } else {
            LoadedRows::default()
        };
        let state = ProfileState::try_new(capabilities, profile.entities, dismantle.entities)?;
        let next_entity_id =
            NonZeroU64::new(next_id).ok_or(JsonProfileError::EntityIdentityExhausted)?;

        Ok(Self {
            schema_version,
            capabilities,
            state,
            next_entity_id,
            profile_order: profile.order,
            profile_rows: profile.raw,
            profile_changes: BTreeMap::new(),
            dismantle_managed,
            dismantle_order: dismantle.order,
            dismantle_rows: dismantle.raw,
            rewritten_dismantle_rows: BTreeSet::new(),
        })
    }

    #[cfg(test)]
    pub(crate) const fn capabilities(&self) -> ProfileCapabilities {
        self.capabilities
    }

    pub(crate) const fn state(&self) -> &ProfileState {
        &self.state
    }

    pub(crate) const fn next_entity_id(&self) -> EntityId {
        EntityId::new(self.next_entity_id)
    }

    pub(crate) fn apply_profile_item(
        &self,
        document: &Value,
        command: ProfileItemCommand,
    ) -> JsonProfileResult<(Self, Value)> {
        let mut candidate = self.clone();
        let (id, changes) = match &command {
            ProfileItemCommand::Add(item) => (item.id, Some(ProfileFieldChanges::ALL)),
            ProfileItemCommand::SetDefinitionHash { id, .. } => (
                *id,
                Some(ProfileFieldChanges {
                    definition_hash: true,
                    quantity: false,
                }),
            ),
            ProfileItemCommand::SetQuantity { id, .. } => (
                *id,
                Some(ProfileFieldChanges {
                    definition_hash: false,
                    quantity: true,
                }),
            ),
            ProfileItemCommand::Remove { id } => (*id, None),
        };
        candidate
            .state
            .apply_profile_item(candidate.capabilities, command)?;
        if let Some(changes) = changes {
            let accumulated = candidate.profile_changes.entry(id).or_default();
            accumulated.definition_hash |= changes.definition_hash;
            accumulated.quantity |= changes.quantity;
        }
        candidate.advance_next_entity_id(id)?;
        let projected = candidate.project(document)?;
        Ok((candidate, projected))
    }

    pub(crate) fn apply_dismantle_reward(
        &self,
        document: &Value,
        command: DismantleRewardCommand,
    ) -> JsonProfileResult<(Self, Value)> {
        if !self.dismantle_managed && self.capabilities.dismantle_rewards_writable {
            return Err(JsonProfileError::format(
                "/state/account/dismantle_rewards",
                "dismantle rewards were not loaded by this JSON projection",
            ));
        }
        let mut candidate = self.clone();
        let (id, rewrite) = match &command {
            DismantleRewardCommand::AddForDefinition { id, .. } => (*id, true),
            DismantleRewardCommand::SetPolicy(reward) => (reward.id, true),
            DismantleRewardCommand::Remove { id } => (*id, false),
        };
        candidate
            .state
            .apply_dismantle_reward(candidate.capabilities, command)?;
        if rewrite {
            candidate.rewritten_dismantle_rows.insert(id);
        }
        candidate.advance_next_entity_id(id)?;
        let projected = candidate.project(document)?;
        Ok((candidate, projected))
    }

    fn advance_next_entity_id(&mut self, observed: EntityId) -> JsonProfileResult<()> {
        if observed.get() < self.next_entity_id.get() {
            return Ok(());
        }
        self.next_entity_id = observed
            .get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(JsonProfileError::EntityIdentityExhausted)?;
        Ok(())
    }

    fn project(&self, document: &Value) -> JsonProfileResult<Value> {
        if schema_version(document)? != self.schema_version {
            return Err(JsonProfileError::format(
                "/version",
                "the JSON schema changed after the account projection was loaded",
            ));
        }

        let mut candidate = document.clone();
        if self.profile_changed() {
            let profile_items = self
                .state
                .profile_items()
                .iter()
                .map(|item| Value::Object(self.project_profile_item(item)))
                .collect();
            account_mut(&mut candidate)?
                .insert("profile_items".into(), Value::Array(profile_items));
        }
        if self.dismantle_changed() {
            let rewards = self
                .state
                .dismantle_rewards()
                .iter()
                .map(|reward| Value::Object(self.project_dismantle_reward(reward)))
                .collect();
            account_mut(&mut candidate)?.insert("dismantle_rewards".into(), Value::Array(rewards));
        }
        Ok(candidate)
    }

    fn profile_changed(&self) -> bool {
        !self.profile_changes.is_empty()
            || self.profile_order
                != self
                    .state
                    .profile_items()
                    .iter()
                    .map(|item| item.id)
                    .collect::<Vec<_>>()
    }

    fn dismantle_changed(&self) -> bool {
        self.dismantle_managed
            && (!self.rewritten_dismantle_rows.is_empty()
                || self.dismantle_order
                    != self
                        .state
                        .dismantle_rewards()
                        .iter()
                        .map(|reward| reward.id)
                        .collect::<Vec<_>>())
    }

    fn project_profile_item(&self, item: &ProfileItem) -> Map<String, Value> {
        let mut row = self.profile_rows.get(&item.id).cloned().unwrap_or_default();
        let changes = self
            .profile_changes
            .get(&item.id)
            .copied()
            .unwrap_or_else(|| {
                if self.profile_rows.contains_key(&item.id) {
                    ProfileFieldChanges::default()
                } else {
                    ProfileFieldChanges::ALL
                }
            });
        if changes.definition_hash {
            row.insert(
                "definition_hash".into(),
                Value::String(format_definition_hash(item.definition_hash)),
            );
        }
        if changes.quantity {
            row.insert("quantity".into(), Value::from(item.quantity));
        }
        row
    }

    fn project_dismantle_reward(&self, reward: &DismantleReward) -> Map<String, Value> {
        let mut row = self
            .dismantle_rows
            .get(&reward.id)
            .cloned()
            .unwrap_or_default();
        if !self.rewritten_dismantle_rows.contains(&reward.id)
            && self.dismantle_rows.contains_key(&reward.id)
        {
            return row;
        }

        row.insert(
            "definition_hash".into(),
            Value::String(format_definition_hash(reward.definition_hash)),
        );
        row.insert("quantity".into(), Value::from(reward.quantity));
        if self.capabilities.filtered_dismantle_rewards {
            write_rarity_filter(&mut row, &reward.rarities);
            match reward.gear_class {
                Some(gear_class) => {
                    row.insert(
                        "class".into(),
                        Value::String(gear_class_token(gear_class).into()),
                    );
                }
                None => {
                    row.remove("class");
                }
            }
            match reward.masterworked {
                Some(masterworked) => {
                    row.insert("masterworked".into(), Value::Bool(masterworked));
                }
                None => {
                    row.remove("masterworked");
                }
            }
        } else {
            row.remove("rarity");
            row.remove("class");
            row.remove("masterworked");
        }
        row
    }
}

fn schema_version(document: &Value) -> JsonProfileResult<u64> {
    document
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            JsonProfileError::format("/version", "settings schema version is missing or invalid")
        })
}

fn capabilities_for_schema(schema_version: u64) -> JsonProfileResult<ProfileCapabilities> {
    if schema_version < MIN_SUPPORTED_JSON_SCHEMA {
        return Err(JsonProfileError::format(
            "/version",
            format!(
                "settings schema {schema_version} predates supported schema {MIN_SUPPORTED_JSON_SCHEMA}"
            ),
        ));
    }
    let future = schema_version > MAX_SUPPORTED_JSON_SCHEMA;
    Ok(ProfileCapabilities {
        profile_items_writable: true,
        profile_item_capacity: Some(if schema_version <= 3 {
            LEGACY_PROFILE_ITEM_CAPACITY
        } else {
            PROFILE_ITEM_CAPACITY
        }),
        enforce_loaded_profile_item_capacity: !future,
        dismantle_rewards_writable: (5..=MAX_SUPPORTED_JSON_SCHEMA).contains(&schema_version),
        dismantle_reward_capacity: match schema_version {
            5..=7 => Some(LEGACY_DISMANTLE_REWARD_CAPACITY),
            MAX_SUPPORTED_JSON_SCHEMA => Some(FILTERED_DISMANTLE_REWARD_CAPACITY),
            _ => None,
        },
        filtered_dismantle_rewards: schema_version >= MAX_SUPPORTED_JSON_SCHEMA,
        combined_dismantle_gear_class: false,
    })
}

fn optional_account(document: &Value) -> JsonProfileResult<Option<&Map<String, Value>>> {
    let root = document
        .as_object()
        .ok_or_else(|| JsonProfileError::format("", "settings document must be an object"))?;
    let Some(state) = root.get("state") else {
        return Ok(None);
    };
    let state = state
        .as_object()
        .ok_or_else(|| JsonProfileError::format("/state", "state must be an object"))?;
    let Some(account) = state.get("account") else {
        return Ok(None);
    };
    account
        .as_object()
        .map(Some)
        .ok_or_else(|| JsonProfileError::format("/state/account", "account must be an object"))
}

fn account_mut(document: &mut Value) -> JsonProfileResult<&mut Map<String, Value>> {
    document
        .get_mut("state")
        .and_then(Value::as_object_mut)
        .and_then(|state| state.get_mut("account"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| JsonProfileError::format("/state/account", "account must be an object"))
}

fn load_profile_items(
    account: Option<&Map<String, Value>>,
    capabilities: ProfileCapabilities,
    next_id: &mut u64,
) -> JsonProfileResult<LoadedRows<ProfileItem>> {
    let Some(value) = account.and_then(|account| account.get("profile_items")) else {
        return Ok(LoadedRows::default());
    };
    let rows = value.as_array().ok_or_else(|| {
        JsonProfileError::format(
            "/state/account/profile_items",
            "profile_items must be an array",
        )
    })?;
    if capabilities.enforce_loaded_profile_item_capacity
        && let Some(capacity) = capabilities.profile_item_capacity
        && rows.len() > capacity
    {
        return Err(JsonProfileError::format(
            "/state/account/profile_items",
            format!(
                "profile_items contains {} rows; maximum is {capacity}",
                rows.len()
            ),
        ));
    }

    let mut items = Vec::with_capacity(rows.len());
    let mut order = Vec::with_capacity(rows.len());
    let mut raw = BTreeMap::new();
    for (index, value) in rows.iter().enumerate() {
        let path = format!("/state/account/profile_items/{index}");
        let row = value
            .as_object()
            .ok_or_else(|| JsonProfileError::format(&path, "profile item must be an object"))?;
        let id = take_entity_id(next_id)?;
        items.push(ProfileItem {
            id,
            definition_hash: parse_definition_hash(row, &path)?,
            quantity: parse_quantity(row, &path)?,
        });
        order.push(id);
        raw.insert(id, row.clone());
    }
    Ok(LoadedRows {
        entities: items,
        order,
        raw,
    })
}

fn load_dismantle_rewards(
    account: Option<&Map<String, Value>>,
    capabilities: ProfileCapabilities,
    next_id: &mut u64,
) -> JsonProfileResult<LoadedRows<DismantleReward>> {
    let Some(value) = account.and_then(|account| account.get("dismantle_rewards")) else {
        return Ok(LoadedRows::default());
    };
    let rows = value.as_array().ok_or_else(|| {
        JsonProfileError::format(
            "/state/account/dismantle_rewards",
            "dismantle_rewards must be an array",
        )
    })?;
    if let Some(capacity) = capabilities.dismantle_reward_capacity
        && rows.len() > capacity
    {
        return Err(JsonProfileError::format(
            "/state/account/dismantle_rewards",
            format!(
                "dismantle_rewards contains {} rows; maximum is {capacity}",
                rows.len()
            ),
        ));
    }

    let mut rewards = Vec::with_capacity(rows.len());
    let mut order = Vec::with_capacity(rows.len());
    let mut raw = BTreeMap::new();
    for (index, value) in rows.iter().enumerate() {
        let path = format!("/state/account/dismantle_rewards/{index}");
        let row = value
            .as_object()
            .ok_or_else(|| JsonProfileError::format(&path, "dismantle reward must be an object"))?;
        let id = take_entity_id(next_id)?;
        let definition_hash = parse_definition_hash(row, &path)?;
        if definition_hash.get() == 0 {
            return Err(JsonProfileError::format(
                format!("{path}/definition_hash"),
                "definition_hash must be nonzero",
            ));
        }
        let (rarities, gear_class, masterworked) = if capabilities.filtered_dismantle_rewards {
            (
                parse_rarity_filter(row.get("rarity"), &format!("{path}/rarity"))?,
                parse_gear_class(row.get("class"), &format!("{path}/class"))?,
                parse_optional_bool(row.get("masterworked"), &format!("{path}/masterworked"))?,
            )
        } else {
            (Vec::new(), None, None)
        };
        rewards.push(DismantleReward {
            id,
            definition_hash,
            quantity: parse_quantity(row, &path)?,
            rarities,
            gear_class,
            masterworked,
        });
        order.push(id);
        raw.insert(id, row.clone());
    }
    Ok(LoadedRows {
        entities: rewards,
        order,
        raw,
    })
}

fn take_entity_id(next_id: &mut u64) -> JsonProfileResult<EntityId> {
    let id = NonZeroU64::new(*next_id).ok_or(JsonProfileError::EntityIdentityExhausted)?;
    *next_id = next_id
        .checked_add(1)
        .ok_or(JsonProfileError::EntityIdentityExhausted)?;
    Ok(EntityId::new(id))
}

fn parse_definition_hash(
    row: &Map<String, Value>,
    row_path: &str,
) -> JsonProfileResult<DefinitionHash> {
    let path = format!("{row_path}/definition_hash");
    let value = row
        .get("definition_hash")
        .ok_or_else(|| JsonProfileError::format(&path, "item is missing definition_hash"))?;
    let hash = parse_unsigned_value(value).ok_or_else(|| {
        JsonProfileError::format(
            &path,
            "definition_hash must be an unsigned integer or a 0x hex string",
        )
    })?;
    u32::try_from(hash).map(DefinitionHash::new).map_err(|_| {
        JsonProfileError::format(
            &path,
            "definition_hash must fit in an unsigned 32-bit value",
        )
    })
}

fn parse_quantity(row: &Map<String, Value>, row_path: &str) -> JsonProfileResult<i32> {
    let path = format!("{row_path}/quantity");
    let quantity = row
        .get("quantity")
        .and_then(Value::as_i64)
        .ok_or_else(|| JsonProfileError::format(&path, "quantity must be a positive integer"))?;
    i32::try_from(quantity)
        .ok()
        .filter(|quantity| *quantity > 0)
        .ok_or_else(|| {
            JsonProfileError::format(&path, "quantity must be a positive 32-bit integer")
        })
}

fn parse_rarity_filter(
    value: Option<&Value>,
    path: &str,
) -> JsonProfileResult<Vec<DismantleRarity>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if let Some(token) = value.as_str() {
        return parse_rarity(token, path).map(|rarity| vec![rarity]);
    }
    let values = value.as_array().ok_or_else(|| {
        JsonProfileError::format(path, "rarity must be a name or an array of names")
    })?;
    if values.is_empty() {
        return Err(JsonProfileError::format(
            path,
            "rarity arrays must contain at least one name",
        ));
    }
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let value_path = format!("{path}/{index}");
            value
                .as_str()
                .ok_or_else(|| {
                    JsonProfileError::format(&value_path, "rarity must contain rarity names")
                })
                .and_then(|token| parse_rarity(token, &value_path))
        })
        .collect()
}

fn parse_rarity(token: &str, path: &str) -> JsonProfileResult<DismantleRarity> {
    match token {
        "common" => Ok(DismantleRarity::Common),
        "uncommon" => Ok(DismantleRarity::Uncommon),
        "rare" => Ok(DismantleRarity::Rare),
        "legendary" => Ok(DismantleRarity::Legendary),
        "exotic" => Ok(DismantleRarity::Exotic),
        _ => Err(JsonProfileError::format(
            path,
            "rarity must be common, uncommon, rare, legendary, or exotic",
        )),
    }
}

fn parse_gear_class(
    value: Option<&Value>,
    path: &str,
) -> JsonProfileResult<Option<DismantleGearClass>> {
    match value {
        None => Ok(None),
        Some(Value::String(token)) if token == "weapon" => Ok(Some(DismantleGearClass::Weapon)),
        Some(Value::String(token)) if token == "armor" => Ok(Some(DismantleGearClass::Armor)),
        Some(_) => Err(JsonProfileError::format(
            path,
            "class must be \"weapon\" or \"armor\"",
        )),
    }
}

fn parse_optional_bool(value: Option<&Value>, path: &str) -> JsonProfileResult<Option<bool>> {
    match value {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(JsonProfileError::format(
            path,
            "masterworked must be true or false",
        )),
    }
}

fn format_definition_hash(hash: DefinitionHash) -> String {
    format!("0x{:08X}", hash.get())
}

fn write_rarity_filter(row: &mut Map<String, Value>, rarities: &[DismantleRarity]) {
    match rarities {
        [] => {
            row.remove("rarity");
        }
        [rarity] => {
            row.insert("rarity".into(), Value::String(rarity_token(*rarity).into()));
        }
        rarities => {
            row.insert(
                "rarity".into(),
                Value::Array(
                    rarities
                        .iter()
                        .map(|rarity| Value::String(rarity_token(*rarity).into()))
                        .collect(),
                ),
            );
        }
    }
}

const fn rarity_token(rarity: DismantleRarity) -> &'static str {
    match rarity {
        DismantleRarity::Common => "common",
        DismantleRarity::Uncommon => "uncommon",
        DismantleRarity::Rare => "rare",
        DismantleRarity::Legendary => "legendary",
        DismantleRarity::Exotic => "exotic",
    }
}

const fn gear_class_token(gear_class: DismantleGearClass) -> &'static str {
    match gear_class {
        DismantleGearClass::Weapon => "weapon",
        DismantleGearClass::Armor => "armor",
        DismantleGearClass::Both => "both",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn document(version: u64) -> Value {
        json!({
            "version": version,
            "state": {
                "account": {
                    "primary_soid": "0x9EAA300100100100",
                    "profile_items": []
                },
                "characters": []
            }
        })
    }

    #[test]
    fn loading_and_projecting_without_a_command_is_lossless() {
        let mut document = document(8);
        *document
            .pointer_mut("/state/account/profile_items")
            .unwrap() = json!([{
            "definition_hash": 44,
            "quantity": 2,
            "future": {"keep": true}
        }]);
        let adapter = JsonProfileAdapter::load(&document).unwrap();

        assert_eq!(adapter.project(&document).unwrap(), document);
    }

    #[test]
    fn profile_field_edits_preserve_other_known_representations_and_unknown_members() {
        let mut document = document(8);
        *document
            .pointer_mut("/state/account/profile_items")
            .unwrap() = json!([{
            "definition_hash": 44,
            "quantity": 2,
            "future": {"keep": true}
        }]);
        let adapter = JsonProfileAdapter::load(&document).unwrap();
        let id = adapter.state().profile_items()[0].id;

        let (_, projected) = adapter
            .apply_profile_item(
                &document,
                ProfileItemCommand::SetQuantity { id, quantity: 9 },
            )
            .unwrap();

        assert_eq!(
            projected.pointer("/state/account/profile_items/0"),
            Some(&json!({
                "definition_hash": 44,
                "quantity": 9,
                "future": {"keep": true}
            }))
        );
    }

    #[test]
    fn future_schema_dismantle_rows_remain_opaque() {
        let mut document = document(MAX_SUPPORTED_JSON_SCHEMA + 1);
        document
            .pointer_mut("/state/account")
            .and_then(Value::as_object_mut)
            .unwrap()
            .insert("dismantle_rewards".into(), json!({"future_layout": true}));
        let adapter = JsonProfileAdapter::load(&document).unwrap();
        let new_id = adapter.next_entity_id();

        let (_, projected) = adapter
            .apply_profile_item(
                &document,
                ProfileItemCommand::Add(ProfileItem {
                    id: new_id,
                    definition_hash: DefinitionHash::new(44),
                    quantity: 1,
                }),
            )
            .unwrap();

        assert_eq!(
            projected.pointer("/state/account/dismantle_rewards"),
            document.pointer("/state/account/dismantle_rewards")
        );
    }

    #[test]
    fn missing_account_rejects_projection_without_mutating_inputs() {
        let document = json!({"version": 8, "state": {"characters": []}});
        let adapter = JsonProfileAdapter::load(&document).unwrap();
        let before = adapter.clone();

        assert!(
            adapter
                .apply_profile_item(
                    &document,
                    ProfileItemCommand::Add(ProfileItem {
                        id: adapter.next_entity_id(),
                        definition_hash: DefinitionHash::new(44),
                        quantity: 1,
                    }),
                )
                .is_err()
        );
        assert_eq!(adapter, before);
        assert_eq!(document, json!({"version": 8, "state": {"characters": []}}));
    }
}
