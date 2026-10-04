#[cfg(feature = "d2-model-importer")]
mod importer;
pub(crate) mod pickers;
mod ui_state;
#[cfg(test)]
mod ui_tests;

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

use ui_state::{
    BuildDialogStep, RecipeSaveStatus, RuntimeEditorLayout, WorkbenchPage, preview_column_width,
    preview_height, safe_content_width, workbench_left_column_width,
};

use sundial::activity_log::Entry as LogEntry;
use sundial::investment::{
    CatalogLoadProgress, CatalogLoadingView, InvestmentCatalog, PlugChoicePickerButton,
    PlugSelectionMode, WeaponAmmoType, WeaponDamageProfile, WeaponDonor, WeaponDonorPickerAction,
    WeaponDonorPickerClearChoice, WeaponDonorPickerOptions, WeaponDonorSummary,
    WeaponInventorySlot, WeaponInvestmentStat, WeaponOrnamentAppearance, WeaponRarity,
    WeaponSandboxPerkChoice, WeaponTraitChoice, authored_socket_choice_limit,
    draw_authoring_info_icon, draw_authoring_toolbar, draw_authoring_warning_icon,
    draw_catalog_loading_view, draw_plug_safety_selector, draw_plug_safety_warning,
};
use sundial::package_authoring::{
    PackageAuthoringPreferences, PackageAuthoringUpdate, PackageAuthoringUtility,
    entity::{
        WEAPON_BARREL_COMPONENT_KEY, WEAPON_CONTROLLER_COMPONENT_KEY, WEAPON_INPUT_COMPONENT_KEY,
        WEAPON_MAGAZINE_COMPONENT_KEY, WEAPON_RELOAD_COMPONENT_KEY,
        WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, WEAPON_TRIGGER_CHARGE_COMPONENT_KEY,
        WEAPON_TRIGGER_COMPONENT_KEY,
    },
    open_directory, open_shadowkeep_package_manager, resolve_live_named_tag,
    runtime::{
        WeaponRuntimeField, WeaponRuntimeFieldLocator, WeaponRuntimeFieldSource,
        WeaponRuntimeGraph, WeaponRuntimeValue, WeaponRuntimeValueKind, WeaponRuntimeValueOverride,
        encode_weapon_runtime_value, load_weapon_runtime_graph_for_entity,
    },
    sandbox_perk::load_sandbox_perk_runtime_action,
};
use tiger_pkg::TagHash;

use crate::capabilities::{AuthoringDiagnosticCode, AuthoringField};
use crate::capabilities::{variable_damage_resting_type, variable_damage_supported};
use crate::icon_edit::{IconLayers, WeaponIconEditor, WeaponIconEditorAction};
use crate::install::{
    AccountResyncReport, InstallReport, InstallRequest, MAX_PACKAGE_BACKUP_RETENTION,
    install_staged_packages_with_progress,
};
use crate::preferences::ParhelionPreferences;
use crate::recipe::VariableDamageRecipe;
use crate::runtime::{RuntimeGraphKey, load_effective_runtime_graph};
use crate::workflow::{
    BatchBuildRequest, BatchBuildSnapshot, BuildFailure, BuildPhase, BuildProgress, BuildReport,
    build_and_stage_snapshot_reporting, default_backup_root, default_staging_root,
};
use crate::{
    CombatProfileAction, HexHash, ItemKind, RecipeAmmoType, RecipeLibrary, RecipeLibraryEntry,
    RecipeRarity, RecipeRawPayloadTarget, SupportedPlugSet, WeaponArtArrangementRecipe,
    WeaponCloneIdentity, WeaponDonorReference, WeaponDyeReferenceRecipe, WeaponLocaleTextRecipe,
    WeaponNumericInstructionRecipe, WeaponRawPayloadPatchRecipe, WeaponRecipe,
    WeaponRecipeOverrides, WeaponRuntimeResourcePatchRecipe, WeaponSandboxPerkRuntimeRecipe,
    WeaponSocketColumnRecipe, WeaponSocketPlugVariantRecipe, WeaponStatOverride,
    apply_combat_profile_action, authored_inventory_slot,
    presentation_donor_candidate_is_compatible, recipe_combat_profile_action,
    reconcile_presentation_donor, selected_presentation_donor_is_compatible,
    weapon_authoring_capabilities,
};

const WINDOW_TITLE: &str = "Parhelion";

/// The window's size when it opens: most of the monitor the host window is on, within bounds,
/// so the workbench's columns and the model preview have their room from the first frame.
const WINDOW_MIN_SIZE: [f32; 2] = [900.0, 640.0];
const WINDOW_MAX_SIZE: [f32; 2] = [1_760.0, 1_080.0];
const WINDOW_MONITOR_SHARE: f32 = 0.85;

fn window_size(context: &egui::Context) -> [f32; 2] {
    let monitor = context.input(|input| input.viewport().monitor_size);
    let wanted = monitor.map_or([1_480.0, 940.0], |monitor| {
        [
            monitor.x * WINDOW_MONITOR_SHARE,
            monitor.y * WINDOW_MONITOR_SHARE,
        ]
    });
    std::array::from_fn(|axis| {
        wanted[axis]
            .clamp(WINDOW_MIN_SIZE[axis], WINDOW_MAX_SIZE[axis])
            .floor()
    })
}

fn load_parhelion_logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let image = image::load_from_memory(include_bytes!("../../../assets/sundial-alt.png"))
        .expect("bundled Parhelion logo must be a valid PNG")
        .into_rgba8();
    let (width, height) = image.dimensions();
    let image =
        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], image.as_raw());
    ctx.load_texture("parhelion-logo", image, egui::TextureOptions::LINEAR)
}

#[derive(Clone, Copy)]
struct RuntimeComponentControl {
    binding_hash: u32,
    label: &'static str,
    tooltip: &'static str,
}

const PRIMARY_RUNTIME_COMPONENTS: [RuntimeComponentControl; 4] = [
    RuntimeComponentControl {
        binding_hash: WEAPON_TRIGGER_COMPONENT_KEY,
        label: "Firing Behavior",
        tooltip: "Changes firing cadence and trigger behavior. Stats and appearance are unchanged.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_BARREL_COMPONENT_KEY,
        label: "Barrel Runtime",
        tooltip: "One part of projectile emission. Projectile speed can come from elsewhere.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_MAGAZINE_COMPONENT_KEY,
        label: "Magazine Behavior",
        tooltip: "Magazine and reserve behavior. Ammo Type is on the Weapon tab.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_RELOAD_COMPONENT_KEY,
        label: "Reload Behavior",
        tooltip: "The whole reload component, not only the animation. Depends on the other components.",
    },
];

const ADDITIONAL_RUNTIME_COMPONENTS: [RuntimeComponentControl; 4] = [
    RuntimeComponentControl {
        binding_hash: WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
        label: "Weapon Stat Translator",
        tooltip: "Specific to the weapon family, not a projectile speed control. Swapping across families can freeze the game.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_CONTROLLER_COMPONENT_KEY,
        label: "Weapon Controller",
        tooltip: "Broader than trigger or barrel. Affects several behaviors at once.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_INPUT_COMPONENT_KEY,
        label: "Input",
        tooltip: "The weapon's input handling.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_TRIGGER_CHARGE_COMPONENT_KEY,
        label: "Trigger Charge",
        tooltip: "Optional. The gameplay donor and component donor must both have it.",
    },
];

const SOCKET_CHOICE_PAGE_SIZE: usize = 12;
const LIVE_SOCKET_DIAGNOSTIC_CHOICE_LIMIT: usize = 512;

enum CatalogEvent {
    Progress(CatalogLoadProgress),
    Finished(Box<Result<InvestmentCatalog, String>>),
}

#[derive(Clone, Debug)]
struct TimedBuildProgress {
    phase: BuildPhase,
    current_artifact: Option<String>,
    completed: usize,
    total: usize,
    elapsed: Duration,
}

impl TimedBuildProgress {
    fn from_progress(progress: BuildProgress, elapsed: Duration) -> Self {
        Self {
            phase: progress.phase,
            current_artifact: progress.current_artifact,
            completed: progress.completed,
            total: progress.total,
            elapsed,
        }
    }

    fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.completed as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }
}

