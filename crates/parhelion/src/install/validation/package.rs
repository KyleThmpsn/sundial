//! Immutable package evidence, with current stock compatibility checked separately.
use super::*;
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::install) struct Checked {
    pub artifact: ArtifactMetadata,
    pub structure: Structure,
}

/// Immutable structure retained after format and runtime-map payload validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::install) struct Structure {
    package_id: u16,
    patch_id: u16,
    entry_count: usize,
    block_ends: BTreeMap<u16, u64>,
    inherited_table: Arc<[u8]>,
}

impl Structure {
    pub(super) fn from_layout(layout: &PackageLayout, bytes: &[u8]) -> Self {
        let table = &bytes[layout.block_table_offset
            ..layout.block_table_offset + layout.block_count * crate::format::BLOCK_HEADER_SIZE];
        let mut block_ends = BTreeMap::<u16, u64>::new();
        for row in table.chunks_exact(crate::format::BLOCK_HEADER_SIZE) {
            let offset = u32::from_le_bytes(row[..4].try_into().unwrap());
            let size = u32::from_le_bytes(row[4..8].try_into().unwrap());
            let patch = u16::from_le_bytes(row[8..10].try_into().unwrap());
            if size != 0 {
                let end = u64::from(offset) + u64::from(size);
                block_ends
                    .entry(patch)
                    .and_modify(|old| *old = (*old).max(end))
                    .or_insert(end);
            }
        }
        Self {
            package_id: layout.package_id,
            patch_id: layout.patch_id,
            entry_count: layout.entry_count,
            block_ends,
            inherited_table: if canonical_package(layout.package_id).is_some() {
                Arc::from(table)
            } else {
                Arc::from([])
            },
        }
    }

    /// Content identity is checked by the caller. Bounds, entry counts and inherited tables
    /// are still compared to the current installation on every loader validation.
    pub(in crate::install) fn verify_sources(
        &self,
        target: &Path,
        authored: &Path,
    ) -> Result<(), InstallError> {
        for (&patch, &end) in &self.block_ends {
            let path = if patch == self.patch_id {
                authored.to_owned()
            } else {
                let stock = canonical_package(self.package_id).ok_or_else(|| {
                    InstallError::validation(
                        "Standalone package references an unavailable predecessor",
                    )
                })?;
                target.join(stock.stock_file_name(patch))
            };
            reject_symlink(&path, "block source package")?;
            if end
                > fs::metadata(&path)
                    .map_err(|e| InstallError::validation(e.to_string()))?
                    .len()
            {
                return Err(InstallError::validation(format!(
                    "Package {:04X} has a block extending beyond generation {patch}",
                    self.package_id
                )));
            }
        }
        if let Some(stock) = canonical_package(self.package_id) {
            if self.patch_id != stock.stock_patch_id + 1 {
                return Err(InstallError::validation(
                    "A loader patch must immediately follow its stock generation",
                ));
            }
            let path = target.join(stock.stock_file_name(stock.stock_patch_id));
            reject_symlink(&path, "stock block table")?;
            let baseline = fs::read(&path).map_err(|e| InstallError::validation(e.to_string()))?;
            let original = PackageLayout::parse(&baseline)
                .map_err(|e| InstallError::validation(e.to_string()))?;
            if self.entry_count != original.entry_count {
                return Err(InstallError::validation(format!(
                    "{} has {} entries where its stock generation has {}. Sunrise refuses a patch that adds entries, so build again for this installation",
                    authored.display(),
                    self.entry_count,
                    original.entry_count
                )));
            }
            let size = original
                .block_count
                .checked_mul(crate::format::BLOCK_HEADER_SIZE)
                .ok_or_else(|| InstallError::validation("Block table length overflow"))?;
            if self.inherited_table.get(..size)
                != baseline.get(original.block_table_offset..original.block_table_offset + size)
            {
                return Err(InstallError::validation(format!(
                    "{} changes the inherited stock block table",
                    authored.display()
                )));
            }
        } else if self.patch_id != 0 {
            return Err(InstallError::validation(
                "A new managed package must begin at generation zero",
            ));
        }
        Ok(())
    }
}
