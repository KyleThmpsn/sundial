//! Inventory-item table scanning and item-domain catalog assembly.

use std::collections::HashMap;

use tiger_pkg::{PackageManager, TagHash};

use crate::{
    class_items,
    hash::{format_hash_hex, parse_hash_hex},
};

use super::{
    AbilityOptions, ItemDef, ItemPackageMetadata, ItemRarity, ItemRenderOverride,
    ItemStatDefinition, SocketDef,
    abilities::{AbilityDisplayData, build_subclass_choices},
    inventory::{InventoryBucketDescriptor, item_inventory_metadata},
    investment::{item_investment_stats, masterwork_label, stat_allocation_labels},
    item_damage_type,
    perks::item_intrinsic_perk_hashes,
    quality::item_power_cap,
    sockets::{ORDINARY_SOCKET_CLASS, build_socket_choices, socket_package_sources},
};
use crate::catalog::{
    CatalogProgress, InventoryMetadata, ItemMaterialRequirementSetIndices, ObjectiveDef,
    ObjectiveOwnerDef, ObjectiveOwnerKind, UnlockDefinition,
    collections::item_material_requirement_set_indices_from_data,
    icons::item_icon_container,
    localization::{LocalizedStringCache, resolve_string},
    package::{array_at, i32_at, i64_at, relative_offset, u16_at, u32_at},
    progression::{
        ItemProgressionContext, PendingProgressionContext, add_objective_owner,
        attach_item_condition_contexts, item_objective_indices,
    },
};

const MAX_ITEM_SCAN_PROGRESS_UPDATES: usize = 200;
const ITEM_DESCRIPTION_OFFSET: usize = 0x98;
const EQUIPMENT_BLOCK_OFFSET: usize = 16;
const EQUIPMENT_SLOT_OFFSET: usize = 24;
const EQUIPMENT_SLOT_COUNT: i32 = 20;
const SOCKET_ENTRY_LIST_BLOCK_OFFSET: usize = 128;
const SOCKET_ENTRY_LIST_BLOCK_SIZE: usize = 12;
const PLUG_CATEGORY_OFFSET: usize = 392;
const PLUG_BLOCK_CLASS: u32 = 0x8080_77E3;
const PLUG_BLOCK_SEARCH_START: usize = 0x100;
const PLUG_BLOCK_SEARCH_END: usize = 0x300;
const PLUG_BLOCK_CATEGORY_OFFSET: usize = 4;
const PLUG_BLOCK_ROLL_SET_OFFSET: usize = 0x26;
const LINKED_PLUG_CLASS: u32 = 0x8080_3036;
const LINKED_PLUG_INDEX_OFFSET: usize = 12;
const ART_BLOCK_OFFSET: usize = 136;
const GEAR_ART_INDEX_OFFSET: usize = 88;
const ART_ROW_CLASS: u32 = 0x8080_77B5;
const ART_ROW_STRIDE: usize = 4;
const ART_ROW_VALUE_OFFSET: usize = 2;
const MATERIAL_OVERRIDE_CLASS: u32 = 0x8080_77B3;
const MATERIAL_STAGE_OFFSETS: [(u8, usize); 3] = [(0, 40), (1, 56), (2, 72)];
const MATERIAL_ROW_STRIDE: usize = 4;
const MATERIAL_ROW_VALUE_OFFSET: usize = 2;
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

struct ItemMetadataSources<'a> {
    hashes: &'a [u64],
    inventory_buckets: &'a HashMap<u8, InventoryBucketDescriptor>,
    item_stat_definitions: &'a [ItemStatDefinition],
    sandbox_perk_hashes: &'a [u64],
}

