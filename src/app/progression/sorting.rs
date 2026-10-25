//! Table ordering with missing values last and expensive keys computed once per row.

pub(super) use super::hierarchy::sort_by_optional_cached_key as by_key;
