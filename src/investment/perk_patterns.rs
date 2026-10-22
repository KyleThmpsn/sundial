//! Observed native defaults, kept separate from claims about required patterns.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PerkPatternUse {
    pub pattern_index: u16,
    pub weapon_hash: u32,
    pub weapon_name: String,
    pub perk_index: u16,
    /// None denotes a base-item perk. Some denotes the native default socket plug.
    pub source_plug: Option<u32>,
}

impl InvestmentCatalog {
    /// Reports only base perks and native default plugs, never optional socket pools.
    /// These stock observations do not establish required or exclusive host patterns.
    #[must_use]
    pub fn perk_pattern_uses(&self) -> Vec<PerkPatternUse> {
        let mut result = Vec::new();
        for item in &self.catalog.items {
            let Ok(hash) = u32::try_from(item.hash) else {
                continue;
            };
            let Some(pattern) = self
                .catalog
                .item_package_metadata(item.hash)
                .and_then(|metadata| metadata.weapon_pattern_index)
            else {
                continue;
            };
            let mut sources = BTreeSet::new();
            for perk_index in self.item_sandbox_perk_indices(hash) {
                sources.insert((perk_index, None));
            }
            for plug in item
                .default_plugs
                .iter()
                .filter_map(Option::as_deref)
                .filter_map(parse_hash_hex)
                .filter_map(|hash| u32::try_from(hash).ok())
            {
                for perk_index in self.item_sandbox_perk_indices(plug) {
                    sources.insert((perk_index, Some(plug)));
                }
            }
            result.extend(
                sources
                    .into_iter()
                    .map(|(perk_index, source_plug)| PerkPatternUse {
                        pattern_index: pattern,
                        weapon_hash: hash,
                        weapon_name: item.name.clone(),
                        perk_index,
                        source_plug,
                    }),
            );
        }
        result.sort_by(|left, right| {
            (
                left.perk_index,
                left.pattern_index,
                &left.weapon_name,
                left.weapon_hash,
                left.source_plug,
            )
                .cmp(&(
                    right.perk_index,
                    right.pattern_index,
                    &right.weapon_name,
                    right.weapon_hash,
                    right.source_plug,
                ))
        });
        result
    }
}
