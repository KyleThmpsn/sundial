//! Authored action programs, independent of a stock perk's action graph.
//!
//! The compiler owns routing masks, condition ordinals and retained-state counts.
//! Native entities remain reusable building blocks, with edits kept on private clones.
use serde::{Deserialize, Serialize};

use crate::weapon_runtime::WeaponRuntimeValueOverride;

mod compiler;
pub use compiler::{Compiled, compile};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Equipped,
    #[default]
    Drawn,
    WeaponKill,
    PrecisionKill,
    MeleeKill,
    GrenadeKill,
    AnyKill,
}

impl Trigger {
    pub const ALL: [Self; 7] = [
        Self::Equipped,
        Self::Drawn,
        Self::WeaponKill,
        Self::PrecisionKill,
        Self::MeleeKill,
        Self::GrenadeKill,
        Self::AnyKill,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Equipped => "While Equipped",
            Self::Drawn => "While Drawn",
            Self::WeaponKill => "On Weapon Kill",
            Self::PrecisionKill => "On Precision Weapon Kill",
            Self::MeleeKill => "On Melee Kill",
            Self::GrenadeKill => "On Grenade Kill",
            Self::AnyKill => "On Any Credited Kill",
        }
    }

    #[must_use]
    pub const fn is_event(self) -> bool {
        !matches!(self, Self::Equipped | Self::Drawn)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    #[default]
    Owner,
    Event,
}

impl Position {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Owner => "Owner Position",
            Self::Event => "Event Position",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub graph: u32,
    /// Original native spelling, used for display and the compiled debug reference.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<WeaponRuntimeValueOverride>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Spawn {
        asset: Asset,
        #[serde(default)]
        position: Position,
    },
    Attach {
        asset: Asset,
    },
    Pattern {
        asset: Asset,
    },
}

impl Action {
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Spawn { .. } => "Spawn Entity",
            Self::Attach { .. } => "Attach Entity",
            Self::Pattern { .. } => "Override Weapon Pattern",
        }
    }

    #[must_use]
    pub const fn asset(&self) -> &Asset {
        match self {
            Self::Spawn { asset, .. } | Self::Attach { asset } | Self::Pattern { asset } => asset,
        }
    }

    pub const fn asset_mut(&mut self) -> &mut Asset {
        match self {
            Self::Spawn { asset, .. } | Self::Attach { asset } | Self::Pattern { asset } => asset,
        }
    }

    #[must_use]
    pub const fn retained(&self) -> bool {
        !matches!(self, Self::Spawn { .. })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    pub name: String,
    pub trigger: Trigger,
    pub duration_ms: u32,
    pub cooldown_ms: u32,
    /// Probability in hundredths of a percent. Integers keep recipe equality stable.
    pub chance_permyriad: u16,
    pub actions: Vec<Action>,
}

impl Default for Program {
    fn default() -> Self {
        Self {
            name: "Custom Effect".into(),
            trigger: Trigger::Drawn,
            duration_ms: 1000,
            cooldown_ms: 0,
            chance_permyriad: 10_000,
            actions: Vec::new(),
        }
    }
}

impl Program {
    /// Drafts can be empty. Build readiness is checked separately.
    pub fn validate_structure(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.contains('\0') {
            return Err("Enter a name for the custom effect.".into());
        }
        if self.actions.len() > 16 {
            return Err("A custom effect can contain up to 16 actions.".into());
        }
        if self.chance_permyriad > 10_000
            || self.duration_ms > 3_600_000
            || self.cooldown_ms > 3_600_000
        {
            return Err("Use a chance from 0 to 100 percent and times up to one hour.".into());
        }
        if self
            .actions
            .iter()
            .filter(|action| matches!(action, Action::Pattern { .. }))
            .count()
            > 1
        {
            return Err("An effect can select one weapon pattern at a time.".into());
        }
        for action in &self.actions {
            let asset = action.asset();
            if asset.path.contains('\0') || asset.path.len() > 1024 {
                return Err("The selected asset has an invalid native path.".into());
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_structure()?;
        if self.actions.is_empty() {
            return Err("Add an action to the custom effect.".into());
        }
        if self.trigger.is_event() && self.duration_ms == 0 {
            return Err(
                "An event effect needs a duration greater than zero before it can be ready again."
                    .into(),
            );
        }
        for action in &self.actions {
            if action.asset().graph == 0 || action.asset().graph == u32::MAX {
                return Err("Choose an asset for every action.".into());
            }
            if matches!(
                action,
                Action::Spawn {
                    position: Position::Event,
                    ..
                }
            ) && !self.trigger.is_event()
            {
                return Err("Event Position requires a kill trigger.".into());
            }
        }
        Ok(())
    }
}