enum BuildWorkerEvent {
    Progress(TimedBuildProgress),
    Finished {
        result: Result<BuildReport, BuildFailure>,
        elapsed: Duration,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PendingRecipeAction {
    Close,
    New(ItemKind),
    Open(PathBuf),
    Import,
}

/// The failure the bottom bar reports. Only an activity-log notice can be dismissed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActionDiagnostic {
    AccountSync,
    Installation,
    Build,
    Notice,
}

impl ActionDiagnostic {
    const fn title(self) -> &'static str {
        match self {
            Self::AccountSync => "Account Sync Incomplete",
            Self::Installation => "Installation Blocked",
            Self::Build => "Build Blocked",
            Self::Notice => "Action Needs Attention",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AuthoredIconPreviewKey {
    corner_icon: Option<crate::presentation::Artwork>,
    item_hash: u32,
    container_tag: u32,
    rarity: crate::AuthoredWeaponRarity,
    edit: crate::WeaponIconEdit,
    /// A subclass icon, shown without a rarity plate or watermark.
    plain: bool,
}

impl AuthoredIconPreviewKey {
    /// Whether both previews read the same layers from the packages, whatever their edits.
    fn same_layers(&self, other: &Self) -> bool {
        self.container_tag == other.container_tag
            && self.rarity == other.rarity
            && self.corner_icon == other.corner_icon
            && self.plain == other.plain
    }
}

fn effective_icon_rarity(
    authored: Option<RecipeRarity>,
    inherited: Option<WeaponRarity>,
) -> Option<crate::AuthoredWeaponRarity> {
    use crate::AuthoredWeaponRarity as R;
    authored.map(Into::into).or_else(|| match inherited? {
        WeaponRarity::Common => Some(R::Common),
        WeaponRarity::Uncommon => Some(R::Uncommon),
        WeaponRarity::Rare => Some(R::Rare),
        WeaponRarity::Legendary => Some(R::Legendary),
        WeaponRarity::Exotic => Some(R::Exotic),
        WeaponRarity::Unknown => None,
    })
}

/// A rendered icon, or why it could not render, with what it was rendered from.
enum AuthoredIconPreview<Key = AuthoredIconPreviewKey> {
    Ready {
        key: Key,
        texture: egui::TextureHandle,
    },
    Failed {
        key: Key,
        error: String,
    },
}

/// Parhelion's Sundial-hosted native window.
pub struct Parhelion {
    app: PackageAuthoringApp,
    open: bool,
    close_pending: bool,
    focus_requested: bool,
    icon: Arc<egui::IconData>,
    /// The size the window opened at, held while it is open: the viewport builder is compared
    /// frame to frame, so a changing size would resize the window.
    window_size: Option<[f32; 2]>,
}

impl Default for Parhelion {
    fn default() -> Self {
        let image = image::load_from_memory(include_bytes!(
            "../../../assets/linux/io.github.kylethmpsn.Sundial-window.png"
        ))
        .expect("bundled package-authoring icon must be valid PNG")
        .into_rgba8();
        let (width, height) = image.dimensions();
        let icon = egui::IconData {
            rgba: image.into_raw(),
            width,
            height,
        };
        Self {
            app: PackageAuthoringApp::default(),
            open: false,
            close_pending: false,
            focus_requested: false,
            icon: Arc::new(icon),
            window_size: None,
        }
    }
}

impl PackageAuthoringUtility for Parhelion {
    fn open(
        &mut self,
        context: &egui::Context,
        install_directory: &Path,
        preferences: PackageAuthoringPreferences,
    ) -> Result<(), String> {
        let packages = install_directory.join("packages");
        let branding = crate::branding::Branding::detect(install_directory);
        self.app.perk_workbench.branding = branding;
        if self.app.presentation_editor.branding() != branding {
            self.app.presentation_editor.set_branding(branding);
            self.app.invalidate_results();
            self.app.authored_icon_preview = None;
            self.app.authored_icon_layers = None;
            self.app.library_icons = library_view::LibraryIcons::default();
        }
        if !packages.is_dir() {
            return Err(format!(
                "The selected installation has no packages directory at {}",
                packages.display()
            ));
        }
        if self.app.packages != packages {
            self.app.packages = packages;
            self.app.reset_catalog_load();
            self.app.invalidate_results();
        }
        self.app.show_plug_safety_warnings = sundial::investment::show_plug_safety_warnings();
        self.app.initialize_storage();
        self.app.plug_selection_mode = sundial::investment::default_plug_selection_mode();
        self.app.show_experimental_options = preferences.show_parhelion_experimental_options;
        self.app.preferences_changed = false;
        self.app.scroll_recipe_to_top = true;
        sundial::investment::configure_authoring_fonts(context, install_directory)?;
        self.open = true;
        self.close_pending = false;
        self.focus_requested = true;
        Ok(())
    }

    fn update(&mut self, context: &egui::Context) -> PackageAuthoringUpdate {
        if !self.open {
            return PackageAuthoringUpdate::default();
        }

        let focus_requested = std::mem::take(&mut self.focus_requested);
        let size = *self.window_size.get_or_insert_with(|| window_size(context));
        let close_now = context.show_viewport_immediate(
            egui::ViewportId::from_hash_of("parhelion"),
            egui::ViewportBuilder::default()
                .with_title(WINDOW_TITLE)
                .with_app_id("io.github.kylethmpsn.Sundial.Parhelion")
                .with_inner_size(size)
                .with_min_inner_size(WINDOW_MIN_SIZE)
                .with_icon(self.icon.clone()),
            |viewport_context, _class| {
                if focus_requested {
                    viewport_context.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                self.app.update_ui(viewport_context);
                sundial::ui::model_preview::pause_source(
                    viewport_context,
                    self.app.install_receiver.is_some()
                        || self.app.uninstall.busy()
                        || self.app.catalog_is_loading(),
                );
                let close_requested =
                    viewport_context.input(|input| input.viewport().close_requested());
                if close_requested || self.close_pending {
                    // A view's read is not worth waiting on to close.
                    self.app.perk_workbench.stop_optional_reads();
                }
                let busy = self.app.has_background_work();
                if close_requested {
                    self.close_pending = true;
                    viewport_context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
                if self.close_pending && !busy {
                    self.close_pending = false;
                    self.app.request_recipe_action(PendingRecipeAction::Close);
                }
                if self.app.take_close_approved() {
                    viewport_context.send_viewport_cmd(egui::ViewportCommand::Close);
                    true
                } else {
                    false
                }
            },
        );

        if close_now {
            self.open = false;
            self.close_pending = false;
            self.window_size = None;
            self.app.release_package_access();
        }
        PackageAuthoringUpdate {
            open: self.open,
            busy: self.app.has_background_work(),
            dirty: self.app.recipe_dirty,
            packages_changed: self.app.take_packages_changed(),
            account_changed: std::mem::take(&mut self.app.account_changed),
            preferences_changed: self.app.take_preferences_changed(),
            open_sundial_preferences: std::mem::take(&mut self.app.open_sundial_preferences),
        }
    }

    fn save_on_exit(&mut self) {
        #[cfg(feature = "d2-model-importer")]
        self.app.importer.save_view();
    }
}

struct PackageAuthoringApp {
    recipe: WeaponRecipe,
    /// The (lane, plug) pairs `sync_behavior_socket_pins` wrote itself, so deselecting a behavior
    /// takes back exactly those and never a perk the author placed by hand.
    behavior_pins: socket_editor::BehaviorPins,
    observed_recipe: WeaponRecipe,
    /// Advances whenever the recipe or its baseline is replaced or edited.
    recipe_revision: u64,
    recipe_baseline: WeaponRecipe,
    recipe_path: Option<PathBuf>,
    recipe_dirty: bool,
    recipe_requires_initial_save: bool,
    library_open: bool,
    #[cfg(feature = "d2-model-importer")]
    importer: importer::Importer,
    library_query: String,
    library_state: library_view::LibraryState,
    restore_defaults_preview: Option<crate::recipe_library::RestoreDefaults>,
    invalid_weapon_name: Option<(String, String)>,
    recipe_search_focus_pending: bool,
    build_selection_error: Option<String>,
    build_selection_draft: Option<BTreeSet<PathBuf>>,
    /// The installed items a build would leave out, asked about before it runs.
    build_check: build_check::Check,
    /// The items the installation carries, for the recipe lists' Installed marks.
    installed: installed::Installed,
    build_selection_query: String,
    recipe_library: Option<RecipeLibrary>,
    recipe_entries: Vec<RecipeLibraryEntry>,
    enabled_recipe_paths: BTreeSet<PathBuf>,
    packages: PathBuf,
    staging: String,
    ignore_installed: bool,
    build_receiver: Option<Receiver<BuildWorkerEvent>>,
    build_invalidated: bool,
    build_progress: Option<TimedBuildProgress>,
    build_activity: build_status::Activity,
    install_status: build_status::InstallStatus,
    build_started: Option<Instant>,
    latest_build: Option<Result<BuildReport, String>>,
    /// The namespace of the recipe the latest build stopped on, when one recipe caused it.
    build_blocker: Option<String>,
    build_status_open: bool,
    backup_root: String,
    limit_package_backups: bool,
    /// Shows the Technical Build window for the staged build.
    show_technical_build: bool,
    technical_build_open: bool,
    package_backup_retention: usize,
    backup_recipe_snapshots: bool,
    preferences_open: bool,
    preferences_error: Option<String>,
    perk_workbench: custom_perks::workbench::Workbench,
    preferences_page: preferences_view::PreferencesPage,
    activity_log_open: bool,
    build_dialog_step: BuildDialogStep,
    install_receiver: Option<Receiver<Result<InstallReport, String>>>,
    latest_install: Option<Result<InstallReport, String>>,
    account_resync_receiver: Option<Receiver<Result<AccountResyncReport, String>>>,
    replacement_review: Option<Result<crate::install::ReplacementReview, String>>,
    replacement_receiver: Option<Receiver<Result<crate::install::ReplacementReview, String>>>,
    uninstall: uninstall_view::UninstallUi,
    catalog: Option<InvestmentCatalog>,
    /// Advances whenever a catalog is loaded or dropped.
    catalog_revision: u64,
    catalog_receiver: Option<Receiver<CatalogEvent>>,
    catalog_worker: Option<thread::JoinHandle<()>>,
    catalog_progress: Option<CatalogLoadProgress>,
    catalog_load_requested: bool,
    catalog_reload_pending: bool,
    catalog_force_rebuild: bool,
    donor_summaries: Vec<WeaponDonorSummary>,
    /// Stock bases for each non-weapon kind, in catalog order.
    gear_donors: BTreeMap<ItemKind, Vec<WeaponDonorSummary>>,
    /// Stock subclasses with their ability names, the bases and sources of subclass recipes.
    subclasses: Vec<sundial::investment::SubclassSummary>,
    /// Weapon and gear bases together, for Library rows of every kind.
    library_donors: Vec<WeaponDonorSummary>,
    /// Compatible plugs for the open gear recipe's base item, keyed by that item.
    gear_plug_sets: Option<(
        u32,
        Result<Vec<sundial::investment::WeaponSupportedPlugSet>, String>,
    )>,
    /// Search text in the open gear plug list.
    gear_plug_query: String,
    /// Every ornament that changes a model, each paired with a weapon that lends it a rig.
    ornament_appearances: Vec<WeaponOrnamentAppearance>,
    /// Search text for the Behavior browser.
    behavior_query: String,
    sandbox_perk_choices: Vec<WeaponSandboxPerkChoice>,
    trait_choices: Vec<WeaponTraitChoice>,
    donor_query: String,
    presentation_donor_query: String,
    icon_donor_query: String,
    emblem_page: emblem_view::EmblemPage,
    render_gear_donor_query: String,
    runtime_component_queries: BTreeMap<u32, String>,
    runtime_binding_filter: String,
    runtime_bindings_open: bool,
    runtime_donors: runtime_donors::Browser,
    runtime_dependencies: runtime_dependencies::Browser,
    runtime_graph: Option<(RuntimeGraphKey, Arc<WeaponRuntimeGraph>)>,
    /// The appearance whose rig and animations the loaded graph carries, when it carries one.
    runtime_rig_appearance: Option<u32>,
    /// The rendered runtime-registry section and the (graph, field choice) it was built for.
    technical_registry: Option<technical_build::RegistryCache>,
    technical_registry_fields: bool,
    /// The rendered Technical Build report and the inputs it was rendered from.
    technical_report: Option<technical_build::ReportCache>,
    /// The gear-art markers of the appearance the report describes, and the arrangements they
    /// were read for.
    technical_markers: Option<technical_build::MarkerRead>,
    /// Advances whenever `technical_markers` changes.
    technical_marker_revision: u64,
    technical_marker_job: Option<technical_build::MarkerJob>,
    runtime_graph_error: Option<(RuntimeGraphKey, String)>,
    runtime_graph_job: Option<jobs::RuntimeGraphJob>,
    runtime_graph_target: Option<RuntimeGraphKey>,
    runtime_value_query: String,
    runtime_value_text: BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
    /// Per-field facts and the filtered list for the graph the Runtime Values list last drew.
    runtime_values_cache: Option<RuntimeValuesCache>,
    show_technical_runtime_values: bool,
    weapon_pattern_query: String,
    stat_group_query: String,
    plug_queries: Vec<BTreeMap<usize, String>>,
    socket_choice_pages: Vec<usize>,
    plug_selection_mode: PlugSelectionMode,
    show_plug_safety_warnings: bool,
    show_experimental_options: bool,
    preferences_changed: bool,
    open_sundial_preferences: bool,
    show_internal_stats: bool,
    show_technical_socket_rows: bool,
    perk_request: Option<custom_perks::workbench::Request>,
    icon_editor: Option<WeaponIconEditor>,
    hud_icon_editor: crate::hud_icon::ui::Editor,
    presentation_editor: crate::presentation::ui::Editor,
    authored_icon_preview: Option<AuthoredIconPreview>,
    /// The layers the authored icon preview composes, and the preview they were read for.
    authored_icon_layers: Option<(AuthoredIconPreviewKey, Result<IconLayers, String>)>,
    library_icons: library_view::LibraryIcons,
    dye_colors: donor_view::DyeColors,
    /// What each stock weapon can lend through a part row, read once in the background.
    lenders: donor_view::parts::Lenders,
    /// The Markers section's view and selection.
    marker_editor: donor_view::markers::MarkerEditor,
    animation_query: String,
    type_marker_query: String,
    /// The game's iridescence lookup rows, for the shader page's pickers.
    iridescence: shader_view::Iridescence,
    /// The dye materials the shader page draws its surfaces and icon from.
    dye_materials: shader_view::DyeMaterials,
    /// A shader surface or texture copy waiting for its source shader's dye to load.
    pending_dye_copy: Option<shader_view::DyeCopy>,
    /// The gear type the shader page's dyes show and edit, or every gear type.
    shader_dye_gear: Option<crate::dye::GearType>,
    /// The surface the shader page's inspector shows.
    shader_surface: (crate::dye::DyeChannel, crate::dye::DyeSurface),
    /// The stock shader under the pointer in the shader page's texture menu.
    shader_texture_browse: Option<u32>,
    /// The subclass page's selection, searches and artwork browser.
    subclass_page: subclass_view::PageState,
    /// The item the shader page shows its shader on, and the search of its picker.
    shader_preview_item: Option<u32>,
    shader_preview_query: String,
    appearance_ornaments: donor_view::ornaments::Ornaments,
    pending_recipe_action: Option<PendingRecipeAction>,
    pending_recipe_error: Option<String>,
    scroll_recipe_to_top: bool,
    workbench_page: WorkbenchPage,
    close_approved: bool,
    log: ActivityLog,
    packages_changed: bool,
    account_changed: bool,
    logo: Option<egui::TextureHandle>,
}

impl Default for PackageAuthoringApp {
    fn default() -> Self {
        let recipe = WeaponRecipe::new_unbound("New Recipe")
            .expect("the built-in New Recipe identity must remain valid");
        let recipe_path = None;
        let recipe_entries = Vec::new();
        let enabled_recipe_paths = BTreeSet::new();
        let log = ActivityLog::default();
        let backup_preferences = ParhelionPreferences::default();
        let recipe_library = None;
        let observed_recipe = recipe.clone();
        Self {
            recipe_baseline: recipe.clone(),
            recipe,
            observed_recipe,
            recipe_revision: 0,
            recipe_path,
            recipe_dirty: false,
            recipe_requires_initial_save: false,
            library_open: false,
            #[cfg(feature = "d2-model-importer")]
            importer: importer::Importer::default(),
            library_query: String::new(),
            library_state: library_view::LibraryState::default(),
            restore_defaults_preview: None,
            invalid_weapon_name: None,
            recipe_search_focus_pending: false,
            build_selection_error: None,
            build_selection_draft: None,
            build_check: build_check::Check::default(),
            installed: installed::Installed::default(),
            build_selection_query: String::new(),
            recipe_library,
            recipe_entries,
            enabled_recipe_paths,
            packages: PathBuf::new(),
            staging: default_staging_root().display().to_string(),
            ignore_installed: true,
            build_receiver: None,
            build_invalidated: false,
            build_progress: None,
            build_activity: build_status::Activity::default(),
            install_status: build_status::InstallStatus::default(),
            build_started: None,
            latest_build: None,
            build_blocker: None,
            build_status_open: false,
            backup_root: default_backup_root().display().to_string(),
            limit_package_backups: backup_preferences.limit_package_backups,
            show_technical_build: backup_preferences.show_technical_build,
            technical_build_open: false,
            package_backup_retention: backup_preferences.package_backup_retention,
            backup_recipe_snapshots: backup_preferences.backup_recipe_snapshots,
            preferences_open: false,
            preferences_error: None,
            perk_workbench: custom_perks::workbench::Workbench::default(),
            preferences_page: preferences_view::PreferencesPage::default(),
            activity_log_open: false,
            build_dialog_step: BuildDialogStep::Build,
            install_receiver: None,
            latest_install: None,
            account_resync_receiver: None,
            replacement_review: None,
            replacement_receiver: None,
            uninstall: uninstall_view::UninstallUi::default(),
            catalog: None,
            catalog_revision: 0,
            catalog_receiver: None,
            catalog_worker: None,
            catalog_progress: None,
            catalog_load_requested: false,
            catalog_reload_pending: false,
            catalog_force_rebuild: false,
            donor_summaries: Vec::new(),
            gear_donors: BTreeMap::new(),
            subclasses: Vec::new(),
            library_donors: Vec::new(),
            gear_plug_sets: None,
            gear_plug_query: String::new(),
            ornament_appearances: Vec::new(),
            behavior_query: String::new(),
            sandbox_perk_choices: Vec::new(),
            trait_choices: Vec::new(),
            donor_query: String::new(),
            presentation_donor_query: String::new(),
            icon_donor_query: String::new(),
            emblem_page: emblem_view::EmblemPage::default(),
            render_gear_donor_query: String::new(),
            runtime_component_queries: BTreeMap::new(),
            runtime_binding_filter: String::new(),
            runtime_bindings_open: false,
            runtime_donors: runtime_donors::Browser::default(),
            runtime_dependencies: runtime_dependencies::Browser::default(),
            runtime_graph: None,
            runtime_rig_appearance: None,
            technical_registry: None,
            technical_registry_fields: false,
            technical_report: None,
            technical_markers: None,
            technical_marker_revision: 0,
            technical_marker_job: None,
            runtime_graph_error: None,
            runtime_graph_job: None,
            runtime_graph_target: None,
            runtime_value_query: String::new(),
            runtime_value_text: BTreeMap::new(),
            runtime_values_cache: None,
            show_technical_runtime_values: false,
            weapon_pattern_query: String::new(),
            stat_group_query: String::new(),
            plug_queries: Vec::new(),
            socket_choice_pages: Vec::new(),
            plug_selection_mode: sundial::investment::default_plug_selection_mode(),
            show_plug_safety_warnings: sundial::investment::show_plug_safety_warnings(),
            show_experimental_options: false,
            behavior_pins: socket_editor::BehaviorPins::default(),
            preferences_changed: false,
            open_sundial_preferences: false,
            show_internal_stats: false,
            show_technical_socket_rows: false,
            perk_request: None,
            icon_editor: None,
            hud_icon_editor: crate::hud_icon::ui::Editor::default(),
            presentation_editor: crate::presentation::ui::Editor::default(),
            authored_icon_preview: None,
            authored_icon_layers: None,
            library_icons: library_view::LibraryIcons::default(),
            dye_colors: donor_view::DyeColors::default(),
            lenders: donor_view::parts::Lenders::default(),
            marker_editor: donor_view::markers::MarkerEditor::default(),
            animation_query: String::new(),
            type_marker_query: String::new(),
            iridescence: shader_view::Iridescence::default(),
            dye_materials: shader_view::DyeMaterials::default(),
            pending_dye_copy: None,
            shader_dye_gear: None,
            shader_surface: (
                crate::dye::DyeChannel::Armor,
                crate::dye::DyeSurface::Primary,
            ),
            shader_texture_browse: None,
            subclass_page: subclass_view::PageState::default(),
            shader_preview_item: None,
            shader_preview_query: String::new(),
            appearance_ornaments: donor_view::ornaments::Ornaments::default(),
            pending_recipe_action: None,
            pending_recipe_error: None,
            scroll_recipe_to_top: true,
            workbench_page: WorkbenchPage::default(),
            close_approved: false,
            log,
            packages_changed: false,
            account_changed: false,
            logo: None,
        }
    }
}

impl PackageAuthoringApp {
    fn update_ui(&mut self, ctx: &egui::Context) {
        self.poll_build();
        self.poll_build_check();
        self.poll_install();
        self.poll_account_resync();
        self.poll_replacement_review();
        self.poll_uninstall();
        self.refresh_installed(ctx);
        self.poll_runtime_donors();
        self.poll_technical_markers();
        self.lenders.poll();
        self.marker_editor.poll();
        self.runtime_dependencies.poll();
        if self.uninstall.open {
            egui::CentralPanel::default().show(ctx, |_| {});
            self.draw_uninstall_window(ctx);
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        self.poll_catalog();
        self.poll_runtime_graph();
        if !self.catalog_load_requested && self.install_receiver.is_none() {
            self.start_catalog_load(ctx);
        }
        if self.catalog_is_loading() {
            self.draw_catalog_loading_screen(ctx);
            self.draw_activity_log_window(ctx);
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        self.ensure_runtime_graph(ctx);
        egui::TopBottomPanel::bottom("parhelion_build_actions")
            .resizable(false)
            .show_separator_line(true)
            .show(ctx, |ui| {
                ui.scope(|ui| {
                    workbench_style(ui);
                    ui.set_max_width(safe_content_width(ui.available_width()));
                    ui.add_space(5.0);
                    self.draw_action_error(ui);
                    self.draw_actions(ui);
                    ui.add_space(5.0);
                });
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            workbench_style(ui);
            ui.set_max_width(safe_content_width(ui.available_width()));
            let running = self.build_receiver.is_some()
                || self.library_state.busy()
                || self.install_receiver.is_some()
                || self.perk_workbench.editing();
            let mut recipe_replaced = false;
            ui.add_enabled_ui(!running, |ui| {
                recipe_replaced |= self.draw_recipe_library(ui);
            });
            if recipe_replaced {
                self.scroll_recipe_to_top = true;
                self.workbench_page = WorkbenchPage::Weapon;
            }
            ui.separator();
            ui.add_enabled_ui(!running, |ui| self.draw_workbench_tabs(ui));
            let mut recipe_scroll = egui::ScrollArea::vertical()
                .id_salt(("parhelion-workbench", self.workbench_page))
                .auto_shrink([false, false])
                .drag_to_scroll(false)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded);
            if std::mem::take(&mut self.scroll_recipe_to_top) {
                recipe_scroll = recipe_scroll.vertical_scroll_offset(0.0);
            }
            recipe_scroll.show(ui, |ui| {
                ui.set_max_width(safe_content_width(ui.available_width()));
                ui.add_enabled_ui(!running, |ui| {
                    self.draw_recipe_editor(ui);
                });
            });
            self.synchronize_recipe_dirty();
        });
        #[cfg(feature = "d2-model-importer")]
        self.draw_importer(ctx);
        self.draw_build_status_window(ctx);
        self.draw_runtime_bindings_window(ctx);
        self.draw_perk_workbench(ctx);
        self.draw_runtime_donor_browser(ctx);
        self.draw_runtime_dependencies(ctx);
        self.draw_icon_editor(ctx);
        self.draw_artwork_editor(ctx);
        self.draw_preferences_window(ctx);
        self.draw_activity_log_window(ctx);
        self.draw_technical_build_window(ctx);
        self.draw_build_check(ctx);
        self.draw_discard_confirmation(ctx);
        self.draw_library_windows(ctx);
        self.synchronize_recipe_dirty();
        self.follow_appearance_preview(ctx);
        if self.has_background_work() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn catalog_is_loading(&self) -> bool {
        self.catalog_receiver.is_some()
            || self.catalog_worker.is_some()
            || self.catalog_reload_pending
    }

    fn draw_catalog_loading_screen(&mut self, ctx: &egui::Context) {
        let logo = self
            .logo
            .get_or_insert_with(|| load_parhelion_logo_texture(ctx))
            .clone();
        let progress = self.catalog_progress.unwrap_or(CatalogLoadProgress {
            message: "Loading weapon catalog…",
            completed: 0,
            total: 0,
        });
        draw_catalog_loading_view(
            ctx,
            &logo,
            CatalogLoadingView {
                product_name: WINDOW_TITLE,
                version: sundial::version::display(),
                message: progress.message,
                completed: progress.completed,
                total: progress.total,
                source_path: self.packages.parent().or(Some(self.packages.as_path())),
            },
        );
    }

    fn has_background_work(&self) -> bool {
        #[cfg(feature = "d2-model-importer")]
        if self.importer.busy() {
            return true;
        }
        self.uninstall.busy()
            || self.installed.busy()
            || self.library_state.busy()
            || self.build_receiver.is_some()
            || self.install_receiver.is_some()
            || self.account_resync_receiver.is_some()
            || self.replacement_receiver.is_some()
            || self.build_check.busy()
            || self.catalog_is_loading()
            || self.runtime_graph_job.is_some()
            || self.runtime_donors.busy()
            || self.runtime_dependencies.busy()
            || self.perk_workbench.busy()
            || self.technical_markers_busy()
            || self.lenders.busy()
            || self.marker_editor.busy()
    }

    fn importer_busy(&self) -> bool {
        #[cfg(feature = "d2-model-importer")]
        {
            self.importer.busy()
        }
        #[cfg(not(feature = "d2-model-importer"))]
        {
            false
        }
    }

    fn release_package_access(&mut self) {
        debug_assert!(!self.has_background_work());
        self.drop_loaded_catalog();
        self.catalog_progress = None;
        self.catalog_load_requested = false;
        self.catalog_reload_pending = false;
    }

    fn drop_loaded_catalog(&mut self) {
        self.runtime_donors.invalidate();
        self.runtime_dependencies.invalidate();
        self.perk_workbench.invalidate();
        self.technical_markers = None;
        self.technical_marker_revision = self.technical_marker_revision.wrapping_add(1);
        self.hud_icon_editor = crate::hud_icon::ui::Editor::default();
        self.presentation_editor.reset();
        self.library_icons = library_view::LibraryIcons::default();
        self.dye_colors = donor_view::DyeColors::default();
        self.lenders = donor_view::parts::Lenders::default();
        self.iridescence = shader_view::Iridescence::default();
        self.dye_materials = shader_view::DyeMaterials::default();
        self.pending_dye_copy = None;
        self.appearance_ornaments = donor_view::ornaments::Ornaments::default();
        self.catalog = None;
        self.catalog_revision = self.catalog_revision.wrapping_add(1);
        self.donor_summaries.clear();
        self.gear_donors.clear();
        self.subclasses.clear();
        self.library_donors.clear();
        self.gear_plug_sets = None;
        self.ornament_appearances.clear();
        self.library_state.refresh_donors(&self.library_donors);
        self.sandbox_perk_choices.clear();
        self.trait_choices.clear();
        self.plug_queries.clear();
        self.socket_choice_pages.clear();
        self.icon_editor = None;
        self.authored_icon_preview = None;
        self.authored_icon_layers = None;
        self.runtime_graph = None;
        self.runtime_rig_appearance = None;
        self.runtime_graph_error = None;
        self.runtime_graph_target = None;
        self.runtime_value_text.clear();
    }

    fn clear_dependent_picker_queries(&mut self) {
        self.runtime_donors.invalidate();
        self.invalid_weapon_name = None;
        self.clear_presentation_picker_queries();
        self.perk_request = None;
        self.runtime_bindings_open = false;
        self.workbench_page = WorkbenchPage::Weapon;
        self.presentation_donor_query.clear();
        self.runtime_component_queries.clear();
        self.runtime_graph = None;
        self.runtime_rig_appearance = None;
        self.runtime_graph_error = None;
        self.runtime_graph_target = None;
        self.runtime_value_text.clear();
        self.weapon_pattern_query.clear();
        self.stat_group_query.clear();
        self.type_marker_query.clear();
        self.plug_queries.clear();
        self.socket_choice_pages.clear();
    }

    fn clear_presentation_picker_queries(&mut self) {
        self.animation_query.clear();
        self.hud_icon_editor = crate::hud_icon::ui::Editor::default();
        self.presentation_editor.reset();
        self.icon_donor_query.clear();
        self.emblem_page = emblem_view::EmblemPage::default();
        self.render_gear_donor_query.clear();
        self.icon_editor = None;
        self.authored_icon_preview = None;
    }

    fn draw_icon_editor(&mut self, ctx: &egui::Context) {
        if self.build_receiver.is_some() || self.install_receiver.is_some() {
            return;
        }
        let action = self
            .icon_editor
            .as_mut()
            .and_then(|editor| editor.show(ctx));
        match action {
            Some(WeaponIconEditorAction::Apply(edit)) => {
                if self.recipe.overrides.icon_edit != edit {
                    self.recipe.overrides.icon_edit = edit;
                    self.recipe_dirty = true;
                    self.invalidate_results();
                }
                self.icon_editor = None;
            }
            Some(WeaponIconEditorAction::Cancel) => self.icon_editor = None,
            None => {}
        }
    }

    fn draw_artwork_editor(&mut self, ctx: &egui::Context) {
        if !self.presentation_editor.editing()
            || self.build_receiver.is_some()
            || self.install_receiver.is_some()
        {
            return;
        }
        let icon = (|| {
            let donor = self
                .recipe
                .icon_donor
                .as_ref()
                .or(self.recipe.presentation_donor.as_ref())
                .unwrap_or(&self.recipe.donor);
            let hash = donor.item_hash.parse_u32().ok()?;
            let tag = self.catalog.as_ref()?.weapon_icon_container(hash)?;
            Some((
                TagHash(tag),
                self.authored_icon_rarity()?,
                self.recipe.overrides.icon_edit.clone(),
            ))
        })();
        if self.presentation_editor.show(
            ctx,
            &mut self.recipe.overrides,
            &self.packages,
            self.catalog.as_ref(),
            icon,
        ) {
            self.recipe_dirty = true;
            self.authored_icon_preview = None;
            self.invalidate_results();
        }
    }

    fn authored_icon_preview(
        &mut self,
        context: &egui::Context,
        item_hash: u32,
        container_tag: TagHash,
    ) -> Result<Option<egui::TextureHandle>, String> {
        let rarity = self
            .authored_icon_rarity()
            .ok_or("Choose a base item to set the icon rarity")?;
        let edit = &self.recipe.overrides.icon_edit;
        let key = AuthoredIconPreviewKey {
            corner_icon: self.recipe.overrides.corner_icon.clone(),
            item_hash,
            container_tag: u32::from(container_tag),
            rarity,
            edit: edit.clone(),
            plain: self.recipe.kind == crate::ItemKind::Subclass,
        };
        let cached_matches = match self.authored_icon_preview.as_ref() {
            Some(AuthoredIconPreview::Ready { key: cached, .. })
            | Some(AuthoredIconPreview::Failed { key: cached, .. }) => *cached == key,
            None => false,
        };
        if !cached_matches {
            // An edit alone composes the layers already read, so a shader drawing its icon from
            // its dyes never opens the packages again as its values change.
            let layers = match self.authored_icon_layers.take() {
                Some((read, layers)) if read.same_layers(&key) => (read, layers),
                _ => {
                    let layers = IconLayers::load(
                        &self.packages,
                        container_tag,
                        rarity,
                        key.corner_icon.as_ref(),
                        key.plain,
                    );
                    (key.clone(), layers)
                }
            };
            let image = match &layers.1 {
                Ok(layers) => layers.render(&key.edit),
                Err(error) => Err(error.clone()),
            };
            self.authored_icon_layers = Some(layers);
            self.authored_icon_preview = Some(match image {
                Ok(image) => AuthoredIconPreview::Ready {
                    key,
                    texture: context.load_texture(
                        format!("parhelion-authored-icon-{item_hash:08X}"),
                        image,
                        egui::TextureOptions::LINEAR,
                    ),
                },
                Err(error) => AuthoredIconPreview::Failed { key, error },
            });
        }
        match self.authored_icon_preview.as_ref() {
            Some(AuthoredIconPreview::Ready { texture, .. }) => Ok(Some(texture.clone())),
            Some(AuthoredIconPreview::Failed { error, .. }) => Err(error.clone()),
            None => Ok(None),
        }
    }

    fn take_packages_changed(&mut self) -> bool {
        std::mem::take(&mut self.packages_changed)
    }

    fn set_show_experimental_options(&mut self, show: bool) {
        if !show {
            self.runtime_bindings_open = false;
            self.runtime_donors.close();
        }
        if self.show_experimental_options != show {
            self.show_experimental_options = show;
            self.preferences_changed = true;
        }
    }

    fn take_preferences_changed(&mut self) -> Option<PackageAuthoringPreferences> {
        std::mem::take(&mut self.preferences_changed).then_some(PackageAuthoringPreferences {
            show_parhelion_experimental_options: self.show_experimental_options,
        })
    }

    fn save_backup_preferences(&mut self) {
        let preferences = ParhelionPreferences {
            limit_package_backups: self.limit_package_backups,
            show_technical_build: self.show_technical_build,
            package_backup_retention: self.package_backup_retention,
            backup_recipe_snapshots: self.backup_recipe_snapshots,
            ..ParhelionPreferences::default()
        };
        match preferences.save_default() {
            Ok(_) => self.preferences_error = None,
            Err(error) => {
                self.log.push(LogEntry::error(error.clone()));
                self.preferences_error = Some(error);
            }
        }
    }
}

impl PackageAuthoringApp {
    fn draw_workbench_tabs(&mut self, ui: &mut egui::Ui) {
        let kind = self.recipe.kind;
        let pages = WorkbenchPage::for_kind(kind);
        if !pages.contains(&self.workbench_page) {
            self.workbench_page = WorkbenchPage::Weapon;
        }
        ui.horizontal_wrapped(|ui| {
            for &page in pages {
                ui.selectable_value(&mut self.workbench_page, page, page.label_for(kind));
            }
        });
    }

    fn draw_recipe_editor(&mut self, ui: &mut egui::Ui) {
        if self.catalog.is_none() {
            if self.install_receiver.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Installing…");
                });
                return;
            }
            ui.horizontal(|ui| {
                ui.colored_label(ui.visuals().warn_fg_color, "Weapon catalog unavailable.");
                if ui
                    .add_enabled(
                        !self.has_background_work(),
                        egui::Button::new("Reload Catalog"),
                    )
                    .clicked()
                {
                    self.reset_catalog_load();
                }
            });
        }
        if !self.show_experimental_options {
            let hidden_features = technical_recipe_features(&self.recipe);
            if !hidden_features.is_empty() {
                egui::CollapsingHeader::new(format!(
                    "Additional Overrides ({})",
                    hidden_features.len()
                ))
                .id_salt("hidden_recipe_overrides")
                .show(ui, |ui| {
                    for feature in hidden_features {
                        ui.label(feature);
                    }
                    if ui.button("Show Technical Controls").clicked() {
                        self.set_show_experimental_options(true);
                    }
                });
            }
        }
        match self.workbench_page {
            WorkbenchPage::Weapon if !self.recipe.kind.is_weapon() => self.draw_gear_editor(ui),
            WorkbenchPage::Weapon => self.draw_core_recipe_editor(ui),
            WorkbenchPage::Appearance if self.recipe.kind == ItemKind::Subclass => {
                self.draw_subclass_appearance(ui);
            }
            WorkbenchPage::Appearance => self.draw_appearance_workspace(ui),
            WorkbenchPage::Collections => self.draw_collections_workspace(ui),
            WorkbenchPage::Advanced => {
                let donor = self.current_donor();
                self.draw_gameplay_workspace(ui, donor.as_ref());
            }
            WorkbenchPage::Identity => self.draw_identity_workspace(ui),
        }
    }

    fn recipe_panel_scope(&self) -> String {
        self.recipe_path.as_ref().map_or_else(
            || format!("unsaved:{}", self.recipe.identity.item_hash),
            |path| path.display().to_string(),
        )
    }

    fn draw_recipe_library(&mut self, ui: &mut egui::Ui) -> bool {
        let mut replaced = false;
        let Some(library) = self.recipe_library.clone() else {
            draw_authoring_toolbar(ui, |ui| {
                self.draw_new_item_button(ui, &mut replaced);
                ui.separator();
                if ui.button("Custom Perk Workbench…").clicked() {
                    self.perk_workbench.open = true;
                }
                self.draw_tools_menu(ui);
                ui.separator();
                ui.colored_label(ui.visuals().warn_fg_color, "Recipe library unavailable.");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Preferences…").clicked() {
                        self.preferences_open = true;
                    }
                });
            });
            return replaced;
        };

        draw_authoring_toolbar(ui, |ui| {
            self.draw_recipe_library_primary(ui, &library, &mut replaced);
            ui.menu_button("Recipe…", |ui| {
                self.draw_recipe_library_actions(ui, &mut replaced);
            });
            ui.separator();
            if ui.button("Custom Perk Workbench…").clicked() {
                self.perk_workbench.open = true;
            }
            self.draw_tools_menu(ui);
            ui.separator();
            if ui.button("Preferences…").clicked() {
                self.preferences_open = true;
            }
        });
        replaced
    }

