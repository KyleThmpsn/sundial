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
        enroll_dependencies,
        native::{Companions, native_companion},
        remove_dependencies, replace_owner_dependency,
    },
    tag_payload::{read_tag, read_u32, read_u64, synchronize_payload_size, write_u32, write_u64},
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

/// A world's bubble names itself by the FNV-1 hash of its activity's name, which Sunrise and
/// the client read back as the bubble's name. Distant Shore's carries it in its bubble parent,
/// the map root's bubble layout, the activity root, the scenario's bubble rows and states, and
/// two registry objects. A map with its own world gives each copy the map's name.
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

    /// The slice set naming `name` where it named `donor`. Its counted name ends the payload.
    fn renamed(&self, donor: &str, name: &str) -> AuthoringResult<Vec<u8>> {
        renamed_counted(&self.payload, SLICE_SET_NAME, donor, name, "slice set")
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
    slice_set: Option<SliceSet>,
    registry: Option<super::registry::Registry>,
    geometry: Option<super::geometry::Geometry>,
}

impl World {
    pub(super) fn load(
        directory: &Path,
        manager: &PackageManager,
        companions: &mut Companions,
        donor: &str,
        name: &str,
        source: Option<&super::MapSource>,
    ) -> AuthoringResult<Self> {
        for taken in [name.to_owned(), format!("{name}{SCENARIO_SUFFIX}")] {
            if super::donor_tag(manager, &taken).is_ok() {
                return Err(invalid(format!("Named tag {taken} already exists")));
            }
        }
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
        let geometry = source
            .map(|source| {
                super::geometry::Geometry::load(directory, manager, companions, donor, name, source)
            })
            .transpose()?;
        let slice_set = geometry
            .is_some()
            .then(|| SliceSet::load(manager, &scenario))
            .transpose()?;
        let registry = match (&slice_set, &geometry) {
            (Some(slice_set), Some(geometry)) => Some(super::registry::Registry::load(
                directory,
                manager,
                companions,
                &slice_set.payload,
                SLICE_SET_REGISTRY,
                geometry.spawns(),
                geometry.objectives(),
                geometry.volumes(),
                BubbleName::new(donor, name),
            )?),
            _ => None,
        };
        Ok(Self {
            name: name.to_owned(),
            root,
            scenario,
            donor: donor.to_owned(),
            slice_set,
            registry,
            geometry,
        })
    }

    /// The entries the map's world package takes, none without geometry.
    pub(super) fn world_entries(&self) -> usize {
        self.geometry
            .as_ref()
            .map_or(0, super::geometry::Geometry::entries)
    }
    /// The scenario names its activity in a counted string that ends the payload. The copy names
    /// the map instead. Every pointer to the string targets its first byte, which stays put.
    /// A map that owns its slice set names `slice_set` wherever the donor named its own, and its
    /// own bubble wherever the donor named its bubble.
    fn scenario(&self, slice_set: Option<TagHash>) -> AuthoringResult<Vec<u8>> {
        let mut payload = self.scenario.payload.clone();
        if let (Some(owned), Some(tag)) = (&self.slice_set, slice_set) {
            for &field in &owned.fields {
                write_u32(&mut payload, field, tag.0)?;
            }
            if BubbleName::new(&self.donor, &self.name).rename(&mut payload) == 0 {
                return Err(invalid(format!(
                    "The donor scenario names no bubble {}",
                    self.donor
                )));
            }
        }
        let donor = format!("{}\0", self.donor);
        let start = payload
            .len()
            .checked_sub(donor.len())
            .filter(|&start| start >= 8 && payload[start..] == *donor.as_bytes())
            .ok_or_else(|| invalid("The donor scenario does not end with its activity's name"))?;
        if read_u64(&payload, start - 8)? != donor.len() as u64 {
            return Err(invalid("The donor scenario's name is not a counted string"));
        }
        payload.truncate(start);
        payload.extend_from_slice(self.name.as_bytes());
        payload.push(0);
        write_u64(&mut payload, start - 8, self.name.len() as u64 + 1)?;
        synchronize_payload_size(payload)
    }

    /// The map's activity package entries starting at `first`, and the names that select them.
    pub(super) fn activity_tags(
        &self,
        first: TagHash,
    ) -> AuthoringResult<(Vec<NewTagSpec>, Vec<NamedTag>)> {
        let at = |offset: usize| {
            u16::try_from(first.entry_index() as usize + offset)
                .map(|index| TagHash::new(first.pkg_id(), index))
                .map_err(|_| invalid("The map's activity package has too many entries"))
        };
        let (root, root_companion, scenario, scenario_companion) = (at(0)?, at(1)?, at(2)?, at(3)?);
        let slice_set = self.slice_set.as_ref().map(|_| at(ENTRIES)).transpose()?;
        let mut root_payload = self.root.payload.clone();
        write_u32(&mut root_payload, ROOT_SCENARIO, scenario.0)?;
        if self.geometry.is_some() {
            BubbleName::new(&self.donor, &self.name).rename(&mut root_payload);
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
            let mut payload = owned.renamed(&self.donor, &self.name)?;
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

    /// The map's world package entries starting at `first`, the names that select them and the
    /// entry references its buffers need. The slice set names the world's map root by the
    /// map's name, so the activity package names no world entry.
    #[allow(clippy::type_complexity)]
    pub(super) fn world_tags(
        &self,
        first: TagHash,
    ) -> AuthoringResult<(
        Vec<NewTagSpec>,
        Vec<NamedTag>,
        Vec<crate::extend::NewTagReferenceOverride>,
    )> {
        let Some(geometry) = &self.geometry else {
            return Ok(Default::default());
        };
        let (tags, map_root, references) = geometry.tags(first, &self.name)?;
        let named = vec![NamedTag {
            tag: map_root.0,
            class: MAP_ROOT_CLASS,
            name: format!("map:{}:root", self.name),
        }];
        Ok((tags, named, references))
    }
}
