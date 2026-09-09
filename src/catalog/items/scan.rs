//! Inventory-item table scanning and item-domain catalog assembly.

use std::collections::HashMap;

use tiger_pkg::{PackageManager, TagHash};

use crate::{
    class_items,
    hash::{format_hash_hex, parse_hash_hex},
    investment_localization::{LocalizedStringCache, resolve_string},
    investment_schema::{
        ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET as EQUIPMENT_BLOCK_OFFSET,
        ITEM_EQUIPMENT_SLOT_OFFSET as EQUIPMENT_SLOT_OFFSET, ITEM_LINKED_PLUG_BLOCK_CLASS,
        ITEM_LINKED_PLUG_INDEX_OFFSET, ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET,
        ITEM_ORDINARY_SOCKET_POINTER_OFFSET, ITEM_ORDINARY_SOCKET_ROW_SIZE,
        ITEM_PLUG_BLOCK_CATEGORY_OFFSET, ITEM_PLUG_BLOCK_CLASS, ITEM_PLUG_BLOCK_ROLL_SET_OFFSET,
        ITEM_PLUG_BLOCK_SEARCH_END, ITEM_PLUG_BLOCK_SEARCH_START,
        ITEM_PLUG_CATEGORY_FALLBACK_OFFSET, ITEM_RARITY_OFFSET,
        ITEM_SOCKET_ENTRY_LIST_BLOCK_POINTER_OFFSET as SOCKET_ENTRY_LIST_BLOCK_OFFSET,
        ITEM_SOCKET_ENTRY_LIST_BLOCK_SIZE as SOCKET_ENTRY_LIST_BLOCK_SIZE,
        ITEM_STRING_DESCRIPTION_REFERENCE_OFFSET as ITEM_DESCRIPTION_OFFSET, ITEM_TRAIT_ROW_CLASS,
        ITEM_TRAIT_ROW_SIZE, ITEM_TRAITS_DESCRIPTOR_OFFSET,
        ITEM_TRANSLATION_ART_ROW_CLASS as ART_ROW_CLASS,
        ITEM_TRANSLATION_ART_ROW_SIZE as ART_ROW_STRIDE,
        ITEM_TRANSLATION_ART_VARIANT_OFFSET as ART_ROW_VALUE_OFFSET,
        ITEM_TRANSLATION_BLOCK_POINTER_OFFSET as ART_BLOCK_OFFSET,
        ITEM_TRANSLATION_DYE_DESCRIPTOR_OFFSETS,
        ITEM_TRANSLATION_DYE_ROW_CLASS as MATERIAL_OVERRIDE_CLASS,
        ITEM_TRANSLATION_DYE_ROW_SIZE as MATERIAL_ROW_STRIDE,
        ITEM_TRANSLATION_DYE_VARIANT_OFFSET as MATERIAL_ROW_VALUE_OFFSET,
        ITEM_TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET as WEAPON_PATTERN_INDEX_OFFSET,
    },
    package_payload::{array_at, i64_at, relative_offset, u16_at, u32_at},
};

use super::{
    AbilityOptions, ItemArtArrangement, ItemDef, ItemPackageMetadata, ItemRarity,
    ItemRenderOverride, ItemStatDefinition, ItemWeaponInventorySlot, SocketDef,
    abilities::{AbilityDisplayData, build_subclass_choices},
    inventory::{InventoryBucketDescriptor, item_inventory_metadata},
    investment::{
        item_investment_stats, item_stat_group_index, masterwork_label, stat_allocation_labels,
    },
    item_damage_profile, item_weapon_ammo_type,
    perks::item_sandbox_perks,
    quality::item_power_cap_groups,
    resolve_default_plug_damage_profile,
    sockets::{ORDINARY_SOCKET_CLASS, build_socket_choices, socket_package_sources},
};
use crate::catalog::{
    CatalogProgress, InventoryMetadata, ItemMaterialRequirementSetIndices, ObjectiveDef,
    ObjectiveOwnerDef, ObjectiveOwnerKind, UnlockDefinition,
    collections::item_material_requirement_set_indices_from_data,
    icons::item_icon_container,
    progression::{
        ItemProgressionContext, PendingProgressionContext, add_objective_owner,
        attach_item_condition_contexts, item_objective_indices,
    },
};

