use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use sundial::package_authoring::{
    mod_content::Importer, path_is_within, resolve_path_for_comparison,
};

use crate::artifact::{ArtifactMetadata, digest_file, has_pkg_extension};
use crate::item::{compile_with_progress, validate_catalog_with_progress};
use crate::manifest::{
    MANIFEST_FILE_NAME, MANIFEST_SCHEMA, ManifestDocument, ManifestProject, SelectionFingerprint,
    canonical_map, canonical_recipe, selection_fingerprint,
};
#[cfg(test)]
use crate::package_profile::{
    CANONICAL_ARTIFACT_FILE_NAMES, CANONICAL_PACKAGE_IDS, PARHELION_ASSET_PACKAGE_ID,
};
use crate::package_profile::{
    CANONICAL_PACKAGES, PACKAGE_HEADER_PREFIX_SIZE, PackageHeaderPrefix, SHADOWKEEP_HEADER_VERSION,
    authored_package, authored_package_for_file_name, authored_packages_for_file_names,
    canonical_package,
};
use crate::recipe::WeaponRecipe;
use crate::{NewWeaponProjectBundle, SUNDIAL_BUILD_SIGNATURE, WeaponProjectSpec};

mod package_views;
pub(crate) mod staging_retention;

/// A batch selected from Parhelion's recipe library.
///
/// The complete recipe set compiled into one package generation, including required shaders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchBuildRequest {
    pub package_directory: PathBuf,
    pub staging_root: PathBuf,
    pub ignore_installed_authored_overlays: bool,
    pub recipes: Vec<WeaponRecipe>,
    /// Crucible maps the build adds as new activities.
    pub maps: Vec<crate::map::MapRecipe>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchBuildSnapshot {
    pub request: BatchBuildRequest,
    /// Whether every item builds in the Parhelion mod. Otherwise, on Sunrise's mod loader, each
    /// importer's items build in a mod of their own.
    pub one_mod: bool,
    /// Whether imported items the Parhelion mod holds take their importer's badge there, rather
    /// than joining Project Sunrise. An importer's own mod always holds its badge.
    pub source_badges: bool,
}

impl BatchBuildSnapshot {
    /// The selection in build order, each recipe checked. Its fingerprint is taken when the build
    /// stages the recipes, from the encodings it writes, since encoding an imported recipe packs
    /// its whole model.
    pub fn new(mut request: BatchBuildRequest) -> Result<Self, String> {
        if request.recipes.is_empty() && request.maps.is_empty() {
            return Err("No recipes or maps are included in the package set".to_owned());
        }
        request.recipes.sort_by_cached_key(|recipe| {
            (
                recipe.namespace.clone(),
                recipe.identity.item_hash.as_str().to_owned(),
            )
        });
        request
            .maps
            .sort_by(|left, right| left.name.cmp(&right.name));
        for recipe in &request.recipes {
            recipe.validate().map_err(|error| {
                format!(
                    "Recipe: {:?} ({})\nItem: {}\nCould not validate recipe: {error}",
                    recipe.name,
                    recipe.namespace,
                    recipe.identity.item_hash.as_str()
                )
            })?;
        }
        Ok(Self {
            request,
            one_mod: false,
            source_badges: true,
        })
    }

    /// The selection built with every item in the Parhelion mod when `one_mod` is set.
    pub fn in_one_mod(mut self, one_mod: bool) -> Self {
        self.one_mod = one_mod;
        self
    }

    /// The selection built with imported items in the Parhelion mod under their importers'
    /// badges when `source_badges` is set, and under Project Sunrise otherwise.
    pub fn with_source_badges(mut self, source_badges: bool) -> Self {
        self.source_badges = source_badges;
        self
    }

    /// The fingerprint a build of this selection records, which encodes every recipe.
    pub fn fingerprint(&self) -> Result<String, String> {
        selection_fingerprint(&self.request.recipes, &self.request.maps)
    }
}

#[derive(Clone, Debug)]
pub struct BuildReport {
    pub mods: Vec<crate::ModReport>,
    pub import_conversions: BTreeMap<String, usize>,
    pub weapons: Vec<WeaponBuildReport>,
    pub run_directory: PathBuf,
    pub manifest_path: PathBuf,
    pub artifacts: Vec<ArtifactMetadata>,
    pub selection_fingerprint: String,
    pub staged_recipe_paths: Vec<PathBuf>,
    /// Recipes the build left out because each stopped it on its own, in the order it met them.
    pub skipped: Vec<SkippedRecipe>,
}

/// A recipe a build left out, and the failure that it alone caused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkippedRecipe {
    pub namespace: String,
    pub name: String,
    pub reason: String,
}

