//! The class choice of an authored armor piece.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Class {
    Titan,
    Hunter,
    Warlock,
    Any,
}

impl Class {
    pub(crate) const ALL: [Self; 4] = [Self::Titan, Self::Hunter, Self::Warlock, Self::Any];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Titan => "Titan",
            Self::Hunter => "Hunter",
            Self::Warlock => "Warlock",
            Self::Any => "Any Class",
        }
    }

    pub(crate) const fn native_class(self) -> Option<u8> {
        match self {
            Self::Titan => Some(0),
            Self::Hunter => Some(1),
            Self::Warlock => Some(2),
            Self::Any => None,
        }
    }
}