const MAX_ITEM_SCAN_PROGRESS_UPDATES: usize = 200;
const EQUIPMENT_SLOT_COUNT: u16 = 20;
const EQUIPMENT_SLOT_SENTINEL: u16 = u16::MAX;
const MATERIAL_STAGE_OFFSETS: [(u8, usize); 3] = [
    (0, ITEM_TRANSLATION_DYE_DESCRIPTOR_OFFSETS[0]),
    (1, ITEM_TRANSLATION_DYE_DESCRIPTOR_OFFSETS[1]),
    (2, ITEM_TRANSLATION_DYE_DESCRIPTOR_OFFSETS[2]),
];
const RENDER_OVERRIDE_CAPACITY: usize = 32;

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
    pub sandbox_perk_catalog: Option<&'a [bool]>,
    pub trait_definition_count: usize,
    pub ability_displays: &'a HashMap<u16, AbilityDisplayData>,
    pub collectible_item_paths: &'a HashMap<usize, Vec<Vec<String>>>,
    pub collectible_condition_contexts: &'a HashMap<usize, Vec<PendingProgressionContext>>,
    pub localized_tags: &'a [TagHash],
    pub localized_cache: &'a mut LocalizedStringCache,
    pub objectives: &'a mut [ObjectiveDef],
    pub unlock_flag_definitions: &'a mut [UnlockDefinition],
    pub unlock_value_definitions: &'a mut [UnlockDefinition],
}

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
    malformed_investment_stats: usize,
}

struct ItemMetadataSources<'a> {
    hashes: &'a [u64],
    inventory_buckets: &'a HashMap<u8, InventoryBucketDescriptor>,
    item_stat_definitions: &'a [ItemStatDefinition],
    sandbox_perk_catalog: Option<&'a [bool]>,
    trait_definition_count: usize,
}

struct ItemMetadataOutputs<'a> {
    package: &'a mut HashMap<u64, ItemPackageMetadata>,
    inventory: &'a mut HashMap<u64, InventoryMetadata>,
    material_sets: &'a mut HashMap<u64, ItemMaterialRequirementSetIndices>,
    plug_categories: &'a mut HashMap<u64, u32>,
    plug_category_items: &'a mut HashMap<u32, Vec<u64>>,
    malformed_investment_stats: &'a mut usize,
}

fn record_item_metadata(
    item: &[u8],
    hash: u64,
    bucket_hash: Option<u64>,
    sources: &ItemMetadataSources<'_>,
    outputs: ItemMetadataOutputs<'_>,
) -> Option<InventoryMetadata> {
    if let Some(metadata) = outputs.package.get_mut(&hash) {
        metadata.definition_size = u32::try_from(item.len()).ok();
        populate_native_item_metadata(item, sources.hashes, metadata);
        metadata.rarity = ItemRarity::from_package_value(item[ITEM_RARITY_OFFSET]);
        metadata.power_cap_groups = item_power_cap_groups(item);
        metadata.damage_profile = item_damage_profile(item, bucket_hash.unwrap_or_default());
        metadata.damage_type = metadata.damage_profile.damage_type();
        if let Some(bucket_hash) = bucket_hash {
            metadata.weapon_inventory_slot = ItemWeaponInventorySlot::from_bucket_hash(bucket_hash);
        }
        match item_investment_stats(item, sources.item_stat_definitions) {
            Ok(stats) => metadata.investment_stats = stats,
            Err(()) => {
                metadata.investment_stats.clear();
                *outputs.malformed_investment_stats += 1;
            }
        }
        metadata.sandbox_perks = item_sandbox_perks(item, sources.sandbox_perk_catalog);
        metadata.trait_indices = item_trait_indices(item, sources.trait_definition_count);
    }
    if let Some(category) = outputs
        .package
        .get(&hash)
        .and_then(|metadata| metadata.plug_category_hash)
        .and_then(|category| u32::try_from(category).ok())
    {
        outputs.plug_categories.insert(hash, category);
        outputs
            .plug_category_items
            .entry(category)
            .or_default()
            .push(hash);
    }
    let metadata = item_inventory_metadata(item, sources.inventory_buckets);
    if let Some(metadata) = metadata {
        outputs.inventory.insert(hash, metadata);
    }
    if let Some(indices) = item_material_requirement_set_indices_from_data(item) {
        outputs.material_sets.insert(hash, indices);
    }
    metadata
}

