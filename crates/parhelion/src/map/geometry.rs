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
    /// Converts `source` for map `name`, placed by a copy of `donor`'s map root chain.
    pub(super) fn load(
        directory: &Path,
        manager: &PackageManager,
        companions: &mut Companions,
        donor: &str,
        name: &str,
        source: &MapSource,
    ) -> AuthoringResult<Self> {
        let mut chain = Chain::load(directory, manager, companions, donor, name)?;
        let (world_compound, world_mesh) = chain.world_templates()?;
        let sky = chain.sky_templates(manager)?;
        let atmosphere = chain.atmosphere_volume()?;
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
        folder_lock
            .lock()
            .map_err(|error| invalid(format!("Could not lock {}: {error}", lock_path.display())))?;
        let folder = maps.join(name);
        if folder.exists() {
            std::fs::remove_dir_all(&folder).map_err(|error| {
                invalid(format!("Could not clear {}: {error}", folder.display()))
            })?;
        }
        let converted = parhelion_import::halo_reach::level::native::convert(
            &source.maps,
            &source.map,
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
                world_mesh,
                sky,
                ambience: chain.sound_points().zip(source.audio_decoder.clone()).map(
                    |(points, decoder)| parhelion_import::halo_reach::level::ambience::Templates {
                        points,
                        decoder,
                    },
                ),
                atmosphere,
            },
        )
        .map_err(|error| invalid(format!("Map {name} geometry: {error:#}")))?;
        chain.set_light(converted.light);
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
            _folder_lock: folder_lock,
        })
    }

    pub(super) fn entries(&self) -> usize {
        self.chain.entries() + self.nodes.len()
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

    /// The geometry's entries starting at `first`, the map root among them, and the entry-header
    /// references its buffer headers need, by entry ordinal in the package.
    pub(super) fn tags(
        &self,
        first: TagHash,
        name: &str,
    ) -> AuthoringResult<(Vec<NewTagSpec>, TagHash, Vec<NewTagReferenceOverride>)> {
        let base = first.entry_index() as usize + self.chain.entries();
        let entry = |index: usize| {
            u16::try_from(base + index)
                .map(|entry| TagHash::new(first.pkg_id(), entry))
                .map_err(|_| invalid("The map package has too many entries"))
        };
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
        for (index, node) in self.nodes.iter().enumerate() {
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
                    reference: NewTagReference::Appended(base + target),
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
                        reference: NewTagReference::Appended(base + index),
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
        bind_audio(&self.nodes, &mut content, &tag, name)?;
        let pieces = self
            .pieces
            .iter()
            .enumerate()
            .map(|(index, piece)| {
                Ok(chain::Piece {
                    entity: tag(&piece.entity)?,
                    identity: fnv1a64(&format!("parhelion/map/{name}/piece/{index}")) | 1,
                    position: piece.position,
                })
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        let loads = (0..self.nodes.len())
            .map(entry)
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
        let (mut tags, root) = self.chain.tags(
            first,
            &pieces,
            &self.spawns,
            &world,
            &sky,
            sounds,
            atmosphere,
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
