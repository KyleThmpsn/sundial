//! Resolve native owner-companion pairs through the package's enrollment table.
use super::*;
use crate::tag_payload::read_u32;
use std::{fs, path::Path};

/// Parent relation tag to companion identity and decoded payload.
pub(crate) type Companions = BTreeMap<u32, (TagHash, Vec<u8>)>;

pub(crate) fn native_companion<'a>(
    directory: &Path,
    manager: &sundial::package_authoring::PackageManager,
    source_parent: u32,
    companions: &'a mut Companions,
) -> AuthoringResult<&'a (TagHash, Vec<u8>)> {
    if !companions.contains_key(&source_parent) {
        let chain = crate::chain::discover_patch_chain(directory, TagHash(source_parent).pkg_id())?;
        let bytes = fs::read(&chain.latest().path).map_err(|e| invalid(e.to_string()))?;
        let layout = crate::format::PackageLayout::parse(&bytes)?;
        // Enrollment belongs to the package. Cache every row once instead of
        // reopening the same native package for each loading owner.
        for row in layout.shared_tag_enrollment_rows(&bytes)?.chunks_exact(8) {
            companions.entry(read_u32(row, 0)?).or_insert_with(|| {
                (
                    TagHash(u32::from_le_bytes(row[4..8].try_into().unwrap())),
                    Vec::new(),
                )
            });
        }
    }
    let (companion, payload) = companions
        .get_mut(&source_parent)
        .ok_or_else(|| invalid("Native parent has no shared-tag enrollment"))?;
    if payload.is_empty() {
        *payload = manager
            .read_tag(*companion)
            .map_err(|e| invalid(e.to_string()))?;
    }
    companions
        .get(&source_parent)
        .ok_or_else(|| invalid("Native companion cache missing"))
}
