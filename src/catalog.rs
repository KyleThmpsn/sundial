use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use tiger_pkg::TagHash;

use crate::{hash::format_hash_hex, orbit_map, unnamed_plugs};

mod cache;
mod collections;
mod icons;
mod items;
mod localization;
pub(crate) mod package;
mod progression;
mod scan;

pub(crate) use cache::cache_is_current;
use cache::{CACHE_SCHEMA, CatalogCache, CatalogContents, SUNDIAL_VERSION};
pub(crate) use collections::{
    CollectibleDef, CollectionConditionDef, CollectionConditionTokenDef,
    ItemMaterialRequirementSetIndices, MaterialRequirementDef, MaterialRequirementSetDef,
};
use icons::IconRuntime;
#[allow(unused_imports)]
pub(crate) use items::AttunementChoice;
#[allow(unused_imports)]
pub(crate) use items::ItemIntrinsicPerk;
#[allow(unused_imports)]
pub(crate) use items::ItemStackability;
pub(crate) use items::SandboxPerkDefinition;
#[cfg(test)]
pub(crate) use items::SocketDef;
pub(crate) use items::{
    AbilityChoice, AbilityOptions, InventoryDefinition, InventoryMetadata, InventoryScope,
    ItemDamageType, ItemDef, ItemInvestmentStat, ItemPackageMetadata, ItemRarity,
    ItemStatDefinition,
};
use items::{
    GearKind, build_gear_type_options, build_socket_type_options, format_plug_label,
    index_intrinsic_perk_references, intern_socket_pools, sort_plug_options,
};
#[cfg(test)]
use items::{
    attach_item_objective_owners, infer_socket_label, infer_socket_plug_types,
    item_scan_progress_stride, socket_label_for_plug,
};
use localization::resolve_string;
use package::install_fingerprint;
pub(crate) use package::validate_install;
#[cfg(test)]
use package::{array_at, relative_offset};
use progression::unlock_state_indices;
pub(crate) use progression::{
    ObjectiveDef, ObjectiveOwnerDef, ObjectiveOwnerKind, ObjectiveOwnerTraitDef,
    ProgressionContextDef, ProgressionContextKind, ProgressionDefinition,
    ProgressionFactionDefinition, ProgressionScope, UnlockDefinition,
};
use scan::scan_packages;
#[cfg(test)]
use scan::{retain_progression_enrichment, retain_progression_scan};

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
    orbit_backdrops: Vec<String>,
    orbit_map_entries: Vec<orbit_map::Entry>,
    pub names: HashMap<u64, String>,
    type_names: HashMap<u64, String>,
    package_item_names: HashMap<u64, String>,
    package_item_type_names: HashMap<u64, String>,
    descriptions: HashMap<u64, String>,
    icon_containers: HashMap<u64, u32>,
    item_package_metadata: HashMap<u64, ItemPackageMetadata>,
    item_stat_definitions: Vec<ItemStatDefinition>,
    sandbox_perk_definitions: Vec<SandboxPerkDefinition>,
    intrinsic_perk_items: HashMap<u64, Vec<u64>>,
    package_names: HashMap<u16, String>,
    pub cache_path: PathBuf,
    pub loaded_from_cache: bool,
    install_path: PathBuf,
    icon_runtime: Mutex<IconRuntime>,
    inventory_metadata: HashMap<u64, InventoryMetadata>,
    objectives: Vec<ObjectiveDef>,
    unlock_flag_definitions: Vec<UnlockDefinition>,
    unlock_value_definitions: Vec<UnlockDefinition>,
    collectibles: Vec<CollectibleDef>,
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
            mut orbit_backdrops,
            mut orbit_map_entries,
            names,
            type_names,
            package_item_names,
            package_item_type_names,
            descriptions,
            icon_containers,
            item_package_metadata,
            item_stat_definitions,
            sandbox_perk_definitions,
            package_names,
            inventory_metadata,
            objectives,
            unlock_flag_definitions,
            unlock_value_definitions,
            collectibles,
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
        let intrinsic_perk_items = index_intrinsic_perk_references(&item_package_metadata);
        items.sort_by_key(|item| item.name.to_lowercase());
        orbit_backdrops.sort();
        orbit_map_entries.sort_by(|first, second| first.destination.cmp(&second.destination));
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
            orbit_backdrops,
            orbit_map_entries,
            names,
            type_names,
            package_item_names,
            package_item_type_names,
            descriptions,
            icon_containers,
            item_package_metadata,
            item_stat_definitions,
            sandbox_perk_definitions,
            intrinsic_perk_items,
            package_names,
            cache_path,
            loaded_from_cache,
            install_path,
            icon_runtime: Mutex::new(IconRuntime::default()),
            inventory_metadata,
            objectives,
            unlock_flag_definitions,
            unlock_value_definitions,
            collectibles,
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

    pub(crate) fn orbit_backdrops(&self) -> &[String] {
        &self.orbit_backdrops
    }

    pub(crate) fn orbit_map_entries(&self) -> &[orbit_map::Entry] {
        &self.orbit_map_entries
    }

    pub(crate) fn search(
        &self,
        text: &str,
        bucket: u64,
        class_type: u64,
        show_dummy_items: bool,
    ) -> Vec<&ItemDef> {
        let query = CatalogSearchQuery::new(text);
        if query.is_empty() {
            return Vec::new();
        }
        let mut matches = self
            .items
            .iter()
            .filter(|item| {
                compatible(item, bucket, class_type, show_dummy_items)
                    && query.matches(self, item.hash, &[&item.name, &item.type_name])
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
    ) -> Vec<&ItemDef> {
        self.items
            .iter()
            .filter(|item| compatible(item, bucket, class_type, show_dummy_items))
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

fn compatible(item: &ItemDef, bucket: u64, class_type: u64, show_dummy_items: bool) -> bool {
    item.bucket_hash == bucket
        && (item.class_type == 3 || item.class_type == class_type)
        && (show_dummy_items || !crate::dummy_items::contains(item.hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_scan_progress_updates_are_bounded_without_becoming_choppy() {
        assert_eq!(item_scan_progress_stride(0), 64);
        assert_eq!(item_scan_progress_stride(12_800), 64);
        assert_eq!(item_scan_progress_stride(100_000), 500);
        assert!(100_000_usize.div_ceil(item_scan_progress_stride(100_000)) <= 200);
    }

    #[test]
    fn catalog_resolves_state_slots_and_family5_indices_through_package_definitions() {
        let flag = UnlockDefinition {
            hash: 0xAAAA_AAAA,
            code: 1,
            compact_slot: Some(26),
            name: Some("Crucible Access".into()),
            description: None,
            tested_by: Vec::new(),
        };
        let value = UnlockDefinition {
            hash: 0x14D6_FB47,
            code: 0x0201,
            compact_slot: Some(58),
            name: None,
            description: None,
            tested_by: Vec::new(),
        };
        let reader_named_value = UnlockDefinition {
            hash: 0x738D_5E2D,
            code: 1,
            compact_slot: None,
            name: None,
            description: None,
            tested_by: vec![ProgressionContextDef {
                hash: 0x22EB_C08C,
                kind: ProgressionContextKind::Record,
                name: "Tradition Is Bigger Than You".into(),
                type_name: String::new(),
                description: String::new(),
                paths: Vec::new(),
                condition_programs: Vec::new(),
            }],
        };
        let catalog = Catalog::finish(
            CatalogContents {
                items: Vec::new(),
                orbit_backdrops: Vec::new(),
                orbit_map_entries: Vec::new(),
                names: HashMap::new(),
                type_names: HashMap::new(),
                package_item_names: HashMap::new(),
                package_item_type_names: HashMap::new(),
                descriptions: HashMap::new(),
                icon_containers: HashMap::new(),
                item_package_metadata: HashMap::new(),
                item_stat_definitions: Vec::new(),
                sandbox_perk_definitions: Vec::new(),
                package_names: HashMap::new(),
                inventory_metadata: HashMap::new(),
                objectives: vec![ObjectiveDef {
                    hash: value.hash,
                    name: String::new(),
                    display_description: String::new(),
                    progress_description: "C Arc".into(),
                    description: "C Arc".into(),
                    completion_value: 5_000,
                    allow_overcompletion: true,
                    allow_negative_value: false,
                    allow_value_change_when_completed: true,
                    is_counting_downward: false,
                    condition_programs: Vec::new(),
                    referenced_objective_indices: Vec::new(),
                    intrinsic_perk_flag_definition_indices: Vec::new(),
                    owners: Vec::new(),
                    related_unlock_value_definition_index: Some(0),
                }],
                unlock_flag_definitions: vec![flag.clone()],
                unlock_value_definitions: vec![value.clone(), reader_named_value.clone()],
                collectibles: Vec::new(),
                material_requirement_sets: Vec::new(),
                item_material_requirement_set_indices: HashMap::new(),
                progression_definitions: Vec::new(),
                progression_package_error: Some("Objective definitions: unavailable".to_owned()),
                plug_pools: Vec::new(),
            },
            PathBuf::new(),
            PathBuf::new(),
            false,
        );

        assert_eq!(catalog.unlock_flag_for_state(1, 26), Some((0, &flag)));
        assert_eq!(catalog.unlock_value_for_state(1, 58), Some((0, &value)));
        assert!(catalog.unlock_value_for_state(2, 58).is_none());
        assert_eq!(catalog.unlock_value_definition(0), Some(&value));
        assert_eq!(
            catalog.progression_package_error(),
            Some("Objective definitions: unavailable")
        );
        assert_eq!(
            catalog
                .objective_for_unlock_value(0)
                .map(|row| (row.description.as_str(), row.completion_value)),
            Some(("C Arc", 5_000))
        );
        let objective = catalog.objective_for_unlock_value(0).unwrap();
        assert_eq!(objective.maximum_value(), None);
        assert_eq!(objective.minimum_value(), None);
        assert_eq!(catalog.display_name(flag.hash), Some("Crucible Access"));
        assert_eq!(catalog.display_name(value.hash), Some("C Arc"));
        assert_eq!(
            catalog.display_name(reader_named_value.hash),
            Some("Tradition Is Bigger Than You")
        );
    }

    #[test]
    fn progression_scan_failures_fall_back_without_discarding_other_sections() {
        let mut errors = Vec::new();
        let flags = retain_progression_scan("Unlock flag definitions", Ok(vec![1, 2]), &mut errors);
        let values: Vec<u8> = retain_progression_scan(
            "Unlock value definitions",
            Err("table unavailable".into()),
            &mut errors,
        );

        assert_eq!(flags, vec![1, 2]);
        assert!(values.is_empty());
        assert_eq!(errors, vec!["Unlock value definitions: table unavailable"]);
    }

    #[test]
    fn optional_unlock_display_failure_keeps_core_definitions() {
        let definitions = vec![UnlockDefinition {
            hash: 0x1234_5678,
            code: 1,
            compact_slot: Some(26),
            name: None,
            description: None,
            tested_by: Vec::new(),
        }];
        let mut errors = Vec::new();

        let retained = retain_progression_enrichment(
            "Unlock flag displays",
            Err("table unavailable".into()),
            definitions.clone(),
            &mut errors,
        );

        assert_eq!(retained, definitions);
        assert_eq!(errors, vec!["Unlock flag displays: table unavailable"]);
    }

    #[test]
    fn plug_labels_only_include_hashes_when_requested() {
        assert_eq!(format_plug_label("Rampage", 0x12AB, false), "Rampage");
        assert_eq!(
            format_plug_label("Rampage", 0x12AB, true),
            "Rampage  (0x000012AB)"
        );
    }

    #[test]
    fn unnamed_item_objective_owners_keep_the_installed_bucket_type() {
        let mut objectives = vec![ObjectiveDef::default()];
        let metadata = InventoryMetadata {
            scope: InventoryScope::Character,
            native_bucket_id: 37,
            stackability: ItemStackability::Stackable,
            max_stack_size: Some(1),
            bucket_capacity: Some(64),
        };

        attach_item_objective_owners(
            &mut objectives,
            &[0],
            0x1234_5678,
            "",
            "",
            Some(metadata),
            &[],
        );

        assert_eq!(objectives[0].owners.len(), 1);
        assert!(objectives[0].owners[0].name.is_empty());
        assert_eq!(objectives[0].owners[0].type_name, "General inventory");
    }

    #[test]
    fn inventory_apis_resolve_profile_only_items_and_keep_character_items_safe() {
        let character = ItemDef {
            hash: 30,
            name: "Character item".into(),
            type_name: "Helmet".into(),
            bucket_hash: 3_448_274_439,
            class_type: 3,
            default_plugs: Vec::new(),
            sockets: Vec::new(),
            abilities: AbilityOptions::default(),
        };
        let names = HashMap::from([
            (20, "Zeta material".into()),
            (10, "Alpha material".into()),
            (30, character.name.clone()),
        ]);
        let type_names = HashMap::from([
            (10, "Currency".into()),
            (20, "Material".into()),
            (30, character.type_name.clone()),
        ]);
        let profile = |bucket| InventoryMetadata {
            scope: InventoryScope::Profile,
            native_bucket_id: bucket,
            stackability: ItemStackability::Stackable,
            max_stack_size: Some(999),
            bucket_capacity: Some(10),
        };
        let inventory_metadata = HashMap::from([
            (10, profile(1)),
            (20, profile(2)),
            (
                30,
                InventoryMetadata {
                    scope: InventoryScope::Character,
                    native_bucket_id: 3,
                    stackability: ItemStackability::Instanced,
                    max_stack_size: Some(1),
                    bucket_capacity: Some(20),
                },
            ),
        ]);
        let catalog = Catalog::finish(
            CatalogContents {
                items: vec![character],
                orbit_backdrops: Vec::new(),
                orbit_map_entries: Vec::new(),
                names,
                type_names,
                package_item_names: HashMap::new(),
                package_item_type_names: HashMap::new(),
                descriptions: HashMap::new(),
                icon_containers: HashMap::new(),
                item_package_metadata: HashMap::new(),
                item_stat_definitions: Vec::new(),
                sandbox_perk_definitions: Vec::new(),
                package_names: HashMap::new(),
                inventory_metadata,
                objectives: Vec::new(),
                unlock_flag_definitions: Vec::new(),
                unlock_value_definitions: Vec::new(),
                collectibles: Vec::new(),
                material_requirement_sets: Vec::new(),
                item_material_requirement_set_indices: HashMap::new(),
                progression_definitions: Vec::new(),
                progression_package_error: None,
                plug_pools: vec![Vec::new()],
            },
            PathBuf::new(),
            PathBuf::new(),
            false,
        );

        assert_eq!(catalog.item(30).unwrap().name, "Character item");
        assert!(catalog.item(10).is_none());
        let profile_only = catalog.inventory_definition(10).unwrap();
        assert_eq!(profile_only.name, "Alpha material");
        assert_eq!(profile_only.type_name, "Currency");
        assert!(profile_only.item.is_none());
        assert_eq!(
            catalog
                .profile_item_candidates("")
                .map(|definition| definition.hash)
                .collect::<Vec<_>>(),
            vec![10, 20]
        );
        assert_eq!(catalog.profile_item_candidates("material").count(), 2);
        assert_eq!(
            catalog
                .character_inventory_candidates("", 0, false)
                .next()
                .unwrap()
                .hash,
            30
        );
        assert_eq!(
            catalog
                .character_inventory_candidate_buckets(0, false)
                .iter()
                .map(|metadata| metadata.native_bucket_id)
                .collect::<Vec<_>>(),
            vec![3]
        );
        assert_eq!(
            catalog
                .character_inventory_candidate_buckets(99, false)
                .iter()
                .map(|metadata| metadata.native_bucket_id)
                .collect::<Vec<_>>(),
            vec![3]
        );
        assert_eq!(
            catalog
                .inventory_metadata(30)
                .unwrap()
                .authored_row_capacity(),
            Some(20)
        );
    }

    #[test]
    fn equipment_browse_and_search_return_every_compatible_item() {
        let bucket = 1_498_876_634;
        let items = (0_u64..620)
            .rev()
            .map(|index| ItemDef {
                hash: 10_000 + index,
                name: format!("Matching item {index:04}"),
                type_name: "Test weapon".into(),
                bucket_hash: bucket,
                class_type: 3,
                default_plugs: Vec::new(),
                sockets: Vec::new(),
                abilities: AbilityOptions::default(),
            })
            .chain(std::iter::once(ItemDef {
                hash: 99_999,
                name: "Matching incompatible item".into(),
                type_name: "Test weapon".into(),
                bucket_hash: 0,
                class_type: 3,
                default_plugs: Vec::new(),
                sockets: Vec::new(),
                abilities: AbilityOptions::default(),
            }))
            .collect();
        let catalog = Catalog::finish(
            CatalogContents {
                items,
                orbit_backdrops: Vec::new(),
                orbit_map_entries: Vec::new(),
                names: HashMap::new(),
                type_names: HashMap::new(),
                package_item_names: HashMap::new(),
                package_item_type_names: HashMap::new(),
                descriptions: HashMap::from([(10_042, "A description-only match".to_owned())]),
                icon_containers: HashMap::new(),
                item_package_metadata: HashMap::new(),
                item_stat_definitions: Vec::new(),
                sandbox_perk_definitions: Vec::new(),
                package_names: HashMap::new(),
                inventory_metadata: HashMap::new(),
                objectives: Vec::new(),
                unlock_flag_definitions: Vec::new(),
                unlock_value_definitions: Vec::new(),
                collectibles: Vec::new(),
                material_requirement_sets: Vec::new(),
                item_material_requirement_set_indices: HashMap::new(),
                progression_definitions: Vec::new(),
                progression_package_error: None,
                plug_pools: Vec::new(),
            },
            PathBuf::new(),
            PathBuf::new(),
            false,
        );

        let browsed = catalog.browse(bucket, 0, true);
        assert_eq!(browsed.len(), 620);
        assert_eq!(browsed.first().unwrap().name, "Matching item 0000");
        assert_eq!(browsed.last().unwrap().name, "Matching item 0619");
        let bucket_items = catalog.items_for_bucket(bucket).collect::<Vec<_>>();
        assert_eq!(bucket_items.len(), 620);
        assert_eq!(bucket_items.first().unwrap().name, "Matching item 0000");
        assert_eq!(bucket_items.last().unwrap().name, "Matching item 0619");
        assert_eq!(catalog.items_for_bucket(0).count(), 0);

        let searched = catalog.search("matching", bucket, 0, true);
        assert_eq!(searched.len(), 620);
        assert_eq!(searched.first().unwrap().name, "Matching item 0000");
        assert_eq!(searched.last().unwrap().name, "Matching item 0619");
        assert_eq!(catalog.search("description-only", bucket, 0, true).len(), 1);
        assert_eq!(
            catalog
                .search("matching description-only", bucket, 0, true)
                .iter()
                .map(|item| item.hash)
                .collect::<Vec<_>>(),
            vec![10_042]
        );
        assert!(
            catalog
                .search("description-only absent", bucket, 0, true)
                .is_empty()
        );
    }

    #[test]
    fn schema_current_cache_requires_progression_sections() {
        let complete = serde_json::json!({
            "schema": CACHE_SCHEMA,
            "sundial_version": SUNDIAL_VERSION,
            "fingerprint": "test",
            "contents": {
                "items": [],
                "orbit_backdrops": [],
                "orbit_map_entries": [],
                "names": {"3365180871": "Test definition"},
                "type_names": {},
                "objectives": [],
                "unlock_flag_definitions": [],
                "unlock_value_definitions": [],
                "collectibles": [],
                "material_requirement_sets": [],
                "item_material_requirement_set_indices": {},
                "progression_definitions": [],
                "sandbox_perk_definitions": [],
                "plug_pools": [],
            },
        });
        let decoded = serde_json::from_value::<CatalogCache>(complete.clone()).unwrap();
        assert_eq!(
            decoded
                .contents
                .names
                .get(&3_365_180_871)
                .map(String::as_str),
            Some("Test definition")
        );

        for required in [
            "orbit_backdrops",
            "orbit_map_entries",
            "objectives",
            "unlock_flag_definitions",
            "unlock_value_definitions",
            "collectibles",
            "material_requirement_sets",
            "item_material_requirement_set_indices",
            "progression_definitions",
            "sandbox_perk_definitions",
        ] {
            let mut incomplete = complete.clone();
            incomplete["contents"]
                .as_object_mut()
                .unwrap()
                .remove(required);
            assert!(
                serde_json::from_value::<CatalogCache>(incomplete).is_err(),
                "a cache without {required} must be rescanned"
            );
        }
    }

    #[test]
    fn really_unsafe_options_include_every_discovered_plug_once() {
        let names = HashMap::from([
            (1, "Zeta".to_owned()),
            (2, "Alpha".to_owned()),
            (3, "Beta".to_owned()),
        ]);
        let catalog = Catalog::finish(
            CatalogContents {
                items: Vec::new(),
                orbit_backdrops: Vec::new(),
                orbit_map_entries: Vec::new(),
                names,
                type_names: HashMap::new(),
                package_item_names: HashMap::new(),
                package_item_type_names: HashMap::new(),
                descriptions: HashMap::new(),
                icon_containers: HashMap::new(),
                item_package_metadata: HashMap::new(),
                item_stat_definitions: Vec::new(),
                sandbox_perk_definitions: Vec::new(),
                package_names: HashMap::new(),
                inventory_metadata: HashMap::new(),
                objectives: Vec::new(),
                unlock_flag_definitions: Vec::new(),
                unlock_value_definitions: Vec::new(),
                collectibles: Vec::new(),
                material_requirement_sets: Vec::new(),
                item_material_requirement_set_indices: HashMap::new(),
                progression_definitions: Vec::new(),
                progression_package_error: None,
                plug_pools: vec![Vec::new(), vec![4, 3, 1], vec![2, 3]],
            },
            PathBuf::new(),
            PathBuf::new(),
            false,
        );

        assert_eq!(catalog.all_plug_options(), &[2, 3, 1, 4]);
        assert_eq!(catalog.plug_pools[1], [3, 1, 4]);
    }

    #[test]
    fn socket_labels_use_plug_semantics_and_keep_safe_fallbacks() {
        let names = HashMap::from([
            (1, "Default Shader".to_owned()),
            (2, "Celestial Nighthawk Ornament".to_owned()),
            (3, "Telesto Catalyst".to_owned()),
        ]);
        let type_names = HashMap::from([
            (1, "Restore Defaults".to_owned()),
            (2, "Hunter Universal Ornament".to_owned()),
        ]);

        assert_eq!(
            infer_socket_label(180, Some(1), &[1], &names, &type_names),
            "Shader"
        );
        assert_eq!(
            infer_socket_label(384, None, &[2], &names, &type_names),
            "Ornament"
        );
        assert_eq!(
            infer_socket_label(443, Some(3), &[3], &names, &type_names),
            "Catalyst"
        );
        assert_eq!(
            infer_socket_label(65535, None, &[], &names, &type_names),
            ""
        );
        assert_eq!(
            infer_socket_label(
                62,
                None,
                &[4],
                &names,
                &HashMap::from([(4, "Ghost Module".into())])
            ),
            "Sparrow Perk"
        );
        assert_eq!(
            infer_socket_label(29, None, &[], &names, &type_names),
            "Armor Masterwork"
        );
        assert_eq!(
            infer_socket_label(51, None, &[], &names, &type_names),
            "Ghost Perk"
        );
        assert_eq!(
            infer_socket_label(520, None, &[], &names, &type_names),
            "Armor Tier"
        );
        assert_eq!(
            infer_socket_label(676, None, &[], &names, &type_names),
            "Stat Allocation"
        );
        assert_eq!(
            infer_socket_label(678, None, &[], &names, &type_names),
            "Armor Energy Upgrade"
        );
        assert_eq!(
            infer_socket_label(760, None, &[], &names, &type_names),
            "Top Stat Allocation"
        );
        assert_eq!(
            infer_socket_label(763, None, &[], &names, &type_names),
            "Bottom Stat Allocation"
        );
        assert_eq!(
            socket_label_for_plug(
                4,
                &HashMap::from([(4, "Upgrade Armor".into())]),
                &HashMap::new()
            )
            .as_deref(),
            Some("Armor Energy Upgrade")
        );
    }

    #[test]
    fn armor_socket_types_fill_only_missing_plug_types() {
        let items = vec![ItemDef {
            hash: 10,
            name: "Test armor".into(),
            type_name: "Helmet".into(),
            bucket_hash: 3_448_274_439,
            class_type: 3,
            default_plugs: vec![Some("0x00000001".into())],
            sockets: vec![SocketDef {
                socket_type: 520,
                allowed: vec![2],
                ..SocketDef::default()
            }],
            abilities: AbilityOptions::default(),
        }];
        let names = HashMap::from([(3, "Empty Mod Socket".into())]);
        let mut type_names = HashMap::from([(2, "Specific local type".into())]);

        infer_socket_plug_types(&items, &names, &mut type_names);

        assert_eq!(type_names[&1], "Armor Tier");
        assert_eq!(type_names[&2], "Specific local type");
        assert_eq!(type_names[&3], "Armor Mod");
    }

    #[test]
    fn ghost_perk_socket_replaces_the_generic_intrinsic_type() {
        let items = vec![ItemDef {
            hash: 10,
            name: "Test Ghost".into(),
            type_name: "Ghost Shell".into(),
            bucket_hash: 4_023_194_814,
            class_type: 3,
            default_plugs: vec![Some("0x00000001".into())],
            sockets: vec![SocketDef {
                socket_type: 51,
                allowed: vec![2],
                ..SocketDef::default()
            }],
            abilities: AbilityOptions::default(),
        }];
        let mut type_names =
            HashMap::from([(1, "Intrinsic".to_owned()), (2, "Intrinsic".to_owned())]);

        infer_socket_plug_types(&items, &HashMap::new(), &mut type_names);

        assert_eq!(type_names[&1], "Ghost Perk");
        assert_eq!(type_names[&2], "Ghost Perk");
    }

    #[test]
    fn socket_display_labels_preserve_the_native_position() {
        let named = SocketDef {
            label: "Barrel".into(),
            ..SocketDef::default()
        };
        let unnamed = SocketDef::default();

        assert_eq!(named.display_label(1), "2. Barrel");
        assert_eq!(unnamed.display_label(1), "Socket 2");
    }

    #[test]
    fn package_offsets_reject_underflow_and_out_of_bounds_reads() {
        assert!(relative_offset(8, 0, -9).is_err());
        assert!(relative_offset(usize::MAX, 1, 0).is_err());
        assert!(package::u64_at(&[0; 4], usize::MAX).is_err());

        let mut descriptor = [0_u8; 32];
        descriptor[0..8].copy_from_slice(&1_u64.to_le_bytes());
        descriptor[8..16].copy_from_slice(&(-17_i64).to_le_bytes());
        assert!(array_at(&descriptor, 0).is_err());
    }
}