struct ItemMetadataOutputs<'a> {
    package: &'a mut HashMap<u64, ItemPackageMetadata>,
    inventory: &'a mut HashMap<u64, InventoryMetadata>,
    material_sets: &'a mut HashMap<u64, ItemMaterialRequirementSetIndices>,
    plug_categories: &'a mut HashMap<u64, u32>,
    plug_category_items: &'a mut HashMap<u32, Vec<u64>>,
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
        metadata.rarity = ItemRarity::from_package_value(item[186]);
        metadata.power_cap = item_power_cap(item);
        metadata.damage_type =
            bucket_hash.and_then(|bucket_hash| item_damage_type(item, bucket_hash));
        metadata.investment_stats = item_investment_stats(item, sources.item_stat_definitions);
        metadata.intrinsic_perks = item_intrinsic_perk_hashes(item, sources.sandbox_perk_hashes);
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
                            definition_size: None,
                            string_definition_tag: None,
                            icon_container_tag: None,
                            plug_category_hash: None,
                            equipment_slot: None,
                            socket_entry_list_index: None,
                            roll_set_index: None,
                            linked_plug_index: None,
                            linked_plug_hash: None,
                            gear_art_index: None,
                            art_arrangement_indices: [None; 4],
                            render_overrides: Vec::new(),
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
    let item_metadata_sources = ItemMetadataSources {
        hashes,
        inventory_buckets,
        item_stat_definitions,
        sandbox_perk_hashes,
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
        let bucket_hash = super::inventory::bucket_hash(item[184]);
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
    let Ok(relative) = i64_at(item, 104) else {
        return Ok(Some(decoded));
    };
    if relative == 0 {
        return Ok(Some(decoded));
    }
    let Ok(block) = relative_offset(104, 0, relative) else {
        return Ok(None);
    };
    let Ok((socket_count, socket_rows, class)) = array_at(item, block) else {
        return Ok(Some(decoded));
    };
    if class != ORDINARY_SOCKET_CLASS || socket_count > 12 {
        return Ok(Some(decoded));
    }

    for lane in 0..socket_count {
        let base = socket_rows + lane * 80;
        let socket_type = u16_at(item, base)?;
        let plug_index = u16_at(item, base + 2)?;
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
        PLUG_BLOCK_CLASS,
        PLUG_BLOCK_SEARCH_START,
        PLUG_BLOCK_SEARCH_END,
    );
    let category = plug_block
        .and_then(|block| u32_at(item, block + PLUG_BLOCK_CATEGORY_OFFSET).ok())
        .or_else(|| u32_at(item, PLUG_CATEGORY_OFFSET).ok());
    metadata.plug_category_hash = category
        .filter(|category| !matches!(*category, 0 | u32::MAX))
        .map(u64::from);
    metadata.roll_set_index =
        plug_block.and_then(|block| u16_at(item, block + PLUG_BLOCK_ROLL_SET_OFFSET).ok());

    metadata.linked_plug_index = find_record(item, LINKED_PLUG_CLASS, 0, item.len())
        .and_then(|block| u16_at(item, block + LINKED_PLUG_INDEX_OFFSET).ok())
        .filter(|index| *index != u16::MAX);
    metadata.linked_plug_hash = metadata
        .linked_plug_index
        .and_then(|index| item_hashes.get(usize::from(index)).copied());

    populate_item_appearance_metadata(item, metadata);
}

fn item_equipment_slot(item: &[u8]) -> Option<u8> {
    let block = resolve_item_block(item, EQUIPMENT_BLOCK_OFFSET)?;
    let slot = i32_at(item, block.checked_add(EQUIPMENT_SLOT_OFFSET)?).ok()?;
    if !(0..EQUIPMENT_SLOT_COUNT).contains(&slot) {
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
    metadata.gear_art_index = None;
    metadata.art_arrangement_indices = [None; 4];
    metadata.render_overrides.clear();

    let Some(art) = resolve_item_block(item, ART_BLOCK_OFFSET) else {
        return;
    };
    metadata.gear_art_index = art
        .checked_add(GEAR_ART_INDEX_OFFSET)
        .and_then(|offset| u16_at(item, offset).ok())
        .filter(|index| *index != u16::MAX);

    if let Some((count, rows)) = bounded_array(item, art, ART_ROW_CLASS, ART_ROW_STRIDE) {
        for index in 0..count {
            let row = rows + index * ART_ROW_STRIDE;
            let character_class = item[row] as i8;
            let Ok(arrangement) = u16_at(item, row + ART_ROW_VALUE_OFFSET) else {
                break;
            };
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
            if key == -1 {
                continue;
            }
            let Ok(value) = u16_at(item, row + MATERIAL_ROW_VALUE_OFFSET) else {
                break;
            };
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
        write_i32(&mut item, 424, 2);
        write_i64(&mut item, SOCKET_ENTRY_LIST_BLOCK_OFFSET, 384);
        write_u16(&mut item, 512, 37);
        write_u32(&mut item, 300, PLUG_BLOCK_CLASS);
        write_u32(&mut item, 304, 0x1234_5678);
        write_u16(&mut item, 338, 5);
        write_u32(&mut item, 560, LINKED_PLUG_CLASS);
        write_u16(&mut item, 572, 3);

        let mut metadata = ItemPackageMetadata::default();
        populate_native_item_metadata(&item, &[10, 20, 30, 40], &mut metadata);

        assert_eq!(metadata.equipment_slot, Some(2));
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

        assert_eq!(metadata.gear_art_index, Some(9));
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
        write_i32(&mut item, 140, EQUIPMENT_SLOT_COUNT);
        write_i64(&mut item, SOCKET_ENTRY_LIST_BLOCK_OFFSET, i64::MAX);

        assert_eq!(item_equipment_slot(&item), None);
        assert_eq!(item_socket_entry_list_index(&item), None);
    }

    fn write_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_i32(data: &mut [u8], offset: usize, value: i32) {
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
