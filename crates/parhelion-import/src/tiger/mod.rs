//! Shared Tiger package formats and Shadowkeep emission.
//! Source-specific import workflows remain in their own backends.
pub mod animation;
pub mod audio;
pub mod channel;
pub mod draws;
pub mod entity;
pub mod geometry;
pub mod instance;
pub mod markers;
pub mod payload;
pub(crate) mod projectile;
pub mod reader;
pub mod rig;
pub mod shader;
pub mod shadowkeep;
pub(crate) mod skinning;
pub mod texture;
pub mod vertex_input;
