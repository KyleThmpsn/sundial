use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use serde::{Deserialize, Serialize};
use tiger_pkg::{DestinyVersion, GameVersion, PackageManager, TagHash};

use crate::{class_items, unnamed_plugs};

const CACHE_SCHEMA: u32 = 35;
const SUNDIAL_VERSION: &str = env!("CARGO_PKG_VERSION");
const ORDINARY_SOCKET_CLASS: u32 = 0x8080_77C4;
const INVESTMENT_STAT_CLASS: u32 = 0x8080_3033;
const STAT_STRING_MAP_CLASS: u32 = 0x8080_5CC9;
const NO_PLUG_SOURCE: u32 = 0x811C_9DC5;
const INVESTMENT_STAT_DESCRIPTOR: usize = 0x2C0;
const INVESTMENT_STAT_ROW_SIZE: usize = 40;
const STAT_STRING_MAP_INDEX: usize = 59;
const STAT_STRING_ROW_SIZE: usize = 36;

#[derive(Clone, Copy, Debug)]
pub struct CatalogProgress {
    pub message: &'static str,
    pub completed: usize,
    pub total: usize,
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
    pub fn fraction(self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.completed as f32 / self.total as f32
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemDef {
    pub hash: u64,
    pub name: String,
    pub type_name: String,
    pub bucket_hash: u64,
    pub class_type: u64,
    pub default_plugs: Vec<Option<String>>,
    pub sockets: Vec<SocketDef>,
    #[serde(default)]
    pub abilities: AbilityOptions,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AbilityOptions {
    pub movement: Vec<AbilityChoice>,
    pub grenade: Vec<AbilityChoice>,
    pub super_ability: Vec<AbilityChoice>,
    pub melee: Vec<AbilityChoice>,
    pub class_ability: Vec<AbilityChoice>,
    #[serde(default)]
    pub attunements: Vec<AttunementChoice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbilityChoice {
    pub entry: u64,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttunementChoice {
    pub name: String,
    pub super_abilities: Vec<AbilityChoice>,
    pub melee: AbilityChoice,
    pub perks: Vec<AbilityChoice>,
}

#[derive(Default)]
struct AbilityDisplayData {
    names: HashMap<u32, String>,
    attunement_names: Vec<String>,
}

#[derive(Clone)]
struct ParsedAbilityEntry {
    choice: AbilityChoice,
    plug_source: u32,
    group: u8,
}

struct ScannedCatalog {
    items: Vec<ItemDef>,
    names: HashMap<u64, String>,
    type_names: HashMap<u64, String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SocketDef {
    pub socket_type: u16,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub pool: u32,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub allowed: Vec<u64>,
}

#[derive(Serialize, Deserialize)]
struct CatalogCache {
    schema: u32,
    sundial_version: String,
    fingerprint: String,
    items: Vec<ItemDef>,
    names: HashMap<u64, String>,
    type_names: HashMap<u64, String>,
    plug_pools: Vec<Vec<u64>>,
}

pub struct Catalog {
    pub items: Vec<ItemDef>,
    pub names: HashMap<u64, String>,
    type_names: HashMap<u64, String>,
    pub cache_path: PathBuf,
    pub loaded_from_cache: bool,
    plug_pools: Vec<Vec<u64>>,
    socket_type_options: HashMap<u16, Vec<u64>>,
    all_plug_options: Vec<u64>,
}

impl ItemDef {
    pub fn label(&self) -> String {
        if self.type_name.is_empty() {
            format!("{}  ({})", self.name, format_hash(self.hash))
        } else {
            format!(
                "{} — {}  ({})",
                self.name,
                self.type_name,
                format_hash(self.hash)
            )
        }
    }
}

impl SocketDef {
    pub fn display_label(&self, index: usize) -> String {
        if self.label.is_empty() {
            format!("Socket {}", index + 1)
        } else {
            format!("{}. {}", index + 1, self.label)
        }
    }
}

impl Catalog {
    pub fn load_or_scan_with_progress(
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
                            cache.items,
                            cache.names,
                            cache.type_names,
                            cache.plug_pools,
                            cache_path,
                            true,
                        ));
                    }
                }
            }
        }
        let ScannedCatalog {
            mut items,
            mut names,
            mut type_names,
        } = scan_packages(install, &mut report)?;
        report(CatalogProgress::stage("Optimizing the local catalog…"));
        unnamed_plugs::apply_to_catalog(&mut names, &mut type_names);
        let plug_pools = intern_socket_pools(&mut items, &names)?;
        let type_names = plug_pools
            .iter()
            .flatten()
            .filter_map(|hash| type_names.get(hash).cloned().map(|name| (*hash, name)))
            .collect();
        let cache = CatalogCache {
            schema: CACHE_SCHEMA,
            sundial_version: SUNDIAL_VERSION.into(),
            fingerprint,
            items,
            names,
            type_names,
            plug_pools,
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
            cache.items,
            cache.names,
            cache.type_names,
            cache.plug_pools,
            cache_path,
            false,
        ))
    }

    fn finish(
        mut items: Vec<ItemDef>,
        mut names: HashMap<u64, String>,
        mut type_names: HashMap<u64, String>,
        mut plug_pools: Vec<Vec<u64>>,
        cache_path: PathBuf,
        loaded_from_cache: bool,
    ) -> Self {
        unnamed_plugs::apply_to_catalog(&mut names, &mut type_names);
        for pool in &mut plug_pools {
            sort_plug_options(pool, &names);
        }
        let mut socket_type_options = HashMap::<u16, Vec<u64>>::new();
        for item in &items {
            for socket in &item.sockets {
                if let Some(pool) = plug_pools.get(socket.pool as usize) {
                    socket_type_options
                        .entry(socket.socket_type)
                        .or_default()
                        .extend(pool.iter().copied());
                }
            }
        }
        for options in socket_type_options.values_mut() {
            sort_plug_options(options, &names);
        }
        let mut all_plug_options = plug_pools.iter().flatten().copied().collect();
        sort_plug_options(&mut all_plug_options, &names);
        items.sort_by_key(|item| item.name.to_lowercase());
        Self {
            items,
            names,
            type_names,
            cache_path,
            loaded_from_cache,
            plug_pools,
            socket_type_options,
            all_plug_options,
        }
    }

    pub fn get_for_bucket(&self, hash: u64, bucket: u64) -> Option<&ItemDef> {
        self.items
            .iter()
            .find(|item| item.hash == hash && item.bucket_hash == bucket)
    }

    pub fn search(
        &self,
        text: &str,
        bucket: u64,
        class_type: u64,
        show_dummy_items: bool,
    ) -> Vec<&ItemDef> {
        let needle = text.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        self.items
            .iter()
            .filter(|item| {
                compatible(item, bucket, class_type, show_dummy_items)
                    && (item.name.to_lowercase().contains(&needle)
                        || item.type_name.to_lowercase().contains(&needle)
                        || format_hash(item.hash).to_lowercase().contains(&needle))
            })
            .take(40)
            .collect()
    }

    pub fn browse(&self, bucket: u64, class_type: u64, show_dummy_items: bool) -> Vec<&ItemDef> {
        self.items
            .iter()
            .filter(|item| compatible(item, bucket, class_type, show_dummy_items))
            .collect()
    }

    pub fn plug_label(&self, hash: u64) -> String {
        let name = self.names.get(&hash).map_or("Unknown plug", String::as_str);
        format!("{name}  ({})", format_hash(hash))
    }

    pub fn plug_type_name(&self, hash: u64) -> Option<&str> {
        self.type_names.get(&hash).map(String::as_str)
    }

    pub fn socket_options(&self, socket: &SocketDef) -> &[u64] {
        self.plug_pools
            .get(socket.pool as usize)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn socket_type_options(&self, socket_type: u16) -> &[u64] {
        self.socket_type_options
            .get(&socket_type)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn all_plug_options(&self) -> &[u64] {
        &self.all_plug_options
    }
}

fn sort_plug_options(options: &mut Vec<u64>, names: &HashMap<u64, String>) {
    options.sort_unstable();
    options.dedup();
    options.sort_by_cached_key(|hash| {
        let name = names.get(hash).map_or("", String::as_str).trim();
        (name.is_empty(), name.to_lowercase(), *hash)
    });
}

pub fn cache_is_current(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut prefix = [0u8; 128];
    let Ok(read) = file.read(&mut prefix) else {
        return false;
    };
    cache_header_is_current(&String::from_utf8_lossy(&prefix[..read]))
}

fn cache_header_is_current(prefix: &str) -> bool {
    prefix.contains(&format!("\"schema\":{CACHE_SCHEMA}"))
        && prefix.contains(&format!("\"sundial_version\":\"{SUNDIAL_VERSION}\""))
}

fn intern_socket_pools(
    items: &mut [ItemDef],
    names: &HashMap<u64, String>,
) -> Result<Vec<Vec<u64>>, String> {
    let mut pools = vec![Vec::new()];
    let mut indices = HashMap::<Vec<u64>, u32>::new();
    indices.insert(Vec::new(), 0);
    for item in items {
        for socket in &mut item.sockets {
            socket.allowed.sort_by_key(|hash| {
                names
                    .get(hash)
                    .map(|name| name.to_lowercase())
                    .unwrap_or_default()
            });
            socket.allowed.dedup();
            let pool = if let Some(index) = indices.get(&socket.allowed) {
                *index
            } else {
                let index = u32::try_from(pools.len())
                    .map_err(|_| "The catalog contains too many distinct socket pools")?;
                let values = std::mem::take(&mut socket.allowed);
                indices.insert(values.clone(), index);
                pools.push(values);
                index
            };
            socket.pool = pool;
            socket.allowed.clear();
        }
    }
    Ok(pools)
}

fn compatible(item: &ItemDef, bucket: u64, class_type: u64, show_dummy_items: bool) -> bool {
    item.bucket_hash == bucket
        && (item.class_type == 3 || item.class_type == class_type)
        && (show_dummy_items || !crate::dummy_items::contains(item.hash))
}

pub fn validate_install(install: &Path) -> Result<(), String> {
    if install.join("destiny2.exe").is_file()
        && install.join("packages").is_dir()
        && install.join("bin/x64/oo2core_3_win64.dll").is_file()
    {
        Ok(())
    } else {
        Err("Not a Shadowkeep install: expected destiny2.exe, packages, and bin\\x64\\oo2core_3_win64.dll".into())
    }
}

fn install_fingerprint(install: &Path) -> Result<String, String> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(install.join("packages"))
        .map_err(|e| format!("Could not read packages: {e}"))?
    {
        let entry = entry.map_err(|e| format!("Could not inspect packages: {e}"))?;
        let path = entry.path();
        if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("pkg"))
        {
            let meta = entry
                .metadata()
                .map_err(|e| format!("Could not inspect {}: {e}", path.display()))?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            entries.push(format!(
                "{}:{}:{}",
                entry.file_name().to_string_lossy(),
                meta.len(),
                modified
            ));
        }
    }
    entries.sort();
    Ok(entries.join("|"))
}

