//! The importable Reach corpus: one row per weapon or vehicle path with a render model.
//! Map-local copies of a path collapse onto the cache holding the most weapons and vehicles.
use super::{
    ExportRequest, ImportRequest, View,
    cache::{Cache, Tag},
    object::Object,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

/// Bumped when the stored row changes, so an older listing is read again.
const SCHEMA: u32 = 2;

/// Native summons a vehicle import drives. Tank physics keeps the tank's turret controls.
const TANK: (&str, u32) = ("tank", 0x80BF_AF2D);
const INTERCEPTOR: (&str, u32) = ("interceptor", 0x80C0_D7A6);
const HEAVY_PIKE: (&str, u32) = ("heavy_pike", 0x80C0_C1EC);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Entry {
    /// The cache the import reads.
    pub cache: String,
    pub group: String,
    /// The source path exactly as `cache` stores it.
    pub path: String,
    /// Every cache holding this path.
    pub caches: Vec<String>,
    /// Lowercase name unique within the listing. It also names the authored identity.
    pub slug: String,
    /// A world render model. Without one the import uses the Spartan first-person model.
    pub world: bool,
    pub first_person: bool,
    /// Vehicle physics kinds, such as `human_tank`.
    #[serde(default)]
    pub physics: Vec<String>,
    /// Barrels that fire a projectile.
    #[serde(default)]
    pub barrels: usize,
    #[serde(default)]
    pub projectiles_per_shot: i16,
    #[serde(default)]
    pub rounds_per_second: f32,
    #[serde(default)]
    pub charged: bool,
}

/// Source path segments, lowercased, with either separator.
fn segments(path: &str) -> Vec<String> {
    path.split(['\\', '/'])
        .filter(|part| !part.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// `support_high` reads Support High, and a word without vowels such as `dmr` reads DMR.
fn words(text: &str) -> String {
    text.split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            if word.chars().all(|c| c.is_ascii_alphabetic())
                && !word.chars().any(|c| "aeiouy".contains(c))
            {
                word.to_uppercase()
            } else {
                let mut chars = word.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(chars).collect()
                })
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn fnv(text: &str) -> u32 {
    text.bytes().fold(0x811C_9DC5, |h, b| {
        h.wrapping_mul(0x0100_0193) ^ u32::from(b)
    })
}

impl Entry {
    fn read(cache: &str, tag: &Tag, object: &Object) -> Self {
        let gameplay = &object.gameplay;
        let barrels: Vec<&Value> = gameplay["barrels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|barrel| !barrel["projectile"].is_null())
            .collect();
        Self {
            cache: cache.to_owned(),
            group: tag.group.clone(),
            path: tag.path.clone(),
            caches: vec![cache.to_owned()],
            slug: String::new(),
            world: object
                .world
                .as_ref()
                .is_some_and(|branch| branch.model.is_some()),
            first_person: object
                .first_person
                .iter()
                .any(|branch| branch.model.is_some()),
            physics: gameplay["physics"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|physics| physics["kind"].as_str().map(str::to_owned))
                .collect(),
            barrels: barrels.len(),
            projectiles_per_shot: barrels
                .iter()
                .filter_map(|barrel| barrel["projectiles_per_shot"].as_i64())
                .max()
                .and_then(|count| i16::try_from(count).ok())
                .unwrap_or(0),
            rounds_per_second: barrels
                .iter()
                .filter_map(|barrel| barrel["rounds_per_second"][1].as_f64())
                .fold(0.0, f64::max) as f32,
            charged: gameplay["triggers"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|trigger| {
                    trigger["charging_seconds"]
                        .as_f64()
                        .is_some_and(|s| s > 0.0)
                }),
        }
    }

    pub fn vehicle(&self) -> bool {
        self.group == "vehi"
    }

    fn leaf(&self) -> String {
        segments(&self.path).pop().unwrap_or_default()
    }

    /// The path's last component as words: `sniper_rifle_june` reads Sniper Rifle June.
    pub fn name(&self) -> String {
        words(&self.leaf())
    }

    /// The source folder grouping the object: the weapon family under `objects\weapons`, the
    /// faction under `objects\vehicles`, otherwise the top folder.
    pub fn category(&self) -> String {
        let parts = segments(&self.path);
        let folder = match parts.as_slice() {
            [objects, kind, family, _, ..]
                if objects == "objects" && (kind == "weapons" || kind == "vehicles") =>
            {
                family
            }
            [objects, folder, _, ..] if objects == "objects" => folder,
            [folder, ..] => folder,
            [] => return String::new(),
        };
        words(folder)
    }

    /// The native weapon type this weapon fires as. Names decide first, then barrels.
    pub fn weapon_type(&self) -> &'static str {
        if self.barrels == 0 || self.category() == "Melee" {
            return "Sword";
        }
        if let Some(kind) = crate::weapon_type::from_text(&self.leaf()) {
            return kind;
        }
        if self.projectiles_per_shot >= 4 {
            "Shotgun"
        } else if self.charged {
            "Fusion Rifle"
        } else if self.category() == "Turret" {
            "Machine Gun"
        } else if self.rounds_per_second >= 8.0 {
            "Auto Rifle"
        } else {
            "Scout Rifle"
        }
    }

    /// Wheeled and fighter assemblies use Heavy Pike. Tank and hover sources retain their carriers.
    fn summon(&self) -> (&'static str, u32) {
        if self.physics.iter().any(|kind| kind == "human_tank") {
            TANK
        } else if self
            .physics
            .iter()
            .any(|kind| matches!(kind.as_str(), "human_jeep" | "alien_fighter"))
        {
            HEAVY_PIKE
        } else {
            INTERCEPTOR
        }
    }

    fn namespace(&self) -> String {
        format!("parhelion.reach.{}", self.slug)
    }

    /// The authored item hash an import of this entry receives.
    pub fn destination(&self) -> Result<u32> {
        crate::graph::hash(
            &crate::graph::import_identity(&self.namespace())?,
            "item_hash",
        )
    }

    /// A normal recipe on `donor`. Conversion automatically attempts its modeled primary
    /// projectile. A vehicle summons the native vehicle its physics matches.
    pub fn request(&self, donor: u32, donor_name: &str) -> Result<ImportRequest> {
        ensure!(!self.slug.is_empty(), "Reach entry has no slug");
        let vehicle = self.vehicle();
        let mut recipe = json!({
            "schema": 1,
            "kind": if vehicle { "sparrow" } else { "weapon" },
            "namespace": self.namespace(),
            "collection_placement": "sunrise_badge",
            "donor": {"item_hash": format!("0x{donor:08X}"), "expected_name": donor_name},
            "identity": crate::graph::import_identity(&self.namespace())?,
            "name": self.name(),
            "flavor": "Imported from Halo: Reach.",
            "source": "Source: Halo: Reach MCC",
            "overrides": {},
        });
        if vehicle {
            recipe["overrides"]["sparrow"] = json!({"summon": self.summon().0});
        }
        Ok(ImportRequest {
            slug: self.slug.clone(),
            source: ExportRequest {
                cache: self.cache.clone(),
                group: self.group.clone(),
                path: self.path.clone(),
                view: if self.world {
                    View::World
                } else {
                    View::FirstPersonSpartan
                },
                variant: None,
                permutations: BTreeMap::new(),
                audio: None,
            },
            recipe,
            scale: if vehicle {
                super::native::scale::VEHICLE_WORLD
            } else {
                1.
            },
            inspection_scale: if vehicle {
                super::native::scale::VEHICLE_INSPECTION
            } else {
                1.
            },
            runtime_entity: vehicle.then(|| self.summon().1),
            driver_pose: None,
            projectile_entity: None,
            bone_aliases: BTreeMap::new(),
            animation_bindings: Vec::new(),
            first_person: None,
            character: None,
            material_overrides: BTreeMap::new(),
            paint: BTreeMap::new(),
            optic: None,
            sights: None,
            optic_item: None,
            icon: None,
        })
    }
}

fn map_files(maps: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(maps)
        .with_context(|| format!("Reading {}", maps.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("map"))
    });
    paths.sort();
    ensure!(!paths.is_empty(), "No Reach maps in {}", maps.display());
    Ok(paths)
}

/// The maps folder of a chosen folder: the folder itself, or the maps under an MCC
/// installation or its `haloreach` folder.
pub fn maps_path(chosen: &Path) -> Result<PathBuf> {
    [
        chosen.to_path_buf(),
        chosen.join("maps"),
        chosen.join("haloreach").join("maps"),
    ]
    .into_iter()
    .find(|path| map_files(path).is_ok())
    .with_context(|| format!("No Halo: Reach maps in {}", chosen.display()))
}

/// Any added, removed or rewritten map invalidates a stored listing.
fn stamp(maps: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(maps.to_string_lossy().as_bytes());
    hash.update(SCHEMA.to_le_bytes());
    for path in map_files(maps)? {
        let metadata = fs::metadata(&path)?;
        let modified = metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        hash.update(
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_bytes(),
        );
        hash.update([0]);
        hash.update(metadata.len().to_le_bytes());
        hash.update(modified.to_le_bytes());
    }
    Ok(hex::encode(hash.finalize()))
}

/// Every weapon and vehicle path with a render model, named and sorted. `progress`
/// receives the caches read so far and the total.
pub fn browse(maps: &Path, mut progress: impl FnMut(usize, usize)) -> Result<Vec<Entry>> {
    let maps = maps.canonicalize().context("Reach maps folder")?;
    let paths = map_files(&maps)?;
    let mut found: BTreeMap<(String, String), (Entry, usize)> = BTreeMap::new();
    for (index, path) in paths.iter().enumerate() {
        crate::cancellation::check()?;
        progress(index, paths.len());
        let cache = Cache::open(path).with_context(|| format!("Opening {}", path.display()))?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("UTF-8 cache filename")?
            .to_owned();
        let objects: Vec<&Tag> = cache
            .tags
            .iter()
            .flatten()
            .filter(|tag| tag.group == "weap" || tag.group == "vehi")
            .collect();
        let weight = objects.len();
        for tag in objects {
            let object = Object::read(&cache, tag)
                .with_context(|| format!("Reading {} in {name}", tag.path))?;
            let read = Entry::read(&name, tag, &object);
            if !read.world && !read.first_person {
                continue;
            }
            let key = (tag.group.clone(), segments(&tag.path).join("\\"));
            match found.get_mut(&key) {
                Some((entry, best)) => {
                    let mut caches = std::mem::take(&mut entry.caches);
                    caches.push(name.clone());
                    if weight > *best {
                        *entry = read;
                        *best = weight;
                    }
                    entry.caches = caches;
                }
                None => {
                    found.insert(key, (read, weight));
                }
            }
        }
    }
    progress(paths.len(), paths.len());
    let mut entries: Vec<Entry> = found.into_values().map(|(entry, _)| entry).collect();
    // A leaf shared by different paths gets its path's hash, so each identity stays distinct.
    let mut leaves = BTreeMap::<String, usize>::new();
    for entry in &entries {
        *leaves.entry(entry.leaf()).or_default() += 1;
    }
    let mut slugs = BTreeSet::new();
    for entry in &mut entries {
        let leaf = entry.leaf();
        let mut slug: String = leaf
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        if leaves[&leaf] > 1 {
            slug = format!("{slug}-{:08x}", fnv(&segments(&entry.path).join("\\")));
        }
        ensure!(slugs.insert(slug.clone()), "Repeated Reach slug {slug}");
        entry.slug = slug;
    }
    entries.sort_by_key(|entry| (entry.vehicle(), entry.name().to_lowercase()));
    Ok(entries)
}

#[derive(Deserialize, Serialize)]
struct Stored {
    stamp: String,
    entries: Vec<Entry>,
}

/// `browse` through a listing stored in `cache_dir`, read again when a map changes or
/// `refresh` is set.
pub fn browse_cached(
    maps: &Path,
    cache_dir: &Path,
    refresh: bool,
    progress: impl FnMut(usize, usize),
) -> Result<Vec<Entry>> {
    let maps = maps.canonicalize().context("Reach maps folder")?;
    let stamp = stamp(&maps)?;
    let path = cache_dir.join("halo_reach.json");
    crate::cache::take_over(&cache_dir.join("reach-listing.json"), &path);
    if !refresh
        && let Some(stored) = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
            .filter(|stored| stored.stamp == stamp)
    {
        return Ok(stored.entries);
    }
    let entries = browse(&maps, progress)?;
    ensure!(
        !entries.is_empty(),
        "No importable Reach weapons or vehicles"
    );
    // A failed write only costs the next listing a reread.
    let _ = crate::cache::write_json(
        &path,
        &Stored {
            stamp,
            entries: entries.clone(),
        },
    );
    Ok(entries)
}

/// Translate one entry into `output`, which must not exist. Returns the prepared recipe.
pub fn import(
    request: &ImportRequest,
    maps: &Path,
    native_packages: &Path,
    output: &Path,
) -> Result<PathBuf> {
    let parent = output.parent().context("Reach import folder")?;
    fs::create_dir_all(parent)?;
    let mut plan = tempfile::Builder::new()
        .prefix("reach-plan-")
        .suffix(".json")
        .tempfile_in(parent)?;
    std::io::Write::write_all(plan.as_file_mut(), &serde_json::to_vec_pretty(&[request])?)?;
    plan.as_file().sync_all()?;
    let report = super::prepare(plan.path(), maps, native_packages, output)?;
    let recipe = report["items"][0]["recipe"]
        .as_str()
        .context("Reach import produced no recipe")?;
    Ok(PathBuf::from(recipe))
}
