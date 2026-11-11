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
        if self.source_trajectory || self.retained_trajectory {
            return Ok(());
        }
        // A perk projectile leaves the same Barrel, so it is fitted to the same launch input.
        let roots = std::iter::once(symbol.to_owned())
            .chain(self.perk_projectiles.iter().cloned())
            .collect::<Vec<_>>();
        for root in roots {
            super::super::assets::projectile::prepare(
                &mut self.nodes,
                &root,
                manager,
                host,
                group,
                boost,
            )?;
        }
        Ok(())
    }
}
