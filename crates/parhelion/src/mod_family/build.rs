//! The family build's state, in three parts. [`begin`] plans the build from the installed members
//! ([`Plan`], fixed from then on), the passes' reviews decide what the next pass does differently
//! ([`Decisions`]), and each pass's member builds take ids and record what they built ([`Pass`]).
//! `begin` returns the [`Family`] value that the workflow passes through each pass and ends with
//! [`Family::commit`]. Member builds record from deep inside compilation, so a pass's state lives
//! in this module's slot while it runs, and staging reads the committed pass from there.
//! `package_profile::Profile` serializes builds and clears the slot when its turn ends, including
//! failed builds.
#![cfg_attr(not(feature = "d2-model-importer"), allow(dead_code, unused_mut))]
use super::*;
use crate::{NewTagReference, NewTagReferenceOverride, NewTagSpec, NewTagStorageMode};
use std::{
    fs,
    sync::{Mutex, MutexGuard},
};
use sundial::package_authoring::mod_content::{ImportContent, Importer};
use tiger_pkg::{Package, PackageD2PreBL, TagHash};
mod seed;

/// Bump when the linked entry representation or package writer's byte contract changes.
const FORMAT: u32 = 2;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    format: u32,
    groups: BTreeMap<String, BTreeMap<String, u32>>,
    packages: BTreeMap<u16, PackageState>,
    maps: BTreeMap<String, MapLinks>,
    map_packages: BTreeMap<String, Vec<u16>>,
    dependencies: Vec<ArtifactMetadata>,
    #[serde(default, skip_serializing_if = "Own::is_empty")]
    own: Own,
}

/// The ids an importer's own build takes past its resources: its records host, its record and
/// private pages and its asset packages, each by position. The next build takes the same ids in
/// the same places, so an unchanged member builds the same bytes whatever the others change.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct Own {
    host: Option<u16>,
    records: Vec<u16>,
    private: Vec<u16>,
    assets: Vec<u16>,
}

impl Own {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    fn list(&mut self, kind: OwnKind) -> &mut Vec<u16> {
        match kind {
            OwnKind::Records => &mut self.records,
            OwnKind::Private => &mut self.private,
            OwnKind::Assets => &mut self.assets,
        }
    }

    fn of(&self, kind: OwnKind) -> &[u16] {
        match kind {
            OwnKind::Records => &self.records,
            OwnKind::Private => &self.private,
            OwnKind::Assets => &self.assets,
        }
    }

    fn ids(&self) -> impl Iterator<Item = u16> + '_ {
        self.host
            .iter()
            .chain(&self.records)
            .chain(&self.private)
            .chain(&self.assets)
            .copied()
    }
}

#[derive(Clone, Copy)]
enum OwnKind {
    Records,
    Private,
    Assets,
}

/// A map's links: its conversion without payloads or reports, which a later build reads to link
/// the map's world against the installed packages. They run to tens of megabytes for a campaign,
/// so they stay with the conversion in the data folder's imports, in its `links.json`, and the
/// receipt names that file by its digest.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum MapLinks {
    Stored {
        file: PathBuf,
        sha256: String,
    },
    /// The links themselves, as earlier receipts and builds without a map conversion folder keep
    /// them. The serialized bytes are kept, since parsing floating-point placement metadata into a
    /// JSON value again can change its spelling and invalidate the fingerprint.
    Text(String),
}

impl MapLinks {
    /// The map's links, or `None` when the stored file is missing or changed.
    fn text(&self) -> Option<String> {
        match self {
            Self::Stored { file, sha256 } => {
                let bytes = fs::read(file).ok()?;
                (crate::artifact::digest_bytes(&bytes).sha256 == *sha256)
                    .then(|| String::from_utf8(bytes).ok())
                    .flatten()
            }
            Self::Text(text) => Some(text.clone()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PackageState {
    name: String,
    entries: Vec<Entry>,
    references: Vec<Reference>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    template: u32,
    storage: NewTagStorageMode,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    ordinal: usize,
    mode: ReferenceMode,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
enum ReferenceMode {
    Template,
    Ordinal(usize),
    Tag(u32),
}

impl From<&NewTagReferenceOverride> for Reference {
    fn from(reference: &NewTagReferenceOverride) -> Self {
        Self {
            ordinal: reference.new_tag_ordinal,
            mode: match reference.reference {
                NewTagReference::Template => ReferenceMode::Template,
                NewTagReference::Appended(ordinal) => ReferenceMode::Ordinal(ordinal),
                NewTagReference::Tag(tag) => ReferenceMode::Tag(tag.0),
            },
        }
    }
}

impl Reference {
    fn native(&self) -> NewTagReferenceOverride {
        NewTagReferenceOverride {
            new_tag_ordinal: self.ordinal,
            reference: match self.mode {
                ReferenceMode::Template => NewTagReference::Template,
                ReferenceMode::Ordinal(ordinal) => NewTagReference::Appended(ordinal),
                ReferenceMode::Tag(tag) => NewTagReference::Tag(TagHash(tag)),
            },
        }
    }
}

/// What planning found for one importer's content, which no pass changes: its input identity, its
/// installed member and whether that member's resources are kept, and the ids the installed
/// member's own build took.
#[derive(Clone)]
struct Planned {
    inputs: String,
    source_inputs: Vec<ArtifactMetadata>,
    installed: Option<(PathBuf, Member)>,
    kept: bool,
    /// The ids the installed member's own build took, which this build takes again.
    previous: Own,
}

/// What one pass's builds take and record for an importer's content: its resource allocation,
/// the ids it used, the state its receipt keeps and how many conversions it linked. Every pass
/// starts from the installed allocation, and from the installed state when it is kept.
#[derive(Clone, Debug)]
struct Taken {
    allocation: Vec<u16>,
    used: BTreeSet<u16>,
    state: State,
    conversions: usize,
}

/// What [`begin`] decides before any pass.
#[derive(Clone, Default)]
struct Plan {
    imports: BTreeMap<Importer, Planned>,
    /// The importer whose mod holds each recipe, in the project's order, and each map, in the
    /// request's order. `None` is the main mod.
    recipe_owners: Vec<Option<Importer>>,
    map_owners: Vec<Option<Importer>>,
    /// The importer each imported item comes from, by item hash, in a build without mod folders.
    /// Its one mod still gives each importer's items that importer's badge.
    sources: BTreeMap<u32, Importer>,
    #[cfg(feature = "d2-model-importer")]
    maps: Vec<(crate::map::MapRecipe, serde_json::Value)>,
    /// Where the decisions a committed build settled on are kept for the install's next build.
    seed: Option<PathBuf>,
}

/// What the passes so far decided the next pass does differently. Only [`Family::retry`] changes
/// it, between passes. It only grows, except the contributions, which each retry takes from the
/// pass that ended.
#[derive(Clone, Default)]
struct Decisions {
    /// Importers whose recipes compile with the main mod's, because an entry their own build
    /// changes wouldn't merge with another member's and no table can host it, each with
    /// sentences saying why. The main mod then holds their resources too.
    coupled: BTreeMap<Importer, Vec<String>>,
    /// Whether the main mod builds in every pass to host the importers' shared rows, even with no
    /// recipe or map of its own. A family whose recipes are all imported has no main mod build
    /// otherwise, and no member could hold another's rows.
    host: bool,
    /// The shared tables whose rows for an importer's items the main mod's build writes, because
    /// another member changes them too. The importer keeps its items and requires the main mod.
    hosting: BTreeMap<Importer, BTreeSet<Shared>>,
    /// How many entries the importers put before the stock entries of each stock collectible
    /// array, by node and array offset, as the pass before recorded them. The main mod's build
    /// keeps that many stock rows in place after its own entries for the importers to write over,
    /// and moves the stock entries down past them.
    fronts: BTreeMap<(usize, usize), usize>,
    /// The contributions the main mod's build writes, which the importers' builds recorded in
    /// the pass before.
    contributions: BTreeMap<Importer, Contributions>,
}

/// What one pass's member builds take and record. Every pass starts from the pass [`begin`]
/// planned, so nothing an earlier pass allocated or recorded reaches it except through
/// [`Decisions`].
#[derive(Clone, Default)]
struct Pass {
    imports: BTreeMap<Importer, Taken>,
    reserved: BTreeSet<u16>,
    native: BTreeSet<u16>,
    /// The packages that hold the build's private runtime copies, in order.
    private: Vec<u16>,
    records: Vec<u16>,
    /// The shared tables Sunrise can't merge that each member's build changed, by stock entry.
    /// `None` is the main mod.
    produced: BTreeMap<Option<Importer>, BTreeMap<u32, Shared>>,
    /// What each importer's items add to the shared tables, recorded by its build.
    recorded: BTreeMap<Importer, Contributions>,
    /// The main mod's finished payloads of the shared entries it hosts, which an importer that
    /// builds on them reads in the same pass.
    hosted_payloads: BTreeMap<u32, Vec<u8>>,
    /// The shared entries whose main mod payloads each importer's build follows, whether or not
    /// the main mod wrote them in the pass. Only these payloads decide whether a later pass keeps
    /// the build ([`member_inputs`]).
    reads: BTreeMap<Importer, BTreeSet<u32>>,
    /// Importers whose builds read the main mod's finished payloads of a shared table, by table.
    needs_main: BTreeMap<Importer, BTreeSet<Shared>>,
    /// Whether the main mod builds in this pass, so it can write the importers' rows.
    main_builds: bool,
    /// Each member's badges, `None` for the main mod. The main mod's build counts them all in the
    /// Badges total, since Sunrise keeps one mod's value of it, and arranges the Badges root
    /// around the importers that have one.
    badges: Option<BTreeMap<Option<Importer>, usize>>,
    /// The stock Collections arrays each member's build puts entries before the stock entries
    /// of, by node and array offset, each with whether items set to First lead there. `None` is
    /// the main mod.
    leads: Leads,
    /// The entries each member's build put before the stock entries of each stock collectible
    /// array, by node and array offset. The members' entries come first in compile order, the
    /// main mod's ahead of every importer's.
    fronts: BTreeMap<(usize, usize), Front>,
    /// Controllers are generated after activity tags have their final addresses.
    mission_scripts: BTreeMap<String, String>,
    /// The imported abilities each member's subclasses carry and the ones its perks check, by
    /// source and identity. `None` is the main mod.
    abilities: BTreeMap<Option<Importer>, Abilities>,
}

/// The members that lead each stock Collections array, by node and array offset, each with
/// whether items set to First lead there rather than automatic entries. `None` is the main mod.
type Leads = BTreeMap<(usize, usize), BTreeMap<Option<Importer>, bool>>;

/// The entries the members put before the stock entries of one stock collectible array in a pass.
#[derive(Clone, Debug, Default)]
struct Front {
    /// How many entries the array's stock held, as the first member to put entries there read it.
    stock: usize,
    /// How many entries each member put there. `None` is the main mod.
    members: BTreeMap<Option<Importer>, usize>,
}

/// Where a member's next entries go before the stock entries of a stock collectible array.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Slot {
    /// The row the first of them takes, after the earlier members' entries and its own earlier
    /// ones.
    pub(crate) row: usize,
    /// How many entries the array's stock holds.
    pub(crate) stock: usize,
}

/// One member's imported abilities, by source game and identity there.
#[derive(Clone, Debug, Default)]
struct Abilities {
    /// Abilities whose roots the member's build allocated.
    carried: BTreeSet<(String, u32)>,
    /// Abilities whose Shadowkeep counterparts the member's perk conditions name.
    checked: BTreeSet<(String, u32)>,
}

/// The slot's contents: the plan and decisions the running pass builds with, and the pass. Only
/// the pass changes while it runs.
#[derive(Default)]
struct Session {
    plan: Plan,
    decisions: Decisions,
    pass: Pass,
}

impl Session {
    /// `owner`'s planned content and what the pass took for it.
    fn import(&self, owner: Importer) -> Option<(&Planned, &Taken)> {
        Some((
            self.plan.imports.get(&owner)?,
            self.pass.imports.get(&owner)?,
        ))
    }

    /// Every importer's planned content and what the pass took for it, in importer order.
    fn imports(&self) -> impl Iterator<Item = (Importer, &Planned, &Taken)> + '_ {
        self.plan
            .imports
            .iter()
            .filter_map(|(owner, planned)| Some((*owner, planned, self.pass.imports.get(owner)?)))
    }
}

/// A shared table Sunrise can't merge. When several members' builds add rows to one, the main
/// mod's build writes it with every member's rows, and each importer keeps the assets its rows
/// name. A row whose importer is removed names assets no loaded package holds, and nothing applies
/// it, since only that importer's items name its keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub(crate) enum Shared {
    /// The crosshair table `80B479A9`, whose rows name converted crosshair and inspect scenes.
    Crosshair,
    /// The HUD status name bank `815B961E` and its locales.
    HudNames,
    /// The ability banks, their referrers and the movement controllers evade rows extend. An
    /// importer's private copies name a bank's blocks by offset, so the main mod writes every
    /// importer's rows whether or not another member changes the bank.
    Banks,
    /// The character animation tables and dictionaries.
    Character,
    /// The activity and private match tables. Their rows show maps, so an importer's rows can't
    /// stay behind when it is removed. The importer builds its tables on the main mod's instead,
    /// and Sunrise serves them from it while it loads.
    Activities,
    /// The stock Collections objectives' targets and acquired counts, which count each member's
    /// built items. Every member with items on a page changes the same words, so the main mod
    /// writes every member's counts whenever it builds.
    Collections,
}

impl Shared {
    /// What the table holds, for a sentence the user reads.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Crosshair => "the crosshair table",
            Self::HudNames => "HUD status names",
            Self::Banks => "the ability banks",
            Self::Character => "the character animation tables",
            Self::Activities => "the activity tables",
            Self::Collections => "the Collections counts",
        }
    }
}

