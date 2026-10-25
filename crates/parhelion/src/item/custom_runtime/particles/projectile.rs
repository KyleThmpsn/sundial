//! Fit a private imported carrier to the selected weapon's launch input.
use super::*;

impl ImportedParticles {
    pub(in crate::item) fn prepare_runtime(
        &mut self,
        manager: &PackageManager,
        symbol: Option<&str>,
        host: &[u8],
        group: u32,
        boost: f32,
    ) -> AuthoringResult<()> {
        self.repair_instances(manager)?;
        let Some(symbol) = symbol else {
            return Ok(());
        };
        self.projectile(symbol)?;
        if self.source_trajectory {
            return Ok(());
        }
        super::super::assets::projectile::prepare(
            &mut self.nodes,
            symbol,
            manager,
            host,
            group,
            boost,
        )
    }
}
