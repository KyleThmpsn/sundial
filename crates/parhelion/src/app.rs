mod account_resync;
mod activity_log;
mod events;
mod fields;
mod icon_preview;
#[cfg(feature = "d2-model-importer")]
mod importer;
pub(crate) mod pickers;
mod runtime_components;
mod shell;
mod technical_features;
mod ui_state;
mod window;
use activity_log::*;
use events::*;
use fields::*;
use icon_preview::*;
use runtime_components::*;
use technical_features::*;
pub use window::Parhelion;
use window::*;

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
    draw_catalog_loading_view, draw_plug_safety_warning,
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
    technical_build_open: bool,
    package_backup_retention: usize,
    backup_recipe_snapshots: bool,
    show_preview_fps: bool,
    play_preview_animations: bool,
    preferences_open: bool,
    preferences_error: Option<String>,
    perk_workbench: custom_perks::workbench::Workbench,
    preferences_page: preferences_view::PreferencesPage,
    activity_log_open: bool,
    build_dialog_step: BuildDialogStep,
    install_receiver: Option<Receiver<Result<InstallReport, String>>>,
    latest_install: Option<Result<InstallReport, String>>,
    account_resync_receiver: Option<Receiver<Result<AccountResyncReport, String>>>,
    account_resync: account_resync::Status,
    replacement_review: Option<Result<crate::install::ReplacementReview, String>>,
    replacement_receiver: Option<Receiver<Result<crate::install::ReplacementReview, String>>>,
    replacement_status: build_status::InstallStatus,
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
    authored_icon_layers: Option<(AuthoredIconPreviewKey, ReadIconLayers)>,
    /// The layers a worker is reading, and the preview they are read for.
    authored_icon_loading: Option<(AuthoredIconPreviewKey, Receiver<ReadIconLayers>)>,
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
    /// The projectile the open weapon fires, and its cards on the Gameplay tab.
    fired_projectile: runtime_view::FiredProjectile,
    barrel_controls: runtime_view::BarrelControls,
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
            technical_build_open: false,
            package_backup_retention: backup_preferences.package_backup_retention,
            backup_recipe_snapshots: backup_preferences.backup_recipe_snapshots,
            show_preview_fps: backup_preferences.show_preview_fps,
            play_preview_animations: backup_preferences.play_preview_animations,
            preferences_open: false,
            preferences_error: None,
            perk_workbench: custom_perks::workbench::Workbench::default(),
            preferences_page: preferences_view::PreferencesPage::default(),
            activity_log_open: false,
            build_dialog_step: BuildDialogStep::Build,
            install_receiver: None,
            latest_install: None,
            account_resync_receiver: None,
            account_resync: account_resync::Status::default(),
            replacement_review: None,
            replacement_receiver: None,
            replacement_status: build_status::InstallStatus::default(),
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
            authored_icon_loading: None,
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
            fired_projectile: runtime_view::FiredProjectile::default(),
            barrel_controls: runtime_view::BarrelControls::default(),
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
    fn update_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        self.apply_preview_preferences(ctx);
        self.poll_build();
        self.poll_build_check();
        self.poll_install();
        self.poll_account_resync();
        self.draw_account_resync_window(ctx);
        self.poll_replacement_review();
        self.poll_uninstall();
        self.refresh_installed(ctx);
        self.poll_runtime_donors();
        self.poll_technical_markers();
        self.lenders.poll();
        self.marker_editor.poll();
        self.runtime_dependencies.poll();
        self.barrel_controls.poll_finished();
        if self.uninstall.open {
            egui::CentralPanel::default().show(ui, |_| {});
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
            self.draw_catalog_loading_screen(ui);
            self.draw_activity_log_window(ctx);
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        self.ensure_runtime_graph(ctx);
        egui::Panel::bottom("parhelion_build_actions")
            .resizable(false)
            .show_separator_line(true)
            .show(ui, |ui| {
                ui.scope(|ui| {
                    workbench_style(ui);
                    ui.set_max_width(safe_content_width(ui.available_width()));
                    ui.add_space(5.0);
                    self.draw_action_error(ui);
                    self.draw_actions(ui);
                    ui.add_space(5.0);
                });
            });
        egui::CentralPanel::default().show(ui, |ui| {
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
                .scroll_source(egui::scroll_area::ScrollSource {
                    drag: egui::scroll_area::DragScroll::Never,
                    ..Default::default()
                })
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

    fn draw_catalog_loading_screen(&mut self, ui: &mut egui::Ui) {
        let logo = self
            .logo
            .get_or_insert_with(|| load_parhelion_logo_texture(ui.ctx()))
            .clone();
        let progress = self.catalog_progress.unwrap_or(CatalogLoadProgress {
            message: "Loading weapon catalog…",
            completed: 0,
            total: 0,
        });
        draw_catalog_loading_view(
            ui,
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
            || self.barrel_controls.busy()
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
        self.barrel_controls.invalidate();
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
        self.authored_icon_loading = None;
        self.runtime_graph = None;
        self.runtime_rig_appearance = None;
        self.runtime_graph_error = None;
        self.runtime_graph_target = None;
        self.runtime_value_text.clear();
    }

    fn clear_dependent_picker_queries(&mut self) {
        self.barrel_controls.invalidate();
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
        self.emblem_page.reset();
        self.render_gear_donor_query.clear();
        self.icon_editor = None;
        self.authored_icon_preview = None;
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

    fn save_workbench_preferences(&mut self) {
        let preferences = ParhelionPreferences {
            limit_package_backups: self.limit_package_backups,
            package_backup_retention: self.package_backup_retention,
            backup_recipe_snapshots: self.backup_recipe_snapshots,
            show_preview_fps: self.show_preview_fps,
            play_preview_animations: self.play_preview_animations,
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
#[cfg(test)]
pub(crate) use custom_perks::workbench::tests::capture;
#[cfg(test)]
pub(crate) use custom_perks::workbench::{WeaponProperties, WeaponPropertyPart, weapon_properties};
mod document;
mod donor_view;
mod editor_view;
mod emblem_view;
mod gear_view;
mod image_files;
mod installed;
mod jobs;
mod mod_view;
mod preferences_view;
mod runtime_dependencies;
mod runtime_donors;
mod runtime_view;
mod shader_view;
mod socket_editor;
mod subclass_view;
mod technical_build;
use custom_perks::*;
#[cfg(test)]
pub(crate) use technical_build::technical_build_report;
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
