//! A map's own activity root and scenario. Each is a copy of the donor's type-16 owner followed
//! by its shared-tag companion, which lists what loads with it. The names that select them live
//! in the same package as the tags, so a reader that trusts only a tag's own package for its
//! names, as Sunrise's activity inventory does, finds the map under its own name. The scenario
//! still names the donor's world, unless the map has geometry: then its slice set, object
//! registry and map root chain are the map's own, and its bubble takes the map's name.
use std::path::Path;

use sundial::package_authoring::PackageManager;
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult, NewTagSpec, NewTagStorageMode,
    error::invalid,
    format::NamedTag,
    shared_tag_dependency_index::{
        enroll_dependencies, enroll_inherited_dependencies,
        native::{Companions, native_companion},
        remove_dependencies, replace_owner_dependency,
    },
    tag_payload::{
        append_native_array, array_at, read_tag, read_u32, read_u64, synchronize_payload_size,
        write_u32, write_u64,
    },
};

const ACTIVITY_ROOT_CLASS: u32 = 0x8080_8AAE;
const SCENARIO_CLASS: u32 = 0x8080_9994;
const MAP_ROOT_CLASS: u32 = 0x8080_91DE;
/// A scenario's bubble states name a slice set, which names the world's map root by activity name.
const SLICE_SET_CLASS: u32 = 0x8080_925B;
/// The slice set's self-relative pointer to its counted name.
const SLICE_SET_NAME: usize = 0x08;
/// The slice set's object registry, which places the activity's scripted objects.
const SLICE_SET_REGISTRY: usize = 0x14;
/// The activity root names its scenario here.
const ROOT_SCENARIO: usize = 0x40;
/// A companion names itself, then its owner.
const COMPANION_SELF: usize = 0x08;
const COMPANION_OWNER: usize = 0x0C;
const SCENARIO_SUFFIX: &str = ":scenario_client";
/// Entries every map takes: the root, its companion, the scenario and its companion.
const ENTRIES: usize = 4;

/// Shadowkeep's summon gate reads this byte from the selected bubble state. Other summon
/// restrictions, including combat and collision regions, are checked independently.
fn set_vehicle_summoning(payload: &mut [u8], allowed: bool) -> AuthoringResult<()> {
    fn rows(
        payload: &[u8],
        descriptor: usize,
        expected: u32,
        stride: usize,
    ) -> AuthoringResult<std::ops::Range<usize>> {
        let (count, _, start, class) = crate::tag_payload::array_at(payload, descriptor)?;
        let end = count
            .checked_mul(stride)
            .and_then(|span| start.checked_add(span))
            .filter(|&end| end <= payload.len())
            .ok_or_else(|| invalid("Scenario state array leaves its payload"))?;
        if class != expected {
            return Err(invalid(format!(
                "Scenario array at {descriptor:#X} has class {class:08X}, expected {expected:08X}"
            )));
        }
        Ok(start..end)
    }
    for bubble in rows(payload, 0x50, 0x8080_924D, 24)?.step_by(24) {
        for state in rows(payload, bubble + 8, 0x8080_924F, 0x4C)?.step_by(0x4C) {
            payload[state + 0x48] = u8::from(!allowed);
        }
    }
    Ok(())
}

/// A donor type-16 owner and its companion.
pub(super) struct Owner {
    pub(super) tag: TagHash,
    pub(super) payload: Vec<u8>,
    companion: TagHash,
    companion_payload: Vec<u8>,
}

impl Owner {
    fn load(
        directory: &Path,
        manager: &PackageManager,
        companions: &mut Companions,
        name: &str,
        class: u32,
    ) -> AuthoringResult<Self> {
        let (tag, _) = super::donor_tag(manager, name)?;
        Self::load_tag(directory, manager, companions, tag, class, name)
    }

    /// The owner `tag`, which must be of `class`. `label` names it in errors.
    pub(super) fn load_tag(
        directory: &Path,
        manager: &PackageManager,
        companions: &mut Companions,
        tag: TagHash,
        class: u32,
        label: &str,
    ) -> AuthoringResult<Self> {
        let found = manager
            .get_entry(tag)
            .map(|entry| entry.reference)
            .ok_or_else(|| invalid(format!("Donor {label} {tag} has no entry")))?;
        if found != class {
            return Err(invalid(format!(
                "Donor {label} is class {found:08X}, expected {class:08X}"
            )));
        }
        let (companion, companion_payload) =
            native_companion(directory, manager, tag.0, companions)?.clone();
        Ok(Self {
            tag,
            payload: read_tag(manager, tag, label)?,
            companion,
            companion_payload,
        })
    }

