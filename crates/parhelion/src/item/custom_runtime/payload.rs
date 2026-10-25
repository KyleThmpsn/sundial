//! Read the final runtime through the pending allocation before looking in stock packages.
use super::*;

pub(super) fn private_index(
    tag: u32,
    allocator: AppendedTagAllocator,
    tags: &[NewTagSpec],
) -> AuthoringResult<Option<usize>> {
    for index in 0..tags.len() {
        if allocator
            .assigned_tag(index, "Final runtime", "runtime resource")?
            .0
            == tag
        {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

pub(super) fn read(
    manager: &PackageManager,
    tag: u32,
    allocator: AppendedTagAllocator,
    tags: &[NewTagSpec],
) -> AuthoringResult<Vec<u8>> {
    if let Some(index) = private_index(tag, allocator, tags)? {
        Ok(tags[index].payload.clone())
    } else {
        read_tag(manager, TagHash(tag), "final runtime resource")
    }
}
