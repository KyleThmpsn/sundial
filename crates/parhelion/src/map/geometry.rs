//! A map's own geometry: another game's level converted to native pieces, placed by the map's own
//! map root chain. The converter writes a graph of tags whose payloads name each other by symbol.
//! Here each node becomes an entry of the map package after the chain, its patches receive the
//! entries they name, and a buffer header's entry names its data.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use parhelion_import::d2_mot::{AudioNamespace, ConvertedAudioBank};
use serde_json::Value;
use sundial::package_authoring::PackageManager;
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult, NewTagSpec, NewTagStorageMode,
    error::invalid,
    extend::{NewTagReference, NewTagReferenceOverride},
    shared_tag_dependency_index::native::Companions,
    tag_payload::{read_u32, write_u32},
};

mod cache;
mod sharing;

pub(super) use sharing::Textures;

use super::{
    MapSource,
    chain::{self, Chain},
};

pub(super) struct Geometry {
    chain: Chain,
    /// The level's conversion, which every area of the map shares.
    shared: Arc<Shared>,
    /// Whether this is the map's first area, whose entries hold the shared graph that the other
    /// areas' chains name.
    first: bool,
    pieces: Vec<Piece>,
    props: Vec<parhelion_import::halo_reach::level::native::Prop>,
    spawns: Vec<parhelion_import::halo_reach::level::spawn::Spawn>,
    objectives: Vec<parhelion_import::halo_reach::level::objectives::Objective>,
    volumes: Vec<parhelion_import::halo_reach::level::volumes::Volume>,
    /// The level's AI squads in this area, with their spawn points in the map's coordinates.
    squads: Vec<parhelion_import::halo_reach::level::squads::Squad>,
    world: parhelion_import::halo_reach::level::native::World,
    /// The preheader of the static instance group that places this area's pieces.
    statics: String,
    /// The symbol of the sound point table that places the level's ambience.
    ambience: Option<String>,
    /// This area's static instances as its tome's Umbra objects, empty without connected areas.
    visibility: Vec<parhelion_import::halo_reach::level::native::Visible>,
    /// The decorator records this area's placement table places, by symbol.
    decorators: Vec<String>,
    /// The graph nodes, by index, that this area's chain loads, or all of them.
    loads: Option<Vec<usize>>,
    /// An exclusive lock on the map's cache folder, held until the stage has read its payloads.
    /// The conversion clears the folder, so a second stage of the same map in another process
    /// waits here rather than removing files the first is still reading.
    _folder_lock: Option<std::fs::File>,
    _generation: Option<parhelion_import::cache::Generation>,
}

/// One conversion of a map's level. A campaign map's areas each place the pieces and collision
/// of what they load from it, and the first area's entries hold its graph once.
struct Shared {
    kept: bool,
    folder: PathBuf,
    nodes: Vec<Value>,
    pieces: Vec<Piece>,
    props: Vec<parhelion_import::halo_reach::level::native::Prop>,
    /// The connected areas, or none when the level is one area.
    areas: Vec<parhelion_import::halo_reach::level::native::Area>,
    spawns: Vec<parhelion_import::halo_reach::level::spawn::Spawn>,
    world: parhelion_import::halo_reach::level::native::World,
    statics: String,
    sky: Vec<parhelion_import::halo_reach::level::native::sky::Object>,
    ambience: Option<String>,
    /// The first area's static instances, which a single-area map's tome lists.
    visibility: Vec<parhelion_import::halo_reach::level::native::Visible>,
    /// The first area's decorator records, which a single-area map places.
    decorators: Vec<String>,
    /// The stock decorator placement every area's decorator rows copy, when the level's
    /// decorators draw as Destiny's instanced decorators.
    decorator_placement: Option<chain::Decorators>,
    /// The symbol of the atmosphere volume the map's atmosphere names.
    atmosphere: Option<String>,
    /// The symbol of the distance lookup the map's atmosphere names.
    atmosphere_lookup: Option<String>,
    /// The symbol of the open image the map's atmosphere names at `+0x9C`.
    atmosphere_image: Option<String>,
    /// The level's occlusion probes, each a position and a sky visibility.
    occlusion: Vec<[f32; 4]>,
    /// The boxes of the level's structure clusters, which the lighting cells follow.
    clusters: Vec<parhelion_import::halo_reach::level::streaming::Bounds>,
    /// The level's cubemap volumes.
    cubemaps: Vec<parhelion_import::halo_reach::level::native::CubemapVolume>,
    /// The level's lights.
    lights: Vec<parhelion_import::halo_reach::level::native::lights::MapLight>,
    light: Option<[f32; 3]>,
    /// The native sun the level asks for, which every area's environment sets.
    native_sun: Option<parhelion_import::halo_reach::level::native::NativeSun>,
    /// The nodes no area's chain reaches, which every area loads.
    everywhere: BTreeSet<usize>,
    source: MapSource,
    /// The donor world and its map root chain as [`Chain::load`] left them, which connected areas
    /// copy under their own names ([`Chain::renamed`]).
    donor: (String, Chain),
}

