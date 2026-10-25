//! Ability authoring: the runtime side of Subclass abilities a build changes. Weapons and
//! gear live in `weapon` and Subclass lists in `subclass`; this module holds what a project's
//! private perks need from the ability banks in the sandbox packages.
pub(crate) mod banks;
#[cfg(test)]
mod tests;
