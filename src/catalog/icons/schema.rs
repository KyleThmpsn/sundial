//! Native icon-definition and layer layouts shared by catalog readers and authoring tools.

pub const ICON_DEFINITION_SIZE: usize = 0x80;
pub const ICON_DEFINITION_CLASS: u32 = 0x8080_4A53;

pub const ICON_PRIMARY_LAYER_OFFSET: usize = 0x14;
pub const ICON_BACKGROUND_LAYER_OFFSET: usize = 0x1C;
pub const ICON_WATERMARK_LAYER_OFFSET: usize = 0x20;
pub const ICON_FOREGROUND_LAYER_OFFSET: usize = 0x24;
pub const ICON_LAYER_REFERENCE_OFFSETS: [usize; 6] = [0x14, 0x18, 0x1C, 0x20, 0x24, 0x28];

pub const ICON_LAYER_CLASS: u32 = 0x8080_4A69;
pub const ICON_LAYER_ARRAY_CLASS: u32 = 0x8080_9FBD;
pub const ICON_LAYER_LANE_CLASS: u32 = 0x8080_4A6C;
pub const ICON_LAYER_TEXTURE_CLASS: u32 = 0x8080_4A6F;
pub const MAX_ICON_LAYER_LANES: usize = 32;
pub const MAX_ICON_TEXTURES_PER_LANE: usize = 32;
