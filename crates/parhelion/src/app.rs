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
    AdvancedGameplayPage, BuildDialogStep, RecipeSaveStatus, RuntimeEditorLayout, WorkbenchPage,
    runtime_workspace_donor_width, safe_content_width, workbench_left_column_width,
};

use sundial::activity_log::Entry as LogEntry;
use sundial::investment::{
    CatalogLoadProgress, CatalogLoadingView, InvestmentCatalog, PlugChoicePickerButton,
    PlugSelectionMode, WeaponAmmoType, WeaponDamageProfile, WeaponDonor, WeaponDonorPickerAction,
    WeaponDonorPickerClearChoice, WeaponDonorPickerOptions, WeaponDonorSummary,
    WeaponInventorySlot, WeaponInvestmentStat, WeaponRarity, WeaponSandboxPerkChoice,
    WeaponTraitChoice, authored_socket_choice_limit, authoring_socket_label_width,
    draw_authoring_info_icon, draw_authoring_toolbar, draw_catalog_loading_view,
    draw_plug_safety_selector, draw_plug_safety_warning,
};
use sundial::package_authoring::{
    PackageAuthoringPreferences, PackageAuthoringUpdate, PackageAuthoringUtility, open_directory,
    open_shadowkeep_package_manager, resolve_live_named_tag,
    sandbox_perk::load_sandbox_perk_runtime_action,
    weapon_entity::{
        WEAPON_BARREL_COMPONENT_KEY, WEAPON_CONTROLLER_COMPONENT_KEY, WEAPON_INPUT_COMPONENT_KEY,
        WEAPON_MAGAZINE_COMPONENT_KEY, WEAPON_RELOAD_COMPONENT_KEY,
        WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, WEAPON_TRIGGER_CHARGE_COMPONENT_KEY,
        WEAPON_TRIGGER_COMPONENT_KEY,
    },
    weapon_runtime::{
        WeaponRuntimeField, WeaponRuntimeFieldLocator, WeaponRuntimeFieldSource,
        WeaponRuntimeGraph, WeaponRuntimeValue, WeaponRuntimeValueKind, WeaponRuntimeValueOverride,
        encode_weapon_runtime_value, load_weapon_runtime_graph_for_entity,
    },
};
use tiger_pkg::TagHash;

use crate::capabilities::{AuthoringDiagnosticCode, AuthoringField};
use crate::icon_edit::{WeaponIconEditor, WeaponIconEditorAction, render_weapon_icon_preview};
#[cfg(test)]
use crate::install::CANONICAL_ARTIFACT_FILE_NAMES;
use crate::install::{
    InstallReport, InstallRequest, MAX_PACKAGE_BACKUP_RETENTION, install_staged_packages,
};
use crate::preferences::ParhelionPreferences;
use crate::runtime::{RuntimeGraphKey, load_effective_runtime_graph};
use crate::workflow::{
    BatchBuildRequest, BatchBuildSnapshot, BuildPhase, BuildProgress, BuildReport,
    build_and_stage_snapshot_with_progress, default_backup_root, default_staging_root,
};
use crate::{
    CombatProfileAction, HexHash, RecipeAmmoType, RecipeLibrary, RecipeLibraryEntry, RecipeRarity,
    RecipeRawPayloadTarget, SupportedPlugSet, WeaponArtArrangementRecipe, WeaponCloneIdentity,
    WeaponDonorReference, WeaponDyeReferenceRecipe, WeaponLocaleTextRecipe,
    WeaponNumericInstructionRecipe, WeaponRawPayloadPatchRecipe, WeaponRecipe,
    WeaponRecipeOverrides, WeaponRuntimeResourcePatchRecipe, WeaponSandboxPerkRuntimeRecipe,
    WeaponSocketColumnRecipe, WeaponSocketPlugVariantRecipe, WeaponStatOverride,
    apply_combat_profile_action, authored_inventory_slot,
    presentation_donor_candidate_is_compatible, recipe_combat_profile_action,
    reconcile_presentation_donor, selected_presentation_donor_is_compatible,
    validate_socket_column_overrides_with_socket_types, weapon_authoring_capabilities,
};

const WINDOW_TITLE: &str = "Parhelion";
const DISPLAY_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

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
        tooltip: "The native trigger binding used by the runtime weapon entity. This can change firing cadence and trigger behavior without changing the item stats or appearance.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_BARREL_COMPONENT_KEY,
        label: "Barrel Runtime",
        tooltip: "The native barrel binding. It is one part of projectile emission, but projectile speed and family-specific translation can live elsewhere in the runtime graph.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_MAGAZINE_COMPONENT_KEY,
        label: "Magazine Behavior",
        tooltip: "The native magazine binding controls magazine and reserve behavior. Choose Primary, Special, or Heavy with Ammo type on the Weapon tab; that setting writes the native weapon-content ammo override as well as display metadata.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_RELOAD_COMPONENT_KEY,
        label: "Reload Behavior",
        tooltip: "Replaces the complete reload component, not just an animation. Compatibility also depends on the weapon's other runtime components.",
    },
];

