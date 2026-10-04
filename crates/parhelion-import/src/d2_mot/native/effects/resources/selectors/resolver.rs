//! External selector references require checked entry metadata in both dialects.
use super::*;

/// Metadata facts supplied by package inspection or completed private enrollment.
/// The maps remain separate because the same tag can name different entry classes
/// in the source and native package sets.
pub struct ResourceResolver {
    pub hashes: BTreeMap<u64, u32>,
    pub source_classes: BTreeMap<u32, u32>,
    pub native_classes: BTreeMap<u32, u32>,
}

impl ResourceResolver {
    pub(super) fn source(&self, p: &Payload, at: usize) -> Result<u32> {
        let tag = p.u32(at)?;
        let form = p.u32(at + 4)?;
        let hash = p.u64(at + 8)?;
        let tag = if tag == u32::MAX && form == 0 && hash != 0 {
            *self
                .hashes
                .get(&hash)
                .with_context(|| format!("external selector hash {hash:016X} is unresolved"))?
        } else {
            ensure!(
                form <= 2 && hash == 0,
                "external selector reference form differs"
            );
            tag
        };
        ensure!(
            (0x80800001..=0x81FFFFFF).contains(&tag) && tag != 0x811C9DC5,
            "external selector resource is invalid or null"
        );
        ensure!(
            self.source_classes.get(&tag) == Some(&MODERN_CLASS),
            "external selector source {tag:08X} has no validated selector entry class"
        );
        Ok(tag)
    }

    pub(super) fn target(&self, source: u32, resources: &Resources) -> Result<u32> {
        let tag = *resources
            .tags
            .get(&source)
            .with_context(|| format!("external selector {source:08X} has no native binding"))?;
        ensure!(
            (0x80800001..=0x81FFFFFF).contains(&tag) && tag != 0x811C9DC5,
            "external native selector resource is invalid or null"
        );
        ensure!(
            self.native_classes.get(&tag) == Some(&NATIVE_CLASS),
            "external selector target {tag:08X} has no validated native selector entry class"
        );
        Ok(tag)
    }
}
