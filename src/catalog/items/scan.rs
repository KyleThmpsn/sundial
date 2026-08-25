//! Inventory-item table scanning and item-domain catalog assembly.

use std::collections::HashMap;

use tiger_pkg::{PackageManager, TagHash};

use crate::{
    class_items,
    hash::{format_hash_hex, parse_hash_hex},
};

use super::{
    AbilityOptions, ItemDef, ItemPackageMetadata, ItemRarity, ItemStatDefinition, SocketDef,
    abilities::{AbilityDisplayData, build_subclass_choices},
    inventory::{InventoryBucketDescriptor, item_inventory_metadata},
    investment::{item_investment_stats, masterwork_label, stat_allocation_labels},
    item_damage_type,
    perks::item_intrinsic_perk_hashes,
    quality::item_power_cap,
    sockets::{ORDINARY_SOCKET_CLASS, build_socket_choices, socket_allowed_hashes},
};
use crate::catalog::{
    CatalogProgress, InventoryMetadata, ItemMaterialRequirementSetIndices, ObjectiveDef,
    ObjectiveOwnerDef, ObjectiveOwnerKind, UnlockDefinition,
    collections::item_material_requirement_set_indices_from_data,
    icons::item_icon_container,
    localization::{LocalizedStringCache, resolve_string},
    package::{array_at, i64_at, relative_offset, u16_at, u32_at},
    progression::{
        ItemProgressionContext, PendingProgressionContext, add_objective_owner,
        attach_item_condition_contexts, item_objective_indices,
    },
};

const MAX_ITEM_SCAN_PROGRESS_UPDATES: usize = 200;
const ITEM_DESCRIPTION_OFFSET: usize = 0x98;

pub(in crate::catalog) struct ItemScanContext<'a> {
    pub manager: &'a PackageManager,
    pub root: &'a [u8],
    pub hashes: &'a [u64],
    pub definition_tags: &'a [u32],
    pub string_map: &'a [u8],
    pub string_count: usize,
    pub string_rows: usize,
    pub plug_set_table: &'a [u8],
    pub icon_containers_by_index: &'a [Option<u32>],
    pub inventory_buckets: &'a HashMap<u8, InventoryBucketDescriptor>,
    pub item_stat_definitions: &'a [ItemStatDefinition],
    pub stat_names: &'a [String],
    pub sandbox_perk_hashes: &'a [u64],
    pub ability_displays: &'a HashMap<u16, AbilityDisplayData>,
    pub collectible_item_paths: &'a HashMap<usize, Vec<Vec<String>>>,
    pub collectible_condition_contexts: &'a HashMap<usize, Vec<PendingProgressionContext>>,
    pub localized_tags: &'a [TagHash],
    pub localized_cache: &'a mut LocalizedStringCache,
    pub objectives: &'a mut [ObjectiveDef],
    pub unlock_flag_definitions: &'a mut [UnlockDefinition],
    pub unlock_value_definitions: &'a mut [UnlockDefinition],
}

// This opt-in package probe intentionally lives beside the scanner data it diagnoses.
#[allow(clippy::items_after_test_module)]
pub(in crate::catalog) struct ItemScan {
    pub items: Vec<ItemDef>,
    pub names: HashMap<u64, String>,
    pub type_names: HashMap<u64, String>,
    pub package_item_names: HashMap<u64, String>,
    pub package_item_type_names: HashMap<u64, String>,
    pub descriptions: HashMap<u64, String>,
    pub icon_containers: HashMap<u64, u32>,
    pub item_package_metadata: HashMap<u64, ItemPackageMetadata>,
    pub inventory_metadata: HashMap<u64, InventoryMetadata>,
    pub item_material_requirement_set_indices: HashMap<u64, ItemMaterialRequirementSetIndices>,
    pub diagnostics: ItemScanDiagnostics,
}

pub(in crate::catalog) struct ItemScanDiagnostics {
    unreadable_item_definitions: usize,
    short_item_definitions: usize,
    unreadable_item_strings: usize,
}

impl ItemScanDiagnostics {
    pub(in crate::catalog) fn append_to(self, errors: &mut Vec<String>) {
        for (label, count) in [
            (
                "Unreadable item definitions",
                self.unreadable_item_definitions,
            ),
            ("Truncated item definitions", self.short_item_definitions),
            (
                "Unreadable item string definitions",
                self.unreadable_item_strings,
            ),
        ] {
            if count > 0 {
                errors.push(format!("{label}: {count}"));
            }
        }
    }
}

