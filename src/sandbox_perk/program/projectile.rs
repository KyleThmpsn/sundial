//! Projectile dependencies of native action programs, identified by their property contract.
use super::{Action, Program};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dependency {
    WolfpackRounds,
}

impl Dependency {
    pub const fn label(self) -> &'static str {
        match self {
            Self::WolfpackRounds => "Wolfpack Rounds",
        }
    }
}

impl Program {
    /// The rocket channel is an enable property, not the projectile implementation.
    /// These selectors and lifetime values match the supported native emitter gate.
    pub fn projectile_dependency(&self) -> Option<Dependency> {
        self.actions.iter().find_map(|action| match action {
            Action::Property {
                key: 0x456E_31D3,
                target: 2,
                operation_byte: 0,
                removal: 1,
                value_bits,
                restore_bits: 0,
                ability_mask: 0,
                input: 0,
                flag: 1,
            } if *value_bits == 1.0f32.to_bits() => Some(Dependency::WolfpackRounds),
            _ => None,
        })
    }
}
