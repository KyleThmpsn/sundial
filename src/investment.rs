//! Installed-item catalog and picker controls shared by Sundial and Parhelion.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(crate) mod plug_selection;
pub use plug_selection::PlugSelectionMode;

mod account_sync;
mod controls;
mod definitions;
pub use account_sync::{
    AuthoredAccountCleanup, AuthoredCollectionUnlock, AuthoredProfileSyncReport,
    preview_authored_account_cleanup, synchronize_authored_collection_unlocks,
    validate_authored_cleanup_backend,
};
pub use controls::{
    AUTHORING_SOCKET_RESET_WIDTH, CatalogLoadingView, PlugChoicePickerButton, PlugSelection,
    WeaponDonorPickerAction, WeaponDonorPickerClearChoice, WeaponDonorPickerOptions,
    authoring_button_width, authoring_socket_label_width, authoring_socket_reset_width,
    configure_authoring_fonts, default_plug_selection_mode, draw_authoring_info_icon,
    draw_authoring_socket_label, draw_authoring_socket_reset, draw_authoring_toolbar,
    draw_catalog_loading_view, draw_plug_safety_selector, draw_plug_safety_warning,
    show_plug_safety_warnings,
};
pub use definitions::{
    PowerCapChoice, WeaponAmmoType, WeaponArtArrangement, WeaponDamageCarrierFamily,
    WeaponDamageProfile, WeaponDamageType, WeaponDonor, WeaponDonorSummary, WeaponDyeReference,
    WeaponInventorySlot, WeaponInvestmentStat, WeaponRarity, WeaponSandboxPerkChoice, WeaponSocket,
    WeaponSocketTypeChoice, WeaponStatDisplayPoint, WeaponSupportedPlugSet, WeaponTraitChoice,
};

use crate::{
    catalog::{Catalog, ItemWeaponInventorySlot, is_authorable_weapon_item},
    hash::parse_hash_hex,
    paths,
};

#[cfg(test)]
use crate::catalog::is_weapon_bucket;

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
    /// Loads (or scans) the installed Shadowkeep catalog using Sundial's shared cache.
    pub fn load(
        install_directory: &Path,
        force_rebuild: bool,
        mut report: impl FnMut(CatalogLoadProgress),
    ) -> Result<Self, String> {
        let cache_path = paths::shadowkeep_catalog_path()
            .ok_or_else(|| "Could not locate Sundial's local catalog folder".to_owned())?;
        let catalog = Catalog::load_or_scan_with_progress(
            install_directory,
            cache_path,
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
            for perk in &metadata.sandbox_perks {
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
        let hash = u64::from(hash);
        let item = self.catalog.item(hash)?;
        let metadata = self.catalog.item_package_metadata(hash);
        if stat_group_index
            .is_some_and(|index| self.catalog.item_stat_group_by_index(index).is_none())
        {
            return None;
        }
        let effective_stat_group_index =
            stat_group_index.or_else(|| metadata.and_then(|metadata| metadata.stat_group_index));
        let summary = self.weapon_donor_summary(item)?;
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

    /// Returns compatible plug hashes using explicit socket-type replacements where supplied.
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
        item.sockets
            .iter()
            .enumerate()
            .map(|(socket_index, socket)| {
                let socket_type = socket_types
                    .get(socket_index)
                    .copied()
                    .flatten()
                    .unwrap_or(socket.socket_type);
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
                let mut plug_hashes = if socket_type == socket.socket_type {
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
                if socket_type == socket.socket_type
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
    use crate::catalog::ItemRarity;

    #[test]
    fn sandbox_effect_labels_prefer_named_plugs_over_weapon_names() {
        let plug = WeaponSandboxPerkChoice {
            perk_index: 421,
            representative_hash: 1,
            representative_name: "Outlaw".into(),
            representative_type_name: "Trait".into(),
        };
        let mut weapon = plug.clone();
        weapon.representative_name = "Gun".into();
        weapon.representative_type_name = "Hand Cannon".into();
        assert!(
            representative_perk_label_quality(&plug, true)
                < representative_perk_label_quality(&weapon, false)
        );
        let mut unnamed = plug.clone();
        unnamed.representative_name = "Item 0x00000001".into();
        assert!(
            representative_perk_label_quality(&weapon, false)
                < representative_perk_label_quality(&unnamed, true)
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

    #[test]
    fn authored_socket_choice_capacity_matches_compiler_contract() {
        assert_eq!(authored_socket_choice_limit(u16::MAX), 0);
        assert_eq!(available_authored_socket_choices(u16::MAX), 0);
        for socket_type in [42, 176, 180, 483, 518, 687, 700] {
            assert_eq!(
                authored_socket_choice_limit(socket_type),
                MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES
            );
            assert_eq!(
                available_authored_socket_choices(socket_type),
                MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES
            );
        }
    }

    #[test]
    fn socket_type_choice_keeps_semantic_label_and_native_id() {
        assert_eq!(
            WeaponSocketTypeChoice {
                socket_type: 65,
                label: "Barrel".into(),
                compatible_plug_count: 37,
            }
            .display_label(),
            "Barrel · 65"
        );
        assert_eq!(
            WeaponSocketTypeChoice {
                socket_type: 999,
                label: String::new(),
                compatible_plug_count: 0,
            }
            .display_label(),
            "Type 999"
        );
    }

    #[test]
    fn recognizes_all_three_weapon_buckets() {
        assert!(is_weapon_bucket(1_498_876_634));
        assert!(is_weapon_bucket(2_465_295_065));
        assert!(is_weapon_bucket(953_998_645));
        assert!(!is_weapon_bucket(0));
    }

    #[test]
    fn donor_rarity_mapping_and_labels_are_stable_for_external_tools() {
        let mappings = [
            (ItemRarity::Unknown, WeaponRarity::Unknown, "Unknown"),
            (ItemRarity::Common, WeaponRarity::Common, "Common"),
            (ItemRarity::Uncommon, WeaponRarity::Uncommon, "Uncommon"),
            (ItemRarity::Rare, WeaponRarity::Rare, "Rare"),
            (ItemRarity::Legendary, WeaponRarity::Legendary, "Legendary"),
            (ItemRarity::Exotic, WeaponRarity::Exotic, "Exotic"),
        ];
        for (source, expected, label) in mappings {
            let rarity = WeaponRarity::from(source);
            assert_eq!(rarity, expected);
            assert_eq!(rarity.label(), label);
        }
    }

    #[test]
    fn parhelion_recipe_library_is_below_sundial_data() {
        let expected_suffix = std::path::Path::new("parhelion").join("recipes");
        let directory = crate::package_authoring::parhelion_recipe_library_directory()
            .expect("the test platform should expose a per-user data directory");

        assert!(directory.ends_with(expected_suffix));
    }
}
