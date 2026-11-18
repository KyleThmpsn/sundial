//! The Shadowkeep package layer: the file format and its block storage, patch chains, the overlays
//! and appended tags a build writes, the packages Parhelion authors and the merge data beside them.
pub(crate) mod appended_tags;
pub(crate) mod block_codec;
pub(crate) mod chain;
pub(crate) mod extend;
pub(crate) mod format;
pub(crate) mod merge_data;
pub(crate) mod profile;
