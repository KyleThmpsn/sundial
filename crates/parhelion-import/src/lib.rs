//! Cross-game model import backends for Parhelion.
// A Reach map's conversion report is one JSON literal deeper than the default expansion limit.
#![recursion_limit = "256"]
pub mod artwork;
pub mod cache;
pub mod cancellation;
pub mod compute;
pub mod d1_roi;
pub mod d2_mot;
mod graph;
pub mod halo_reach;
pub mod io;
pub mod marathon;
pub mod preview;
mod stat_group;
pub mod tiger;
pub mod tools;
pub mod weapon_type;
pub use graph::CONVERTER_REVISION;
pub use graph::GraphReference;
pub use graph::manifest_converter_revision;
pub use graph::model_symbols;

mod presentation;