    /// New Weapon stays one click. The caret beside it lists every kind.
    fn draw_new_item_button(&mut self, ui: &mut egui::Ui, replaced: &mut bool) {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            if ui.button("New Weapon").clicked() {
                *replaced |= self.request_recipe_action(PendingRecipeAction::New(ItemKind::Weapon));
            }
            let menu = ui.menu_button(
                crate::app::style::icon(ui, egui_phosphor::regular::CARET_DOWN),
                |ui| {
                    for kind in ItemKind::ALL {
                        // Weapons and armor, then the rest of the loadout.
                        if kind == ItemKind::Sparrow {
                            ui.separator();
                        }
                        if ui.button(kind.label()).clicked() {
                            *replaced |= self.request_recipe_action(PendingRecipeAction::New(kind));
                            ui.close_menu();
                        }
                    }
                },
            );
            named_control(menu.response, "New Item").on_hover_text("New Item");
        });
    }

    fn draw_tools_menu(&mut self, ui: &mut egui::Ui) {
        #[cfg(feature = "d2-model-importer")]
        let importer_enabled = self.importer.enabled;
        #[cfg(not(feature = "d2-model-importer"))]
        let importer_enabled = false;
        let has_tools =
            self.show_experimental_options || importer_enabled || self.show_technical_build;
        ui.menu_button("Tools", |ui| {
            #[cfg(feature = "d2-model-importer")]
            if importer_enabled && ui.button("D2 Importer…").clicked() {
                self.importer.open = true;
                ui.close_menu();
            }
            if self.show_experimental_options && ui.button("Engine Catalog…").clicked() {
                self.perk_workbench.open_engine_catalog();
                ui.close_menu();
            }
            if self.show_technical_build
                && ui
                    .button("Technical Build…")
                    .on_hover_text(
                        "What the next build assigns, or what the staged build produced.",
                    )
                    .clicked()
            {
                self.technical_build_open = true;
                ui.close_menu();
            }
            if has_tools {
                ui.separator();
            }
            if ui
                .add_enabled(
                    self.account_resync_receiver.is_none()
                        && self.install_receiver.is_none()
                        && !self.uninstall.open,
                    egui::Button::new("Resync Account"),
                )
                .on_hover_text(
                    "Apply the installed unlocks and items to the current account again.",
                )
                .clicked()
            {
                self.start_account_resync();
                ui.close_menu();
            }
        });
    }

    fn draw_recipe_library_primary(
        &mut self,
        ui: &mut egui::Ui,
        library: &RecipeLibrary,
        replaced: &mut bool,
    ) {
        if ui
            .add_enabled(
                self.build_selection_draft.is_none(),
                egui::Button::new("Library…"),
            )
            .clicked()
        {
            self.library_open = true;
            self.recipe_search_focus_pending = true;
        }
        self.draw_new_item_button(ui, replaced);
        ui.separator();
        ui.allocate_ui_with_layout(
            egui::vec2(
                ui.available_width().min(220.0),
                ui.spacing().interact_size.y,
            ),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&self.recipe.name).strong()).truncate())
            },
        )
        .inner
        .on_hover_text(&self.recipe.name);
        let can_save_existing = self
            .recipe_path
            .as_ref()
            .is_some_and(|path| path.starts_with(library.root()));
        let save_status = RecipeSaveStatus::derive(can_save_existing, self.recipe_dirty);
        let status_color = match save_status {
            RecipeSaveStatus::NotSavedYet => ui.visuals().text_color(),
            RecipeSaveStatus::UnsavedChanges => ui.visuals().warn_fg_color,
            RecipeSaveStatus::Saved => style::success_color(ui.visuals()),
        };
        ui.label(egui::RichText::new(save_status.label()).color(status_color));
        ui.separator();
        let save_label = if can_save_existing {
            "Save Changes"
        } else {
            "Save Recipe"
        };
        let can_save =
            (self.recipe_dirty || !can_save_existing) && self.invalid_weapon_name.is_none();
        if ui
            .add_enabled(can_save, egui::Button::new(save_label))
            .on_disabled_hover_text(if self.invalid_weapon_name.is_some() {
                "Enter a valid weapon name before saving"
            } else {
                "This recipe has no unsaved changes"
            })
            .clicked()
        {
            if can_save_existing {
                self.save_library_recipe();
            } else {
                self.save_recipe_copy();
            }
        }
    }

    fn draw_recipe_library_actions(&mut self, ui: &mut egui::Ui, replaced: &mut bool) {
        if ui
            .button("Duplicate")
            .on_hover_text(
                "Copy this weapon and its custom perks as a new weapon. Save to keep it.",
            )
            .clicked()
        {
            *replaced |= self.duplicate_recipe();
            ui.close_menu();
        }
        if ui.button("Export…").clicked() {
            self.export_recipe();
            ui.close_menu();
        }
        if ui.button("Import…").clicked() {
            *replaced |= self.request_recipe_action(PendingRecipeAction::Import);
            ui.close_menu();
        }
        ui.separator();
        if ui
            .add_enabled(self.recipe_dirty, egui::Button::new("Discard Changes"))
            .on_hover_text("Revert to the last saved version")
            .clicked()
        {
            self.discard_recipe_changes();
            *replaced = true;
            ui.close_menu();
        }
    }

    fn draw_discard_confirmation(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending_recipe_action.clone() else {
            return;
        };
        let mut save = false;
        let mut discard = false;
        let mut cancel = false;
        let response = egui::Modal::new("parhelion_discard_recipe".into()).show(ctx, |ui| {
            workbench_style(ui);
            ui.set_width(420.0_f32.min((ctx.screen_rect().width() - 48.0).max(180.0)));
            ui.heading("Unsaved Recipe Changes");
            ui.add_space(6.0);
            ui.label(match &action {
                PendingRecipeAction::Close => {
                    "Save this recipe before closing Parhelion, or discard its unsaved changes."
                }
                PendingRecipeAction::New(_) => {
                    "Save this recipe before creating a new one, or discard its unsaved changes."
                }
                PendingRecipeAction::Open(_) => {
                    "Save this recipe before opening another, or discard its unsaved changes."
                }
                PendingRecipeAction::Import => {
                    "Save this recipe before importing another, or discard its unsaved changes."
                }
            });
            if let Some(error) = self.pending_recipe_error.as_deref().or_else(|| {
                self.invalid_weapon_name
                    .as_ref()
                    .map(|(_, error)| error.as_str())
            }) {
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color))
                        .wrap(),
                );
            }
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                save = ui
                    .add_enabled(
                        self.invalid_weapon_name.is_none(),
                        egui::Button::new(match action {
                            PendingRecipeAction::Close => "Save and Close",
                            _ => "Save and Continue",
                        })
                        .fill(ui.visuals().selection.bg_fill),
                    )
                    .clicked();
                if ui
                    .button(match action {
                        PendingRecipeAction::Close => "Discard and Close",
                        _ => "Discard and Continue",
                    })
                    .clicked()
                {
                    discard = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        cancel |= response.should_close();
        if save {
            match self.try_save_open_recipe() {
                Ok(()) => {
                    self.pending_recipe_action = None;
                    self.pending_recipe_error = None;
                    self.execute_recipe_action(action);
                }
                Err(error) => {
                    self.log.push(LogEntry::error(&error));
                    self.pending_recipe_error = Some(error);
                }
            }
        } else if discard {
            self.pending_recipe_action = None;
            self.pending_recipe_error = None;
            self.execute_recipe_action(action);
        } else if cancel {
            self.pending_recipe_action = None;
            self.pending_recipe_error = None;
        }
    }

    fn draw_actions(&mut self, ui: &mut egui::Ui) {
        let building = self.build_receiver.is_some();
        let installing = self.install_receiver.is_some();
        let running =
            building || installing || self.perk_workbench.editing() || self.library_state.busy();
        let included_count = self.enabled_recipe_paths.len();
        let catalog_ready = self.catalog.is_some()
            && self.catalog_receiver.is_none()
            && self.catalog_worker.is_none()
            && !self.catalog_reload_pending;
        let project_ready = included_count > 0 && catalog_ready;
        let noun = ItemKind::count_noun(
            self.enabled_recipe_paths.iter().filter_map(|path| {
                self.recipe_entries
                    .iter()
                    .find(|entry| &entry.path == path)
                    .map(|entry| entry.kind)
            }),
            included_count,
        );
        ui.horizontal_wrapped(|ui| {
            let primary_fill = ui.visuals().selection.bg_fill;
            if ui
                .add_enabled(
                    !running,
                    egui::Button::new(format!("{included_count} {noun} in Build…"))
                        .min_size([0.0, ui.spacing().interact_size.y].into()),
                )
                .clicked()
            {
                self.open_build_selection();
            }
            let checking = self.build_check.busy();
            if ui
                .add_enabled(
                    !running && !checking && project_ready && self.build_selection_draft.is_none(),
                    egui::Button::new(
                        egui::RichText::new(if checking {
                            "Checking Installed Items…"
                        } else {
                            "Build & Stage"
                        })
                        .strong(),
                    )
                    .fill(primary_fill)
                    .min_size([160.0, ui.spacing().interact_size.y].into()),
                )
                .clicked()
            {
                self.start_build_checked(ui.ctx());
            }
            if (building || installing || self.latest_build.is_some())
                && ui.button("Build & Install Status…").clicked()
            {
                self.build_status_open = true;
            }
            self.draw_build_notice(ui);
        });
        if included_count == 0 {
            ui.colored_label(ui.visuals().warn_fg_color, "Nothing selected to build.");
        } else if !catalog_ready && !installing {
            ui.colored_label(ui.visuals().warn_fg_color, "Weapon catalog unavailable.");
        }
    }

    fn draw_build_notice(&self, ui: &mut egui::Ui) {
        if self.current_recipe_is_in_build() {
            return;
        }
        let message = if self.recipe_path.is_none() {
            "Recipe not saved or in build."
        } else {
            "Open recipe not in build."
        };
        let color = ui.visuals().warn_fg_color;
        let width = ui
            .available_size_before_wrap()
            .x
            .max(260.0)
            .min(ui.max_rect().width());
        ui.allocate_ui_with_layout(
            egui::vec2(width, ui.spacing().interact_size.y),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(message).color(color))
                        .wrap()
                        .halign(egui::Align::RIGHT),
                );
            },
        );
    }

    fn draw_action_error(&mut self, ui: &mut egui::Ui) {
        let diagnostic = self
            .latest_install
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .and_then(|report| {
                [
                    report
                        .profile_sync
                        .as_ref()
                        .and_then(|result| result.as_ref().err()),
                    report
                        .item_grants
                        .as_ref()
                        .and_then(|result| result.as_ref().err()),
                ]
                .into_iter()
                .flatten()
                .next()
            })
            .map(|error| (ActionDiagnostic::AccountSync, error.as_str()))
            .or_else(|| {
                self.latest_install
                    .as_ref()
                    .and_then(|report| report.as_ref().err())
                    .map(|error| (ActionDiagnostic::Installation, error.as_str()))
            })
            .or_else(|| {
                self.latest_build
                    .as_ref()
                    .and_then(|report| report.as_ref().err())
                    .map(|error| (ActionDiagnostic::Build, error.as_str()))
            })
            .or_else(|| {
                self.log
                    .notice
                    .as_deref()
                    .map(|error| (ActionDiagnostic::Notice, error))
            });
        let Some((kind, error)) = diagnostic else {
            return;
        };

        let mut dismiss = false;
        egui::ScrollArea::vertical()
            .id_salt("parhelion_action_error")
            .max_height(72.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.set_width(safe_content_width(ui.available_width()));
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(kind.title())
                            .strong()
                            .color(ui.visuals().error_fg_color),
                    );
                    if kind == ActionDiagnostic::Notice {
                        dismiss = ui.button("Dismiss").clicked();
                    }
                });
                ui.add(
                    egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color))
                        .wrap(),
                );
            });
        if dismiss {
            self.log.notice = None;
        }
        ui.add_space(5.0);
    }

    fn current_donor(&self) -> Option<WeaponDonor> {
        let hash = self.recipe.donor.item_hash.parse_u32().ok()?;
        let mut donor = self
            .catalog
            .as_ref()?
            .weapon_donor_with_stat_group_index(hash, self.recipe.overrides.stat_group_index)?;
        // A stat group of the recipe's own shows its stats as the build will.
        if let Some(group) = &self.recipe.overrides.custom_stat_group {
            group.apply(&mut donor.investment_stats);
            group.apply(&mut donor.addable_investment_stats);
        }
        Some(donor)
    }

    /// The open recipe's base item, weapon or gear.
    fn current_item_donor(&self) -> Option<WeaponDonor> {
        if self.recipe.kind.is_weapon() {
            self.current_donor()
        } else {
            self.current_gear_donor()
        }
    }

    /// Stock bases for `kind`, or nothing for weapons, which use [`Self::donor_summaries`].
    fn gear_donors_for(&self, kind: ItemKind) -> &[WeaponDonorSummary] {
        self.gear_donors.get(&kind).map_or(&[], Vec::as_slice)
    }

    fn authored_icon_rarity(&self) -> Option<crate::AuthoredWeaponRarity> {
        let hash = self.recipe.donor.item_hash.parse_u32().ok();
        effective_icon_rarity(
            self.recipe.overrides.rarity,
            (if self.recipe.kind.is_weapon() {
                self.donor_summaries.as_slice()
            } else {
                self.gear_donors_for(self.recipe.kind)
            })
            .iter()
            .find(|donor| Some(donor.hash) == hash)
            .map(|donor| donor.rarity),
        )
    }

    fn current_geometry_donor(&self) -> Option<WeaponDonor> {
        let hash = self
            .recipe
            .presentation_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok())
            .or_else(|| self.recipe.donor.item_hash.parse_u32().ok())?;
        self.catalog
            .as_ref()?
            .weapon_donor_with_stat_group_index(hash, None)
    }

    fn current_render_gear_donor(&self) -> Option<WeaponDonor> {
        let hash = self
            .recipe
            .render_gear_donor
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok())
            .or_else(|| {
                self.recipe
                    .presentation_donor
                    .as_ref()
                    .and_then(|donor| donor.item_hash.parse_u32().ok())
            })
            .or_else(|| self.recipe.donor.item_hash.parse_u32().ok())?;
        self.catalog
            .as_ref()?
            .weapon_donor_with_stat_group_index(hash, None)
    }
}

