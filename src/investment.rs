//! Installed-item catalog and picker controls shared by Sundial and Parhelion.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(crate) mod plug_selection;
pub use plug_selection::PlugSelectionMode;

mod controls;
mod definitions;
mod lore;
pub(crate) mod titles;
pub use lore::{LoreEntry, load_item_lore};
mod perk_patterns;
mod perk_sources;
pub use perk_sources::{PerkSource, PerkSources};
pub mod discovery;
mod ingredients;
pub use ingredients::{IngredientCatalog, IngredientSource};

/// The Shaders inventory bucket, and the plug category every shader carries.
pub const SHADER_BUCKET_HASH: u64 = 2_973_005_342;

/// Every stock shader fills its custom and default dye arrays with the same fifteen channel rows
/// and leaves its locked array empty. Other plugs in the category, such as Shared Experience,
/// do not, and are not shaders a recipe can remix.
fn is_stock_shader(metadata: &crate::catalog::ItemPackageMetadata) -> bool {
    let [custom, default, locked] = &metadata.translation_dye_rows;
    metadata.plug_category_hash == Some(SHADER_BUCKET_HASH)
        && custom.len() == 15
        && locked.is_empty()
        && custom
            .iter()
            .map(|row| (row.key, row.value))
            .eq(default.iter().map(|row| (row.key, row.value)))
}
pub(crate) mod seasonal;
pub use controls::{
    AUTHORING_SOCKET_RESET_WIDTH, CatalogLoadingView, DisplayTooltip, IconOverride,
    PlugChoicePickerButton, PlugChoicePickerOptions, PlugSelection, PlugTooltip,
    WeaponDonorPickerAction, WeaponDonorPickerClearChoice, WeaponDonorPickerOptions,
    authoring_button_width, authoring_choice_row_height, authoring_socket_label_width,
    authoring_socket_reset_width, configure_authoring_fonts, default_plug_selection_mode,
    draw_asset_choice_row, draw_asset_choice_row_plain, draw_authoring_info_icon,
    draw_authoring_socket_label, draw_authoring_socket_reset, draw_authoring_toolbar,
    draw_authoring_warning_icon, draw_catalog_loading_view, draw_display_tooltip,
    draw_plug_safety_selector, draw_plug_safety_warning, progress_bar, show_plug_safety_warnings,
    tooltip_title,
};
pub use definitions::{
    PowerCapChoice, SubclassSummary, WeaponAmmoType, WeaponArtArrangement,
    WeaponDamageCarrierFamily, WeaponDamageProfile, WeaponDamageType, WeaponDonor,
    WeaponDonorSummary, WeaponDyeReference, WeaponInventorySlot, WeaponInvestmentStat,
    WeaponOrnament, WeaponOrnamentAppearance, WeaponRarity, WeaponSandboxPerkChoice, WeaponSocket,
    WeaponSocketTypeChoice, WeaponStatDisplayPoint, WeaponSupportedPlugSet, WeaponTraitChoice,
};
pub use perk_patterns::PerkPatternUse;

