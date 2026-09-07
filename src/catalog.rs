use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use tiger_pkg::TagHash;

use crate::{
    hash::{format_hash_hex, parse_hash_hex},
    unnamed_plugs,
};

mod cache;
mod collections;
mod icons;
mod items;
pub(crate) mod package;
mod package_access;
mod progression;
mod scan;

use crate::investment_localization::resolve_string;
pub(crate) use cache::cache_is_current;
use cache::{CACHE_SCHEMA, CatalogCache, CatalogContents, SUNDIAL_VERSION};
pub(crate) use collections::{
    COLLECTIBLE_ACQUIRED_CONDITION_FIELD, CollectibleDef, CollectionConditionDef,
    CollectionConditionTokenDef, ItemMaterialRequirementSetIndices, MaterialRequirementDef,
    MaterialRequirementSetDef,
};
use icons::IconRuntime;
use items::PowerCapDefinition;
#[cfg(test)]
pub(crate) use items::SocketDef;
pub(crate) use items::is_authorable_weapon_item;
#[cfg(test)]
pub(crate) use items::is_weapon_bucket;
pub(crate) use items::{
    AbilityChoice, AbilityOptions, InventoryDefinition, InventoryMetadata, InventoryScope,
    InvestmentStatDisplayPoint, ItemDamageProfile, ItemDamageType, ItemDef, ItemInvestmentStat,
    ItemPackageMetadata, ItemRarity, ItemStatDefinition, ItemStatGroup, ItemWeaponAmmoType,
    ItemWeaponInventorySlot, format_in_game_investment_stat, interpolate_investment_stat_display,
};
#[cfg(test)]
pub(crate) use items::{AttunementChoice, ItemStackability};
use items::{
    GearKind, build_gear_type_options, build_socket_type_options, format_plug_label,
    intern_socket_pools, sort_plug_options,
};
use package::install_fingerprint;
pub(crate) use package::validate_install;
pub(crate) use package_access::PackageInspectionAccess;
use progression::unlock_state_indices;
pub(crate) use progression::{
    ObjectiveDef, ObjectiveOwnerDef, ObjectiveOwnerKind, ObjectiveOwnerTraitDef,
    ProgressionContextDef, ProgressionContextKind, ProgressionDefinition,
    ProgressionFactionDefinition, ProgressionScope, UnlockDefinition,
};
use scan::scan_packages;

