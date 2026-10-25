//! Named native selectors retain their numeric recipe representation and unknown bytes.
use serde::{Deserialize, Serialize};

macro_rules! selector {
    ($(#[$meta:meta])* $name:ident { $first:ident = $default:literal $(, $variant:ident = $value:literal)* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(from = "u8", into = "u8")]
        pub enum $name {
            #[default]
            $first,
            $($variant,)*
            Unknown(u8),
        }

        impl $name {
            pub const fn from_byte(byte: u8) -> Self {
                match byte {
                    $default => Self::$first,
                    $($value => Self::$variant,)*
                    byte => Self::Unknown(byte),
                }
            }

            pub const fn byte(self) -> u8 {
                match self {
                    Self::$first => $default,
                    $(Self::$variant => $value,)*
                    Self::Unknown(byte) => byte,
                }
            }
        }

        impl From<u8> for $name {
            fn from(byte: u8) -> Self { Self::from_byte(byte) }
        }

        impl From<$name> for u8 {
            fn from(value: $name) -> Self { value.byte() }
        }
    };
}

selector! {
    /// Recipient of an entity attachment in the triggering event's context.
    AttachmentTarget { Player = 1, ThisItem = 0, TriggeringWeapon = 2, OtherCombatant = 3 }
}
selector! {
    /// Native damage mode. Its byte order is independent of investment enum order.
    DamageMode { Kinetic = 0, Solar = 1, Arc = 2, Void = 3 }
}
selector! {
    /// Activity gate for an ability energy adjustment.
    AbilityState { Any = 0, Inactive = 1, Active = 2 }
}
selector! {
    /// Current or original ability. Every nonzero native byte selects the original.
    AbilityVersion { Current = 0, Base = 1 }
}
selector! {
    /// Adds or removes a reference to a bank property. Every nonzero byte removes.
    PropertyOperation { Apply = 0, Remove = 1 }
}
selector! {
    /// Inputs established for the common action-value consumer, not Named Property's dialect.
    ValueSource {
        None = 255, Stacks = 0, MissingRounds = 1, MagazineRounds = 2,
        NearbyEnemies = 6, NearbyAllies = 7, OtherFireteamMembers = 8,
        LivingFireteamMembers = 9, DefeatedFireteamMembers = 10, TargetValue = 11,
    }
}
