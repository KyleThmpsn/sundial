//! The build plan of the library's maps: their activity, display and private match rows, their
//! strings, and each map's packages.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use sundial::package_authoring::{
    PackageManager, investment_schema::GLOBALS_ITEM_ICON_TABLE_SLOT, resolve_live_named_tag,
};
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult, NewTagSpec, ReplacementSpec,
    appended_tags::MAX_PACKAGE_ENTRY_COUNT,
    error::invalid,
    format::NamedTag,
    tag_payload::{read_tag, read_u32, write_u32},
};

use super::{MapRecipe, Picture, activity, picture, private_match, world};

const INVESTMENT_ROOT_CLASS: u32 = 0x8080_7D84;
const ROOT_ACTIVITY_TABLE_SLOT: usize = 4;
const ROOT_PRIVATE_MATCH_TABLE_SLOT: usize = 64;
const PRIVATE_MATCH_TABLE_CLASS: u32 = 0x8080_7571;
const GLOBALS_PRIVATE_MATCH_DISPLAY_TABLE_SLOT: usize = 43;
const PRIVATE_MATCH_DISPLAY_TABLE_CLASS: u32 = 0x8080_59B9;
const GLOBALS_ACTIVITY_DISPLAY_TABLE_SLOT: usize = 3;
const ROOT_TABLE_TAGS_OFFSET: usize = 0x08;
const GLOBALS_TABLE_TAGS_OFFSET: usize = 0x10;
const TABLE_TAG_STRIDE: usize = 0x10;
/// Suffixes of the loading screen tags an activity's internal name selects. The map's activity
/// root (`<name>`) and scenario (`<name>:scenario_client`) are its own, and its map root is
/// `map:<name>:root`.
const LOADING_SUFFIXES: [&str; 3] = [
    ":spaceflight_filler",
    ":same_dest_filler",
    ":spaceflight_out",
];

pub(super) fn entry_tag(ids: &[u16], entry: usize) -> AuthoringResult<TagHash> {
    let id = ids
        .get(entry / MAX_PACKAGE_ENTRY_COUNT)
        .ok_or_else(|| invalid("A map entry exceeds its allocated packages"))?;
    Ok(TagHash::new(*id, (entry % MAX_PACKAGE_ENTRY_COUNT) as u16))
}

/// A companion, its owner and the package and entry of everything it lists.
type Companion = (TagHash, TagHash, BTreeSet<(u16, u16)>);

/// The companion at world ordinal `entry`: a dependency index that names its own entry at
/// `+0x08` and another owner in the allocated packages at `+0x0C`.
fn companion(addresses: &[TagHash], entry: usize, spec: &NewTagSpec) -> Option<Companion> {
    let tag = *addresses.get(entry)?;
    let own = read_u32(&spec.payload, 0x08).ok()?;
    let owner = TagHash(read_u32(&spec.payload, 0x0C).ok()?);
    if own != tag.0 || owner == tag || !addresses.iter().any(|tag| tag.pkg_id() == owner.pkg_id()) {
        return None;
    }
    let dependencies =
        crate::shared_tag_dependency_index::dependency_entries(&spec.payload, tag, owner).ok()?;
    Some((tag, owner, dependencies))
}

/// Makes each companion also list what the companions of the allocated packages' other
/// owners in its list list, recursively. The client walks the list of an owner it requests, but an
/// owner that loads as an entry of that list loads without its own companion's list. With the
/// map's container copies in the bubble's list, the containers loaded and their placement tables
/// did not, and placing the bubble's containers faulted at `exe+0x422C78` on the unloaded tables.
/// Distant Shore's bubble companion `80F2D244` lists none of its enrolled containers but
/// `80EB0469`.
fn flatten(ids: &[u16], tags: &mut [NewTagSpec]) -> AuthoringResult<()> {
    let addresses = (0..tags.len())
        .map(|entry| entry_tag(ids, entry))
        .collect::<AuthoringResult<Vec<_>>>()?;
    flatten_at(&addresses, tags)
}

