//! Source ability intent and controller evidence, separate from native ability rows and banks.
//!
//! Investment grants and perks are separate routes to source entity roots and controllers.
//! Capturing those roots does not resolve their native banks, activation or lifetime.

pub mod animation;
pub mod effects;
pub mod grants;
mod plan;
pub(crate) mod source;
pub use plan::{Controller, ControllerStatus, Plan, Runtime};

use serde::{Deserialize, Serialize};

/// The requested role is researcher-supplied intent, not a decoded source socket-type enum.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Super,
    Melee,
    Grenade,
    ClassAbility,
    Movement,
    Aspect,
    Fragment,
}

impl Role {
    pub(crate) fn active(self) -> bool {
        !matches!(self, Self::Aspect | Self::Fragment)
    }
}

/// Source membership uses a socket ordinal and an investment hash. Neither is a native row.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Selection {
    pub socket_index: u32,
    pub plug_hash: u32,
    pub role: Role,
}

/// Intended element. Source damage providers still need to be resolved before translation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Element {
    Kinetic,
    Arc,
    Solar,
    Void,
    Stasis,
    Strand,
}

impl Element {
    pub(crate) fn native(self) -> Option<Self> {
        match self {
            Self::Strand => Some(Self::Kinetic),
            Self::Stasis => None,
            other => Some(other),
        }
    }
}

/// Missing contracts are work to resolve, never switches that declare a port supported.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Contract {
    SourceSubclassIdentity,
    InnateControllers,
    SourceAbilityGraph,
    SourceSelectionRules,
    PerkAssignment,
    ControllerFormat,
    NativePerkLowering,
    NativeAbilityLowering,
    Effects,
    Animation,
    SourceDamageProviders,
    DamageType,
    NativeSelection,
    PrivateOwnership,
    ActivationAndCleanup,
    NativeEmission,
    Gameplay,
}

pub(crate) use plan::{plan, runtime};