const ADDITIONAL_RUNTIME_COMPONENTS: [RuntimeComponentControl; 4] = [
    RuntimeComponentControl {
        binding_hash: WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
        label: "Weapon stat translator",
        tooltip: "A coupled, family-specific runtime translator, not a projectile-speed control. Cross-family replacements can freeze the game even when package validation succeeds. Preserve the weapon's translator for private projectile edits.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_CONTROLLER_COMPONENT_KEY,
        label: "Weapon controller",
        tooltip: "The native weapon binding. It is broader than trigger or barrel and may affect several runtime behaviors at once.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_INPUT_COMPONENT_KEY,
        label: "Input",
        tooltip: "The native input binding consumed by the weapon entity.",
    },
    RuntimeComponentControl {
        binding_hash: WEAPON_TRIGGER_CHARGE_COMPONENT_KEY,
        label: "Trigger charge",
        tooltip: "The optional native trigger-charge binding. Both the gameplay donor and selected component donor must define it.",
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
        result: Result<BuildReport, String>,
        elapsed: Duration,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PendingRecipeAction {
    Close,
    New,
    Open(PathBuf),
    Import,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AuthoredIconPreviewKey {
    item_hash: u32,
    container_tag: u32,
    rarity: crate::AuthoredWeaponRarity,
    edit: crate::WeaponIconEdit,
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

enum AuthoredIconPreview {
    Ready {
        key: AuthoredIconPreviewKey,
        texture: egui::TextureHandle,
    },
    Failed {
        key: AuthoredIconPreviewKey,
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
}

impl Default for Parhelion {
    fn default() -> Self {
        let image = image::load_from_memory(include_bytes!("../../../assets/sundial-alt.png"))
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
        let close_now = context.show_viewport_immediate(
            egui::ViewportId::from_hash_of("parhelion"),
            egui::ViewportBuilder::default()
                .with_title(WINDOW_TITLE)
                .with_app_id("io.github.kylethmpsn.Sundial.Parhelion")
                .with_inner_size([1_320.0, 900.0])
                .with_min_inner_size([900.0, 640.0])
                .with_icon(self.icon.clone()),
            |viewport_context, _class| {
                if focus_requested {
                    viewport_context.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                self.app.update_ui(viewport_context);
                let close_requested =
                    viewport_context.input(|input| input.viewport().close_requested());
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
            self.app.release_package_access();
        }
        PackageAuthoringUpdate {
            open: self.open,
            busy: self.app.has_background_work(),
            dirty: self.app.recipe_dirty,
            packages_changed: self.app.take_packages_changed(),
            preferences_changed: self.app.take_preferences_changed(),
        }
    }
}

struct PackageAuthoringApp {
    recipe: WeaponRecipe,
    observed_recipe: WeaponRecipe,
    recipe_baseline: WeaponRecipe,
    recipe_path: Option<PathBuf>,
    recipe_dirty: bool,
    recipe_requires_initial_save: bool,
    library_open: bool,
    library_query: String,
    restore_defaults_preview: Option<crate::recipe_library::RestoreDefaults>,
    invalid_weapon_name: Option<(String, String)>,
    recipe_search_focus_pending: bool,
    build_selection_error: Option<String>,
    build_selection_draft: Option<BTreeSet<PathBuf>>,
    build_selection_query: String,
    advanced_gameplay_page: AdvancedGameplayPage,
    recipe_library: Option<RecipeLibrary>,
    recipe_entries: Vec<RecipeLibraryEntry>,
    enabled_recipe_paths: BTreeSet<PathBuf>,
    packages: PathBuf,
    staging: String,
    ignore_installed: bool,
    build_receiver: Option<Receiver<BuildWorkerEvent>>,
    build_progress: Option<TimedBuildProgress>,
    build_started: Option<Instant>,
    latest_build: Option<Result<BuildReport, String>>,
    build_status_open: bool,
    backup_root: String,
    limit_package_backups: bool,
    package_backup_retention: usize,
    backup_recipe_snapshots: bool,
    preferences_open: bool,
    preferences_page: preferences_view::PreferencesPage,
    activity_log_open: bool,
    build_dialog_step: BuildDialogStep,
    install_receiver: Option<Receiver<Result<InstallReport, String>>>,
    latest_install: Option<Result<InstallReport, String>>,
    replacement_review: Option<Result<crate::install::ReplacementReview, String>>,
    replacement_receiver: Option<Receiver<Result<crate::install::ReplacementReview, String>>>,
    uninstall: uninstall_view::UninstallUi,
    catalog: Option<InvestmentCatalog>,
    catalog_receiver: Option<Receiver<CatalogEvent>>,
    catalog_worker: Option<thread::JoinHandle<()>>,
    catalog_progress: Option<CatalogLoadProgress>,
    catalog_load_requested: bool,
    catalog_reload_pending: bool,
    catalog_force_rebuild: bool,
    donor_summaries: Vec<WeaponDonorSummary>,
    sandbox_perk_choices: Vec<WeaponSandboxPerkChoice>,
    trait_choices: Vec<WeaponTraitChoice>,
    donor_query: String,
    presentation_donor_query: String,
    icon_donor_query: String,
    render_gear_donor_query: String,
    runtime_component_queries: BTreeMap<u32, String>,
    runtime_binding_filter: String,
    runtime_bindings_open: bool,
    runtime_donors: runtime_donors::Browser,
    runtime_graph: Option<(RuntimeGraphKey, Arc<WeaponRuntimeGraph>)>,
    runtime_graph_error: Option<(RuntimeGraphKey, String)>,
    runtime_graph_job: Option<jobs::RuntimeGraphJob>,
    runtime_graph_target: Option<RuntimeGraphKey>,
    runtime_value_query: String,
    runtime_value_text: BTreeMap<(WeaponRuntimeFieldLocator, u8), String>,
    show_technical_runtime_values: bool,
    weapon_pattern_query: String,
    stat_group_query: String,
    plug_queries: Vec<BTreeMap<usize, String>>,
    socket_choice_pages: Vec<usize>,
    plug_selection_mode: PlugSelectionMode,
    show_plug_safety_warnings: bool,
    show_experimental_options: bool,
    preferences_changed: bool,
    show_internal_stats: bool,
    show_technical_socket_rows: bool,
    perk_editor: Option<PerkEditor>,
    private_perk_socket: Option<usize>,
    custom_perk_reuse: Option<custom_perks::ReusePicker>,
    icon_editor: Option<WeaponIconEditor>,
    hud_icon_editor: crate::hud_icon::ui::Editor,
    authored_icon_preview: Option<AuthoredIconPreview>,
    library_icons: library_view::LibraryIcons,
    dye_colors: donor_view::DyeColors,
    pending_recipe_action: Option<PendingRecipeAction>,
    scroll_recipe_to_top: bool,
    workbench_page: WorkbenchPage,
    close_approved: bool,
    log: ActivityLog,
    packages_changed: bool,
    logo: Option<egui::TextureHandle>,
}

impl Default for PackageAuthoringApp {
    fn default() -> Self {
        let recipe = WeaponRecipe::new_unbound("New Recipe")
            .expect("the built-in New Recipe identity must remain valid");
        let recipe_path = None;
        let recipe_entries = Vec::new();
        let enabled_recipe_paths = BTreeSet::new();
        let log = ActivityLog::new(LogEntry::info(
            "Ready. Source packages remain read-only; output is written to a fresh staging run.",
        ));
        let backup_preferences = ParhelionPreferences::default();
        let recipe_library = None;
        let observed_recipe = recipe.clone();
        Self {
            recipe_baseline: recipe.clone(),
            recipe,
            observed_recipe,
            recipe_path,
            recipe_dirty: false,
            recipe_requires_initial_save: false,
            library_open: false,
            library_query: String::new(),
            restore_defaults_preview: None,
            invalid_weapon_name: None,
            recipe_search_focus_pending: false,
            build_selection_error: None,
            build_selection_draft: None,
            build_selection_query: String::new(),
            advanced_gameplay_page: AdvancedGameplayPage::default(),
            recipe_library,
            recipe_entries,
            enabled_recipe_paths,
            packages: PathBuf::new(),
            staging: default_staging_root().display().to_string(),
            ignore_installed: true,
            build_receiver: None,
            build_progress: None,
            build_started: None,
            latest_build: None,
            build_status_open: false,
            backup_root: default_backup_root().display().to_string(),
            limit_package_backups: backup_preferences.limit_package_backups,
            package_backup_retention: backup_preferences.package_backup_retention,
            backup_recipe_snapshots: backup_preferences.backup_recipe_snapshots,
            preferences_open: false,
            preferences_page: preferences_view::PreferencesPage::default(),
            activity_log_open: false,
            build_dialog_step: BuildDialogStep::Build,
            install_receiver: None,
            latest_install: None,
            replacement_review: None,
            replacement_receiver: None,
            uninstall: uninstall_view::UninstallUi::default(),
            catalog: None,
            catalog_receiver: None,
            catalog_worker: None,
            catalog_progress: None,
            catalog_load_requested: false,
            catalog_reload_pending: false,
            catalog_force_rebuild: false,
            donor_summaries: Vec::new(),
            sandbox_perk_choices: Vec::new(),
            trait_choices: Vec::new(),
            donor_query: String::new(),
            presentation_donor_query: String::new(),
            icon_donor_query: String::new(),
            render_gear_donor_query: String::new(),
            runtime_component_queries: BTreeMap::new(),
            runtime_binding_filter: String::new(),
            runtime_bindings_open: false,
            runtime_donors: runtime_donors::Browser::default(),
            runtime_graph: None,
            runtime_graph_error: None,
            runtime_graph_job: None,
            runtime_graph_target: None,
            runtime_value_query: String::new(),
            runtime_value_text: BTreeMap::new(),
            show_technical_runtime_values: false,
            weapon_pattern_query: String::new(),
            stat_group_query: String::new(),
            plug_queries: Vec::new(),
            socket_choice_pages: Vec::new(),
            plug_selection_mode: sundial::investment::default_plug_selection_mode(),
            show_plug_safety_warnings: sundial::investment::show_plug_safety_warnings(),
            show_experimental_options: false,
            preferences_changed: false,
            show_internal_stats: false,
            show_technical_socket_rows: false,
            perk_editor: None,
            private_perk_socket: None,
            custom_perk_reuse: None,
            icon_editor: None,
            hud_icon_editor: crate::hud_icon::ui::Editor::default(),
            authored_icon_preview: None,
            library_icons: library_view::LibraryIcons::default(),
            dye_colors: donor_view::DyeColors::default(),
            pending_recipe_action: None,
            scroll_recipe_to_top: true,
            workbench_page: WorkbenchPage::default(),
            close_approved: false,
            log,
            packages_changed: false,
            logo: None,
        }
    }
}

impl PackageAuthoringApp {
    fn update_ui(&mut self, ctx: &egui::Context) {
        self.poll_build();
        self.poll_install();
        self.poll_uninstall();
        self.poll_runtime_donors();
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
                || self.install_receiver.is_some()
                || self.perk_editor.is_some();
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
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible);
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
        self.draw_build_status_window(ctx);
        // Both views share one window ID and must never render in the same frame.
        if self.perk_editor.is_some() {
            self.draw_perk_editor(ctx);
        } else {
            self.draw_custom_perks_window(ctx);
        }
        self.draw_runtime_bindings_window(ctx);
        self.draw_runtime_donor_browser(ctx);
        self.draw_icon_editor(ctx);
        self.draw_preferences_window(ctx);
        self.draw_activity_log_window(ctx);
        self.draw_discard_confirmation(ctx);
        self.draw_library_windows(ctx);
        self.synchronize_recipe_dirty();
        if self.build_receiver.is_some()
            || self.install_receiver.is_some()
            || self.catalog_receiver.is_some()
            || self.runtime_graph_job.is_some()
            || self.runtime_donors.busy()
            || self
                .perk_editor
                .as_ref()
                .is_some_and(PerkEditor::is_loading)
        {
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
            message: "Loading Sundial weapon catalog…",
            completed: 0,
            total: 0,
        });
        draw_catalog_loading_view(
            ctx,
            &logo,
            CatalogLoadingView {
                product_name: WINDOW_TITLE,
                version: DISPLAY_VERSION,
                message: progress.message,
                completed: progress.completed,
                total: progress.total,
                source_path: self.packages.parent().or(Some(self.packages.as_path())),
            },
        );
    }

    fn has_background_work(&self) -> bool {
        self.uninstall.busy()
            || self.build_receiver.is_some()
            || self.install_receiver.is_some()
            || self.catalog_is_loading()
            || self.runtime_graph_job.is_some()
            || self.runtime_donors.busy()
            || self
                .perk_editor
                .as_ref()
                .is_some_and(PerkEditor::has_background_work)
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
        self.hud_icon_editor = crate::hud_icon::ui::Editor::default();
        self.library_icons = library_view::LibraryIcons::default();
        self.dye_colors = donor_view::DyeColors::default();
        self.catalog = None;
        self.donor_summaries.clear();
        self.sandbox_perk_choices.clear();
        self.trait_choices.clear();
        self.plug_queries.clear();
        self.socket_choice_pages.clear();
        self.perk_editor = None;
        self.icon_editor = None;
        self.authored_icon_preview = None;
        self.runtime_graph = None;
        self.runtime_graph_error = None;
        self.runtime_graph_target = None;
        self.runtime_value_text.clear();
    }

    fn clear_dependent_picker_queries(&mut self) {
        self.runtime_donors.invalidate();
        self.invalid_weapon_name = None;
        self.clear_presentation_picker_queries();
        self.private_perk_socket = None;
        self.custom_perk_reuse = None;
        self.runtime_bindings_open = false;
        self.workbench_page = WorkbenchPage::Weapon;
        self.presentation_donor_query.clear();
        self.runtime_component_queries.clear();
        self.runtime_graph = None;
        self.runtime_graph_error = None;
        self.runtime_graph_target = None;
        self.runtime_value_text.clear();
        self.weapon_pattern_query.clear();
        self.stat_group_query.clear();
        self.plug_queries.clear();
        self.socket_choice_pages.clear();
        self.perk_editor = None;
    }

    fn clear_presentation_picker_queries(&mut self) {
        self.hud_icon_editor = crate::hud_icon::ui::Editor::default();
        self.icon_donor_query.clear();
        self.render_gear_donor_query.clear();
        self.icon_editor = None;
        self.authored_icon_preview = None;
    }

    fn draw_icon_editor(&mut self, ctx: &egui::Context) {
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

    fn authored_icon_preview(
        &mut self,
        context: &egui::Context,
        item_hash: u32,
        container_tag: TagHash,
    ) -> Result<Option<egui::TextureHandle>, String> {
        let rarity = self
            .authored_icon_rarity()
            .ok_or("Select an available gameplay donor to resolve the icon rarity")?;
        let edit = &self.recipe.overrides.icon_edit;
        let key = AuthoredIconPreviewKey {
            item_hash,
            container_tag: u32::from(container_tag),
            rarity,
            edit: edit.clone(),
        };
        let cached_matches = match self.authored_icon_preview.as_ref() {
            Some(AuthoredIconPreview::Ready { key: cached, .. })
            | Some(AuthoredIconPreview::Failed { key: cached, .. }) => *cached == key,
            None => false,
        };
        if !cached_matches {
            self.authored_icon_preview = Some(
                match render_weapon_icon_preview(&self.packages, container_tag, rarity, edit) {
                    Ok(image) => AuthoredIconPreview::Ready {
                        key,
                        texture: context.load_texture(
                            format!("parhelion-authored-icon-{item_hash:08X}"),
                            image,
                            egui::TextureOptions::LINEAR,
                        ),
                    },
                    Err(error) => AuthoredIconPreview::Failed { key, error },
                },
            );
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
            package_backup_retention: self.package_backup_retention,
            backup_recipe_snapshots: self.backup_recipe_snapshots,
            ..ParhelionPreferences::default()
        };
        match preferences.save_default() {
            Ok(path) => self.log.push(LogEntry::info(format!(
                "Saved backup preferences to {}",
                path.display()
            ))),
            Err(error) => self.log.push(LogEntry::error(error)),
        }
    }
}

impl PackageAuthoringApp {
    fn draw_workbench_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for page in WorkbenchPage::ALL {
                ui.selectable_value(&mut self.workbench_page, page, page.label());
            }
        });
    }

    fn draw_recipe_editor(&mut self, ui: &mut egui::Ui) {
        if !self.show_experimental_options {
            let hidden_features = technical_recipe_features(&self.recipe);
            if !hidden_features.is_empty() {
                egui::CollapsingHeader::new(format!(
                    "Additional Overrides ({})",
                    hidden_features.len()
                ))
                .id_salt("hidden_recipe_overrides")
                .show(ui, |ui| {
                    ui.label("These saved settings are included in the build.");
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
            WorkbenchPage::Weapon => self.draw_core_recipe_editor(ui),
            WorkbenchPage::Appearance => self.draw_appearance_workspace(ui),
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
                if ui.button("New Recipe").clicked() {
                    replaced |= self.request_recipe_action(PendingRecipeAction::New);
                }
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "The recipe library is unavailable; this recipe can still be edited for this session.",
                );
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
            if ui.button("Preferences…").clicked() {
                self.preferences_open = true;
            }
        });
        replaced
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
        if ui.button("New Recipe").clicked() {
            *replaced |= self.request_recipe_action(PendingRecipeAction::New);
        }
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
        if ui.button("Duplicate").on_hover_text("Create a new weapon identity from this draft, including its custom perks. Save the copy to keep it in your library.").clicked() {
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
            .on_hover_text("Restore the last saved or newly-created version of this recipe")
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
        let mut discard = false;
        let mut cancel = false;
        let response = egui::Modal::new("parhelion_discard_recipe".into()).show(ctx, |ui| {
            workbench_style(ui);
            ui.set_width(420.0);
            ui.heading("Discard unsaved recipe changes?");
            ui.add_space(6.0);
            ui.label(match &action {
                PendingRecipeAction::Close => {
                    "Closing Parhelion will discard this recipe's unsaved changes."
                }
                PendingRecipeAction::New => {
                    "Creating a new recipe will discard this recipe's unsaved changes."
                }
                PendingRecipeAction::Open(_) => {
                    "Opening another recipe will discard this recipe's unsaved changes."
                }
                PendingRecipeAction::Import => {
                    "Importing a recipe will discard this recipe's unsaved changes."
                }
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .button(match action {
                        PendingRecipeAction::Close => "Discard and close",
                        _ => "Discard and continue",
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
        if discard {
            self.pending_recipe_action = None;
            self.execute_recipe_action(action);
        } else if cancel {
            self.pending_recipe_action = None;
        }
    }

    fn draw_actions(&mut self, ui: &mut egui::Ui) {
        let building = self.build_receiver.is_some();
        let installing = self.install_receiver.is_some();
        let running = building || installing || self.perk_editor.is_some();
        let included_count = self.enabled_recipe_paths.len();
        let catalog_ready = self.catalog.is_some()
            && self.catalog_receiver.is_none()
            && self.catalog_worker.is_none()
            && !self.catalog_reload_pending;
        let project_ready = included_count > 0 && catalog_ready;
        ui.horizontal_wrapped(|ui| {
            let primary_fill = ui.visuals().selection.bg_fill;
            if ui
                .add_enabled(
                    !running,
                    egui::Button::new(format!(
                        "{included_count} {} selected for build…",
                        if included_count == 1 {
                            "weapon"
                        } else {
                            "weapons"
                        }
                    ))
                    .min_size([0.0, ui.spacing().interact_size.y].into()),
                )
                .clicked()
            {
                self.open_build_selection();
            }
            if ui
                .add_enabled(
                    !running && project_ready && self.build_selection_draft.is_none(),
                    egui::Button::new(egui::RichText::new("Build & Stage").strong())
                        .fill(primary_fill)
                        .min_size([160.0, ui.spacing().interact_size.y].into()),
                )
                .clicked()
            {
                self.start_build();
            }
            if (building || installing || self.latest_build.is_some())
                && ui.button("Build & Install Status…").clicked()
            {
                self.build_status_open = true;
            }
        });
        if !self.current_recipe_is_in_build() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(ui.visuals().warn_fg_color, if self.recipe_path.is_none() {
                    "This recipe is not saved or included in the build. Save it, then add it using the weapon selection button."
                } else {
                    "The open recipe is not included in this build. Only the selected weapons will be built."
                });
            });
        } else if self.recipe_dirty {
            ui.label("Build & Stage saves your current edits before compiling.");
        }
        if included_count == 0 {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Include at least one library recipe before building.",
            );
        } else if !catalog_ready {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Finish loading the Sundial weapon catalog before building.",
            );
        }
    }

    fn draw_build_status_window(&mut self, ctx: &egui::Context) {
        if !self.build_status_open {
            return;
        }

        let building = self.build_receiver.is_some();
        let can_install = !building
            && matches!(self.latest_build, Some(Ok(_)))
            && self.install_receiver.is_none()
            && self.catalog_receiver.is_none()
            && self.catalog_worker.is_none()
            && !self.catalog_reload_pending;
        let staging_directory = self
            .latest_build
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(|report| report.run_directory.clone());
        let mut open = true;
        let mut close_requested = false;
        let mut open_staging_requested = false;
        let mut install_requested = false;

        egui::Window::new("Build & Install")
            .id(egui::Id::new("parhelion_build_status"))
            .collapsible(false)
            .resizable(true)
            .default_width(960.0)
            .default_height(460.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(620.0);
                workbench_style(ui);
                match self.build_dialog_step {
                    BuildDialogStep::ReviewInstall => {
                        self.draw_install_confirmation(ui);
                        return;
                    }
                    BuildDialogStep::Install => {
                        self.draw_install_status(ui);
                        return;
                    }
                    BuildDialogStep::Build => {}
                }
                if building {
                    ctx.request_repaint_after(Duration::from_millis(100));
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.heading(
                            self.build_progress
                                .as_ref()
                                .map_or("Starting package build", |progress| {
                                    progress.phase.label()
                                }),
                        );
                    });
                    if let Some(progress) = &self.build_progress {
                        let progress_text = format!(
                            "{} of {}",
                            progress.completed.min(progress.total),
                            progress.total
                        );
                        ui.add(
                            sundial::investment::progress_bar(progress.fraction())
                                .text(progress_text),
                        );
                        ui.horizontal(|ui| {
                            if let Some(artifact) = &progress.current_artifact {
                                ui.strong("Current");
                                ui.monospace(artifact);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(format!("Elapsed {}", format_elapsed(self.build_elapsed(Instant::now()))));
                                },
                            );
                        });
                    }
                    ui.add_space(6.0);
                    ui.label(format!(
                        "Weapons selected: {}. Packages will be staged for review before installation.",
                        self.enabled_recipe_paths.len()
                    ));
                } else {
                    if let Some(progress) = &self.build_progress {
                        ui.label(
                            egui::RichText::new(format!(
                                "Elapsed {}",
                                format_elapsed(progress.elapsed)
                            ))
                            .weak(),
                        );
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("parhelion-build-status-report")
                        .max_height((ui.available_height() - 70.0).max(120.0))
                        .auto_shrink([false, false])
                        .show(ui, |ui| match self.latest_build.as_ref() {
                            Some(Ok(report)) => draw_build_report(ui, report),
                            Some(Err(error)) => {
                                ui.heading(
                                    egui::RichText::new("Build blocked")
                                        .color(ui.visuals().error_fg_color),
                                );
                                ui.colored_label(ui.visuals().error_fg_color, error);
                            }
                            None => {
                                ui.label("No build result is available.");
                            }
                        });
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close")
                            .on_hover_text("Closing this window does not stop the build.")
                            .clicked() {
                            close_requested = true;
                        }
                        let mut install_button = egui::Button::new(
                            egui::RichText::new("Review Installation").strong(),
                        )
                        .min_size([180.0, ui.spacing().interact_size.y].into());
                        if can_install {
                            install_button = install_button.fill(ui.visuals().selection.bg_fill);
                        }
                        if ui.add_enabled(can_install, install_button).clicked() {
                            install_requested = true;
                        }
                        if ui
                            .add_enabled(
                                staging_directory.is_some(),
                                egui::Button::new("Open Staging Folder"),
                            )
                            .clicked()
                        {
                            open_staging_requested = true;
                        }
                    });
                });
            });

        if open_staging_requested
            && let Some(path) = staging_directory.as_deref()
            && let Err(error) = open_directory(path)
        {
            self.log.push(LogEntry::error(error));
        }
        if install_requested {
            self.start_replacement_review();
            self.build_dialog_step = BuildDialogStep::ReviewInstall;
        }
        if close_requested {
            open = false;
        }
        self.build_status_open &= open;
    }

    fn draw_install_confirmation(&mut self, ui: &mut egui::Ui) {
        self.poll_replacement_review();
        let Some(Ok(build)) = self.latest_build.as_ref() else {
            self.build_dialog_step = BuildDialogStep::Build;
            return;
        };
        ui.heading("Review Installation");
        egui::ScrollArea::vertical()
            .id_salt("install-review-contents")
            .max_height((ui.available_height() - 56.0).max(120.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.strong(format!("Install {} weapons from {} verified packages", build.weapons.len(), build.artifacts.len()));
                ui.label("This replaces your installed custom weapon set. Include every weapon you want to keep.");
                ui.colored_label(ui.visuals().warn_fg_color, "Close Destiny 2 before installing.");
                ui.add_space(8.0);
                self.draw_account_changes(ui);
                ui.add_space(8.0);
                egui::CollapsingHeader::new("Installation Details").show(ui, |ui| {
                    reports::path_row(ui, "Staged Run", &build.run_directory);
                    reports::path_row(ui, "Target Packages", &self.packages);
                    reports::path_row(ui, "Backup Folder", Path::new(self.backup_root.trim()));
                    ui.label(if self.limit_package_backups {
                        format!("Automatic package backups: keep newest {}", self.package_backup_retention)
                    } else {
                        "Automatic package backups: unlimited".to_owned()
                    });
                    ui.label(if self.backup_recipe_snapshots { "Recipe snapshots included." } else { "Recipe snapshots not included." });
                    ui.label("Other accounts sharing these packages are not changed.");
                    ui.add_space(5.0);
                    ui.strong(format!("Files to Install ({})", build.artifacts.len()));
                    for artifact in &build.artifacts { ui.monospace(&artifact.file_name); }
                });
            });
        ui.add_space(6.0);
        ui.separator();
        let mut install_requested = false;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if self
                .replacement_review
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .is_some_and(|r| r.removes_account_data())
            {
                "Back Up, Remove & Install"
            } else if self
                .replacement_review
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .is_some_and(|r| r.changes_account())
            {
                "Back Up, Update & Install"
            } else {
                "Back Up & Install"
            };
            install_requested = ui
                .add_enabled(
                    self.install_receiver.is_none()
                        && matches!(self.replacement_review, Some(Ok(_)))
                        && self.replacement_receiver.is_none()
                        && self.catalog_receiver.is_none()
                        && self.catalog_worker.is_none()
                        && !self.catalog_reload_pending,
                    egui::Button::new(label).fill(ui.visuals().selection.bg_fill),
                )
                .clicked();
            if ui.button("Cancel").clicked() {
                self.build_dialog_step = BuildDialogStep::Build;
            }
        });
        if install_requested {
            self.start_install();
        }
    }

    fn draw_account_changes(&self, ui: &mut egui::Ui) {
        if self.replacement_receiver.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Checking saved weapons and custom perks…");
            });
            ui.ctx().request_repaint_after(Duration::from_millis(100));
            return;
        }
        let review = match &self.replacement_review {
            Some(Ok(review)) => review,
            Some(Err(error)) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
                return;
            }
            None => return,
        };
        if !review.changes_account() {
            ui.label("No saved items, socket selections, or unlocks need changes. A package backup will be saved before installation.");
            return;
        }
        let Some(cleanup) = review.account_cleanup() else {
            return;
        };
        ui.strong("Account Changes");
        let count = cleanup.removed_items.values().sum::<usize>();
        if count > 0 {
            ui.label(format!(
                "Remove {count} saved {}:",
                if count == 1 { "item" } else { "items" }
            ));
            for (hash, count) in &cleanup.removed_items {
                let name = self
                    .catalog
                    .as_ref()
                    .map(|catalog| catalog.plug_label(*hash, false));
                ui.label(format!(
                    "{} × {count}",
                    name.unwrap_or_else(|| format!("Item 0x{hash:08X}"))
                ));
            }
            ui.label("Applies to every character, including equipped items. Removed equipment leaves empty slots.");
        }
        for (count, label) in [
            (cleanup.cleared_plugs, "custom plug references"),
            (cleanup.cleared_unlocks, "Collections unlocks"),
            (cleanup.removed_reward_rules, "reward rules"),
        ] {
            if count > 0 {
                ui.label(format!("Clear {count} {label}."));
            }
        }
        for change in review.socket_changes() {
            let Some(count) = cleanup.resized_items.get(&change.definition_hash) else {
                continue;
            };
            let name = self
                .catalog
                .as_ref()
                .map(|catalog| catalog.plug_label(change.definition_hash, false))
                .unwrap_or_else(|| format!("Item 0x{:08X}", change.definition_hash));
            ui.label(format!(
                "Update {count} saved {name} socket lists from {} to {} sockets.",
                change.previous_socket_count,
                change.default_plugs.len(),
            ));
            ui.label(if change.default_plugs.len() > change.previous_socket_count {
                "Existing socket selections are kept. Added sockets use the new definition's defaults."
            } else {
                "Selections in the remaining sockets are kept. Selections in removed sockets are deleted."
            });
        }
        reports::path_row(ui, "Account", &cleanup.settings_path);
        ui.label("Remaining weapons and unrelated progress are kept. The account and package backup is protected from automatic cleanup.");
    }

    fn draw_install_status(&mut self, ui: &mut egui::Ui) {
        if self.install_receiver.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.heading("Installing Packages");
            });
            ui.label("Verifying staged files, creating a backup, and installing the selected package set. Keep Destiny 2 closed.");
            ui.label("You can close this window. Installation will continue.");
            return;
        }
        let mut open_backup = None;
        egui::ScrollArea::vertical()
            .id_salt("install-result-contents")
            .max_height((ui.available_height() - 40.0).max(120.0))
            .auto_shrink([false, false])
            .show(ui, |ui| match &self.latest_install {
                Some(Ok(report)) => {
                    if draw_install_report(ui, report) {
                        open_backup = Some(report.backup_directory.clone());
                    }
                }
                Some(Err(error)) => {
                    ui.heading("Installation Blocked");
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                None => {
                    ui.label("No installation result is available.");
                }
            });
        if let Some(path) = open_backup
            && let Err(error) = open_directory(&path)
        {
            self.log.push(LogEntry::error(error));
        }
        ui.separator();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                self.build_status_open = false;
            }
            if matches!(self.latest_install, Some(Err(_)))
                && ui.button("Review Installation").clicked()
            {
                self.start_replacement_review();
                self.build_dialog_step = BuildDialogStep::ReviewInstall;
            }
        });
    }

    fn draw_action_error(&mut self, ui: &mut egui::Ui) {
        let diagnostic = self
            .latest_install
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .and_then(|report| report.profile_sync.as_ref())
            .and_then(|result| result.as_ref().err())
            .map(|error| ("Account sync incomplete", error.as_str()))
            .or_else(|| {
                self.latest_install
                    .as_ref()
                    .and_then(|report| report.as_ref().err())
                    .map(|error| ("Installation blocked", error.as_str()))
            })
            .or_else(|| {
                self.latest_build
                    .as_ref()
                    .and_then(|report| report.as_ref().err())
                    .map(|error| ("Build blocked", error.as_str()))
            })
            .or_else(|| {
                self.log
                    .notice
                    .as_deref()
                    .map(|error| ("Action needs attention", error))
            });
        let Some((title, error)) = diagnostic else {
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
                        egui::RichText::new(title)
                            .strong()
                            .color(ui.visuals().error_fg_color),
                    );
                    if title == "Action needs attention" {
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
        self.catalog
            .as_ref()?
            .weapon_donor_with_stat_group_index(hash, self.recipe.overrides.stat_group_index)
    }

    fn authored_icon_rarity(&self) -> Option<crate::AuthoredWeaponRarity> {
        let hash = self.recipe.donor.item_hash.parse_u32().ok();
        effective_icon_rarity(
            self.recipe.overrides.rarity,
            self.donor_summaries
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
            features.push(format!("Socket {}: {}. Edit under Weapon → Perks & Sockets → Socket Options → Show Native Rows.", index + 1, fields.join(", ")));
        }
    }
    features
}

