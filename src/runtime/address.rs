//! Distinct identities in a persisted runtime field address. Native bytes remain unchanged.
use serde::{Deserialize, Serialize};

macro_rules! identity {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(u32);

        impl $name {
            /// Retains a native value, including field-specific sentinels. Resolution validates it.
            pub const fn new(value: u32) -> Self { Self(value) }
            pub const fn get(self) -> u32 { self.0 }
        }

        impl From<u32> for $name {
            fn from(value: u32) -> Self { Self::new(value) }
        }

        impl std::fmt::UpperHex for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::UpperHex::fmt(&self.0, formatter)
            }
        }

        impl std::fmt::LowerHex for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::LowerHex::fmt(&self.0, formatter)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Display::fmt(&self.0, formatter)
            }
        }
    };
}

identity! { /// Package address selecting a private perk's entity graph.
GraphTag }
identity! { /// Component binding name hash within an entity graph.
BindingHash }
identity! { /// Native type or declaration handle resolved through the schema registry.
SchemaHandle }
