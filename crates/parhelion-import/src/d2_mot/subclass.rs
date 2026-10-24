//! Backend-only modern subclass research: capture, validate selection, plan, then publish evidence.
//!
//! This route is independent of weapon discovery and automatic donor matching. It emits neither
//! recipes nor asset graphs. Roles and the element are explicit intent until their source
//! consumers are resolved. All native selection, ability and private ownership contracts remain
//! visible in the plan, including when a source perk controller can already be decoded.

mod artifact;
mod plan;
pub mod source;

pub use artifact::{inspect, plan_file};
pub use plan::{Damage, DamagePolicy, Input, Plan, Request, plan};

pub const TARGET_BUILD: &str = "86657.20.08.23.1800.d2_rc___release";
