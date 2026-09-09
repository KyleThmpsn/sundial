//! Reusable armor-stat allocation targeting and presentation.
//!
//! Allocation plugs encode either the top stat group (Mobility, Resilience,
//! Recovery) or the bottom stat group (Discipline, Intellect, Strength). This
//! module keeps target entry, compatible-pool solving, and result feedback
//! independent from the random-item workspace so other item editors can reuse it.

mod model;
mod solver;
#[cfg(test)]
mod tests;
mod ui;

pub(super) use model::{STAT_NAMES, State, TARGET_MAX};
pub(super) use solver::{
    is_allocation_plug, is_allocation_socket, is_intrinsic_plug, selected_totals,
    socket_stat_values,
};
pub(super) use ui::{INLINE_CONTENT_WIDTH, draw, is_available};