fn flatten_at(addresses: &[TagHash], tags: &mut [NewTagSpec]) -> AuthoringResult<()> {
    let ids = addresses
        .iter()
        .map(|tag| tag.pkg_id())
        .collect::<BTreeSet<_>>();
    let found = (0..tags.len())
        .filter_map(|entry| companion(addresses, entry, &tags[entry]).map(|found| (entry, found)))
        .collect::<Vec<_>>();
    let by_owner = found
        .iter()
        .enumerate()
        .map(|(index, (_, (_, owner, _)))| ((owner.pkg_id(), owner.entry_index()), index))
        .collect::<BTreeMap<_, _>>();
    let own = |listed: &BTreeSet<(u16, u16)>| {
        listed
            .iter()
            .filter(|&&(package, _)| ids.contains(&package))
            .copied()
            .collect::<Vec<_>>()
    };
    let mut flattened = Vec::new();
    for (index, (entry, (tag, owner, listed))) in found.iter().enumerate() {
        let mut nested = BTreeSet::new();
        let mut pending = own(listed);
        while let Some(listed_entry) = pending.pop() {
            if let Some(&other) = by_owner.get(&listed_entry)
                && other != index
                && nested.insert(other)
            {
                pending.extend(own(&found[other].1.2));
            }
        }
        if nested.is_empty() {
            continue;
        }
        let sources = nested
            .iter()
            .map(|&other| {
                let (at, (companion, owner, _)) = &found[other];
                (tags[*at].payload.as_slice(), *companion, *owner)
            })
            .collect::<Vec<_>>();
        flattened.push((
            *entry,
            crate::shared_tag_dependency_index::enroll_inherited_dependencies(
                &tags[*entry].payload,
                *tag,
                *owner,
                &[],
                &sources,
            )?,
        ));
    }
    for (entry, payload) in flattened {
        tags[entry].payload = payload;
    }
    Ok(())
}

/// The entries across `ids`, holding `tags`, that are companions. Together they must name
/// every other entry, or that entry could never load. Also returns every other package they name.
fn companions(
    addresses: &[TagHash],
    tags: &[NewTagSpec],
) -> AuthoringResult<(Vec<TagHash>, BTreeSet<u16>)> {
    let ids = addresses
        .iter()
        .map(|tag| tag.pkg_id())
        .collect::<BTreeSet<_>>();
    let mut companions = Vec::new();
    let mut covered = BTreeSet::new();
    let mut packages = BTreeSet::new();
    for (entry, spec) in tags.iter().enumerate() {
        let Some((tag, _, dependencies)) = companion(addresses, entry, spec) else {
            continue;
        };
        covered.insert((tag.pkg_id(), tag.entry_index()));
        for (package, entry) in dependencies {
            if ids.contains(&package) {
                covered.insert((package, entry));
            } else {
                packages.insert(package);
            }
        }
        companions.push(tag);
    }
    let expected = addresses
        .iter()
        .map(|tag| (tag.pkg_id(), tag.entry_index()))
        .collect::<BTreeSet<_>>();
    if companions.is_empty() || covered != expected {
        return Err(invalid(format!(
            "Map packages {ids:04X?} have {} unnamed entries and {} out-of-range dependencies",
            expected.difference(&covered).count(),
            covered.difference(&expected).count(),
        )));
    }
    Ok((companions, packages))
}

/// For each installed package, an entry whose name in the investment loading root's companion
/// registers the package at boot without loading anything else: its smallest data entry (types 40
/// to 42), else its smallest Havok packfile (26:7), else its smallest audio stream (26:6). The
/// stock companion names entries of each kind, and none of them names a tag.
fn anchors(manager: &PackageManager) -> BTreeMap<u16, TagHash> {
    const KINDS: [fn(u8, u8) -> bool; 3] = [
        |kind, _| (40..=42).contains(&kind),
        |kind, subtype| (kind, subtype) == (26, 7),
        |kind, subtype| (kind, subtype) == (26, 6),
    ];
    let mut anchors = BTreeMap::new();
    for (&package, entries) in &manager.lookup.tag32_entries_by_pkg {
        let anchor = KINDS.iter().find_map(|accepts| {
            entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| accepts(entry.file_type, entry.file_subtype))
                .min_by_key(|(index, entry)| (entry.file_size, *index))
                .and_then(|(index, _)| u16::try_from(index).ok())
        });
        if let Some(entry) = anchor {
            anchors.insert(package, TagHash::new(package, entry));
        }
    }
    anchors
}

