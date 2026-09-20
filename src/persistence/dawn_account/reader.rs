//! Reads one Dawn player-state database into storage-neutral account state.
//!
//! Every check here mirrors one Dawn performs while loading. Reporting them as an incompatibility
//! keeps a database Dawn would refuse to boot from reaching the editor as if it were sound.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};
use sundial_account::{
    AccountSettingGroup, AccountSettingKey, AccountSettingValue, AccountSettingsCapabilities,
    AccountSettingsState, Character, CharacterAbilities, CharacterCapabilities, CharacterMetadata,
    CharacterState, DefinitionHash, DismantleReward, EntityId, EquipmentSlot, FiniteF64,
    InstanceSoid, ItemInstance, ItemPlugs, KEY_BINDING_ACTIONS, KeyBindingSlot,
    ProfileCapabilities, ProfileItem, ProfileState,
};

use super::contract;
use super::error::DawnAccountIncompatibility;

type Incompatible<T> = Result<Result<T, DawnAccountIncompatibility>, rusqlite::Error>;

fn row(detail: impl Into<String>) -> DawnAccountIncompatibility {
    DawnAccountIncompatibility::Row {
        detail: detail.into(),
    }
}

/// Sequential identity for loaded entities. Dawn keys rows by position, not by an id column.
struct Ids(u64);

impl Ids {
    const fn new() -> Self {
        Self(0)
    }
    fn next(&mut self) -> EntityId {
        self.0 += 1;
        EntityId::new(std::num::NonZeroU64::new(self.0).expect("counter starts at one"))
    }
}

