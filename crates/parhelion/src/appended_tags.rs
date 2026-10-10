use sundial::package_authoring::is_valid_package_tag;
use tiger_pkg::TagHash;

use crate::package_profile::{
    MAX_AUTHORED_STANDALONE_PACKAGE_ID, PRIVATE_PERK_RUNTIME_PACKAGE_ID,
    is_authored_standalone_package_id,
};
use crate::{AuthoringError, AuthoringResult};

pub(crate) const MAX_PACKAGE_ENTRY_COUNT: usize = 8192;

/// Appended tags by package: the allocation's own package's, then each overflow package's with
/// its id.
pub(crate) type Split<T> = (Vec<T>, Vec<(u16, Vec<T>)>);

/// Assigns destination tags for entries appended to one package table, and past its last entry
/// to fresh standalone packages when the allocation may overflow.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AppendedTagAllocator {
    package_id: u16,
    current_entry_count: usize,
    /// The standalone package that takes the tags past this package's last entry, then each one
    /// below it, every one from its first entry. Without one, those tags are refused.
    overflow: Option<u16>,
}

impl AppendedTagAllocator {
    pub(crate) const fn new(package_id: u16, current_entry_count: usize) -> Self {
        Self {
            package_id,
            current_entry_count,
            overflow: None,
        }
    }

    /// Where a build's private runtime goes: the private perk-runtime package's table after its
    /// historical high-water mark `start`, then fresh standalone packages from the top of the
    /// authored range down. The asset packages count up from its bottom, so the two meet only
    /// once the range is spent.
    pub(crate) const fn private_runtime(start: usize) -> Self {
        Self::new(PRIVATE_PERK_RUNTIME_PACKAGE_ID, start)
            .overflowing_into(MAX_AUTHORED_STANDALONE_PACKAGE_ID)
    }

    /// This allocation, continuing past its package's last entry in standalone package `top`, then
    /// each one below it.
    pub(crate) const fn overflowing_into(self, top: u16) -> Self {
        Self {
            overflow: Some(top),
            ..self
        }
    }

    /// How many of its tags its own package holds before any overflow.
    pub(crate) const fn capacity(self) -> usize {
        MAX_PACKAGE_ENTRY_COUNT.saturating_sub(self.current_entry_count)
    }

    /// `tags`, which this allocation assigned in order, by the package each went to: its own
    /// package's, then each overflow package's with its id.
    pub(crate) fn split<T>(self, mut tags: Vec<T>) -> AuthoringResult<Split<T>> {
        let mut spilled = tags.split_off(self.capacity().min(tags.len())).into_iter();
        let mut overflow = Vec::new();
        loop {
            let chunk = spilled
                .by_ref()
                .take(MAX_PACKAGE_ENTRY_COUNT)
                .collect::<Vec<_>>();
            if chunk.is_empty() {
                break;
            }
            let first = self.capacity() + overflow.len() * MAX_PACKAGE_ENTRY_COUNT;
            let package = self
                .assigned_tag(first, "Overflow package", "overflowing tag")?
                .pkg_id();
            overflow.push((package, chunk));
        }
        Ok((tags, overflow))
    }

    #[cfg(feature = "d2-model-importer")]
    pub(crate) fn ordinal(self, tag: TagHash) -> AuthoringResult<usize> {
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
        let index = self
            .current_entry_count
            .checked_add(ordinal)
            .ok_or_else(|| {
                AuthoringError::InvalidInput(format!("{destination_description} index overflowed"))
            })?;
        if index >= MAX_PACKAGE_ENTRY_COUNT {
            let Some(top) = self.overflow else {
                return Err(AuthoringError::InvalidInput(format!(
                    "{destination_description} index {index} exceeds the package-table limit"
                )));
            };
            // Each overflow package holds a whole table, from its first entry.
            let spill = index - MAX_PACKAGE_ENTRY_COUNT;
            let package = u16::try_from(spill / MAX_PACKAGE_ENTRY_COUNT)
                .ok()
                .and_then(|below| top.checked_sub(below))
                .filter(|&package| is_authored_standalone_package_id(package))
                .ok_or_else(|| {
                    AuthoringError::InvalidInput(format!(
                        "{destination_description} index {index} exceeds every authored package"
                    ))
                })?;
            return Self::new(package, 0).assigned_tag(
                spill % MAX_PACKAGE_ENTRY_COUNT,
                destination_description,
                tag_description,
            );
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

    #[test]
    fn an_overflowing_allocation_continues_down_the_standalone_range() {
        let allocator =
            AppendedTagAllocator::new(0x01BB, MAX_PACKAGE_ENTRY_COUNT - 2).overflowing_into(0x0CFF);
        let tag = |ordinal| {
            allocator
                .assigned_tag(ordinal, "Test destination", "test tag")
                .unwrap()
        };
        // Its own table first, then each overflow package from its first entry.
        assert_eq!(tag(1), TagHash::new(0x01BB, 0x1FFF));
        assert_eq!(tag(2), TagHash::new(0x0CFF, 0));
        assert_eq!(
            tag(1 + MAX_PACKAGE_ENTRY_COUNT),
            TagHash::new(0x0CFF, 0x1FFF)
        );
        assert_eq!(tag(2 + MAX_PACKAGE_ENTRY_COUNT), TagHash::new(0x0CFE, 0));
        // The tags split by the package each was assigned to.
        let count = 2 + MAX_PACKAGE_ENTRY_COUNT + 3;
        let (own, overflow) = allocator.split((0..count).collect::<Vec<_>>()).unwrap();
        assert_eq!(own, [0, 1]);
        assert_eq!(
            overflow
                .iter()
                .map(|(id, tags)| (*id, tags.len(), tags[0]))
                .collect::<Vec<_>>(),
            [
                (0x0CFF, MAX_PACKAGE_ENTRY_COUNT, 2),
                (0x0CFE, 3, 2 + MAX_PACKAGE_ENTRY_COUNT)
            ]
        );
        // The range ends rather than running into stock package ids.
        let low =
            AppendedTagAllocator::new(0x01BB, MAX_PACKAGE_ENTRY_COUNT).overflowing_into(0x0AA0);
        assert!(
            low.assigned_tag(MAX_PACKAGE_ENTRY_COUNT, "Test destination", "test tag")
                .is_err()
        );
    }
}