type Piece = parhelion_import::halo_reach::level::native::Piece;

impl Shared {
    /// The symbols area `area`'s chain names: its pieces' statics, its world collision, ambience
    /// and decorator records, and the sky, atmosphere and cubemaps every area places.
    fn roots<'a>(
        &'a self,
        area: &'a parhelion_import::halo_reach::level::native::Area,
    ) -> Vec<&'a str> {
        area.pieces
            .iter()
            .map(|&piece| self.pieces[piece].mesh.as_str())
            .chain([area.statics.as_str(), area.world.compound.as_str()])
            .chain(area.props.iter().map(|prop| prop.entity.as_str()))
            .chain(area.world.meshes.iter().map(String::as_str))
            .chain(self.sky.iter().map(|object| object.reference.as_str()))
            .chain(area.ambience.as_deref())
            .chain(area.decorators.iter().map(String::as_str))
            .chain(self.atmosphere.as_deref())
            .chain(self.atmosphere_lookup.as_deref())
            .chain(self.atmosphere_image.as_deref())
            .chain(self.cubemaps.iter().map(|volume| volume.texture.as_str()))
            .collect()
    }

    /// The graph nodes `roots` reach through patches, buffer references and audio bindings.
    fn reach<'a>(
        &self,
        roots: impl IntoIterator<Item = &'a str>,
    ) -> AuthoringResult<BTreeSet<usize>> {
        let index = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(at, node)| Some((node["symbol"].as_str()?, at)))
            .collect::<BTreeMap<_, _>>();
        let find = |symbol: &str| {
            index
                .get(symbol)
                .copied()
                .ok_or_else(|| invalid(format!("Map graph names missing symbol {symbol}")))
        };
        let mut pending = roots
            .into_iter()
            .map(find)
            .collect::<AuthoringResult<Vec<_>>>()?;
        let mut reached = BTreeSet::new();
        while let Some(at) = pending.pop() {
            if !reached.insert(at) {
                continue;
            }
            let node = &self.nodes[at];
            let named = node["patches"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|patch| &patch["symbol"])
                .chain([&node["reference"], &node["audio_namespace"]])
                .chain(
                    node["audio_media"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|medium| &medium["symbol"]),
                );
            for symbol in named.filter_map(Value::as_str) {
                pending.push(find(symbol)?);
            }
        }
        Ok(reached)
    }
}

/// The chain's lights for converted `lights` of map `name`, each with an occlusion identifier of
/// its own.
fn map_lights(
    lights: &[parhelion_import::halo_reach::level::native::lights::MapLight],
    name: &str,
) -> Vec<chain::Light> {
    lights
        .iter()
        .enumerate()
        .map(|(index, light)| chain::Light {
            position: light.position,
            range: light.range,
            color: light.color,
            identity: fnv1a64(&format!("parhelion/map/{name}/light/{index}")) as u32 | 1,
        })
        .collect()
}

/// The chain's environment for a level that asks for native sun `sun`, with the sun off in
/// `interiors`. A level without baked light asks for none and keeps the global defaults.
fn map_environment(
    sun: Option<parhelion_import::halo_reach::level::native::NativeSun>,
    interiors: &[parhelion_import::halo_reach::level::native::Interior],
) -> Option<chain::Environment> {
    let sun = sun?;
    Some(chain::Environment {
        sun_intensity: sun.intensity,
        sun_color: sun.color,
        interiors: interiors
            .iter()
            .map(|interior| (interior.minimum, interior.maximum))
            .collect(),
    })
}

