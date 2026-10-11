//! Compatibility exports for native model draw declarations and validation.
pub(super) use crate::tiger::draws::validate;
pub use crate::tiger::draws::{
    DRAW_INDEX, DRAW_INDEX_CAPACITY, declare_draw_indices, declare_model_draw_indices,
    draw_index_capacity,
};