fn merge_stat_profile_investment_rows(
    overrides: &mut WeaponRecipeOverrides,
    gameplay_stats: &[WeaponInvestmentStat],
    profile_stats: &[WeaponInvestmentStat],
) {
    let profile_definitions = profile_stats
        .iter()
        .map(|stat| stat.definition_index)
        .collect::<BTreeSet<_>>();
    overrides
        .removed_investment_stats
        .retain(|definition| !profile_definitions.contains(definition));

    for stat in profile_stats {
        let inherited = gameplay_stats
            .iter()
            .any(|gameplay| gameplay.definition_index == stat.definition_index);
        let already_authored = overrides
            .investment_stats
            .iter()
            .any(|value| value.definition_index == stat.definition_index);
        if !inherited && !already_authored {
            overrides.investment_stats.push(WeaponStatOverride {
                definition_index: stat.definition_index,
                value: stat.value,
            });
        }
    }
    overrides
        .investment_stats
        .sort_by_key(|value| value.definition_index);
}

#[derive(Clone, Copy)]
enum StatRowAction {
    RemoveAdded(u16),
    RemoveDonor(u16),
    RestoreDonor(u16),
}

const fn is_internal_weapon_stat(definition_index: u16) -> bool {
    matches!(definition_index, 0 | 1 | 13)
}