    /// The companion's template tag.
    pub(super) fn companion_template(&self) -> TagHash {
        self.companion
    }

    /// The companion re-identified for `owner` at `companion` that also loads `additions`.
    pub(super) fn companion_with(
        &self,
        companion: TagHash,
        owner: TagHash,
        additions: &[TagHash],
    ) -> AuthoringResult<Vec<u8>> {
        let payload = self.companion(companion, owner).map_err(|error| {
            invalid(format!(
                "Companion {} of donor {}: {error}",
                self.companion, self.tag
            ))
        })?;
        enroll_dependencies(&payload, companion, owner, additions).map_err(|error| {
            invalid(format!(
                "Companion {} of donor {} with additions: {error}",
                self.companion, self.tag
            ))
        })
    }

    /// The companion re-identified for `owner` at `companion`. It lists `owner` and itself where
    /// the source listed the donor owner and the donor companion, and keeps every other entry.
    fn companion(&self, companion: TagHash, owner: TagHash) -> AuthoringResult<Vec<u8>> {
        let mut payload = self.companion_payload.clone();
        if read_u32(&payload, COMPANION_SELF)? != self.companion.0
            || read_u32(&payload, COMPANION_OWNER)? != self.tag.0
        {
            return Err(invalid(format!(
                "Companion {} does not name itself and {}",
                self.companion, self.tag
            )));
        }
        write_u32(&mut payload, COMPANION_SELF, companion.0)?;
        write_u32(&mut payload, COMPANION_OWNER, owner.0)?;
        let payload = replace_owner_dependency(&payload, companion, owner, self.tag)?;
        replace_dependencies(&payload, companion, owner, &[self.companion], &[companion])
    }
}

/// A world's bubble names itself by the FNV-1 hash of its world's name, which Sunrise and the
/// client read back as the bubble's name. A Crucible map's world takes its activity's name, while
/// a mission's has its own: The Other Side (`mission_proton`) plays in `trophy_hall`. Distant
/// Shore's carries it in its bubble parent, the map root's bubble layout, the activity root, the
/// scenario's bubble rows and states, and two registry objects. A map with its own world gives
/// each copy the map's name.
#[derive(Clone, Copy)]
pub(super) struct BubbleName {
    donor: [u8; 4],
    map: [u8; 4],
}

impl BubbleName {
    pub(super) fn new(donor: &str, name: &str) -> Self {
        let hash = |name: &str| sundial::package_authoring::fnv1_name_hash(name).to_le_bytes();
        Self {
            donor: hash(donor),
            map: hash(name),
        }
    }

    /// Whether a word of `payload` names the donor's bubble.
    pub(super) fn named(self, payload: &[u8]) -> bool {
        payload.chunks_exact(4).any(|word| word == self.donor)
    }

    /// Renames the donor's bubble to the map's in every word of `payload`, and counts them.
    pub(super) fn rename(self, payload: &mut [u8]) -> usize {
        let mut renamed = 0;
        for word in payload.chunks_exact_mut(4) {
            if *word == self.donor {
                word.copy_from_slice(&self.map);
                renamed += 1;
            }
        }
        renamed
    }
}

/// `payload` naming `name` where its counted name at `field` named `donor`. The name ends the
/// payload, and `label` names the record in errors.
pub(super) fn renamed_counted(
    payload: &[u8],
    field: usize,
    donor: &str,
    name: &str,
    label: &str,
) -> AuthoringResult<Vec<u8>> {
    let mut payload = payload.to_vec();
    let relative = read_u64(&payload, field)?;
    let start = usize::try_from(relative)
        .ok()
        .and_then(|relative| field.checked_add(relative))
        .filter(|&start| start >= 8)
        .ok_or_else(|| invalid(format!("The donor {label}'s name is outside it")))?;
    let donor = format!("{donor}\0");
    if payload.get(start..) != Some(donor.as_bytes())
        || read_u64(&payload, start - 8)? != donor.len() as u64
    {
        return Err(invalid(format!(
            "The donor {label} does not end with its activity's counted name"
        )));
    }
    payload.truncate(start);
    payload.extend_from_slice(name.as_bytes());
    payload.push(0);
    write_u64(&mut payload, start - 8, name.len() as u64 + 1)?;
    synchronize_payload_size(payload)
}

/// `payload` listing `copies` in place of `sources`, which it need not list.
fn replace_dependencies(
    payload: &[u8],
    companion: TagHash,
    owner: TagHash,
    sources: &[TagHash],
    copies: &[TagHash],
) -> AuthoringResult<Vec<u8>> {
    let payload = remove_dependencies(payload, companion, owner, sources)?;
    enroll_dependencies(&payload, companion, owner, copies)
}

