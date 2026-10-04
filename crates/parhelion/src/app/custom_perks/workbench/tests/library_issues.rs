//! The Custom Perks list against a real install and library: every perk with a problem or a
//! warning the workbench names carries the muted warning icon, no other perk does, the icon
//! never covers a name, and a behavior that never ends warns without blocking Apply.
//!
//! Opt in with `PARHELION_WORKBENCH_INSTALL` (an install root), `PARHELION_WORKBENCH_CATALOG` (a
//! copy of the catalog cache, never the live one), `PARHELION_LIBRARY_ROOT` (a Parhelion data
//! directory, whose perks are copied so nothing touches it) and `PARHELION_LIBRARY_ISSUES_OUT`
//! (the report directory). A perk that never ends joins the copy, so one row is always flagged.
//! `PARHELION_UI_CAPTURE_DIR` also captures the list.
use super::*;
use crate::test_support::driver::texts;
use sundial::package_authoring::sandbox_perk::program::{Action, NativeNode, Trigger};

fn env_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} is not set")))
}

fn draw(ctx: &egui::Context, app: &mut crate::app::PackageAuthoringApp) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440.0, 1300.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |_| {});
            app.draw_perk_workbench(ctx);
        },
    )
}

#[test]
#[ignore = "requires PARHELION_WORKBENCH_INSTALL, PARHELION_WORKBENCH_CATALOG, PARHELION_LIBRARY_ROOT and PARHELION_LIBRARY_ISSUES_OUT"]
fn a_real_library_marks_exactly_the_perks_with_problems() {
    let install = env_path("PARHELION_WORKBENCH_INSTALL");
    let cache = env_path("PARHELION_WORKBENCH_CATALOG");
    let source = env_path("PARHELION_LIBRARY_ROOT").join("perks");
    let out = env_path("PARHELION_LIBRARY_ISSUES_OUT");
    std::fs::create_dir_all(&out).unwrap();
    let root = tempfile::tempdir().unwrap();
    for file in std::fs::read_dir(&source).unwrap() {
        let file = file.unwrap().path();
        if file.is_file() {
            std::fs::copy(&file, root.path().join(file.file_name().unwrap())).unwrap();
        }
    }
    let library = Library::open(root.path().to_path_buf()).unwrap();
    assert!(
        !library.scan().unwrap().entries.is_empty(),
        "no configured library perks were copied"
    );
    let mut endless = PerkRecipe::new();
    endless.name = "Library Issues Never Ends".into();
    let mut effect = program::new_effect(1178);
    let never = effect.program.as_mut().unwrap();
    never.trigger = Trigger::Native;
    never.native_trigger = NativeNode::condition(2);
    never.duration_ms = 0;
    never.actions = vec![Action::add_rounds(1)];
    endless.effects = vec![effect];
    library.save(&endless, None).unwrap();

    let catalog = sundial::investment::InvestmentCatalog::load_with_cache_path(
        &install,
        &cache,
        false,
        |_| {},
    )
    .expect("catalog");
    let choices =
        catalog.weapon_sandbox_perk_choices_from(crate::package_profile::is_stock_item_definition);
    let mut app = crate::app::PackageAuthoringApp {
        sandbox_perk_choices: choices,
        catalog: Some(catalog),
        packages: install.join("packages"),
        ..Default::default()
    };
    app.perk_workbench.library = Some(library);
    app.perk_workbench.initialized = true;
    app.perk_workbench.refresh_library();
    app.perk_workbench.open = true;
    let ctx = egui::Context::default();
    ctx.set_theme(egui::Theme::Dark);
    sundial::investment::configure_authoring_fonts(&ctx, &install).ok();

    // Discovery reads the install in the background, and the list checks saved perks a few
    // at a time, so frames run until both are done.
    wait_for_library(&ctx, &mut app);
    let output = draw(&ctx, &mut app);
    capture::write(&ctx, &output, "library-issues");

    let shapes = texts(&output);
    let icons = shapes
        .iter()
        .filter(|(text, _)| text == egui_phosphor::regular::WARNING)
        .map(|(_, rect)| *rect)
        .collect::<Vec<_>>();
    let workbench = &app.perk_workbench;
    let mut report = String::from(
        "# Custom Perks problems\n\n| Perk | Marked | Kind | Message |\n|---|---|---|---|\n",
    );
    let mut failures = Vec::new();
    let mut flagged = 0;
    let mut checked = 0;
    for entry in &workbench.entries {
        // What the status bar would show with the perk open: a problem, else a warning.
        let issue = workbench.validation_issue(&entry.recipe);
        flagged += usize::from(issue.is_some());
        // Only rows drawn on screen under a name no other perk shares can be checked.
        let rows = shapes
            .iter()
            .filter(|(text, rect)| text == &entry.recipe.name && rect.max.y < 1300.0)
            .collect::<Vec<_>>();
        let marked = match rows.as_slice() {
            [(_, name)] => {
                checked += 1;
                let icon = row_icon(&icons, name, &entry.recipe.name, &mut failures);
                if icon.is_some() != issue.is_some() {
                    failures.push(format!(
                        "{}: marked {}, issue {:?}",
                        entry.recipe.name,
                        icon.is_some(),
                        issue.as_ref().map(|issue| &issue.message)
                    ));
                }
                if icon.is_some() { "Yes" } else { "No" }
            }
            _ => "Not drawn",
        };
        let kind = match &issue {
            Some(issue) if issue.blocking => "Problem",
            Some(_) => "Warning",
            None => "",
        };
        report.push_str(&format!(
            "| {} | {marked} | {kind} | {} |\n",
            entry.recipe.name.replace('|', "/"),
            issue
                .as_ref()
                .map_or("", |issue| issue.message.as_str())
                .replace('|', "/")
        ));
    }
    report.push_str(&format!(
        "\n{} perks, {flagged} flagged, {} icons drawn.\n",
        workbench.entries.len(),
        icons.len()
    ));
    std::fs::write(out.join("library-issues.md"), &report).unwrap();
    // A behavior that never ends is a warning: it is marked, but it can still be applied.
    let planted = workbench
        .entries
        .iter()
        .find(|entry| entry.recipe.name == endless.name)
        .expect("the flagged perk is missing from the list");
    let planted = workbench
        .validation_issue(&planted.recipe)
        .expect("the perk that never ends is not flagged");
    assert!(!planted.blocking, "a behavior that never ends blocks Apply");
    assert!(
        checked > 0,
        "no visible library rows were checked:\n{report}"
    );
    assert!(flagged > 0, "no perk was flagged:\n{report}");
    assert!(failures.is_empty(), "{}\n\n{report}", failures.join("\n"));
}

fn row_icon<'a>(
    icons: &'a [egui::Rect],
    name: &egui::Rect,
    perk: &str,
    failures: &mut Vec<String>,
) -> Option<&'a egui::Rect> {
    let icon = icons
        .iter()
        .find(|icon| icon.center().y > name.min.y - 4.0 && icon.center().y < name.max.y + 4.0);
    if let Some(icon) = icon
        && name.max.x > icon.min.x + 1.0
    {
        failures.push(format!("{perk}: the icon covers the name"));
    }
    icon
}

fn wait_for_library(ctx: &egui::Context, app: &mut crate::app::PackageAuthoringApp) {
    let start = std::time::Instant::now();
    loop {
        draw(ctx, app);
        let workbench = &app.perk_workbench;
        if !workbench.busy()
            && workbench.library_issues_ready
            && workbench.library_issues.len() == workbench.entries.len()
        {
            break;
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(600),
            "the list never finished checking the library"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