fn technical_recipe_features(recipe: &WeaponRecipe) -> Vec<String> {
    let overrides = &recipe.overrides;
    let mut features = Vec::new();
    for (active, label) in [
        (
            !recipe.runtime_component_donors.is_empty(),
            "Advanced: runtime component donors",
        ),
        (
            !overrides.runtime_values.is_empty(),
            "Advanced: edited runtime values",
        ),
        (
            overrides.base_sandbox_perks.is_some(),
            "Advanced: base weapon perks",
        ),
        (overrides.trait_indices.is_some(), "Advanced: trait indices"),
        (
            !overrides.runtime_resource_patches.is_empty(),
            "Advanced: runtime resource patches",
        ),
        (
            !overrides.additional_behaviors.is_empty(),
            "Additional behavior",
        ),
        (
            !overrides.raw_payload_patches.is_empty(),
            "Advanced: raw payload patches",
        ),
        (
            overrides.max_stack_size.is_some(),
            "Advanced: maximum stack size",
        ),
        (
            overrides.socket_entry_list_index.is_some(),
            "Advanced: socket entry list",
        ),
        (
            overrides.plug_category_hash.is_some(),
            "Advanced: plug category",
        ),
        (overrides.roll_set_index.is_some(), "Advanced: roll set"),
        (
            overrides.linked_plug_index.is_some(),
            "Advanced: linked plug",
        ),
        (
            overrides.power_cap_groups.is_some(),
            "Advanced: Power cap groups",
        ),
        (
            overrides.art_arrangements.is_some(),
            "Appearance: art arrangement rows",
        ),
        (
            overrides.render_dye_rows.is_some(),
            "Appearance: render dye rows",
        ),
        (
            overrides.subclass_abilities.is_some(),
            "Abilities: authored or from other subclasses",
        ),
    ] {
        if active {
            features.push(label.to_owned());
        }
    }
    for (index, column) in overrides
        .socket_columns
        .iter()
        .enumerate()
        .filter_map(|(index, column)| column.as_ref().map(|column| (index, column)))
    {
        // Socket role is already editable in the ordinary Weapon tab.
        let fields = [
            (!column.choice_weight_bits.is_empty(), "choice weights"),
            (
                !column.choice_conditions.is_empty(),
                "choice availability conditions",
            ),
            (
                column.reusable_plug_set_index.is_some(),
                "reusable plug set",
            ),
            (
                column.randomized_plug_set_index.is_some(),
                "randomized plug set",
            ),
            (
                !column.randomized_selection_program.is_empty(),
                "random choice count program",
            ),
        ]
        .into_iter()
        .filter_map(|(active, label)| active.then_some(label))
        .collect::<Vec<_>>();
        if !fields.is_empty() {
            features.push(format!("Socket {}: {}. Edit under Weapon → Perks & Sockets → Socket Options → Show Socket Details.", index + 1, fields.join(", ")));
        }
    }
    features
}