/// A donor slice set and the scenario offsets that name it.
struct SliceSet {
    tag: TagHash,
    payload: Vec<u8>,
    fields: Vec<usize>,
}

impl SliceSet {
    /// The one slice set `scenario` names.
    fn load(manager: &PackageManager, scenario: &Owner) -> AuthoringResult<Self> {
        let mut found = Vec::new();
        for field in (0..scenario.payload.len().saturating_sub(3)).step_by(4) {
            let tag = TagHash(read_u32(&scenario.payload, field)?);
            if manager.get_entry(tag).map(|entry| entry.reference) == Some(SLICE_SET_CLASS) {
                found.push((tag, field));
            }
        }
        let tag = found
            .first()
            .map(|&(tag, _)| tag)
            .filter(|&tag| found.iter().all(|&(other, _)| other == tag))
            .ok_or_else(|| invalid("The donor scenario does not name exactly one slice set"))?;
        Ok(Self {
            tag,
            payload: read_tag(manager, tag, "slice set")?,
            fields: found.into_iter().map(|(_, field)| field).collect(),
        })
    }

    /// The name of the world the slice set loads, its counted name, which selects the world's map
    /// root `map:<world>:root`.
    fn world(&self) -> AuthoringResult<String> {
        let relative = read_u64(&self.payload, SLICE_SET_NAME)?;
        let start = usize::try_from(relative)
            .ok()
            .and_then(|relative| SLICE_SET_NAME.checked_add(relative))
            .filter(|&start| start >= 8)
            .ok_or_else(|| invalid("The donor slice set's name is outside it"))?;
        let length = usize::try_from(read_u64(&self.payload, start - 8)?)
            .map_err(|_| invalid("The donor slice set's name length is invalid"))?;
        let bytes = self
            .payload
            .get(start..start.saturating_add(length))
            .filter(|bytes| bytes.last() == Some(&0))
            .ok_or_else(|| invalid("The donor slice set's name is not a counted string"))?;
        String::from_utf8(bytes[..bytes.len() - 1].to_vec())
            .map_err(|_| invalid("The donor slice set's name is not text"))
    }

    /// The slice set naming `name` where it named its world `world`. Its counted name ends the
    /// payload.
    fn renamed(&self, world: &str, name: &str) -> AuthoringResult<Vec<u8>> {
        renamed_counted(&self.payload, SLICE_SET_NAME, world, name, "slice set")
    }
}

/// One map's activity root and scenario, and its own geometry when it has a source. A map with
/// geometry also owns a copy of the donor's slice set, which names the map so that its world
/// loads the map's own map root, and names the map's own object registry.
pub(super) struct World {
    name: String,
    root: Owner,
    scenario: Owner,
    donor: String,
    /// The name of the donor's world, which its map root and bubble take. A Crucible donor's is
    /// its activity's name.
    world: String,
    slice_set: Option<SliceSet>,
    registry: Option<super::registry::Registry>,
    geometry: Option<super::geometry::Geometry>,
    extra_geometry: Vec<super::geometry::Geometry>,
    extra_registries: Vec<super::registry::Registry>,
    streaming: Option<super::streaming::Layout>,
    vehicle_summoning: Option<bool>,
}

impl World {
    pub(super) fn encounters(&self) -> Vec<(usize, super::mission::Encounter)> {
        self.registry
            .iter()
            .chain(&self.extra_registries)
            .enumerate()
            .filter_map(|(area, registry)| registry.encounter().map(|encounter| (area, encounter)))
            .collect()
    }