/// A character action an importer's item adds, as the main mod's build adds it to the shared
/// tables: the graph's folder, the private clips the importer's build allocated in its own
/// packages, in the graph's clip order, and the digest that tells two profiles of one name apart.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct HostedCharacter {
    pub(crate) folder: PathBuf,
    pub(crate) clips: Vec<u32>,
    pub(crate) digest: [u8; 32],
}

/// What one importer's build adds to the shared tables, in the form the main mod's build adds it.
/// Rows name the importer's own assets by tag.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub(crate) struct Contributions {
    /// Crosshair table rows that stock lacks.
    pub(crate) crosshair: Vec<crate::weapon::crosshair::Row>,
    /// HUD status names, by hash.
    pub(crate) hud_names: Vec<(u32, String)>,
    /// Ability bank rows and evade profiles.
    pub(crate) banks: crate::ability::banks::Hosted,
    pub(crate) characters: Vec<HostedCharacter>,
    /// The human actions an imported armor's or ability's evade plays, in the same form.
    pub(crate) human_actions: Vec<HostedCharacter>,
    /// How many of its built items each stock Collections objective counts, by objective.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) collections: BTreeMap<u16, usize>,
}

impl Contributions {
    /// Only the parts the main mod writes for `tables`.
    fn within(&self, tables: &BTreeSet<Shared>) -> Self {
        Self {
            crosshair: if tables.contains(&Shared::Crosshair) {
                self.crosshair.clone()
            } else {
                Vec::new()
            },
            hud_names: if tables.contains(&Shared::HudNames) {
                self.hud_names.clone()
            } else {
                Vec::new()
            },
            banks: if tables.contains(&Shared::Banks) {
                self.banks.clone()
            } else {
                Default::default()
            },
            characters: if tables.contains(&Shared::Character) {
                self.characters.clone()
            } else {
                Vec::new()
            },
            human_actions: if tables.contains(&Shared::Character) {
                self.human_actions.clone()
            } else {
                Vec::new()
            },
            collections: if tables.contains(&Shared::Collections) {
                self.collections.clone()
            } else {
                BTreeMap::new()
            },
        }
    }

    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// What one importer's build adds to the stock Collections lists, counted from its recipes and
/// the stock tables before any pass (`item::census`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Census {
    /// How many entries go before the stock entries of each stock page's collectibles, by node.
    pub(crate) fronts: BTreeMap<usize, usize>,
    /// How many of its entries each stock Collections objective counts, by objective.
    pub(crate) counts: BTreeMap<u16, usize>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
fn session() -> MutexGuard<'static, Option<Session>> {
    SESSION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
pub(crate) fn clear() {
    *session() = None;
}

/// The empty session a build's turn starts with. A build without mod folders compiles in it, and
/// [`begin`] replaces it for a mod-loader build.
pub(crate) fn begin_native() {
    *session() = Some(Session::default());
}

/// The recipes and maps one member's build compiles, by their places in the project and request.
#[derive(Clone, Debug, Default)]
pub(crate) struct Partition {
    /// The importer whose mod holds the build's files, or `None` for the main mod.
    pub(crate) owner: Option<Importer>,
    pub(crate) recipes: Vec<usize>,
    pub(crate) maps: Vec<usize>,
}

/// The family build's state between passes: the plan, what the passes so far decided, and the
/// pass every pass starts from. [`begin`] makes it. The workflow starts each pass with
/// [`Family::start_pass`], reads what the pass recorded with [`Family::finish_pass`], and either
/// builds again with [`Family::retry`] or ends the passes with [`Family::commit`].
#[derive(Default)]
pub(crate) struct Family {
    plan: Plan,
    decisions: Decisions,
    start: Pass,
}

/// What a pass's member builds recorded that decides whether the family builds again.
#[derive(Default)]
pub(crate) struct Outcome {
    main_builds: bool,
    produced: BTreeMap<Option<Importer>, BTreeMap<u32, Shared>>,
    recorded: BTreeMap<Importer, Contributions>,
    /// How many entries the importers put before the stock entries of each stock collectible
    /// array in the pass.
    fronts: BTreeMap<(usize, usize), usize>,
    abilities: BTreeMap<Option<Importer>, Abilities>,
}

impl Outcome {
    /// The importers that must build with the main mod because one member's perks check an
    /// imported ability another member's subclass carries, each with a sentence saying why. A
    /// perk accepts an imported ability by its allocated root, which only the build that
    /// allocates it knows, so the perk and the subclass compile together.
    pub(crate) fn split_abilities(&self) -> BTreeMap<Importer, Vec<String>> {
        let name = |member: Option<Importer>| member.map_or("Parhelion", Importer::name);
        let mut coupled = BTreeMap::<Importer, Vec<String>>::new();
        for (checker, checks) in &self.abilities {
            for (carrier, carries) in &self.abilities {
                if checker == carrier || checks.checked.is_disjoint(&carries.carried) {
                    continue;
                }
                if let Some(importer) = *checker {
                    coupled.entry(importer).or_default().push(format!(
                        "Its perks check abilities that {}'s subclasses import.",
                        name(*carrier)
                    ));
                }
                if let Some(importer) = *carrier {
                    coupled.entry(importer).or_default().push(format!(
                        "{}'s perks check abilities that its subclasses import.",
                        name(*checker)
                    ));
                }
            }
        }
        coupled
    }

    /// The shared tables `owner`'s build changed in the pass, by stock entry.
    pub(crate) fn produced_by(&self, owner: Option<Importer>) -> BTreeMap<u32, Shared> {
        self.produced.get(&owner).cloned().unwrap_or_default()
    }
}

/// What the review of a pass changes for the next one.
#[derive(Default)]
pub(crate) struct Retry {
    /// Importers that compile with the main mod's content from the next pass on, and why.
    pub(crate) coupled: BTreeMap<Importer, Vec<String>>,
    /// Whether the main mod builds from the next pass on to host shared rows, though no recipe
    /// or map is its own.
    pub(crate) host: bool,
    /// Shared tables whose rows for an importer's items the main mod's build writes from the next
    /// pass on.
    pub(crate) hosted: BTreeMap<Importer, BTreeSet<Shared>>,
}

impl Retry {
    /// Whether the review changes nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.coupled.is_empty() && self.hosted.is_empty() && !self.host
    }
}

impl Family {
    /// The family as `current` holds it, with its pass as the start of every pass.
    fn of(current: &Session) -> Self {
        Self {
            plan: current.plan.clone(),
            decisions: current.decisions.clone(),
            start: current.pass.clone(),
        }
    }

    /// The builds of a pass: the main mod's first, then each importer's whose content compiles on
    /// its own, in importer order. A build without content is left out. For a layout without mod
    /// folders, everything is the main mod's.
    pub(crate) fn partitions(&self, recipes: usize, maps: usize) -> Vec<Partition> {
        let owner = |owners: &[Option<Importer>], index: usize| {
            owners
                .get(index)
                .copied()
                .flatten()
                .filter(|owner| !self.decisions.coupled.contains_key(owner))
        };
        let mut parts = BTreeMap::<Option<Importer>, Partition>::new();
        for index in 0..recipes {
            let owner = owner(&self.plan.recipe_owners, index);
            let part = parts.entry(owner).or_default();
            part.owner = owner;
            part.recipes.push(index);
        }
        for index in 0..maps {
            let owner = owner(&self.plan.map_owners, index);
            let part = parts.entry(owner).or_default();
            part.owner = owner;
            part.maps.push(index);
        }
        // A main mod that hosts the importers' shared rows builds without content of its own.
        if self.decisions.host {
            parts.entry(None).or_default();
        }
        // `None` sorts first, so the main mod builds first.
        parts.into_values().collect()
    }

    /// Whether the next pass builds the main mod, as [`Self::partitions`] decides: some recipe or
    /// map is its own or a coupled importer's, or it hosts the importers' rows.
    fn main_builds(&self) -> bool {
        let own = |owners: &[Option<Importer>]| {
            owners
                .iter()
                .any(|owner| owner.is_none_or(|owner| self.decisions.coupled.contains_key(&owner)))
        };
        self.decisions.host || own(&self.plan.recipe_owners) || own(&self.plan.map_owners)
    }

    /// The shared tables whose rows for `importer`'s items the main mod's build writes.
    pub(crate) fn hosting(&self, importer: Importer) -> BTreeSet<Shared> {
        self.decisions
            .hosting
            .get(&importer)
            .cloned()
            .unwrap_or_default()
    }