fn runtime_field_is_editable(field: &WeaponRuntimeField) -> bool {
    field.locator.is_buildable()
        && encode_weapon_runtime_value(&field.kind, &field.value)
            .is_ok_and(|bytes| bytes.len() == field.locator.byte_size as usize)
}

fn runtime_field_is_in_editor_scope(
    source: WeaponRuntimeFieldSource,
    buildable: bool,
    customized: bool,
    show_experimental_options: bool,
    show_all_native_values: bool,
) -> bool {
    if customized {
        return true;
    }
    if !buildable {
        return false;
    }
    source != WeaponRuntimeFieldSource::OpaqueNativeType
        || (show_experimental_options && show_all_native_values)
}

fn private_perk_runtime_field_is_visible(
    field: &WeaponRuntimeField,
    query: &str,
    values: &[WeaponRuntimeValueOverride],
    show_all_native_values: bool,
    occurrences: usize,
) -> bool {
    let customized = values.iter().any(|value| value.locator == field.locator);
    if occurrences != 1 && !customized {
        return false;
    }
    if !runtime_field_is_in_editor_scope(
        field.source,
        runtime_field_is_editable(field),
        customized,
        true,
        show_all_native_values,
    ) {
        return false;
    }
    query.is_empty()
        || field.name.to_ascii_lowercase().contains(query)
        || field.path_label.to_ascii_lowercase().contains(query)
        || runtime_value_kind_label(&field.kind)
            .to_ascii_lowercase()
            .contains(query)
        || format!("0x{:08x}", field.locator.binding_hash).contains(query)
        || format!("0x{:08x}", field.locator.root_schema).contains(query)
        || format!("0x{:08x}", field.locator.type_handle).contains(query)
        || format!("0x{:x}", field.owner_offset).contains(query)
        || format!("0x{:x}", field.locator.value_offset).contains(query)
        || field.locator.path.iter().any(|element| {
            format!("0x{:08x}", element.name_hash).contains(query)
                || format!("0x{:08x}", element.type_handle).contains(query)
        })
}

