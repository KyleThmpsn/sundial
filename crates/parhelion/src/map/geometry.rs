//! A map's own geometry: another game's level converted to native pieces, placed by the map's own
//! map root chain. The converter writes a graph of tags whose payloads name each other by symbol.
//! Here each node becomes an entry of the map package after the chain, its patches receive the
//! entries they name, and a buffer header's entry names its data.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
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

use super::{
    MapSource,
    chain::{self, Chain},
};

pub(super) struct Geometry {
    chain: Chain,
    kept: bool,
    folder: PathBuf,
    nodes: Vec<Value>,
    pieces: Vec<parhelion_import::halo_reach::level::native::Piece>,
    spawns: Vec<parhelion_import::halo_reach::level::spawn::Spawn>,
    objectives: Vec<parhelion_import::halo_reach::level::objectives::Objective>,
    volumes: Vec<parhelion_import::halo_reach::level::volumes::Volume>,
    world: parhelion_import::halo_reach::level::native::World,
    sky: Vec<parhelion_import::halo_reach::level::native::sky::Object>,
    /// The symbol of the sound point table that places the level's ambience.
    ambience: Option<String>,
    /// The symbol of the atmosphere volume the map's atmosphere names.
    atmosphere: Option<String>,
    /// The symbol of the distance lookup the map's atmosphere names.
    atmosphere_lookup: Option<String>,
    /// The level's occlusion probes, each a position and a sky visibility.
    occlusion: Vec<[f32; 4]>,
    /// The level's cubemap volumes.
    cubemaps: Vec<parhelion_import::halo_reach::level::native::CubemapVolume>,
    /// An exclusive lock on the map's cache folder, held until the stage has read its payloads.
    /// The conversion clears the folder, so a second stage of the same map in another process
    /// waits here rather than removing files the first is still reading.
    _folder_lock: std::fs::File,
}