impl BuildReport {
    /// The report of a run kept from an earlier install, read from its manifest for the
    /// installation review. It names the run's items and files, without the compile details only
    /// a build in this session has.
    pub fn kept(run_directory: &Path) -> Result<Self, String> {
        let manifest_path = run_directory.join(MANIFEST_FILE_NAME);
        let file = File::open(&manifest_path)
            .map_err(|error| format!("Could not open {}: {error}", manifest_path.display()))?;
        let manifest: ManifestDocument =
            sundial::package_authoring::read_json(file).map_err(|error| error.to_string())?;
        manifest.validate()?;
        Ok(Self {
            mods: Vec::new(),
            import_conversions: BTreeMap::new(),
            weapons: manifest
                .project
                .weapons
                .iter()
                .map(|weapon| WeaponBuildReport {
                    name: weapon.name.clone(),
                    kind: weapon.kind,
                    namespace: weapon.namespace.clone(),
                    item_hash: weapon.item.hash.get(),
                    item_definition_hash: weapon.item.definition_tag.get(),
                    item_string_hash: weapon.item.string_tag.get(),
                    icon_definition_hash: 0,
                    item_index: weapon.item.index,
                    collection: None,
                    custom_plugs: Vec::new(),
                    details: None,
                    subclass: None,
                    recipe_fingerprint: String::new(),
                })
                .collect(),
            run_directory: run_directory.to_owned(),
            manifest_path,
            artifacts: manifest.artifacts,
            selection_fingerprint: manifest.selection_fingerprint,
            staged_recipe_paths: Vec::new(),
            skipped: Vec::new(),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildPhase {
    ReadingRecipes,
    InspectingSource,
    LoadingCatalog,
    CheckingRecipes,
    PreparingSource,
    HashingSource,
    CompilingProject,
    BuildingPayloads,
    RecheckingSource,
    WritingPackages,
    ValidatingPackages,
    StagingRecipes,
    WritingManifest,
    Complete,
}

impl BuildPhase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ReadingRecipes => "Loading Recipes",
            Self::InspectingSource => "Checking Build Inputs",
            Self::LoadingCatalog => "Loading Item Catalog",
            Self::CheckingRecipes => "Checking Donor Compatibility",
            Self::PreparingSource => "Preparing Source Packages",
            Self::HashingSource => "Checking Game Files",
            Self::CompilingProject => "Compiling Items",
            Self::BuildingPayloads => "Building Packages",
            Self::RecheckingSource => "Rechecking Source Packages",
            Self::WritingPackages => "Writing Package Files",
            Self::ValidatingPackages => "Validating Packages",
            Self::StagingRecipes => "Saving Recipe Snapshots",
            Self::WritingManifest => "Writing Build Manifest",
            Self::Complete => "Build Validated",
        }
    }

    /// What this phase counts. Recipe loading and input checks have different totals.
    pub const fn unit(self, total: usize) -> &'static str {
        let (one, many) = match self {
            Self::ReadingRecipes | Self::CheckingRecipes | Self::StagingRecipes => {
                ("recipe", "recipes")
            }
            Self::InspectingSource => ("input check", "input checks"),
            Self::LoadingCatalog => ("catalog step", "catalog steps"),
            Self::CompilingProject => ("build step", "build steps"),
            Self::PreparingSource
            | Self::HashingSource
            | Self::BuildingPayloads
            | Self::RecheckingSource => ("package", "packages"),
            Self::WritingPackages | Self::ValidatingPackages | Self::WritingManifest => {
                ("file", "files")
            }
            Self::Complete => ("build", "builds"),
        };
        if total == 1 { one } else { many }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildActivity {
    Package {
        id: u16,
        step: u32,
        status: OperationStatus,
        duration: Duration,
    },
    Diagnostic(String),
    /// Optional cache diagnostics, stage timings and generated entry ranges.
    RuntimeDetail(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationStatus {
    Started,
    Finished,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildProgress {
    pub phase: BuildPhase,
    pub current_artifact: Option<String>,
    pub completed: usize,
    pub total: usize,
    /// Captured where the operation happens, before coordinator and UI queues.
    pub timestamp: Instant,
    pub activity: Option<BuildActivity>,
    /// The mod a family build is compiling. A build of one mod has none.
    pub member: Option<MemberProgress>,
}

/// One mod of a family build and the pass compiling it. Every mod builds again in a later pass
/// when the review of a pass moves shared rows between them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemberProgress {
    pub name: &'static str,
    pub pass: usize,
    /// The pass's work before this mod's, this mod's and the whole pass's. Each mod counts its
    /// recipes and maps, and one more for its packages, so a pass's mods fill one bar in turn.
    pub before: usize,
    pub weight: usize,
    pub total: usize,
}

impl MemberProgress {
    /// How far the pass is when this mod is `within` of the way through its own build.
    pub fn pass_fraction(self, within: f32) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        ((self.before as f32 + self.weight as f32 * within.clamp(0.0, 1.0)) / self.total as f32)
            .clamp(0.0, 1.0)
    }

    /// The build log's line as the mod starts. It names the pass once a later pass builds the mod
    /// again, since the first pass can't know whether another follows.
    fn start(self, recipes: usize, maps: usize) -> String {
        let count = |count: usize, one: &str, many: &str| {
            format!("{count} {}", if count == 1 { one } else { many })
        };
        let builds = if self.pass > 1 {
            format!("Pass {} builds {}", self.pass, self.name)
        } else {
            format!("Building {}", self.name)
        };
        let content = match (recipes, maps) {
            // A main mod that only hosts its importers' shared rows.
            (0, 0) => return format!("{builds} to hold the rows its importers share."),
            (_, 0) => count(recipes, "recipe", "recipes"),
            (0, _) => count(maps, "map", "maps"),
            _ => format!(
                "{} and {}",
                count(recipes, "recipe", "recipes"),
                count(maps, "map", "maps")
            ),
        };
        format!("{builds} with {content}.")
    }
}

impl BuildProgress {
    pub(crate) fn phase(phase: BuildPhase, completed: usize, total: usize) -> Self {
        Self {
            phase,
            current_artifact: None,
            completed,
            total,
            timestamp: Instant::now(),
            activity: None,
            member: None,
        }
    }

    pub(crate) fn artifact(
        phase: BuildPhase,
        current_artifact: impl Into<String>,
        completed: usize,
        total: usize,
    ) -> Self {
        Self {
            phase,
            current_artifact: Some(current_artifact.into()),
            completed,
            total,
            timestamp: Instant::now(),
            activity: None,
            member: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponBuildReport {
    pub name: String,
    pub kind: crate::ItemKind,
    pub namespace: String,
    pub item_hash: u32,
    pub item_definition_hash: u32,
    pub item_string_hash: u32,
    pub icon_definition_hash: u32,
    pub item_index: u16,
    /// A subclass has no collectible or unlock.
    pub collection: Option<crate::NewCollectionPlan>,
    pub custom_plugs: Vec<CustomPlugBuildReport>,
    pub(crate) details: Option<crate::item::WeaponBuildDetails>,
    /// What the build wrote for a subclass.
    pub(crate) subclass: Option<crate::item::SubclassBuildDetails>,
    pub(crate) recipe_fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomPlugBuildReport {
    pub socket_index: usize,
    pub choice_index: usize,
    pub name: Option<String>,
    pub item_hash: u32,
    pub item_index: u16,
    pub definition_hash: u32,
    pub string_hash: u32,
    pub icon_definition_hash: Option<u32>,
    pub name_hash: Option<u32>,
    pub description_hash: Option<u32>,
    /// The stock shared plug sets the plug joined when offered everywhere.
    pub offered_sets: Vec<u16>,
    pub perks: Vec<PrivatePerkBuildReport>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivatePerkBuildReport {
    pub source_perk_index: usize,
    pub perk_hash: u32,
    pub runtime_key: u32,
}

fn default_data_root() -> PathBuf {
    sundial::package_authoring::parhelion_data_directory()
        .unwrap_or_else(|| env::current_dir().unwrap_or_default().join("parhelion"))
}

pub(crate) fn default_staging_root() -> PathBuf {
    default_data_root().join("staging")
}

pub(crate) fn default_backup_root() -> PathBuf {
    default_data_root().join("backups").join("packages")
}

#[cfg(test)]
fn preflight_snapshot(snapshot: &BatchBuildSnapshot) -> Result<(), String> {
    plan_snapshot(snapshot, &mut |_| {})
        .map(drop)
        .map_err(String::from)
}

/// Why a build stopped, and the recipe it stopped on when a single recipe caused it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildFailure {
    pub message: String,
    /// The namespace of the recipe the build stopped on.
    pub recipe: Option<String>,
}

impl BuildFailure {
    fn authoring(summary: &str, error: &crate::AuthoringError) -> Self {
        Self {
            message: format!("{summary}\n{error:#}"),
            recipe: error.recipe_namespace().map(str::to_owned),
        }
    }
}

impl From<String> for BuildFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            recipe: None,
        }
    }
}

impl From<BuildFailure> for String {
    fn from(failure: BuildFailure) -> Self {
        failure.message
    }
}

impl std::fmt::Display for BuildFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceInspection {
    ignored_authored_files: Vec<String>,
}

struct PlannedSnapshot {
    source_inspection: SourceInspection,
    source_artifacts: Vec<ArtifactMetadata>,
    /// One build per family member, the main mod's first. Dawn and Sunrise before 0.6 build one.
    builds: Vec<MemberBuild>,
    /// The class of each subclass base, which the manifest records for the install.
    subclass_classes: BTreeMap<u32, u8>,
    /// The build's turn with the package layout, held until its packages are staged.
    profile: crate::package_profile::Profile,
}

/// One family member's build of its own recipes and maps.
struct MemberBuild {
    /// The importer whose mod holds the build, or `None` for the main mod.
    owner: Option<sundial::package_authoring::mod_content::Importer>,
    /// The places of its recipes in the request, in order, and of its maps.
    recipes: Vec<usize>,
    maps: Vec<usize>,
    bundle: NewWeaponProjectBundle,
    /// A mod-folder build's merge data, written beside its packages.
    merge_data: Option<crate::merge_data::MergeData>,
}

/// Compiles the request. On Sunrise's mod loader each importer whose content Sunrise can merge
/// with the others' builds on its own against stock, so its mod holds its own records, patches,
/// maps and scripts and loads without the others. An importer whose build changes an entry
/// another member's also changes in a way Sunrise wouldn't merge compiles with the main mod's
/// content instead, and the family builds again.
///
/// `family` is the build's state from `mod_family::build::begin`. Each pass starts from its plan
/// and the decisions earlier passes made, compiles every member, and is reviewed. A pass whose
/// review changes nothing commits the family and returns its builds, and otherwise the review's
/// changes go into the family for the next pass.
fn compile_members(
    source: &Path,
    family: crate::mod_family::build::Family,
    (project, maps): (&WeaponProjectSpec, &[crate::map::MapRecipe]),
    host: crate::package_profile::Host,
    branding: crate::branding::Branding,
    record: &mut dyn FnMut(&str, crate::AuthoringError) -> String,
    progress: &mut dyn FnMut(BuildProgress),
) -> Result<Vec<MemberBuild>, String> {
    let mut compile = |recipes: &[usize],
                       selected: &[usize],
                       member: Option<MemberProgress>,
                       progress: &mut dyn FnMut(BuildProgress)|
     -> Result<NewWeaponProjectBundle, String> {
        let part = WeaponProjectSpec {
            weapons: recipes
                .iter()
                .map(|&index| project.weapons[index].clone())
                .collect(),
        };
        let part_maps = selected
            .iter()
            .map(|&index| maps[index].clone())
            .collect::<Vec<_>>();
        compile_with_progress(source, &part, &part_maps, branding, &mut |event| {
            progress(BuildProgress {
                phase: match event.phase {
                    crate::item::CompilePhase::Authoring => BuildPhase::CompilingProject,
                    crate::item::CompilePhase::Payloads => BuildPhase::BuildingPayloads,
                },
                current_artifact: Some(event.label.to_owned()),
                completed: event.completed,
                total: event.total,
                timestamp: event.timestamp,
                activity: event.activity,
                member,
            });
        })
        .map_err(|error| record("Project compilation failed.", error))
    };
    let all_recipes = (0..project.weapons.len()).collect::<Vec<_>>();
    let all_maps = (0..maps.len()).collect::<Vec<_>>();
    if !host.is_standalone() {
        // One mod holds everything, so it builds once.
        let bundle = compile(&all_recipes, &all_maps, None, progress)?;
        family.commit();
        return Ok(vec![MemberBuild {
            owner: None,
            recipes: all_recipes,
            maps: all_maps,
            bundle,
            merge_data: None,
        }]);
    }
    let mut family = family;
    // Each importer's Collections entries follow from its recipes and the stock tables, so they
    // are counted before the first pass, and the main mod's first build makes room for them and
    // counts them. A count that fails leaves what the install's last build recorded, and the
    // passes still settle on what the importers' builds record.
    let censuses = family
        .partitions(project.weapons.len(), maps.len())
        .into_iter()
        .filter_map(|part| Some((part.owner?, part.recipes)))
        .map(|(owner, recipes)| {
            let part = WeaponProjectSpec {
                weapons: recipes
                    .iter()
                    .map(|&index| project.weapons[index].clone())
                    .collect(),
            };
            Some((owner, crate::item::census(source, &part).ok()?))
        })
        .collect::<Option<BTreeMap<_, _>>>();
    if let Some(censuses) = censuses {
        family.expect_collections(&censuses);
    }
    // A later pass emits most packages of the pass before unchanged, and the turn keeps them.
    let passes = crate::turn_cache::Passes::begin();
    // An importer whose build reads the same family state as in the pass before keeps that build,
    // and the pass repeats what it did to the family state instead of compiling it again. A
    // second pass usually only changes the rows the main mod writes, so it rebuilds the main mod
    // and the importers that read what changed.
    let mut kept = BTreeMap::<Importer, (MemberBuild, Watched)>::new();
    let verify_reuse = std::env::var_os(VERIFY_MEMBER_REUSE).is_some();
    // Each retry hosts more of an importer's tables in the main mod, couples an importer, gives
    // the main mod the rows the importers recorded in the pass before, which the next pass
    // records unchanged, or moves the main mod's automatic Collections entries behind an
    // importer's, so the passes end.
    for pass in 1..=5 {
        passes.next();
        let parts = family.partitions(project.weapons.len(), maps.len());
        let main_builds = parts.iter().any(|part| part.owner.is_none());
        // Sunrise keeps one mod's value of the Badges total, so the main mod's build counts every
        // member's badges in it and arranges the Badges root around the importers' badges.
        family.start_pass(main_builds, || {
            parts
                .iter()
                .map(|part| {
                    let weapons = part.recipes.iter().map(|&index| &project.weapons[index]);
                    (part.owner, crate::badge::count(weapons))
                })
                .collect()
        });
        let mut builds = Vec::with_capacity(parts.len());
        let mut watched = BTreeMap::<Importer, Watched>::new();
        let weight =
            |part: &crate::mod_family::build::Partition| part.recipes.len() + part.maps.len() + 1;
        let pass_total = parts.iter().map(weight).sum::<usize>();
        let mut before = 0;
        for part in parts {
            // An importer's records take the host its installed build had, so an unchanged
            // member builds the same bytes.
            let member_host = match part.owner {
                Some(owner) => crate::package_profile::Host::standalone(
                    crate::mod_family::build::member_host(owner)?,
                ),
                None => host,
            };
            crate::package_profile::begin_partition(member_host, part.owner)?;
            let member = MemberProgress {
                name: part.owner.map_or("Parhelion", |owner| owner.name()),
                pass,
                before,
                weight: weight(&part),
                total: pass_total,
            };
            before += member.weight;
            let earlier = part
                .owner
                .and_then(|_| crate::mod_family::build::snapshot());
            let reusable = part.owner.zip(earlier.as_ref()).and_then(|(owner, now)| {
                let (build, seen) = kept.remove(&owner)?;
                let inputs = crate::mod_family::build::member_inputs(owner, now, &seen.reads)?;
                (seen.inputs == inputs && build.recipes == part.recipes && build.maps == part.maps)
                    .then_some((owner, build, seen))
            });
            if !verify_reuse && let Some((owner, build, seen)) = reusable {
                crate::mod_family::build::replay(owner, &seen.before, &seen.after);
                passes.built(part.owner);
                let mut event = BuildProgress::phase(BuildPhase::BuildingPayloads, 1, 1);
                event.member = Some(member);
                event.activity = Some(BuildActivity::Diagnostic(format!(
                    "Pass {pass} keeps {}'s build from pass {}. Nothing it reads changed.",
                    member.name,
                    pass - 1
                )));
                progress(event);
                watched.insert(owner, seen);
                builds.push(build);
                continue;
            }
            let mut event = BuildProgress::phase(BuildPhase::CompilingProject, 0, 0);
            event.member = Some(member);
            event.activity = Some(BuildActivity::Diagnostic(
                member.start(part.recipes.len(), part.maps.len()),
            ));
            progress(event);
            let bundle = compile(&part.recipes, &part.maps, Some(member), progress)?;
            passes.built(part.owner);
            if let Some((_, kept_build, _)) = &reusable
                && !same_artifacts(&kept_build.bundle, &bundle)
            {
                return Err(format!(
                    "{} built different packages from the same inputs in pass {pass}, so its build can't be kept between passes ({VERIFY_MEMBER_REUSE})",
                    member.name
                ));
            }
            // What the build read is known once it has built, and its digest covers the main
            // mod's payloads of just those entries, as the pass held them when it started.
            if let (Some(owner), Some(earlier), Some(later)) =
                (part.owner, earlier, crate::mod_family::build::snapshot())
            {
                let reads = later.reads(owner);
                if let Some(inputs) =
                    crate::mod_family::build::member_inputs(owner, &earlier, &reads)
                {
                    watched.insert(
                        owner,
                        Watched {
                            inputs,
                            reads,
                            before: earlier,
                            after: later,
                        },
                    );
                }
            }
            // Sunrise merges table rows only for mods in its mod folder.
            let files = bundle
                .artifacts
                .iter()
                .map(|artifact| (artifact.plan.output_file_name.as_str(), artifact.bytes()))
                .collect::<Vec<_>>();
            let merge_data = crate::merge_data::describe(source, &files);
            builds.push(MemberBuild {
                owner: part.owner,
                recipes: part.recipes,
                maps: part.maps,
                bundle,
                merge_data: Some(merge_data),
            });
        }
        let outcome = family.finish_pass();
        let review = review_pass(&family, &outcome, &builds, main_builds)?;
        if review.retry.is_empty() && family.settled(&outcome) {
            crate::package_profile::end_partitions(host);
            family.commit();
            return Ok(builds);
        }
        let mut notes = review.notes();
        let lists = family.fronts_changed(&outcome);
        if lists > 0 {
            notes.push(format!(
                "Parhelion makes room for the importers' entries at the start of {lists} Collections {}.",
                if lists == 1 { "list" } else { "lists" }
            ));
        }
        if pass < 5 {
            notes.push(format!("The mods build again in pass {}.", pass + 1));
        }
        let mut event = BuildProgress::phase(BuildPhase::CompilingProject, 0, 0);
        event.activity = Some(BuildActivity::Diagnostic(notes.join(" ")));
        progress(event);
        family.retry(outcome, review.retry);
        kept = builds
            .into_iter()
            .filter_map(|build| {
                let owner = build.owner?;
                Some((owner, (build, watched.remove(&owner)?)))
            })
            .collect();
    }
    Err("The family's builds kept changing entries that would not merge".into())
}

/// Set to build every importer in each pass even when it could keep its build from the pass
/// before, stopping the build when the packages differ, which would mean the inputs a kept build
/// is checked against leave something out.
const VERIFY_MEMBER_REUSE: &str = "PARHELION_VERIFY_MEMBER_REUSE";

/// What an importer's build read from the family and the family state around it in one pass:
/// the digest of its inputs, which covers the main mod's payloads of the shared entries in
/// `reads`, the ones its build followed.
struct Watched {
    inputs: [u8; 32],
    reads: BTreeSet<u32>,
    before: crate::mod_family::build::Snapshot,
    after: crate::mod_family::build::Snapshot,
}

/// Whether two builds wrote the same package files with the same bytes.
fn same_artifacts(kept: &NewWeaponProjectBundle, built: &NewWeaponProjectBundle) -> bool {
    kept.artifacts.len() == built.artifacts.len()
        && kept
            .artifacts
            .iter()
            .zip(&built.artifacts)
            .all(|(left, right)| {
                left.plan.output_file_name == right.plan.output_file_name
                    && left.bytes() == right.bytes()
            })
}

/// What the review of one pass changes before the family builds again.
struct PassReview {
    retry: crate::mod_family::build::Retry,
}

impl PassReview {
    /// The sentences the build log shows for the review's changes.
    fn notes(&self) -> Vec<String> {
        self.retry
            .host
            .then(|| "Parhelion builds to hold the rows its importers share.".to_owned())
            .into_iter()
            .chain(self.retry.hosted.iter().map(|(importer, tables)| {
                format!(
                    "Parhelion now writes {} for {}.",
                    sentence_list(tables.iter().map(|table| table.label())),
                    importer.name()
                )
            }))
            .chain(self.retry.coupled.iter().map(|(importer, reasons)| {
                format!(
                    "{} builds into Parhelion. {}",
                    importer.name(),
                    reasons.join(" ")
                )
            }))
            .collect()
    }
}

/// Reviews a pass's member builds against what `family` already decided: which tables the main
/// mod hosts and which importers build with it from the next pass on. It changes nothing itself.
fn review_pass(
    family: &crate::mod_family::build::Family,
    outcome: &crate::mod_family::build::Outcome,
    builds: &[MemberBuild],
    main_builds: bool,
) -> Result<PassReview, String> {
    use crate::mod_family::build::Shared;
    let conflicts = crate::merge_data::conflicts(
        &builds
            .iter()
            .filter_map(|build| Some((build.owner, build.merge_data.as_ref()?)))
            .collect::<Vec<_>>(),
    );
    let mut retry = crate::mod_family::build::Retry::default();
    // A table an importer's build adds rows to, which another member changes too, is written by
    // the main mod's build with every member's rows, and the importer keeps everything else. A
    // family whose recipes are all imported has no main mod build, so the main mod then builds
    // without content of its own to hold them (`Retry::host`). An entry no shared table names, or
    // merge data that can't be written, builds the whole importer with the main mod instead.
    for (importer, conflict) in conflicts {
        let produced = outcome.produced_by(Some(importer));
        let already = family.hosting(importer);
        let mut tables = BTreeSet::new();
        let mut unhosted = !conflict.whole.is_empty();
        for tag in conflict.entries.keys() {
            match produced.get(tag) {
                // An importer's activity tables are built on the main mod's, so they hold its
                // rows and Sunrise serves them from the importer while it loads.
                Some(Shared::Activities) if already.contains(&Shared::Activities) => {}
                // A table the main mod hosts still clashes when the main mod didn't build.
                Some(table) if !already.contains(table) || !main_builds => {
                    tables.insert(*table);
                }
                _ => unhosted = true,
            }
        }
        if unhosted {
            retry.coupled.insert(importer, conflict.reasons());
        } else if !tables.is_empty() {
            retry.hosted.insert(importer, tables);
            retry.host |= !main_builds;
        }
    }
    // A perk accepts an imported ability by the root its subclass's build allocated, so a perk
    // and a subclass in different members build together.
    for (importer, reasons) in outcome.split_abilities() {
        retry.hosted.remove(&importer);
        retry.coupled.entry(importer).or_default().extend(reasons);
    }
    // Every member's entries go before the stock entries of a Collections list, each member's in
    // rows of its own (`mod_family::build::front`), so no list needs one member to lead it.
    Ok(PassReview { retry })
}

/// `items` as a sentence names them: "a", "a and b" or "a, b and c".
pub(crate) fn sentence_list<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    let items = items.into_iter().collect::<Vec<_>>();
    match items.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

fn plan_snapshot(
    snapshot: &BatchBuildSnapshot,
    progress: &mut impl FnMut(BuildProgress),
) -> Result<PlannedSnapshot, BuildFailure> {
    let recipe = std::cell::Cell::new(None);
    plan_snapshot_naming(snapshot, progress, &recipe, None).map_err(|message| BuildFailure {
        message,
        recipe: recipe.take(),
    })
}

/// Plans a build, recording in `failed` the recipe an error belongs to when one recipe caused it.
/// With `skipped`, a compile that fails in recipes of its own leaves them out, records them there
/// and compiles the rest. The planned builds then hold only the recipes it kept.
fn plan_snapshot_naming(
    snapshot: &BatchBuildSnapshot,
    progress: &mut impl FnMut(BuildProgress),
    failed: &std::cell::Cell<Option<String>>,
    mut skipped: Option<&mut Vec<SkippedRecipe>>,
) -> Result<PlannedSnapshot, String> {
    // Every authored id the build places reads this window and host, so they are chosen before
    // planning. Sunrise's mod loader refuses a patch that adds entries, so there the build's
    // investment records take the window's first id and the asset packages follow it.
    let directory = &snapshot.request.package_directory;
    let window = crate::install::package_window(directory).map_err(|error| error.to_string())?;
    let (window, host) = if crate::install::uses_mod_loader(directory) {
        let Some(rest) = window.rest() else {
            return Err(
                "The free package ids leave no room for the build's investment records".into(),
            );
        };
        (
            rest,
            crate::package_profile::Host::standalone(if window.descending {
                window.top
            } else {
                window.base
            }),
        )
    } else {
        (window, crate::package_profile::Host::APPENDED)
    };
    let waiting = Instant::now();
    let profile = crate::package_profile::hold(window, host);
    if host.is_standalone() {
        let mut event = BuildProgress::phase(BuildPhase::InspectingSource, 0, 0);
        event.activity = Some(BuildActivity::Diagnostic(format!(
            "Build wait: {:.6}s",
            waiting.elapsed().as_secs_f64()
        )));
        progress(event);
    }
    // Automatic Collections placement follows whether another mod changes the presentation nodes.
    crate::package_profile::set_presentation_nodes_shared(
        crate::install::other_mods_change(
            directory,
            sundial::package_authoring::mod_content::merge::recipes::PRESENTATION_NODES,
        )
        .map_err(|error| error.to_string())?,
    );
    if host.is_standalone() {
        crate::package_profile::name_standalone_package(
            host.id,
            crate::package_profile::RECORDS_FAMILY,
        )?;
    }
    progress(BuildProgress::artifact(
        BuildPhase::InspectingSource,
        "Checking Recipes",
        0,
        2,
    ));
    let mut project = match project_spec(snapshot) {
        Ok(project) => project,
        Err(failure) => {
            failed.set(failure.recipe);
            return Err(failure.message);
        }
    };
    // Each project weapon's recipe name, kept beside the weapons as recipes are left out.
    let mut names = snapshot
        .request
        .recipes
        .iter()
        .map(|recipe| recipe.name.clone())
        .collect::<Vec<_>>();
    let record = |summary: &str, error: crate::AuthoringError| {
        let failure = BuildFailure::authoring(summary, &error);
        failed.set(failure.recipe);
        failure.message
    };
    progress(BuildProgress::artifact(
        BuildPhase::InspectingSource,
        "Inspecting Package Headers",
        1,
        2,
    ));
    let source_inspection = inspect_snapshot(snapshot)?;
    progress(BuildProgress::phase(BuildPhase::InspectingSource, 2, 2));
    let install_directory = snapshot
        .request
        .package_directory
        .parent()
        .ok_or_else(|| "The package directory has no Shadowkeep install root".to_owned())?;
    progress(BuildProgress::phase(BuildPhase::LoadingCatalog, 0, 0));
    let subclass_classes = validate_catalog_with_progress(
        install_directory,
        project.weapons.iter(),
        &mut |loading, label, completed, total| {
            // Catalog labels describe generated operations. Recipe names take the other path.
            let label = if loading {
                label.trim_end_matches('…')
            } else {
                label
            };
            progress(BuildProgress::artifact(
                if loading {
                    BuildPhase::LoadingCatalog
                } else {
                    BuildPhase::CheckingRecipes
                },
                label,
                completed,
                total,
            ));
        },
    )
    .map_err(|error| record("Project compatibility validation failed.", error))?;
    crate::item::expand_subclass_gates(install_directory, &mut project.weapons)
        .map_err(|error| record("Subclass damage-type gates could not be prepared.", error))?;
    progress(BuildProgress::phase(BuildPhase::PreparingSource, 0, 0));
    let source = PackageSource::prepare(
        &snapshot.request.package_directory,
        &source_inspection.ignored_authored_files,
    )?;
    let result = (|| {
        progress(BuildProgress::phase(BuildPhase::HashingSource, 0, 0));
        let source_artifacts =
            source_artifact_reports_with_progress(source.path(), &mut |name, completed, total| {
                progress(BuildProgress::artifact(
                    BuildPhase::HashingSource,
                    name,
                    completed,
                    total,
                ));
            })?;
        let branding = crate::branding::Branding::detect(install_directory);
        // A build that leaves failing recipes out drops each recipe a compile names and compiles
        // the members again without it. The build keeps its turn, source view, hashes and
        // catalog checks, and the runtime cache replays the items it already compiled.
        let builds = loop {
            let mut failures = Vec::new();
            // Planning: which importers' content is kept, which ids it takes again and which mod
            // holds each recipe and map.
            let compilation = crate::mod_family::build::begin(
                directory,
                source.path(),
                &project.weapons,
                &snapshot.request.maps,
                &source_artifacts,
                snapshot.one_mod,
                snapshot.source_badges,
            )
            .and_then(|family| {
                compile_members(
                    source.path(),
                    family,
                    (&project, &snapshot.request.maps),
                    host,
                    branding,
                    &mut |summary: &str, error: crate::AuthoringError| {
                        failures = error
                            .recipe_failures()
                            .into_iter()
                            .map(|(namespace, message)| {
                                (namespace, format!("{summary}\n{message}"))
                            })
                            .collect();
                        record(summary, error)
                    },
                    progress,
                )
            });
            let message = match compilation {
                Ok(builds) => break builds,
                Err(message) => message,
            };
            let Some(skipped) = skipped.as_deref_mut() else {
                return Err(message);
            };
            let mut left_out = 0;
            for (namespace, reason) in failures {
                let Some(at) = project
                    .weapons
                    .iter()
                    .position(|weapon| weapon.namespace == namespace)
                else {
                    continue;
                };
                if project.weapons.len() == 1 && snapshot.request.maps.is_empty() {
                    return Err(message);
                }
                project.weapons.remove(at);
                let name = names.remove(at);
                let mut event = BuildProgress::phase(BuildPhase::CompilingProject, 0, 0);
                event.activity = Some(BuildActivity::Diagnostic(format!(
                    "Left out {name}. {reason}"
                )));
                progress(event);
                skipped.push(SkippedRecipe {
                    namespace,
                    name,
                    reason,
                });
                left_out += 1;
            }
            if left_out == 0 {
                return Err(message);
            }
            // A later failure names its own recipe, if any.
            failed.set(None);
            // The next compile starts from this turn's empty family session and allocations.
            crate::mod_family::build::begin_native();
            crate::appended_tags::clear();
            crate::package_profile::end_partitions(host);
            let mut event = BuildProgress::phase(BuildPhase::CompilingProject, 0, 0);
            event.activity = Some(BuildActivity::Diagnostic(format!(
                "Compiling the other {} items without {}. Source checks are kept, and items the runtime cache holds are reused.",
                project.weapons.len(),
                if left_out == 1 {
                    "the left-out item".to_owned()
                } else {
                    format!("the {left_out} left-out items")
                }
            )));
            progress(event);
        };
        progress(BuildProgress::phase(
            BuildPhase::RecheckingSource,
            0,
            source_artifacts.len(),
        ));
        let source_artifacts_after =
            source_artifact_reports_with_progress(source.path(), &mut |name, completed, total| {
                progress(BuildProgress::artifact(
                    BuildPhase::RecheckingSource,
                    name,
                    completed,
                    total,
                ));
            })?;
        validate_source_artifacts_unchanged(&source_artifacts, &source_artifacts_after)?;
        crate::mod_family::build::verify_inputs()?;
        // The committed pass records its native dependencies for the receipts staging writes.
        crate::mod_family::build::finish_sources(source.path())?;
        Ok(PlannedSnapshot {
            source_inspection,
            source_artifacts,
            builds,
            subclass_classes,
            profile,
        })
    })();
    // Compilation has released its managers and returned owned package bytes. A view that
    // cannot be removed yet is reported as a build message, not a failure: the output is
    // complete and the next build prunes the view.
    source.finish(result, |warning| {
        progress(BuildProgress::artifact(
            BuildPhase::RecheckingSource,
            format!("Build kept. {warning}"),
            1,
            1,
        ));
    })
}

fn inspect_snapshot(snapshot: &BatchBuildSnapshot) -> Result<SourceInspection, String> {
    validate_paths(
        &snapshot.request.package_directory,
        &snapshot.request.staging_root,
    )?;
    inspect_source(
        &snapshot.request.package_directory,
        snapshot.request.ignore_installed_authored_overlays,
    )
}

fn inspect_source(
    package_directory: &Path,
    ignore_installed_authored_overlays: bool,
) -> Result<SourceInspection, String> {
    let mut stock_patches = BTreeMap::<u16, u16>::new();
    let mut authored = Vec::new();

    for entry in fs::read_dir(package_directory)
        .map_err(|error| format!("Could not list {}: {error}", package_directory.display()))?
    {
        let entry = entry.map_err(|error| format!("Could not read a package entry: {error}"))?;
        let path = entry.path();
        if !entry
            .file_type()
            .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?
            .is_file()
            || !has_pkg_extension(&path)
        {
            continue;
        }
        let Some(header) = read_relevant_header(&path)? else {
            continue;
        };
        if header.build_signature == SUNDIAL_BUILD_SIGNATURE {
            let name = file_name(&path)?;
            // A standalone package may carry a map's name instead of Parhelion's asset name, so
            // its own name identifies it when that name holds the header's package id.
            let profile = authored_package_for_file_name(&name)
                .filter(|profile| profile.package_id == header.package_id)
                .or_else(|| authored_package(header.package_id))
                .ok_or_else(|| {
                    format!(
                        "Authored package {} uses unrecognized package id {:04X}",
                        path.display(),
                        header.package_id
                    )
                })?;
            if header.patch_id != profile.patch_id || name != profile.file_name {
                return Err(format!(
                    "Authored package {} has identity {:04X}/{}; expected {} at patch {}",
                    path.display(),
                    header.package_id,
                    header.patch_id,
                    profile.file_name,
                    profile.patch_id
                ));
            }
            authored.push(name);
        } else if crate::package_profile::is_authored_standalone_package_id(header.package_id) {
            return Err(format!(
                "Package id {:04X} is reserved for Parhelion assets but is occupied by {}",
                header.package_id,
                path.display()
            ));
        } else {
            stock_patches
                .entry(header.package_id)
                .and_modify(|patch| *patch = (*patch).max(header.patch_id))
                .or_insert(header.patch_id);
        }
    }

    validate_stock_patches(&stock_patches)?;
    authored.sort();
    validate_authored_set(ignore_installed_authored_overlays, &authored)?;
    Ok(SourceInspection {
        ignored_authored_files: authored,
    })
}

pub fn build_and_stage_snapshot_with_progress(
    snapshot: &BatchBuildSnapshot,
    progress: impl FnMut(BuildProgress),
) -> Result<BuildReport, String> {
    build_and_stage_snapshot_reporting(snapshot, progress).map_err(String::from)
}

/// As [`build_and_stage_snapshot_with_progress`], naming the recipe a failure belongs to.
pub fn build_and_stage_snapshot_reporting(
    snapshot: &BatchBuildSnapshot,
    mut progress: impl FnMut(BuildProgress),
) -> Result<BuildReport, BuildFailure> {
    let planned = plan_snapshot(snapshot, &mut progress)?;
    stage_planned(snapshot, planned, Vec::new(), progress)
}

/// As [`build_and_stage_snapshot_reporting`], leaving out each recipe that stops the build on its
/// own instead of stopping there, and the report lists every recipe it left out with its failure.
/// A compile failure leaves the recipe out and compiles the others again within the same build. A
/// recipe that fails its checks before anything compiles is left out and the checks start over.
/// A failure no single recipe caused still stops the build, as does one that would leave nothing
/// to build.
pub fn build_and_stage_snapshot_skipping(
    snapshot: &BatchBuildSnapshot,
    mut progress: impl FnMut(BuildProgress),
) -> Result<BuildReport, BuildFailure> {
    let mut snapshot = snapshot.clone();
    let mut skipped = Vec::new();
    let planned = loop {
        let recipe = std::cell::Cell::new(None);
        let failure =
            match plan_snapshot_naming(&snapshot, &mut progress, &recipe, Some(&mut skipped)) {
                Ok(planned) => break planned,
                Err(message) => BuildFailure {
                    message,
                    recipe: recipe.take(),
                },
            };
        snapshot.request.recipes.retain(|recipe| {
            !skipped
                .iter()
                .any(|left| left.namespace == recipe.namespace)
        });
        let Some(index) = failure.recipe.as_deref().and_then(|namespace| {
            snapshot
                .request
                .recipes
                .iter()
                .position(|recipe| recipe.namespace == namespace)
        }) else {
            return Err(failure);
        };
        if snapshot.request.recipes.len() == 1 && snapshot.request.maps.is_empty() {
            return Err(failure);
        }
        let recipe = snapshot.request.recipes.remove(index);
        let mut event = BuildProgress::phase(BuildPhase::InspectingSource, 0, 0);
        event.activity = Some(BuildActivity::Diagnostic(format!(
            "Left out {}. Checking the other recipes again before compiling. {}",
            recipe.name, failure.message
        )));
        progress(event);
        skipped.push(SkippedRecipe {
            namespace: recipe.namespace,
            name: recipe.name,
            reason: failure.message,
        });
    };
    // The planned builds hold only the recipes the compile kept, in request order.
    snapshot.request.recipes.retain(|recipe| {
        !skipped
            .iter()
            .any(|left| left.namespace == recipe.namespace)
    });
    stage_planned(&snapshot, planned, skipped, progress)
}

/// Writes and verifies a planned build's packages and recipes in a staging run.
fn stage_planned(
    snapshot: &BatchBuildSnapshot,
    planned: PlannedSnapshot,
    skipped: Vec<SkippedRecipe>,
    mut progress: impl FnMut(BuildProgress),
) -> Result<BuildReport, BuildFailure> {
    let _profile = planned.profile;
    let source_inspection = planned.source_inspection;
    let source_artifacts = planned.source_artifacts;
    let builds = planned.builds;
    let subclass_classes = planned.subclass_classes;
    // Each recipe's plan, from the member build that compiled it.
    let mut placed = vec![None; snapshot.request.recipes.len()];
    for build in &builds {
        for (&index, plan) in build.recipes.iter().zip(&build.bundle.plan.weapons) {
            placed[index] = Some(plan.clone());
        }
    }
    let plans = placed
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "A recipe has no build".to_owned())?;
    fs::create_dir_all(&snapshot.request.staging_root).map_err(|error| {
        format!(
            "Could not create staging root {}: {error}",
            snapshot.request.staging_root.display()
        )
    })?;
    let slug = if snapshot.request.recipes.len() == 1 {
        snapshot.request.recipes[0].slug()
    } else {
        "parhelion-project".to_owned()
    };
    let staged_run = staging_retention::StagedRun::begin(&snapshot.request.staging_root, &slug)?;
    let run_directory = staged_run.directory().to_owned();
    let result = (|| {
        // The main mod's files stay in the run directory, and another member's go to a folder
        // named for it, since two members may each patch the same stock package.
        let mut parts = Vec::with_capacity(builds.len());
        let mut artifacts = Vec::new();
        let mut merge_artifacts = Vec::new();
        let mut map_diagnostics = None;
        let mut mission_scripts = Vec::new();
        for build in &builds {
            let (directory, prefix) = match build.owner {
                Some(owner) => (run_directory.join(owner.id()), format!("{}/", owner.id())),
                None => (run_directory.clone(), String::new()),
            };
            let paths = build
                .bundle
                .write_new_with_progress(&directory, &mut |name, completed, total| {
                    progress(BuildProgress::artifact(
                        BuildPhase::WritingPackages,
                        name,
                        completed,
                        total,
                    ));
                })
                .map_err(|error| format!("Could not write staged packages: {error}"))?;
            let paths = stage_kept_files(&directory, paths)?;
            let mut files = artifact_reports_with_progress(&paths, &mut progress)?;
            validate_outputs(&files)?;
            let merge_paths = match &build.merge_data {
                Some(data) => crate::merge_data::stage(&directory, data)?,
                None => Vec::new(),
            };
            let merge_paths = stage_kept_files(&directory, merge_paths)?;
            let mut merge_files = artifact_reports_with_progress(&merge_paths, &mut progress)?;
            let selected = build
                .maps
                .iter()
                .map(|&index| snapshot.request.maps[index].clone())
                .collect::<Vec<_>>();
            let mut diagnostics = crate::map::diagnostics::stage(&directory, &selected)?;
            let mut scripts = crate::map::mission::stage(&directory, &selected)?;
            for file in files
                .iter_mut()
                .chain(&mut merge_files)
                .chain(&mut diagnostics)
                .chain(&mut scripts)
            {
                file.file_name = format!("{prefix}{}", file.file_name);
            }
            parts.push(crate::mod_family::build::StagedPart {
                owner: build.owner,
                files: files
                    .iter()
                    .chain(&merge_files)
                    .chain(&diagnostics)
                    .chain(&scripts)
                    .cloned()
                    .collect(),
            });
            artifacts.extend(files);
            merge_artifacts.extend(merge_files);
            if diagnostics.is_some() {
                if map_diagnostics.is_some() {
                    return Err("Two members stage map diagnostics".to_owned());
                }
                map_diagnostics = diagnostics;
            }
            mission_scripts.extend(scripts);
        }
        if mission_scripts.len() != crate::mod_family::build::mission_scripts().len() {
            return Err("Mission controller belongs to an unselected map".to_owned());
        }
        let StagedRecipes {
            paths: staged_recipe_paths,
            selection_fingerprint,
            recipe_fingerprints,
        } = stage_recipe_snapshot_with_progress(&run_directory, snapshot, &mut progress)?;
        let staged_recipe_files = staged_recipe_paths
            .iter()
            .take(snapshot.request.recipes.len())
            .map(|path| {
                path.strip_prefix(&run_directory)
                    .unwrap_or(path)
                    .display()
                    .to_string()
            })
            .collect::<Vec<_>>();

        let source_package_directory = fs::canonicalize(&snapshot.request.package_directory)
            .map_err(|error| {
                format!(
                    "Could not canonicalize source package directory {}: {error}",
                    snapshot.request.package_directory.display()
                )
            })?;
        let manifest_path = run_directory.join(MANIFEST_FILE_NAME);
        progress(BuildProgress::artifact(
            BuildPhase::WritingManifest,
            MANIFEST_FILE_NAME,
            0,
            1,
        ));
        // The Project Sunrise section names the badge and watermark of the first mod with items,
        // the main mod's unless every recipe builds in an importer's mod, and lists every mod's
        // watermarked icons. A main mod that only hosts its importers' rows has none of its own.
        let sunrise = {
            let lead = builds
                .iter()
                .find(|build| !build.recipes.is_empty())
                .unwrap_or(&builds[0]);
            let mut sunrise = lead.bundle.plan.sunrise.clone();
            sunrise.watermarked_icon_containers = builds
                .iter()
                .flat_map(|build| &build.bundle.plan.sunrise.watermarked_icon_containers)
                .copied()
                .collect();
            sunrise
        };
        let mut manifest = ManifestDocument {
            mods: Vec::new(),
            schema: MANIFEST_SCHEMA,
            source_package_directory: source_package_directory.display().to_string(),
            source_artifacts: source_artifacts.clone(),
            ignored_authored_files: source_inspection.ignored_authored_files.clone(),
            selection_fingerprint: selection_fingerprint.clone(),
            selected_recipe_files: staged_recipe_files,
            selected_map_files: staged_recipe_paths
                .iter()
                .skip(snapshot.request.recipes.len())
                .map(|path| {
                    path.strip_prefix(&run_directory)
                        .unwrap_or(path)
                        .display()
                        .to_string()
                })
                .collect(),
            project: ManifestProject::from_build(
                &snapshot.request.recipes,
                &plans,
                &sunrise,
                &subclass_classes,
            )?,
            artifacts: artifacts.clone(),
            merge_data: merge_artifacts,
            map_diagnostics,
            mission_scripts,
        };
        manifest.mods = crate::mod_family::build::members(&parts)?;
        write_manifest(&manifest_path, &manifest)?;
        progress(BuildProgress::artifact(
            BuildPhase::WritingManifest,
            MANIFEST_FILE_NAME,
            1,
            1,
        ));

        let report = BuildReport {
            mods: if crate::package_profile::host().is_standalone() {
                let mut reports = crate::mod_family::reports(
                    &manifest.mods,
                    source_package_directory
                        .parent()
                        .ok_or("Missing game root")?,
                    &manifest.artifacts,
                )?;
                // A source that builds into Parhelion says why.
                let coupled = crate::mod_family::build::coupled();
                for report in reports
                    .iter_mut()
                    .filter(|report| report.status == crate::ModStatus::InParhelion)
                {
                    if let Some(reasons) =
                        sundial::package_authoring::mod_content::Importer::from_id(&report.id)
                            .and_then(|importer| coupled.get(&importer))
                    {
                        let reasons = reasons.join(" ");
                        report.note = Some(match report.note.take() {
                            Some(note) => format!("{reasons} {note}"),
                            None => reasons,
                        });
                    }
                }
                // A source whose rows of a shared table Parhelion holds says so, since it then
                // loads only beside Parhelion.
                for report in &mut reports {
                    let Some(importer) =
                        sundial::package_authoring::mod_content::Importer::from_id(&report.id)
                    else {
                        continue;
                    };
                    let tables = crate::mod_family::build::hosted_for(importer);
                    if tables.is_empty() || report.status == crate::ModStatus::InParhelion {
                        continue;
                    }
                    report.note = Some(format!(
                        "Parhelion holds its rows of {}, so it needs Parhelion to load.",
                        sentence_list(tables.iter().map(|table| table.label()))
                    ));
                    report.shared = tables
                        .iter()
                        .map(|table| table.label().to_owned())
                        .collect();
                }
                reports
            } else {
                Vec::new()
            },
            import_conversions: crate::mod_family::build::conversions(),
            weapons: snapshot
                .request
                .recipes
                .iter()
                .zip(&plans)
                .zip(recipe_fingerprints)
                .map(|((recipe, plan), recipe_fingerprint)| {
                    Ok(WeaponBuildReport {
                        name: recipe.name.clone(),
                        kind: plan.kind,
                        namespace: recipe.namespace.clone(),
                        item_hash: plan.item_hash,
                        item_definition_hash: u32::from(plan.definition_tag),
                        item_string_hash: u32::from(plan.string_tag),
                        icon_definition_hash: u32::from(plan.icon_definition_tag),
                        item_index: plan.item_index,
                        collection: plan.collection,
                        details: plan.details.clone(),
                        subclass: plan.subclass.clone(),
                        recipe_fingerprint,
                        custom_plugs: plan
                            .custom_plugs
                            .iter()
                            .map(|plug| CustomPlugBuildReport {
                                socket_index: plug.socket_index,
                                choice_index: plug.choice_index,
                                name: plug.name.clone(),
                                item_hash: plug.item_hash,
                                item_index: plug.item_index,
                                definition_hash: u32::from(plug.definition_tag),
                                string_hash: u32::from(plug.string_tag),
                                icon_definition_hash: plug.icon_definition_tag.map(u32::from),
                                name_hash: plug.name_hash,
                                description_hash: plug.description_hash,
                                offered_sets: plug.offered_sets.clone(),
                                perks: plug
                                    .perks
                                    .iter()
                                    .map(|perk| PrivatePerkBuildReport {
                                        source_perk_index: perk.source_perk_index,
                                        perk_hash: perk.perk_hash,
                                        runtime_key: perk.runtime_key,
                                    })
                                    .collect(),
                            })
                            .collect(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
            run_directory,
            manifest_path,
            artifacts,
            selection_fingerprint,
            staged_recipe_paths,
            skipped,
        };
        Ok(report)
    })();
    let report = staged_run.finish(result)?;
    progress(BuildProgress::phase(BuildPhase::Complete, 1, 1));
    Ok(report)
}

fn project_spec(snapshot: &BatchBuildSnapshot) -> Result<WeaponProjectSpec, BuildFailure> {
    snapshot
        .request
        .recipes
        .iter()
        .map(|recipe| {
            recipe.to_spec().map_err(|error| BuildFailure {
                message: format!(
                    "Recipe: {:?} ({})\nItem: {}\n{error}",
                    recipe.name,
                    recipe.namespace,
                    recipe.identity.item_hash.as_str()
                ),
                recipe: Some(recipe.namespace.clone()),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|weapons| WeaponProjectSpec { weapons })
}

/// The staged recipe snapshots and the fingerprints taken from the encodings they hold.
struct StagedRecipes {
    paths: Vec<PathBuf>,
    selection_fingerprint: String,
    /// Each recipe's own fingerprint, in selection order.
    recipe_fingerprints: Vec<String>,
}

/// Writes each recipe's canonical encoding as its snapshot and fingerprints the selection from
/// the same bytes, so each recipe is encoded once.
fn stage_recipe_snapshot_with_progress(
    run_directory: &Path,
    snapshot: &BatchBuildSnapshot,
    progress: &mut impl FnMut(BuildProgress),
) -> Result<StagedRecipes, String> {
    let recipe_directory = run_directory.join("recipes");
    fs::create_dir(&recipe_directory).map_err(|error| {
        format!(
            "Could not create staged recipe directory {}: {error}",
            recipe_directory.display()
        )
    })?;
    let recipes = &snapshot.request.recipes;
    let total = recipes.len();
    // Names are chosen first, in order, so recipes sharing a slug get the same suffixes.
    let mut paths = Vec::<PathBuf>::with_capacity(total);
    for recipe in recipes {
        let slug = recipe.slug();
        let mut suffix = 0u16;
        let path = loop {
            let file_name = if suffix == 0 {
                format!("{slug}.parhelion.json")
            } else {
                format!("{slug}-{suffix}.parhelion.json")
            };
            let candidate = recipe_directory.join(file_name);
            if !candidate.exists() && !paths.contains(&candidate) {
                break candidate;
            }
            suffix = suffix.checked_add(1).ok_or_else(|| {
                format!("Could not allocate staged filename for {:?}", recipe.name)
            })?;
        };
        paths.push(path);
    }
    // A few recipes are encoded, written and hashed at once, and the selection folds them in order.
    let batch = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .clamp(1, 4);
    let mut selection = SelectionFingerprint::default();
    let mut recipe_fingerprints = Vec::with_capacity(total);
    for (start, chunk) in (0..total)
        .step_by(batch)
        .map(|start| (start, start..(start + batch).min(total)))
    {
        for index in chunk.clone() {
            progress(BuildProgress::artifact(
                BuildPhase::StagingRecipes,
                recipes[index].name.clone(),
                start,
                total,
            ));
        }
        let staged = std::thread::scope(|scope| {
            chunk
                .clone()
                .map(|index| {
                    let (recipe, path) = (&recipes[index], &paths[index]);
                    scope.spawn(move || {
                        let encoded = canonical_recipe(recipe)?;
                        write_staged_recipe(&encoded, path).map_err(|error| {
                            format!(
                                "Could not stage normalized recipe {}: {error}",
                                path.display()
                            )
                        })?;
                        let own = SelectionFingerprint::of(&encoded);
                        Ok::<_, String>((encoded, own))
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|writer| {
                    writer
                        .join()
                        .unwrap_or_else(|_| Err("A recipe snapshot writer panicked".to_owned()))
                })
                .collect::<Vec<_>>()
        });
        for (index, result) in chunk.zip(staged) {
            let (encoded, own) = result?;
            selection.add(&encoded);
            recipe_fingerprints.push(own);
            progress(BuildProgress::artifact(
                BuildPhase::StagingRecipes,
                recipes[index].name.clone(),
                index + 1,
                total,
            ));
        }
    }
    for (index, map) in snapshot.request.maps.iter().enumerate() {
        let encoded = canonical_map(map)?;
        let mut name = format!("map-{index}.parhelion.json");
        while paths.contains(&recipe_directory.join(&name)) {
            name.insert_str(0, "map-");
        }
        let path = recipe_directory.join(name);
        write_staged_recipe(&encoded, &path)?;
        selection.add(&encoded);
        paths.push(path);
        progress(BuildProgress::artifact(
            BuildPhase::StagingRecipes,
            map.title.clone(),
            total + index + 1,
            total + snapshot.request.maps.len(),
        ));
    }
    Ok(StagedRecipes {
        paths,
        selection_fingerprint: selection.finish(),
        recipe_fingerprints,
    })
}

/// Writes `encoded`, a recipe's canonical encoding, as a staged snapshot ending in a newline.
/// Installation fingerprints the file without that newline.
fn write_staged_recipe(encoded: &str, path: &Path) -> Result<(), String> {
    // A staging run is new and exclusively leased until its completion marker is written.
    // Writing the final owned filename avoids abandoned replacement-temp files after a crash.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(encoded.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())
}

fn validate_paths(package_directory: &Path, staging_root: &Path) -> Result<(), String> {
    if !package_directory.is_dir() {
        return Err(format!(
            "Package directory does not exist: {}",
            package_directory.display()
        ));
    }
    let package_directory = fs::canonicalize(package_directory).map_err(|error| {
        format!(
            "Could not resolve package directory {}: {error}",
            package_directory.display()
        )
    })?;
    let staging_root = resolve_path_for_comparison(staging_root)
        .map_err(|error| format!("Could not resolve {}: {error}", staging_root.display()))?;
    if path_is_within(&staging_root, &package_directory) {
        return Err("Staging must be outside the live packages directory".to_owned());
    }
    Ok(())
}

fn validate_stock_patches(stock_patches: &BTreeMap<u16, u16>) -> Result<(), String> {
    for profile in CANONICAL_PACKAGES {
        match stock_patches.get(&profile.package_id) {
            Some(&patch) if patch == profile.stock_patch_id => {}
            Some(patch) => {
                return Err(format!(
                    "Package {:04X} has stock patch {patch}; this profile requires patch {}",
                    profile.package_id, profile.stock_patch_id
                ));
            }
            None => {
                return Err(format!(
                    "Package {:04X} has no stock package chain",
                    profile.package_id
                ));
            }
        }
    }
    Ok(())
}

fn validate_authored_set(
    ignore_installed_authored_overlays: bool,
    authored: &[String],
) -> Result<(), String> {
    if authored.is_empty() {
        return Ok(());
    }
    if !ignore_installed_authored_overlays {
        return Err(format!(
            "The source contains Parhelion-authored packages: {}. Enable the stock-view option or choose a clean package directory",
            authored.join(", ")
        ));
    }

    // `inspect_snapshot` has already authenticated every entry in this list by build
    // signature, package id, patch id, and recognized file name. A prior generation can
    // legitimately be partial (for example, after adding a new output profile), and each
    // recognized overlay can be independently omitted from the filtered stock view. Do
    // not require an obsolete all-or-nothing artifact count here.
    Ok(())
}

fn read_relevant_header(path: &Path) -> Result<Option<PackageHeaderPrefix>, String> {
    let mut prefix = [0u8; PACKAGE_HEADER_PREFIX_SIZE];
    let mut file =
        File::open(path).map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    file.read_exact(&mut prefix)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let header = PackageHeaderPrefix::parse(&prefix);
    if header.version != SHADOWKEEP_HEADER_VERSION {
        return Ok(None);
    }
    if canonical_package(header.package_id).is_none()
        && authored_package(header.package_id).is_none()
    {
        return Ok(None);
    }
    Ok(Some(header))
}

fn file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(OsStr::to_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("Package filename is not valid Unicode: {}", path.display()))
}

fn create_unique_run_directory(root: &Path, slug: &str) -> Result<PathBuf, String> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("System clock is before Unix epoch: {error}"))?
        .as_secs();
    // Retention removes old directories, so a first-free numeric suffix could reuse an old
    // report's path. A fresh random identity keeps stale requests from targeting a later run.
    tempfile::Builder::new()
        .prefix(&format!("{slug}-{seconds}-"))
        .rand_bytes(12)
        .tempdir_in(root)
        .map(tempfile::TempDir::keep)
        .map_err(|error| format!("Could not allocate a unique staging run directory: {error}"))
}

pub(crate) enum PackageSource {
    Direct(PathBuf),
    Filtered(FilteredPackageView),
}

impl PackageSource {
    /// Import preparation and compilation must resolve native tags to the same stock bytes.
    /// Keep the authenticated view alive across the complete conversion batch.
    #[cfg(feature = "d2-model-importer")]
    pub(crate) fn stock(package_directory: &Path) -> Result<Self, String> {
        let inspection = inspect_source(package_directory, true)?;
        Self::prepare(package_directory, &inspection.ignored_authored_files)
    }

    fn prepare(
        package_directory: &Path,
        ignored_authored_files: &[String],
    ) -> Result<Self, String> {
        // Readers of the install project every loaded mod over stock, Parhelion's own and other
        // tools' alike. Builds read stock alone, so any mod content needs the isolated view.
        let modded = package_directory
            .parent()
            .map(sundial::package_authoring::mod_content::content_files)
            .transpose()?
            .is_some_and(|files| !files.is_empty());
        if ignored_authored_files.is_empty() && !modded {
            return Ok(Self::Direct(package_directory.to_path_buf()));
        }
        FilteredPackageView::create(package_directory, ignored_authored_files).map(Self::Filtered)
    }

    pub(crate) fn path(&self) -> &Path {
        match self {
            Self::Direct(path) => path,
            Self::Filtered(view) => &view.package_directory,
        }
    }

    pub(crate) fn finish<T>(
        self,
        result: Result<T, String>,
        warn: impl FnOnce(String),
    ) -> Result<T, String> {
        let cleanup = match self {
            Self::Direct(_) => Ok(()),
            Self::Filtered(view) => view.close(),
        };
        package_views::finish_with_cleanup(result, cleanup, warn)
    }
}

pub(crate) struct FilteredPackageView {
    temporary: Option<tempfile::TempDir>,
    lease: Option<package_views::ViewLease>,
    package_directory: PathBuf,
}

impl FilteredPackageView {
    pub(crate) fn path(&self) -> &Path {
        &self.package_directory
    }

    pub(crate) fn add_overlay(&self, source: &Path) -> Result<(), String> {
        let target = self.package_directory.join(file_name(source)?);
        if target.exists() {
            return Err(format!(
                "Package view already contains {}",
                target.display()
            ));
        }
        // Staging can be on another volume; only the small authored output needs copying.
        if fs::hard_link(source, &target).is_err() {
            let copy = (|| -> std::io::Result<()> {
                let mut source = File::open(source)?;
                let mut destination = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)?;
                std::io::copy(&mut source, &mut destination)?;
                Ok(())
            })();
            copy.map_err(|error| {
                format!(
                    "Could not add {} to the read-only package view: {error}",
                    source.display()
                )
            })?;
        }
        Ok(())
    }

    pub(crate) fn create(source: &Path, ignored: &[String]) -> Result<Self, String> {
        let install_root = source
            .parent()
            .ok_or_else(|| format!("Package directory has no parent: {}", source.display()))?;
        let view_root = package_view_root(source, install_root)?;
        package_views::prune_stale_views(&view_root)?;
        Self::create_in_root(source, ignored, &view_root)
    }

    fn create_in_root(source: &Path, ignored: &[String], view_root: &Path) -> Result<Self, String> {
        package_views::initialize_source_decoder(source)?;
        let install_root = source
            .parent()
            .ok_or_else(|| format!("Package directory has no parent: {}", source.display()))?;
        let temporary = tempfile::Builder::new()
            .prefix(".parhelion-package-view-")
            .tempdir_in(view_root)
            .map_err(|error| {
                format!(
                    "Could not create a temporary stock package view in {}: {error}",
                    view_root.display()
                )
            })?;
        let package_directory = temporary.path().join("packages");
        let lease = package_views::ViewLease::create(temporary.path())?;
        let view = Self {
            temporary: Some(temporary),
            lease: Some(lease),
            package_directory,
        };
        view.populate(source, ignored, install_root)?;
        Ok(view)
    }

    fn populate(
        &self,
        source: &Path,
        ignored: &[String],
        install_root: &Path,
    ) -> Result<(), String> {
        let package_directory = &self.package_directory;
        fs::create_dir(package_directory).map_err(|error| {
            format!(
                "Could not create temporary package view {}: {error}",
                package_directory.display()
            )
        })?;

        let ignored = ignored.iter().map(String::as_str).collect::<BTreeSet<_>>();
        let mut links = Vec::new();
        for entry in fs::read_dir(source)
            .map_err(|error| format!("Could not list {}: {error}", source.display()))?
        {
            let entry = entry.map_err(|error| format!("Could not read package entry: {error}"))?;
            let path = entry.path();
            if !entry
                .file_type()
                .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?
                .is_file()
                || !has_pkg_extension(&path)
            {
                continue;
            }
            let name = file_name(&path)?;
            if ignored.contains(name.as_str()) {
                continue;
            }
            let target = package_directory.join(&name);
            links.push((path, target));
        }
        // A Shadowkeep install holds about 2,200 packages. Linked one at a time they took five
        // seconds, and eight threads link them in about two thirds of that.
        let next = std::sync::atomic::AtomicUsize::new(0);
        let failures = std::thread::scope(|scope| {
            let workers = (0..hash_worker_count().min(links.len()))
                .map(|_| {
                    scope.spawn(|| {
                        let mut failures = Vec::new();
                        loop {
                            let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some((path, target)) = links.get(index) else {
                                return failures;
                            };
                            if let Err(error) = fs::hard_link(path, target) {
                                failures.push((index, error));
                            }
                        }
                    })
                })
                .collect::<Vec<_>>();
            workers
                .into_iter()
                .flat_map(|worker| worker.join().expect("a package link worker panicked"))
                .collect::<Vec<_>>()
        });
        if let Some((index, error)) = failures.into_iter().min_by_key(|(index, _)| *index) {
            return Err(format!(
                "Could not hard-link {} into the temporary stock view: {error}",
                links[index].0.display()
            ));
        }
        copy_oodle_runtime(install_root, package_directory.parent().unwrap())?;
        Ok(())
    }

    pub(crate) fn close(mut self) -> Result<(), String> {
        self.cleanup()
    }

    pub(crate) fn finish<T>(
        self,
        result: Result<T, String>,
        warn: impl FnOnce(String),
    ) -> Result<T, String> {
        package_views::finish_with_cleanup(result, self.close(), warn)
    }

    fn cleanup(&mut self) -> Result<(), String> {
        let Some(temporary) = self.temporary.take() else {
            return Ok(());
        };
        let path = temporary.keep();
        let lease = self
            .lease
            .take()
            .expect("an owned package view has a lease");
        package_views::remove_owned_view(&path, lease)
    }
}

impl Drop for FilteredPackageView {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            // Normal builds use explicit close and report this through the UI. Unwinding and
            // test-only consumers still retain a marked view for a later safe cleanup attempt.
            eprintln!("{error}");
        }
    }
}

fn package_view_root(source: &Path, install_root: &Path) -> Result<PathBuf, String> {
    let preferred = default_data_root().join("package-views");
    match supports_package_hard_links(source, &preferred)? {
        true => Ok(preferred),
        false => {
            let parent = install_root.parent().ok_or_else(|| {
                format!(
                    "The install root {} has no writable sibling for package views",
                    install_root.display()
                )
            })?;
            let same_volume = parent.join(".sundial-parhelion").join("package-views");
            if supports_package_hard_links(source, &same_volume)? {
                Ok(same_volume)
            } else {
                Err(format!(
                    "Could not create a same-volume package view outside {}",
                    install_root.display()
                ))
            }
        }
    }
}

fn supports_package_hard_links(source: &Path, candidate_root: &Path) -> Result<bool, String> {
    fs::create_dir_all(candidate_root).map_err(|error| {
        format!(
            "Could not create package-view root {}: {error}",
            candidate_root.display()
        )
    })?;
    let source_package = first_package_file(source)?;
    let probe = tempfile::Builder::new()
        .prefix(".parhelion-link-probe-")
        .tempdir_in(candidate_root)
        .map_err(|error| {
            format!(
                "Could not create a package-view probe in {}: {error}",
                candidate_root.display()
            )
        })?;
    match fs::hard_link(&source_package, probe.path().join("probe.pkg")) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => Ok(false),
        Err(error) => Err(format!(
            "Could not verify package hard links from {} into {}: {error}",
            source.display(),
            candidate_root.display()
        )),
    }
}

fn first_package_file(directory: &Path) -> Result<PathBuf, String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("Could not list {}: {error}", directory.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|error| {
                format!(
                    "Could not read an entry in {}: {error}",
                    directory.display()
                )
            })?
            .path();
        if !has_pkg_extension(&path) {
            continue;
        }
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
        if metadata.is_file() {
            return Ok(path);
        }
    }
    Err(format!(
        "Package directory contains no .pkg files: {}",
        directory.display()
    ))
}

fn copy_oodle_runtime(install_root: &Path, view_root: &Path) -> Result<(), String> {
    let source_directory = install_root.join("bin").join("x64");
    let target_directory = view_root.join("bin").join("x64");
    for name in ["oo2core_3_win64.dll", "oo2core_9_win64.dll"] {
        let source = source_directory.join(name);
        if !source.is_file() {
            continue;
        }
        fs::create_dir_all(&target_directory).map_err(|error| {
            format!(
                "Could not create temporary Oodle directory {}: {error}",
                target_directory.display()
            )
        })?;
        fs::copy(&source, target_directory.join(name))
            .map_err(|error| format!("Could not stage {}: {error}", source.display()))?;
    }
    Ok(())
}

fn artifact_reports_with_progress(
    paths: &[PathBuf],
    progress: &mut impl FnMut(BuildProgress),
) -> Result<Vec<ArtifactMetadata>, String> {
    let mut reports = Vec::with_capacity(paths.len());
    for (index, path) in paths.iter().enumerate() {
        let artifact_name = file_name(path)?;
        progress(BuildProgress::artifact(
            BuildPhase::ValidatingPackages,
            artifact_name.clone(),
            index,
            paths.len(),
        ));
        fs::metadata(path)
            .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
        let digest = digest_file(path)
            .map_err(|error| format!("Could not hash {}: {error}", path.display()))?;
        reports.push(ArtifactMetadata {
            file_name: artifact_name.clone(),
            byte_length: digest.byte_length,
            sha256: digest.sha256,
        });
        progress(BuildProgress::artifact(
            BuildPhase::ValidatingPackages,
            artifact_name,
            index + 1,
            paths.len(),
        ));
    }
    reports.sort_by(|left, right| left.file_name.cmp(&right.file_name));
    Ok(reports)
}

#[cfg(test)]
fn source_artifact_reports(package_directory: &Path) -> Result<Vec<ArtifactMetadata>, String> {
    source_artifact_reports_with_progress(package_directory, &mut |_, _, _| {})
}

fn source_artifact_reports_with_progress(
    package_directory: &Path,
    progress: &mut dyn FnMut(&str, usize, usize),
) -> Result<Vec<ArtifactMetadata>, String> {
    let mut sources = Vec::new();
    for profile in CANONICAL_PACKAGES {
        let mut has_latest = false;
        for patch_id in 0..=profile.stock_patch_id {
            let file_name = profile.stock_file_name(patch_id);
            let path = package_directory.join(&file_name);
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(format!("Could not inspect {}: {error}", path.display()));
                }
            };
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(format!(
                    "Stock source package is not a regular file: {}",
                    path.display()
                ));
            }
            let header = read_relevant_header(&path)?.ok_or_else(|| {
                format!(
                    "Stock source package {} is not a supported Shadowkeep investment package",
                    path.display()
                )
            })?;
            if header.package_id != profile.package_id
                || header.patch_id != patch_id
                || header.build_signature == SUNDIAL_BUILD_SIGNATURE
            {
                return Err(format!(
                    "Stock source package {} has an incompatible identity",
                    path.display()
                ));
            }
            has_latest |= patch_id == profile.stock_patch_id;
            sources.push((file_name, path));
        }
        if !has_latest {
            return Err(format!(
                "Package {:04X} has no stock patch {} source",
                profile.package_id, profile.stock_patch_id
            ));
        }
    }
    // The stock packages are large and hashing them is pure file reading, so they hash on their
    // own threads. Reports and progress keep source order.
    let total = sources.len();
    let paths = sources
        .iter()
        .map(|(_, path)| path.as_path())
        .collect::<Vec<_>>();
    let mut reports = Vec::with_capacity(total);
    crate::artifact::digest_files(&paths, |index, digest| {
        let (file_name, path) = &sources[index];
        progress(file_name, index, total);
        let digest =
            digest.map_err(|error| format!("Could not hash {}: {error}", path.display()))?;
        reports.push(ArtifactMetadata {
            file_name: file_name.clone(),
            byte_length: digest.byte_length,
            sha256: digest.sha256,
        });
        progress(file_name, reports.len(), total);
        Ok(())
    })?;
    Ok(reports)
}

/// Enough threads to keep the disk busy, capped so a build does not starve the editor.
fn hash_worker_count() -> usize {
    std::thread::available_parallelism()
        .map_or(4, std::num::NonZeroUsize::get)
        .clamp(1, 8)
}

fn validate_source_artifacts_unchanged(
    before: &[ArtifactMetadata],
    after: &[ArtifactMetadata],
) -> Result<(), String> {
    if before != after {
        return Err(
            "Stock source packages changed while the weapon project was being compiled. Discard this staging run and build again"
                .to_owned(),
        );
    }
    Ok(())
}

/// The member's files, with each one a build kept from the installed family linked into its
/// staging folder, so the stage holds the member whole. A coupled importer's reused resources
/// join the main mod's stage this way, from whichever folder holds them.
fn stage_kept_files(directory: &Path, paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    paths
        .into_iter()
        .map(|path| {
            if path.starts_with(directory) {
                return Ok(path);
            }
            let name = path
                .file_name()
                .ok_or_else(|| format!("Kept file {} has no name", path.display()))?;
            let target = directory.join(name);
            if fs::hard_link(&path, &target).is_err() {
                fs::copy(&path, &target).map_err(|error| {
                    format!("Could not stage kept file {}: {error}", path.display())
                })?;
            }
            Ok(target)
        })
        .collect()
}

fn validate_outputs(artifacts: &[ArtifactMetadata]) -> Result<(), String> {
    let names = artifacts
        .iter()
        .map(|artifact| artifact.file_name.as_str())
        .collect::<Vec<_>>();
    authored_packages_for_file_names(names).map_err(|error| {
        format!("Compiler emitted an invalid recipe-selected package set: {error}")
    })?;
    Ok(())
}

fn write_manifest(path: &Path, manifest: &ManifestDocument) -> Result<(), String> {
    manifest
        .validate()
        .map_err(|error| format!("Could not validate build manifest: {error}"))?;
    let encoded = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("Could not encode build manifest: {error}"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
    file.write_all(&encoded)
        .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("Could not sync {}: {error}", path.display()))
}

#[cfg(test)]
mod tests;