    /// Takes what every importer that builds on its own adds to the stock Collections lists,
    /// counted from its recipes before the first pass, in place of what the install's last build
    /// recorded: how many entries lead each list, which the main mod's build keeps rows for, and
    /// what each stock objective counts, which it writes when it hosts the counts. Then the first
    /// pass's main mod build already holds them, and a change to an importer's recipes takes no
    /// second pass for them. A count that differs from what an importer's build records still
    /// leaves the pass unsettled, and the next pass takes the recorded one.
    pub(crate) fn expect_collections(&mut self, censuses: &BTreeMap<Importer, Census>) {
        let mut fronts = BTreeMap::new();
        for census in censuses.values() {
            for (&node, &rows) in &census.fronts {
                *fronts
                    .entry((
                        node,
                        crate::progression::PRESENTATION_NODE_COLLECTIBLES_OFFSET,
                    ))
                    .or_default() += rows;
            }
        }
        self.decisions.fronts = fronts;
        let main_builds = self.main_builds();
        for (importer, census) in censuses {
            let hosted = hosted_tables(&self.decisions, main_builds, *importer)
                .contains(&Shared::Collections);
            self.decisions
                .contributions
                .entry(*importer)
                .or_default()
                .collections = if hosted {
                census.counts.clone()
            } else {
                BTreeMap::new()
            };
        }
        self.decisions
            .contributions
            .retain(|_, contributions| !contributions.is_empty());
    }

    /// Starts a pass from the planned one. The slot then holds the plan, the decisions so far and
    /// a fresh pass that knows whether the main mod builds, which lets it write the importers'
    /// rows, and how many badges each member adds, for the Badges total and root the main mod's
    /// build writes.
    /// `badges` counts each member's badges once the slot holds the pass's decisions, which say
    /// whose items take a source badge in the main mod.
    pub(crate) fn start_pass(
        &self,
        main_builds: bool,
        badges: impl FnOnce() -> BTreeMap<Option<Importer>, usize>,
    ) {
        *session() = Some(Session {
            plan: self.plan.clone(),
            decisions: self.decisions.clone(),
            pass: Pass {
                main_builds,
                ..self.start.clone()
            },
        });
        let badges = badges();
        if let Some(current) = session().as_mut() {
            current.pass.badges = Some(badges);
        }
    }

    /// What the running pass's member builds recorded.
    pub(crate) fn finish_pass(&self) -> Outcome {
        let guard = session();
        let Some(current) = guard.as_ref() else {
            return Outcome::default();
        };
        Outcome {
            main_builds: current.pass.main_builds,
            produced: current.pass.produced.clone(),
            recorded: current.pass.recorded.clone(),
            fronts: importer_fronts(&current.pass),
            abilities: current.pass.abilities.clone(),
        }
    }

    /// How many Collections lists the importers put a different number of entries first in than
    /// the pass's main mod build kept rows for.
    pub(crate) fn fronts_changed(&self, outcome: &Outcome) -> usize {
        outcome
            .fronts
            .keys()
            .chain(self.decisions.fronts.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|list| outcome.fronts.get(list) != self.decisions.fronts.get(list))
            .count()
    }

    /// Whether the pass's main mod build wrote what every importer's build recorded, for the
    /// tables it hosts for each, and kept as many rows before the stock Collections entries as
    /// the importers put there.
    pub(crate) fn settled(&self, outcome: &Outcome) -> bool {
        outcome.fronts == self.decisions.fronts
            && outcome
                .recorded
                .keys()
                .chain(self.decisions.contributions.keys())
                .all(|importer| {
                    let tables = hosted_tables(&self.decisions, outcome.main_builds, *importer);
                    let recorded = outcome
                        .recorded
                        .get(importer)
                        .map(|contributions| contributions.within(&tables))
                        .unwrap_or_default();
                    self.decisions
                        .contributions
                        .get(importer)
                        .cloned()
                        .unwrap_or_default()
                        == recorded
                })
    }

    /// Applies the review of the pass that ended. The importers it couples and the tables it
    /// hosts stay so for every later pass. The main mod keeps as many rows before the stock
    /// Collections entries as the importers put there in the pass. Then what the importers
    /// recorded in the pass, for the tables the main mod hosts for each in the next pass, is what
    /// the main mod's build writes there. The tables follow the next pass, not the one that
    /// ended: after a pass without a main mod build, the next one that hosts the bank rows and
    /// Collections counts writes them at once. Filtered by the pass that ended, they were left
    /// out, the importers recorded them again and the family took a third pass (2026-11-10). A
    /// coupled importer's items compile in the main mod's build, which writes their rows itself,
    /// so what that importer recorded on its own is left out. Its rows name assets its own build
    /// allocated, which the next pass no longer has, and keeping them would leave the next pass
    /// unsettled and cost another.
    pub(crate) fn retry(&mut self, outcome: Outcome, retry: Retry) {
        for (importer, reasons) in retry.coupled {
            self.decisions
                .coupled
                .entry(importer)
                .or_default()
                .extend(reasons);
        }
        self.decisions.host |= retry.host;
        for (importer, tables) in retry.hosted {
            self.decisions
                .hosting
                .entry(importer)
                .or_default()
                .extend(tables);
        }
        self.decisions.fronts = outcome.fronts;
        let main_builds = self.main_builds();
        self.decisions.contributions = outcome
            .recorded
            .iter()
            .filter(|(importer, _)| !self.decisions.coupled.contains_key(importer))
            .map(|(importer, contributions)| {
                let tables = hosted_tables(&self.decisions, main_builds, *importer);
                (*importer, contributions.within(&tables))
            })
            .filter(|(_, contributions)| !contributions.is_empty())
            .collect();
    }

    /// Ends the passes. The slot keeps the last pass with the plan and decisions it built with,
    /// which staging and the receipts read until the build's turn ends.
    pub(crate) fn commit(self) {
        if let Some(current) = session().as_mut() {
            current.plan = self.plan;
            current.decisions = self.decisions;
            if let Some(path) = &current.plan.seed {
                seed::Seed::of(current).save(path);
            }
        }
    }
}

/// The family state of the running pass at one point, kept so a later pass can repeat what one
/// importer's build did to it without building the importer again.
pub(crate) struct Snapshot(Pass);

/// The running pass's state, taken around an importer's build.
pub(crate) fn snapshot() -> Option<Snapshot> {
    session()
        .as_ref()
        .map(|current| Snapshot(current.pass.clone()))
}

impl Snapshot {
    /// The shared entries whose main mod payloads `owner`'s build followed, once it has built.
    pub(crate) fn reads(&self, owner: Importer) -> BTreeSet<u32> {
        self.0.reads.get(&owner).cloned().unwrap_or_default()
    }
}

/// A digest of everything `owner`'s build reads from the family before it compiles, with the pass
/// as `at` holds it: the decisions that apply to it, the ids the pass has taken, the Collections
/// arrays earlier members lead and the main mod's finished payloads of the shared entries in
/// `reads`, which the build recorded it follows. A build that gets the same digest in two passes
/// of one turn builds the same bytes and changes the pass the same way, since nothing else it
/// reads changes between passes. Its packages follow no other shared entry's payload, so a pass
/// whose main mod writes other bank rows keeps it.
pub(crate) fn member_inputs(
    owner: Importer,
    at: &Snapshot,
    reads: &BTreeSet<u32>,
) -> Option<[u8; 32]> {
    use sha2::{Digest, Sha256};
    let guard = session();
    let current = guard.as_ref()?;
    let (decisions, pass) = (&current.decisions, &at.0);
    let mut digest = Sha256::new();
    let mut field = |text: String| {
        digest.update((text.len() as u64).to_le_bytes());
        digest.update(text.as_bytes());
    };
    field(format!("{:?}", decisions.hosting.get(&owner)));
    field(format!(
        "{:?}",
        decisions.coupled.keys().collect::<Vec<_>>()
    ));
    field(format!("{}/{}", decisions.host, pass.main_builds));
    field(format!("{:?}", pass.imports));
    field(format!(
        "{:?}/{:?}/{:?}/{:?}",
        pass.reserved, pass.native, pass.private, pass.records
    ));
    field(format!("{:?}", pass.leads));
    // Where its entries go before the stock Collections entries follows the earlier members', and
    // its badge's place whether the main mod has a badge.
    field(format!("{:?}", pass.fronts));
    field(format!("{:?}", pass.badges));
    field(format!("{:?}", pass.mission_scripts));
    // An entry the main mod doesn't write reads as stock, which a pass that writes it changes.
    for tag in reads {
        let payload = pass.hosted_payloads.get(tag).map_or_else(
            || "stock".to_owned(),
            |payload| hex::encode(Sha256::digest(payload)),
        );
        field(format!("{tag:08X}/{payload}"));
    }
    Some(digest.finalize().into())
}

/// Repeats in the running pass what `owner`'s build did to the pass between `before` and `after`
/// in the pass before: its own allocation, records and Collections leads, and the ids it took
/// from the window. The caller has checked that the build got the same inputs, so the ids it
/// takes are still free and follow the same earlier ones.
pub(crate) fn replay(owner: Importer, before: &Snapshot, after: &Snapshot) {
    let mut guard = session();
    let Some(current) = guard.as_mut() else {
        return;
    };
    let (before, after, pass) = (&before.0, &after.0, &mut current.pass);
    if let Some(taken) = after.imports.get(&owner) {
        pass.imports.insert(owner, taken.clone());
    }
    pass.reserved
        .extend(after.reserved.difference(&before.reserved));
    pass.native.extend(after.native.difference(&before.native));
    pass.private.extend_from_slice(
        after
            .private
            .get(before.private.len()..)
            .unwrap_or_default(),
    );
    pass.records.extend_from_slice(
        after
            .records
            .get(before.records.len()..)
            .unwrap_or_default(),
    );
    let member = Some(owner);
    if let Some(produced) = after.produced.get(&member) {
        pass.produced.insert(member, produced.clone());
    }
    if let Some(recorded) = after.recorded.get(&owner) {
        pass.recorded.insert(owner, recorded.clone());
    }
    if let Some(tables) = after.needs_main.get(&owner) {
        pass.needs_main.insert(owner, tables.clone());
    }
    if let Some(reads) = after.reads.get(&owner) {
        pass.reads.insert(owner, reads.clone());
    }
    if let Some(abilities) = after.abilities.get(&member) {
        pass.abilities.insert(member, abilities.clone());
    }
    for (list, leaders) in &after.leads {
        if let Some(&first) = leaders.get(&member) {
            pass.leads.entry(*list).or_default().insert(member, first);
        }
    }
    for (list, front) in &after.fronts {
        if let Some(&rows) = front.members.get(&member) {
            pass.fronts
                .entry(*list)
                .or_insert_with(|| Front {
                    stock: front.stock,
                    members: BTreeMap::new(),
                })
                .members
                .insert(member, rows);
        }
    }
    for (name, script) in &after.mission_scripts {
        if !before.mission_scripts.contains_key(name) {
            pass.mission_scripts.insert(name.clone(), script.clone());
        }
    }
}

/// Whether the build now compiling is a main mod that hosts the importers' shared rows, which
/// builds even with no recipe or map of its own.
pub(crate) fn main_hosts() -> bool {
    crate::package_profile::member().is_none()
        && session()
            .as_ref()
            .is_some_and(|current| current.decisions.host)
}

/// The importers whose recipes compile with the main mod's, and why.
pub(crate) fn coupled() -> BTreeMap<Importer, Vec<String>> {
    session()
        .as_ref()
        .map(|current| current.decisions.coupled.clone())
        .unwrap_or_default()
}