fn fnv1a(text: &str) -> u32 {
    text.bytes().fold(0x811C_9DC5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// A hash for `role` of `map`, the first of its nonce series that `accept` takes.
pub(super) fn allocate(
    map: &str,
    role: &str,
    mut accept: impl FnMut(u32) -> bool,
) -> AuthoringResult<u32> {
    (0..0x10000u32)
        .map(|nonce| fnv1a(&format!("parhelion/map/{map}/{role}/{nonce}")))
        .find(|&hash| hash != 0 && hash != u32::MAX && hash != 0x811C_9DC5 && accept(hash))
        .ok_or_else(|| invalid(format!("Map {map} has no free {role} hash")))
}

/// A map's identities, chosen once so its rows and strings agree.
pub(super) struct Resolved {
    pub(super) recipe: MapRecipe,
    /// One new activity hash for each of the donor's rows, in the donor's row order.
    pub(super) activities: Vec<u32>,
    pub(super) title_hash: u32,
    pub(super) description_hash: u32,
}

/// One of a map's packages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Part {
    /// The map root chain, converted pieces and their materials and collision.
    World,
    /// The activity root and scenario, the slice set and the object registry.
    Activity,
}

pub(crate) struct SplitWorld {
    pub native_entries: usize,
    pub resource_ids: Vec<u16>,
    pub addresses: Vec<TagHash>,
    pub tags: Vec<NewTagSpec>,
    pub names: Vec<NamedTag>,
    pub references: Vec<crate::NewTagReferenceOverride>,
}

/// What the build emits for every library map.
#[derive(Default)]
pub(crate) struct Plan {
    maps: Vec<Resolved>,
    /// Each map's activity root and scenario.
    worlds: Vec<world::World>,
    /// Names each map shares with its donor's world: its loading screens, and its map root
    /// unless it has geometry of its own. One list per map.
    aliases: Vec<Vec<NamedTag>>,
    /// The activity, display, private match and private match display tables with every map's
    /// rows and entries appended.
    pub replacements: Vec<ReplacementSpec>,
    /// The entry that registers each installed package at boot, from [`anchors`].
    anchors: BTreeMap<u16, TagHash>,
    /// Each map's own private match picture, for the maps whose source level has one.
    pictures: Vec<Picture>,
    /// Each map's new private match displays, which name its picture's row once it has one.
    displays: Vec<private_match::Display>,
}

/// The private match display table's place in [`Plan::replacements`].
const PRIVATE_MATCH_DISPLAYS: usize = 3;

fn root_slot(data: &[u8], base: usize, slot: usize) -> AuthoringResult<TagHash> {
    Ok(TagHash(read_u32(data, base + slot * TABLE_TAG_STRIDE)?))
}

impl Plan {
    /// Resolves the library's maps against the source tables. Hashes avoid every existing
    /// activity, and string hashes stay above `string_floor` and out of `taken_strings`.
    pub(crate) fn load(
        directory: &Path,
        manager: &PackageManager,
        recipes: Vec<MapRecipe>,
        string_floor: u32,
        report: &mut dyn FnMut(&str),
    ) -> AuthoringResult<Self> {
        if recipes.is_empty() {
            return Ok(Self::default());
        }
        report("Planning Maps: Reading Activity Tables");
        let globals_tag =
            resolve_live_named_tag(manager, "investment_globals", None).map_err(invalid)?;
        let globals = read_tag(manager, globals_tag, "investment globals")?;
        let root_tag = TagHash(read_u32(&globals, GLOBALS_TABLE_TAGS_OFFSET)?);
        let root = read_tag(manager, root_tag, "investment root")?;
        if manager.get_entry(root_tag).map(|entry| entry.reference) != Some(INVESTMENT_ROOT_CLASS) {
            return Err(invalid("The investment root moved"));
        }
        let activity_tag = root_slot(&root, ROOT_TABLE_TAGS_OFFSET, ROOT_ACTIVITY_TABLE_SLOT)?;
        let display_tag = root_slot(
            &globals,
            GLOBALS_TABLE_TAGS_OFFSET,
            GLOBALS_ACTIVITY_DISPLAY_TABLE_SLOT,
        )?;
        let private_match_tag =
            root_slot(&root, ROOT_TABLE_TAGS_OFFSET, ROOT_PRIVATE_MATCH_TABLE_SLOT)?;
        let private_match_display_tag = root_slot(
            &globals,
            GLOBALS_TABLE_TAGS_OFFSET,
            GLOBALS_PRIVATE_MATCH_DISPLAY_TABLE_SLOT,
        )?;
        for (tag, class) in [
            (private_match_tag, PRIVATE_MATCH_TABLE_CLASS),
            (private_match_display_tag, PRIVATE_MATCH_DISPLAY_TABLE_CLASS),
        ] {
            if manager.get_entry(tag).map(|entry| entry.reference) != Some(class) {
                return Err(invalid(format!("Private match table {tag} moved")));
            }
        }
        let activities = read_tag(manager, activity_tag, "activity table")?;
        let displays = read_tag(manager, display_tag, "activity display table")?;
        let rows = activity::Rows::read(&activities)?;
        let mut taken = rows.hashes(&activities)?;
        let mut strings = BTreeSet::new();
        let mut maps = Vec::with_capacity(recipes.len());
        let total = recipes.len();
        for (index, recipe) in recipes.into_iter().enumerate() {
            report(&format!(
                "Planning Map {}/{} / {} / Resolving Donor {}",
                index + 1,
                total,
                recipe.title,
                recipe.donor
            ));
            if !rows.ordinals_named(&activities, &recipe.name)?.is_empty() {
                return Err(invalid(format!(
                    "Map {} already has activity rows",
                    recipe.name
                )));
            }
            let donors = rows.ordinals_named(&activities, &recipe.donor)?.len();
            if donors == 0 {
                return Err(invalid(format!(
                    "Map {} names donor {}, which has no activity rows",
                    recipe.name, recipe.donor
                )));
            }
            let mut hashes = Vec::with_capacity(donors);
            for row in 0..donors {
                let hash = allocate(&recipe.name, &format!("activity/{row}"), |hash| {
                    !taken.contains(&hash)
                })?;
                taken.insert(hash);
                hashes.push(hash);
            }
            let mut string = |role: &str| {
                let hash = allocate(&recipe.name, role, |hash| {
                    hash > string_floor && !strings.contains(&hash)
                })?;
                strings.insert(hash);
                Ok::<_, crate::AuthoringError>(hash)
            };
            let title_hash = string("title")?;
            let description_hash = string("description")?;
            maps.push(Resolved {
                recipe,
                activities: hashes,
                title_hash,
                description_hash,
            });
        }
        report("Planning Maps: Preparing Activity and Display Rows");
        let (activities, displays, copies) = activity::append(&activities, &displays, &maps)?;
        let private_table = read_tag(manager, private_match_tag, "private match table")?;
        let private_displays = read_tag(
            manager,
            private_match_display_tag,
            "private match display table",
        )?;
        let image_rows = private_match::image_rows(&activities, &private_table, &private_displays)?;
        let bank = u32::try_from(crate::package_profile::LOCALIZATION_DONOR_TABLE_INDEX)
            .map_err(|_| invalid("Localized bank index does not fit 32 bits"))?;
        let (private_matches, mut private_match_displays, picture_displays) =
            private_match::append(&private_table, &private_displays, &maps, &copies, bank)?;
        let icons = read_tag(
            manager,
            root_slot(
                &globals,
                GLOBALS_TABLE_TAGS_OFFSET,
                GLOBALS_ITEM_ICON_TABLE_SLOT,
            )?,
            "item icon table",
        )?;
        let mut pictures = Vec::new();
        for (index, map) in maps.iter().enumerate() {
            report(&format!(
                "Planning Map {}/{} / {} / Preparing Images",
                index + 1,
                total,
                map.recipe.title
            ));
            let Some(display) = picture_displays.iter().find(|display| display.map == index) else {
                continue;
            };
            if let Some(donor) = &map.recipe.image_donor {
                let row = *image_rows.get(donor).ok_or_else(|| {
                    invalid(format!("Activity {donor} has no private-match artwork"))
                })?;
                for display in picture_displays
                    .iter()
                    .filter(|display| display.map == index)
                {
                    write_u32(
                        &mut private_match_displays,
                        display.offset + private_match::MAP_DISPLAY_IMAGE,
                        row,
                    )?;
                }
                continue;
            }
            pictures.extend(picture::load(
                manager,
                &icons,
                display.donor_image,
                index,
                &map.recipe.name,
                &map.recipe,
            )?);
        }
        let mut plan = Self {
            replacements: vec![
                ReplacementSpec {
                    tag: activity_tag,
                    payload: activities,
                },
                ReplacementSpec {
                    tag: display_tag,
                    payload: displays,
                },
                ReplacementSpec {
                    tag: private_match_tag,
                    payload: private_matches,
                },
                ReplacementSpec {
                    tag: private_match_display_tag,
                    payload: private_match_displays,
                },
            ],
            anchors: anchors(manager),
            pictures,
            displays: picture_displays,
            ..Self::default()
        };
        let mut companions = Default::default();
        for (index, map) in maps.iter().enumerate() {
            let prefix = format!(
                "Planning Map {}/{} / {}",
                index + 1,
                total,
                map.recipe.title
            );
            let mut detail = |step: &str| report(&format!("{prefix} / {step}"));
            plan.worlds.push(world::World::load(
                directory,
                manager,
                &mut companions,
                &map.recipe.donor,
                &map.recipe.name,
                map.recipe.source.as_ref(),
                &map.recipe.options,
                &mut detail,
            )?);
            detail("Preparing Loading Screens and Package Names");
            plan.aliases.push(aliases(manager, map)?);
        }
        plan.maps = maps;
        Ok(plan)
    }

    /// How many maps the build holds.
    pub(crate) fn len(&self) -> usize {
        self.worlds.len()
    }

    /// The packages map `index` takes, as a stock map's: its world when it has geometry, then its
    /// activity.
    pub(crate) fn parts(&self, index: usize) -> Vec<Part> {
        let world = self.worlds[index].world_entries() > 0;
        [Part::World, Part::Activity]
            .into_iter()
            .filter(|part| *part == Part::Activity || world)
            .collect()
    }

    /// How many packages the build's maps take.
    pub(crate) fn package_count(&self) -> usize {
        (0..self.len())
            .map(|index| {
                self.parts(index)
                    .iter()
                    .map(|part| self.part_count(index, *part))
                    .sum::<usize>()
            })
            .sum()
    }

    pub(crate) fn part_count(&self, index: usize, part: Part) -> usize {
        match part {
            Part::World if crate::package_profile::host().is_standalone() => {
                let (_, resources) = self.worlds[index].split_counts().unwrap();
                1 + resources.div_ceil(MAX_PACKAGE_ENTRY_COUNT)
            }
            Part::World => self.worlds[index]
                .world_entries()
                .div_ceil(MAX_PACKAGE_ENTRY_COUNT),
            Part::Activity => 1,
        }
    }

    pub(crate) fn split_world(
        &self,
        index: usize,
        part: Part,
        native: u16,
    ) -> AuthoringResult<Option<SplitWorld>> {
        if part != Part::World || !crate::package_profile::host().is_standalone() {
            return Ok(None);
        }
        let world = &self.worlds[index];
        let (native_entries, resources) = world
            .split_counts()
            .ok_or_else(|| invalid("Map geometry is missing"))?;
        crate::mod_family::build::native(native);
        let resource_ids = crate::mod_family::build::map_packages(
            &self.maps[index].recipe.name,
            resources.div_ceil(MAX_PACKAGE_ENTRY_COUNT),
        )
        .map_err(invalid)?;
        let mut addresses = (0..native_entries)
            .map(|entry| TagHash::new(native, entry as u16))
            .collect::<Vec<_>>();
        addresses.extend(
            (0..resources)
                .map(|entry| entry_tag(&resource_ids, entry))
                .collect::<AuthoringResult<Vec<_>>>()?,
        );
        let (mut tags, names, references) = world.split_tags(native, &resource_ids)?;
        if tags.len() != addresses.len() {
            return Err(invalid("Split map entry counts differ"));
        }
        flatten_at(&addresses, &mut tags)?;
        Ok(Some(SplitWorld {
            native_entries,
            resource_ids,
            addresses,
            tags,
            names,
            references,
        }))
    }

    /// The family in the file name of map `index`'s package `part`: the map's name for its world
    /// and the name with `_activities` for its activity, as stock maps' packages are named.
    pub(crate) fn family(&self, index: usize, part: Part) -> String {
        let name = &self.maps[index].recipe.name;
        match part {
            Part::World => name.clone(),
            Part::Activity => format!("{name}_activities"),
        }
    }

    /// The entries the investment loading root names for map packages `ids` holding
    /// `tags`. Its companions register the package and stay resident from boot, as stock maps keep
    /// only their companions in packages loaded early. Each owner then loads with its companion's
    /// dependency list when the map is played.
    ///
    /// The client resolves a tag through its package's datum row, which registration fills. A
    /// stock map's own packages register when its activity is requested, while the map's records
    /// still name its donor's, such as the bubble's direct rows to containers in the donor's world
    /// package. So each installed package the companions name that the loading root does not
    /// name (`registered`) and the build does not author gets its anchor named as well, which
    /// registers it without loading its content. Packages the install lacks stay unnamed, as in
    /// stock lists. The bubble's placement fault was the unloaded tables [`flatten`] answers.
    pub(crate) fn registration(
        &self,
        ids: &[u16],
        tags: &[NewTagSpec],
        registered: &BTreeSet<u16>,
        manager: &PackageManager,
    ) -> AuthoringResult<Vec<TagHash>> {
        let addresses = (0..tags.len())
            .map(|entry| entry_tag(ids, entry))
            .collect::<AuthoringResult<Vec<_>>>()?;
        self.registration_at(&addresses, tags, registered, manager)
    }

    pub(crate) fn registration_at(
        &self,
        addresses: &[TagHash],
        tags: &[NewTagSpec],
        registered: &BTreeSet<u16>,
        manager: &PackageManager,
    ) -> AuthoringResult<Vec<TagHash>> {
        let ids = addresses
            .iter()
            .map(|tag| tag.pkg_id())
            .collect::<BTreeSet<_>>();
        let (mut entries, packages) = companions(addresses, tags)?;
        // World overflow contains raw resources rather than companions. Register each package
        // through its smallest inert data entry before any named root can request its content.
        for &id in &ids {
            if entries.iter().any(|tag| tag.pkg_id() == id) {
                continue;
            }
            let anchor = addresses
                .iter()
                .enumerate()
                .filter(|(_, tag)| tag.pkg_id() == id)
                .filter_map(|(index, _)| {
                    let spec = &tags[index];
                    let entry = manager.get_entry(spec.template_tag)?;
                    let rank = match spec.storage {
                        crate::NewTagStorageMode::AudioMedia => 2,
                        crate::NewTagStorageMode::AudioBank => return None,
                        crate::NewTagStorageMode::GpuHull | crate::NewTagStorageMode::GpuDomain => {
                            return None;
                        }
                        crate::NewTagStorageMode::InheritTemplate => {
                            match (entry.file_type, entry.file_subtype) {
                                (40..=42, _) => 0,
                                (26, 7) => 1,
                                (26, 6) => 2,
                                _ => return None,
                            }
                        }
                    };
                    Some((rank, spec.payload.len(), index))
                })
                .min()
                .ok_or_else(|| {
                    invalid(format!(
                        "Map package {id:04X} has no inert registration entry"
                    ))
                })?;
            entries.push(addresses[anchor.2]);
        }
        let authored = crate::package_profile::window();
        for package in packages {
            if registered.contains(&package) || (authored.base..=authored.top).contains(&package) {
                continue;
            }
            if let Some(&anchor) = self.anchors.get(&package) {
                entries.push(anchor);
            }
        }
        Ok(entries)
    }

    /// The entries and names of map `index`'s `part`, allocated across `ids`. The activity package
    /// also holds the names the map shares with its donor's world.
    #[allow(clippy::type_complexity)]
    pub(crate) fn package(
        &self,
        index: usize,
        part: Part,
        ids: &[u16],
    ) -> AuthoringResult<(
        Vec<NewTagSpec>,
        Vec<NamedTag>,
        Vec<crate::extend::NewTagReferenceOverride>,
    )> {
        let world = &self.worlds[index];
        match part {
            Part::World => {
                let (mut tags, named, references) = world.world_tags(ids)?;
                flatten(ids, &mut tags)?;
                Ok((tags, named, references))
            }
            Part::Activity => {
                let (mut tags, names) = world.activity_tags(entry_tag(ids, 0)?)?;
                if tags.len() > MAX_PACKAGE_ENTRY_COUNT {
                    return Err(invalid("The map activity exceeds one package"));
                }
                flatten(ids, &mut tags)?;
                let mut named = self.aliases[index].clone();
                named.extend(names);
                Ok((tags, named, Vec::new()))
            }
        }
    }

    /// The localized strings the maps' display rows name, by hash.
    pub(crate) fn strings(&self) -> Vec<(u32, String)> {
        self.maps
            .iter()
            .flat_map(|map| {
                [
                    (map.title_hash, map.recipe.title.clone()),
                    (map.description_hash, map.recipe.description.clone()),
                ]
            })
            .collect()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.maps.is_empty()
    }

    /// The maps' own private match pictures.
    pub(crate) fn pictures(&self) -> &[Picture] {
        &self.pictures
    }

    /// Names each picture's item icon row, given by map in `rows`, in its map's private match
    /// displays.
    pub(crate) fn set_pictures(&mut self, rows: &[(usize, u16)]) -> AuthoringResult<()> {
        if rows.len() != self.pictures.len() {
            return Err(invalid("Every map picture needs an item icon row"));
        }
        if rows.is_empty() {
            return Ok(());
        }
        let table = &mut self
            .replacements
            .get_mut(PRIVATE_MATCH_DISPLAYS)
            .ok_or_else(|| invalid("The plan holds no private match display table"))?
            .payload;
        for &(map, row) in rows {
            for display in self.displays.iter().filter(|display| display.map == map) {
                write_u32(
                    table,
                    display.offset + private_match::MAP_DISPLAY_IMAGE,
                    u32::from(row),
                )?;
            }
        }
        Ok(())
    }
}

/// A live named tag and its class.
pub(super) fn donor_tag(manager: &PackageManager, name: &str) -> AuthoringResult<(TagHash, u32)> {
    let tag = resolve_live_named_tag(manager, name, None).map_err(invalid)?;
    let class = manager
        .get_entry(tag)
        .map(|entry| entry.reference)
        .ok_or_else(|| invalid(format!("Named tag {name} has no entry")))?;
    Ok((tag, class))
}

/// The map's names for the donor's map root and loading screens.
fn aliases(manager: &PackageManager, map: &Resolved) -> AuthoringResult<Vec<NamedTag>> {
    let mut names = LOADING_SUFFIXES
        .iter()
        .map(|suffix| {
            (
                format!("{}{suffix}", map.recipe.donor),
                format!("{}{suffix}", map.recipe.name),
            )
        })
        .collect::<Vec<_>>();
    if map.recipe.source.is_none() {
        names.push((
            format!("map:{}:root", map.recipe.donor),
            format!("map:{}:root", map.recipe.name),
        ));
    } else if resolve_live_named_tag(manager, &format!("map:{}:root", map.recipe.name), None)
        .is_ok()
    {
        return Err(invalid(format!(
            "Named tag map:{}:root already exists",
            map.recipe.name
        )));
    }
    let mut tags = Vec::new();
    for (donor, name) in names {
        if resolve_live_named_tag(manager, &name, None).is_ok() {
            return Err(invalid(format!("Named tag {name} already exists")));
        }
        // A donor without a loading screen of one kind leaves the map without it too.
        let Ok((tag, class)) = donor_tag(manager, &donor) else {
            if donor.starts_with("map:") {
                return Err(invalid(format!("Donor named tag {donor} is missing")));
            }
            continue;
        };
        tags.push(NamedTag {
            tag: tag.0,
            class,
            name,
        });
    }
    Ok(tags)
}
