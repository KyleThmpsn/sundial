//! Native ability property targets, independent of recipe positions and bank handlers.
use serde::{Deserialize, Serialize};

/// The ability selected by a native perk action. Unknown bytes remain exact.
#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(from = "u8", into = "u8")]
pub enum AbilityTarget {
    #[default]
    Grenade,
    Super,
    Melee,
    Jump,
    Movement,
    ClassAbility,
    Unknown(u8),
}

impl AbilityTarget {
    pub const fn from_byte(byte: u8) -> Self {
        match byte {
            0 => Self::Grenade,
            1 => Self::Super,
            2 => Self::Melee,
            3 => Self::Jump,
            4 => Self::Movement,
            7 => Self::ClassAbility,
            byte => Self::Unknown(byte),
        }
    }

    pub const fn byte(self) -> u8 {
        match self {
            Self::Grenade => 0,
            Self::Super => 1,
            Self::Melee => 2,
            Self::Jump => 3,
            Self::Movement => 4,
            Self::ClassAbility => 7,
            Self::Unknown(byte) => byte,
        }
    }

    pub const fn label(self) -> Option<&'static str> {
        match self {
            Self::Grenade => Some("Grenade"),
            Self::Super => Some("Super"),
            Self::Melee => Some("Melee"),
            Self::Jump => Some("Jump"),
            Self::Movement => Some("Movement"),
            Self::ClassAbility => Some("Class Ability"),
            Self::Unknown(_) => None,
        }
    }
}

impl From<u8> for AbilityTarget {
    fn from(byte: u8) -> Self {
        Self::from_byte(byte)
    }
}

impl From<AbilityTarget> for u8 {
    fn from(target: AbilityTarget) -> Self {
        target.byte()
    }
}

impl std::fmt::Display for AbilityTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.byte(), formatter)
    }
}