/// Whether the importer now compiling leaves its rows of `table` to the main mod's build. Its
/// bank rows always go there.
pub(crate) fn hosts(table: Shared) -> bool {
    let Some(owner) = crate::package_profile::member() else {
        return false;
    };
    session().as_ref().is_some_and(|current| {
        hosted_tables(&current.decisions, current.pass.main_builds, owner).contains(&table)
    })
}

/// How many badges the family's members add in this pass, or `None` outside a family build.
pub(crate) fn badges() -> Option<usize> {
    session()
        .as_ref()
        .and_then(|current| Some(current.pass.badges.as_ref()?.values().sum()))
}

/// Whether the main mod's build adds a badge in this pass, which then takes the Badges root's
/// first place, ahead of the importers' places.
pub(crate) fn main_has_badge() -> bool {
    session().as_ref().is_some_and(|current| {
        current
            .pass
            .badges
            .as_ref()
            .and_then(|badges| badges.get(&None))
            .is_some_and(|count| *count > 0)
    })
}

/// Whether `importer`'s build adds a badge in this pass.
pub(crate) fn has_badge(importer: Importer) -> bool {
    session().as_ref().is_some_and(|current| {
        current
            .pass
            .badges
            .as_ref()
            .and_then(|badges| badges.get(&Some(importer)))
            .is_some_and(|count| *count > 0)
    })
}

/// Records that the member now compiling puts entries before the stock entries of the array at
/// `array` of stock node `node`, and whether items set to First put them there.
pub(crate) fn lead(node: usize, array: usize, first: bool) {
    let owner = crate::package_profile::member();
    if let Some(current) = session().as_mut() {
        let first_here = current
            .pass
            .leads
            .entry((node, array))
            .or_default()
            .entry(owner)
            .or_default();
        *first_here |= first;
    }
}

/// Whether a member that compiled before the one now compiling, in this pass, leads the array at
/// `array` of stock node `node`. Members compile in a fixed order, the main mod first.
pub(crate) fn led_before(node: usize, array: usize) -> bool {
    let owner = crate::package_profile::member();
    session().as_ref().is_some_and(|current| {
        current
            .pass
            .leads
            .get(&(node, array))
            .is_some_and(|leaders| leaders.keys().any(|leader| *leader != owner))
    })
}

/// Records the imported abilities whose roots the member now compiling allocated.
pub(crate) fn carries_abilities(abilities: impl IntoIterator<Item = (String, u32)>) {
    let owner = crate::package_profile::member();
    if let Some(current) = session().as_mut() {
        let entry = current.pass.abilities.entry(owner).or_default();
        entry.carried.extend(abilities);
    }
}

/// Records the imported abilities a perk of the member now compiling checks.
pub(crate) fn checks_abilities(abilities: impl IntoIterator<Item = (String, u32)>) {
    let owner = crate::package_profile::member();
    if let Some(current) = session().as_mut() {
        let entry = current.pass.abilities.entry(owner).or_default();
        entry.checked.extend(abilities);
    }
}

/// Records that the member now compiling puts `count` more entries before the stock entries of
/// the collectible array at `array` of stock node `node`, which holds `rows` entries now, and
/// returns where they go. Their row follows every earlier member's entries and the member's own
/// earlier ones. Members compile in a fixed order, the main mod first, so every other member
/// recorded here compiled before this one. The main mod's entries push the stock entries down, so
/// its row is 0, and its build later keeps the rows the importers take ([`kept_fronts`]). The
/// stock count is the array's length when the first member puts entries there, which every
/// member reads before it adds any after the stock entries. Outside a family build the row is 0.
pub(crate) fn front(node: usize, array: usize, count: usize, rows: usize) -> Slot {
    let owner = crate::package_profile::member();
    let mut guard = session();
    let Some(current) = guard.as_mut() else {
        return Slot {
            row: 0,
            stock: rows,
        };
    };
    let front = current
        .pass
        .fronts
        .entry((node, array))
        .or_insert_with(|| Front {
            stock: rows,
            members: BTreeMap::new(),
        });
    let row = if owner.is_none() {
        0
    } else {
        front.members.values().sum()
    };
    *front.members.entry(owner).or_default() += count;
    Slot {
        row,
        stock: front.stock,
    }
}

/// For the main mod's build, each stock collectible array whose stock entries move down past the
/// importers' entries, by node and array offset, with how many entries the main mod put first
/// there and how many the importers do, as the pass before recorded them. The main mod's build
/// keeps that many stock rows after its own entries for the importers to write over. Empty for
/// any other member's build.
pub(crate) fn kept_fronts() -> BTreeMap<(usize, usize), (usize, usize)> {
    if crate::package_profile::member().is_some() {
        return BTreeMap::new();
    }
    let guard = session();
    let Some(current) = guard.as_ref() else {
        return BTreeMap::new();
    };
    current
        .decisions
        .fronts
        .iter()
        .filter(|(_, rows)| **rows > 0)
        .map(|(list, rows)| {
            let own = current
                .pass
                .fronts
                .get(list)
                .and_then(|front| front.members.get(&None))
                .copied()
                .unwrap_or_default();
            (*list, (own, *rows))
        })
        .collect()
}

/// How many entries the importers put before the stock entries of each stock collectible array
/// in `pass`.
fn importer_fronts(pass: &Pass) -> BTreeMap<(usize, usize), usize> {
    pass.fronts
        .iter()
        .map(|(list, front)| {
            let rows = front
                .members
                .iter()
                .filter(|(member, _)| member.is_some())
                .map(|(_, rows)| rows)
                .sum::<usize>();
            (*list, rows)
        })
        .filter(|(_, rows)| *rows > 0)
        .collect()
}

/// Records that the member now compiling changed `tags` of `table`.
pub(crate) fn produced(table: Shared, tags: impl IntoIterator<Item = u32>) {
    let owner = crate::package_profile::member();
    if let Some(current) = session().as_mut() {
        let produced = current.pass.produced.entry(owner).or_default();
        for tag in tags {
            produced.insert(tag, table);
        }
    }
}

/// The shared tables `owner`'s build changed in this pass, by stock entry.
pub(crate) fn produced_by(owner: Option<Importer>) -> BTreeMap<u32, Shared> {
    session()
        .as_ref()
        .and_then(|current| current.pass.produced.get(&owner).cloned())
        .unwrap_or_default()
}

/// Records what the importer now compiling adds to the shared tables, whether or not the main
/// mod writes it yet, so the next pass can.
pub(crate) fn contribute(record: impl FnOnce(&mut Contributions)) {
    let Some(owner) = crate::package_profile::member() else {
        return;
    };
    if let Some(current) = session().as_mut() {
        record(current.pass.recorded.entry(owner).or_default());
    }
}

/// What the main mod's build writes for the importers in this pass, in importer order. Empty for
/// any other member's build.
pub(crate) fn contributions() -> Vec<Contributions> {
    contributors()
        .into_iter()
        .map(|(_, contributions)| contributions)
        .collect()
}