struct SocketPickerContext<'a> {
    catalog: &'a InvestmentCatalog,
    recipe_library: Option<&'a RecipeLibrary>,
    recipe: &'a mut WeaponRecipe,
    queries: &'a mut Vec<BTreeMap<usize, String>>,
    pages: &'a mut Vec<usize>,
    plug_selection_mode: &'a mut PlugSelectionMode,
    show_plug_safety_warnings: bool,
    show_experimental_options: bool,
    show_technical_rows: &'a mut bool,
    perk_request: &'a mut Option<crate::app::custom_perks::workbench::Request>,
    donor: &'a WeaponDonor,
    log: &'a mut ActivityLog,
}

/// The spacing and rule between two stacked workbench sections.
fn draw_stacked_section_break(ui: &mut egui::Ui) {
    ui.add_space(3.0);
    ui.separator();
    ui.add_space(3.0);
}

/// Gameplay and Appearance sit side by side when there is room, and stack when there is not.
fn donor_section_column_count(available_width: f32) -> usize {
    if available_width >= 780.0 { 2 } else { 1 }
}

fn core_profile_column_count(available_width: f32) -> usize {
    if available_width >= 1_080.0 {
        5
    } else if available_width >= 720.0 {
        3
    } else if available_width >= 560.0 {
        2
    } else {
        1
    }
}

