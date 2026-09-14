//! Ingredient origin comes from installed equipment and subclass relationships.
use super::*;
use crate::sandbox_perk::ingredients::AbilitySource;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum IngredientSource {
    Weapon,
    Armor,
    Ability,
}
impl IngredientSource {
    pub const ALL: [Self; 3] = [Self::Weapon, Self::Armor, Self::Ability];
    pub fn label(self) -> &'static str {
        match self {
            Self::Weapon => "Weapon",
            Self::Armor => "Armor",
            Self::Ability => "Ability",
        }
    }
}

pub struct IngredientCatalog {
    pub choices: Vec<WeaponSandboxPerkChoice>,
    pub references: PerkSources,
    pub sources: BTreeMap<u16, BTreeSet<IngredientSource>>,
    pub context: BTreeMap<u16, BTreeSet<String>>,
}

impl InvestmentCatalog {
    pub fn perk_ingredients(
        &self,
        choices: &[WeaponSandboxPerkChoice],
        abilities: &[AbilitySource],
    ) -> IngredientCatalog {
        let mut result = IngredientCatalog {
            choices: choices.to_vec(),
            references: self.perk_sources(),
            sources: BTreeMap::new(),
            context: BTreeMap::new(),
        };
        let mut origins = BTreeMap::<u64, BTreeSet<IngredientSource>>::new();
        for item in &self.catalog.items {
            let source = if ItemWeaponInventorySlot::from_bucket_hash(item.bucket_hash).is_some() {
                Some(IngredientSource::Weapon)
            } else if matches!(
                item.bucket_hash,
                3_448_274_439 | 3_551_918_588 | 14_239_492 | 20_886_954 | 1_585_787_867
            ) {
                Some(IngredientSource::Armor)
            } else {
                None
            };
            let Some(source) = source else {
                continue;
            };
            origins.entry(item.hash).or_default().insert(source);
            for hash in item
                .default_plugs
                .iter()
                .flatten()
                .filter_map(|text| parse_hash_hex(text))
                .chain(
                    item.sockets
                        .iter()
                        .flat_map(|socket| self.catalog.socket_options(socket).iter().copied()),
                )
            {
                origins.entry(hash).or_default().insert(source);
            }
        }
        for (hash, sources) in origins {
            if let Some(metadata) = self.catalog.item_package_metadata(hash) {
                for perk in &metadata.sandbox_perks {
                    result
                        .sources
                        .entry(perk.perk_index)
                        .or_default()
                        .extend(&sources);
                }
            }
        }
        for source in abilities {
            result
                .sources
                .entry(source.perk_index)
                .or_default()
                .insert(IngredientSource::Ability);
            let mut named = None;
            for item in &self.catalog.items {
                if item.bucket_hash != 3_284_755_031
                    || self
                        .catalog
                        .item_package_metadata(item.hash)
                        .and_then(|m| m.socket_entry_list_index)
                        != Some(source.list)
                {
                    continue;
                }
                let a = &item.abilities;
                let entry = a
                    .movement
                    .iter()
                    .chain(&a.grenade)
                    .chain(&a.super_ability)
                    .chain(&a.melee)
                    .chain(&a.class_ability)
                    .chain(a.attunements.iter().flat_map(|path| {
                        path.perks
                            .iter()
                            .chain(&path.super_abilities)
                            .chain(std::iter::once(&path.melee))
                    }))
                    .find(|entry| entry.entry == u64::from(source.entry));
                if let Some(entry) = entry {
                    named = Some((item.hash as u32, entry.name.clone(), item.name.clone()));
                    break;
                }
            }
            let (hash, name, subclass) = named.unwrap_or_else(|| {
                (
                    0,
                    format!("Ability {} / {}", source.list, source.entry),
                    "Shared Ability".into(),
                )
            });
            result
                .context
                .entry(source.perk_index)
                .or_default()
                .insert(format!("{subclass}: {name}"));
            let existing = result
                .choices
                .iter_mut()
                .find(|choice| choice.perk_index == source.perk_index);
            if existing.is_none()
                || (hash != 0
                    && existing
                        .as_ref()
                        .is_some_and(|choice| choice.representative_hash == 0))
            {
                let choice = WeaponSandboxPerkChoice {
                    perk_index: source.perk_index,
                    representative_hash: hash,
                    representative_name: name,
                    representative_type_name: format!("Ability · {subclass}"),
                };
                if let Some(existing) = existing {
                    *existing = choice;
                } else {
                    result.choices.push(choice);
                }
            }
        }
        result.choices.sort_by_key(|choice| choice.perk_index);
        result
    }
}
