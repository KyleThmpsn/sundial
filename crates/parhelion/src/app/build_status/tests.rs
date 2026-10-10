//! A recipe that stops a build is named, and Build Blocked offers to remove it from the build or
//! open it. Built with real packages and clicked in the real dialog. The ways this could still be
//! wrong, each checked here:
//!
//! - the failure names no recipe, or another recipe than the one that stopped the build
//! - Build Blocked offers no Remove from Build or Open Recipe
//! - Remove from Build removes another recipe, or the library does not keep the new selection
//! - the build still stops once that recipe is out of it
//! - Open Recipe opens another recipe than the one the build stopped on
use super::*;
use std::fmt::Write as _;
use std::sync::mpsc;

mod parallel;
mod resize;
mod review;

const BUILDS: (u32, &str) = (0xA25B_8F8F, "Arc Logic");
/// A Legendary whose game data carries no ammo type, so Collections refuses it without one.
const NO_AMMO: (u32, &str) = (0x90D4_2800, "Rose");

fn frame(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 800.0),
        )),
        events,
        ..Default::default()
    };
    ctx.run_ui(input, |ui| app.draw_build_status_window(ui))
}

/// Every button the dialog draws, with its center.
fn buttons(output: &egui::FullOutput) -> Vec<(String, egui::Pos2)> {
    let tree = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit tree");
    tree.nodes
        .iter()
        .filter(|(_, node)| node.role() == egui::accesskit::Role::Button)
        .filter_map(|(_, node)| {
            let bounds = node.bounds()?;
            let center = egui::pos2(
                ((bounds.x0 + bounds.x1) / 2.0) as f32,
                ((bounds.y0 + bounds.y1) / 2.0) as f32,
            );
            Some((node.label()?.to_owned(), center))
        })
        .collect()
}

/// The dialog's buttons once its layout has settled.
fn settled(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> Vec<(String, egui::Pos2)> {
    for _ in 0..3 {
        frame(ctx, app, Vec::new());
    }
    buttons(&frame(ctx, app, Vec::new()))
}

fn click(ctx: &egui::Context, app: &mut PackageAuthoringApp, label: &str) {
    let offered = settled(ctx, app);
    let at = offered
        .iter()
        .find(|(name, _)| name == label)
        .map(|(_, at)| *at)
        .unwrap_or_else(|| panic!("no {label} button among {offered:?}"));
    for events in crate::test_support::driver::tap(at) {
        frame(ctx, app, events);
    }
}

fn build(app: &PackageAuthoringApp) -> Result<BuildReport, BuildFailure> {
    let snapshot = BatchBuildSnapshot::new(app.batch_request()?)?;
    build_and_stage_snapshot_reporting(&snapshot, |_| {})
}

/// Hands a finished build to the app the way its worker does, with the dialog open.
fn finish(app: &mut PackageAuthoringApp, result: Result<BuildReport, BuildFailure>) {
    let (sender, receiver) = mpsc::channel();
    app.build_receiver = Some(receiver);
    sender
        .send(BuildWorkerEvent::Finished {
            result,
            elapsed: Duration::from_secs(1),
        })
        .unwrap();
    app.poll_build();
    app.build_status_open = true;
}

/// The two recipes in a library of their own, and an app building exactly them.
struct Setup {
    _directory: tempfile::TempDir,
    library: RecipeLibrary,
    app: PackageAuthoringApp,
    builds: PathBuf,
    blocking: PathBuf,
    namespace: String,
}

fn setup(packages: PathBuf) -> Setup {
    let directory = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(directory.path().join("recipes")).unwrap();
    let save = |name: &str, (hash, donor): (u32, &str)| {
        let recipe = WeaponRecipe::new_named_weapon_for_donor(name, hash, donor).unwrap();
        (library.save_new(&recipe).unwrap(), recipe.namespace)
    };
    let (builds, _) = save("Blocker Check Arc Logic", BUILDS);
    let (blocking, namespace) = save("Blocker Check Rose", NO_AMMO);
    let mut app = PackageAuthoringApp {
        recipe_entries: library.scan().unwrap().entries,
        recipe_library: Some(library.clone()),
        packages,
        staging: directory.path().join("staging").display().to_string(),
        ..Default::default()
    };
    // Startup discovers the bundled recipes before any selection is edited.
    app.enabled_recipe_paths = library.enabled_paths(&app.recipe_entries).unwrap();
    app.apply_build_selection([builds.clone(), blocking.clone()].into())
        .unwrap();
    Setup {
        _directory: directory,
        library,
        app,
        builds,
        blocking,
        namespace,
    }
}

/// Remove from Build leaves only the other recipe in the build, and the build then goes through.
fn remove(setup: &mut Setup, ctx: &egui::Context, report: &mut String) {
    click(ctx, &mut setup.app, "Remove from Build");
    let remaining = BTreeSet::from([setup.builds.clone()]);
    assert_eq!(setup.app.enabled_recipe_paths, remaining);
    assert_eq!(
        setup
            .library
            .enabled_paths(&setup.app.recipe_entries)
            .unwrap(),
        remaining,
        "the library keeps the new selection"
    );
    assert!(!setup.app.build_status_open);
    let staged = build(&setup.app).expect("the build goes through without the recipe");
    assert_eq!(staged.weapons.len(), 1);
    let _ = writeln!(
        report,
        "## Remove from Build\n\nThe build now holds only `{}` and built {} weapon.\n",
        setup.builds.display(),
        staged.weapons.len()
    );
}

/// Open Recipe opens the recipe the build stopped on.
fn open(setup: &mut Setup, ctx: &egui::Context, failure: BuildFailure, report: &mut String) {
    setup
        .app
        .apply_build_selection([setup.builds.clone(), setup.blocking.clone()].into())
        .unwrap();
    finish(&mut setup.app, Err(failure));
    click(ctx, &mut setup.app, "Open Recipe");
    assert_eq!(setup.app.recipe_path.as_ref(), Some(&setup.blocking));
    assert_eq!(setup.app.recipe.namespace, setup.namespace);
    let _ = writeln!(
        report,
        "## Open Recipe\n\nOpened `{}` in the editor.",
        setup.blocking.display()
    );
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn a_recipe_that_stops_the_build_can_be_removed_from_it_or_opened() {
    let packages = crate::test_support::stock_packages();
    let mut setup = setup(packages);
    let failure = build(&setup.app).expect_err("a recipe with no ammo type stops the build");
    assert_eq!(failure.recipe.as_deref(), Some(setup.namespace.as_str()));
    assert!(failure.message.contains("\"Blocker Check Rose\""));
    assert!(failure.message.contains("no ammo type for Collections"));
    let mut report = format!(
        "# Build Blocker E2E\n\n## First Build\n\nStopped on `{}`:\n\n```\n{}\n```\n\n",
        setup.namespace, failure.message
    );
    finish(&mut setup.app, Err(failure.clone()));
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let offered = settled(&ctx, &mut setup.app)
        .into_iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    for label in ["Copy Error", "Remove from Build", "Open Recipe"] {
        assert!(
            offered.iter().any(|name| name == label),
            "{label} in {offered:?}"
        );
    }
    let _ = writeln!(report, "Build Blocked offers: {}.\n", offered.join(", "));
    remove(&mut setup, &ctx, &mut report);
    open(&mut setup, &ctx, failure, &mut report);
    let out = crate::test_support::artifacts("build-blocker")
        .unwrap_or_else(|| std::env::temp_dir().join("parhelion-build-blocker"));
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("report.md"), &report).unwrap();
    eprintln!("{report}\nReport: {}", out.join("report.md").display());
}