/// Reads the three metadata rows in the order Dawn reads them.
pub(super) fn metadata(connection: &Connection) -> Incompatible<DawnMetadata> {
    let mut statement = connection.prepare("SELECT key,value FROM metadata ORDER BY key")?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let keys = rows.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>();
    if keys != contract::METADATA_KEYS {
        return Ok(Err(DawnAccountIncompatibility::Metadata {
            detail: format!(
                "expected exactly {:?} and found {keys:?}",
                contract::METADATA_KEYS
            ),
        }));
    }
    let (revision, imported, epoch) = (rows[0].1, rows[1].1, rows[2].1);
    if revision < 0 {
        return Ok(Err(DawnAccountIncompatibility::Metadata {
            detail: format!("account_revision is {revision}"),
        }));
    }
    if imported != 1 {
        return Ok(Err(DawnAccountIncompatibility::Metadata {
            detail: "legacy_import_complete is not 1, so Dawn has not finished its first import"
                .into(),
        }));
    }
    if epoch <= 0 {
        return Ok(Err(DawnAccountIncompatibility::Metadata {
            detail: format!("reward_epoch is {epoch}"),
        }));
    }
    Ok(Ok(DawnMetadata {
        account_revision: revision,
        reward_epoch: epoch,
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DawnMetadata {
    pub account_revision: i64,
    pub reward_epoch: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DawnAllocators {
    pub item: u64,
    pub profile_item: u64,
}

/// Reads the two allocator rows and their floors.
pub(super) fn allocators(connection: &Connection) -> Incompatible<DawnAllocators> {
    let mut statement =
        connection.prepare("SELECT name,next_value FROM allocators ORDER BY name")?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut item = None;
    let mut profile_item = None;
    for (name, text) in &rows {
        let Some(value) = contract::parse_soid(text) else {
            return Ok(Err(DawnAccountIncompatibility::Allocators {
                detail: format!("{name} holds {text:?}, which is not sixteen hexadecimal digits"),
            }));
        };
        match name.as_str() {
            contract::ITEM_ALLOCATOR => item = Some(value),
            contract::PROFILE_ITEM_ALLOCATOR => profile_item = Some(value),
            other => {
                return Ok(Err(DawnAccountIncompatibility::Allocators {
                    detail: format!("{other} is not an allocator Dawn reads"),
                }));
            }
        }
    }
    let (Some(item), Some(profile_item)) = (item, profile_item) else {
        return Ok(Err(DawnAccountIncompatibility::Allocators {
            detail: format!(
                "expected {} and {}",
                contract::ITEM_ALLOCATOR,
                contract::PROFILE_ITEM_ALLOCATOR
            ),
        }));
    };
    if item < contract::FIRST_ITEM_SOID || profile_item < contract::FIRST_PROFILE_ITEM_SOID {
        return Ok(Err(DawnAccountIncompatibility::Allocators {
            detail: "an allocator is below the floor Dawn requires".into(),
        }));
    }
    Ok(Ok(DawnAllocators { item, profile_item }))
}

/// Reads `account.primary_soid`.
pub(super) fn primary_soid(connection: &Connection) -> Incompatible<InstanceSoid> {
    let text: Option<String> = connection
        .query_row("SELECT primary_soid FROM account WHERE id=1", [], |row| {
            row.get(0)
        })
        .optional()?;
    let Some(text) = text else {
        return Ok(Err(row("the account row is missing")));
    };
    match contract::parse_soid(&text).and_then(InstanceSoid::try_from_u64) {
        Some(soid) => Ok(Ok(soid)),
        None => Ok(Err(row(format!("primary_soid {text:?} is unusable")))),
    }
}

/// Reads profile stacks. Dawn requires `position` to run contiguously from zero.
pub(super) fn profile(connection: &Connection) -> Incompatible<ProfileState> {
    let mut statement = connection
        .prepare(
            "SELECT position,definition_hash,quantity,instance_soid FROM profile_items              ORDER BY position",
        )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() > contract::PROFILE_ITEM_CAPACITY {
        return Ok(Err(row(format!(
            "{} profile items exceed the {} Dawn stores",
            rows.len(),
            contract::PROFILE_ITEM_CAPACITY
        ))));
    }
    let mut ids = Ids::new();
    let mut items = Vec::with_capacity(rows.len());
    for (index, (position, hash, quantity, soid)) in rows.iter().enumerate() {
        if *position != index as i64 {
            return Ok(Err(row(format!(
                "profile item positions are not contiguous from zero at {position}"
            ))));
        }
        let Ok(hash) = u32::try_from(*hash) else {
            return Ok(Err(row(format!(
                "profile item {position} hash {hash} is not a u32"
            ))));
        };
        let Ok(quantity) = i32::try_from(*quantity) else {
            return Ok(Err(row(format!(
                "profile item {position} quantity is out of range"
            ))));
        };
        // Dawn's ordinary stacks legitimately use zero. Only socket action sources need a
        // nonzero identity, which Dawn canonicalizes using its installed socket relation.
        let Some(soid) = contract::parse_soid(soid) else {
            return Ok(Err(row(format!(
                "profile item {position} has an invalid identity"
            ))));
        };
        items.push(ProfileItem {
            id: ids.next(),
            definition_hash: DefinitionHash::new(hash),
            quantity,
            instance_soid: InstanceSoid::try_from_u64(soid),
        });
    }
    let mut rewards = Vec::new();
    let mut query = connection.prepare(
        "SELECT position,definition_hash,quantity FROM dismantle_rewards ORDER BY position",
    )?;
    for row_data in query.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, u32>(1)?,
            r.get::<_, i32>(2)?,
        ))
    })? {
        let (position, hash, quantity) = row_data?;
        if position != rewards.len() as i64 {
            return Ok(Err(row(
                "dismantle reward positions are not contiguous from zero",
            )));
        }
        rewards.push(DismantleReward {
            id: ids.next(),
            definition_hash: DefinitionHash::new(hash),
            quantity,
            rarities: Vec::new(),
            gear_class: None,
            masterworked: None,
        });
    }
    let capabilities = ProfileCapabilities {
        profile_items_writable: false,
        profile_item_capacity: Some(contract::PROFILE_ITEM_CAPACITY),
        enforce_loaded_profile_item_capacity: true,
        dismantle_rewards_writable: false,
        dismantle_reward_capacity: Some(contract::DISMANTLE_REWARD_CAPACITY),
        filtered_dismantle_rewards: false,
        combined_dismantle_gear_class: false,
    };
    match ProfileState::try_new(capabilities, items, rewards) {
        Ok(state) => Ok(Ok(state)),
        Err(error) => Ok(Err(row(error.to_string()))),
    }
}

struct LoadedItem {
    character: u64,
    location: i64,
    position: i64,
    instance: ItemInstance,
}

/// Reads characters with their equipment and inventory, applying Dawn's range and order checks.
pub(super) fn characters(connection: &Connection) -> Incompatible<CharacterState> {
    let mut statement = connection.prepare(
        "SELECT position,soid,race,gender,class,level,movement_ability,grenade_ability,\
         super_ability,melee_ability,class_ability FROM characters ORDER BY position",
    )?;
    let rows = statement
        .query_map([], |row| {
            let mut values = [0_i64; 9];
            for (index, value) in values.iter_mut().enumerate() {
                *value = row.get::<_, i64>(index + 2)?;
            }
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, values))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() > contract::CHARACTER_CAPACITY {
        return Ok(Err(row(format!(
            "{} characters exceed the {} Dawn stores",
            rows.len(),
            contract::CHARACTER_CAPACITY
        ))));
    }

    let mut ids = Ids::new();
    let mut characters = Vec::with_capacity(rows.len());
    let mut soids = Vec::with_capacity(rows.len());
    for (index, (position, soid, values)) in rows.iter().enumerate() {
        if *position != index as i64 {
            return Ok(Err(row(format!(
                "character positions are not contiguous from zero at {position}"
            ))));
        }
        for ((name, maximum), value) in contract::CHARACTER_RANGES.iter().zip(values) {
            if *value < 0 || value > maximum {
                return Ok(Err(row(format!(
                    "character {position} has {name} {value}, outside 0 to {maximum}"
                ))));
            }
        }
        let Some(soid) = contract::parse_soid(soid).and_then(InstanceSoid::try_from_u64) else {
            return Ok(Err(row(format!(
                "character {position} soid {soid:?} is unusable"
            ))));
        };
        soids.push(soid.get());
        characters.push(Character {
            id: ids.next(),
            soid: Some(soid),
            metadata: Some(CharacterMetadata {
                race: values[0] as u8,
                gender: values[1] as u8,
                class_type: values[2] as u8,
                abilities: CharacterAbilities {
                    movement: values[4] as u8,
                    grenade: values[5] as u8,
                    super_ability: values[6] as u8,
                    melee: values[7] as u8,
                    class_ability: values[8] as u8,
                },
            }),
            inventory: Vec::new(),
            equipment: BTreeMap::new(),
        });
    }

    let items = match read_items(connection, &mut ids)? {
        Ok(items) => items,
        Err(problem) => return Ok(Err(problem)),
    };
    for item in items {
        let Some(index) = soids.iter().position(|soid| *soid == item.character) else {
            return Ok(Err(row(format!(
                "item at location {} position {} names an unknown character",
                item.location, item.position
            ))));
        };
        let character = &mut characters[index];
        match item.location {
            contract::EQUIPMENT_LOCATION => {
                // Equipment is sparse. A character with nothing in the first slots really does
                // start at a later position, so only the bound is checked.
                let Ok(slot) = usize::try_from(item.position) else {
                    return Ok(Err(row("an equipment position is negative")));
                };
                let Some(name) = contract::EQUIPMENT_SLOTS.get(slot) else {
                    return Ok(Err(row(format!(
                        "equipment slot {slot} is outside Dawn's sixteen"
                    ))));
                };
                if character
                    .equipment
                    .insert(EquipmentSlot::new(*name), Some(item.instance))
                    .is_some()
                {
                    return Ok(Err(row(format!("equipment slot {name} is occupied twice"))));
                }
            }
            contract::INVENTORY_LOCATION => {
                if item.position != character.inventory.len() as i64 {
                    return Ok(Err(row(format!(
                        "inventory positions are not contiguous from zero at {}",
                        item.position
                    ))));
                }
                character.inventory.push(item.instance);
            }
            other => {
                return Ok(Err(row(format!(
                    "location {other} is not equipment or inventory"
                ))));
            }
        }
    }

    let capabilities = CharacterCapabilities {
        metadata_writable: false,
        inventory_writable: false,
        equipment_writable: false,
        equipment_flags_writable: false,
        inventory_capacity: Some(contract::CHARACTER_ITEM_CAPACITY),
        enforce_loaded_inventory_capacity: true,
        max_item_plugs: contract::PLUG_CAPACITY,
        item_flag_mask: crate::account_contract::INVENTORY_FLAG_MASK.into(),
        enforce_unique_instance_soids: true,
    };
    match CharacterState::try_new(capabilities, Vec::new(), characters) {
        Ok(state) => Ok(Ok(state)),
        Err(error) => Ok(Err(row(error.to_string()))),
    }
}

/// Reads `character_items` joined with their `item_sockets` lanes.
fn read_items(connection: &Connection, ids: &mut Ids) -> Incompatible<Vec<LoadedItem>> {
    let mut lanes: BTreeMap<u64, Vec<Option<DefinitionHash>>> = BTreeMap::new();
    let mut statement = connection.prepare(
        "SELECT instance_soid,lane,plug_hash FROM item_sockets ORDER BY instance_soid,lane",
    )?;
    let socket_rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (soid, lane, plug) in socket_rows {
        let Some(soid) = contract::parse_soid(&soid) else {
            return Ok(Err(row(format!(
                "socket instance_soid {soid:?} is unusable"
            ))));
        };
        let entry = lanes.entry(soid).or_default();
        if lane != entry.len() as i64 {
            return Ok(Err(row(format!(
                "socket lanes are not contiguous from zero at lane {lane}"
            ))));
        }
        if entry.len() >= contract::PLUG_CAPACITY {
            return Ok(Err(row(format!(
                "an item has more than {} socket lanes",
                contract::PLUG_CAPACITY
            ))));
        }
        let plug = match plug {
            None => None,
            Some(value) => match u32::try_from(value) {
                Ok(value) => Some(DefinitionHash::new(value)),
                Err(_) => return Ok(Err(row(format!("plug hash {value} is not a u32")))),
            },
        };
        entry.push(plug);
    }

    let mut statement = connection.prepare(
        "SELECT character_soid,location,position,instance_soid,definition_hash,level,quantity,\
         flags,socket_policy FROM character_items ORDER BY character_soid,location,position",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut items = Vec::with_capacity(rows.len());
    for (character, location, position, soid, hash, level, quantity, flags, policy) in rows {
        let (Some(character), Some(soid)) = (
            contract::parse_soid(&character),
            contract::parse_soid(&soid),
        ) else {
            return Ok(Err(row("an item SOID is not sixteen hexadecimal digits")));
        };
        let Some(instance_soid) = InstanceSoid::try_from_u64(soid) else {
            return Ok(Err(row("an item instance_soid is zero")));
        };
        let (Ok(hash), Ok(level), Ok(quantity), Ok(flags)) = (
            u32::try_from(hash),
            i32::try_from(level),
            i32::try_from(quantity),
            u32::try_from(flags),
        ) else {
            return Ok(Err(row(format!(
                "item {soid:016X} has a column outside its range"
            ))));
        };
        let plugs = match policy {
            0 => ItemPlugs::NativeDefaults,
            1 => ItemPlugs::Authored(lanes.get(&soid).cloned().unwrap_or_default()),
            other => return Ok(Err(row(format!("socket_policy {other} is not 0 or 1")))),
        };
        if policy == 0 && lanes.contains_key(&soid) {
            return Ok(Err(row(format!(
                "item {soid:016X} inherits native plugs but still stores socket lanes"
            ))));
        }
        items.push(LoadedItem {
            character,
            location,
            position,
            instance: ItemInstance {
                id: ids.next(),
                instance_soid,
                definition_hash: DefinitionHash::new(hash),
                level,
                quantity,
                plugs,
                flags: Some(flags),
            },
        });
    }
    Ok(Ok(items))
}

/// Reads `settings_values` and `key_bindings` into storage-neutral settings, with an index of
/// where each one came from so a later write goes back to exactly that row and column.
pub(super) fn settings(
    connection: &Connection,
) -> Incompatible<(AccountSettingsState, super::settings::SettingsIndex)> {
    use super::settings::{SettingColumn, SettingEncoding, SettingLocation, SettingsIndex};
    let mut values: BTreeMap<AccountSettingKey, AccountSettingValue> = BTreeMap::new();
    let mut index = SettingsIndex::default();

    let mut statement = connection
        .prepare("SELECT key,integer_value,real_value FROM settings_values ORDER BY key")?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<f64>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let seed_version = match rows.iter().find(|(key, _, _)| key == "pc.seedVersion") {
        None => 0,
        Some((_, Some(version @ 0..=1), None)) => *version,
        Some((_, integer, real)) => {
            return Ok(Err(row(format!(
                "setting pc.seedVersion must be integer 0 or 1, found {integer:?}/{real:?}"
            ))));
        }
    };
    for (key, integer, real) in rows {
        let (model_key, encoding) = match key.as_str() {
            // Dawn keeps this initialization gate durable, but it is not a player preference.
            "pc.seedVersion" => continue,
            // Before the first PC seed completes, Dawn mirrors the old social row. Afterwards the
            // dedicated PC row is authoritative. Expose exactly the value Dawn sends to the game.
            "pc.voiceChatEnabled" if seed_version == 1 => (
                AccountSettingKey::preference(AccountSettingGroup::Social, "voice_chat_enabled"),
                SettingEncoding::Direct,
            ),
            "pc.voiceChatEnabled" => continue,
            "social.voiceChatEnabled" if seed_version == 1 => continue,
            "pc.verticalSyncMode" => (
                AccountSettingKey::preference(
                    AccountSettingGroup::Display,
                    "vertical_sync_interval",
                ),
                SettingEncoding::VerticalSyncMode,
            ),
            "pc.fieldOfViewAdjustment" => (
                AccountSettingKey::preference(AccountSettingGroup::Display, "field_of_view"),
                SettingEncoding::Direct,
            ),
            "pc.useLocalKeyBindings" => (
                AccountSettingKey::preference(AccountSettingGroup::Root, "key_binding_source"),
                SettingEncoding::BindingSource,
            ),
            _ => {
                let Some((group, name)) = key.split_once('.') else {
                    // Dawn stores ungrouped switches such as "configured". They carry no editable
                    // preference, so they stay in the file rather than becoming account settings.
                    continue;
                };
                let Some(group) = setting_group(group) else {
                    continue;
                };
                (
                    AccountSettingKey::preference(group, snake_case(name)),
                    SettingEncoding::Direct,
                )
            }
        };
        let value = match (integer, real) {
            // Dawn carries the native signed scalar without applying the menu's FOV range. Keep
            // an out-of-range negative value opaque instead of inventing a control for it.
            (Some(integer), None) if key == "pc.fieldOfViewAdjustment" && integer < 0 => {
                continue;
            }
            (Some(integer), None) => match u64::try_from(integer) {
                Ok(integer) => AccountSettingValue::Unsigned(integer),
                Err(_) => {
                    return Ok(Err(row(format!(
                        "setting {key} holds {integer}, which is negative"
                    ))));
                }
            },
            (None, Some(real)) => match FiniteF64::new(real) {
                Some(real) => AccountSettingValue::Decimal(real),
                None => {
                    return Ok(Err(row(format!(
                        "setting {key} holds a value that is not finite"
                    ))));
                }
            },
            _ => {
                return Ok(Err(row(format!(
                    "setting {key} must hold exactly one of integer_value or real_value"
                ))));
            }
        };
        // Dawn stores every preference as a number, while the storage-neutral model types each
        // one. Offering the alternatives and keeping the first the account crate accepts avoids
        // mirroring the full table here. A preference it does not model stays in the database.
        let column = if integer.is_some() {
            SettingColumn::Integer
        } else {
            SettingColumn::Real
        };
        let value = match (encoding, value) {
            (SettingEncoding::BindingSource, AccountSettingValue::Unsigned(0)) => {
                Some(AccountSettingValue::text("account"))
            }
            (SettingEncoding::BindingSource, AccountSettingValue::Unsigned(1)) => {
                Some(AccountSettingValue::text("computer"))
            }
            (SettingEncoding::VerticalSyncMode, AccountSettingValue::Unsigned(value))
                if value <= 2 =>
            {
                Some(AccountSettingValue::Unsigned(value))
            }
            (SettingEncoding::VerticalSyncMode, AccountSettingValue::Unsigned(value)) => {
                return Ok(Err(row(format!(
                    "setting {key} holds {value}, which is outside Dawn's 0 to 2 range"
                ))));
            }
            (SettingEncoding::Direct, value) => first_supported(&model_key, value),
            _ => None,
        };
        if let Some(value) = value {
            index.preferences.insert(
                model_key.clone(),
                SettingLocation {
                    key: key.clone(),
                    column,
                    encoding,
                },
            );
            values.insert(model_key, value);
        }
    }

    let mut statement = connection
        .prepare("SELECT action,primary_input,secondary_input FROM key_bindings ORDER BY action")?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (action, primary, secondary) in rows {
        let Some(name) = usize::try_from(action)
            .ok()
            .and_then(|index| KEY_BINDING_ACTIONS.get(index))
        else {
            return Ok(Err(row(format!(
                "key binding action {action} is outside the {} Dawn stores",
                KEY_BINDING_ACTIONS.len()
            ))));
        };
        for (slot, code) in [
            (KeyBindingSlot::Primary, primary),
            (KeyBindingSlot::Secondary, secondary),
        ] {
            // Dawn stores an unbound half as NULL rather than omitting the row.
            let value = match code {
                None => AccountSettingValue::Unassigned,
                Some(code) => match u16::try_from(code) {
                    Ok(code) => AccountSettingValue::InputCode(code),
                    Err(_) => {
                        return Ok(Err(row(format!(
                            "{name} holds input code {code}, which is not a u16"
                        ))));
                    }
                },
            };
            let key = AccountSettingKey::key_binding(*name, slot);
            if let Some(value) = first_supported(&key, value) {
                index.actions.insert((*name).to_owned(), action);
                values.insert(key, value);
            }
        }
    }

    let capabilities = AccountSettingsCapabilities {
        writable: false,
        named_key_bindings_writable: false,
        numeric_key_bindings_writable: false,
        extended_field_of_view: false,
    };
    match AccountSettingsState::try_new(capabilities, values) {
        Ok(state) => Ok(Ok((state, index))),
        Err(error) => Ok(Err(row(error.to_string()))),
    }
}

/// Returns the first typed form of one Dawn number the account crate accepts.
fn first_supported(
    key: &AccountSettingKey,
    value: AccountSettingValue,
) -> Option<AccountSettingValue> {
    let boolean = match value {
        AccountSettingValue::Unsigned(0) => Some(AccountSettingValue::Boolean(false)),
        AccountSettingValue::Unsigned(1) => Some(AccountSettingValue::Boolean(true)),
        _ => None,
    };
    [Some(value), boolean]
        .into_iter()
        .flatten()
        .find(|candidate| {
            AccountSettingsState::try_new(
                AccountSettingsCapabilities {
                    writable: false,
                    named_key_bindings_writable: false,
                    numeric_key_bindings_writable: false,
                    extended_field_of_view: true,
                },
                BTreeMap::from([(key.clone(), candidate.clone())]),
            )
            .is_ok()
        })
}

/// Maps Dawn's dotted settings prefix onto a storage-neutral group.
fn setting_group(group: &str) -> Option<AccountSettingGroup> {
    Some(match group {
        "audio" => AccountSettingGroup::Audio,
        "controls" => AccountSettingGroup::Controls,
        "display" => AccountSettingGroup::Display,
        "interface" => AccountSettingGroup::Interface,
        "social" => AccountSettingGroup::Social,
        _ => return None,
    })
}

/// Dawn names settings in camel case while the storage-neutral keys use snake case.
fn snake_case(name: &str) -> String {
    let mut output = String::with_capacity(name.len() + 4);
    for character in name.chars() {
        if character.is_ascii_uppercase() {
            output.push('_');
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}
