//! Queries over decoded native action kinds.
use crate::sandbox_perk::{
    dependencies,
    nodes::{CONDITIONS, EFFECTS, NodeKind},
};
/// The two families of nodes an action is built from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Effects,
    Conditions,
}

impl Family {
    pub const ALL: [Self; 2] = [Self::Effects, Self::Conditions];

    pub fn label(self) -> &'static str {
        match self {
            Self::Effects => "Effects",
            Self::Conditions => "Conditions",
        }
    }

    pub fn nodes(self) -> &'static [NodeKind] {
        match self {
            Self::Effects => &EFFECTS,
            Self::Conditions => &CONDITIONS,
        }
    }

    fn kinds(self, behavior: &dependencies::Behavior) -> &[u8] {
        match self {
            Self::Effects => &behavior.effect_kinds,
            Self::Conditions => &behavior.condition_kinds,
        }
    }
}

/// The stock perks whose decoded action carries a node of one kind, in index order.
pub fn users(perks: &[dependencies::Perk], family: Family, kind: u8) -> Vec<&dependencies::Perk> {
    perks
        .iter()
        .filter(|perk| {
            perk.error.is_none()
                && perk.action.is_some()
                && perk
                    .behavior
                    .as_ref()
                    .is_some_and(|behavior| family.kinds(behavior).contains(&kind))
        })
        .collect()
}