struct SocketTechnicalFields<'a> {
    catalog: &'a InvestmentCatalog,
    recipe: &'a mut WeaponRecipe,
    donor: &'a WeaponDonor,
    socket_index: usize,
    is_added: bool,
    inherited: &'a [u32],
    page: &'a mut usize,
    queries: &'a mut BTreeMap<usize, String>,
    scroll_to_header: bool,
}

fn path_row(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) -> bool {
    let mut changed = false;
    ui.strong(label).on_hover_text(hint);
    ui.horizontal(|ui| {
        let field_width = (ui.available_width() - 86.0).max(160.0);
        changed |= ui
            .add_sized(
                [field_width, ui.spacing().interact_size.y],
                egui::TextEdit::singleline(value),
            )
            .on_hover_text(hint)
            .changed();
        if ui.button("Browse…").clicked()
            && let Some(folder) = rfd::FileDialog::new().pick_folder()
        {
            *value = folder.display().to_string();
            changed = true;
        }
    });
    changed
}

struct IdentityField {
    label: String,
    value: String,
    copyable: bool,
    help: Option<&'static str>,
}

impl IdentityField {
    fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            copyable: true,
            help: None,
        }
    }

    fn assigned_during_build(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: "Assigned during build".into(),
            copyable: false,
            help: None,
        }
    }
}