fn fnv1a64(text: &str) -> u64 {
    text.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

/// Removes the work folders other maps' stages left in `maps`, and with `cached`, the current
/// map's own, which a conversion read from the area cache leaves unread. Only the stage that
/// converted into a work folder reads it. A later stage restores installed packages, reads the
/// area cache, or clears the folder and converts again. A stage holds its map's lock until its
/// payloads are read, so a folder whose lock is taken stays. The caller holds `current`'s lock.
fn remove_finished_work_folders(maps: &Path, current: &str, cached: bool) {
    if cached {
        let _ = std::fs::remove_dir_all(maps.join(current));
    }
    let Ok(entries) = std::fs::read_dir(maps) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        // Links are not followed. A folder of another kind is not a work folder.
        if name == current || !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(lock) = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(maps.join(format!("{name}.lock")))
        else {
            continue;
        };
        if lock.try_lock().is_ok() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

impl Geometry {
    /// Converts `source` for map `name`, placed by a copy of the map root chain of the donor's
    /// world `world`. With connected `areas`, each a list of the zone sets that load together, the
    /// level converts once for all of them and this is the first area ([`Geometry::area`]).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn load(
        directory: &Path,
        manager: &PackageManager,
        companions: &mut Companions,
        world: &str,
        name: &str,
        source: &MapSource,
        areas: &[Vec<usize>],
        mission_recordings: bool,
        report: &mut dyn FnMut(&str),
    ) -> AuthoringResult<Self> {
        let mission_bindings = source
            .mission_bindings
            .as_ref()
            .map(|path| {
                if source.mission_scripts.is_empty() {
                    return Err(invalid("Mission bindings require selected mission scripts"));
                }
                let bytes = std::fs::read(path).map_err(|error| {
                    invalid(format!(
                        "Could not read mission bindings {}: {error}",
                        path.display()
                    ))
                })?;
                serde_json::from_slice(&bytes).map_err(|error| {
                    invalid(format!(
                        "Invalid mission bindings {}: {error}",
                        path.display()
                    ))
                })
            })
            .transpose()?;
        report("Reading Donor World Templates");
        let mut chain = Chain::load(directory, manager, companions, world, name)?;
        let donor = (world.to_owned(), chain.clone());
        let (world_compound, world_meshes) = chain.world_templates()?;
        let sky = chain.sky_templates(manager)?;
        let atmosphere = chain.atmosphere_volume()?;
        let atmosphere_lookup = chain.atmosphere_lookup()?;
        let atmosphere_image = chain.atmosphere_image()?;
        let occlusion_probes = chain.occlusion_probes()?;
        let cubemaps = chain.cubemaps();
        let maps = sundial::package_authoring::parhelion_data_directory()
            .ok_or_else(|| invalid("Could not locate Parhelion's data folder"))?
            .join("cache")
            .join("maps");
        std::fs::create_dir_all(&maps)
            .map_err(|error| invalid(format!("Could not create {}: {error}", maps.display())))?;
        let lock_path = maps.join(format!("{name}.lock"));
        let folder_lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|error| invalid(format!("Could not open {}: {error}", lock_path.display())))?;
        report("Waiting for Map Cache Lock");
        folder_lock
            .lock()
            .map_err(|error| invalid(format!("Could not lock {}: {error}", lock_path.display())))?;
        let folder = maps.join(name);
        let decoder = source
            .audio_decoder
            .clone()
            .or_else(|| parhelion_import::tools::located(parhelion_import::tools::Tool::Vgmstream));
        let mission_audio = decoder
            .clone()
            .filter(|_| mission_recordings)
            .map(|decoder| {
                let banks = if source.audio_banks.is_empty() {
                    let root = source.maps.join("../fmod/pc");
                    vec![root.join("sfx.fsb"), root.join("english.fsb")]
                } else {
                    source.audio_banks.clone()
                };
                parhelion_import::halo_reach::level::mission::AudioOptions { banks, decoder }
            });
        // The level's decorators draw as Destiny's instanced decorators when the recipe asks and
        // the stock records they copy are present, and otherwise as copies among the pieces.
        let (decorators, decorator_placement) = match source.decorators {
            super::Decorators::Static => (None, None),
            super::Decorators::Native => {
                let found =
                    parhelion_import::halo_reach::level::native::DecoratorTemplates::find(|tag| {
                        manager.get_entry(TagHash(tag)).map(|entry| entry.reference)
                    })
                    .and_then(|templates| {
                        chain::decorators(
                            manager,
                            TagHash(templates.table),
                            &[TagHash(templates.surface), TagHash(templates.depth)],
                        )
                        .map(|placement| (templates, placement))
                        .map_err(|error| error.to_string())
                    });
                match found {
                    Ok((templates, placement)) => (Some(templates), Some(placement)),
                    Err(error) => {
                        report(&format!(
                            "Native Decorators Unavailable, Copying Decorators / {error}"
                        ));
                        (None, None)
                    }
                }
            }
        };
        let templates = parhelion_import::halo_reach::level::native::Templates {
            model: chain.prop().model,
            props: parhelion_import::halo_reach::level::native::PropTemplates {
                entity: chain.prop().entity,
                render: chain.prop().render,
            },
            statics: chain.static_templates(),
            world_compound,
            world_meshes,
            sky,
            // Without a decoder in the recipe, the one chosen in the importer's Tools.
            ambience: chain.sound_points().zip(decoder).map(|(points, decoder)| {
                parhelion_import::halo_reach::level::ambience::Templates { points, decoder }
            }),
            mission_audio,
            mission_execution: (!source.mission_scripts.is_empty()).then(|| {
                parhelion_import::halo_reach::level::mission::execution::Options {
                    entrypoints: source.mission_scripts.clone(),
                    namespace: name.to_owned(),
                    native: mission_bindings,
                }
            }),
            atmosphere,
            atmosphere_lookup,
            atmosphere_image,
            occlusion_probes,
            cubemaps,
            decorators,
        };
        let cached = crate::mod_family::build::map_state(name).map_err(invalid)?;
        let kept = cached.is_some();
        let mut conversion_cache = if kept {
            None
        } else {
            report("Checking Converted Area Cache");
            match cache::Cache::open(
                &maps.join("../map-conversions").join(name),
                source,
                &templates,
                manager,
            ) {
                Ok(cache) => cache,
                Err(error) if parhelion_import::cancellation::is_cancelled(&error) => {
                    return Err(invalid(error.to_string()));
                }
                Err(_) => None,
            }
        };
        remove_finished_work_folders(&maps, name, conversion_cache.is_some());
        let folder = conversion_cache
            .as_ref()
            .map_or(folder, |cache| cache.folder().to_owned());
        let converted: parhelion_import::halo_reach::level::native::Converted =
            if let Some(cached) = cached {
                report("Reusing Verified Map Geometry");
                serde_json::from_value(cached)
                    .map_err(|e| invalid(format!("Kept map metadata: {e}")))?
            } else {
                let reused = conversion_cache
                    .as_mut()
                    .and_then(|cache| cache.converted.take());
                let mut converted = if let Some(converted) = reused {
                    report("Reusing Verified Area Conversion");
                    converted
                } else {
                    if conversion_cache.is_none() && folder.exists() {
                        std::fs::remove_dir_all(&folder).map_err(|error| {
                            invalid(format!("Could not clear {}: {error}", folder.display()))
                        })?;
                    }
                    let converted = parhelion_import::halo_reach::level::native::convert_areas(
                        &source.maps,
                        &source.map,
                        &source.zone_sets,
                        areas,
                        source.variant.as_deref(),
                        directory,
                        &folder,
                        source.offset.map(|v| v as f32),
                        &templates,
                        report,
                    )
                    .map_err(|error| invalid(format!("Map {name} geometry: {error:#}")))?;
                    if let Some(cache) = &conversion_cache {
                        report("Saving Verified Area Conversion");
                        if let Err(error) = cache.publish(&converted, manager) {
                            if parhelion_import::cancellation::is_cancelled(&error) {
                                return Err(invalid(error.to_string()));
                            }
                            report(&format!("Area Cache Unavailable / {error:#}"));
                        }
                    }
                    converted
                };
                report("Identifying Shared Textures");
                sharing::identify(&mut converted.nodes, &folder, manager)?;
                report("Keeping Converted Map Details");
                crate::mod_family::build::remember_map(
                    name,
                    serde_json::to_value(&converted).map_err(|e| invalid(e.to_string()))?,
                )
                .map_err(invalid)?;
                converted
            };
        chain.set_light(converted.light);
        chain.set_lights(map_lights(&converted.lights, name));
        chain.set_environment(map_environment(
            converted.native_sun,
            converted
                .areas
                .first()
                .map_or(&converted.interiors, |area| &area.interiors),
        ));
        chain.set_decorators(decorator_placement.clone());
        if converted.areas.len() != areas.len()
            || converted
                .areas
                .iter()
                .zip(areas)
                .any(|(area, zone_sets)| &area.zone_sets != zone_sets)
        {
            return Err(invalid(format!(
                "Map {name}'s converted areas differ from its connected zone sets"
            )));
        }
        let mut shared = Shared {
            kept,
            folder,
            nodes: converted.nodes,
            pieces: converted.pieces,
            props: converted.props,
            areas: converted.areas,
            spawns: converted.spawns,
            world: converted.world,
            statics: converted.statics,
            sky: converted.sky,
            ambience: converted.ambience,
            visibility: converted.visibility,
            decorators: converted.decorators,
            decorator_placement,
            atmosphere: converted.atmosphere,
            atmosphere_lookup: converted.atmosphere_lookup,
            atmosphere_image: converted.atmosphere_image,
            occlusion: converted.occlusion,
            clusters: converted.clusters,
            cubemaps: converted.cubemaps,
            lights: converted.lights,
            light: converted.light,
            native_sun: converted.native_sun,
            everywhere: BTreeSet::new(),
            source: source.clone(),
            donor,
        };
        // Nodes only an implicit binding reaches, as kept audio banks, load with every area.
        if !shared.areas.is_empty() {
            let reached = shared.reach(shared.areas.iter().flat_map(|area| shared.roots(area)))?;
            shared.everywhere = (0..shared.nodes.len())
                .filter(|at| !reached.contains(at))
                .collect();
        }
        report("Reading Source Spawns, Objectives and Death Boundaries");
        Self::assemble(
            chain,
            Arc::new(shared),
            0,
            name,
            Some(folder_lock),
            conversion_cache.map(cache::Cache::lease),
        )
    }

    /// Connected area `index` of the map whose first area is `self`, named `name` and placed by
    /// its own copy of the map root chain of the donor's world `world`. It names the first area's
    /// graph and writes none of its own.
    pub(super) fn area(
        &self,
        directory: &Path,
        manager: &PackageManager,
        companions: &mut Companions,
        world: &str,
        name: &str,
        index: usize,
    ) -> AuthoringResult<Self> {
        if !self.first || index == 0 || index >= self.shared.areas.len() {
            return Err(invalid(format!("Map area {index} is not a connected area")));
        }
        let mut chain = match &self.shared.donor {
            (donor, chain) if donor == world => chain.renamed(manager, world, name)?,
            _ => Chain::load(directory, manager, companions, world, name)?,
        };
        chain.set_light(self.shared.light);
        chain.set_lights(map_lights(&self.shared.lights, name));
        chain.set_environment(map_environment(
            self.shared.native_sun,
            &self.shared.areas[index].interiors,
        ));
        chain.set_decorators(self.shared.decorator_placement.clone());
        Self::assemble(chain, self.shared.clone(), index, name, None, None)
    }

    /// Area `index` of `shared`, or the whole level when it has no connected areas, with the
    /// objectives and death boundaries its spawns place.
    fn assemble(
        chain: Chain,
        shared: Arc<Shared>,
        index: usize,
        name: &str,
        folder_lock: Option<std::fs::File>,
        generation: Option<parhelion_import::cache::Generation>,
    ) -> AuthoringResult<Self> {
        let (pieces, props, spawns, world, statics, ambience, visibility, loads) =
            match shared.areas.get(index) {
                Some(area) => {
                    let mut loads = shared.reach(shared.roots(area))?;
                    loads.extend(&shared.everywhere);
                    (
                        area.pieces
                            .iter()
                            .map(|&piece| shared.pieces[piece].clone())
                            .collect(),
                        area.props.clone(),
                        area.spawns.clone(),
                        area.world.clone(),
                        area.statics.clone(),
                        area.ambience.clone(),
                        area.visibility.clone(),
                        Some(loads.into_iter().collect()),
                    )
                }
                None => (
                    shared.pieces.clone(),
                    shared.props.clone(),
                    shared.spawns.clone(),
                    shared.world.clone(),
                    shared.statics.clone(),
                    shared.ambience.clone(),
                    shared.visibility.clone(),
                    None,
                ),
            };
        let decorators = shared
            .areas
            .get(index)
            .map_or(&shared.decorators, |area| &area.decorators)
            .clone();
        let source = &shared.source;
        let zone_sets = shared
            .areas
            .get(index)
            .map_or(&source.zone_sets, |area| &area.zone_sets);
        let activity = parhelion_import::halo_reach::level::activity::load(
            &source.maps,
            &source.map,
            source.variant.as_deref(),
            source.offset.map(|v| v as f32),
            &spawns,
            zone_sets,
        )
        .map_err(|error| invalid(format!("Map {name} activity: {error:#}")))?;
        Ok(Self {
            chain,
            first: index == 0,
            pieces,
            props,
            spawns,
            objectives: activity.objectives,
            volumes: activity.volumes,
            squads: activity.squads,
            world,
            statics,
            ambience,
            visibility,
            decorators,
            loads,
            shared,
            _folder_lock: folder_lock,
            _generation: generation,
        })
    }

    pub(super) fn entries(&self) -> usize {
        self.chain.entries() + self.resource_entries()
    }

    pub(super) fn disable_ambience(&mut self) {
        self.ambience = None;
    }
    /// This area's static instances, which its tome lists as Umbra objects.
    pub(super) fn visible(&self) -> &[parhelion_import::halo_reach::level::native::Visible] {
        &self.visibility
    }

    /// The level's player spawn points, in the map's coordinates.
    pub(super) fn spawns(&self) -> &[parhelion_import::halo_reach::level::spawn::Spawn] {
        &self.spawns
    }

    /// The level's labelled objectives, in the map's coordinates.
    pub(super) fn objectives(
        &self,
    ) -> &[parhelion_import::halo_reach::level::objectives::Objective] {
        &self.objectives
    }

    /// The level's kill volumes, in the map's coordinates.
    pub(super) fn volumes(&self) -> &[parhelion_import::halo_reach::level::volumes::Volume] {
        &self.volumes
    }

    /// The level's AI squads in this area, with their spawn points in the map's coordinates.
    pub(super) fn squads(&self) -> &[parhelion_import::halo_reach::level::squads::Squad] {
        &self.squads
    }

    /// The geometry's entries across `ids`, the map root among them, and the entry-header
    /// references its buffer headers need, by ordinal across the whole world.
    pub(super) fn tags(
        &self,
        ids: &[u16],
        name: &str,
    ) -> AuthoringResult<(Vec<NewTagSpec>, TagHash, Vec<NewTagReferenceOverride>)> {
        self.tags_with(
            super::plan::entry_tag(ids, 0)?,
            name,
            &mut Textures::new(),
            None,
            |index| super::plan::entry_tag(ids, self.chain.entries() + index),
        )
    }

    pub(super) fn chain_entries(&self) -> usize {
        self.chain.entries()
    }
    pub(super) fn own_containers(
        &mut self,
        directory: &Path,
        manager: &PackageManager,
        companions: &mut Companions,
    ) -> AuthoringResult<()> {
        self.chain.own_containers(directory, manager, companions)
    }
    /// The graph entries this area writes: the shared graph for the first area, none for the
    /// others, which name the first's.
    pub(super) fn resource_entries(&self) -> usize {
        if self.first {
            self.shared.nodes.len()
        } else {
            0
        }
    }

    pub(super) fn layout(&self) -> &[u8] {
        self.chain.layout()
    }

    pub(super) fn visibility(
        &self,
        manager: &PackageManager,
    ) -> AuthoringResult<(TagHash, Vec<u8>)> {
        self.chain.visibility(manager)
    }

    pub(super) fn bounds(
        &self,
    ) -> AuthoringResult<parhelion_import::halo_reach::level::streaming::Bounds> {
        let mut minimum = [f32::INFINITY; 3];
        let mut maximum = [f32::NEG_INFINITY; 3];
        for piece in &self.pieces {
            for k in 0..3 {
                minimum[k] = minimum[k].min(piece.minimum[k]);
                maximum[k] = maximum[k].max(piece.maximum[k]);
            }
        }
        if (0..3)
            .any(|k| !minimum[k].is_finite() || !maximum[k].is_finite() || minimum[k] >= maximum[k])
        {
            return Err(invalid("Streaming bubble has no finite geometry bounds"));
        }
        Ok(parhelion_import::halo_reach::level::streaming::Bounds { minimum, maximum })
    }

    /// The area's entries from `first`, its graph's entries from `resource_offset` across `ids`,
    /// with its occlusion record assigning probe `clusters[c]` to its spatial tome's cluster `c`.
    pub(super) fn split_tags_at(
        &self,
        first: TagHash,
        ids: &[u16],
        resource_offset: usize,
        name: &str,
        textures: &mut Textures,
        clusters: Option<&[u16]>,
    ) -> AuthoringResult<(Vec<NewTagSpec>, TagHash, Vec<NewTagReferenceOverride>)> {
        self.tags_with(first, name, textures, clusters, |index| {
            super::plan::entry_tag(ids, resource_offset + index)
        })
    }

    /// The level's occlusion probes, each a position in the map's coordinates and a sky
    /// visibility, which the area's occlusion record holds after its first.
    pub(super) fn occlusion(&self) -> &[[f32; 4]] {
        &self.shared.occlusion
    }

    /// The boxes of the level's structure clusters in the map's coordinates, whose faces bound
    /// the area's lighting cells.
    pub(super) fn clusters(&self) -> &[parhelion_import::halo_reach::level::streaming::Bounds] {
        &self.shared.clusters
    }

    pub(super) fn split_tags(
        &self,
        native: u16,
        ids: &[u16],
        name: &str,
    ) -> AuthoringResult<(Vec<NewTagSpec>, TagHash, Vec<NewTagReferenceOverride>)> {
        self.tags_with(
            TagHash::new(native, 0),
            name,
            &mut Textures::new(),
            None,
            |index| super::plan::entry_tag(ids, index),
        )
    }

    fn tags_with(
        &self,
        first: TagHash,
        name: &str,
        textures: &mut Textures,
        clusters: Option<&[u16]>,
        entry: impl Fn(usize) -> AuthoringResult<TagHash>,
    ) -> AuthoringResult<(Vec<NewTagSpec>, TagHash, Vec<NewTagReferenceOverride>)> {
        let base = self.chain.entries();
        if base > crate::appended_tags::MAX_PACKAGE_ENTRY_COUNT {
            return Err(invalid("The map root chain exceeds one package"));
        }
        let mut symbols = BTreeMap::new();
        let nodes = &self.shared.nodes;
        for (index, node) in nodes.iter().enumerate() {
            let symbol = node["symbol"]
                .as_str()
                .ok_or_else(|| invalid("Map graph node has no symbol"))?;
            if symbols.insert(symbol.to_owned(), index).is_some() {
                return Err(invalid(format!("Map graph repeats symbol {symbol}")));
            }
        }
        let aliases = sharing::resolve(nodes, &entry, textures)?;
        let resolved = |index: usize| {
            aliases
                .get(&index)
                .copied()
                .map(Ok)
                .unwrap_or_else(|| entry(index))
        };
        let tag = |symbol: &str| {
            symbols
                .get(symbol)
                .ok_or_else(|| invalid(format!("Map graph names missing symbol {symbol}")))
                .and_then(|&index| resolved(index))
        };
        // Only the first area writes the graph, and the others' chains name its entries.
        let written: &[Value] = if self.first { nodes } else { &[] };
        let mut content = Vec::<NewTagSpec>::with_capacity(written.len());
        let mut references = Vec::new();
        let mut restored = BTreeMap::new();
        for (index, node) in written.iter().enumerate() {
            if self.shared.kept {
                let address = entry(index)?;
                if let std::collections::btree_map::Entry::Vacant(slot) =
                    restored.entry(address.pkg_id())
                {
                    slot.insert(
                        crate::mod_family::build::restore(address.pkg_id()).map_err(invalid)?,
                    );
                }
                let package = &restored[&address.pkg_id()];
                let spec = package
                    .tags
                    .get(usize::from(address.entry_index()))
                    .ok_or_else(|| invalid("Kept map resource entry is missing"))?;
                let mut spec = spec.clone();
                for patch in node["patches"].as_array().into_iter().flatten() {
                    let offset = patch["offset"]
                        .as_u64()
                        .and_then(|offset| usize::try_from(offset).ok())
                        .ok_or_else(|| invalid("Kept map patch has no offset"))?;
                    let symbol = patch["symbol"]
                        .as_str()
                        .ok_or_else(|| invalid("Kept map patch has no symbol"))?;
                    write_u32(&mut spec.payload, offset, tag(symbol)?.0)?;
                }
                content.push(spec);
                for reference in package
                    .references
                    .iter()
                    .filter(|r| r.new_tag_ordinal == usize::from(address.entry_index()))
                {
                    references.push(NewTagReferenceOverride {
                        new_tag_ordinal: base + index,
                        reference: if let Some(symbol) = node["reference"].as_str() {
                            NewTagReference::Tag(tag(symbol)?)
                        } else {
                            match reference.reference {
                                NewTagReference::Appended(at) => {
                                    NewTagReference::Tag(TagHash::new(address.pkg_id(), at as u16))
                                }
                                other => other,
                            }
                        },
                    });
                }
                continue;
            }
            let file = node["file"]
                .as_str()
                .ok_or_else(|| invalid("Map graph node has no payload"))?;
            let mut payload = std::fs::read(self.shared.folder.join(file))
                .map_err(|error| invalid(format!("Map graph payload {file}: {error}")))?;
            for patch in node["patches"].as_array().into_iter().flatten() {
                let offset = patch["offset"]
                    .as_u64()
                    .and_then(|offset| usize::try_from(offset).ok())
                    .ok_or_else(|| invalid("Map graph patch has no offset"))?;
                let symbol = patch["symbol"]
                    .as_str()
                    .ok_or_else(|| invalid("Map graph patch has no symbol"))?;
                if read_u32(&payload, offset)? != u32::MAX {
                    return Err(invalid(format!(
                        "Map graph patch {file}+{offset:#x} does not hold a placeholder"
                    )));
                }
                write_u32(&mut payload, offset, tag(symbol)?.0)?;
            }
            if let Some(reference) = node["reference"].as_str() {
                references.push(NewTagReferenceOverride {
                    new_tag_ordinal: base + index,
                    reference: NewTagReference::Tag(tag(reference)?),
                });
            }
            let template = node["template"]
                .as_u64()
                .and_then(|template| u32::try_from(template).ok())
                .ok_or_else(|| invalid("Map graph node has no template"))?;
            let storage = match node["storage"].as_str() {
                None => NewTagStorageMode::InheritTemplate,
                Some("audio_bank") => NewTagStorageMode::AudioBank,
                // A medium's entry reference is its own tag, the media ID banks name it by.
                Some("audio_media") => {
                    references.push(NewTagReferenceOverride {
                        new_tag_ordinal: base + index,
                        reference: NewTagReference::Tag(entry(index)?),
                    });
                    NewTagStorageMode::AudioMedia
                }
                Some(other) => {
                    return Err(invalid(format!(
                        "Map graph node {file} has storage {other}"
                    )));
                }
            };
            content.push(NewTagSpec {
                template_tag: TagHash(template),
                payload,
                storage,
            });
        }
        if self.first && !self.shared.kept {
            bind_audio(nodes, &mut content, &tag, name)?;
        }
        let pieces = self
            .pieces
            .iter()
            .map(|piece| chain::Piece {
                minimum: piece.minimum,
                maximum: piece.maximum,
            })
            .collect::<Vec<_>>();
        let statics = tag(&self.statics)?;
        let props = self
            .props
            .iter()
            .map(|prop| Ok((tag(&prop.entity)?, prop.position)))
            .collect::<AuthoringResult<Vec<_>>>()?;
        let decorators = self
            .decorators
            .iter()
            .map(|record| tag(record))
            .collect::<AuthoringResult<Vec<_>>>()?;
        let loads = self
            .loads
            .clone()
            .unwrap_or_else(|| (0..nodes.len()).collect())
            .into_iter()
            .map(&resolved)
            .collect::<AuthoringResult<std::collections::BTreeSet<_>>>()?
            .into_iter()
            .collect::<Vec<_>>();
        let meshes = self
            .world
            .meshes
            .iter()
            .map(|symbol| tag(symbol.as_str()))
            .collect::<AuthoringResult<Vec<_>>>()?;
        let world = chain::World {
            compound: tag(&self.world.compound)?,
            meshes: &meshes,
        };
        let sky = self
            .shared
            .sky
            .iter()
            .map(|object| {
                Ok(chain::SkyObject {
                    reference: tag(&object.reference)?,
                    position: object.position,
                    minimum: object.minimum,
                    maximum: object.maximum,
                    order: object.order,
                })
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        let sounds = self.ambience.as_deref().map(tag).transpose()?;
        let atmosphere = self.shared.atmosphere.as_deref().map(tag).transpose()?;
        let lookup = self
            .shared
            .atmosphere_lookup
            .as_deref()
            .map(tag)
            .transpose()?;
        let image = self
            .shared
            .atmosphere_image
            .as_deref()
            .map(tag)
            .transpose()?;
        let cubemaps = self
            .shared
            .cubemaps
            .iter()
            .enumerate()
            .map(|(index, volume)| {
                Ok(chain::Cubemap {
                    texture: tag(&volume.texture)?,
                    center: volume.center,
                    extents: volume.extents,
                    capture: volume.capture,
                    identity: fnv1a64(&format!("parhelion/map/{name}/cubemap/{index}")) | 1,
                })
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        let (mut tags, root) = self.chain.tags(
            first,
            statics,
            &pieces,
            &props,
            &decorators,
            &self.spawns,
            &world,
            &sky,
            sounds,
            atmosphere,
            lookup,
            image,
            &self.shared.occlusion,
            clusters,
            &cubemaps,
            &loads,
        )?;
        tags.extend(content);
        Ok((tags, root, references))
    }
}

/// Gives each graph bank in `content` private Wwise identities of its own, names its media by
/// their tags, and points each sound record at its bank's play and stop events. A bank node holds
/// its converted bank (`audio_bank`) and the media symbols of its source media IDs
/// (`audio_media`), and a record its event fields (`audio_events`) and bank (`audio_namespace`).
fn bind_audio(
    nodes: &[Value],
    content: &mut [NewTagSpec],
    tag: &impl Fn(&str) -> AuthoringResult<TagHash>,
    name: &str,
) -> AuthoringResult<()> {
    let index = |symbol: &str| {
        nodes
            .iter()
            .position(|node| node["symbol"] == symbol)
            .ok_or_else(|| invalid(format!("Map graph names missing symbol {symbol}")))
    };
    let mut namespaces = BTreeMap::new();
    for (at, node) in nodes.iter().enumerate() {
        if node["audio_bank"].is_null() {
            continue;
        }
        let symbol = node["symbol"].as_str().unwrap_or_default();
        let bank: ConvertedAudioBank = serde_json::from_value(node["audio_bank"].clone())
            .map_err(|error| invalid(format!("Map audio bank {symbol}: {error}")))?;
        if content[at].storage != NewTagStorageMode::AudioBank || content[at].payload != bank.bytes
        {
            return Err(invalid(format!(
                "Map audio bank {symbol} differs from its payload"
            )));
        }
        let mut media = BTreeMap::new();
        let mut sizes = BTreeMap::new();
        for row in node["audio_media"].as_array().into_iter().flatten() {
            let (Some(id), Some(medium)) = (
                row["id"].as_u64().and_then(|id| u32::try_from(id).ok()),
                row["symbol"].as_str(),
            ) else {
                return Err(invalid(format!(
                    "Map audio bank {symbol} lists a bad medium"
                )));
            };
            let spec = &content[index(medium)?];
            if spec.storage != NewTagStorageMode::AudioMedia {
                return Err(invalid(format!("Map audio medium {medium} is not media")));
            }
            media.insert(id, tag(medium)?.0);
            sizes.insert(
                id,
                u32::try_from(spec.payload.len())
                    .map_err(|_| invalid(format!("Map audio medium {medium} is too large")))?,
            );
        }
        // Every bank was written with the same local identities, so each takes its own space.
        let namespace = AudioNamespace::new(
            &format!("parhelion/map/{name}/{symbol}"),
            std::slice::from_ref(&bank),
        )
        .map_err(|error| invalid(format!("Map audio bank {symbol}: {error:#}")))?;
        content[at].payload = namespace
            .instantiate(&bank, &media, &sizes)
            .map_err(|error| invalid(format!("Map audio bank {symbol}: {error:#}")))?;
        namespaces.insert(symbol.to_owned(), namespace);
    }
    for (at, node) in nodes.iter().enumerate() {
        let Some(events) = node["audio_events"].as_array() else {
            continue;
        };
        let bank = node["audio_namespace"].as_str().unwrap_or_default();
        let namespace = namespaces
            .get(bank)
            .ok_or_else(|| invalid(format!("Map audio events name no bank {bank}")))?;
        for event in events {
            let (Some(offset), Some(id)) = (
                event["offset"]
                    .as_u64()
                    .and_then(|offset| usize::try_from(offset).ok()),
                event["id"].as_u64().and_then(|id| u32::try_from(id).ok()),
            ) else {
                return Err(invalid("Map audio event is malformed"));
            };
            if read_u32(&content[at].payload, offset)? != id {
                return Err(invalid("Map audio event field changed"));
            }
            let private = namespace
                .object(id)
                .map_err(|error| invalid(format!("Map audio event: {error:#}")))?;
            write_u32(&mut content[at].payload, offset, private)?;
        }
    }
    Ok(())
}