fn item_trait_indices(item: &[u8], trait_definition_count: usize) -> Vec<u16> {
    if item
        .get(ITEM_TRAITS_DESCRIPTOR_OFFSET..ITEM_TRAITS_DESCRIPTOR_OFFSET + 16)
        .is_some_and(|descriptor| descriptor == [0; 16])
    {
        return Vec::new();
    }
    let Ok((count, rows, class)) = array_at(item, ITEM_TRAITS_DESCRIPTOR_OFFSET) else {
        return Vec::new();
    };
    if class != ITEM_TRAIT_ROW_CLASS || count > trait_definition_count {
        return Vec::new();
    }
    (0..count)
        .map(|index| u16_at(item, rows + index * ITEM_TRAIT_ROW_SIZE))
        .collect::<Result<Vec<_>, _>>()
        .ok()
        .filter(|indices| {
            indices
                .iter()
                .all(|index| usize::from(*index) < trait_definition_count)
        })
        .unwrap_or_default()
}

fn report_item_scan_progress(
    index: usize,
    count: usize,
    progress_stride: usize,
    report: &mut dyn FnMut(CatalogProgress),
) {
    if index % progress_stride == 0 {
        report(CatalogProgress {
            message: "Reading item definitions…",
            completed: index,
            total: count,
        });
    }
}