struct SocketPickerContext<'a> {
    catalog: &'a InvestmentCatalog,
    recipe: &'a mut WeaponRecipe,
    queries: &'a mut Vec<BTreeMap<usize, String>>,
    pages: &'a mut Vec<usize>,
    plug_selection_mode: &'a mut PlugSelectionMode,
    show_plug_safety_warnings: bool,
    show_experimental_options: bool,
    show_technical_rows: &'a mut bool,
    private_perk_socket: &'a mut Option<usize>,
    donor: &'a WeaponDonor,
    log: &'a mut ActivityLog,
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
    ui.label(egui::RichText::new(label).strong())
        .on_hover_text(hint);
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

fn draw_identity_group<'a>(
    ui: &mut egui::Ui,
    title: &str,
    fields: impl Iterator<Item = &'a (&'a str, String, bool)>,
) {
    ui.strong(title);
    egui::Grid::new(title)
        .striped(true)
        .spacing([12.0, 5.0])
        .show(ui, |ui| {
            for (label, value, copyable) in fields {
                ui.label(*label);
                ui.add(egui::Label::new(egui::RichText::new(value).monospace()).selectable(true));
                if ui
                    .add_enabled(*copyable, egui::Button::new("Copy"))
                    .on_disabled_hover_text("Build to allocate the authored icon-definition hash.")
                    .clicked()
                {
                    ui.ctx().copy_text(value.to_owned());
                }
                ui.end_row();
            }
        });
}

