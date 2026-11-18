//! Sunrise investment schema selection and native capacities.

/// The newest account schema this build reads.
pub(super) const SCHEMA_VERSION: i64 = 8;

/// Whether `version` is a schema this build edits. Reset and recovery keep an account within its
/// own supported schema, since migration belongs to Sunrise.
pub(super) fn supported(version: i64) -> bool {
    matches!(version, 2 | 3 | 5 | 7 | 8)
}

/// Sunrise's account schemas. Versions 4 and 6 are only steps of Sunrise's upgrades to 5 and 7.
/// Version 8 is schema 7 with one more table, which Sunrise adds to a schema 7 save in place.
/// Each commits in its own transaction before the step that follows it, so no build creates or
/// keeps a save at either version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Schema {
    V2,
    V3,
    V5,
    V7,
    V8,
}

impl Schema {
    pub(crate) fn from_sql(sql: &str) -> Result<Self, super::SunriseAccountError> {
        let db = rusqlite::Connection::open_in_memory().map_err(|error| {
            super::SunriseAccountError::sqlite("inspect bundled schema for", error)
        })?;
        db.execute_batch(sql).map_err(|error| {
            super::SunriseAccountError::sqlite("inspect bundled schema for", error)
        })?;
        Self::read(&db)
    }
    pub(crate) fn read(db: &rusqlite::Connection) -> Result<Self, super::SunriseAccountError> {
        let version = db
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .map_err(|error| {
                super::SunriseAccountError::sqlite("read schema version from", error)
            })?;
        match version {
            2 => Ok(Self::V2),
            3 => Ok(Self::V3),
            5 => Ok(Self::V5),
            7 => Ok(Self::V7),
            8 => Ok(Self::V8),
            4 | 6 => Err(super::SunriseAccountError::InvalidSchema(format!(
                "user_version {version} is an unfinished Sunrise upgrade. Start Project Sunrise once to finish it"
            ))),
            _ => Err(super::SunriseAccountError::InvalidSchema(format!(
                "expected user_version 2, 3, 5, 7 or 8, found {version}"
            ))),
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::V2 => "SQLite (Schema 2)",
            Self::V3 => "SQLite (Schema 3)",
            Self::V5 => "SQLite (Schema 5)",
            Self::V7 => "SQLite (Schema 7)",
            Self::V8 => "SQLite (Schema 8)",
        }
    }

    pub(super) fn backup_prefix(self) -> &'static str {
        match self {
            Self::V2 => "investment-v2",
            Self::V3 => "investment-v3",
            Self::V5 => "investment-v5",
            Self::V7 => "investment-v7",
            Self::V8 => "investment-v8",
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

    /// Whether items and material stacks name the bucket they occupy in a `placement` column: 0
    /// for the bucket their definition authors, 1 for its recovery bucket. Schema 7 added it and
    /// with it lets a character hold several stacks of one material, told apart by their
    /// mutation serials.
    pub(crate) fn has_placement(self) -> bool {
        matches!(self, Self::V7 | Self::V8)
    }

    /// Whether `item_objectives` holds per-instance values that follow an item's SOID.
    pub(crate) fn has_item_objectives(self) -> bool {
        matches!(self, Self::V7 | Self::V8)
    }

    /// Whether a pending reward stores how it is delivered. Schema 7 dropped the column, and
    /// Sunrise places every reward by its definition.
    pub(crate) fn has_reward_kind(self) -> bool {
        !matches!(self, Self::V7 | Self::V8)
    }

    /// Whether `mission_flags` holds the per-save flags mission scripts keep, keyed by a mod
    /// identifier and a name. Schema 8 added the table and changed nothing else.
    pub(crate) fn has_mission_flags(self) -> bool {
        self == Self::V8
    }

    /// The `user_version` a save of this schema carries.
    pub(super) fn version(self) -> i64 {
        match self {
            Self::V2 => 2,
            Self::V3 => 3,
            Self::V5 => 5,
            Self::V7 => 7,
            Self::V8 => 8,
        }
    }

    /// The schema of a new save made from a bundled schema with this stamp, for an account at
    /// `account`. The bundled schema still stamps 7 in the builds whose saves are schema 8. They
    /// add `mission_flags` in code and stamp the save again, so the bundled stamp serves both
    /// schemas and a reset keeps the account at the one it already has.
    pub(super) fn for_account(self, account: i64) -> Self {
        if self == Self::V7 && account == Self::V8.version() {
            Self::V8
        } else {
            self
        }
    }

    /// Unequipped item rows one character can hold. Schema 7 widened the bound to every character
    /// row its 17 equipment slots leave free.
    pub(crate) fn inventory_capacity(self) -> usize {
        match self {
            Self::V2 | Self::V3 | Self::V5 => {
                crate::account::contract::CHARACTER_INVENTORY_CAPACITY
            }
            Self::V7 | Self::V8 => 333,
        }
    }

    /// Material stack rows one character can hold.
    pub(crate) fn stack_capacity(self) -> usize {
        match self {
            Self::V2 | Self::V3 | Self::V5 => 32,
            Self::V7 | Self::V8 => 333,
        }
    }

    /// Tables Sunrise creates in code rather than from a bundled resource, which a save of this
    /// schema holds.
    pub(crate) fn code_tables(self) -> &'static str {
        match self {
            Self::V2 | Self::V3 => "",
            Self::V5 => PARKED_ITEMS_V5,
            Self::V7 => CODE_TABLES_V7,
            Self::V8 => CODE_TABLES_V8,
        }
    }

    pub(super) fn initialize_reference(self, db: &rusqlite::Connection) -> rusqlite::Result<()> {
        db.execute_batch(match self {
            Self::V2 => SCHEMA,
            Self::V3 => SCHEMA_V3,
            Self::V5 => SCHEMA_V5,
            // Schema 8 bundles the schema 7 resource and adds its table in code.
            Self::V7 | Self::V8 => SCHEMA_V7,
        })?;
        db.execute_batch(self.code_tables())?;
        match self {
            Self::V2 => {}
            Self::V3 => db.execute_batch(STARTER_V3)?,
            // Schemas 7 and 8 left the starter tables as schema 5 declares them.
            Self::V5 | Self::V7 | Self::V8 => db.execute_batch(STARTER_V5)?,
        }
        db.execute_batch(SETTINGS_SCHEMA)
    }
}
pub(super) const CHARACTER_CAPACITY: usize = crate::account::contract::CHARACTER_CAPACITY;
pub(super) const PROFILE_ITEM_CAPACITY: usize = crate::account::contract::PROFILE_ITEM_CAPACITY;
pub(super) const DISMANTLE_REWARD_CAPACITY: usize =
    crate::account::contract::FILTERED_DISMANTLE_REWARD_CAPACITY;
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
pub(super) const SCHEMA_V7: &str = include_str!("fixtures/v7/investment_schema.sql");
pub(super) const CODE_TABLES_V7: &str = include_str!("fixtures/v7/code_tables.sql");
pub(super) const CODE_TABLES_V8: &str = include_str!("fixtures/v8/code_tables.sql");
