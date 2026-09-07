use std::path::Path;

use crate::{AuthoringError, AuthoringResult, format::BLOCK_SIZE};

pub(crate) const FULL_RAW_FLAGS: u16 = 0;

#[derive(Clone, Debug)]
pub(crate) struct EncodedPackageBlock {
    pub stored: Vec<u8>,
    pub flags: u16,
    pub gcm_tag: [u8; 16],
}

pub(crate) struct PackageBlockEncoder;

impl PackageBlockEncoder {
    pub(crate) fn open_for_packages(package_directory: &Path) -> AuthoringResult<Self> {
        if !package_directory.is_dir() {
            return Err(AuthoringError::InvalidInput(format!(
                "Package directory does not exist at {}",
                package_directory.display()
            )));
        }
        Ok(Self)
    }

    pub(crate) fn encode(
        &self,
        _package_id: u16,
        plaintext: &[u8],
    ) -> AuthoringResult<EncodedPackageBlock> {
        if plaintext.is_empty() || plaintext.len() > BLOCK_SIZE {
            return Err(AuthoringError::InvalidInput(format!(
                "A package block must contain between 1 and {BLOCK_SIZE} plaintext bytes"
            )));
        }
        Ok(EncodedPackageBlock {
            // A flags-0 block is stored verbatim. `stored_size` may be smaller than the logical
            // 0x40000-byte block span, so the final chunk does not need zero padding.
            stored: plaintext.to_vec(),
            flags: FULL_RAW_FLAGS,
            gcm_tag: [0; 16],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_blocks_store_only_the_plaintext_range() {
        let encoder = PackageBlockEncoder;
        let block = encoder
            .encode(0x058C, b"Every End")
            .expect("raw block should encode");

        assert_eq!(block.stored, b"Every End");
        assert_eq!(block.flags, FULL_RAW_FLAGS);
        assert_eq!(block.gcm_tag, [0; 16]);
    }

    #[test]
    fn full_raw_blocks_keep_their_complete_logical_span() {
        let encoder = PackageBlockEncoder;
        let plaintext = vec![0xA5; BLOCK_SIZE];
        let block = encoder
            .encode(0x058C, &plaintext)
            .expect("a full raw block should encode");

        assert_eq!(block.stored, plaintext);
    }
}