fn runtime_field_tooltip(field: &WeaponRuntimeField) -> String {
    let source = match field.source {
        WeaponRuntimeFieldSource::GeneratedSchema => "generated package schema",
        WeaponRuntimeFieldSource::NativeMember => "named native member",
        WeaponRuntimeFieldSource::OpaqueNativeType => "unnamed native fixed-size type",
    };
    let path = field
        .locator
        .path
        .iter()
        .map(|element| {
            format!(
                "0x{:08X}:0x{:08X}@+0x{:X}",
                element.name_hash, element.type_handle, element.byte_offset
            )
        })
        .collect::<Vec<_>>()
        .join(" / ");
    let generated_kind = field
        .generated_kind
        .map_or_else(|| "N/A".to_owned(), |kind| format!("0x{kind:02X}"));
    format!(
        "{}\nSource: {source}\nValue Type: {}\nBinding: 0x{:08X}, resource index {} (zero-based)\nRoot: {} · schema 0x{:08X}\nType: 0x{:08X} · generated kind {generated_kind}\nRoot offset: 0x{:X} · resolved owner offset: 0x{:X} · {} bytes\nReflected path: {path}",
        field.name,
        runtime_value_kind_label(&field.kind),
        field.locator.binding_hash,
        field.locator.resource_index,
        field.locator.root.label(),
        field.locator.root_schema,
        field.locator.type_handle,
        field.locator.value_offset,
        field.owner_offset,
        field.locator.byte_size,
    )
}