fn item_string_tag(
    string_map: &[u8],
    string_rows: usize,
    index: usize,
    hash: u64,
    string_tags: &HashMap<u64, TagHash>,
) -> Result<Option<TagHash>, String> {
    let row = string_rows + index * 24;
    if u32_at(string_map, row).ok().map(u64::from) == Some(hash) {
        return u32_at(string_map, row + 16).map(TagHash).map(Some);
    }

    Ok(string_tags.get(&hash).copied())
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
            (
                "Malformed item investment-stat definitions",
                self.malformed_investment_stats,
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
        sandbox_perk_catalog,
        trait_definition_count,
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
                            definition_size: None,
                            string_definition_tag: None,
                            stat_group_index: None,
                            icon_container_tag: None,
                            plug_category_hash: None,
                            equipment_slot: None,
                            socket_entry_list_index: None,
                            roll_set_index: None,
                            linked_plug_index: None,
                            linked_plug_hash: None,
                            weapon_pattern_index: None,
                            weapon_translation_group: None,
                            art_arrangement_indices: [None; 4],
                            art_arrangements: Vec::new(),
                            render_overrides: Vec::new(),
                            translation_dye_rows: Default::default(),
                            rarity: ItemRarity::Unknown,
                            power_cap: None,
                            power_cap_groups: Vec::new(),
                            damage_type: None,
                            damage_profile: Default::default(),
                            weapon_inventory_slot: None,
                            weapon_ammo_type: None,
                            investment_stats: Vec::new(),
                            sandbox_perks: Vec::new(),
                            trait_indices: Vec::new(),
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
    let mut malformed_investment_stats = 0_usize;
    let item_metadata_sources = ItemMetadataSources {
        hashes,
        inventory_buckets,
        item_stat_definitions,
        sandbox_perk_catalog,
        trait_definition_count,
    };
    report(CatalogProgress {
        message: "Reading item definitions…",
        completed: 0,
        total: count,
    });
    let progress_stride = item_scan_progress_stride(count);
    for index in 0..count {
        report_item_scan_progress(index, count, progress_stride, report);
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
        let bucket_hash = super::inventory::item_bucket_hash(hash, item[184]);
        let metadata = record_item_metadata(
            &item,
            hash,
            bucket_hash,
            &item_metadata_sources,
            ItemMetadataOutputs {
                package: &mut item_package_metadata,
                inventory: &mut inventory_metadata,
                material_sets: &mut item_material_requirement_set_indices,
                plug_categories: &mut plug_category_by_hash,
                plug_category_items: &mut plug_category_items,
                malformed_investment_stats: &mut malformed_investment_stats,
            },
        );
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
        let Some(string_tag) = item_string_tag(string_map, string_rows, index, hash, &string_tags)?
        else {
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
        if let Some(metadata) = item_package_metadata.get_mut(&hash) {
            metadata.string_definition_tag = Some(string_tag.0);
        }
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
        if let Some(metadata) = item_package_metadata.get_mut(&hash) {
            metadata.stat_group_index = item_stat_group_index(&string_thing);
            metadata.weapon_ammo_type = item_weapon_ammo_type(&string_thing);
        }
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
            if let Some(metadata) = item_package_metadata.get_mut(&hash) {
                metadata.icon_container_tag = Some(container);
            }
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
        if let Some(list_index) = item_package_metadata
            .get(&hash)
            .and_then(|metadata| metadata.socket_entry_list_index)
        {
            item_socket_lists.push((items.len(), list_index));
        }
        let Some(decoded_sockets) = decode_item_sockets(&item, hashes, plug_set_table)? else {
            continue;
        };
        load_socket_plug_names(
            manager,
            localized_tags,
            localized_cache,
            &string_tags,
            &decoded_sockets,
            &mut package_item_names,
            &mut names,
        );
        items.push(ItemDef {
            hash,
            name,
            type_name,
            bucket_hash,
            class_type: class_items::class_type(hash).unwrap_or(3),
            default_plugs: decoded_sockets.default_plugs,
            sockets: decoded_sockets.sockets,
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
    refine_weapon_damage_profiles(&items, &mut item_package_metadata);
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
            malformed_investment_stats,
        },
    })
}

fn refine_weapon_damage_profiles(
    items: &[ItemDef],
    metadata: &mut HashMap<u64, ItemPackageMetadata>,
) {
    for item in items
        .iter()
        .filter(|item| super::is_authorable_weapon_item(item))
    {
        let Some(base) = metadata
            .get(&item.hash)
            .map(|metadata| metadata.damage_profile)
        else {
            continue;
        };
        let plug_profiles = item
            .default_plugs
            .iter()
            .flatten()
            .filter_map(|hash| parse_hash_hex(hash))
            .filter_map(|hash| metadata.get(&hash))
            .map(|metadata| metadata.damage_profile)
            .collect::<Vec<_>>();
        let mut profile = resolve_default_plug_damage_profile(base, plug_profiles);
        // Slot placement does not imply elemental damage. Keep unresolved native
        // elemental sockets ambiguous; do not invent a carrier from the bucket.
        if profile == (super::ItemDamageProfile::PlugOrEmptyAmbiguous { damage_type: None })
            && !item.sockets.iter().any(|socket| socket.socket_type == 68)
        {
            profile = super::ItemDamageProfile::KineticEmpty;
        }
        if let Some(metadata) = metadata.get_mut(&item.hash) {
            metadata.damage_profile = profile;
            metadata.damage_type = profile.damage_type();
        }
    }
}

#[cfg(test)]
mod independent_damage_tests {
    use super::*;
    use crate::catalog::items::{ItemDamageProfile, SocketDef};

    #[test]
    fn empty_weapon_damage_requires_checking_the_carrier_not_the_bucket() {
        for bucket in [1_498_876_634, 2_465_295_065, 953_998_645] {
            for elemental_socket in [false, true] {
                let item = ItemDef {
                    hash: 1,
                    name: "Trial".to_owned(),
                    type_name: "Auto Rifle".to_owned(),
                    bucket_hash: bucket,
                    class_type: 3,
                    default_plugs: Vec::new(),
                    sockets: if elemental_socket {
                        vec![SocketDef {
                            socket_type: 68,
                            label: String::new(),
                            pool: 0,
                            allowed: Vec::new(),
                            sources: Vec::new(),
                        }]
                    } else {
                        Vec::new()
                    },
                    abilities: Default::default(),
                };
                let ambiguous = ItemDamageProfile::PlugOrEmptyAmbiguous { damage_type: None };
                let mut metadata = HashMap::from([(
                    1,
                    ItemPackageMetadata {
                        damage_profile: ambiguous,
                        ..Default::default()
                    },
                )]);
                refine_weapon_damage_profiles(&[item], &mut metadata);
                assert_eq!(
                    metadata[&1].damage_profile,
                    if elemental_socket {
                        ambiguous
                    } else {
                        ItemDamageProfile::KineticEmpty
                    }
                );
            }
        }
    }
}

struct DecodedItemSockets {
    default_plugs: Vec<Option<String>>,
    sockets: Vec<SocketDef>,
}

fn decode_item_sockets(
    item: &[u8],
    hashes: &[u64],
    plug_set_table: &[u8],
) -> Result<Option<DecodedItemSockets>, String> {
    let mut decoded = DecodedItemSockets {
        default_plugs: Vec::new(),
        sockets: Vec::new(),
    };
    let Ok(relative) = i64_at(item, ITEM_ORDINARY_SOCKET_POINTER_OFFSET) else {
        return Ok(Some(decoded));
    };
    if relative == 0 {
        return Ok(Some(decoded));
    }
    let Ok(block) = relative_offset(ITEM_ORDINARY_SOCKET_POINTER_OFFSET, 0, relative) else {
        return Ok(None);
    };
    let Ok((socket_count, socket_rows, class)) = array_at(item, block) else {
        return Ok(Some(decoded));
    };
    if class != ORDINARY_SOCKET_CLASS || socket_count > 12 {
        return Ok(Some(decoded));
    }

    for lane in 0..socket_count {
        let base = socket_rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE;
        let socket_type = u16_at(item, base)?;
        let plug_index = u16_at(item, base + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET)?;
        let plug = (plug_index != u16::MAX)
            .then(|| hashes.get(plug_index as usize).copied())
            .flatten();
        decoded.default_plugs.push(plug.map(format_hash_hex));
        let sources = socket_package_sources(item, base, hashes, plug_set_table);
        let mut allowed = sources
            .iter()
            .flat_map(|source| source.allowed.iter().copied())
            .collect::<Vec<_>>();
        if let Some(hash) = plug {
            allowed.push(hash);
        }
        allowed.sort_unstable();
        allowed.dedup();
        decoded.sockets.push(SocketDef {
            socket_type,
            label: String::new(),
            pool: 0,
            allowed,
            sources,
        });
    }
    Ok(Some(decoded))
}

fn load_socket_plug_names(
    manager: &PackageManager,
    localized_tags: &[TagHash],
    localized_cache: &mut LocalizedStringCache,
    string_tags: &HashMap<u64, TagHash>,
    decoded: &DecodedItemSockets,
    package_item_names: &mut HashMap<u64, String>,
    names: &mut HashMap<u64, String>,
) {
    let plug_hashes = decoded
        .default_plugs
        .iter()
        .flatten()
        .filter_map(|value| parse_hash_hex(value))
        .chain(
            decoded
                .sockets
                .iter()
                .flat_map(|socket| socket.allowed.iter().copied()),
        )
        .collect::<Vec<_>>();
    for plug in plug_hashes {
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

fn populate_native_item_metadata(
    item: &[u8],
    item_hashes: &[u64],
    metadata: &mut ItemPackageMetadata,
) {
    metadata.equipment_slot = item_equipment_slot(item);
    metadata.socket_entry_list_index = item_socket_entry_list_index(item);

    let plug_block = find_record(
        item,
        ITEM_PLUG_BLOCK_CLASS,
        ITEM_PLUG_BLOCK_SEARCH_START,
        ITEM_PLUG_BLOCK_SEARCH_END,
    );
    let category = plug_block
        .and_then(|block| u32_at(item, block + ITEM_PLUG_BLOCK_CATEGORY_OFFSET).ok())
        .or_else(|| u32_at(item, ITEM_PLUG_CATEGORY_FALLBACK_OFFSET).ok());
    metadata.plug_category_hash = category
        .filter(|category| !matches!(*category, 0 | u32::MAX))
        .map(u64::from);
    metadata.roll_set_index =
        plug_block.and_then(|block| u16_at(item, block + ITEM_PLUG_BLOCK_ROLL_SET_OFFSET).ok());

    metadata.linked_plug_index = find_record(item, ITEM_LINKED_PLUG_BLOCK_CLASS, 0, item.len())
        .and_then(|block| u16_at(item, block + ITEM_LINKED_PLUG_INDEX_OFFSET).ok())
        .filter(|index| *index != u16::MAX);
    metadata.linked_plug_hash = metadata
        .linked_plug_index
        .and_then(|index| item_hashes.get(usize::from(index)).copied());

    populate_item_appearance_metadata(item, metadata);
}

fn item_equipment_slot(item: &[u8]) -> Option<u8> {
    let block = resolve_item_block(item, EQUIPMENT_BLOCK_OFFSET)?;
    let slot_offset = block.checked_add(EQUIPMENT_SLOT_OFFSET)?;
    // Shadowkeep stores the equipment slot in the low u16 and an adjacent 0xFFFF sentinel in the
    // high u16. Interpreting the pair as i32 turns every valid slot into a negative number.
    let slot = u16_at(item, slot_offset).ok()?;
    let sentinel = u16_at(item, slot_offset.checked_add(2)?).ok()?;
    if slot >= EQUIPMENT_SLOT_COUNT || sentinel != EQUIPMENT_SLOT_SENTINEL {
        return None;
    }
    u8::try_from(slot).ok()
}

fn item_socket_entry_list_index(item: &[u8]) -> Option<u16> {
    let block = resolve_item_block(item, SOCKET_ENTRY_LIST_BLOCK_OFFSET)?;
    let end = block.checked_add(SOCKET_ENTRY_LIST_BLOCK_SIZE)?;
    if end > item.len() {
        return None;
    }
    u16_at(item, block).ok()
}

fn populate_item_appearance_metadata(item: &[u8], metadata: &mut ItemPackageMetadata) {
    metadata.weapon_pattern_index = None;
    metadata.art_arrangement_indices = [None; 4];
    metadata.art_arrangements.clear();
    metadata.render_overrides.clear();
    for rows in &mut metadata.translation_dye_rows {
        rows.clear();
    }

    let Some(art) = resolve_item_block(item, ART_BLOCK_OFFSET) else {
        return;
    };
    metadata.weapon_pattern_index = art
        .checked_add(WEAPON_PATTERN_INDEX_OFFSET)
        .and_then(|offset| u16_at(item, offset).ok())
        .filter(|index| *index != u16::MAX);

    if let Some((count, rows)) = bounded_array(item, art, ART_ROW_CLASS, ART_ROW_STRIDE) {
        for index in 0..count {
            let row = rows + index * ART_ROW_STRIDE;
            let character_class = item[row] as i8;
            let Ok(arrangement) = u16_at(item, row + ART_ROW_VALUE_OFFSET) else {
                break;
            };
            metadata.art_arrangements.push(ItemArtArrangement {
                character_class,
                arrangement,
            });
            let slot = if character_class == -1 {
                Some(0)
            } else {
                usize::try_from(character_class)
                    .ok()
                    .and_then(|class| class.checked_add(1))
            };
            if let Some(slot) = slot.filter(|slot| *slot < 4)
                && arrangement != u16::MAX
                && metadata.art_arrangement_indices[slot].is_none()
            {
                metadata.art_arrangement_indices[slot] = Some(arrangement);
            }
        }
    }

    for (stage, offset) in MATERIAL_STAGE_OFFSETS {
        let Some(descriptor) = art.checked_add(offset) else {
            continue;
        };
        let Some((count, rows)) = bounded_array(
            item,
            descriptor,
            MATERIAL_OVERRIDE_CLASS,
            MATERIAL_ROW_STRIDE,
        ) else {
            continue;
        };
        for index in 0..count {
            if metadata.render_overrides.len() >= RENDER_OVERRIDE_CAPACITY {
                return;
            }
            let row = rows + index * MATERIAL_ROW_STRIDE;
            let key = item[row] as i8;
            let Ok(value) = u16_at(item, row + MATERIAL_ROW_VALUE_OFFSET) else {
                break;
            };
            metadata.translation_dye_rows[usize::from(stage)].push(ItemRenderOverride {
                stage,
                key,
                value,
            });
            if key == -1 {
                continue;
            }
            metadata
                .render_overrides
                .push(ItemRenderOverride { stage, key, value });
        }
    }
}

fn resolve_item_block(item: &[u8], member: usize) -> Option<usize> {
    let relative = i64_at(item, member)
        .ok()
        .filter(|relative| *relative != 0)?;
    relative_offset(member, 0, relative)
        .ok()
        .filter(|block| *block < item.len())
}

fn bounded_array(
    item: &[u8],
    descriptor: usize,
    expected_class: u32,
    stride: usize,
) -> Option<(usize, usize)> {
    let (count, rows, class) = array_at(item, descriptor).ok()?;
    if class != expected_class || rows > item.len() || stride == 0 {
        return None;
    }
    Some((count.min((item.len() - rows) / stride), rows))
}

fn find_record(item: &[u8], class: u32, start: usize, end: usize) -> Option<usize> {
    let limit = end.min(item.len());
    (start..limit.saturating_sub(3))
        .step_by(4)
        .find(|offset| u32_at(item, *offset).ok() == Some(class))
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

#[cfg(test)]
mod native_metadata_tests {
    use super::*;

    #[test]
    fn reads_native_equipment_socket_and_plug_context() {
        let mut item = vec![0_u8; 700];
        write_i64(&mut item, EQUIPMENT_BLOCK_OFFSET, 384);
        write_u16(&mut item, 424, 8);
        write_u16(&mut item, 426, EQUIPMENT_SLOT_SENTINEL);
        write_i64(&mut item, SOCKET_ENTRY_LIST_BLOCK_OFFSET, 384);
        write_u16(&mut item, 512, 37);
        write_u32(&mut item, 300, ITEM_PLUG_BLOCK_CLASS);
        write_u32(&mut item, 304, 0x1234_5678);
        write_u16(&mut item, 338, 5);
        write_u32(&mut item, 560, ITEM_LINKED_PLUG_BLOCK_CLASS);
        write_u16(&mut item, 572, 3);

        let mut metadata = ItemPackageMetadata::default();
        populate_native_item_metadata(&item, &[10, 20, 30, 40], &mut metadata);

        assert_eq!(metadata.equipment_slot, Some(8));
        assert_eq!(metadata.socket_entry_list_index, Some(37));
        assert_eq!(metadata.plug_category_hash, Some(0x1234_5678));
        assert_eq!(metadata.roll_set_index, Some(5));
        assert_eq!(metadata.linked_plug_index, Some(3));
        assert_eq!(metadata.linked_plug_hash, Some(40));
    }

    #[test]
    fn reads_class_art_and_nonempty_material_overrides() {
        let mut item = vec![0_u8; 560];
        write_i64(&mut item, ART_BLOCK_OFFSET, 104);
        write_u16(&mut item, 328, 9);

        write_array_descriptor(&mut item, 240, 2, 400, ART_ROW_CLASS);
        item[416] = u8::MAX;
        write_u16(&mut item, 418, 11);
        item[420] = 0;
        write_u16(&mut item, 422, 22);

        write_array_descriptor(&mut item, 280, 2, 450, MATERIAL_OVERRIDE_CLASS);
        item[466] = u8::MAX;
        item[470] = 3;
        write_u16(&mut item, 472, 77);

        let mut metadata = ItemPackageMetadata::default();
        populate_item_appearance_metadata(&item, &mut metadata);

        assert_eq!(metadata.weapon_pattern_index, Some(9));
        assert_eq!(
            metadata.art_arrangement_indices,
            [Some(11), Some(22), None, None]
        );
        assert_eq!(
            metadata.render_overrides,
            vec![ItemRenderOverride {
                stage: 0,
                key: 3,
                value: 77,
            }]
        );
    }

    #[test]
    fn rejects_out_of_range_relative_blocks_and_equipment_slots() {
        let mut item = vec![0_u8; 188];
        write_i64(&mut item, EQUIPMENT_BLOCK_OFFSET, 100);
        write_u16(&mut item, 140, EQUIPMENT_SLOT_COUNT);
        write_u16(&mut item, 142, EQUIPMENT_SLOT_SENTINEL);
        write_i64(&mut item, SOCKET_ENTRY_LIST_BLOCK_OFFSET, i64::MAX);

        assert_eq!(item_equipment_slot(&item), None);
        assert_eq!(item_socket_entry_list_index(&item), None);

        write_u16(&mut item, 140, 8);
        write_u16(&mut item, 142, 0);
        assert_eq!(item_equipment_slot(&item), None);

        write_u16(&mut item, 142, EQUIPMENT_SLOT_SENTINEL);
        assert_eq!(item_equipment_slot(&item), Some(8));
    }

    fn write_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_i64(data: &mut [u8], offset: usize, value: i64) {
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u64(data: &mut [u8], offset: usize, value: u64) {
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn write_array_descriptor(
        data: &mut [u8],
        descriptor: usize,
        count: u64,
        header: usize,
        class: u32,
    ) {
        write_u64(data, descriptor, count);
        let pointer = descriptor + 8;
        write_i64(data, pointer, i64::try_from(header - pointer).unwrap());
        write_u64(data, header, count);
        write_u32(data, header + 8, class);
    }
}