fn fnv1a64(text: &str) -> u64 {
    text.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

impl Geometry {
    /// Converts `source` for map `name`, placed by a copy of the map root chain of the donor's
    /// world `world`.
    pub(super) fn load(
        directory: &Path,
        manager: &PackageManager,
        companions: &mut Companions,
        world: &str,
        name: &str,
        source: &MapSource,
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
        let (world_compound, world_meshes) = chain.world_templates()?;
        let sky = chain.sky_templates(manager)?;
        let atmosphere = chain.atmosphere_volume()?;
        let atmosphere_lookup = chain.atmosphere_lookup()?;
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
        let mission_audio = decoder.clone().map(|decoder| {
            let banks = if source.audio_banks.is_empty() {
                let root = source.maps.join("../fmod/pc");
                vec![root.join("sfx.fsb"), root.join("english.fsb")]
            } else {
                source.audio_banks.clone()
            };
            parhelion_import::halo_reach::level::mission::AudioOptions { banks, decoder }
        });
        let cached = crate::mod_family::build::map_state(name).map_err(invalid)?;
        let kept = cached.is_some();
        let converted: parhelion_import::halo_reach::level::native::Converted =
            if let Some(cached) = cached {
                report("Reusing Verified Map Geometry");
                serde_json::from_value(cached)
                    .map_err(|e| invalid(format!("Kept map metadata: {e}")))?
            } else {
                if folder.exists() {
                    std::fs::remove_dir_all(&folder).map_err(|error| {
                        invalid(format!("Could not clear {}: {error}", folder.display()))
                    })?;
                }
                let converted = parhelion_import::halo_reach::level::native::convert_with_progress(
                    &source.maps,
                    &source.map,
                    &source.zone_sets,
                    source.variant.as_deref(),
                    directory,
                    &folder,
                    source.offset.map(|v| v as f32),
                    &parhelion_import::halo_reach::level::native::Templates {
                        entity: chain.prop().entity,
                        render: chain.prop().render,
                        body: chain.prop().body,
                        model: chain.prop().model,
                        collision: chain.prop().collision,
                        world_compound,
                        world_meshes,
                        sky,
                        // Without a decoder in the recipe, the one chosen in the importer's Tools.
                        ambience: chain.sound_points().zip(decoder).map(|(points, decoder)| {
                            parhelion_import::halo_reach::level::ambience::Templates {
                                points,
                                decoder,
                            }
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
                        occlusion_probes,
                        cubemaps,
                    },
                    report,
                )
                .map_err(|error| invalid(format!("Map {name} geometry: {error:#}")))?;
                crate::mod_family::build::remember_map(
                    name,
                    serde_json::to_value(&converted).map_err(|e| invalid(e.to_string()))?,
                )
                .map_err(invalid)?;
                converted
            };
        chain.set_light(converted.light);
        report("Reading Source Spawns, Objectives and Death Boundaries");
        let activity = parhelion_import::halo_reach::level::activity::load(
            &source.maps,
            &source.map,
            source.variant.as_deref(),
            source.offset.map(|v| v as f32),
            &converted.spawns,
        )
        .map_err(|error| invalid(format!("Map {name} activity: {error:#}")))?;
        Ok(Self {
            chain,
            kept,
            folder,
            nodes: converted.nodes,
            pieces: converted.pieces,
            spawns: converted.spawns,
            objectives: activity.objectives,
            volumes: activity.volumes,
            world: converted.world,
            sky: converted.sky,
            ambience: converted.ambience,
            atmosphere: converted.atmosphere,
            atmosphere_lookup: converted.atmosphere_lookup,
            occlusion: converted.occlusion,
            cubemaps: converted.cubemaps,
            _folder_lock: folder_lock,
        })
    }

    pub(super) fn entries(&self) -> usize {
        self.chain.entries() + self.nodes.len()
    }

    pub(super) fn disable_ambience(&mut self) {
        self.ambience = None;
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

    /// The geometry's entries across `ids`, the map root among them, and the entry-header
    /// references its buffer headers need, by ordinal across the whole world.
    pub(super) fn tags(
        &self,
        ids: &[u16],
        name: &str,
    ) -> AuthoringResult<(Vec<NewTagSpec>, TagHash, Vec<NewTagReferenceOverride>)> {
        self.tags_with(super::plan::entry_tag(ids, 0)?, name, |index| {
            super::plan::entry_tag(ids, self.chain.entries() + index)
        })
    }

    pub(super) fn chain_entries(&self) -> usize {
        self.chain.entries()
    }
    pub(super) fn resource_entries(&self) -> usize {
        self.nodes.len()
    }

    pub(super) fn split_tags(
        &self,
        native: u16,
        ids: &[u16],
        name: &str,
    ) -> AuthoringResult<(Vec<NewTagSpec>, TagHash, Vec<NewTagReferenceOverride>)> {
        self.tags_with(TagHash::new(native, 0), name, |index| {
            super::plan::entry_tag(ids, index)
        })
    }

    fn tags_with(
        &self,
        first: TagHash,
        name: &str,
        entry: impl Fn(usize) -> AuthoringResult<TagHash>,
    ) -> AuthoringResult<(Vec<NewTagSpec>, TagHash, Vec<NewTagReferenceOverride>)> {
        let base = self.chain.entries();
        if base > crate::appended_tags::MAX_PACKAGE_ENTRY_COUNT {
            return Err(invalid("The map root chain exceeds one package"));
        }
        let mut symbols = BTreeMap::new();
        for (index, node) in self.nodes.iter().enumerate() {
            let symbol = node["symbol"]
                .as_str()
                .ok_or_else(|| invalid("Map graph node has no symbol"))?;
            if symbols.insert(symbol.to_owned(), index).is_some() {
                return Err(invalid(format!("Map graph repeats symbol {symbol}")));
            }
        }
        let tag = |symbol: &str| {
            symbols
                .get(symbol)
                .ok_or_else(|| invalid(format!("Map graph names missing symbol {symbol}")))
                .and_then(|&index| entry(index))
        };
        let mut content = Vec::<NewTagSpec>::with_capacity(self.nodes.len());
        let mut references = Vec::new();
        let mut restored = BTreeMap::new();
        for (index, node) in self.nodes.iter().enumerate() {
            if self.kept {
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
                content.push(spec.clone());
                for reference in package
                    .references
                    .iter()
                    .filter(|r| r.new_tag_ordinal == usize::from(address.entry_index()))
                {
                    references.push(NewTagReferenceOverride {
                        new_tag_ordinal: base + index,
                        reference: match reference.reference {
                            NewTagReference::Appended(at) => {
                                NewTagReference::Tag(TagHash::new(address.pkg_id(), at as u16))
                            }
                            other => other,
                        },
                    });
                }
                continue;
            }
            let file = node["file"]
                .as_str()
                .ok_or_else(|| invalid("Map graph node has no payload"))?;
            let mut payload = std::fs::read(self.folder.join(file))
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
                let target = *symbols.get(reference).ok_or_else(|| {
                    invalid(format!("Map graph names missing symbol {reference}"))
                })?;
                references.push(NewTagReferenceOverride {
                    new_tag_ordinal: base + index,
                    reference: NewTagReference::Tag(entry(target)?),
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
        if !self.kept {
            bind_audio(&self.nodes, &mut content, &tag, name)?;
        }
        let pieces = self
            .pieces
            .iter()
            .enumerate()
            .map(|(index, piece)| {
                Ok(chain::Piece {
                    entity: tag(&piece.entity)?,
                    identity: fnv1a64(&format!("parhelion/map/{name}/piece/{index}")) | 1,
                    position: piece.position,
                    minimum: piece.minimum,
                    maximum: piece.maximum,
                })
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        let loads = (0..self.nodes.len())
            .map(&entry)
            .collect::<AuthoringResult<Vec<_>>>()?;
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
        let atmosphere = self.atmosphere.as_deref().map(tag).transpose()?;
        let lookup = self.atmosphere_lookup.as_deref().map(tag).transpose()?;
        let cubemaps = self
            .cubemaps
            .iter()
            .enumerate()
            .map(|(index, volume)| {
                Ok(chain::Cubemap {
                    texture: tag(&volume.texture)?,
                    center: volume.center,
                    extents: volume.extents,
                    capture: volume.capture,
                    priority: volume.priority,
                    identity: fnv1a64(&format!("parhelion/map/{name}/cubemap/{index}")) | 1,
                })
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        let (mut tags, root) = self.chain.tags(
            first,
            &pieces,
            &self.spawns,
            &world,
            &sky,
            sounds,
            atmosphere,
            lookup,
            &self.occlusion,
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