fn parse_runtime_hex_u64(value: &str) -> Option<u64> {
    let digits = value
        .trim()
        .strip_prefix("0x")
        .or_else(|| value.trim().strip_prefix("0X"))
        .unwrap_or(value.trim())
        .replace('_', "");
    (!digits.is_empty())
        .then(|| u64::from_str_radix(&digits, 16).ok())
        .flatten()
}

fn format_runtime_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_runtime_hex_bytes(value: &str, expected_size: usize) -> Option<Vec<u8>> {
    let digits = value
        .chars()
        .filter(|character| !character.is_ascii_whitespace() && *character != '_')
        .collect::<String>();
    let digits = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
        .unwrap_or(&digits);
    if digits.len() != expected_size.checked_mul(2)?
        || !digits.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    (0..expected_size)
        .map(|index| u8::from_str_radix(&digits[index * 2..index * 2 + 2], 16).ok())
        .collect()
}

fn runtime_component_control(binding_hash: u32) -> Option<RuntimeComponentControl> {
    PRIMARY_RUNTIME_COMPONENTS
        .into_iter()
        .chain(ADDITIONAL_RUNTIME_COMPONENTS)
        .find(|control| control.binding_hash == binding_hash)
}

fn valid_hex_patch_text(value: &str) -> bool {
    let digits = value
        .chars()
        .filter(|character| !character.is_ascii_whitespace() && *character != '_')
        .collect::<String>();
    let digits = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
        .unwrap_or(&digits);
    !digits.is_empty()
        && digits.len() % 2 == 0
        && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn draw_donor_section_label(ui: &mut egui::Ui, label: &str, tooltip: Option<&str>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).strong());
        if let Some(tooltip) = tooltip {
            draw_authoring_info_icon(ui, tooltip);
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
            || format!("{trait_index} · not present in installed trait table"),
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
pub(crate) use style::workbench_style;
mod custom_perks;
mod document;
mod donor_view;
mod editor_view;
mod jobs;
mod preferences_view;
mod runtime_donors;
mod runtime_view;
mod socket_editor;
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
