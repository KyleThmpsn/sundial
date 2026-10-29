//! Retained native Radiance components and their explicit source translation contracts.
//!
//! A property bank alone does not establish a complete ability. The controller's connected
//! inputs, lifecycle interfaces, source effects and selected subclass contributions must all
//! be supplied before a prepared root can be published to a subclass recipe.
mod bank;
mod defaults;
mod lifecycle;
pub mod native;
mod owner;
mod sequence;

pub(super) use bank::revival_keys;
pub use bank::{
    Built as PreparedBank, build as prepare_bank, resources as required_resources,
    selected as prepare_selected_bank,
};
pub use lifecycle::initialize;
pub use sequence::{Sequences, Written as WrittenSequenceInputs};

/// Clear and apply the original native controller declaration defaults.
///
/// These are unbound class instances, not an importable entity. The graph producer must assign
/// reciprocal ownership, registered callbacks, connected inputs and translated source values
/// before publishing them. Their absent references must never be treated as completed bindings.
pub fn controller_defaults() -> (Vec<u8>, Vec<u8>) {
    (
        defaults::runtime_controller(),
        defaults::definition_controller(),
    )
}