use crate::{
    catalog::{Catalog, ItemWeaponInventorySlot, is_authorable_weapon_item, is_weapon_bucket},
    hash::parse_hash_hex,
    paths,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogLoadProgress {
    pub message: &'static str,
    pub completed: usize,
    pub total: usize,
}

fn representative_perk_label_quality(
    choice: &WeaponSandboxPerkChoice,
    is_plug: bool,
) -> (bool, bool, bool, usize, u32) {
    (
        choice.representative_name.starts_with("Item 0x"),
        !is_plug,
        choice.representative_type_name.trim().is_empty(),
        choice.representative_name.len(),
        choice.representative_hash,
    )
}

/// The same installed package catalog used by Sundial, exposed through an authoring-safe facade.
pub struct InvestmentCatalog {
    catalog: Catalog,
    authorable_weapon_stat_indices: Vec<u16>,
}

impl InvestmentCatalog {
    /// Items whose Collections entries read progression flags unrelated to their
    /// own acquired flag. Such items are weaker bases for imported weapons.
    #[must_use]
    pub fn items_with_unrelated_collection_conditions(&self) -> BTreeSet<u32> {
        self.catalog
            .collectibles()
            .iter()
            .filter_map(|collectible| {
                let acquired: BTreeSet<u32> = collectible
                    .conditions
                    .iter()
                    .filter(|condition| {
                        condition.field == crate::catalog::COLLECTIBLE_ACQUIRED_CONDITION_FIELD
                    })
                    .flat_map(|condition| &condition.tokens)
                    .filter(|token| token.kind == 1)
                    .map(|token| token.operand)
                    .collect();
                let unrelated = collectible.conditions.iter().any(|condition| {
                    condition.field != crate::catalog::COLLECTIBLE_ACQUIRED_CONDITION_FIELD
                        && condition
                            .tokens
                            .iter()
                            .any(|token| token.kind == 1 && !acquired.contains(&token.operand))
                });
                unrelated
                    .then(|| u32::try_from(collectible.item_hash).ok())
                    .flatten()
            })
            .collect()
    }

    /// Installed presentation-node identities in native table order.
    #[must_use]
    pub fn presentation_node_hashes(&self) -> &[u64] {
        self.catalog.presentation_node_hashes()
    }

    /// The presentation nodes that list an item's collectibles as children, in package order.
    #[must_use]
    pub fn item_collection_parents(&self, item_hash: u32) -> Vec<u64> {
        self.catalog
            .collectibles()
            .iter()
            .filter(|collectible| collectible.item_hash == u64::from(item_hash))
            .flat_map(|collectible| collectible.parent_nodes.iter().copied())
            .collect()
    }

    /// Where an item appears in Collections, one node-name path per placement.
    #[must_use]
    pub fn item_collection_paths(&self, item_hash: u32) -> Vec<Vec<String>> {
        self.catalog
            .collectibles()
            .iter()
            .filter(|collectible| collectible.item_hash == u64::from(item_hash))
            .flat_map(|collectible| collectible.paths.iter().cloned())
            .collect()
    }

    /// Loads (or scans) the installed Shadowkeep catalog using Sundial's shared cache.
    pub fn load(
        install_directory: &Path,
        force_rebuild: bool,
        report: impl FnMut(CatalogLoadProgress),
    ) -> Result<Self, String> {
        let cache_path = paths::shadowkeep_catalog_path()
            .ok_or_else(|| "Could not locate Sundial's local catalog folder".to_owned())?;
        Self::load_with_cache_path(install_directory, &cache_path, force_rebuild, report)
    }

    /// Scans an isolated package view without replacing the active installation's shared cache.
    pub fn load_with_cache_path(
        install_directory: &Path,
        cache_path: &Path,
        force_rebuild: bool,
        mut report: impl FnMut(CatalogLoadProgress),
    ) -> Result<Self, String> {
        let catalog = Catalog::load_or_scan_with_progress(
            install_directory,
            cache_path.to_path_buf(),
            force_rebuild,
            |progress| {
                report(CatalogLoadProgress {
                    message: progress.message,
                    completed: progress.completed,
                    total: progress.total,
                });
            },
        )?;
        let authorable_weapon_stat_indices = (0..catalog.item_stat_definition_count())
            .filter_map(|index| u16::try_from(index).ok())
            .filter(|definition_index| u8::try_from(*definition_index).is_ok())
            .collect();
        Ok(Self {
            catalog,
            authorable_weapon_stat_indices,
        })
    }

    /// Describes the authored private plug, including its effective native classification.
    /// Stock source metadata is only a fallback for fields the recipe does not override.
    #[must_use]
    pub fn private_plug_tooltip(
        &self,
        source: u32,
        classification: Option<u32>,
        name: Option<&str>,
        description: Option<&str>,
    ) -> String {
        let name = name
            .or_else(|| self.catalog.display_name(u64::from(source)))
            .unwrap_or("Private perk");
        let type_name = self
            .catalog
            .plug_type_name(u64::from(classification.unwrap_or(source)))
            .unwrap_or("");
        let description = description
            .or_else(|| self.catalog.description(u64::from(source)))
            .unwrap_or("");
        format!("{name}\n{type_name}\n\n{description}")
    }

    /// Returns the localized stock label, optionally including its hash for disambiguation.
    #[must_use]
    pub fn plug_label(&self, hash: u32, include_hash: bool) -> String {
        self.catalog.plug_label(u64::from(hash), include_hash)
    }

    /// The localized item type of a weapon or plug, such as "Hand Cannon", when the
    /// installation names one.
    #[must_use]
    pub fn item_type_name(&self, hash: u32) -> Option<String> {
        let hash = u64::from(hash);
        self.catalog
            .plug_type_name(hash)
            .or_else(|| self.catalog.package_item_type_name(hash))
            .map(str::to_owned)
    }

    /// Returns the localized display name of any installed item, including plugs.
    #[must_use]
    pub fn item_display_name(&self, hash: u32) -> Option<&str> {
        self.catalog.display_name(u64::from(hash))
    }

    /// Native definition identity for authoring clients that need to distinguish generated plugs.
    #[must_use]
    pub fn item_definition_tag(&self, hash: u32) -> Option<u32> {
        self.catalog
            .item_package_metadata(u64::from(hash))
            .map(|metadata| metadata.definition_tag)
    }

    /// Returns every native cap-table row from the loaded installation, including
    /// duplicate values and large limits. Legacy "version group" fields hold table indices.
    #[must_use]
    pub fn power_cap_choices(&self) -> Vec<PowerCapChoice> {
        self.catalog
            .power_cap_definitions()
            .iter()
            .enumerate()
            .map(|(index, definition)| {
                let index = u16::try_from(index).expect("validated native cap-table index");
                PowerCapChoice {
                    version_group_start: index,
                    version_group_end: index,
                    authoring_version_group: index,
                    power_cap: definition.power_cap,
                    definition_hash: definition.hash,
                    version_group_range_label: format!("Cap table index {index}"),
                    picker_label: format!(
                        "{} power cap (table index {index})",
                        definition.power_cap
                    ),
                }
            })
            .collect()
    }

    /// Lists installed weapon definitions. Collection-backed donors sort first because they can
    /// participate in the safe collection/unlock clone workflow.
    #[must_use]
    pub fn weapon_donors(&self) -> Vec<WeaponDonorSummary> {
        let mut donors = self
            .catalog
            .items
            .iter()
            .filter_map(|item| self.weapon_donor_summary(item))
            .collect::<Vec<_>>();
        donors.sort_by_cached_key(|donor| {
            (
                !donor.collection_backed,
                donor.type_name.to_lowercase(),
                donor.name.to_lowercase(),
                donor.hash,
            )
        });
        donors
    }

    /// Lists every ornament that changes a model, paired with a weapon that can wear it.
    ///
    /// `weapon_ornaments` answers "what can this weapon wear". This answers the other
    /// direction, "which weapon lends this ornament its rig", which is what an appearance
    /// needs: the ornament supplies the model rows and colours, the weapon supplies the
    /// gear-art pattern row, the animation group and the runtime entity.
    ///
    /// Ornaments carrying no translation-art rows are left out. They can lend an icon, which
    /// the icon donor already covers, but they cannot stand in for an appearance.
    ///
    /// Several weapons can offer the same ornament. Keep every pairing here so the appearance
    /// picker can choose a host compatible with the authored weapon's inventory slot.
    #[must_use]
    pub fn weapon_ornament_appearances(
        &self,
        donors: &[WeaponDonorSummary],
    ) -> Vec<WeaponOrnamentAppearance> {
        let mut appearances = Vec::new();
        for donor in donors {
            for ornament in self.weapon_ornaments(donor.hash) {
                if ornament.art_arrangements.is_empty() {
                    continue;
                }
                appearances.push(WeaponOrnamentAppearance {
                    ornament,
                    host_item_hash: donor.hash,
                    host_name: donor.name.clone(),
                });
            }
        }
        appearances.sort_by_cached_key(|appearance| {
            (
                appearance.ornament.name.to_lowercase(),
                appearance.ornament.hash,
            )
        });
        appearances
    }

    /// Lists the ornaments offered by an installed weapon's own sockets.
    ///
    /// Only the weapon's own socket pools are consulted, so this is the set a player could apply
    /// to that weapon in game, not every ornament shipped for its frame. Ornaments that carry no
    /// translation-art rows are still listed because they can lend their icon.
    #[must_use]
    pub fn weapon_ornaments(&self, item_hash: u32) -> Vec<WeaponOrnament> {
        let Some(item) = self.catalog.item(u64::from(item_hash)) else {
            return Vec::new();
        };
        let mut ornaments = BTreeMap::new();
        for (socket_index, socket) in item.sockets.iter().enumerate() {
            for plug in self.catalog.socket_options(socket) {
                if !self.catalog.is_weapon_ornament(*plug) {
                    continue;
                }
                let Ok(hash) = u32::try_from(*plug) else {
                    continue;
                };
                let metadata = self.catalog.item_package_metadata(u64::from(hash));
                ornaments.entry(hash).or_insert_with(|| WeaponOrnament {
                    hash,
                    name: self.catalog.plug_label(u64::from(hash), false),
                    rarity: metadata
                        .map_or(WeaponRarity::Unknown, |metadata| metadata.rarity.into()),
                    socket_index,
                    art_arrangements: metadata
                        .into_iter()
                        .flat_map(|metadata| &metadata.art_arrangements)
                        .map(|row| WeaponArtArrangement {
                            character_class: row.character_class,
                            arrangement: row.arrangement,
                        })
                        .collect(),
                    icon_container_tag: metadata.and_then(|metadata| metadata.icon_container_tag),
                    render_dye_rows: std::array::from_fn(|stage| {
                        metadata
                            .into_iter()
                            .flat_map(|metadata| &metadata.translation_dye_rows[stage])
                            .map(|row| WeaponDyeReference {
                                channel_index: row.key,
                                dye_reference_index: row.value,
                            })
                            .collect()
                    }),
                });
            }
        }
        let mut ornaments = ornaments.into_values().collect::<Vec<_>>();
        ornaments.sort_by_cached_key(|ornament| {
            (
                ornament.socket_index,
                ornament.name.to_lowercase(),
                ornament.hash,
            )
        });
        ornaments
    }

    /// Returns the installed icon-container tag selected by a weapon's item-string row.
    #[must_use]
    pub fn weapon_icon_container(&self, item_hash: u32) -> Option<u32> {
        self.catalog
            .item_package_metadata(u64::from(item_hash))?
            .icon_container_tag
    }

    /// Returns the active finished sandbox-perk indices carried directly by an installed item.
    #[must_use]
    pub fn item_sandbox_perk_indices(&self, item_hash: u32) -> Vec<u16> {
        self.catalog
            .item_package_metadata(u64::from(item_hash))
            .map(|metadata| {
                metadata
                    .sandbox_perks
                    .iter()
                    .map(|perk| perk.perk_index)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Lists every active sandbox-perk index referenced by an installed item. The representative
    /// item is display metadata only; recipes store and author the finished numeric index.
    #[must_use]
    pub fn weapon_sandbox_perk_choices(&self) -> Vec<WeaponSandboxPerkChoice> {
        self.weapon_sandbox_perk_choices_from(|_| true)
    }

    /// Lists effect sources from accepted native item-definition tags. Authoring clients can
    /// exclude generated items whose private effect indices are absent from their build source.
    /// Every sandbox-perk index an installed item references, including declaration-only rows.
    ///
    /// [`Self::weapon_sandbox_perk_choices_from`] is the authoring choice set and is active-only.
    /// This is the wider "does the installed catalog know this index at all" set. Stock ships
    /// inactive rows on real plugs, such as 479 on Hard Light's intrinsic, so a validation guard
    /// that rejects them is stricter than the shipped data.
    pub fn referenced_sandbox_perk_indices(
        &self,
        include_definition: impl Fn(u32) -> bool,
    ) -> std::collections::BTreeSet<u16> {
        let hashes = self
            .catalog
            .items
            .iter()
            .map(|item| item.hash)
            .chain(self.catalog.all_plug_options().iter().copied())
            .collect::<BTreeSet<_>>();
        let mut indices = std::collections::BTreeSet::new();
        for item_hash in hashes {
            let Some(metadata) = self.catalog.item_package_metadata(item_hash) else {
                continue;
            };
            if !include_definition(metadata.definition_tag) {
                continue;
            }
            indices.extend(metadata.sandbox_perks.iter().map(|perk| perk.perk_index));
        }
        indices
    }

    pub fn weapon_sandbox_perk_choices_from(
        &self,
        include_definition: impl Fn(u32) -> bool,
    ) -> Vec<WeaponSandboxPerkChoice> {
        let mut choices = BTreeMap::new();
        // Perk plugs are not inventory items. Scanning only `items` hides effects
        // such as Outlaw and Rampage from the custom-perk effect selector.
        let hashes = self
            .catalog
            .items
            .iter()
            .map(|item| item.hash)
            .chain(self.catalog.all_plug_options().iter().copied())
            .collect::<BTreeSet<_>>();
        for item_hash in hashes {
            let Some(hash) = u32::try_from(item_hash).ok() else {
                continue;
            };
            let Some(metadata) = self.catalog.item_package_metadata(item_hash) else {
                continue;
            };
            if !include_definition(metadata.definition_tag) {
                continue;
            }
            // This list is the authoring choice set, and its contract is active rows only.
            // Declaration-only rows such as 479 stay in `item_sandbox_perk_indices`, which
            // reports what an item carries rather than what the producer will run.
            for perk in metadata.sandbox_perks.iter().filter(|perk| perk.active) {
                let name = self
                    .catalog
                    .display_name(item_hash)
                    .filter(|name| !name.trim().is_empty())
                    .or_else(|| self.catalog.package_item_name(item_hash))
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("Item 0x{hash:08X}"));
                let candidate = WeaponSandboxPerkChoice {
                    perk_index: perk.perk_index,
                    representative_hash: hash,
                    representative_name: name,
                    representative_type_name: self
                        .catalog
                        .plug_type_name(item_hash)
                        .or_else(|| self.catalog.package_item_type_name(item_hash))
                        .unwrap_or("")
                        .to_owned(),
                };
                choices
                    .entry(perk.perk_index)
                    .and_modify(|current: &mut WeaponSandboxPerkChoice| {
                        let current_quality = representative_perk_label_quality(
                            current,
                            self.catalog
                                .contains_plug(u64::from(current.representative_hash)),
                        );
                        let candidate_quality = representative_perk_label_quality(
                            &candidate,
                            self.catalog.contains_plug(item_hash),
                        );
                        if candidate_quality < current_quality {
                            *current = candidate.clone();
                        }
                    })
                    .or_insert(candidate);
            }
        }
        choices.into_values().collect()
    }

    /// Every named plug remains available as a starting point, including aliases and stat-only plugs.
    /// This list must not be reduced to one representative per runtime effect.
    #[must_use]
    pub fn perk_template_choices_from(
        &self,
        include_definition: impl Fn(u32) -> bool,
    ) -> Vec<WeaponSandboxPerkChoice> {
        let mut choices = self
            .catalog
            .all_plug_options()
            .iter()
            .filter_map(|&item_hash| {
                let hash = u32::try_from(item_hash).ok()?;
                let metadata = self.catalog.item_package_metadata(item_hash)?;
                if !include_definition(metadata.definition_tag) {
                    return None;
                }
                let name = self
                    .catalog
                    .display_name(item_hash)
                    .or_else(|| self.catalog.package_item_name(item_hash))?;
                if name.trim().is_empty() {
                    return None;
                }
                Some(WeaponSandboxPerkChoice {
                    perk_index: metadata
                        .sandbox_perks
                        .first()
                        .map_or(0, |perk| perk.perk_index),
                    representative_hash: hash,
                    representative_name: name.to_owned(),
                    representative_type_name: self
                        .catalog
                        .plug_type_name(item_hash)
                        .or_else(|| self.catalog.package_item_type_name(item_hash))
                        .unwrap_or_default()
                        .to_owned(),
                })
            })
            .collect::<Vec<_>>();
        choices.sort_by_cached_key(|choice| {
            (
                choice.representative_name.to_lowercase(),
                choice.representative_hash,
            )
        });
        choices
    }

    /// Lists every installed trait definition, including definitions not currently referenced by
    /// a stock weapon. Recipes store the native table index because that is what item payloads use.
    #[must_use]
    pub fn weapon_trait_choices(&self) -> Vec<WeaponTraitChoice> {
        self.catalog
            .trait_definitions()
            .iter()
            .enumerate()
            .filter_map(|(index, definition)| {
                Some(WeaponTraitChoice {
                    trait_index: u16::try_from(index).ok()?,
                    hash: u32::try_from(definition.hash).ok()?,
                    name: definition.name.clone(),
                    description: definition.description.clone(),
                })
            })
            .collect()
    }

    /// Number of addressable rows in the installed reusable/randomized plug-set table.
    #[must_use]
    pub fn reusable_plug_set_count(&self) -> usize {
        self.catalog.reusable_plug_set_count()
    }

    /// Number of addressable rows in the installed socket-entry-list table.
    #[must_use]
    pub fn socket_entry_list_count(&self) -> usize {
        self.catalog.socket_entry_list_count()
    }

    #[must_use]
    pub fn weapon_donor(&self, hash: u32) -> Option<WeaponDonor> {
        self.weapon_donor_with_stat_group_index(hash, None)
    }

    /// Returns an installed weapon donor while decoding stat bounds and display interpolation from
    /// an explicitly selected installed stat group. `None` preserves the donor's native group.
    /// Unknown group indices are rejected rather than exposed as unbounded authoring profiles.
    #[must_use]
    pub fn weapon_donor_with_stat_group_index(
        &self,
        hash: u32,
        stat_group_index: Option<u16>,
    ) -> Option<WeaponDonor> {
        let item = self.catalog.item(u64::from(hash))?;
        let summary = self.weapon_donor_summary(item)?;
        self.donor_from_item(item, summary, stat_group_index)
    }

    /// Lists installed items from `bucket_hashes` that can serve as the base of an authored armor
    /// piece, Sparrow, Ship or Ghost Shell. Collection-backed items sort first, because the build
    /// places the authored item on its base item's own Collections page.
    #[must_use]
    pub fn gear_donors(&self, bucket_hashes: &[u64]) -> Vec<WeaponDonorSummary> {
        let mut donors = self
            .catalog
            .items
            .iter()
            .filter(|item| bucket_hashes.contains(&item.bucket_hash))
            .filter_map(|item| self.gear_donor_summary(item))
            .collect::<Vec<_>>();
        donors.sort_by_cached_key(|donor| {
            (
                !donor.collection_backed,
                donor.type_name.to_lowercase(),
                donor.name.to_lowercase(),
                donor.hash,
            )
        });
        donors
    }

    /// An installed armor piece, Sparrow, Ship or Ghost Shell with its sockets and stats.
    #[must_use]
    pub fn gear_donor(&self, hash: u32) -> Option<WeaponDonor> {
        let item = self.catalog.item(u64::from(hash))?;
        let summary = self.gear_donor_summary(item)?;
        self.donor_from_item(item, summary, None)
    }

    /// Installed subclasses whose definitions `include_definition` accepts, with the names of
    /// their abilities and attunements, in class and name order.
    #[must_use]
    pub fn subclasses(&self, include_definition: impl Fn(u32) -> bool) -> Vec<SubclassSummary> {
        let mut subclasses = self
            .catalog
            .items
            .iter()
            .filter(|item| item.bucket_hash == crate::catalog::SUBCLASS_BUCKET_HASH)
            .filter(|item| {
                self.catalog
                    .item_package_metadata(item.hash)
                    .is_some_and(|metadata| include_definition(metadata.definition_tag))
            })
            .filter_map(|item| {
                let abilities = &item.abilities;
                let mut entry_names = BTreeMap::new();
                for choice in abilities
                    .class_ability
                    .iter()
                    .chain(&abilities.movement)
                    .chain(&abilities.grenade)
                    .chain(&abilities.super_ability)
                    .chain(abilities.attunements.iter().flat_map(|path| &path.perks))
                {
                    entry_names.insert(u8::try_from(choice.entry).ok()?, choice.name.clone());
                }
                Some(SubclassSummary {
                    hash: u32::try_from(item.hash).ok()?,
                    name: item.name.clone(),
                    class_type: u8::try_from(item.class_type).unwrap_or(3),
                    entry_names,
                    attunement_names: abilities
                        .attunements
                        .iter()
                        .map(|path| path.name.clone())
                        .collect(),
                    entry_perks: abilities
                        .entry_perks
                        .iter()
                        .filter_map(|(entry, perks)| {
                            Some((u8::try_from(*entry).ok()?, perks.clone()))
                        })
                        .collect(),
                    entry_icons: abilities
                        .entry_icons
                        .iter()
                        .filter_map(|(entry, icon)| Some((u8::try_from(*entry).ok()?, *icon)))
                        .collect(),
                    entry_descriptions: abilities
                        .entry_descriptions
                        .iter()
                        .filter_map(|(entry, description)| {
                            Some((u8::try_from(*entry).ok()?, description.clone()))
                        })
                        .collect(),
                })
            })
            .collect::<Vec<_>>();
        subclasses.sort_by_cached_key(|subclass| {
            (
                subclass.class_type,
                subclass.name.to_lowercase(),
                subclass.hash,
            )
        });
        subclasses
    }

    /// Stock shaders that can serve as a shader recipe's base, those in Collections first.
    #[must_use]
    pub fn shader_donors(&self) -> Vec<WeaponDonorSummary> {
        let mut donors = self
            .catalog
            .package_metadata()
            .filter(|(_, metadata)| is_stock_shader(metadata))
            .filter_map(|(hash, metadata)| {
                Some(WeaponDonorSummary {
                    hash: u32::try_from(hash).ok()?,
                    name: self.catalog.display_name(hash)?.to_owned(),
                    type_name: "Shader".to_owned(),
                    bucket_hash: SHADER_BUCKET_HASH,
                    collection_backed: self.catalog.item_has_collectible(hash),
                    power_cap: None,
                    damage_type: None,
                    inventory_slot: None,
                    ammo_type: None,
                    weapon_pattern_index: None,
                    weapon_translation_group: None,
                    stat_group_index: None,
                    damage_profile: WeaponDamageProfile::Unknown,
                    rarity: metadata.rarity.into(),
                })
            })
            .collect::<Vec<_>>();
        donors.sort_by_cached_key(|donor| {
            (
                !donor.collection_backed,
                donor.name.to_lowercase(),
                donor.hash,
            )
        });
        donors
    }

    /// The most an item's stat group lets any of its stats show: 42 on Armor 2.0, 3 on Armor 1.0.
    #[must_use]
    pub fn item_stat_maximum(&self, hash: u32) -> Option<i32> {
        let index = self
            .catalog
            .item_package_metadata(u64::from(hash))?
            .stat_group_index?;
        self.catalog
            .item_stat_group_by_index(index)
            .map(|group| group.maximum_value)
    }

    /// Whether an installed plug is a stock-shaped shader.
    #[must_use]
    pub fn is_shader(&self, hash: u32) -> bool {
        self.catalog
            .item_package_metadata(u64::from(hash))
            .is_some_and(is_stock_shader)
    }

    /// The class an armor piece is for: 0 Titan, 1 Hunter, 2 Warlock, 3 any.
    #[must_use]
    pub fn item_class_type(&self, hash: u32) -> Option<u8> {
        self.catalog
            .item(u64::from(hash))
            .and_then(|item| u8::try_from(item.class_type).ok())
    }

    /// The socket types an installed item carries, in socket order. Empty for an unknown item.
    #[must_use]
    pub fn item_socket_types(&self, hash: u32) -> Vec<u16> {
        self.catalog
            .item(u64::from(hash))
            .map(|item| {
                item.sockets
                    .iter()
                    .map(|socket| socket.socket_type)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn gear_donor_summary(&self, item: &crate::catalog::ItemDef) -> Option<WeaponDonorSummary> {
        if is_weapon_bucket(item.bucket_hash) {
            return None;
        }
        let metadata = self.catalog.item_package_metadata(item.hash);
        Some(WeaponDonorSummary {
            hash: u32::try_from(item.hash).ok()?,
            name: item.name.clone(),
            type_name: item.type_name.trim().to_owned(),
            bucket_hash: item.bucket_hash,
            collection_backed: self.catalog.item_has_collectible(item.hash),
            power_cap: metadata.and_then(|metadata| metadata.power_cap),
            damage_type: None,
            inventory_slot: None,
            ammo_type: None,
            weapon_pattern_index: None,
            weapon_translation_group: None,
            stat_group_index: metadata.and_then(|metadata| metadata.stat_group_index),
            damage_profile: WeaponDamageProfile::Unknown,
            rarity: metadata.map_or(WeaponRarity::Unknown, |metadata| metadata.rarity.into()),
        })
    }

    fn donor_from_item(
        &self,
        item: &crate::catalog::ItemDef,
        summary: WeaponDonorSummary,
        stat_group_index: Option<u16>,
    ) -> Option<WeaponDonor> {
        let hash = item.hash;
        let metadata = self.catalog.item_package_metadata(hash);
        if stat_group_index
            .is_some_and(|index| self.catalog.item_stat_group_by_index(index).is_none())
        {
            return None;
        }
        let effective_stat_group_index =
            stat_group_index.or_else(|| metadata.and_then(|metadata| metadata.stat_group_index));
        let sockets = item
            .sockets
            .iter()
            .enumerate()
            .map(|(index, socket)| {
                let native_default = item
                    .default_plugs
                    .get(index)
                    .and_then(Option::as_deref)
                    .and_then(parse_hash_hex)
                    .and_then(|hash| u32::try_from(hash).ok());
                let compatible_plug_count = self.catalog.socket_options(socket).len();
                WeaponSocket {
                    index,
                    socket_type: socket.socket_type,
                    label: socket.display_label(index),
                    native_default,
                    ordered_embedded_choices: socket
                        .ordered_embedded_choices()
                        .iter()
                        .copied()
                        .filter_map(|hash| u32::try_from(hash).ok())
                        .collect(),
                    max_authored_choices: available_authored_socket_choices(socket.socket_type),
                    compatible_plug_count,
                    reusable_plug_set_index: socket.reusable_set_index(),
                    randomized_plug_set_index: socket.randomized_set_index(),
                }
            })
            .collect();
        let investment_stats = metadata
            .into_iter()
            .flat_map(|metadata| &metadata.investment_stats)
            .map(|stat| {
                self.weapon_investment_stat(
                    effective_stat_group_index,
                    stat.definition_index,
                    stat.value,
                )
            })
            .collect::<Vec<_>>();
        let native_stat_indices = investment_stats
            .iter()
            .map(|stat| stat.definition_index)
            .collect::<BTreeSet<_>>();
        let addable_investment_stats = self
            .authorable_weapon_stat_indices
            .iter()
            .copied()
            .filter(|definition_index| !native_stat_indices.contains(definition_index))
            .map(|definition_index| {
                self.weapon_investment_stat(effective_stat_group_index, definition_index, 0)
            })
            .collect();
        Some(WeaponDonor {
            summary,
            power_cap_groups: metadata
                .map_or_else(Vec::new, |metadata| metadata.power_cap_groups.clone()),
            equipment_slot: metadata
                .and_then(|metadata| metadata.equipment_slot)
                .and_then(ItemWeaponInventorySlot::from_equipment_slot)
                .map(WeaponInventorySlot::from),
            sockets,
            investment_stats,
            addable_investment_stats,
            base_sandbox_perks: metadata
                .into_iter()
                .flat_map(|metadata| &metadata.sandbox_perks)
                .map(|perk| perk.perk_index)
                .collect(),
            trait_indices: metadata
                .map_or_else(Vec::new, |metadata| metadata.trait_indices.clone()),
            max_stack_size: self
                .catalog
                .inventory_metadata(hash)
                .and_then(|metadata| metadata.max_stack_size),
            socket_entry_list_index: metadata.and_then(|metadata| metadata.socket_entry_list_index),
            plug_category_hash: metadata
                .and_then(|metadata| metadata.plug_category_hash)
                .and_then(|hash| u32::try_from(hash).ok()),
            roll_set_index: metadata.and_then(|metadata| metadata.roll_set_index),
            linked_plug_index: metadata.and_then(|metadata| metadata.linked_plug_index),
            linked_plug_hash: metadata
                .and_then(|metadata| metadata.linked_plug_hash)
                .and_then(|hash| u32::try_from(hash).ok()),
            art_arrangements: metadata
                .into_iter()
                .flat_map(|metadata| &metadata.art_arrangements)
                .map(|row| WeaponArtArrangement {
                    character_class: row.character_class,
                    arrangement: row.arrangement,
                })
                .collect(),
            render_dye_rows: std::array::from_fn(|stage| {
                metadata
                    .into_iter()
                    .flat_map(|metadata| &metadata.translation_dye_rows[stage])
                    .map(|row| WeaponDyeReference {
                        channel_index: row.key,
                        dye_reference_index: row.value,
                    })
                    .collect()
            }),
        })
    }

    /// Declared stat contributions on an item or plug, without weapon display scaling.
    pub fn item_stat_contributions(&self, hash: u32) -> Vec<WeaponInvestmentStat> {
        self.catalog
            .item_package_metadata(u64::from(hash))
            .into_iter()
            .flat_map(|metadata| &metadata.investment_stats)
            .map(|stat| self.weapon_investment_stat(None, stat.definition_index, stat.value))
            .collect()
    }

    /// Native appearance colors on a weapon, ornament or shader plug.
    pub fn item_render_dye_rows(&self, hash: u32) -> [Vec<WeaponDyeReference>; 3] {
        let metadata = self.catalog.item_package_metadata(u64::from(hash));
        std::array::from_fn(|stage| {
            metadata
                .into_iter()
                .flat_map(|metadata| &metadata.translation_dye_rows[stage])
                .map(|row| WeaponDyeReference {
                    channel_index: row.key,
                    dye_reference_index: row.value,
                })
                .collect()
        })
    }

    /// Native stat definitions for independent perk authoring, without a weapon context.
    #[must_use]
    pub fn perk_stat_choices(&self) -> Vec<WeaponInvestmentStat> {
        (0..self.catalog.item_stat_definition_count().min(256))
            .filter_map(|index| u16::try_from(index).ok())
            .map(|index| self.weapon_investment_stat(None, index, 0))
            .collect()
    }

    #[must_use]
    pub fn is_plug(&self, hash: u32) -> bool {
        self.catalog.contains_plug(u64::from(hash))
    }

    #[must_use]
    pub fn perk_description(&self, hash: u32) -> Option<&str> {
        self.catalog.description(u64::from(hash))
    }

    /// Localized description attached to this exact finished perk row.
    /// Item text can describe an entire mod or subclass and is not interchangeable.
    #[must_use]
    pub fn perk_component_description(&self, index: u16) -> Option<&str> {
        self.catalog.perk_description(index)
    }

    fn weapon_investment_stat(
        &self,
        stat_group_index: Option<u16>,
        definition_index: u16,
        value: i32,
    ) -> WeaponInvestmentStat {
        let definition = self.catalog.item_stat_definition(definition_index);
        let stat_group =
            stat_group_index.and_then(|index| self.catalog.item_stat_group_by_index(index));
        let scaled_stat = stat_group.and_then(|group| {
            group
                .scaled_stats
                .iter()
                .find(|stat| stat.definition_index == definition_index)
        });
        let definition_hash = definition.and_then(|definition| u32::try_from(definition.hash).ok());
        WeaponInvestmentStat {
            definition_index,
            definition_hash,
            name: definition
                .map(|definition| definition.name.trim())
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Stat {definition_index}")),
            value,
            minimum_value: stat_group.and_then(|group| group.minimum_value(definition_index)),
            maximum_value: stat_group.map(|group| group.maximum_value),
            display_as_numeric: scaled_stat
                .is_some_and(|scaled_stat| scaled_stat.display_as_numeric),
            is_linear: scaled_stat.is_some_and(|scaled_stat| scaled_stat.is_linear),
            display_interpolation: scaled_stat
                .into_iter()
                .flat_map(|scaled_stat| &scaled_stat.display_interpolation)
                .map(|point| WeaponStatDisplayPoint {
                    investment_value: point.investment_value,
                    display_value: point.display_value,
                })
                .collect(),
        }
    }

    fn weapon_donor_summary(&self, item: &crate::catalog::ItemDef) -> Option<WeaponDonorSummary> {
        if !is_authorable_weapon_item(item) {
            return None;
        }
        let metadata = self.catalog.item_package_metadata(item.hash);
        Some(WeaponDonorSummary {
            hash: u32::try_from(item.hash).ok()?,
            name: item.name.clone(),
            type_name: item.type_name.clone(),
            bucket_hash: item.bucket_hash,
            collection_backed: self.catalog.item_has_collectible(item.hash),
            power_cap: metadata.and_then(|metadata| metadata.power_cap),
            damage_type: metadata
                .and_then(|metadata| metadata.damage_type)
                .map(WeaponDamageType::from),
            inventory_slot: metadata
                .and_then(|metadata| metadata.weapon_inventory_slot)
                .map(WeaponInventorySlot::from),
            ammo_type: metadata
                .and_then(|metadata| metadata.weapon_ammo_type)
                .map(WeaponAmmoType::from),
            weapon_pattern_index: metadata.and_then(|metadata| metadata.weapon_pattern_index),
            weapon_translation_group: metadata
                .and_then(|metadata| metadata.weapon_translation_group),
            stat_group_index: metadata.and_then(|metadata| metadata.stat_group_index),
            damage_profile: metadata.map_or(WeaponDamageProfile::Unknown, |metadata| {
                metadata.damage_profile.into()
            }),
            rarity: metadata.map_or(WeaponRarity::Unknown, |metadata| metadata.rarity.into()),
        })
    }

    /// Returns the normalized compatible plug hashes for every socket on a donor.
    ///
    /// These broad, sorted picker pools combine compatible package data across weapons of the
    /// same gear and socket type. They are intentionally different from
    /// [`WeaponSocket::ordered_embedded_choices`], which preserves only the donor's native
    /// embedded column. The native default is retained when a pool omits it. A disabled value is
    /// exposed only for a natively disabled `FFFF` socket.
    pub fn weapon_supported_plug_sets(
        &self,
        donor_hash: u32,
    ) -> Result<Vec<WeaponSupportedPlugSet>, String> {
        self.weapon_supported_plug_sets_with_socket_types(donor_hash, &[])
    }

    /// Returns compatible plug hashes for replaced and appended socket types.
    pub fn weapon_supported_plug_sets_with_socket_types(
        &self,
        donor_hash: u32,
        socket_types: &[Option<u16>],
    ) -> Result<Vec<WeaponSupportedPlugSet>, String> {
        let item = self
            .catalog
            .item(u64::from(donor_hash))
            .ok_or_else(|| format!("Unknown donor weapon 0x{donor_hash:08X}"))?;
        if !is_authorable_weapon_item(item) {
            return Err(format!(
                "Item 0x{donor_hash:08X} is not an authorable weapon"
            ));
        }
        self.supported_plug_sets(item, donor_hash, socket_types)
    }

    /// Compatible plugs for every socket on an armor piece, Sparrow, Ship or Ghost Shell.
    pub fn gear_supported_plug_sets(
        &self,
        donor_hash: u32,
    ) -> Result<Vec<WeaponSupportedPlugSet>, String> {
        let item = self
            .catalog
            .item(u64::from(donor_hash))
            .ok_or_else(|| format!("Unknown base item 0x{donor_hash:08X}"))?;
        if is_weapon_bucket(item.bucket_hash) {
            return Err(format!("Item 0x{donor_hash:08X} is a weapon"));
        }
        self.supported_plug_sets(item, donor_hash, &[])
    }

    fn supported_plug_sets(
        &self,
        item: &crate::catalog::ItemDef,
        donor_hash: u32,
        socket_types: &[Option<u16>],
    ) -> Result<Vec<WeaponSupportedPlugSet>, String> {
        let socket_count = item.sockets.len().max(socket_types.len());
        if socket_count > MAX_WEAPON_SOCKETS {
            return Err(format!(
                "Weapons support at most {MAX_WEAPON_SOCKETS} ordinary sockets"
            ));
        }
        (0..socket_count)
            .map(|socket_index| {
                let socket = item.sockets.get(socket_index);
                let socket_type = socket_types
                    .get(socket_index)
                    .copied()
                    .flatten()
                    .or_else(|| socket.map(|socket| socket.socket_type))
                    .ok_or_else(|| format!("Added socket {} requires a socket type", socket_index + 1))?;
                let native_default = item
                    .default_plugs
                    .get(socket_index)
                    .and_then(Option::as_deref)
                    .and_then(parse_hash_hex)
                    .map(u32::try_from)
                    .transpose()
                    .map_err(|_| {
                        format!(
                            "Donor weapon 0x{donor_hash:08X} socket {socket_index} has a default plug hash larger than 32 bits"
                        )
                    })?;
                let unchanged_type = socket.is_some_and(|socket| socket_type == socket.socket_type);
                let mut plug_hashes = if unchanged_type {
                    self.catalog
                        .socket_and_gear_type_options(item, socket_index)
                } else {
                    self.catalog
                        .socket_and_gear_type_options_for_type(item, socket_type)
                }
                    .iter()
                    .copied()
                    .map(u32::try_from)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| {
                        format!(
                            "Donor weapon 0x{donor_hash:08X} socket {socket_index} has a compatible plug hash larger than 32 bits"
                        )
                    })?;
                if unchanged_type
                    && let Some(native_default) = native_default
                {
                    plug_hashes.push(native_default);
                }
                plug_hashes.sort_unstable();
                plug_hashes.dedup();
                Ok(WeaponSupportedPlugSet {
                    socket_index,
                    plug_hashes,
                    allows_disabled: socket_type == u16::MAX && native_default.is_none(),
                })
            })
            .collect()
    }

    /// Socket categories present in the installed package catalog, sorted by native ID.
    pub fn weapon_socket_type_choices(
        &self,
        donor_hash: u32,
    ) -> Result<Vec<WeaponSocketTypeChoice>, String> {
        let item = self
            .catalog
            .item(u64::from(donor_hash))
            .ok_or_else(|| format!("Unknown donor weapon 0x{donor_hash:08X}"))?;
        if !is_authorable_weapon_item(item) {
            return Err(format!(
                "Item 0x{donor_hash:08X} is not an authorable weapon"
            ));
        }
        Ok(self
            .catalog
            .socket_and_gear_type_option_counts(item)
            .into_iter()
            .filter(|(socket_type, _)| *socket_type != u16::MAX)
            .map(
                |(socket_type, compatible_plug_count)| WeaponSocketTypeChoice {
                    socket_type,
                    label: self.catalog.socket_type_label_for_item(item, socket_type),
                    compatible_plug_count,
                },
            )
            .collect())
    }

    /// Whether a native socket category is represented by the donor's installed weapon family.
    #[must_use]
    pub fn weapon_socket_type_is_known(&self, donor_hash: u32, socket_type: u16) -> bool {
        socket_type != u16::MAX
            && self
                .catalog
                .item(u64::from(donor_hash))
                .is_some_and(|item| {
                    is_authorable_weapon_item(item)
                        && self
                            .catalog
                            .socket_type_is_known_for_item(item, socket_type)
                })
    }
}

/// Structural authoring ceiling for one embedded socket-member array.
///
/// The native array count is `u64`, but each unique member resolves to a live `u16` item-table
/// index and `u16::MAX` is the disabled sentinel. That leaves exactly 65,535 addressable member
/// indices. Reusable and randomized plug-set sizes are separate package structures and do not
/// affect this limit.
pub const MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES: usize = u16::MAX as usize;

/// Fixed number of ordinary socket lanes carried by a native item instance.
pub const MAX_WEAPON_SOCKETS: usize = 12;

/// Maximum number of embedded plug choices the package compiler accepts for a socket column.
#[must_use]
pub const fn authored_socket_choice_limit(socket_type: u16) -> usize {
    if socket_type == u16::MAX {
        0
    } else {
        MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES
    }
}

const fn available_authored_socket_choices(socket_type: u16) -> usize {
    authored_socket_choice_limit(socket_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appended_socket_pools_follow_explicit_type_and_native_limit() {
        use crate::catalog::{ItemDef, SocketDef};
        let catalog = InvestmentCatalog {
            catalog: Catalog::for_test(
                vec![ItemDef {
                    hash: 1,
                    name: "Test weapon".into(),
                    type_name: "Auto Rifle".into(),
                    bucket_hash: 1_498_876_634,
                    class_type: 3,
                    default_plugs: vec![Some("0x65".into())],
                    sockets: vec![SocketDef {
                        socket_type: 700,
                        allowed: vec![101, 102],
                        ..Default::default()
                    }],
                    abilities: Default::default(),
                }],
                Default::default(),
            ),
            authorable_weapon_stat_indices: Vec::new(),
        };
        let pools = catalog
            .weapon_supported_plug_sets_with_socket_types(1, &[None, Some(700)])
            .unwrap();
        assert_eq!(pools.len(), 2);
        assert_eq!(pools[1].socket_index, 1);
        assert_eq!(pools[1].plug_hashes, pools[0].plug_hashes);
        assert_eq!(pools[1].plug_hashes, vec![101, 102]);
        assert!(!pools[1].allows_disabled);
        assert!(
            catalog
                .weapon_supported_plug_sets_with_socket_types(1, &[None, None])
                .is_err()
        );
        assert!(
            catalog
                .weapon_supported_plug_sets_with_socket_types(1, &[Some(700); MAX_WEAPON_SOCKETS])
                .is_ok()
        );
        assert!(
            catalog
                .weapon_supported_plug_sets_with_socket_types(
                    1,
                    &[Some(700); MAX_WEAPON_SOCKETS + 1]
                )
                .is_err()
        );
    }

    #[test]
    #[ignore = "requires SUNDIAL_TEST_INSTALL; native item and plug effect catalog"]
    fn sandbox_effect_choices_include_conditional_plug_effects() {
        let install = std::path::PathBuf::from(std::env::var_os("SUNDIAL_TEST_INSTALL").unwrap());
        let catalog = InvestmentCatalog::load(&install, false, |_| {}).unwrap();
        let choices = catalog.weapon_sandbox_perk_choices();
        for (index, name) in [
            (421, "Outlaw"),
            (351, "Rampage"),
            (352, "Grave Robber"),
            (1300, "Swashbuckler"),
            (951, "Ashes to Assets"),
        ] {
            let choice = choices
                .iter()
                .find(|choice| choice.perk_index == index)
                .expect("stock conditional effect");
            assert_eq!(choice.representative_name, name);
            assert!(
                catalog
                    .item_sandbox_perk_indices(choice.representative_hash)
                    .contains(&index)
            );
        }
    }

    #[test]
    #[ignore = "requires SUNDIAL_TEST_INSTALL; reads native plug classification without changing the shared cache"]
    fn private_tooltip_uses_authored_intrinsic_without_relabeling_stock_trait() {
        let install = std::path::PathBuf::from(std::env::var_os("SUNDIAL_TEST_INSTALL").unwrap());
        let temporary = crate::test_support::TestDirectory::new("private-plug-tooltip");
        let catalog = InvestmentCatalog {
            catalog: Catalog::load_or_scan_with_progress(
                &install,
                temporary.0.join("catalog.json"),
                true,
                |_| {},
            )
            .unwrap(),
            authorable_weapon_stat_indices: Vec::new(),
        };
        let source = 0xDD5C_B37A;
        let stock = catalog.private_plug_tooltip(source, None, None, None);
        assert_eq!(stock.lines().nth(1), Some("Trait"));
        let description = "This weapon fires a high-speed micro-missile in a straight line. Move faster with this weapon equipped.";
        for intrinsic in [0xC684_24BC, 0x6185_084A] {
            assert_eq!(
                catalog.private_plug_tooltip(
                    source,
                    Some(intrinsic),
                    Some("Micro-Missile Frame"),
                    Some(description),
                ),
                format!("Micro-Missile Frame\nIntrinsic\n\n{description}")
            );
        }
        assert_eq!(
            catalog.private_plug_tooltip(source, None, None, None),
            stock
        );
    }
}
