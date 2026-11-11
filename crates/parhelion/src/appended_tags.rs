use sundial::package_authoring::is_valid_package_tag;
use tiger_pkg::TagHash;

use crate::package_profile::PRIVATE_PERK_RUNTIME_PACKAGE_ID;
use crate::{AuthoringError, AuthoringResult};

pub(crate) const MAX_PACKAGE_ENTRY_COUNT: usize = 8192;
mod records;
pub(crate) use records::checkpoint;

pub(crate) fn clear() {
    records::clear();
    crate::asset_packages::stream::clear();
}

/// The end of the record indices the build has assigned so far, when it has assigned any.
pub(crate) fn records_end() -> Option<usize> {
    records::exposed_end(false)
}

/// Appended tags by package: the allocation's own package's, then each overflow package's with
/// its id.
pub(crate) type Split<T> = (Vec<T>, Vec<(u16, Vec<T>)>);

/// Assigns destination tags for entries appended to one package table. Record and private runtime
/// allocations continue past its last entry in packages the build reserves, and others refuse
/// those tags.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AppendedTagAllocator {
    package_id: u16,
    current_entry_count: usize,
    /// Logical investment records or private runtime, sharing build-scoped package reservations.
    paged: Option<bool>,
    stream: Option<usize>,
}

impl AppendedTagAllocator {
    pub(crate) const fn new(package_id: u16, current_entry_count: usize) -> Self {
        Self {
            package_id,
            current_entry_count,
            paged: None,
            stream: None,
        }
    }

    #[cfg(feature = "d2-model-importer")]
    pub(crate) fn assets(id: usize) -> Self {
        Self {
            stream: Some(id),
            ..Self::new(0, 0)
        }
    }

    /// Editor validation does not append to the installed host. Use a free, temporary identity
    /// so a full legacy host table cannot reject a valid in-memory runtime edit.
    pub(crate) fn scratch(
        manager: &sundial::package_authoring::PackageManager,
    ) -> AuthoringResult<Self> {
        (crate::package_profile::MIN_AUTHORED_STANDALONE_PACKAGE_ID
            ..=crate::package_profile::MAX_AUTHORED_STANDALONE_PACKAGE_ID)
            .find(|id| !manager.lookup.tag32_entries_by_pkg.contains_key(id))
            .map(|id| Self::new(id, 0))
            .ok_or_else(|| {
                AuthoringError::InvalidInput(
                    "No unused package identity is available for editor validation".into(),
                )
            })
    }

    /// Streaming assets reserve their actual payload before its address becomes observable.
    /// Fixed record allocations keep their existing entry planning.
    #[cfg(feature = "d2-model-importer")]
    pub(crate) fn reserve_payload(self, ordinal: usize, size: usize) -> AuthoringResult<()> {
        if let Some(id) = self.stream {
            crate::asset_packages::stream::reserve(id, ordinal, size)?;
        }
        Ok(())
    }

    /// Where a build's private runtime goes: the private perk-runtime package's table after its
    /// historical high-water mark `start`, then fresh standalone packages. Each takes the
    /// highest free ID, sharing reservations with investment records and imported content.
    pub(crate) fn private_runtime(start: usize) -> Self {
        Self {
            paged: Some(true),
            ..Self::new(PRIVATE_PERK_RUNTIME_PACKAGE_ID, start)
        }
    }

    pub(crate) fn records(start: usize) -> Self {
        Self {
            paged: Some(false),
            ..Self::new(crate::package_profile::host().id, start)
        }
    }

    /// Artwork also targets strict asset packages. Only the build's record host may spill.
    pub(crate) fn destination(package: u16, start: usize) -> Self {
        if crate::package_profile::holding() && package == crate::package_profile::host().id {
            Self::records(start)
        } else {
            Self::new(package, start)
        }
    }

    pub(crate) fn reserve(self, ordinal: usize, count: usize) -> AuthoringResult<()> {
        let index = Self::checked_ordinal(self.current_entry_count, ordinal, "Record group")?;
        if let Some(private) = self.paged {
            records::reserve(private, self.current_entry_count, index, count)
        } else {
            self.assigned_tag(ordinal, "Record group", "first record")?;
            self.assigned_tag(
                Self::checked_ordinal(ordinal, count.saturating_sub(1), "Record group")?,
                "Record group",
                "last record",
            )?;
            Ok(())
        }
    }

    /// How many of its tags its own package holds before any overflow.
    pub(crate) const fn capacity(self) -> usize {
        MAX_PACKAGE_ENTRY_COUNT.saturating_sub(self.current_entry_count)
    }

    /// `tags`, which this allocation assigned in order, by the package each went to: its own
    /// package's, then each overflow package's with its id.
    pub(crate) fn split<T>(self, tags: Vec<T>) -> AuthoringResult<Split<T>> {
        if let Some(private) = self.paged {
            return records::split(private, self.current_entry_count, tags);
        }
        let capacity = self.capacity();
        if tags.len() > capacity {
            return Err(AuthoringError::InvalidInput(format!(
                "{} appended tags exceed the {capacity} entries package {:04X} has room for",
                tags.len(),
                self.package_id
            )));
        }
        Ok((tags, Vec::new()))
    }

    pub(crate) fn ordinal(self, tag: TagHash) -> AuthoringResult<usize> {
        if let Some(id) = self.stream {
            return crate::asset_packages::stream::ordinal(id, tag);
        }
        if let Some(private) = self.paged {
            return records::ordinal(private, self.current_entry_count, tag);
        }
        if tag.pkg_id() != self.package_id
            || (tag.entry_index() as usize) < self.current_entry_count
        {
            return Err(AuthoringError::InvalidInput(format!(
                "{tag} is outside this appended-tag allocation"
            )));
        }
        Ok(tag.entry_index() as usize - self.current_entry_count)
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
        if let Some(id) = self.stream {
            return crate::asset_packages::stream::tag(id, ordinal);
        }
        let index = self
            .current_entry_count
            .checked_add(ordinal)
            .ok_or_else(|| {
                AuthoringError::InvalidInput(format!("{destination_description} index overflowed"))
            })?;
        if let Some(private) = self.paged {
            return records::tag(private, self.current_entry_count, index);
        }
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

        let message = error.to_string();
        assert!(message.contains("Test destination"));
        assert!(message.contains("8192") && message.contains("package-table limit"));
        let error = AppendedTagAllocator::checked_ordinal(usize::MAX, 1, "test tag").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("test tag"));
        assert!(message.contains("ordinal") && message.contains("overflow"));
    }
}
