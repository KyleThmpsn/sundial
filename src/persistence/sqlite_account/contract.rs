//! Sunrise investment schema selection and native capacities.

pub(super) const SCHEMA_VERSION: i64 = 5;

/// Sunrise's account schemas. Version 4 is only a step of Sunrise's upgrade to 5, which commits
/// it in its own transaction and then adds the parked items table, so no build creates or keeps a
/// version 4 save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Schema {
    V2,
    V3,
    V5,
}

impl Schema {
    pub(crate) fn from_sql(sql: &str) -> Result<Self, super::SqliteAccountError> {
        let db = rusqlite::Connection::open_in_memory().map_err(|error| {
            super::SqliteAccountError::sqlite("inspect bundled schema for", error)
        })?;
        db.execute_batch(sql).map_err(|error| {
            super::SqliteAccountError::sqlite("inspect bundled schema for", error)
        })?;
        Self::read(&db)
    }
    pub(crate) fn read(db: &rusqlite::Connection) -> Result<Self, super::SqliteAccountError> {
        let version = db
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .map_err(|error| {
                super::SqliteAccountError::sqlite("read schema version from", error)
            })?;
        match version {
            2 => Ok(Self::V2),
            3 => Ok(Self::V3),
            4 => Err(super::SqliteAccountError::InvalidSchema(
                "user_version 4 is an unfinished Sunrise upgrade. Start Project Sunrise once to finish it".into(),
            )),
            5 => Ok(Self::V5),
            _ => Err(super::SqliteAccountError::InvalidSchema(format!(
                "expected user_version 2, 3 or 5, found {version}"
            ))),
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::V2 => "SQLite (Schema 2)",
            Self::V3 => "SQLite (Schema 3)",
            Self::V5 => "SQLite (Schema 5)",
        }
    }

    pub(super) fn backup_prefix(self) -> &'static str {
        match self {
            Self::V2 => "investment-v2",
            Self::V3 => "investment-v3",
            Self::V5 => "investment-v5",
        }
    }

    /// Whether characters store an appearance header and the save holds the character starter
    /// tables, as schema 3 and later do.
    pub(crate) fn has_appearance(self) -> bool {
        self != Self::V2
    }

    /// Whether `characters.appearance_header_valid` marks a captured header. Schema 4 gave every
    /// character a header and dropped the column.
    pub(crate) fn has_appearance_validity(self) -> bool {
        self == Self::V3
    }

    /// Tables Sunrise creates in code rather than from a bundled resource, which a save of this
    /// schema holds.
    pub(crate) fn code_tables(self) -> &'static str {
        match self {
            Self::V2 | Self::V3 => "",
            Self::V5 => PARKED_ITEMS_V5,
        }
    }

    pub(super) fn initialize_reference(self, db: &rusqlite::Connection) -> rusqlite::Result<()> {
        db.execute_batch(match self {
            Self::V2 => SCHEMA,
            Self::V3 => SCHEMA_V3,
            Self::V5 => SCHEMA_V5,
        })?;
        db.execute_batch(self.code_tables())?;
        match self {
            Self::V2 => {}
            Self::V3 => db.execute_batch(STARTER_V3)?,
            Self::V5 => db.execute_batch(STARTER_V5)?,
        }
        db.execute_batch(SETTINGS_SCHEMA)
    }
}
pub(super) const CHARACTER_CAPACITY: usize = crate::account::contract::CHARACTER_CAPACITY;
pub(super) const PROFILE_ITEM_CAPACITY: usize = crate::account::contract::PROFILE_ITEM_CAPACITY;
pub(super) const DISMANTLE_REWARD_CAPACITY: usize =
    crate::account::contract::FILTERED_DISMANTLE_REWARD_CAPACITY;
pub(super) const CHARACTER_ITEM_CAPACITY: usize =
    crate::account::contract::CHARACTER_INVENTORY_CAPACITY;
pub(super) const PLUG_CAPACITY: usize = crate::account::contract::MAX_ITEM_PLUGS;

pub(super) const EQUIPMENT_LOCATION: i64 = 0;
pub(super) const INVENTORY_LOCATION: i64 = 1;

pub(super) const EQUIPMENT_SLOTS: [&str; 17] = {
    let metadata = crate::account::contract::ALL_EQUIPMENT_SLOTS;
    let mut slots = [""; 17];
    let mut index = 0;
    while index < slots.len() {
        slots[index] = metadata[index].0;
        index += 1;
    }
    slots
};

pub(super) use sundial_account::KEY_BINDING_ACTIONS;

pub(super) const APPLICATION_ID: i64 = 1397902921;
pub(super) const SCHEMA: &str = include_str!("fixtures/investment_schema.sql");
pub(super) const SETTINGS_SCHEMA: &str = include_str!("fixtures/account_settings_schema.sql");
pub(super) const SCHEMA_V3: &str = include_str!("fixtures/v3/investment_schema.sql");
pub(super) const STARTER_V3: &str = include_str!("fixtures/v3/character_starter.sql");
pub(super) const SCHEMA_V5: &str = include_str!("fixtures/v5/investment_schema.sql");
pub(super) const STARTER_V5: &str = include_str!("fixtures/v5/character_starter.sql");
pub(super) const PARKED_ITEMS_V5: &str = include_str!("fixtures/v5/parked_items.sql");