#[derive(Clone, Copy, Debug)]
pub(crate) struct CatalogProgress {
    pub message: &'static str,
    pub completed: usize,
    pub total: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CatalogStats {
    pub items: usize,
    pub plugs: usize,
    pub icons: usize,
    pub descriptions: usize,
}

/// A whitespace-tokenized catalog query. Every term must match at least one
/// available field, so searches such as `ace catalyst` can span an item name
/// and its description without requiring one contiguous phrase.
pub(crate) struct CatalogSearchQuery {
    terms: Vec<String>,
}

impl CatalogSearchQuery {
    pub(crate) fn new(text: &str) -> Self {
        Self {
            terms: text.split_whitespace().map(str::to_lowercase).collect(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub(crate) fn matches(&self, catalog: &Catalog, hash: u64, additional_fields: &[&str]) -> bool {
        if self.terms.is_empty() {
            return true;
        }
        let hash_hex = format_hash_hex(hash);
        let mut fields = additional_fields
            .iter()
            .copied()
            .chain(catalog.display_name(hash))
            .chain(catalog.plug_type_name(hash))
            .chain(catalog.description(hash))
            .chain(catalog.package_item_name(hash))
            .chain(catalog.package_item_type_name(hash))
            .chain(catalog.item_package_name(hash))
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        fields.push(hash_hex.to_lowercase());
        self.terms
            .iter()
            .all(|term| fields.iter().any(|field| field.contains(term)))
    }

    pub(crate) fn name_match_count(&self, name: &str) -> usize {
        let name = name.to_lowercase();
        self.terms
            .iter()
            .filter(|term| name.contains(term.as_str()))
            .count()
    }
}

impl CatalogProgress {
    const fn stage(message: &'static str) -> Self {
        Self {
            message,
            completed: 0,
            total: 0,
        }
    }

    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn fraction(self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.completed as f32 / self.total as f32
        }
    }
}

pub(crate) struct Catalog {
    pub items: Vec<Arc<ItemDef>>,
    pub names: HashMap<u64, String>,
    type_names: HashMap<u64, String>,
    package_item_names: HashMap<u64, String>,
    package_item_type_names: HashMap<u64, String>,
    descriptions: HashMap<u64, String>,
    icon_containers: HashMap<u64, u32>,
    item_package_metadata: HashMap<u64, ItemPackageMetadata>,
    item_stat_definitions: Vec<ItemStatDefinition>,
    power_cap_definitions: Vec<PowerCapDefinition>,
    item_stat_groups: Vec<ItemStatGroup>,
    trait_definitions: Vec<ObjectiveOwnerTraitDef>,
    reusable_plug_set_count: usize,
    socket_entry_list_count: usize,
    package_names: HashMap<u16, String>,
    pub cache_path: PathBuf,
    pub loaded_from_cache: bool,
    install_path: PathBuf,
    inspection_access: Arc<PackageInspectionAccess>,
    icon_runtime: Mutex<IconRuntime>,
    inventory_metadata: HashMap<u64, InventoryMetadata>,
    objectives: Vec<ObjectiveDef>,
    unlock_flag_definitions: Vec<UnlockDefinition>,
    unlock_value_definitions: Vec<UnlockDefinition>,
    collectibles: Vec<CollectibleDef>,
    shared_expression_pool: Vec<Vec<CollectionConditionTokenDef>>,
    material_requirement_sets: Vec<MaterialRequirementSetDef>,
    item_material_requirement_set_indices: HashMap<u64, ItemMaterialRequirementSetIndices>,
    progression_definitions: Vec<ProgressionDefinition>,
    progression_package_error: Option<String>,
    unlock_flag_state_indices: HashMap<(u8, u16), usize>,
    unlock_value_state_indices: HashMap<(u8, u16), usize>,
    objectives_by_unlock_value: HashMap<usize, Vec<usize>>,
    progression_names: HashMap<u64, String>,
    inventory_hashes: Vec<u64>,
    character_inventory_candidate_buckets: CharacterInventoryCandidateBuckets,
    item_indices: HashMap<u64, usize>,
    bucket_item_indices: HashMap<u64, Vec<usize>>,
    plug_pools: Vec<Vec<u64>>,
    socket_type_options: HashMap<u16, Vec<u64>>,
    socket_and_gear_type_options: HashMap<String, HashMap<u16, Vec<u64>>>,
    gear_type_options: HashMap<GearKind, Vec<u64>>,
    cosmetic_socket_pools: HashSet<u32>,
    all_plug_options: Vec<u64>,
    plug_hashes: HashSet<u64>,
}

#[derive(Default)]
struct CharacterInventoryCandidateBuckets {
    standard: [Vec<InventoryMetadata>; 4],
    including_dummy_items: [Vec<InventoryMetadata>; 4],
}

impl CharacterInventoryCandidateBuckets {
    fn build(
        inventory_hashes: &[u64],
        item_indices: &HashMap<u64, usize>,
        items: &[ItemDef],
        metadata: &HashMap<u64, InventoryMetadata>,
    ) -> Self {
        let mut buckets = Self::default();
        for hash in inventory_hashes {
            let Some(item) = item_indices.get(hash).and_then(|index| items.get(*index)) else {
                continue;
            };
            let Some(metadata) = metadata
                .get(hash)
                .filter(|metadata| metadata.is_character_inventory_candidate())
                .copied()
            else {
                continue;
            };
            let class_indices = match item.class_type {
                0 => &[0][..],
                1 => &[1][..],
                2 => &[2][..],
                3 => &[0, 1, 2, 3][..],
                _ => continue,
            };
            for class_index in class_indices {
                Self::push_unique(&mut buckets.including_dummy_items[*class_index], metadata);
                if !crate::dummy_items::contains(*hash) {
                    Self::push_unique(&mut buckets.standard[*class_index], metadata);
                }
            }
        }
        buckets
    }

    fn push_unique(buckets: &mut Vec<InventoryMetadata>, candidate: InventoryMetadata) {
        if !buckets.iter().any(|existing| {
            existing.scope == candidate.scope
                && existing.native_bucket_id == candidate.native_bucket_id
        }) {
            buckets.push(candidate);
        }
    }

    fn get(&self, class_type: u64, show_dummy_items: bool) -> &[InventoryMetadata] {
        let class_index = usize::try_from(class_type)
            .ok()
            .filter(|class_index| *class_index <= 2)
            .unwrap_or(3);
        if show_dummy_items {
            &self.including_dummy_items[class_index]
        } else {
            &self.standard[class_index]
        }
    }
}

fn insert_progression_name(names: &mut HashMap<u64, String>, hash: u64, candidate: &str) {
    let candidate = candidate.trim();
    if !candidate.is_empty() {
        names.entry(hash).or_insert_with(|| candidate.to_owned());
    }
}

fn bucket_item_indices(items: &[ItemDef]) -> HashMap<u64, Vec<usize>> {
    let mut indices = HashMap::<u64, Vec<usize>>::new();
    for (index, item) in items.iter().enumerate() {
        if item.bucket_hash != 0 {
            indices.entry(item.bucket_hash).or_default().push(index);
        }
    }
    indices
}

fn add_objective_progression_names(names: &mut HashMap<u64, String>, objectives: &[ObjectiveDef]) {
    for objective in objectives {
        for candidate in [
            objective.name.as_str(),
            objective.progress_description.as_str(),
            objective.display_description.as_str(),
            objective.description.as_str(),
        ] {
            insert_progression_name(names, objective.hash, candidate);
        }
        for owner in &objective.owners {
            insert_progression_name(names, objective.hash, &owner.name);
            insert_progression_name(names, owner.hash, &owner.name);
            for trait_definition in &owner.traits {
                insert_progression_name(names, trait_definition.hash, &trait_definition.name);
            }
        }
    }
}

fn add_referenced_objective_names(names: &mut HashMap<u64, String>, objectives: &[ObjectiveDef]) {
    for objective in objectives {
        if names.contains_key(&objective.hash) {
            continue;
        }
        let referenced_name = objective
            .referenced_objective_indices
            .iter()
            .filter_map(|index| objectives.get(usize::from(*index)))
            .find_map(|target| names.get(&target.hash))
            .cloned();
        if let Some(name) = referenced_name {
            insert_progression_name(names, objective.hash, &name);
        }
    }
}

fn add_unlock_progression_names(
    names: &mut HashMap<u64, String>,
    flag_definitions: &[UnlockDefinition],
    value_definitions: &[UnlockDefinition],
) {
    for definition in flag_definitions.iter().chain(value_definitions) {
        if let Some(name) = definition.name.as_deref() {
            insert_progression_name(names, definition.hash, name);
        }
        for context in &definition.tested_by {
            for candidate in [
                context.name.as_str(),
                context.type_name.as_str(),
                context.description.as_str(),
            ] {
                insert_progression_name(names, definition.hash, candidate);
            }
            for component in context.paths.iter().flatten() {
                insert_progression_name(names, definition.hash, component);
            }
            insert_progression_name(names, context.hash, &context.name);
        }
    }
}

fn progression_names(
    objectives: &[ObjectiveDef],
    unlock_flag_definitions: &[UnlockDefinition],
    unlock_value_definitions: &[UnlockDefinition],
    collectibles: &[CollectibleDef],
) -> HashMap<u64, String> {
    let mut names = HashMap::new();
    add_objective_progression_names(&mut names, objectives);
    add_referenced_objective_names(&mut names, objectives);
    add_unlock_progression_names(
        &mut names,
        unlock_flag_definitions,
        unlock_value_definitions,
    );
    for definition in collectibles {
        insert_progression_name(&mut names, definition.hash, &definition.name);
        insert_progression_name(&mut names, definition.item_hash, &definition.name);
    }
    names
}

fn objectives_by_unlock_value(objectives: &[ObjectiveDef]) -> HashMap<usize, Vec<usize>> {
    let mut indices = HashMap::<usize, Vec<usize>>::new();
    for (objective_index, objective) in objectives.iter().enumerate() {
        if let Some(definition_index) = objective.related_unlock_value_definition_index {
            indices
                .entry(usize::from(definition_index))
                .or_default()
                .push(objective_index);
        }
    }
    indices
}

impl Catalog {
    #[cfg(test)]
    pub(crate) fn for_test(
        items: Vec<ItemDef>,
        metadata: HashMap<u64, ItemPackageMetadata>,
    ) -> Self {
        Self::finish(
            CatalogContents {
                items,
                item_package_metadata: metadata,
                ..Default::default()
            },
            PathBuf::new(),
            PathBuf::new(),
            false,
        )
    }

    pub(crate) fn load_or_scan_with_progress(
        install: &Path,
        cache_path: PathBuf,
        force: bool,
        mut report: impl FnMut(CatalogProgress),
    ) -> Result<Self, String> {
        report(CatalogProgress::stage("Checking the local catalog…"));
        validate_install(install)?;
        let fingerprint = install_fingerprint(install)?;
        if !force && cache_is_current(&cache_path) {
            if let Ok(raw) = fs::read(&cache_path) {
                if let Ok(cache) = serde_json::from_slice::<CatalogCache>(&raw) {
                    if cache.schema == CACHE_SCHEMA
                        && cache.sundial_version == SUNDIAL_VERSION
                        && cache.fingerprint == fingerprint
                    {
                        report(CatalogProgress {
                            message: "Loaded the local catalog",
                            completed: 1,
                            total: 1,
                        });
                        return Ok(Self::finish(
                            cache.contents,
                            cache_path,
                            install.to_path_buf(),
                            true,
                        ));
                    }
                }
            }
        }
        let mut contents = scan_packages(install, &mut report)?;
        report(CatalogProgress::stage("Optimizing the local catalog…"));
        unnamed_plugs::apply_to_catalog(&mut contents.names, &mut contents.type_names);
        contents.plug_pools = intern_socket_pools(&mut contents.items, &contents.names)?;
        contents.type_names = contents
            .plug_pools
            .iter()
            .flatten()
            .chain(contents.inventory_metadata.keys())
            .filter_map(|hash| {
                contents
                    .type_names
                    .get(hash)
                    .cloned()
                    .map(|name| (*hash, name))
            })
            .collect();
        let cache = CatalogCache {
            schema: CACHE_SCHEMA,
            sundial_version: SUNDIAL_VERSION.into(),
            fingerprint,
            contents,
        };
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Could not create catalog cache: {e}"))?;
        }
        let encoded =
            serde_json::to_vec(&cache).map_err(|e| format!("Could not encode catalog: {e}"))?;
        report(CatalogProgress::stage("Saving the local catalog…"));
        crate::storage::replace_file(&cache_path, &encoded)
            .map_err(|e| format!("Could not save catalog cache: {e}"))?;
        report(CatalogProgress {
            message: "Local catalog ready",
            completed: 1,
            total: 1,
        });
        Ok(Self::finish(
            cache.contents,
            cache_path,
            install.to_path_buf(),
            false,
        ))
    }

    fn finish(
        contents: CatalogContents,
        cache_path: PathBuf,
        install_path: PathBuf,
        loaded_from_cache: bool,
    ) -> Self {
        let CatalogContents {
            mut items,
            names,
            type_names,
            package_item_names,
            package_item_type_names,
            descriptions,
            icon_containers,
            item_package_metadata,
            item_stat_definitions,
            power_cap_definitions,
            item_stat_groups,
            trait_definitions,
            reusable_plug_set_count,
            socket_entry_list_count,
            package_names,
            inventory_metadata,
            objectives,
            unlock_flag_definitions,
            unlock_value_definitions,
            collectibles,
            shared_expression_pool,
            material_requirement_sets,
            item_material_requirement_set_indices,
            progression_definitions,
            progression_package_error,
            mut plug_pools,
        } = contents;
        for pool in &mut plug_pools {
            sort_plug_options(pool, &names);
        }
        let (socket_type_options, socket_and_gear_type_options) =
            build_socket_type_options(&items, &plug_pools, &names);
        let (gear_type_options, cosmetic_socket_pools) =
            build_gear_type_options(&items, &plug_pools, &names);
        let mut all_plug_options = plug_pools.iter().flatten().copied().collect();
        sort_plug_options(&mut all_plug_options, &names);
        let plug_hashes = all_plug_options
            .iter()
            .copied()
            .chain(
                items
                    .iter()
                    .flat_map(|item| &item.default_plugs)
                    .filter_map(|plug| plug.as_deref().and_then(parse_hash_hex)),
            )
            .collect();
        items.sort_by_cached_key(|item| item.name.to_lowercase());
        let mut inventory_hashes = inventory_metadata
            .keys()
            .filter(|hash| names.contains_key(hash))
            .copied()
            .collect::<Vec<_>>();
        inventory_hashes.sort_by_cached_key(|hash| {
            (
                names
                    .get(hash)
                    .map_or_else(String::new, |name| name.to_lowercase()),
                *hash,
            )
        });
        let item_indices = items
            .iter()
            .enumerate()
            .map(|(index, item)| (item.hash, index))
            .collect::<HashMap<_, _>>();
        let character_inventory_candidate_buckets = CharacterInventoryCandidateBuckets::build(
            &inventory_hashes,
            &item_indices,
            &items,
            &inventory_metadata,
        );
        let bucket_item_indices = bucket_item_indices(&items);
        let unlock_flag_state_indices = unlock_state_indices(&unlock_flag_definitions);
        let unlock_value_state_indices = unlock_state_indices(&unlock_value_definitions);
        let progression_names = progression_names(
            &objectives,
            &unlock_flag_definitions,
            &unlock_value_definitions,
            &collectibles,
        );
        let objectives_by_unlock_value = objectives_by_unlock_value(&objectives);
        Self {
            items: items.into_iter().map(Arc::new).collect(),
            names,
            type_names,
            package_item_names,
            package_item_type_names,
            descriptions,
            icon_containers,
            item_package_metadata,
            item_stat_definitions,
            power_cap_definitions,
            item_stat_groups,
            trait_definitions,
            reusable_plug_set_count,
            socket_entry_list_count,
            package_names,
            cache_path,
            loaded_from_cache,
            install_path,
            inspection_access: Arc::default(),
            icon_runtime: Mutex::new(IconRuntime::default()),
            inventory_metadata,
            objectives,
            unlock_flag_definitions,
            unlock_value_definitions,
            collectibles,
            shared_expression_pool,
            material_requirement_sets,
            item_material_requirement_set_indices,
            progression_definitions,
            progression_package_error,
            unlock_flag_state_indices,
            unlock_value_state_indices,
            objectives_by_unlock_value,
            progression_names,
            inventory_hashes,
            character_inventory_candidate_buckets,
            item_indices,
            bucket_item_indices,
            plug_pools,
            socket_type_options,
            socket_and_gear_type_options,
            gear_type_options,
            cosmetic_socket_pools,
            all_plug_options,
            plug_hashes,
        }
    }

    pub(crate) fn get_for_bucket(&self, hash: u64, bucket: u64) -> Option<&ItemDef> {
        self.item(hash).filter(|item| item.bucket_hash == bucket)
    }

    pub(crate) fn item_handle_for_bucket(&self, hash: u64, bucket: u64) -> Option<Arc<ItemDef>> {
        self.item_handle(hash)
            .filter(|item| item.bucket_hash == bucket)
    }

    /// Finds an existing equipment definition without requiring its bucket hash.
    pub(crate) fn item(&self, hash: u64) -> Option<&ItemDef> {
        self.item_indices
            .get(&hash)
            .and_then(|index| self.items.get(*index))
            .map(Arc::as_ref)
    }

    pub(crate) fn item_has_collectible(&self, hash: u64) -> bool {
        self.collectibles
            .iter()
            .any(|collectible| collectible.item_hash == hash)
    }

    pub(crate) fn items_for_bucket(&self, bucket_hash: u64) -> impl Iterator<Item = &ItemDef> {
        self.bucket_item_indices
            .get(&bucket_hash)
            .into_iter()
            .flatten()
            .filter_map(|index| self.items.get(*index))
            .map(Arc::as_ref)
    }

    pub(crate) fn item_handle(&self, hash: u64) -> Option<Arc<ItemDef>> {
        self.item_indices
            .get(&hash)
            .and_then(|index| self.items.get(*index))
            .cloned()
    }

    pub(crate) fn search(
        &self,
        text: &str,
        bucket: u64,
        class_type: u64,
        show_dummy_items: bool,
        allow_cross_class_subclasses: bool,
    ) -> Vec<&ItemDef> {
        let query = CatalogSearchQuery::new(text);
        if query.is_empty() {
            return Vec::new();
        }
        let mut matches = self
            .items
            .iter()
            .filter(|item| {
                compatible(
                    item,
                    bucket,
                    class_type,
                    show_dummy_items,
                    allow_cross_class_subclasses,
                ) && query.matches(self, item.hash, &[&item.name, &item.type_name])
            })
            .map(Arc::as_ref)
            .collect::<Vec<_>>();
        matches.sort_by_cached_key(|item| {
            (
                std::cmp::Reverse(query.name_match_count(&item.name)),
                item.name.to_lowercase(),
                item.hash,
            )
        });
        matches
    }

    pub(crate) fn browse(
        &self,
        bucket: u64,
        class_type: u64,
        show_dummy_items: bool,
        allow_cross_class_subclasses: bool,
    ) -> Vec<&ItemDef> {
        self.items
            .iter()
            .filter(|item| {
                compatible(
                    item,
                    bucket,
                    class_type,
                    show_dummy_items,
                    allow_cross_class_subclasses,
                )
            })
            .map(Arc::as_ref)
            .collect()
    }

    pub(crate) fn plug_label(&self, hash: u64, include_hash: bool) -> String {
        let name = self.names.get(&hash).map_or("Unknown plug", String::as_str);
        format_plug_label(name, hash, include_hash)
    }

    pub(crate) fn plug_type_name(&self, hash: u64) -> Option<&str> {
        self.type_names
            .get(&hash)
            .map(String::as_str)
            .filter(|name| !name.trim().is_empty())
    }

    pub(crate) fn package_item_name(&self, hash: u64) -> Option<&str> {
        self.package_item_names
            .get(&hash)
            .map(String::as_str)
            .filter(|name| !name.trim().is_empty())
    }

    pub(crate) fn package_item_type_name(&self, hash: u64) -> Option<&str> {
        self.package_item_type_names
            .get(&hash)
            .map(String::as_str)
            .filter(|name| !name.trim().is_empty())
    }

    pub(crate) fn item_package_metadata(&self, hash: u64) -> Option<&ItemPackageMetadata> {
        self.item_package_metadata.get(&hash)
    }

    pub(crate) fn install_path(&self) -> &Path {
        &self.install_path
    }

    pub(crate) fn inspection_access(&self) -> Arc<PackageInspectionAccess> {
        Arc::clone(&self.inspection_access)
    }

    pub(crate) fn item_stat_definition_count(&self) -> usize {
        self.item_stat_definitions.len()
    }

    pub(crate) fn trait_definitions(&self) -> &[ObjectiveOwnerTraitDef] {
        &self.trait_definitions
    }

    pub(crate) fn reusable_plug_set_count(&self) -> usize {
        self.reusable_plug_set_count
    }

    pub(crate) fn socket_entry_list_count(&self) -> usize {
        self.socket_entry_list_count
    }

    pub(crate) fn item_package_name(&self, hash: u64) -> Option<&str> {
        let metadata = self.item_package_metadata.get(&hash)?;
        self.package_names
            .get(&TagHash(metadata.definition_tag).pkg_id())
            .map(String::as_str)
            .filter(|name| !name.trim().is_empty())
    }

    pub(crate) fn display_name(&self, hash: u64) -> Option<&str> {
        self.names
            .get(&hash)
            .map(String::as_str)
            .filter(|name| !name.trim().is_empty())
            .or_else(|| {
                self.item(hash)
                    .map(|item| item.name.as_str())
                    .filter(|name| !name.trim().is_empty())
            })
            .or_else(|| self.progression_names.get(&hash).map(String::as_str))
    }

    pub(crate) fn stats(&self) -> CatalogStats {
        CatalogStats {
            items: self.items.len(),
            plugs: self.all_plug_options.len(),
            icons: self.icon_containers.len(),
            descriptions: self.descriptions.len(),
        }
    }

    pub(crate) fn description(&self, hash: u64) -> Option<&str> {
        self.descriptions.get(&hash).map(String::as_str)
    }
}

fn compatible(
    item: &ItemDef,
    bucket: u64,
    class_type: u64,
    show_dummy_items: bool,
    allow_cross_class_subclasses: bool,
) -> bool {
    item.bucket_hash == bucket
        && (item.class_type == 3
            || item.class_type == class_type
            || (allow_cross_class_subclasses && item.bucket_hash == 3_284_755_031))
        && (show_dummy_items || !crate::dummy_items::contains(item.hash))
}

#[cfg(test)]
mod tests;
