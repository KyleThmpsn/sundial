//! Recipe settings. Defaults retain the selected vehicle's stock behavior.
use crate::HexHash;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Summon {
    #[default]
    Sparrow,
    Pike,
    HeavyPike,
    Interceptor,
    SuperInterceptor,
    Tank,
    Other {
        entity: HexHash,
    },
}

impl Summon {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Sparrow => "Sparrow",
            Self::Pike => "Pike",
            Self::HeavyPike => "Heavy Pike",
            Self::Interceptor => "Interceptor",
            Self::SuperInterceptor => "Super Interceptor",
            Self::Tank => "Tank",
            Self::Other { .. } => "Other Vehicle",
        }
    }

    pub(crate) fn entity(&self) -> Result<Option<u32>, String> {
        Ok(match self {
            Self::Sparrow => None,
            Self::Pike => Some(0x80C0_D9FA),
            Self::HeavyPike => Some(0x80C0_C1EC),
            Self::Interceptor => Some(0x80C0_D7A6),
            Self::SuperInterceptor => Some(0x80BC_9A34),
            Self::Tank => Some(0x80BF_AF2D),
            Self::Other { entity } => Some(
                entity
                    .parse_u32()
                    .map_err(|e| format!("Vehicle entity tag: {e}"))?,
            ),
        })
    }

    pub const fn hover(&self) -> bool {
        !matches!(self, Self::Tank)
    }
    pub const fn armed(&self) -> bool {
        !matches!(self, Self::Sparrow)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Driving {
    pub acceleration_percent: u16,
    pub braking_percent: u16,
    pub boost_percent: u16,
}

impl Default for Driving {
    fn default() -> Self {
        Self {
            acceleration_percent: 100,
            braking_percent: 100,
            boost_percent: 100,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Handling {
    pub side_dodges: bool,
    pub air_control: bool,
    pub roll_tricks: bool,
    pub fast_summon: bool,
}

impl Handling {
    pub fn sparrow_traits(&self) -> bool {
        self.side_dodges || self.air_control || self.roll_tricks
    }
    pub fn has_changes(&self) -> bool {
        self.sparrow_traits() || self.fast_summon
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Durability {
    pub health_percent: u16,
    pub repair_delay_percent: u16,
    pub repair_rate_percent: u16,
}

impl Default for Durability {
    fn default() -> Self {
        Self {
            health_percent: 100,
            repair_delay_percent: 100,
            repair_rate_percent: 100,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Projectile {
    #[default]
    Stock,
    Pike,
    HeavyPike,
    Interceptor,
    SuperInterceptor,
    Tank,
    Other {
        entity: HexHash,
    },
}

impl Projectile {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Stock => "Original Projectiles",
            Self::Pike => "Pike Rounds",
            Self::HeavyPike => "Heavy Pike Rounds",
            Self::Interceptor => "Interceptor Shells",
            Self::SuperInterceptor => "Super Interceptor Shells",
            Self::Tank => "Tank Shells",
            Self::Other { .. } => "Other Projectile",
        }
    }
    pub(super) fn donor(&self) -> Option<Summon> {
        match self {
            Self::Pike => Some(Summon::Pike),
            Self::HeavyPike => Some(Summon::HeavyPike),
            Self::Interceptor => Some(Summon::Interceptor),
            Self::SuperInterceptor => Some(Summon::SuperInterceptor),
            Self::Tank => Some(Summon::Tank),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Weapons {
    pub damage_percent: u16,
    pub firing_rate_percent: u16,
    pub projectile: Projectile,
}

impl Default for Weapons {
    fn default() -> Self {
        Self {
            damage_percent: 100,
            firing_rate_percent: 100,
            projectile: Projectile::Stock,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sparrow {
    pub summon: Summon,
    /// Kept at its original wire location so existing recipes retain their speed.
    pub speed_percent: u16,
    pub driving: Driving,
    pub handling: Handling,
    pub durability: Durability,
    pub weapons: Weapons,
    /// Whether the inventory icon shows the summoned vehicle's HUD silhouette in place of the
    /// item's art, when the vehicle has one and the icon has no image of its own.
    #[serde(skip_serializing_if = "is_true")]
    pub vehicle_icon: bool,
    /// What the inventory and inspect screens draw when it summons another vehicle.
    #[serde(skip_serializing_if = "InventoryModel::is_sparrow")]
    pub inventory_model: InventoryModel,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_true(value: &bool) -> bool {
    *value
}

/// What the inventory and inspect screens draw for a Sparrow that summons another vehicle. They
/// draw the item's gear art, apart from the graph it summons.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryModel {
    /// The Sparrow's own model.
    #[default]
    Sparrow,
    /// The summoned vehicle's model.
    Vehicle,
    /// No model.
    Empty,
}

impl InventoryModel {
    pub const ALL: [Self; 3] = [Self::Sparrow, Self::Vehicle, Self::Empty];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Sparrow => "Sparrow",
            Self::Vehicle => "Vehicle",
            Self::Empty => "None",
        }
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    const fn is_sparrow(&self) -> bool {
        matches!(self, Self::Sparrow)
    }
}

impl Default for Sparrow {
    fn default() -> Self {
        Self {
            summon: Summon::Sparrow,
            speed_percent: 100,
            driving: Driving::default(),
            handling: Handling::default(),
            durability: Durability::default(),
            weapons: Weapons::default(),
            vehicle_icon: true,
            inventory_model: InventoryModel::Sparrow,
        }
    }
}

impl Sparrow {
    pub fn has_changes(&self) -> bool {
        self != &Self::default()
    }
    pub fn motion_changes(&self) -> bool {
        self.speed_percent != 100 || self.driving != Driving::default()
    }
    pub fn runtime_changes(&self) -> bool {
        self.motion_changes()
            || self.durability != Durability::default()
            || self.weapons != Weapons::default()
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("Driving speed", self.speed_percent),
            ("Acceleration", self.driving.acceleration_percent),
            ("Braking", self.driving.braking_percent),
            ("Boost strength", self.driving.boost_percent),
            ("Health", self.durability.health_percent),
            ("Repair rate", self.durability.repair_rate_percent),
            ("Weapon damage", self.weapons.damage_percent),
            ("Firing rate", self.weapons.firing_rate_percent),
        ] {
            if !(1..=1000).contains(&value) {
                return Err(format!("{name} must be between 1% and 1000%."));
            }
        }
        if self.durability.repair_delay_percent > 1000 {
            return Err("Repair delay must be between 0% and 1000%.".into());
        }
        if !self.summon.hover() && self.motion_changes() {
            return Err("Tank motion does not support Driving Speed, acceleration, braking or boost tuning. Use stock values.".into());
        }
        if self.summon == Summon::Sparrow && self.inventory_model != InventoryModel::Sparrow {
            return Err("Inventory Model needs a summoned vehicle other than the Sparrow.".into());
        }
        if self.summon != Summon::Sparrow && self.handling.sparrow_traits() {
            return Err(
                "Side dodges, air control and roll tricks require a Sparrow summon.".into(),
            );
        }
        if !self.summon.armed() && self.weapons != Weapons::default() {
            return Err("Vehicle weapon controls require an armed summon.".into());
        }
        self.summon.entity()?;
        if let Projectile::Other { entity } = &self.weapons.projectile {
            entity
                .parse_u32()
                .map_err(|e| format!("Projectile entity tag: {e}"))?;
        }
        Ok(())
    }
}
