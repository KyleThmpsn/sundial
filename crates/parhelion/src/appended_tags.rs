use sundial::package_authoring::is_valid_package_tag;
use tiger_pkg::TagHash;

use crate::{AuthoringError, AuthoringResult};

pub(crate) const MAX_PACKAGE_ENTRY_COUNT: usize = 8192;

/// Assigns destination tags for entries appended to one package table.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AppendedTagAllocator {
    package_id: u16,
    current_entry_count: usize,
}

impl AppendedTagAllocator {
    pub(crate) const fn new(package_id: u16, current_entry_count: usize) -> Self {
        Self {
            package_id,
            current_entry_count,
        }
    }

    pub(crate) fn checked_ordinal(
        base: usize,
        local: usize,
        description: &str,
    ) -> AuthoringResult<usize> {
        base.checked_add(local).ok_or_else(|| {
            AuthoringError::InvalidInput(format!("{description} appended-tag ordinal overflowed"))
        })
    }

    pub(crate) fn assigned_tag(
        self,
        ordinal: usize,
        destination_description: &str,
        tag_description: &str,
    ) -> AuthoringResult<TagHash> {
        let index = self
            .current_entry_count
            .checked_add(ordinal)
            .ok_or_else(|| {
                AuthoringError::InvalidInput(format!("{destination_description} index overflowed"))
            })?;
        if index >= MAX_PACKAGE_ENTRY_COUNT {
            return Err(AuthoringError::InvalidInput(format!(
                "{destination_description} index {index} exceeds the package-table limit"
            )));
        }

        let encoded_index = u16::try_from(index).map_err(|_| {
            AuthoringError::InvalidInput(format!(
                "{destination_description} index {index} does not fit 16 bits"
            ))
        })?;
        let tag = TagHash::new(self.package_id, encoded_index);
        if !is_valid_package_tag(tag)
            || tag.pkg_id() != self.package_id
            || tag.entry_index() as usize != index
        {
            return Err(AuthoringError::InvalidInput(format!(
                "Destination package id {:04X} cannot encode {tag_description} index {index}",
                self.package_id
            )));
        }
        Ok(tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_preserves_package_and_entry_indices_through_the_runtime_limit() {
        let ordinal = AppendedTagAllocator::checked_ordinal(20, 3, "test tag").unwrap();
        for (package, base, ordinal, expected) in [
            (0x0914, 100, ordinal, 123),
            (0x0CFF, MAX_PACKAGE_ENTRY_COUNT - 1, 0, 0x1FFF),
        ] {
            let tag = AppendedTagAllocator::new(package, base)
                .assigned_tag(ordinal, "Test destination", "test tag")
                .unwrap();
            assert_eq!(tag.pkg_id(), package);
            assert_eq!(tag.entry_index(), expected);
        }
    }

    #[test]
    fn allocator_enforces_the_package_table_limit() {
        let error = AppendedTagAllocator::new(0x0914, MAX_PACKAGE_ENTRY_COUNT - 1)
            .assigned_tag(1, "Test destination", "test tag")
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Test destination index 8192 exceeds the package-table limit"
        );
        let error = AppendedTagAllocator::checked_ordinal(usize::MAX, 1, "test tag").unwrap_err();
        assert_eq!(
            error.to_string(),
            "test tag appended-tag ordinal overflowed"
        );
    }
}
