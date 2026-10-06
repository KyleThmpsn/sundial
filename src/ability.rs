//! The runtime side of abilities: the resources a Subclass node's ability resolves to in
//! the sandbox packages, as opposed to the catalog's view of ability choices in
//! `catalog::items::abilities` and the authored lists in `subclass`.
pub mod bank;
pub mod definition;
pub mod materials;
pub mod modifier;
pub mod movement;
pub mod palette;
pub mod reference;
pub mod spawns;
mod target;
pub mod tint;
pub use target::AbilityTarget;