    pub(super) fn triggers(&self) -> Vec<(usize, super::mission::Triggers)> {
        self.registry
            .iter()
            .chain(&self.extra_registries)
            .enumerate()
            .filter_map(|(area, registry)| registry.triggers().map(|triggers| (area, triggers)))
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn load(
        directory: &Path,
        manager: &PackageManager,
        companions: &mut Companions,
        donor: &str,
        name: &str,
        source: Option<&super::MapSource>,
        options: &super::MapOptions,
        report: &mut dyn FnMut(&str),
    ) -> AuthoringResult<Self> {
        for taken in [name.to_owned(), format!("{name}{SCENARIO_SUFFIX}")] {
            if super::donor_tag(manager, &taken).is_ok() {
                return Err(invalid(format!("Named tag {taken} already exists")));
            }
        }
        report("Reading Donor Activity and Scenario");
        let root = Owner::load(directory, manager, companions, donor, ACTIVITY_ROOT_CLASS)?;
        let scenario = Owner::load(
            directory,
            manager,
            companions,
            &format!("{donor}{SCENARIO_SUFFIX}"),
            SCENARIO_CLASS,
        )?;
        if read_u32(&root.payload, ROOT_SCENARIO)? != scenario.tag.0 {
            return Err(invalid(format!(
                "Donor {donor}'s activity root does not name its scenario"
            )));
        }
        let slice_set = source
            .is_some()
            .then(|| SliceSet::load(manager, &scenario))
            .transpose()?;
        let world = match &slice_set {
            Some(slice_set) => slice_set.world()?,
            None => donor.to_owned(),
        };
        let streaming_plan = source
            .map(|source| {
                parhelion_import::halo_reach::level::streaming::read(
                    &source.maps.join(&source.map),
                    &source.zone_sets,
                    source.offset.map(|v| v as f32),
                )
                .map_err(|e| invalid(format!("Map streaming: {e:#}")))
            })
            .transpose()?
            .flatten()
            // A level of one area takes the same layout as a campaign's first, its own spatial tome,
            // visibility record, lighting cells and layout graph. With the donor's layout kept and
            // only a tome added, a converted Breakneck drew none of its statics.
            .or_else(|| {
                source.map(
                    |source| parhelion_import::halo_reach::level::streaming::Plan {
                        bubbles: vec![parhelion_import::halo_reach::level::streaming::Bubble {
                            name: name.to_owned(),
                            zone_sets: source.zone_sets.clone(),
                            preload: Vec::new(),
                            commit: Vec::new(),
                        }],
                    },
                )
            });
        // A campaign's connected areas convert together, each placing what its zone sets load.
        let areas = streaming_plan.as_ref().map_or_else(Vec::new, |plan| {
            plan.bubbles
                .iter()
                .map(|bubble| bubble.zone_sets.clone())
                .collect()
        });
        let mut geometry = source
            .map(|source| {
                super::geometry::Geometry::load(
                    directory, manager, companions, &world, name, source, &areas, true, report,
                )
            })
            .transpose()?;
        if !options.ambient_audio
            && let Some(geometry) = &mut geometry
        {
            geometry.disable_ambience();
        }
        report("Preparing Spawns, Objectives and Death Boundaries");
        let mission_volumes = super::mission::volumes(source)?;
        let triggers_in = |geometry: &super::geometry::Geometry| -> AuthoringResult<Vec<_>> {
            if mission_volumes.is_empty() {
                return Ok(Vec::new());
            }
            let bounds = geometry.bounds()?;
            Ok(mission_volumes
                .iter()
                .filter(|volume| {
                    let (minimum, maximum) = volume.bounds();
                    (0..3).all(|axis| {
                        minimum[axis] <= bounds.maximum[axis]
                            && maximum[axis] >= bounds.minimum[axis]
                    })
                })
                .cloned()
                .collect())
        };
        report("Reading Map Registry");
        let registry = match (&slice_set, &geometry) {
            (Some(slice_set), Some(geometry)) => Some(super::registry::Registry::load(
                directory,
                manager,
                companions,
                &slice_set.payload,
                SLICE_SET_REGISTRY,
                geometry.spawns(),
                geometry.squads(),
                geometry.objectives(),
                if options.death_boundaries {
                    geometry.volumes()
                } else {
                    &[]
                },
                &triggers_in(geometry)?,
                BubbleName::new(&world, name),
                name,
            )?),
            _ => None,
        };
        let mut extra_geometry = Vec::new();
        let mut extra_registries = Vec::new();
        let streaming = if let Some(plan) = streaming_plan {
            let first = geometry
                .as_mut()
                .ok_or_else(|| invalid("Streaming geometry is missing"))?;
            report("Owning Map Containers");
            first.own_containers(directory, manager, companions)?;
            report("Laying Out Connected Areas");
            let layout = super::streaming::Layout::new(
                name,
                plan,
                first.visibility(manager)?,
                first.layout(),
                manager,
            )?;
            for index in 1..layout.plan.bubbles.len() {
                report(&format!(
                    "Preparing Connected Area {} of {}",
                    index + 1,
                    layout.names.len()
                ));
                let mut geometry = first.area(
                    directory,
                    manager,
                    companions,
                    &world,
                    &layout.names[index],
                    index,
                )?;
                report("Owning Connected Area Containers");
                geometry.own_containers(directory, manager, companions)?;
                if !options.ambient_audio {
                    geometry.disable_ambience();
                }
                report("Reading Connected Area Registry");
                let registry = super::registry::Registry::load(
                    directory,
                    manager,
                    companions,
                    &slice_set
                        .as_ref()
                        .ok_or_else(|| invalid("Streaming slice set is missing"))?
                        .payload,
                    SLICE_SET_REGISTRY,
                    geometry.spawns(),
                    geometry.squads(),
                    geometry.objectives(),
                    if options.death_boundaries {
                        geometry.volumes()
                    } else {
                        &[]
                    },
                    &triggers_in(&geometry)?,
                    BubbleName::new(&world, &layout.names[index]),
                    &layout.names[index],
                )?;
                extra_geometry.push(geometry);
                extra_registries.push(registry);
            }
            Some(layout)
        } else {
            None
        };
        Ok(Self {
            name: name.to_owned(),
            root,
            scenario,
            donor: donor.to_owned(),
            world,
            slice_set,
            registry,
            geometry,
            extra_geometry,
            extra_registries,
            streaming,
            vehicle_summoning: options.vehicle_summoning(name),
        })
    }

    /// The entries the map's world package takes, none without geometry.
    pub(super) fn world_entries(&self) -> usize {
        self.geometries()
            .map(super::geometry::Geometry::entries)
            .sum::<usize>()
            + self
                .streaming
                .as_ref()
                .map_or(0, |s| s.names.len() * super::streaming::ENTRIES)
    }

    fn geometries(&self) -> impl Iterator<Item = &super::geometry::Geometry> {
        self.geometry.iter().chain(&self.extra_geometry)
    }
    /// The scenario names its world and its activity in strings that end the payload, counted
    /// together. A Crucible donor's world takes its activity's name, so its scenario holds one
    /// string, while a mission's holds its world's then its activity's: The Other Side's ends with
    /// `trophy_hall` and `mission_proton`. The copy names the map in each, and every self-relative
    /// pointer to a string that moves follows it. A map that owns its slice set names `slice_set`
    /// wherever the donor named its own, and its own bubble wherever the donor named its bubble.
    fn scenario(&self, slice_set: Option<TagHash>) -> AuthoringResult<Vec<u8>> {
        let mut payload = self.scenario.payload.clone();
        if let Some(allowed) = self.vehicle_summoning {
            set_vehicle_summoning(&mut payload, allowed)?;
        }
        if let (Some(owned), Some(tag)) = (&self.slice_set, slice_set) {
            for &field in &owned.fields {
                write_u32(&mut payload, field, tag.0)?;
            }
            if BubbleName::new(&self.world, &self.name).rename(&mut payload) == 0 {
                return Err(invalid(format!(
                    "The donor scenario names no bubble {}",
                    self.world
                )));
            }
        }
        let mut layouts = vec![vec![self.donor.as_str()]];
        if self.world != self.donor {
            layouts.push(vec![self.world.as_str(), self.donor.as_str()]);
        }
        let (start, strings) = layouts
            .into_iter()
            .find_map(|strings| {
                let block = strings.iter().map(|s| format!("{s}\0")).collect::<String>();
                let start = payload
                    .len()
                    .checked_sub(block.len())
                    .filter(|&start| start >= 8)?;
                (payload[start..] == *block.as_bytes()
                    && read_u64(&payload, start - 8).ok() == Some(block.len() as u64))
                .then_some((start, strings))
            })
            .ok_or_else(|| {
                invalid("The donor scenario does not end with its world's and activity's names")
            })?;
        // Where each string starts in the donor and in the copy.
        let place = |length: &dyn Fn(&str) -> usize| {
            strings
                .iter()
                .scan(start, |at, string| {
                    let here = *at;
                    *at += length(string) + 1;
                    Some(here)
                })
                .collect::<Vec<_>>()
        };
        let old = place(&|string| string.len());
        let new = place(&|_| self.name.len());
        for field in (0..(start - 8).saturating_sub(7)).step_by(4) {
            let relative = read_u64(&payload, field)? as i64;
            if relative == 0 {
                continue;
            }
            let target = field as i64 + relative;
            for (&from, &to) in old.iter().zip(&new).filter(|(from, to)| from != to) {
                if target == from as i64 {
                    write_u64(&mut payload, field, (to as i64 - field as i64) as u64)?;
                }
            }
        }
        payload.truncate(start);
        for _ in &strings {
            payload.extend_from_slice(self.name.as_bytes());
            payload.push(0);
        }
        write_u64(
            &mut payload,
            start - 8,
            (strings.len() * (self.name.len() + 1)) as u64,
        )?;
        synchronize_payload_size(payload)
    }

    /// The map's activity package entries starting at `first`, and the names that select them.
    pub(super) fn activity_tags(
        &self,
        first: TagHash,
    ) -> AuthoringResult<(Vec<NewTagSpec>, Vec<NamedTag>)> {
        match &self.streaming {
            Some(layout) => self.streaming_activity(first, layout),
            None => self.single_activity(first),
        }
    }

    fn single_activity(&self, first: TagHash) -> AuthoringResult<(Vec<NewTagSpec>, Vec<NamedTag>)> {
        let at = |offset: usize| {
            let index = first.entry_index() as usize + offset;
            if index >= crate::appended_tags::MAX_PACKAGE_ENTRY_COUNT {
                return Err(invalid("The map's activity package has too many entries"));
            }
            Ok(TagHash::new(first.pkg_id(), index as u16))
        };
        let (root, root_companion, scenario, scenario_companion) = (at(0)?, at(1)?, at(2)?, at(3)?);
        let slice_set = self.slice_set.as_ref().map(|_| at(ENTRIES)).transpose()?;
        let mut root_payload = self.root.payload.clone();
        write_u32(&mut root_payload, ROOT_SCENARIO, scenario.0)?;
        if self.geometry.is_some() {
            BubbleName::new(&self.world, &self.name).rename(&mut root_payload);
        }
        // The scenario's companion loads its slice set, which is the map's own copy when the map
        // has geometry.
        let mut scenario_companion_payload =
            self.scenario.companion(scenario_companion, scenario)?;
        if let (Some(owned), Some(tag)) = (&self.slice_set, slice_set) {
            scenario_companion_payload = replace_dependencies(
                &scenario_companion_payload,
                scenario_companion,
                scenario,
                &[owned.tag],
                &[tag],
            )?;
        }
        let spec = |template: TagHash, payload: Vec<u8>| NewTagSpec {
            template_tag: template,
            payload,
            storage: NewTagStorageMode::InheritTemplate,
        };
        let mut tags = vec![
            spec(self.root.tag, root_payload),
            spec(
                self.root.companion,
                self.root.companion(root_companion, root)?,
            ),
            spec(self.scenario.tag, self.scenario(slice_set)?),
            spec(self.scenario.companion, scenario_companion_payload),
        ];
        let named = vec![
            NamedTag {
                tag: root.0,
                class: ACTIVITY_ROOT_CLASS,
                name: self.name.clone(),
            },
            NamedTag {
                tag: scenario.0,
                class: SCENARIO_CLASS,
                name: format!("{}{SCENARIO_SUFFIX}", self.name),
            },
        ];
        if let Some(owned) = &self.slice_set {
            let mut payload = owned.renamed(&self.world, &self.name)?;
            if let Some(registry) = &self.registry {
                let first = at(ENTRIES + 1)?;
                write_u32(&mut payload, SLICE_SET_REGISTRY, first.0)?;
                tags.push(spec(owned.tag, payload));
                tags.extend(registry.tags(first)?);
            } else {
                tags.push(spec(owned.tag, payload));
            }
        }
        Ok((tags, named))
    }

    /// The map's world entries across `ids`, the names that select them and the
    /// entry references its buffers need. The slice set names the world's map root by the
    /// map's name, so the activity package names no world entry.
    #[allow(clippy::type_complexity)]
    pub(super) fn world_tags(
        &self,
        ids: &[u16],
    ) -> AuthoringResult<(
        Vec<NewTagSpec>,
        Vec<NamedTag>,
        Vec<crate::extend::NewTagReferenceOverride>,
    )> {
        if self.streaming.is_some() {
            return Err(invalid(
                "Imported map geometry requires the managed standalone mod layout",
            ));
        }
        let Some(geometry) = &self.geometry else {
            return Ok(Default::default());
        };
        let (tags, map_root, references) = geometry.tags(ids, &self.name)?;
        let named = vec![NamedTag {
            tag: map_root.0,
            class: MAP_ROOT_CLASS,
            name: format!("map:{}:root", self.name),
        }];
        Ok((tags, named, references))
    }

    pub(super) fn split_counts(&self) -> Option<(usize, usize)> {
        self.geometry.as_ref().map(|_| {
            (
                self.geometries()
                    .map(super::geometry::Geometry::chain_entries)
                    .sum::<usize>()
                    + self
                        .streaming
                        .as_ref()
                        .map_or(0, |s| s.names.len() * super::streaming::ENTRIES),
                self.geometries()
                    .map(super::geometry::Geometry::resource_entries)
                    .sum(),
            )
        })
    }

    #[allow(clippy::type_complexity)]
    pub(super) fn split_tags(
        &self,
        native: u16,
        resources: &[u16],
    ) -> AuthoringResult<(
        Vec<NewTagSpec>,
        Vec<NamedTag>,
        Vec<crate::extend::NewTagReferenceOverride>,
    )> {
        if let Some(layout) = &self.streaming {
            return self.streaming_tags(native, resources, layout);
        }
        let geometry = self
            .geometry
            .as_ref()
            .ok_or_else(|| invalid("Map has no imported geometry"))?;
        let (tags, root, references) = geometry.split_tags(native, resources, &self.name)?;
        Ok((
            tags,
            vec![NamedTag {
                tag: root.0,
                class: MAP_ROOT_CLASS,
                name: format!("map:{}:root", self.name),
            }],
            references,
        ))
    }

    fn streaming_activity(
        &self,
        first: TagHash,
        layout: &super::streaming::Layout,
    ) -> AuthoringResult<(Vec<NewTagSpec>, Vec<NamedTag>)> {
        let (mut tags, names) = self.single_activity(first)?;
        let at = |offset: usize| -> AuthoringResult<TagHash> {
            let index = first.entry_index() as usize + offset;
            if index >= crate::appended_tags::MAX_PACKAGE_ENTRY_COUNT {
                return Err(invalid("Streaming activity exceeds one package"));
            }
            Ok(TagHash::new(first.pkg_id(), index as u16))
        };
        let owned = self
            .slice_set
            .as_ref()
            .ok_or_else(|| invalid("Streaming slice set is absent"))?;
        let mut slices = vec![at(ENTRIES)?];
        layout.patch_registry(&mut tags[ENTRIES + 1..], at(ENTRIES + 1)?, 0)?;
        for (bubble, registry) in self
            .extra_registries
            .iter()
            .enumerate()
            .map(|(i, r)| (i + 1, r))
        {
            let slice = at(tags.len())?;
            let registry_first = at(tags.len() + 1)?;
            let mut payload = owned.renamed(&self.world, &self.name)?;
            // This stored field is the bubble ordinal. The runtime derives region = ordinal * 8.
            write_u32(&mut payload, 0x10, bubble as u32)?;
            write_u32(&mut payload, SLICE_SET_REGISTRY, registry_first.0)?;
            tags.push(NewTagSpec {
                template_tag: owned.tag,
                payload,
                storage: NewTagStorageMode::InheritTemplate,
            });
            let mut records = registry.tags(registry_first)?;
            layout.patch_registry(&mut records, registry_first, bubble)?;
            tags.extend(records);
            slices.push(slice);
        }
        if tags.len() > crate::appended_tags::MAX_PACKAGE_ENTRY_COUNT {
            return Err(invalid("Streaming activity exceeds one package"));
        }
        let scenario = &mut tags[2].payload;
        let (count, _, old_bubble, _) = array_at(scenario, 0x50)?;
        if count != 1 {
            return Err(invalid("Streaming donor must have one bubble"));
        }
        let (states, _, old_state, _) = array_at(scenario, old_bubble + 8)?;
        if states != 1 {
            return Err(invalid("Streaming donor must have one initial state"));
        }
        let template = scenario[old_state..old_state + 76].to_vec();
        append_native_array(
            scenario,
            0x50,
            0x8080_924D,
            slices.len(),
            &vec![0u8; slices.len() * 24],
        )?;
        let rows = array_at(scenario, 0x50)?.2;
        for (bubble, slice) in slices.iter().enumerate() {
            let row = rows + bubble * 24;
            write_u32(scenario, row, layout.hash(bubble))?;
            let mut state = template.clone();
            if bubble != 0 {
                write_u32(
                    &mut state,
                    4,
                    sundial::package_authoring::fnv1_name_hash(&format!(
                        "{}/default",
                        layout.names[bubble]
                    )),
                )?;
            }
            write_u32(&mut state, 0x1C, bubble as u32)?;
            state[0x20..0x40].fill(0);
            write_u64(&mut state, 0x20, layout.mask(bubble))?;
            write_u32(&mut state, 0x40, layout.hash(bubble))?;
            write_u32(&mut state, 0x44, slice.0)?;
            append_native_array(scenario, row + 8, 0x8080_924F, 1, &state)?;
        }
        *scenario = synchronize_payload_size(std::mem::take(scenario))?;
        tags[3].payload = enroll_dependencies(&tags[3].payload, at(3)?, at(2)?, &slices)?;
        Ok((tags, names))
    }

    #[allow(clippy::type_complexity)]
    fn streaming_tags(
        &self,
        native: u16,
        resources: &[u16],
        layout: &super::streaming::Layout,
    ) -> AuthoringResult<(
        Vec<NewTagSpec>,
        Vec<NamedTag>,
        Vec<crate::extend::NewTagReferenceOverride>,
    )> {
        let native_count = self
            .split_counts()
            .ok_or_else(|| invalid("Streaming geometry is absent"))?
            .0;
        if native_count > crate::appended_tags::MAX_PACKAGE_ENTRY_COUNT {
            return Err(invalid("Streaming world metadata exceeds one package"));
        }
        let mut native_tags = Vec::new();
        let mut resource_tags = Vec::new();
        let mut references = Vec::new();
        let mut parents = Vec::new();
        let mut previous_entry = None;
        let mut textures = super::geometry::Textures::new();
        for (bubble, geometry) in self.geometries().enumerate() {
            let first = TagHash::new(native, native_tags.len() as u16);
            parents.push(TagHash::new(native, first.entry_index() + 2));
            // The first area writes the graph every area's chain names.
            let resource_offset = 0;
            // The tome's clusters come first, since the area's occlusion record assigns each a
            // probe.
            let cells = layout.cells(
                bubble,
                previous_entry,
                geometry.occlusion(),
                geometry.bounds()?,
            )?;
            let assignments = cells.assignments(geometry.occlusion());
            let (mut tags, _, overrides) = geometry.split_tags_at(
                first,
                resources,
                resource_offset,
                &layout.names[bubble],
                &mut textures,
                Some(&assignments),
            )?;
            let content = tags.split_off(geometry.chain_entries());
            layout.patch_chain(&mut tags, bubble)?;
            let spatial = layout.spatial(
                &mut tags,
                first,
                bubble,
                geometry.bounds()?,
                cells,
                geometry.visible(),
            )?;
            previous_entry = geometry.spawns().first().map(|spawn| spawn.position);
            for mut reference in overrides {
                reference.new_tag_ordinal = native_count
                    + resource_offset
                    + reference
                        .new_tag_ordinal
                        .checked_sub(geometry.chain_entries())
                        .ok_or_else(|| invalid("Unexpected native chain entry reference"))?;
                references.push(reference);
            }
            native_tags.extend(tags);
            native_tags.extend(spatial);
            resource_tags.extend(content);
        }
        let parent_rows = parents
            .iter()
            .flat_map(|tag| {
                tag.0
                    .to_le_bytes()
                    .into_iter()
                    .chain(1u32.to_le_bytes())
                    .chain(0u64.to_le_bytes())
            })
            .collect::<Vec<_>>();
        let class = array_at(&native_tags[0].payload, 0x10)?.3;
        append_native_array(
            &mut native_tags[0].payload,
            0x10,
            class,
            parents.len(),
            &parent_rows,
        )?;
        native_tags[0].payload =
            synchronize_payload_size(std::mem::take(&mut native_tags[0].payload))?;
        native_tags[9].payload = layout.graph(&native_tags[9].payload)?;
        // Startup walks all root parents, including areas that have not been requested.
        // Their companions contain only parent metadata and bounds, never bubble content.
        let parent_sources = parents
            .iter()
            .map(|&parent| {
                let companion = TagHash::new(native, parent.entry_index() + 1);
                (
                    native_tags[companion.entry_index() as usize]
                        .payload
                        .as_slice(),
                    companion,
                    parent,
                )
            })
            .collect::<Vec<_>>();
        native_tags[1].payload = enroll_inherited_dependencies(
            &native_tags[1].payload,
            TagHash::new(native, 1),
            TagHash::new(native, 0),
            &[],
            &parent_sources,
        )?;
        if native_tags.len() != native_count {
            return Err(invalid("Streaming native entry count changed"));
        }
        native_tags.extend(resource_tags);
        let mut named = vec![NamedTag {
            tag: TagHash::new(native, 0).0,
            class: MAP_ROOT_CLASS,
            name: format!("map:{}:root", self.name),
        }];
        // Native and host name dictionaries can resolve each additional bubble hash from its
        // own parent name. The first already has the activity's published name.
        named.extend(
            parents
                .iter()
                .zip(&layout.names)
                .skip(1)
                .map(|(parent, name)| NamedTag {
                    tag: parent.0,
                    class: 0x8080_7DAE,
                    name: name.clone(),
                }),
        );
        Ok((native_tags, named, references))
    }
}