/// [`contributions`] with the importer each came from.
pub(crate) fn contributors() -> Vec<(Importer, Contributions)> {
    if crate::package_profile::member().is_some() {
        return Vec::new();
    }
    session()
        .as_ref()
        .map(|current| {
            current
                .decisions
                .contributions
                .iter()
                .map(|(importer, contributions)| (*importer, contributions.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// The tables whose rows for `importer`'s items the main mod's build writes: the ones `decisions`
/// host for it, and the ability banks and Collections counts whenever the main mod builds.
fn hosted_tables(decisions: &Decisions, main_builds: bool, importer: Importer) -> BTreeSet<Shared> {
    let mut tables = decisions
        .hosting
        .get(&importer)
        .cloned()
        .unwrap_or_default();
    if main_builds {
        tables.insert(Shared::Banks);
        tables.insert(Shared::Collections);
    }
    tables
}

/// Keeps the main mod's finished payload of a shared entry for the importers that build on it
/// or name its blocks by offset in this pass.
pub(crate) fn finish_shared(tag: u32, payload: &[u8]) {
    if crate::package_profile::member().is_some() {
        return;
    }
    if let Some(current) = session().as_mut() {
        current.pass.hosted_payloads.insert(tag, payload.to_vec());
    }
}

/// The main mod's finished payload of `tag` in this pass, for an importer's build that hosts
/// `table` with it. The importer then needs the main mod to load.
pub(crate) fn shared_payload(table: Shared, tag: u32) -> Option<Vec<u8>> {
    let owner = crate::package_profile::member()?;
    if !hosts(table) {
        return None;
    }
    reads_shared([tag]);
    let mut guard = session();
    let current = guard.as_mut()?;
    let payload = current.pass.hosted_payloads.get(&tag).cloned()?;
    current
        .pass
        .needs_main
        .entry(owner)
        .or_default()
        .insert(table);
    Some(payload)
}

/// Records that the importer now compiling follows the main mod's payloads of `tags`, whether or
/// not the main mod writes them in this pass.
pub(crate) fn reads_shared(tags: impl IntoIterator<Item = u32>) {
    let Some(owner) = crate::package_profile::member() else {
        return;
    };
    if let Some(current) = session().as_mut() {
        current.pass.reads.entry(owner).or_default().extend(tags);
    }
}

/// The main mod's finished payloads of every shared entry it wrote in this pass, for an
/// importer's build, which records the ones it follows ([`reads_shared`]).
pub(crate) fn shared_payloads() -> BTreeMap<u32, Vec<u8>> {
    if crate::package_profile::member().is_none() {
        return BTreeMap::new();
    }
    session()
        .as_ref()
        .map(|current| current.pass.hosted_payloads.clone())
        .unwrap_or_default()
}

/// Records that the importer now compiling names the main mod's finished payloads of `table`, so
/// it needs the main mod to load.
pub(crate) fn needs_main(table: Shared) {
    let Some(owner) = crate::package_profile::member() else {
        return;
    };
    if let Some(current) = session().as_mut() {
        current
            .pass
            .needs_main
            .entry(owner)
            .or_default()
            .insert(table);
    }
}

/// The shared tables whose rows the main mod's build holds for `importer`, or whose finished
/// payloads its build names, which make it need the main mod.
pub(crate) fn hosted_for(importer: Importer) -> BTreeSet<Shared> {
    session()
        .as_ref()
        .map(|current| needed_from_main(current, importer))
        .unwrap_or_default()
}

/// [`hosted_for`] in `current`. The Collections counts aren't among them: without the main mod
/// an importer's items still load and only leave the stock pages' counts as they were, so they
/// don't make the importer require it. Every importer with items records counts whenever the
/// main mod builds.
fn needed_from_main(current: &Session, importer: Importer) -> BTreeSet<Shared> {
    let mut tables = current
        .pass
        .needs_main
        .get(&importer)
        .cloned()
        .unwrap_or_default();
    if let Some(contributions) = current.decisions.contributions.get(&importer) {
        for (table, empty) in [
            (Shared::Crosshair, contributions.crosshair.is_empty()),
            (Shared::HudNames, contributions.hud_names.is_empty()),
            (Shared::Banks, contributions.banks.is_empty()),
            (
                Shared::Character,
                contributions.characters.is_empty() && contributions.human_actions.is_empty(),
            ),
        ] {
            if !empty {
                tables.insert(table);
            }
        }
    }
    tables
}

/// The window's highest id that no package of the build or an importer holds, now reserved.
fn fresh(pass: &mut Pass) -> Result<u16, String> {
    let window = crate::package_profile::window();
    let host = crate::package_profile::host().id;
    let id = (window.base..=window.top)
        .rev()
        .find(|id| *id != host && !pass.reserved.contains(id) && !pass.native.contains(id))
        .ok_or("No free package ID remains for authored records")?;
    pass.reserved.insert(id);
    Ok(id)
}

/// The id at `position` of the `kind` an importer's own build takes: the one its installed
/// build took there, or a fresh one.
fn own_id(
    current: &mut Session,
    owner: Importer,
    kind: OwnKind,
    position: usize,
) -> Result<u16, String> {
    let Session { plan, pass, .. } = current;
    let previous = &plan
        .imports
        .get(&owner)
        .ok_or("An importer's build has no session")?
        .previous;
    loop {
        let taken = pass
            .imports
            .get_mut(&owner)
            .ok_or("An importer's build has no session")?
            .state
            .own
            .list(kind);
        if let Some(&id) = taken.get(position) {
            return Ok(id);
        }
        let next = taken.len();
        let id = match previous.of(kind).get(next).copied() {
            Some(id) => id,
            None => fresh(pass)?,
        };
        pass.imports
            .get_mut(&owner)
            .unwrap()
            .state
            .own
            .list(kind)
            .push(id);
    }
}

/// The records host of `owner`'s own build.
pub(crate) fn member_host(owner: Importer) -> Result<u16, String> {
    let mut guard = session();
    let Session { plan, pass, .. } = guard.as_mut().ok_or("An importer's build has no session")?;
    let previous = plan
        .imports
        .get(&owner)
        .ok_or("An importer's build has no session")?
        .previous
        .host;
    let taken = pass
        .imports
        .get(&owner)
        .ok_or("An importer's build has no session")?
        .state
        .own
        .host;
    let id = match taken.or(previous) {
        Some(id) => id,
        None => fresh(pass)?,
    };
    pass.imports.get_mut(&owner).unwrap().state.own.host = Some(id);
    Ok(id)
}

/// The package at `position` of the asset packages that `owner`'s own build adds outside its
/// resources, where the main mod's take the window's ids in order.
pub(crate) fn member_asset(owner: Importer, position: usize) -> Result<u16, String> {
    let mut guard = session();
    let current = guard.as_mut().ok_or("An importer's build has no session")?;
    own_id(current, owner, OwnKind::Assets, position)
}

/// Allocation-only snapshot of the running pass, for speculative runtime-cache relocation.
pub(crate) struct Reservations {
    reserved: BTreeSet<u16>,
    native: BTreeSet<u16>,
    private: Vec<u16>,
    records: Vec<u16>,
    own: BTreeMap<Importer, Own>,
}

pub(crate) fn reservations() -> Reservations {
    let mut guard = session();
    let pass = &guard.get_or_insert_with(Session::default).pass;
    Reservations {
        reserved: pass.reserved.clone(),
        native: pass.native.clone(),
        private: pass.private.clone(),
        records: pass.records.clone(),
        own: pass
            .imports
            .iter()
            .map(|(owner, taken)| (*owner, taken.state.own.clone()))
            .collect(),
    }
}

pub(crate) fn restore_reservations(saved: Reservations) {
    let mut guard = session();
    let pass = &mut guard.get_or_insert_with(Session::default).pass;
    pass.reserved = saved.reserved;
    pass.native = saved.native;
    pass.private = saved.private;
    pass.records = saved.records;
    for (owner, own) in saved.own {
        if let Some(taken) = pass.imports.get_mut(&owner) {
            taken.state.own = own;
        }
    }
}

pub(crate) fn mission_script(name: &str, source: String) -> Result<(), String> {
    let mut guard = session();
    let session = guard
        .as_mut()
        .ok_or("Mission controller has no active build")?;
    if session
        .pass
        .mission_scripts
        .insert(name.to_owned(), source)
        .is_some()
    {
        return Err(format!(
            "Mission controller {name} was generated more than once"
        ));
    }
    Ok(())
}

pub(crate) fn mission_scripts() -> BTreeMap<String, String> {
    session()
        .as_ref()
        .map(|s| s.pass.mission_scripts.clone())
        .unwrap_or_default()
}
pub(crate) fn reserved(id: u16) -> bool {
    session()
        .as_ref()
        .is_some_and(|session| session.pass.reserved.contains(&id))
}
pub(crate) fn native(id: u16) {
    if let Some(session) = session().as_mut() {
        session.pass.native.insert(id);
    }
}
pub(crate) fn owner(id: u16) -> Option<Importer> {
    session()
        .as_ref()?
        .pass
        .imports
        .iter()
        .find(|(_, taken)| taken.used.contains(&id))
        .map(|(owner, _)| *owner)
}
pub(crate) fn is_kept(owner: Importer) -> bool {
    session()
        .as_ref()
        .and_then(|s| s.plan.imports.get(&owner))
        .is_some_and(|i| i.kept)
}

fn hash(value: &impl Serialize) -> Result<String, String> {
    Ok(
        crate::artifact::digest_bytes(&serde_json::to_vec(value).map_err(|e| e.to_string())?)
            .sha256,
    )
}

fn fingerprint(inputs: &str, allocation: &[u16], state: &State) -> Result<String, String> {
    hash(&(FORMAT, inputs, allocation, state))
}

/// The origin of a complete graph. A graph with native nodes or borrowing installed shader
/// textures remains native, as does an explicitly mixed graph. Kept donor parts don't make a
/// graph native. Each is the import's own private copy of a stock donor part, under a key from
/// the importer's own key space, and its rows are the ones the import's own parts write, so the
/// importer's mod builds it from stock and merges it as it merges the rest of the import.
#[cfg(feature = "d2-model-importer")]
pub(crate) fn graph_origin(
    graph: &serde_json::Value,
    spec: Option<&crate::WeaponCloneSpec>,
) -> Option<Importer> {
    let owner = match graph["source"].as_str() {
        Some(source) => Importer::from_source(source)?,
        // The D2 shader translator predates an explicit source marker. This recorded adapter
        // belongs to that converter. Graphs without either provenance stay in the main mod.
        None if graph["source_shader_adapter"]["source_shader_equations_retained"] == true
            && graph["source_models"].is_array() =>
        {
            Importer::D2Mot
        }
        None => return None,
    };
    if graph["nodes"].as_array()?.iter().any(|node| {
        node["native"] == true
            || node["source"]
                .as_str()
                .is_some_and(|source| source != owner.source())
    }) || spec.is_some_and(|spec| {
        spec.overrides.dye_texture_edits.iter().any(|edit| {
            [edit.detail, edit.normal]
                .into_iter()
                .flatten()
                .any(|tag| crate::shader::texture_symbol(tag).is_none())
        })
    }) {
        return None;
    }
    Some(owner)
}

/// The importer whose badge `spec`'s item takes in the main mod's build, by the rule that gives a
/// recipe to an importer's mod: every imported recipe's importer in a build without mod folders,
/// and in a family build an importer that builds with the main mod. An importer building in its
/// own mod gives its items that mod's badge, so its recipes have none here. Before 2026-11-09 a
/// coupled importer's items joined Project Sunrise, and only the importers left in their own mods
/// showed a badge.
pub(crate) fn source_of(spec: &crate::WeaponCloneSpec) -> Option<Importer> {
    let standalone = crate::package_profile::host().is_standalone();
    session().as_ref().and_then(|current| {
        let source = current
            .plan
            .sources
            .get(&spec.identity.item_hash)
            .copied()?;
        (!standalone || current.decisions.coupled.contains_key(&source)).then_some(source)
    })
}

/// The importer each recipe belongs to, by item hash: the one every graph of the recipe, its own
/// and its attachments', comes from.
#[cfg(feature = "d2-model-importer")]
fn sources(weapons: &[crate::WeaponCloneSpec]) -> Result<BTreeMap<u32, Importer>, String> {
    let mut sources = BTreeMap::new();
    for spec in weapons {
        let Some(reference) = &spec.overrides.imported_graph else {
            continue;
        };
        let mut owners = Vec::new();
        for (graph, binding) in std::iter::once((reference, Some(spec))).chain(
            reference.attachments.iter().map(|attachment| {
                (
                    attachment,
                    (spec.kind == crate::ItemKind::Armor).then_some(spec),
                )
            }),
        ) {
            let description: serde_json::Value = serde_json::from_slice(
                &fs::read(graph.directory.join("asset-graph.json")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            owners.push(graph_origin(&description, binding));
        }
        let first = owners.first().copied().flatten();
        if let Some(owner) = first.filter(|first| owners.iter().all(|o| *o == Some(*first))) {
            sources.insert(spec.identity.item_hash, owner);
        }
    }
    Ok(sources)
}

#[cfg(not(feature = "d2-model-importer"))]
fn sources(_: &[crate::WeaponCloneSpec]) -> Result<BTreeMap<u32, Importer>, String> {
    Ok(BTreeMap::new())
}

#[cfg(feature = "d2-model-importer")]
pub(crate) fn group_key(folder: &Path, spec: Option<&crate::WeaponCloneSpec>) -> String {
    format!(
        "{}:{}",
        folder.display(),
        spec.map_or("", |spec| spec.namespace.as_str())
    )
}

/// Why an importer builds in the main mod when the preference puts every item there.
const ONE_MOD_REASON: &str = "Preferences put every item in the Parhelion mod.";

/// Plans the build: installs immutable input identities before any native tag is allocated, and
/// returns the family every pass starts from. Pins are checked by the ordinary graph validator
/// before and after linking. File hashes, rather than timestamps, decide whether map inputs and
/// installed packages can be reused.
///
/// With `one_mod` every importer builds with the main mod from the first pass, as a review
/// couples one, so the main mod holds every item and resource and installing removes the
/// importers' folders. Without `source_badges` the imported items the main mod holds join Project
/// Sunrise rather than their importers' badges. An importer's own mod keeps its badge either way.
pub(crate) fn begin(
    target: &Path,
    source: &Path,
    weapons: &[crate::WeaponCloneSpec],
    maps: &[crate::map::MapRecipe],
    stock: &[ArtifactMetadata],
    one_mod: bool,
    source_badges: bool,
) -> Result<Family, String> {
    if !crate::package_profile::host().is_standalone() {
        // One mod holds every item, and each importer's items take that importer's badge unless
        // the preference leaves them in Project Sunrise.
        let sources = if source_badges {
            sources(weapons)?
        } else {
            BTreeMap::new()
        };
        let mut guard = session();
        let Some(current) = guard.as_mut() else {
            return Ok(Family::default());
        };
        current.plan.sources = sources;
        return Ok(Family::of(current));
    }
    let mut inputs: BTreeMap<Importer, Vec<serde_json::Value>> = BTreeMap::new();
    let mut templates = BTreeMap::<Importer, BTreeSet<u16>>::new();
    // A recipe belongs to an importer's mod when its graph and every attachment come from that
    // importer, and a map with a source belongs to Halo: Reach's.
    let mut recipe_owners = Vec::with_capacity(weapons.len());
    let map_owners = maps
        .iter()
        .map(|map| map.source.is_some().then_some(Importer::HaloReach))
        .collect::<Vec<_>>();
    #[cfg(feature = "d2-model-importer")]
    let mut map_records = Vec::new();
    #[cfg(feature = "d2-model-importer")]
    {
        for spec in weapons {
            let Some(reference) = &spec.overrides.imported_graph else {
                recipe_owners.push(None);
                continue;
            };
            let mut owners = Vec::new();
            for (graph, binding) in std::iter::once((reference, Some(spec))).chain(
                reference.attachments.iter().map(|attachment| {
                    (
                        attachment,
                        (spec.kind == crate::ItemKind::Armor).then_some(spec),
                    )
                }),
            ) {
                let description: serde_json::Value = serde_json::from_slice(
                    &fs::read(graph.directory.join("asset-graph.json"))
                        .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                let origin = graph_origin(&description, binding);
                owners.push(origin);
                let Some(origin) = origin else {
                    continue;
                };
                let ids = templates.entry(origin).or_default();
                for node in description["nodes"].as_array().into_iter().flatten() {
                    for tag in [node.get("template"), node.get("source_parent")]
                        .into_iter()
                        .flatten()
                        .chain(node["inherited"].as_array().into_iter().flatten())
                        .filter_map(serde_json::Value::as_u64)
                    {
                        ids.insert(TagHash(tag as u32).pkg_id());
                    }
                }
                inputs.entry(origin).or_default().push(serde_json::json!({
                    "key": group_key(&graph.directory, binding),
                    "pin": {
                        "directory": graph.directory,
                        "sha256": graph.sha256,
                        "converter_revision": graph.converter_revision,
                        "material_inputs": graph.material_inputs,
                    },
                    "markers": binding.map(|spec| &spec.overrides.marker_offsets),
                    "dyes": binding.map(|spec| &spec.overrides.dye_edits),
                    "textures": binding.map(|spec| &spec.overrides.dye_texture_edits),
                    "rows": binding.and_then(|spec| spec.overrides.render_dye_rows.as_ref())
                        .map(|rows| rows.iter().map(|row| row.iter().map(|entry|
                            (entry.channel_index, entry.dye_reference_index)
                        ).collect::<Vec<_>>()).collect::<Vec<_>>()),
                    "namespace": binding.map(|spec| &spec.namespace),
                    "identity": binding.map(|spec| spec.identity.item_hash),
                }));
            }
            let first = owners.first().copied().flatten();
            recipe_owners.push(first.filter(|first| owners.iter().all(|o| *o == Some(*first))));
        }
        for map in maps {
            if let Some(source) = &map.source {
                let input = map_inputs(map, source)?;
                inputs
                    .entry(Importer::HaloReach)
                    .or_default()
                    .push(input.clone());
                map_records.push((map.clone(), input));
            }
        }
    }
    #[cfg(not(feature = "d2-model-importer"))]
    let _ = (weapons, maps);
    // A recipe whose importer builds with the main mod still takes that importer's badge there,
    // unless the preference leaves it in Project Sunrise.
    let sources = if source_badges {
        weapons
            .iter()
            .zip(&recipe_owners)
            .filter_map(|(spec, owner)| Some((spec.identity.item_hash, (*owner)?)))
            .collect()
    } else {
        BTreeMap::new()
    };
    let mut plan = Plan {
        recipe_owners,
        map_owners,
        sources,
        ..Plan::default()
    };
    if inputs.is_empty() {
        // The private runtime's packages still take their ids from the session.
        return Ok(install(Session {
            plan,
            ..Session::default()
        }));
    }
    let install_root = target.parent().ok_or("Missing game root")?;
    mod_content::files(install_root)?;
    let directories = mod_content::family_directories(install_root)?;
    let main_installed = directories
        .get(mod_content::ID)
        .map(|directory| installed_member(directory))
        .transpose()?;
    let mut pass = Pass::default();
    #[cfg(feature = "d2-model-importer")]
    {
        plan.maps = map_records;
    }
    for (origin, mut items) in inputs {
        items.sort_by_cached_key(|item| item.to_string());
        let source_inputs = source_dependencies(
            source,
            &templates.remove(&origin).unwrap_or_default(),
            &mut BTreeMap::new(),
        )?;
        #[cfg(feature = "d2-model-importer")]
        let converter = parhelion_import::CONVERTER_REVISION;
        #[cfg(not(feature = "d2-model-importer"))]
        let converter = 0;
        let inputs = hash(&(
            FORMAT,
            env!("CARGO_PKG_VERSION"),
            converter,
            stock,
            &source_inputs,
            items,
        ))?;
        // An importer that built on its own keeps its content in its folder's receipt. One whose
        // items built with the main mod's has no folder: its content is in the main mod's receipt
        // and its resources among the main mod's packages.
        let own_folder = directories
            .get(&origin.id())
            .map(|directory| installed_member(directory))
            .transpose()?;
        let in_main = own_folder.is_none();
        let (installed, content) = match own_folder {
            Some(member) => {
                let content = member.1.receipt.content.clone();
                (Some(member), content)
            }
            None => main_installed
                .as_ref()
                .and_then(|main| {
                    let content = main.1.receipt.coupled.get(origin.source())?.clone();
                    Some((Some(main.clone()), Some(content)))
                })
                .unwrap_or((None, None)),
        };
        let mut allocation = Vec::new();
        let mut state = State {
            format: FORMAT,
            ..Default::default()
        };
        let mut kept = false;
        let mut previous = Own::default();
        if let Some((directory, member)) = &installed {
            verify_installed_artifacts(
                directory,
                member,
                content
                    .as_ref()
                    .map(|content| content.allocation.as_slice()),
                in_main,
            )?;
            if let Some(content) = &content {
                allocation = content.allocation.clone();
                if let Ok(mut last) = serde_json::from_value::<State>(content.state.clone()) {
                    // A map conversion removed since, with the details it kept, builds again.
                    kept = last.format == FORMAT
                        && inputs == content.inputs
                        && fingerprint(&inputs, &allocation, &last)? == content.fingerprint
                        // A member that needs the main mod requires it, which changes nothing
                        // its resources hold.
                        && (in_main
                            || Manifest {
                                dependencies: Vec::new(),
                                ..member.manifest.clone()
                            } == Manifest::for_importer(origin))
                        && dependencies_match(target, &last.dependencies)?
                        && last.maps.values().all(|map| map.text().is_some());
                    // The member's own ids leave the resource allocation, which resources take
                    // by position, and this build records the ids it takes again.
                    previous = std::mem::take(&mut last.own);
                    allocation.retain(|id| !previous.ids().any(|own| own == *id));
                    if kept {
                        state = last;
                    }
                }
            }
            // A member with its own patches lists stock packages too, which it never allocates.
            if allocation.is_empty() && !in_main {
                allocation = member
                    .receipt
                    .artifacts
                    .iter()
                    .filter_map(|artifact| {
                        (artifact.path.starts_with("packages/") && artifact.path.ends_with(".pkg"))
                            .then(|| package_id(&artifact.path))
                            .flatten()
                    })
                    .collect();
            }
        }
        // Every pass starts with the installed allocation and own ids reserved.
        pass.reserved.extend(&allocation);
        pass.reserved.extend(previous.ids());
        plan.imports.insert(
            origin,
            Planned {
                inputs,
                source_inputs,
                installed,
                kept,
                previous,
            },
        );
        pass.imports.insert(
            origin,
            Taken {
                allocation,
                used: BTreeSet::new(),
                state,
                conversions: 0,
            },
        );
    }
    validate_reserved_ids(&pass.reserved)?;
    let decisions = initial_decisions(&mut plan, install_root, one_mod);
    Ok(install(Session {
        plan,
        decisions,
        pass,
    }))
}

/// The decisions the first pass starts from: every importer coupled when one mod holds every
/// item, and otherwise those the install's last settled build kept. Either way the main mod
/// builds whenever an importer has content. Only its build counts every member's badges in the
/// Badges total and writes the importers' bank rows and Collections counts, which no importer
/// can write beside another's. Until 2026-11-10 a family of imported recipes alone started
/// without it, so the Badges total left out the importers' badges whenever no shared table
/// clashed, and a clash cost a pass to discover.
fn initial_decisions(plan: &mut Plan, install_root: &Path, one_mod: bool) -> Decisions {
    let mut decisions = if one_mod {
        Decisions {
            coupled: plan
                .imports
                .keys()
                .map(|importer| (*importer, vec![ONE_MOD_REASON.to_owned()]))
                .collect(),
            ..Decisions::default()
        }
    } else {
        plan.seed = seed::path(install_root);
        plan.seed
            .as_deref()
            .map(|path| seed::Seed::load(path, plan))
            .unwrap_or_default()
    };
    decisions.host |= plan.importers_have_content();
    decisions
}

impl Plan {
    /// Whether an importer's mod holds a recipe or map, which has the main mod build to host its
    /// rows.
    fn importers_have_content(&self) -> bool {
        self.recipe_owners
            .iter()
            .chain(&self.map_owners)
            .any(Option::is_some)
    }
}

/// Puts the planned session in the slot, so everything before the first pass reads it, and
/// returns the family whose passes start from it.
fn install(current: Session) -> Family {
    let family = Family::of(&current);
    *session() = Some(current);
    family
}

fn validate_reserved_ids(reserved: &BTreeSet<u16>) -> Result<(), String> {
    let window = crate::package_profile::window();
    let host = crate::package_profile::host().id;
    if reserved
        .iter()
        .any(|id| !(window.base..=window.top).contains(id) || *id == host)
    {
        return Err("An installed importer ID is outside the free package window. Resolve the conflicting installed content before rebuilding".into());
    }
    Ok(())
}

fn verify_installed_artifacts(
    directory: &Path,
    member: &Member,
    reused_allocation: Option<&[u16]>,
    in_main: bool,
) -> Result<(), String> {
    // The files this importer would reuse: its whole folder, or its resources among the
    // main mod's packages.
    let reused = |artifact: &&mod_content::Artifact| {
        !in_main
            || package_id(&artifact.path).is_some_and(|id| {
                artifact.path.starts_with("packages/")
                    && reused_allocation.is_some_and(|allocation| allocation.contains(&id))
            })
    };
    for artifact in member.receipt.artifacts.iter().filter(reused) {
        let digest = crate::artifact::digest_file(&directory.join(&artifact.path))
            .map_err(|e| e.to_string())?;
        if digest.byte_length != artifact.byte_length
            || !digest.sha256.eq_ignore_ascii_case(&artifact.sha256)
        {
            return Err(format!(
                "{} changed outside Parhelion. Its files were preserved",
                member.manifest.name
            ));
        }
    }
    Ok(())
}

/// The installed member in `directory`, with its manifest and receipt checked.
fn installed_member(directory: &Path) -> Result<(PathBuf, Member), String> {
    let manifest: Manifest = mod_content::read_json(&directory.join(mod_content::MANIFEST))?;
    let receipt: Receipt = mod_content::read_json(&directory.join(mod_content::RECEIPT))?;
    manifest.validate()?;
    receipt.validate()?;
    Ok((
        directory.to_path_buf(),
        Member {
            manifest,
            receipt,
            kept: false,
        },
    ))
}

fn dependencies_match(target: &Path, dependencies: &[ArtifactMetadata]) -> Result<bool, String> {
    let mut paths = Vec::with_capacity(dependencies.len());
    for expected in dependencies {
        if Path::new(&expected.file_name).components().count() != 1 {
            return Err("Invalid native dependency path in importer receipt".into());
        }
        let path = target.join(&expected.file_name);
        mod_content::checked_path(target, &path)?;
        paths.push(path);
    }
    // Dependencies hash on their own threads and compare in order. Past the first that differs,
    // the rest don't matter.
    let borrowed = paths.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    let mut matched = true;
    crate::artifact::digest_files(&borrowed, |index, actual| {
        let actual = match actual {
            Ok(actual) => actual,
            Err(_) if !matched => return Ok(()),
            Err(e) => {
                return Err(format!(
                    "Could not verify native importer dependency {}: {e}",
                    paths[index].display()
                ));
            }
        };
        let expected = &dependencies[index];
        matched &= actual.byte_length == expected.byte_length && actual.sha256 == expected.sha256;
        Ok(())
    })?;
    Ok(matched)
}

/// Records each built importer's native dependencies in the committed pass, and checks that no
/// importer's native inputs changed during the build. Every template's stock package generations
/// participate in the importer identity. This also covers cloned native loading companions and
/// source material envelopes, which are absent from the investment-only source-artifact list.
pub(crate) fn finish_sources(source: &Path) -> Result<(), String> {
    let mut guard = session();
    let Some(current) = guard.as_mut() else {
        return Ok(());
    };
    let Session { plan, pass, .. } = current;
    let mut digests = BTreeMap::new();
    for (owner, planned) in &plan.imports {
        let Some(taken) = pass.imports.get_mut(owner) else {
            continue;
        };
        if !dependencies_match(source, &planned.source_inputs)? {
            return Err("An importer's native inputs changed during the build".into());
        }
        if planned.kept {
            if !dependencies_match(source, &taken.state.dependencies)? {
                return Err("A kept importer's native templates changed during the build".into());
            }
            continue;
        }
        let ids = taken
            .state
            .packages
            .values()
            .flat_map(|p| &p.entries)
            .map(|entry| TagHash(entry.template).pkg_id())
            .collect::<BTreeSet<_>>();
        taken.state.dependencies = source_dependencies(source, &ids, &mut digests)?;
    }
    Ok(())
}

fn source_dependencies(
    source: &Path,
    ids: &BTreeSet<u16>,
    digests: &mut BTreeMap<String, ArtifactMetadata>,
) -> Result<Vec<ArtifactMetadata>, String> {
    let mut names = Vec::new();
    let mut unhashed = Vec::new();
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.extension().is_some_and(|extension| extension == "pkg")
            || !package_id(path.to_str().ok_or("Native dependency path is not UTF-8")?)
                .is_some_and(|id| ids.contains(&id))
        {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Native dependency name is not UTF-8")?;
        if !digests.contains_key(&name) {
            unhashed.push((name.clone(), path));
        }
        names.push(name);
    }
    // Packages a template names can be large, so they hash on their own threads.
    let paths = unhashed
        .iter()
        .map(|(_, path)| path.as_path())
        .collect::<Vec<_>>();
    crate::artifact::digest_files(&paths, |index, digest| {
        let name = &unhashed[index].0;
        let digest = digest.map_err(|e| e.to_string())?;
        digests.insert(
            name.clone(),
            ArtifactMetadata {
                file_name: name.clone(),
                byte_length: digest.byte_length,
                sha256: digest.sha256,
            },
        );
        Ok(())
    })?;
    let mut dependencies = names
        .iter()
        .map(|name| digests[name].clone())
        .collect::<Vec<_>>();
    dependencies.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(dependencies)
}

#[cfg(feature = "d2-model-importer")]
fn map_inputs(
    map: &crate::map::MapRecipe,
    source: &crate::map::MapSource,
) -> Result<serde_json::Value, String> {
    let mut paths = parhelion_import::halo_reach::level::source_files(&source.maps, &source.map)
        .map_err(|e| format!("Map {} inputs: {e:#}", map.name))?;
    paths.extend(source.variant.iter().cloned());
    paths.extend(source.mission_bindings.iter().cloned());
    let decoder = source.decoder();
    if let Some(decoder) = &decoder {
        paths.push(decoder.clone());
        paths.push(source.maps.join("../fmod/pc/sfx.fsb"));
        {
            if source.audio_banks.is_empty() {
                paths.push(source.maps.join("../fmod/pc/english.fsb"));
            } else {
                paths.extend(source.audio_banks.iter().cloned());
            }
        }
    }
    let info = paths
        .iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "fsb"))
        .map(|path| path.with_extension("fsb.info"))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    paths.extend(info);
    paths.sort();
    paths.dedup();
    // The map caches and sound banks run to gigabytes, so they hash on their own threads.
    let mut files = BTreeMap::new();
    let borrowed = paths.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    crate::artifact::digest_files(&borrowed, |index, digest| {
        let path = &paths[index];
        let digest =
            digest.map_err(|e| format!("Could not hash map input {}: {e}", path.display()))?;
        files.insert(path.clone(), digest);
        Ok(())
    })?;
    Ok(serde_json::json!({
        "map": map.name, "donor": map.donor, "source": source, "decoder": decoder, "files": files,
        "reach_revision": parhelion_import::halo_reach::REVISION,
        // A campaign's areas share one conversion, so kept geometry from per-area builds differs.
        "areas": "shared",
    }))
}

/// Reuse the build's checked file identities across its areas, whose zone sets and offsets differ.
#[cfg(feature = "d2-model-importer")]
pub(crate) fn map_source_identity(source: &crate::map::MapSource) -> Option<serde_json::Value> {
    let guard = session();
    guard.as_ref()?.plan.maps.iter().find_map(|(map, input)| {
        let original = map.source.as_ref()?;
        (original.maps == source.maps
            && original.map == source.map
            && original.variant == source.variant
            && (source.mission_bindings.is_none()
                || original.mission_bindings == source.mission_bindings)
            && original.audio_decoder == source.audio_decoder
            && original.audio_banks == source.audio_banks)
            .then(|| input["files"].clone())
    })
}

pub(crate) fn verify_inputs() -> Result<(), String> {
    #[cfg(feature = "d2-model-importer")]
    {
        let maps = session()
            .as_ref()
            .map(|s| s.plan.maps.clone())
            .unwrap_or_default();
        for (map, input) in maps {
            if map_inputs(&map, map.source.as_ref().unwrap())? != input {
                return Err(format!("Map {} inputs changed during the build", map.name));
            }
        }
    }
    Ok(())
}

#[cfg(feature = "d2-model-importer")]
pub(crate) fn map_state(name: &str) -> Result<Option<serde_json::Value>, String> {
    let guard = session();
    let Some((_, taken)) = guard
        .as_ref()
        .and_then(|session| session.import(Importer::HaloReach))
        .filter(|(planned, _)| planned.kept)
    else {
        return Ok(None);
    };
    taken
        .state
        .maps
        .get(name)
        .map(|map| {
            let text = map
                .text()
                .ok_or_else(|| format!("Map {name}'s links changed during the build"))?;
            serde_json::from_str(&text).map_err(|e| e.to_string())
        })
        .transpose()
}

/// Keeps the links of map `name`'s conversion `value`, which a later build reads to link the
/// map's world against its installed packages. With `stored`, the `links.json` of a published
/// conversion, they are written there and the receipt names the file, and otherwise the receipt
/// holds them.
#[cfg(feature = "d2-model-importer")]
pub(crate) fn remember_map(
    name: &str,
    mut value: serde_json::Value,
    stored: Option<&Path>,
) -> Result<(), String> {
    let wanted = session()
        .as_ref()
        .and_then(|s| s.plan.imports.get(&Importer::HaloReach))
        .is_some_and(|planned| !planned.kept);
    if !wanted {
        return Ok(());
    }
    // Reused resources come from installed packages. Their ordered symbols, typed fixups and
    // placement metadata rebuild the native world chain. Conversion reports and
    // source audio bytes would duplicate the cache and grow receipts with every map.
    if let Some(nodes) = value["nodes"].as_array_mut() {
        for node in nodes {
            *node = serde_json::json!({
                "symbol": node["symbol"], "patches": node["patches"],
                "reference": node["reference"], "texture_pair": node["texture_pair"],
            });
        }
    }
    value["report"] = serde_json::Value::Null;
    let text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    // The file is written without holding the build's session, which allocation waits on.
    let links = match stored {
        Some(file) => {
            // The same conversion always has the same links, so a file already there is only
            // replaced when it differs.
            let sha256 = crate::artifact::digest_bytes(text.as_bytes()).sha256;
            if !fs::read(file)
                .is_ok_and(|bytes| crate::artifact::digest_bytes(&bytes).sha256 == sha256)
            {
                sundial::package_authoring::replace_authoring_file(file, text.as_bytes())
                    .map_err(|error| format!("Could not write {}: {error}", file.display()))?;
            }
            MapLinks::Stored {
                file: file.to_owned(),
                sha256,
            }
        }
        None => MapLinks::Text(text),
    };
    if let Some(taken) = session()
        .as_mut()
        .and_then(|s| s.pass.imports.get_mut(&Importer::HaloReach))
    {
        taken.conversions += 1;
        taken.state.maps.insert(name.into(), links);
    }
    Ok(())
}

fn package_id(name: &str) -> Option<u16> {
    let parsed = tiger_pkg::manager::PackagePath::parse(name)?;
    u16::from_str_radix(&parsed.id, 16).ok()
}

pub(crate) fn allocate(owner: Importer, index: usize) -> Result<u16, String> {
    let mut guard = session();
    let current = guard
        .as_mut()
        .ok_or("Importer allocation has no build session")?;
    let Pass {
        imports,
        reserved,
        native,
        ..
    } = &mut current.pass;
    let taken = imports
        .get_mut(&owner)
        .ok_or("Importer was not declared before allocation")?;
    if let Some(&id) = taken.allocation.get(index) {
        return Ok(id);
    }
    let window = crate::package_profile::window();
    let ids = (window.base..=window.top).rev();
    let id = ids
        .into_iter()
        .find(|id| !reserved.contains(id) && !native.contains(id))
        .ok_or("No free package ID remains for imported content")?;
    taken.allocation.push(id);
    reserved.insert(id);
    Ok(id)
}

/// The package that holds page `index` of the build's records, or of its private runtime copies
/// when `private`. Each page takes the highest id of the window that no package of the build or
/// an importer holds, as importer packages do, and asset and map packages count past it. Every
/// package a build adds then sits at the top of the window, clear of the ids Sunrise Studio 007dd7a6
/// gives a mod's new packages from `0x0AA0` up without reading other mods. Studio 3a0e6f7 probes
/// from a hash of the mod id, so it can land here too. Record pages share the
/// importer's reservations, and the first asset ID stays fixed even when records spill before
/// the badge package has been constructed.
pub(crate) fn record_page(private: bool, index: usize) -> Result<u16, String> {
    let first_asset = crate::package_profile::window().first();
    let mut guard = session();
    let current = guard.get_or_insert_with(Session::default);
    if let Some(owner) = crate::package_profile::member() {
        let kind = if private {
            OwnKind::Private
        } else {
            OwnKind::Records
        };
        return own_id(current, owner, kind, index);
    }
    let Pass {
        reserved,
        native,
        private: private_pages,
        records,
        ..
    } = &mut current.pass;
    native.insert(first_asset);
    let window = crate::package_profile::window();
    let host = crate::package_profile::host().id;
    let pages = if private { private_pages } else { records };
    while pages.len() <= index {
        let id = (window.base..=window.top)
            .rev()
            .find(|id| *id != host && !reserved.contains(id) && !native.contains(id))
            .ok_or("No free package ID remains for authored records")?;
        pages.push(id);
        reserved.insert(id);
    }
    Ok(pages[index])
}

pub(crate) fn use_id(owner: Importer, id: u16) {
    if let Some(taken) = session()
        .as_mut()
        .and_then(|s| s.pass.imports.get_mut(&owner))
    {
        taken.used.insert(id);
    }
}

pub(crate) fn map_packages(name: &str, count: usize) -> Result<Vec<u16>, String> {
    let owner = Importer::HaloReach;
    let kept = {
        let guard = session();
        let (planned, taken) = guard
            .as_ref()
            .and_then(|s| s.import(owner))
            .ok_or("Map allocation has no importer session")?;
        if planned.kept {
            Some(
                taken
                    .state
                    .map_packages
                    .get(name)
                    .cloned()
                    .filter(|ids| ids.len() == count)
                    .ok_or("Kept map allocation differs from its resource count")?,
            )
        } else {
            None
        }
    };
    if let Some(ids) = kept {
        for &id in &ids {
            use_id(owner, id);
        }
        return Ok(ids);
    }
    let start = session()
        .as_ref()
        .and_then(|s| s.pass.imports.get(&owner))
        .ok_or("Importer allocation has no build session")?
        .used
        .len();
    let mut ids = Vec::new();
    for offset in 0..count {
        let id = allocate(owner, start + offset)?;
        use_id(owner, id);
        ids.push(id);
    }
    session()
        .as_mut()
        .unwrap()
        .pass
        .imports
        .get_mut(&owner)
        .unwrap()
        .state
        .map_packages
        .insert(name.into(), ids.clone());
    Ok(ids)
}

pub(crate) fn record_package(
    id: u16,
    name: &str,
    tags: &[NewTagSpec],
    references: &[NewTagReferenceOverride],
) {
    let mut guard = session();
    let Some(current) = guard.as_mut() else {
        return;
    };
    let Some((owner, taken)) = current
        .pass
        .imports
        .iter_mut()
        .find(|(_, taken)| taken.used.contains(&id))
    else {
        return;
    };
    if current
        .plan
        .imports
        .get(owner)
        .is_none_or(|planned| planned.kept)
    {
        return;
    }
    taken.state.packages.insert(
        id,
        PackageState {
            name: name.into(),
            entries: tags
                .iter()
                .map(|tag| Entry {
                    template: tag.template_tag.0,
                    storage: tag.storage,
                })
                .collect(),
            references: references.iter().map(Reference::from).collect(),
        },
    );
}

pub(crate) fn remember_group(owner: Importer, key: String, symbols: &BTreeMap<String, TagHash>) {
    if let Some(taken) = session()
        .as_mut()
        .and_then(|s| s.pass.imports.get_mut(&owner))
    {
        taken.conversions += 1;
        taken.state.groups.insert(
            key,
            symbols
                .iter()
                .map(|(name, tag)| (name.clone(), tag.0))
                .collect(),
        );
    }
}

pub(crate) fn group(owner: Importer, key: &str) -> Option<BTreeMap<String, TagHash>> {
    let guard = session();
    let (planned, taken) = guard.as_ref()?.import(owner)?;
    if !planned.kept {
        return None;
    }
    Some(
        taken
            .state
            .groups
            .get(key)?
            .iter()
            .map(|(name, tag)| (name.clone(), TagHash(*tag)))
            .collect(),
    )
}

/// Rehydrate linked entries for loading-index validation and main-mod references. The output
/// package job reads the verified installed bytes, so these payloads never pass through a writer.
pub(crate) fn restore(id: u16) -> Result<crate::asset_packages::AssetPackage, String> {
    let (path, state) = {
        let guard = session();
        let (_, planned, taken) = guard
            .as_ref()
            .and_then(|s| {
                s.imports()
                    .find(|(_, planned, taken)| planned.kept && taken.allocation.contains(&id))
            })
            .ok_or("No kept importer owns the requested package")?;
        let state = taken
            .state
            .packages
            .get(&id)
            .ok_or("Kept package metadata is missing")?
            .clone();
        let path = planned
            .installed
            .as_ref()
            .unwrap()
            .0
            .join("packages")
            .join(&state.name);
        (path, state)
    };
    let package = PackageD2PreBL::open(path.to_str().ok_or("Package path is not UTF-8")?)
        .map_err(|e| e.to_string())?;
    if package.entries().len() != state.entries.len() {
        return Err("Kept package entry count changed".into());
    }
    let tags = state
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            package
                .read_entry(index)
                .map(|payload| NewTagSpec {
                    template_tag: TagHash(entry.template),
                    payload,
                    storage: entry.storage,
                })
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(crate::asset_packages::AssetPackage {
        id,
        tags,
        references: state.references.iter().map(Reference::native).collect(),
    })
}

/// The installed file a kept importer's resource package `name` reuses. Only resources are kept:
/// an importer that builds on its own also holds stock patches, records and asset packages,
/// which every build writes again, and its patches share their names with other members'.
pub(crate) fn kept_path(name: &str) -> Option<PathBuf> {
    let guard = session();
    guard.as_ref()?.imports().find_map(|(_, planned, taken)| {
        if !planned.kept {
            return None;
        }
        let (directory, member) = planned.installed.as_ref()?;
        // A resource's merge data sits beside it under the package's own name.
        let package = name
            .strip_suffix(".merge")
            .map_or_else(|| name.to_owned(), |stem| format!("{stem}.pkg"));
        let resource = package_id(&package).is_some_and(|id| taken.allocation.contains(&id));
        let artifact = member
            .receipt
            .artifacts
            .iter()
            .find(|a| resource && a.path.strip_prefix("packages/") == Some(name))?;
        Some(directory.join(&artifact.path))
    })
}

pub(crate) fn conversions() -> BTreeMap<String, usize> {
    session()
        .as_ref()
        .map(|s| {
            s.pass
                .imports
                .iter()
                .map(|(origin, taken)| (origin.source().into(), taken.conversions))
                .collect()
        })
        .unwrap_or_default()
}

/// The staged files of one member's build, as `ArtifactMetadata::file_name` names them: the bare
/// name for the main mod's files, `<member id>/<name>` for another member's.
pub(crate) struct StagedPart {
    pub(crate) owner: Option<Importer>,
    pub(crate) files: Vec<ArtifactMetadata>,
}

/// The installed path a staged file takes in its member's folder.
fn receipt_path(file_name: &str) -> String {
    let name = file_name.rsplit('/').next().unwrap_or(file_name);
    if let Some(path) = mod_content::mission_script::path(name) {
        return path;
    }
    if name == mod_content::map_diagnostics::FILE_NAME {
        return name.to_owned();
    }
    let package_name = name
        .strip_suffix(".merge")
        .map(|stem| format!("{stem}.pkg"))
        .unwrap_or_else(|| name.to_owned());
    let profile = crate::package_profile::authored_package_for_file_name(&package_name);
    let folder = if profile.is_some_and(|profile| profile.stock_overlay) {
        "patches"
    } else {
        "packages"
    };
    format!("{folder}/{name}")
}

/// The family a build stages, from the committed pass. Each importer whose content built on its
/// own holds every file of its build. An importer whose items built with the main mod's has no
/// folder: the main mod holds its resources too, so removing any other folder leaves the main mod
/// whole.
pub(crate) fn members(parts: &[StagedPart]) -> Result<Vec<Member>, String> {
    let guard = session();
    let Some(current) = guard.as_ref() else {
        return Ok(Vec::new());
    };
    let independent = parts.iter().any(|part| part.owner.is_some());
    let coupled = current
        .pass
        .imports
        .iter()
        .filter(|(origin, taken)| {
            !taken.used.is_empty() && !parts.iter().any(|part| part.owner == Some(**origin))
        })
        .map(|(origin, _)| *origin)
        .collect::<Vec<_>>();
    if !independent && coupled.is_empty() {
        return Ok(Vec::new());
    }
    let receipt = |mod_id: String, content: Option<ImportContent>| Receipt {
        schema: 1,
        mod_id,
        artifacts: vec![],
        content,
        coupled: BTreeMap::new(),
    };
    let artifact = |file: &ArtifactMetadata| mod_content::Artifact {
        path: receipt_path(&file.file_name),
        byte_length: file.byte_length,
        sha256: file.sha256.clone(),
    };
    let content =
        |planned: &Planned, taken: &Taken, allocation: Vec<u16>| -> Result<ImportContent, String> {
            Ok(ImportContent {
                fingerprint: fingerprint(&planned.inputs, &allocation, &taken.state)?,
                inputs: planned.inputs.clone(),
                allocation,
                state: serde_json::to_value(&taken.state).map_err(|e| e.to_string())?,
            })
        };
    let mut members = Vec::new();
    for part in parts {
        let Some(origin) = part.owner else {
            continue;
        };
        let (planned, taken) = current
            .import(origin)
            .ok_or("An importer's build has no session")?;
        let mut member = Member {
            manifest: Manifest::for_importer(origin),
            receipt: receipt(origin.id(), None),
            kept: false,
        };
        member.receipt.artifacts = part.files.iter().map(artifact).collect();
        // Resource packages keep their places in the allocation, so a later build reuses them by
        // index. The member's records and map packages follow. A resource the main mod's build
        // allocated for this importer, for a native item's imported attachment, stays the main
        // mod's.
        let owned = member
            .receipt
            .artifacts
            .iter()
            .filter(|path| path.path.starts_with("packages/") && path.path.ends_with(".pkg"))
            .filter_map(|path| package_id(&path.path))
            .collect::<Vec<_>>();
        let mut allocation = taken
            .allocation
            .iter()
            .filter(|id| owned.contains(id))
            .copied()
            .collect::<Vec<_>>();
        for id in owned {
            if !allocation.contains(&id) {
                allocation.push(id);
            }
        }
        member.receipt.content = Some(content(planned, taken, allocation)?);
        // The main mod writes this member's rows of a shared table, or this member builds on the
        // main mod's, so it loads only beside the main mod.
        if !needed_from_main(current, origin).is_empty() {
            member.manifest.require(&Manifest::default());
        }
        members.push(member);
    }
    // A coupled importer's resources are the main mod's packages, so the main mod needs no other
    // member, and its receipt keeps the importer's content for the next build to reuse.
    if let Some(part) = parts.iter().find(|part| part.owner.is_none()) {
        let mut main = Member {
            manifest: Manifest::default(),
            receipt: receipt(mod_content::ID.into(), None),
            kept: false,
        };
        main.receipt.artifacts = part.files.iter().map(artifact).collect();
        let held = main
            .receipt
            .artifacts
            .iter()
            .filter(|path| path.path.starts_with("packages/") && path.path.ends_with(".pkg"))
            .filter_map(|path| package_id(&path.path))
            .collect::<BTreeSet<_>>();
        for origin in &coupled {
            let (planned, taken) = current
                .import(*origin)
                .ok_or("An importer's build has no session")?;
            let allocation = taken
                .allocation
                .iter()
                .filter(|id| taken.used.contains(id) && held.contains(id))
                .copied()
                .collect::<Vec<_>>();
            if !allocation.is_empty() {
                main.receipt.coupled.insert(
                    origin.source().to_owned(),
                    content(planned, taken, allocation)?,
                );
            }
        }
        members.insert(0, main);
    }
    for member in &mut members {
        member.receipt.artifacts.sort_by(|a, b| a.path.cmp(&b.path));
        member.receipt.validate()?;
    }
    Ok(members)
}