fn scan_packages(
    install: &Path,
    report: &mut dyn FnMut(CatalogProgress),
) -> Result<ScannedCatalog, String> {
    report(CatalogProgress::stage(
        "Opening the installed game packages…",
    ));
    let manager = PackageManager::new(
        install.join("packages"),
        GameVersion::Destiny(DestinyVersion::Destiny2Shadowkeep),
        None,
    )
    .map_err(|e| format!("Could not open the Shadowkeep packages: {e}"))?;
    let globals = manager
        .lookup
        .named_tags
        .iter()
        .find(|entry| entry.name == "investment_globals")
        .ok_or("The install has no investment_globals tag")?;
    let globals_data = manager
        .read_tag(globals.hash)
        .map_err(|e| format!("Could not read investment globals: {e}"))?;
    let localized_index = manager
        .read_tag(TagHash(u32_at(&globals_data, 16 + 72 * 16)?))
        .map_err(|e| format!("Could not read localized-string index: {e}"))?;
    let (localized_count, localized_rows, _) = array_at(&localized_index, 8)?;
    let localized_tags: Vec<TagHash> = (0..localized_count)
        .filter_map(|i| {
            u32_at(&localized_index, localized_rows + i * 8 + 4)
                .ok()
                .map(TagHash)
        })
        .collect();
    let mut localized_cache = HashMap::<u32, HashMap<u32, String>>::new();
    report(CatalogProgress::stage("Reading subclass ability names…"));
    let ability_displays = scan_ability_displays(&manager, &localized_tags, &mut localized_cache);
    let stat_names = scan_stat_names(
        &manager,
        &globals_data,
        &localized_tags,
        &mut localized_cache,
    );
    let root = manager
        .read_tag(TagHash(u32_at(&globals_data, 16)?))
        .map_err(|e| format!("Could not read investment root: {e}"))?;
    let plug_set_table = manager
        .read_tag(TagHash(u32_at(&root, 8 + 51 * 16)?))
        .map_err(|e| format!("Could not read reusable plug sets: {e}"))?;
    let item_table = manager
        .read_tag(TagHash(u32_at(&root, 8 + 48 * 16)?))
        .map_err(|e| format!("Could not read item table: {e}"))?;
    let (count, rows, _) = array_at(&item_table, 8)?;
    let string_map = manager
        .read_tag(TagHash(u32_at(&globals_data, 16 + 33 * 16)?))
        .map_err(|e| format!("Could not read item strings: {e}"))?;
    let (string_count, string_rows, _) = array_at(&string_map, 8)?;
    if count != string_count {
        return Err("The installed item and string tables do not match".into());
    }

    let hashes: Vec<u64> = (0..count)
        .map(|i| u32_at(&item_table, rows + i * 24).map(u64::from))
        .collect::<Result<_, _>>()?;
    let string_tags: HashMap<u64, TagHash> = (0..string_count)
        .filter_map(|i| {
            let base = string_rows + i * 24;
            Some((
                u64::from(u32_at(&string_map, base).ok()?),
                TagHash(u32_at(&string_map, base + 16).ok()?),
            ))
        })
        .collect();
    let mut names = HashMap::new();
    let mut type_names = HashMap::new();
    let mut items = Vec::new();
    let mut item_socket_lists = Vec::<(usize, u16)>::new();
    let mut plug_category_by_hash = HashMap::<u64, u32>::new();
    let mut plug_category_items = HashMap::<u32, Vec<u64>>::new();
    report(CatalogProgress {
        message: "Reading item definitions…",
        completed: 0,
        total: count,
    });
    for index in 0..count {
        if index % 64 == 0 {
            report(CatalogProgress {
                message: "Reading item definitions…",
                completed: index,
                total: count,
            });
        }
        let hash = hashes[index];
        let item_tag = TagHash(u32_at(&item_table, rows + index * 24 + 16)?);
        let Ok(item) = manager.read_tag(item_tag) else {
            continue;
        };
        if item.len() < 188 {
            continue;
        }
        let string_row = string_rows + index * 24;
        let string_tag = if u32_at(&string_map, string_row).ok().map(u64::from) == Some(hash) {
            TagHash(u32_at(&string_map, string_row + 16)?)
        } else {
            let Some(&tag) = string_tags.get(&hash) else {
                continue;
            };
            tag
        };
        let Ok(string_thing) = manager.read_tag(string_tag) else {
            continue;
        };
        let mut name = resolve_string(
            &manager,
            &localized_tags,
            &mut localized_cache,
            &string_thing,
            0x84,
        )
        .unwrap_or_default();
        let mut derived_masterwork_name = false;
        if name == "Masterwork"
            && let Some(label) = masterwork_label(&item, &stat_names)
        {
            name = label;
            derived_masterwork_name = true;
        }
        let mut type_name = resolve_string(
            &manager,
            &localized_tags,
            &mut localized_cache,
            &string_thing,
            0x90,
        )
        .unwrap_or_default();
        if name.trim().is_empty() {
            let Some((derived_name, derived_type_name)) =
                stat_allocation_labels(&item, &stat_names)
            else {
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
        if let Ok(category) = u32_at(&item, 392) {
            if category != 0 && category != u32::MAX {
                plug_category_by_hash.insert(hash, category);
                plug_category_items.entry(category).or_default().push(hash);
            }
        }
        let Some(bucket_hash) = bucket_hash(item[184]) else {
            continue;
        };
        if let Ok(relative) = i64_at(&item, 128) {
            if relative != 0 {
                let Ok(block) = relative_offset(128, 0, relative) else {
                    continue;
                };
                if let Ok(list_index) = u16_at(&item, block) {
                    item_socket_lists.push((items.len(), list_index));
                }
            }
        }
        let mut default_plugs = Vec::new();
        let mut sockets = Vec::new();
        if let Ok(relative) = i64_at(&item, 104) {
            if relative != 0 {
                let Ok(block) = relative_offset(104, 0, relative) else {
                    continue;
                };
                if let Ok((socket_count, socket_rows, class)) = array_at(&item, block) {
                    if class == ORDINARY_SOCKET_CLASS && socket_count <= 12 {
                        for lane in 0..socket_count {
                            let base = socket_rows + lane * 80;
                            let socket_type = u16_at(&item, base)?;
                            let plug_index = u16_at(&item, base + 2)?;
                            let plug = (plug_index != u16::MAX)
                                .then(|| hashes.get(plug_index as usize).copied())
                                .flatten();
                            default_plugs.push(plug.map(format_hash));
                            let mut allowed =
                                socket_allowed_hashes(&item, base, &hashes, &plug_set_table);
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
            }
        }
        let plug_hashes = default_plugs
            .iter()
            .flatten()
            .filter_map(|s| parse_hash(s))
            .chain(
                sockets
                    .iter()
                    .flat_map(|socket| socket.allowed.iter().copied()),
            )
            .collect::<Vec<_>>();
        for plug in plug_hashes {
            if string_tags.contains_key(&plug) {
                let Some(&s_tag) = string_tags.get(&plug) else {
                    continue;
                };
                if let Ok(s) = manager.read_tag(s_tag) {
                    if let Some(name) =
                        resolve_string(&manager, &localized_tags, &mut localized_cache, &s, 0x84)
                    {
                        names.entry(plug).or_insert(name);
                    }
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
    for item in &mut items {
        for (socket_index, socket) in item.sockets.iter_mut().enumerate() {
            let mut seeds = socket.allowed.clone();
            if let Some(Some(default)) = item.default_plugs.get(socket_index) {
                if let Some(hash) = parse_hash(default) {
                    seeds.push(hash);
                }
            }
            for seed in seeds {
                let Some(category) = plug_category_by_hash.get(&seed) else {
                    continue;
                };
                if matches!(*category, 0xB134_761E | 0x8772_7F34 | 0x6C86_3692) {
                    let Some(category_items) = plug_category_items.get(category) else {
                        continue;
                    };
                    socket.allowed.extend(category_items.iter().copied());
                }
            }
            socket.allowed.sort_unstable();
            socket.allowed.dedup();
        }
    }
    infer_socket_plug_types(&items, &names, &mut type_names);
    let tracker_plugs = [2_285_418_970, 2_302_094_943, 38_912_240];
    for item in &mut items {
        for socket in &mut item.sockets {
            // Kill/Crucible tracker sockets use a small synthetic plug set
            // keyed by socket type rather than a package plug-set row.
            if socket.socket_type == 518 {
                socket.allowed.extend(tracker_plugs);
                socket.allowed.sort_unstable();
                socket.allowed.dedup();
            }
        }
    }
    for item in &mut items {
        for (socket_index, socket) in item.sockets.iter_mut().enumerate() {
            let default = item
                .default_plugs
                .get(socket_index)
                .and_then(Option::as_deref)
                .and_then(parse_hash);
            socket.label = infer_socket_label(
                socket.socket_type,
                default,
                &socket.allowed,
                &names,
                &type_names,
            );
        }
    }
    let list_table = manager
        .read_tag(TagHash(u32_at(&root, 8 + 97 * 16)?))
        .map_err(|e| format!("Could not read subclass ability table: {e}"))?;
    report(CatalogProgress::stage("Building subclass choices…"));
    let (list_count, list_rows, _) = array_at(&list_table, 8)?;
    for (item_index, list_index) in item_socket_lists {
        let Some(item) = items.get_mut(item_index) else {
            continue;
        };
        if item.bucket_hash != 3_284_755_031 {
            continue;
        }
        if list_index as usize >= list_count {
            continue;
        }
        let list_tag = TagHash(u32_at(
            &list_table,
            list_rows + list_index as usize * 24 + 16,
        )?);
        if let Ok(list) = manager.read_tag(list_tag) {
            if let Some(display) = ability_displays.get(&list_index) {
                item.abilities = parse_abilities(&list, display, list_index);
                item.class_type = match list_index {
                    1..=3 => 1,  // Hunter
                    5..=7 => 0,  // Titan
                    9..=11 => 2, // Warlock
                    _ => 3,
                };
            }
        }
    }
    Ok(ScannedCatalog {
        items,
        names,
        type_names,
    })
}

fn infer_socket_label(
    socket_type: u16,
    default: Option<u64>,
    allowed: &[u64],
    names: &HashMap<u64, String>,
    type_names: &HashMap<u64, String>,
) -> String {
    if let Some(label) = verified_socket_label(socket_type) {
        return label.into();
    }

    if let Some(label) = default.and_then(|hash| socket_label_for_plug(hash, names, type_names)) {
        return label;
    }

    let mut counts = HashMap::<String, usize>::new();
    for &hash in allowed {
        if let Some(label) = socket_label_for_plug(hash, names, type_names) {
            *counts.entry(label).or_default() += 1;
        }
    }
    if let Some((label, count)) =
        counts
            .iter()
            .max_by(|(left_label, left_count), (right_label, right_count)| {
                left_count
                    .cmp(right_count)
                    .then_with(|| right_label.cmp(left_label))
            })
        && count.saturating_mul(2) >= counts.values().sum()
    {
        return label.clone();
    }

    String::new()
}

fn verified_socket_label(socket_type: u16) -> Option<&'static str> {
    match socket_type {
        29..=43 => Some("Armor Masterwork"),
        // Shadowkeep's public manifest categorizes these as GHOST SHELL PERKS;
        // individual perk definitions use the overly generic type "Intrinsic".
        51 => Some("Ghost Perk"),
        62 => Some("Sparrow Perk"),
        483 => Some("Weapon Masterwork"),
        518 => Some("Kill Tracker"),
        520 => Some("Armor Tier"),
        676 => Some("Stat Allocation"),
        678 | 679 => Some("Armor Energy Upgrade"),
        760 | 761 => Some("Top Stat Allocation"),
        762 | 763 => Some("Bottom Stat Allocation"),
        _ => None,
    }
}

fn inferred_plug_type_for_socket(socket_type: u16) -> Option<&'static str> {
    match socket_type {
        29..=43 => Some("Armor Masterwork"),
        51 => Some("Ghost Perk"),
        520 => Some("Armor Tier"),
        678 | 679 => Some("Armor Energy"),
        760 | 761 => Some("Top Stat Allocation"),
        762 | 763 => Some("Bottom Stat Allocation"),
        _ => None,
    }
}

fn infer_socket_plug_types(
    items: &[ItemDef],
    names: &HashMap<u64, String>,
    type_names: &mut HashMap<u64, String>,
) {
    for item in items {
        for (socket_index, socket) in item.sockets.iter().enumerate() {
            let Some(type_name) = inferred_plug_type_for_socket(socket.socket_type) else {
                continue;
            };
            let default = item
                .default_plugs
                .get(socket_index)
                .and_then(Option::as_deref)
                .and_then(parse_hash);
            for hash in socket.allowed.iter().copied().chain(default) {
                if socket.socket_type == 51 {
                    // "Intrinsic" is not useful here and is inconsistent with the
                    // manifest's Ghost Shell Perks socket category.
                    type_names.insert(hash, type_name.into());
                } else {
                    type_names.entry(hash).or_insert_with(|| type_name.into());
                }
            }
        }
    }

    for (&hash, name) in names {
        if name.trim().eq_ignore_ascii_case("Empty Mod Socket") {
            type_names.entry(hash).or_insert_with(|| "Armor Mod".into());
        }
    }
}

fn socket_label_for_plug(
    hash: u64,
    names: &HashMap<u64, String>,
    type_names: &HashMap<u64, String>,
) -> Option<String> {
    let name = names.get(&hash).map_or("", String::as_str).trim();
    let lower_name = name.to_ascii_lowercase();
    if lower_name == "default shader" {
        return Some("Shader".into());
    }
    if lower_name.contains("ornament") {
        return Some("Ornament".into());
    }
    if lower_name == "no projection" {
        return Some("Ghost Projection".into());
    }
    if lower_name == "default effect" {
        return Some("Transmat Effect".into());
    }
    if lower_name.contains("tracker") {
        return Some("Kill Tracker".into());
    }
    if lower_name.contains("catalyst") {
        return Some("Catalyst".into());
    }
    if lower_name.starts_with("tier ") && lower_name.ends_with(" weapon")
        || lower_name.starts_with("masterwork:")
    {
        return Some("Weapon Masterwork".into());
    }
    if lower_name.starts_with("tier ") && lower_name.ends_with(" armor") {
        return Some("Armor Tier".into());
    }
    if matches!(lower_name.as_str(), "upgrade armor" | "change energy type") {
        return Some("Armor Energy Upgrade".into());
    }

    let type_name = type_names.get(&hash).map_or("", String::as_str).trim();
    if type_name.is_empty() || type_name == "Restore Defaults" {
        return None;
    }
    if type_name.contains("Ornament") {
        Some("Ornament".into())
    } else {
        Some(type_name.into())
    }
}

fn parse_abilities(list: &[u8], display: &AbilityDisplayData, list_index: u16) -> AbilityOptions {
    let Ok((count, rows, _)) = array_at(list, 16) else {
        return AbilityOptions::default();
    };
    let mut entries = Vec::new();
    for index in 0..count.min(64) {
        let base = rows + index * 64;
        let Ok(display_hash) = u32_at(list, base) else {
            break;
        };
        let Ok(plug_source) = u32_at(list, base + 8) else {
            break;
        };
        let Some(&group) = list.get(base + 12) else {
            break;
        };
        let name = display
            .names
            .get(&display_hash)
            .cloned()
            .unwrap_or_else(|| format!("Unknown ability (0x{display_hash:08X})"));
        entries.push(ParsedAbilityEntry {
            choice: AbilityChoice {
                entry: index as u64,
                name,
            },
            plug_source,
            group,
        });
    }
    let choices = |indices: &[usize]| -> Vec<AbilityChoice> {
        indices
            .iter()
            .filter_map(|&index| entries.get(index).map(|entry| entry.choice.clone()))
            .collect()
    };
    let attunements = parse_attunements(&entries, &display.attunement_names, list_index);
    let mut super_ability = attunements
        .iter()
        .flat_map(|attunement| attunement.super_abilities.iter().cloned())
        .collect::<Vec<_>>();
    let mut seen_super_entries = Vec::new();
    super_ability.retain(|choice| {
        if seen_super_entries.contains(&choice.entry) {
            false
        } else {
            seen_super_entries.push(choice.entry);
            true
        }
    });
    let melee = attunements
        .iter()
        .map(|attunement| attunement.melee.clone())
        .collect();
    AbilityOptions {
        class_ability: choices(&[2, 3]),
        movement: choices(&[4, 5, 6]),
        grenade: choices(&[7, 8, 9]),
        super_ability,
        melee,
        attunements,
    }
}

fn parse_attunements(
    entries: &[ParsedAbilityEntry],
    names: &[String],
    list_index: u16,
) -> Vec<AttunementChoice> {
    let mut sources = Vec::<u32>::new();
    for entry in entries {
        if entry.group == 3
            && entry.plug_source != NO_PLUG_SOURCE
            && !sources.contains(&entry.plug_source)
        {
            sources.push(entry.plug_source);
        }
    }
    sources
        .into_iter()
        .enumerate()
        .filter_map(|(path_index, source)| {
            let perks = entries
                .iter()
                .filter(|entry| entry.group == 3 && entry.plug_source == source)
                .map(|entry| entry.choice.clone())
                .collect::<Vec<_>>();
            let melee = if perks.first().is_some_and(|choice| choice.entry == 20) {
                perks.get(1)
            } else {
                perks.first()
            }?
            .clone();
            let matching_super = super_entry_indices(list_index).iter().find_map(|&index| {
                let entry = entries.get(index)?;
                (entry.plug_source == source).then(|| entry.choice.clone())
            });
            // The top and bottom paths select the base super lane at entry 10.
            // Most Forsaken middle paths carry a distinct super at entry 20,
            // but Arcstrider and Sentinel route their guard super through the
            // path selected by the melee entry and keep the base super lane.
            let super_ability = if path_index == 2 && !middle_path_uses_base_super(list_index) {
                matching_super
            } else {
                entries.get(10).map(|entry| entry.choice.clone())
            };
            let super_abilities = super_ability.into_iter().collect();
            let name = names
                .get(path_index)
                .cloned()
                .unwrap_or_else(|| match path_index {
                    0 => "Top path".into(),
                    1 => "Bottom path".into(),
                    _ => "Middle path".into(),
                });
            Some(AttunementChoice {
                name,
                super_abilities,
                melee,
                perks,
            })
        })
        .collect()
}

const fn middle_path_uses_base_super(list_index: u16) -> bool {
    matches!(list_index, 1 | 6)
}

fn socket_allowed_hashes(
    item: &[u8],
    socket_base: usize,
    item_hashes: &[u64],
    plug_set_table: &[u8],
) -> Vec<u64> {
    let mut allowed = Vec::new();

    // Small reusable lists, such as a fixed shader choice, are embedded in
    // the inventory item definition.
    if let Ok((count, rows, _)) = array_at(item, socket_base + 64) {
        for index in 0..count.min(65_535) {
            if let Ok(item_index) = u32_at(item, rows + index * 32) {
                if let Some(hash) = item_hashes.get(item_index as usize) {
                    allowed.push(*hash);
                }
            }
        }
    }

    // Larger option pools use the shared DestinyPlugSetDefinition table.
    // Reusable and randomized plug sets have separate row indices at +12 and
    // +32 respectively.
    let Ok((set_count, set_rows, _)) = array_at(plug_set_table, 8) else {
        return allowed;
    };
    for set_offset in [12, 32] {
        let Ok(set_index) = u16_at(item, socket_base + set_offset) else {
            continue;
        };
        if set_index == u16::MAX || set_index as usize >= set_count {
            continue;
        }
        let descriptor = set_rows + set_index as usize * 24 + 8;
        if let Ok((count, rows, _)) = array_at(plug_set_table, descriptor) {
            for index in 0..count.min(65_535) {
                if let Ok(item_index) = u32_at(plug_set_table, rows + index * 32) {
                    if let Some(hash) = item_hashes.get(item_index as usize) {
                        allowed.push(*hash);
                    }
                }
            }
        }
    }
    allowed
}

const fn super_entry_indices(list_index: u16) -> &'static [usize] {
    match list_index {
        // Arcstrider
        1 => &[10, 14, 20],
        // Gunslinger: Golden Gun, Deadshot/Six-Shooter and precision-tree
        // modifiers, plus Blade Barrage.
        2 => &[10, 13, 14, 17, 18, 20],
        // Nightstalker
        3 => &[10, 13, 18, 20],
        // Striker, Sentinel, and Voidwalker
        5 | 6 | 10 => &[10, 14, 18, 20],
        // Sunbreaker
        7 => &[10, 13, 14, 18, 20],
        // Dawnblade
        9 => &[10, 16, 17, 18, 20],
        // Stormcaller
        11 => &[10, 12, 14, 16, 20],
        _ => &[10, 20],
    }
}

fn scan_ability_displays(
    manager: &PackageManager,
    localized_tags: &[TagHash],
    localized_cache: &mut HashMap<u32, HashMap<u32, String>>,
) -> HashMap<u16, AbilityDisplayData> {
    // Shadowkeep's nine subclass socket lists are sparse. The display tables
    // are stored in descending socket-list order; list IDs 4 and 8 are not
    // subclass definitions.
    const SUBCLASS_LIST_IDS: [u16; 9] = [11, 10, 9, 7, 6, 5, 3, 2, 1];

    let mut tables: Vec<TagHash> = manager
        .get_all_by_reference(0x8080_5C42)
        .into_iter()
        .map(|(tag, _)| tag)
        .filter(|tag| manager.read_tag(*tag).is_ok_and(|data| data.len() > 700))
        .collect();
    tables.sort_by_key(|tag| tag.0);
    if tables.len() > 9 {
        tables = tables.split_off(tables.len() - 9);
    }
    let mut result = HashMap::new();
    for (list_id, tag) in SUBCLASS_LIST_IDS.into_iter().zip(tables) {
        let mut names = HashMap::new();
        let mut localized_indices = Vec::new();
        let Ok(table) = manager.read_tag(tag) else {
            continue;
        };
        for offset in (16..table.len()).step_by(4) {
            let Ok(raw_tag) = u32_at(&table, offset) else {
                continue;
            };
            let candidate = TagHash(raw_tag);
            if manager
                .get_entry(candidate)
                .is_none_or(|entry| entry.reference != 0x8080_5C49)
            {
                continue;
            }
            let Ok(display_hash) = u32_at(&table, offset - 16) else {
                continue;
            };
            let Ok(display) = manager.read_tag(candidate) else {
                continue;
            };
            if let Ok(index) = u32_at(&display, 160) {
                if (index as usize) < localized_tags.len() && !localized_indices.contains(&index) {
                    localized_indices.push(index);
                }
            }
            if let Some(name) =
                resolve_string(manager, localized_tags, localized_cache, &display, 160)
            {
                names.entry(display_hash).or_insert(name);
            }
        }
        // These three hashes are the native localized titles for the top,
        // bottom and Forsaken middle subclass paths. Their string banks are
        // identified by the entry display records above, so no game text is
        // embedded in Sundial.
        let attunement_names = [0xDF41_7340, 0x7308_73A5, 0x761A_F51A]
            .into_iter()
            .filter_map(|hash| {
                resolve_localized_hash(
                    manager,
                    localized_tags,
                    localized_cache,
                    &localized_indices,
                    hash,
                )
            })
            .collect();
        result.insert(
            list_id,
            AbilityDisplayData {
                names,
                attunement_names,
            },
        );
    }
    result
}

fn resolve_localized_hash(
    manager: &PackageManager,
    tags: &[TagHash],
    cache: &mut HashMap<u32, HashMap<u32, String>>,
    indices: &[u32],
    hash: u32,
) -> Option<String> {
    for &index in indices {
        if index as usize >= tags.len() {
            continue;
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = cache.entry(index) {
            let values = decode_strings(manager, tags[index as usize])
                .ok()?
                .into_iter()
                .collect();
            entry.insert(values);
        }
        if let Some(value) = cache.get(&index).and_then(|values| values.get(&hash)) {
            return Some(value.clone());
        }
    }
    None
}

fn scan_stat_names(
    manager: &PackageManager,
    globals: &[u8],
    localized_tags: &[TagHash],
    localized_cache: &mut HashMap<u32, HashMap<u32, String>>,
) -> Vec<String> {
    let tag_offset = 16 + STAT_STRING_MAP_INDEX * 16;
    let Ok(tag) = u32_at(globals, tag_offset) else {
        return Vec::new();
    };
    let Ok(table) = manager.read_tag(TagHash(tag)) else {
        return Vec::new();
    };
    let Ok((count, rows, class)) = array_at(&table, 8) else {
        return Vec::new();
    };
    if class != STAT_STRING_MAP_CLASS || count > 256 {
        return Vec::new();
    }
    (0..count)
        .map(|index| {
            resolve_string(
                manager,
                localized_tags,
                localized_cache,
                &table,
                rows + index * STAT_STRING_ROW_SIZE + 4,
            )
            .unwrap_or_default()
        })
        .collect()
}

fn stat_allocation_labels(item: &[u8], stat_names: &[String]) -> Option<(String, &'static str)> {
    let (count, rows, class) = array_at(item, INVESTMENT_STAT_DESCRIPTOR).ok()?;
    if class != INVESTMENT_STAT_CLASS || count != 3 {
        return None;
    }

    let mut stats = [(0_usize, 0_u32); 3];
    for (row_index, stat) in stats.iter_mut().enumerate() {
        let row = rows.checked_add(row_index.checked_mul(INVESTMENT_STAT_ROW_SIZE)?)?;
        *stat = (
            usize::from(u16_at(item, row).ok()?),
            u32_at(item, row + 4).ok()?,
        );
    }

    let (expected_indexes, type_name) = if stats.iter().all(|(index, _)| (3..=5).contains(index)) {
        ([3, 4, 5], "Top Stat Allocation")
    } else if stats.iter().all(|(index, _)| (6..=8).contains(index)) {
        ([6, 7, 8], "Bottom Stat Allocation")
    } else {
        return None;
    };

    let mut parts = Vec::with_capacity(3);
    for expected_index in expected_indexes {
        let value = stats
            .iter()
            .find_map(|(index, value)| (*index == expected_index).then_some(*value))?;
        if value == 0 || value > 100 {
            return None;
        }
        let stat_name = stat_names.get(expected_index)?.trim();
        if stat_name.is_empty() {
            return None;
        }
        parts.push(format!("{value} {stat_name}"));
    }

    Some((parts.join(" / "), type_name))
}

fn masterwork_label(item: &[u8], stat_names: &[String]) -> Option<String> {
    let (count, rows, class) = array_at(item, INVESTMENT_STAT_DESCRIPTOR).ok()?;
    if class != INVESTMENT_STAT_CLASS || !(1..=4).contains(&count) {
        return None;
    }
    let primary_stat = usize::from(u16_at(item, rows).ok()?);
    let name = stat_names.get(primary_stat)?.trim();
    (!name.is_empty()).then(|| format!("Masterwork: {name}"))
}

fn resolve_string(
    manager: &PackageManager,
    tags: &[TagHash],
    cache: &mut HashMap<u32, HashMap<u32, String>>,
    data: &[u8],
    offset: usize,
) -> Option<String> {
    let index = u32_at(data, offset).ok()?;
    if index == 0xFFFF || index as usize >= tags.len() {
        return None;
    }
    let hash = u32_at(data, offset + 4).ok()?;
    if let std::collections::hash_map::Entry::Vacant(entry) = cache.entry(index) {
        let values: HashMap<u32, String> = decode_strings(manager, tags[index as usize])
            .ok()?
            .into_iter()
            .collect();
        entry.insert(values);
    }
    cache.get(&index)?.get(&hash).cloned()
}

fn decode_strings(manager: &PackageManager, tag: TagHash) -> Result<Vec<(u32, String)>, String> {
    let header = manager.read_tag(tag).map_err(|e| e.to_string())?;
    let (hash_count, hash_data, _) = array_at(&header, 8)?;
    let data = manager
        .read_tag(TagHash(u32_at(&header, 24)?))
        .map_err(|e| e.to_string())?;
    let (part_count, parts, _) = array_at(&data, 8)?;
    let (combo_count, combos, _) = array_at(&data, 0x48)?;
    if hash_count != combo_count {
        return Err("Localized string table mismatch".into());
    }
    let mut result = Vec::with_capacity(hash_count);
    for index in 0..combo_count {
        let combo = combos + index * 0x10;
        let first = relative_offset(combo, 0, i64_at(&data, combo)?)?;
        let count = usize::try_from(i64_at(&data, combo + 8)?)
            .map_err(|_| "Localized string part count is negative or too large")?;
        let selected_bytes = count
            .checked_mul(0x20)
            .ok_or("Localized string part range overflowed")?;
        let selected_end = first
            .checked_add(selected_bytes)
            .ok_or("Localized string part range overflowed")?;
        let parts_bytes = part_count
            .checked_mul(0x20)
            .ok_or("Localized string table range overflowed")?;
        let parts_end = parts
            .checked_add(parts_bytes)
            .ok_or("Localized string table range overflowed")?;
        if first < parts || selected_end > parts_end {
            continue;
        }
        let mut value = Vec::new();
        for p in 0..count {
            let part = first
                .checked_add(
                    p.checked_mul(0x20)
                        .ok_or("Localized string part offset overflowed")?,
                )
                .ok_or("Localized string part offset overflowed")?;
            let part_pointer = part
                .checked_add(8)
                .ok_or("Localized string pointer overflowed")?;
            let start = relative_offset(part, 8, i64_at(&data, part_pointer)?)?;
            let len = u16_at(&data, part + 0x14)? as usize;
            // Shadowkeep stores this shift in a 16-bit field, but the string
            // codec defines the low byte as the character offset.
            let shift = u16_at(&data, part + 0x18)?.to_le_bytes()[0];
            let Some(end) = start.checked_add(len) else {
                continue;
            };
            let Some(bytes) = data.get(start..end) else {
                continue;
            };
            for ch in String::from_utf8_lossy(bytes).chars() {
                let shifted = char::from_u32(ch as u32 + u32::from(shift)).unwrap_or(ch);
                let mut encoded = [0; 4];
                value.extend_from_slice(shifted.encode_utf8(&mut encoded).as_bytes());
            }
        }
        result.push((
            u32_at(&header, hash_data + index * 4)?,
            String::from_utf8_lossy(&value).into_owned(),
        ));
    }
    Ok(result)
}

fn array_at(data: &[u8], descriptor: usize) -> Result<(usize, usize, u32), String> {
    let count_raw = u64_at(data, descriptor)?;
    let count = usize::try_from(count_raw).map_err(|_| "Package array is too large")?;
    let pointer = descriptor
        .checked_add(8)
        .ok_or("Package array pointer overflowed")?;
    let header = relative_offset(descriptor, 8, i64_at(data, pointer)?)?;
    if u64_at(data, header)? != count_raw {
        return Err("Package array count mismatch".into());
    }
    let rows = header
        .checked_add(16)
        .ok_or("Package array row offset overflowed")?;
    let class_offset = header
        .checked_add(8)
        .ok_or("Package array class offset overflowed")?;
    Ok((count, rows, u32_at(data, class_offset)?))
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(bytes_at(data, offset)?))
}
fn u32_at(data: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(bytes_at(data, offset)?))
}
fn u64_at(data: &[u8], offset: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(bytes_at(data, offset)?))
}
fn i64_at(data: &[u8], offset: usize) -> Result<i64, String> {
    Ok(i64::from_le_bytes(bytes_at(data, offset)?))
}

fn bytes_at<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], String> {
    let end = offset
        .checked_add(N)
        .ok_or_else(|| format!("Package offset overflowed at {offset}"))?;
    data.get(offset..end)
        .ok_or_else(|| format!("Package data ended at {offset}"))?
        .try_into()
        .map_err(|_| format!("Invalid {N}-byte package value"))
}

fn relative_offset(base: usize, bias: usize, relative: i64) -> Result<usize, String> {
    let origin = base
        .checked_add(bias)
        .ok_or("Package relative pointer overflowed")?;
    if relative >= 0 {
        let relative =
            usize::try_from(relative).map_err(|_| "Package relative pointer is too large")?;
        origin
            .checked_add(relative)
            .ok_or_else(|| "Package relative pointer overflowed".into())
    } else {
        let magnitude = usize::try_from(relative.unsigned_abs())
            .map_err(|_| "Package relative pointer is too small")?;
        origin
            .checked_sub(magnitude)
            .ok_or_else(|| "Package relative pointer points before the data".into())
    }
}

const fn bucket_hash(bucket: u8) -> Option<u64> {
    Some(match bucket {
        0 => 1_498_876_634,
        1 => 2_465_295_065,
        2 => 953_998_645,
        3 => 3_448_274_439,
        4 => 3_551_918_588,
        5 => 14_239_492,
        6 => 20_886_954,
        7 => 1_585_787_867,
        8 => 4_023_194_814,
        9 => 2_025_709_351,
        10 => 284_967_655,
        16 => 3_284_755_031,
        17 => 4_292_445_962,
        27 => 4_274_335_291,
        41 => 2_401_704_334,
        47 => 3_683_254_069,
        _ => return None,
    })
}

pub(crate) fn format_hash(hash: u64) -> String {
    format!("0x{hash:08X}")
}
fn parse_hash(text: &str) -> Option<u64> {
    u64::from_str_radix(
        text.trim()
            .trim_start_matches("0x")
            .trim_start_matches("0X"),
        16,
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn super_choices_include_gunslinger_and_dawnblade_alternates() {
        assert!(super_entry_indices(2).contains(&13)); // Deadshot
        assert!(super_entry_indices(9).contains(&20)); // Well of Radiance
    }

    #[test]
    fn attunements_keep_super_and_melee_in_the_same_native_path() {
        let mut entries = (0..24)
            .map(|entry| ParsedAbilityEntry {
                choice: AbilityChoice {
                    entry,
                    name: format!("Entry {entry}"),
                },
                plug_source: NO_PLUG_SOURCE,
                group: u8::MAX,
            })
            .collect::<Vec<_>>();
        for (range, source) in [(11..15, 1), (15..19, 2), (20..24, 3)] {
            for index in range {
                entries[index].plug_source = source;
                entries[index].group = 3;
            }
        }
        let paths = parse_attunements(&entries, &["Sky".into(), "Flame".into(), "Grace".into()], 9);
        assert_eq!(paths.len(), 3);
        assert_eq!(paths[0].melee.entry, 11);
        assert_eq!(paths[1].melee.entry, 15);
        assert_eq!(paths[2].melee.entry, 21);
        assert_eq!(
            paths[1]
                .super_abilities
                .iter()
                .map(|choice| choice.entry)
                .collect::<Vec<_>>(),
            vec![10]
        );
        assert_eq!(paths[1].super_abilities[0].name, "Entry 10");
        assert_eq!(paths[2].super_abilities[0].entry, 20);
    }

    #[test]
    fn all_shadowkeep_attunements_use_the_native_super_and_melee_entries() {
        let mut entries = (0..24)
            .map(|entry| ParsedAbilityEntry {
                choice: AbilityChoice {
                    entry,
                    name: format!("Entry {entry}"),
                },
                plug_source: NO_PLUG_SOURCE,
                group: u8::MAX,
            })
            .collect::<Vec<_>>();
        for (range, source) in [(11..15, 1), (15..19, 2), (20..24, 3)] {
            for index in range {
                entries[index].plug_source = source;
                entries[index].group = 3;
            }
        }

        for (list_index, middle_super) in [
            (1, 10),
            (2, 20),
            (3, 20),
            (5, 20),
            (6, 10),
            (7, 20),
            (9, 20),
            (10, 20),
            (11, 20),
        ] {
            let paths = parse_attunements(
                &entries,
                &["Top".into(), "Bottom".into(), "Middle".into()],
                list_index,
            );
            let pairs = paths
                .iter()
                .map(|path| (path.super_abilities[0].entry, path.melee.entry))
                .collect::<Vec<_>>();
            assert_eq!(
                pairs,
                vec![(10, 11), (10, 15), (middle_super, 21)],
                "socket list {list_index}"
            );
        }
    }

    #[test]
    fn catalog_cache_requires_the_current_sundial_version() {
        assert!(cache_header_is_current(&format!(
            "{{\"schema\":{CACHE_SCHEMA},\"sundial_version\":\"{SUNDIAL_VERSION}\"}}"
        )));
        assert!(!cache_header_is_current(&format!(
            "{{\"schema\":{CACHE_SCHEMA},\"sundial_version\":\"older\"}}"
        )));
        assert!(!cache_header_is_current(&format!(
            "{{\"schema\":{},\"sundial_version\":\"{SUNDIAL_VERSION}\"}}",
            CACHE_SCHEMA - 1
        )));
    }

    #[test]
    fn really_unsafe_options_include_every_discovered_plug_once() {
        let names = HashMap::from([
            (1, "Zeta".to_owned()),
            (2, "Alpha".to_owned()),
            (3, "Beta".to_owned()),
        ]);
        let catalog = Catalog::finish(
            Vec::new(),
            names,
            HashMap::new(),
            vec![Vec::new(), vec![4, 3, 1], vec![2, 3]],
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
    fn unnamed_armor_stat_plugs_use_their_local_investment_values() {
        let mut item = vec![0_u8; 0x300 + INVESTMENT_STAT_ROW_SIZE * 3];
        let count = 3_u64;
        item[INVESTMENT_STAT_DESCRIPTOR..INVESTMENT_STAT_DESCRIPTOR + 8]
            .copy_from_slice(&count.to_le_bytes());
        item[INVESTMENT_STAT_DESCRIPTOR + 8..INVESTMENT_STAT_DESCRIPTOR + 16]
            .copy_from_slice(&(0x28_i64).to_le_bytes());
        item[0x2F0..0x2F8].copy_from_slice(&count.to_le_bytes());
        item[0x2F8..0x2FC].copy_from_slice(&INVESTMENT_STAT_CLASS.to_le_bytes());

        for (row, stat_index, value) in [(0, 5_u16, 7_u32), (1, 3, 13), (2, 4, 1)] {
            let offset = 0x300 + row * INVESTMENT_STAT_ROW_SIZE;
            item[offset..offset + 2].copy_from_slice(&stat_index.to_le_bytes());
            item[offset + 4..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        let stat_names = vec![
            String::new(),
            String::new(),
            String::new(),
            "Mobility".into(),
            "Resilience".into(),
            "Recovery".into(),
        ];

        assert_eq!(
            stat_allocation_labels(&item, &stat_names),
            Some((
                "13 Mobility / 1 Resilience / 7 Recovery".into(),
                "Top Stat Allocation"
            ))
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
    fn masterwork_labels_use_the_primary_local_stat_name() {
        const ROW_SIZE: usize = 48;
        let mut item = vec![0_u8; 0x300 + ROW_SIZE * 2];
        let count = 2_u64;
        item[INVESTMENT_STAT_DESCRIPTOR..INVESTMENT_STAT_DESCRIPTOR + 8]
            .copy_from_slice(&count.to_le_bytes());
        item[INVESTMENT_STAT_DESCRIPTOR + 8..INVESTMENT_STAT_DESCRIPTOR + 16]
            .copy_from_slice(&(0x28_i64).to_le_bytes());
        item[0x2F0..0x2F8].copy_from_slice(&count.to_le_bytes());
        item[0x2F8..0x2FC].copy_from_slice(&INVESTMENT_STAT_CLASS.to_le_bytes());
        item[0x300..0x302].copy_from_slice(&2_u16.to_le_bytes());
        item[0x300 + ROW_SIZE..0x302 + ROW_SIZE].copy_from_slice(&1_u16.to_le_bytes());
        let stat_names = vec![String::new(), "Impact".into(), "Charge Time".into()];

        assert_eq!(
            masterwork_label(&item, &stat_names).as_deref(),
            Some("Masterwork: Charge Time")
        );
        assert_eq!(masterwork_label(&item, &stat_names[..2]), None);
    }

    #[test]
    fn package_offsets_reject_underflow_and_out_of_bounds_reads() {
        assert!(relative_offset(8, 0, -9).is_err());
        assert!(relative_offset(usize::MAX, 1, 0).is_err());
        assert!(u64_at(&[0; 4], usize::MAX).is_err());

        let mut descriptor = [0_u8; 32];
        descriptor[0..8].copy_from_slice(&1_u64.to_le_bytes());
        descriptor[8..16].copy_from_slice(&(-17_i64).to_le_bytes());
        assert!(array_at(&descriptor, 0).is_err());
    }
}