fn draw_identity_group<'a>(
    ui: &mut egui::Ui,
    title: &str,
    fields: impl Iterator<Item = &'a IdentityField>,
) {
    ui.strong(title);
    egui::Grid::new(title)
        .striped(true)
        .spacing([12.0, 5.0])
        .show(ui, |ui| {
            for field in fields {
                ui.horizontal(|ui| {
                    ui.label(&field.label);
                    if let Some(help) = field.help {
                        draw_authoring_info_icon(ui, help);
                    }
                });
                // A value still to come reads as a quiet note, with nothing to copy yet.
                if field.copyable {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&field.value).monospace())
                            .selectable(true),
                    );
                    if ui.button("Copy").clicked() {
                        ui.ctx().copy_text(field.value.clone());
                    }
                } else {
                    ui.weak(&field.value);
                    ui.label("");
                }
                ui.end_row();
            }
        });
}

fn runtime_component_control(binding_hash: u32) -> Option<RuntimeComponentControl> {
    PRIMARY_RUNTIME_COMPONENTS
        .into_iter()
        .chain(ADDITIONAL_RUNTIME_COMPONENTS)
        .find(|control| control.binding_hash == binding_hash)
}

fn draw_donor_section_label(ui: &mut egui::Ui, label: &str, tooltip: Option<&str>) {
    draw_donor_section_label_with_warning(ui, label, tooltip, None);
}

fn draw_donor_section_label_with_warning(
    ui: &mut egui::Ui,
    label: &str,
    tooltip: Option<&str>,
    warning: Option<&str>,
) {
    ui.horizontal(|ui| {
        ui.strong(label);
        if let Some(tooltip) = tooltip {
            draw_authoring_info_icon(ui, tooltip);
        }
        if let Some(warning) = warning {
            draw_authoring_warning_icon(ui, warning);
        }
    });
}

fn sandbox_perk_choice_label(perk: u16, choices: &[WeaponSandboxPerkChoice]) -> String {
    let known = match perk {
        449 => Some("Arc damage"),
        450 => Some("Solar damage"),
        451 => Some("Void damage"),
        _ => None,
    };
    if let Some(label) = known {
        return format!("{perk} · {label}");
    }
    choices
        .iter()
        .find(|choice| choice.perk_index == perk)
        .map_or_else(
            || format!("Effect {perk} · name unavailable"),
            |choice| {
                format!(
                    "{perk} · {} · {}",
                    choice.representative_name, choice.representative_type_name
                )
            },
        )
}

fn trait_choice_label(trait_index: u16, choices: &[WeaponTraitChoice]) -> String {
    choices
        .iter()
        .find(|choice| choice.trait_index == trait_index)
        .map_or_else(
            || format!("{trait_index} · not installed"),
            |choice| {
                let name = choice.name.trim();
                if name.is_empty() {
                    format!("{trait_index} · 0x{:08X}", choice.hash)
                } else {
                    format!("{trait_index} · {name} · 0x{:08X}", choice.hash)
                }
            },
        )
}

const ACTIVITY_LOG_CAPACITY: usize = 30;

#[derive(Default)]
struct ActivityLog {
    entries: VecDeque<LogEntry>,
    notice: Option<String>,
    file: sundial::activity_log::FileLog,
}

impl ActivityLog {
    fn enable_file(&mut self) {
        self.file.enable(sundial::activity_log::Product::Parhelion);
        for entry in &self.entries {
            self.file.append(entry);
        }
    }

    fn new(entry: LogEntry) -> Self {
        let notice = entry.error.then(|| entry.text.clone());
        Self {
            entries: VecDeque::from([entry]),
            notice,
            file: Default::default(),
        }
    }

    fn push(&mut self, entry: LogEntry) {
        self.file.append(&entry);
        if entry.error {
            self.notice = Some(entry.text.clone());
        }
        if self.entries.len() >= ACTIVITY_LOG_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    fn iter(&self) -> impl DoubleEndedIterator<Item = &LogEntry> {
        self.entries.iter()
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

mod library_view;
mod style;
#[cfg(test)]
mod tests;
use style::named_control;
pub(crate) use style::{transparency_backdrop, workbench_style};
mod ability_names;
mod build_check;
mod build_status;
mod collections_view;
mod custom_perks;
mod document;
mod donor_view;
mod editor_view;
mod emblem_view;
mod gear_view;
mod installed;
mod jobs;
mod preferences_view;
mod runtime_dependencies;
mod runtime_donors;
mod runtime_view;
mod shader_view;
mod socket_editor;
mod subclass_view;
mod technical_build;
use custom_perks::*;
mod uninstall_view;
use socket_editor::*;
mod stat_editor;
use stat_editor::*;
mod profile_controls;
use profile_controls::*;
mod runtime_fields;
use runtime_fields::*;

mod reports;
use reports::*;