pub(in crate::catalog) fn scan_items(
    context: ItemScanContext<'_>,
    report: &mut dyn FnMut(CatalogProgress),
) -> Result<ItemScan, String> {
    let ItemScanContext {
        manager,
        root,
        hashes,
        definition_tags,
        string_map,
        string_count,
        string_rows,
        plug_set_table,
        icon_containers_by_index,
        inventory_buckets,
        item_stat_definitions,
        stat_names,
        sandbox_perk_hashes,
        ability_displays,
        collectible_item_paths,
        collectible_condition_contexts,
        localized_tags,
        localized_cache,
        objectives,
        unlock_flag_definitions,
        unlock_value_definitions,
    } = context;
    let count = hashes.len();
    let mut item_package_metadata = hashes
        .iter()
        .copied()
        .zip(definition_tags.iter().copied())
        .enumerate()
        .filter_map(|(definition_index, (hash, definition_tag))| {
            u32::try_from(definition_index)
                .ok()
                .map(|definition_index| {
                    (
                        hash,
                        ItemPackageMetadata {
                            definition_index,
                            definition_tag,
                            plug_category_hash: None,
                            rarity: ItemRarity::Unknown,
                            power_cap: None,
                            damage_type: None,
                            investment_stats: Vec::new(),
                            intrinsic_perks: Vec::new(),
                        },
                    )
                })
        })
        .collect::<HashMap<_, _>>();
    let string_tags: HashMap<u64, TagHash> = (0..string_count)
        .filter_map(|index| {
            let base = string_rows + index * 24;
            Some((
                u64::from(u32_at(string_map, base).ok()?),
                TagHash(u32_at(string_map, base + 16).ok()?),
            ))
        })
        .collect();
    let mut names = HashMap::new();
    let mut type_names = HashMap::new();
    let mut package_item_names = HashMap::new();
    let mut package_item_type_names = HashMap::new();
    let mut descriptions = HashMap::new();
    let mut icon_containers = HashMap::new();
    let mut inventory_metadata = HashMap::new();
    let mut item_material_requirement_set_indices = HashMap::new();
    let mut items = Vec::new();
    let mut item_socket_lists = Vec::<(usize, u16)>::new();
    let mut plug_category_by_hash = HashMap::<u64, u32>::new();
    let mut plug_category_items = HashMap::<u32, Vec<u64>>::new();
    let mut unreadable_item_definitions = 0_usize;
    let mut short_item_definitions = 0_usize;
    let mut unreadable_item_strings = 0_usize;
    report(CatalogProgress {
        message: "Reading item definitions…",
        completed: 0,
        total: count,
    });
    let progress_stride = item_scan_progress_stride(count);
    for index in 0..count {
        if index % progress_stride == 0 {
            report(CatalogProgress {
                message: "Reading item definitions…",
                completed: index,
                total: count,
            });
        }
        let hash = hashes[index];
        let item_tag = TagHash(definition_tags[index]);
        let Ok(item) = manager.read_tag(item_tag) else {
            unreadable_item_definitions += 1;
            continue;
        };
        if item.len() < 188 {
            short_item_definitions += 1;
            continue;
        }
        let bucket_hash = super::inventory::bucket_hash(item[184]);
        if let Some(metadata) = item_package_metadata.get_mut(&hash) {
            metadata.rarity = ItemRarity::from_package_value(item[186]);
            metadata.power_cap = item_power_cap(&item);
            metadata.damage_type =
                bucket_hash.and_then(|bucket_hash| item_damage_type(&item, bucket_hash));
            metadata.investment_stats = item_investment_stats(&item, item_stat_definitions);
            metadata.intrinsic_perks = item_intrinsic_perk_hashes(&item, sandbox_perk_hashes);
        }
        if let Ok(category) = u32_at(&item, 392)
            && category != 0
            && category != u32::MAX
        {
            if let Some(metadata) = item_package_metadata.get_mut(&hash) {
                metadata.plug_category_hash = Some(u64::from(category));
            }
            plug_category_by_hash.insert(hash, category);
            plug_category_items.entry(category).or_default().push(hash);
        }
        let metadata = item_inventory_metadata(&item, inventory_buckets);
        if let Some(metadata) = metadata {
            inventory_metadata.insert(hash, metadata);
        }
        if let Some(indices) = item_material_requirement_set_indices_from_data(&item) {
            item_material_requirement_set_indices.insert(hash, indices);
        }
        let objective_indices = item_objective_indices(&item, objectives.len());
        let objective_paths = collectible_item_paths
            .get(&index)
            .map_or(&[][..], Vec::as_slice);
        attach_item_condition_contexts(
            &item,
            ItemProgressionContext {
                hash,
                name: "",
                type_name: "",
                paths: objective_paths,
            },
            collectible_condition_contexts
                .get(&index)
                .map_or(&[], Vec::as_slice),
            unlock_flag_definitions,
            unlock_value_definitions,
        );
        let string_row = string_rows + index * 24;
        let string_tag = if u32_at(string_map, string_row).ok().map(u64::from) == Some(hash) {
            TagHash(u32_at(string_map, string_row + 16)?)
        } else {
            let Some(&tag) = string_tags.get(&hash) else {
                attach_item_objective_owners(
                    objectives,
                    &objective_indices,
                    hash,
                    "",
                    "",
                    metadata,
                    objective_paths,
                );
                continue;
            };
            tag
        };
        let Ok(string_thing) = manager.read_tag(string_tag) else {
            unreadable_item_strings += 1;
            attach_item_objective_owners(
                objectives,
                &objective_indices,
                hash,
                "",
                "",
                metadata,
                objective_paths,
            );
            continue;
        };
        let mut name = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &string_thing,
            0x84,
        )
        .unwrap_or_default();
        if !name.trim().is_empty() {
            package_item_names
                .entry(hash)
                .or_insert_with(|| name.clone());
        }
        let mut derived_masterwork_name = false;
        if let Some(label) = masterwork_label(&item, stat_names, &name) {
            name = label;
            derived_masterwork_name = true;
        }
        let mut type_name = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &string_thing,
            0x90,
        )
        .unwrap_or_default();
        if !type_name.trim().is_empty() {
            package_item_type_names
                .entry(hash)
                .or_insert_with(|| type_name.clone());
        }
        if let Some(description) = resolve_string(
            manager,
            localized_tags,
            localized_cache,
            &string_thing,
            ITEM_DESCRIPTION_OFFSET,
        )
        .filter(|description| !description.trim().is_empty())
        {
            descriptions.insert(hash, description);
        }
        if let Some(container) = item_icon_container(&string_thing, icon_containers_by_index) {
            icon_containers.insert(hash, container);
        }
        if name.trim().is_empty() {
            let Some((derived_name, derived_type_name)) = stat_allocation_labels(&item, stat_names)
            else {
                attach_item_objective_owners(
                    objectives,
                    &objective_indices,
                    hash,
                    "",
                    &type_name,
                    metadata,
                    objective_paths,
                );
                continue;
            };
            name = derived_name;
            derived_type_name.clone_into(&mut type_name);
        }
        if !type_name.trim().is_empty() {
            type_names.entry(hash).or_insert_with(|| type_name.clone());
        }
        if derived_masterwork_name {
            names.insert(hash, name.clone());
        } else {
            names.entry(hash).or_insert_with(|| name.clone());
        }
        attach_item_condition_contexts(
            &item,
            ItemProgressionContext {
                hash,
                name: &name,
                type_name: &type_name,
                paths: collectible_item_paths
                    .get(&index)
                    .map_or(&[], Vec::as_slice),
            },
            collectible_condition_contexts
                .get(&index)
                .map_or(&[], Vec::as_slice),
            unlock_flag_definitions,
            unlock_value_definitions,
        );
        attach_item_objective_owners(
            objectives,
            &objective_indices,
            hash,
            &name,
            &type_name,
            metadata,
            objective_paths,
        );
        let Some(bucket_hash) = bucket_hash else {
            continue;
        };
        if let Ok(relative) = i64_at(&item, 128)
            && relative != 0
        {
            let Ok(block) = relative_offset(128, 0, relative) else {
                continue;
            };
            if let Ok(list_index) = u16_at(&item, block) {
                item_socket_lists.push((items.len(), list_index));
            }
        }
        let mut default_plugs = Vec::new();
        let mut sockets = Vec::new();
        if let Ok(relative) = i64_at(&item, 104)
            && relative != 0
        {
            let Ok(block) = relative_offset(104, 0, relative) else {
                continue;
            };
            if let Ok((socket_count, socket_rows, class)) = array_at(&item, block)
                && class == ORDINARY_SOCKET_CLASS
                && socket_count <= 12
            {
                for lane in 0..socket_count {
                    let base = socket_rows + lane * 80;
                    let socket_type = u16_at(&item, base)?;
                    let plug_index = u16_at(&item, base + 2)?;
                    let plug = (plug_index != u16::MAX)
                        .then(|| hashes.get(plug_index as usize).copied())
                        .flatten();
                    default_plugs.push(plug.map(format_hash_hex));
                    let mut allowed = socket_allowed_hashes(&item, base, hashes, plug_set_table);
                    if let Some(hash) = plug {
                        allowed.push(hash);
                    }
                    allowed.sort_unstable();
                    allowed.dedup();
                    sockets.push(SocketDef {
                        socket_type,
                        label: String::new(),
                        pool: 0,
                        allowed,
                    });
                }
            }
        }
        let plug_hashes = default_plugs
            .iter()
            .flatten()
            .filter_map(|value| parse_hash_hex(value))
            .chain(
                sockets
                    .iter()
                    .flat_map(|socket| socket.allowed.iter().copied()),
            )
            .collect::<Vec<_>>();
        for plug in plug_hashes {
            if string_tags.contains_key(&plug) {
                let Some(&string_tag) = string_tags.get(&plug) else {
                    continue;
                };
                if let Ok(string_definition) = manager.read_tag(string_tag)
                    && let Some(name) = resolve_string(
                        manager,
                        localized_tags,
                        localized_cache,
                        &string_definition,
                        0x84,
                    )
                {
                    package_item_names
                        .entry(plug)
                        .or_insert_with(|| name.clone());
                    names.entry(plug).or_insert(name);
                }
            }
        }
        items.push(ItemDef {
            hash,
            name,
            type_name,
            bucket_hash,
            class_type: class_items::class_type(hash).unwrap_or(3),
            default_plugs,
            sockets,
            abilities: AbilityOptions::default(),
        });
    }
    report(CatalogProgress {
        message: "Reading item definitions…",
        completed: count,
        total: count,
    });
    report(CatalogProgress::stage("Building socket choices…"));
    build_socket_choices(
        &mut items,
        &plug_category_by_hash,
        &plug_category_items,
        &names,
        &mut type_names,
    );
    report(CatalogProgress::stage("Building subclass choices…"));
    build_subclass_choices(
        manager,
        root,
        ability_displays,
        item_socket_lists,
        &mut items,
    )?;
    Ok(ItemScan {
        items,
        names,
        type_names,
        package_item_names,
        package_item_type_names,
        descriptions,
        icon_containers,
        item_package_metadata,
        inventory_metadata,
        item_material_requirement_set_indices,
        diagnostics: ItemScanDiagnostics {
            unreadable_item_definitions,
            short_item_definitions,
            unreadable_item_strings,
        },
    })
}

pub(in crate::catalog) fn item_scan_progress_stride(item_count: usize) -> usize {
    item_count.div_ceil(MAX_ITEM_SCAN_PROGRESS_UPDATES).max(64)
}

pub(in crate::catalog) fn attach_item_objective_owners(
    objectives: &mut [ObjectiveDef],
    objective_indices: &[usize],
    hash: u64,
    name: &str,
    type_name: &str,
    metadata: Option<InventoryMetadata>,
    paths: &[Vec<String>],
) {
    let owner_type = if type_name.trim().is_empty() {
        metadata.map_or_else(String::new, InventoryMetadata::bucket_label)
    } else {
        type_name.to_owned()
    };
    for &objective_index in objective_indices {
        add_objective_owner(
            objectives,
            objective_index,
            ObjectiveOwnerDef {
                hash,
                kind: ObjectiveOwnerKind::InventoryItem,
                name: name.to_owned(),
                type_name: owner_type.clone(),
                description: String::new(),
                traits: Vec::new(),
                paths: paths.to_vec(),
            },
        );
    }
}
