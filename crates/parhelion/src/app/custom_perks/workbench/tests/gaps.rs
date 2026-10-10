//! Workflows missing from the previous authoring coverage. See the local failure model.
use super::*;
use crate::perk::{preflight, verification};
use sundial::package_authoring::sandbox_perk::{
    action,
    program::{Action, NativeNode, Program, Trigger, native_draft},
};

fn authored(index: u16, trigger: Trigger, rounds: u16) -> WeaponSandboxPerkRuntimeRecipe {
    let mut effect = PerkRecipe::effect(index);
    effect.program = Some(Program {
        name: format!("Behavior {index}"),
        trigger,
        actions: vec![Action::add_rounds(i32::from(rounds))],
        ..Program::default()
    });
    effect
}

#[test]
fn catalog_edit_save_reopen_readback_keeps_native_configuration_and_provenance() {
    use super::super::catalog_insert::{Placement, Request};
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().join("library")).unwrap();
    let mut recipe = PerkRecipe::new();
    recipe.effects.push(authored(405, Trigger::Drawn, 1));
    let donor = authored(406, Trigger::Drawn, 7);
    let payload = native_draft(donor.program.as_ref().unwrap())
        .unwrap()
        .graph
        .emit()
        .unwrap();
    let decoded = action::decode(&payload).unwrap();
    let native = &decoded.groups[0].effects[0];
    let node = NativeNode {
        kind: native.kind,
        bytes: native.native.clone(),
    };
    let request =
        Request::new(&recipe, 405, 0, Placement::Action, node.clone(), Some(406)).unwrap();
    let stale = request.clone();
    request.apply(&mut recipe).unwrap();
    let after = recipe.clone();
    assert!(stale.apply(&mut recipe).is_err());
    assert_eq!(
        recipe, after,
        "a stale catalog window cannot change a new graph"
    );
    let entry = library.save(&recipe, None).unwrap();
    let reopened = library.scan().unwrap().entries.remove(0).recipe;
    assert_eq!(reopened, recipe);
    assert_eq!(reopened.sources.len(), 1);
    assert_eq!(reopened.sources[0].stock_perk, Some(406));
    let program = reopened.effects[0].program.as_ref().unwrap();
    let readback = action::decode(&native_draft(program).unwrap().graph.emit().unwrap()).unwrap();
    assert_eq!(readback.groups[0].effects.len(), 2);
    assert!(
        readback.groups[0]
            .effects
            .iter()
            .any(|effect| effect.native == node.bytes)
    );
    crate::test_support::artifact(
        "perk-catalog-roundtrip.json",
        &serde_json::json!({
            "saved_bytes": std::fs::read_to_string(entry.path).unwrap(),
            "reopened": reopened,
            "actions": readback.groups[0].effects.iter().map(|effect| effect.description()).collect::<Vec<_>>(),
            "stale_insert_rejected": true,
            "gameplay_tested": false,
        }),
    );
    let mut workbench = Workbench::offline();
    workbench.documents.push(Document::new(reopened, None));
    workbench.engine.open = true;
    workbench
        .engine
        .kinds
        .open(sundial::investment::discovery::kinds::Family::Effects, 14);
    let ctx = egui::Context::default();
    let mut output = frame(
        &ctx,
        &mut workbench,
        false,
        egui::vec2(1300.0, 900.0),
        vec![],
    );
    for _ in 0..3 {
        output = frame(
            &ctx,
            &mut workbench,
            false,
            egui::vec2(1300.0, 900.0),
            vec![],
        );
    }
    capture::write(&ctx, &output, "perk-catalog-insertion");
    workbench.engine.open = false;
    workbench.verification.open = true;
    for _ in 0..3 {
        output = frame(
            &ctx,
            &mut workbench,
            false,
            egui::vec2(1300.0, 900.0),
            vec![],
        );
    }
    capture::write(&ctx, &output, "perk-gameplay-verification");
}

#[test]
fn runtime_budget_diagnostics_survive_consolidation_and_refusal() {
    let mut recipe = PerkRecipe::new();
    for index in 405..411 {
        recipe
            .effects
            .push(authored(index, Trigger::Drawn, index - 404));
    }
    let diagnostics = preflight::check(&recipe);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|issue| issue.code == "inactive_effect")
            .count(),
        2
    );
    preflight::consolidate(&mut recipe, 0, 5).unwrap();
    assert_eq!(recipe.effects.len(), 5);
    let program = recipe.effects[0].program.as_ref().unwrap();
    assert_eq!(
        action::decode(&native_draft(program).unwrap().graph.emit().unwrap())
            .unwrap()
            .groups[0]
            .effects
            .len(),
        2
    );
    recipe.effects[4].program.as_mut().unwrap().trigger = Trigger::Always;
    let before = recipe.clone();
    assert!(preflight::consolidate(&mut recipe, 0, 4).is_err());
    assert_eq!(recipe, before);
    crate::test_support::artifact(
        "perk-preflight.json",
        &serde_json::json!({
            "diagnostics": diagnostics, "recipe": recipe, "incompatible_merge_left_unchanged": true,
        }),
    );
}

#[test]
fn native_noop_selector_is_blocked_but_an_unverified_host_remains_authorable() {
    let mut recipe = PerkRecipe::new();
    let mut effect = authored(405, Trigger::Always, 1);
    effect.program.as_mut().unwrap().actions = vec![Action::AdjustComponent {
        target: 0.into(),
        flag: 3.into(),
        option: 0.into(),
        scale_bits: 1.0_f32.to_bits(),
        limit_bits: (-1.0_f32).to_bits(),
        value_bits: 1.0_f32.to_bits(),
        input: 255.into(),
    }];
    recipe.effects.push(effect);
    assert!(
        preflight::check(&recipe)
            .iter()
            .any(|issue| issue.blocking && issue.code == "invalid_state")
    );
    if let Action::AdjustComponent { flag, .. } =
        &mut recipe.effects[0].program.as_mut().unwrap().actions[0]
    {
        *flag = 0.into();
    }
    assert!(!preflight::check(&recipe).iter().any(|issue| issue.blocking));
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES"]
fn discovery_can_stop_between_native_phases_without_publishing_partial_catalog() {
    use sundial::investment::discovery::{DiscoveryEvent, Phase, discover_cancellable};
    let packages = crate::test_support::stock_packages();
    let stop = std::sync::atomic::AtomicBool::new(false);
    let mut phases = Vec::new();
    let result = discover_cancellable(&packages, &stop, |event| {
        if let DiscoveryEvent::Phase(phase) = event {
            phases.push(phase.label());
            if phase == Phase::NativePaths {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    });
    assert!(result.is_err());
    assert!(stop.load(std::sync::atomic::Ordering::Relaxed));
    assert!(!phases.contains(&Phase::Perks.label()));
    crate::test_support::artifact("perk-native-scan-cancellation.json", &phases);
}

#[test]
fn diagnostics_window_shows_all_inactive_effects_and_navigates_to_one() {
    let mut workbench = Workbench::offline();
    let mut recipe = PerkRecipe::new();
    for index in 405..411 {
        recipe.effects.push(authored(index, Trigger::Drawn, 1));
    }
    workbench.documents.push(Document::new(recipe, None));
    workbench.diagnostics_open = true;
    let ctx = egui::Context::default();
    let mut render = |events| {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ui| workbench.show_diagnostics(ui),
        )
    };
    let mut output = render(vec![]);
    for _ in 0..3 {
        output = render(vec![]);
    }
    // Six effects overrun the four-effect budget, so the last two are inactive.
    assert!(
        label(&output, "Open Effect 5").is_some(),
        "the first inactive effect is listed in the diagnostics window"
    );
    let problem = label(&output, "Open Effect 6")
        .expect("the second inactive effect is reachable")
        .center();
    capture::write(&ctx, &output, "perk-diagnostics");
    for events in crate::test_support::driver::tap(problem) {
        render(events);
    }
    assert_eq!(workbench.reveal_problem.as_ref().unwrap().effect, 410);
}

#[test]
fn verification_records_are_bound_to_configuration_and_cannot_invent_gameplay_evidence() {
    let temporary = tempfile::tempdir().unwrap();
    let mut recipe = PerkRecipe::new();
    recipe.effects.push(authored(405, Trigger::Drawn, 1));
    let mut record =
        verification::Record::new(&recipe, crate::ItemKind::Weapon, "86657.20.08.23").unwrap();
    assert!(
        record
            .record(
                verification::Check::Activation,
                verification::Outcome::Passed,
                "Observed in a fixture"
            )
            .is_err(),
        "no compiled package was bound"
    );
    let first = record.save(temporary.path()).unwrap();
    let loaded = verification::Record::load(&first).unwrap();
    assert!(loaded.matches(&recipe));
    assert!(!loaded.gameplay_verified());
    recipe.effects[0].program.as_mut().unwrap().actions = vec![Action::add_rounds(9)];
    assert!(!loaded.matches(&recipe));
    let second = record.save(temporary.path()).unwrap();
    assert_ne!(first, second);
    assert_eq!(verification::Record::load(&first).unwrap(), loaded);
    crate::test_support::artifact("perk-verification-unobserved.json", &loaded);
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn catalog_to_staged_private_perk_has_a_repeatable_package_receipt() {
    use super::super::catalog_insert::{Placement, Request};
    let packages = crate::test_support::stock_packages();
    let staging_root = crate::test_support::artifact_dir("staging");
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let donor = catalog.weapon_donor(0x23DB_942F).unwrap();
    let discovery = sundial::investment::discovery::discover(&packages, |_| {}).unwrap();
    let (stock, node) = discovery
        .perks
        .perks
        .iter()
        .find_map(|stock| {
            let program = stock.behavior.as_ref()?.program.as_ref()?;
            let decoded = crate::perk::preflight::decoded(program).ok()?;
            let node = decoded.effects().find(|node| node.kind == 14)?;
            Some((
                stock,
                NativeNode {
                    kind: node.kind,
                    bytes: node.native.clone(),
                },
            ))
        })
        .expect("installed catalog has a fixed ammunition configuration");
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().join("library")).unwrap();
    let mut perk = PerkRecipe::new();
    perk.name = "Catalog Workflow Perk".into();
    perk.effects.push(authored(405, Trigger::Drawn, 1));
    Request::new(
        &perk,
        405,
        0,
        Placement::Action,
        node.clone(),
        Some(u16::try_from(stock.index).unwrap()),
    )
    .unwrap()
    .apply(&mut perk)
    .unwrap();
    library.save(&perk, None).unwrap();
    let perk = library.scan().unwrap().entries.remove(0).recipe;
    let mut weapon = WeaponRecipe::new_weapon_for_donor(
        "parhelion.catalog-workflow",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    let socket = donor.sockets.len();
    weapon.overrides.socket_columns = vec![None; socket];
    weapon
        .overrides
        .socket_columns
        .push(Some(crate::WeaponSocketColumnRecipe {
            socket_type: Some(92),
            choices: vec![crate::perk::DEFAULT_PLUG_LAYOUT.into()],
            ..Default::default()
        }));
    let expanded = socket_editor::socket_editor_donor(&donor, &weapon).into_owned();
    let target = Target::capture(&weapon, &expanded, socket, 0).unwrap();
    Change {
        target,
        perk: Some(perk.clone()),
    }
    .apply(&mut weapon, &donor)
    .unwrap();
    let snapshot = crate::workflow::BatchBuildSnapshot::new(crate::workflow::BatchBuildRequest {
        package_directory: packages.clone(),
        staging_root,
        ignore_installed_authored_overlays: true,
        recipes: vec![weapon],
    })
    .unwrap();
    let report =
        crate::workflow::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let view = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    for artifact in &report.artifacts {
        view.add_overlay(&report.run_directory.join(&artifact.file_name))
            .unwrap();
    }
    let manager = sundial::package_authoring::PackageManager::new(
        view.path(),
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        None,
    )
    .unwrap();
    use sundial::package_authoring::sandbox_perk::{
        SANDBOX_PERK_RUNTIME_MAP_TAG, sandbox_perk_runtime_assignment,
    };
    let runtime = manager
        .read_tag(tiger_pkg::TagHash(SANDBOX_PERK_RUNTIME_MAP_TAG))
        .unwrap();
    let private = &report.weapons[0]
        .custom_plugs
        .iter()
        .find(|plug| plug.socket_index == socket)
        .unwrap()
        .perks[0];
    let assignment = sandbox_perk_runtime_assignment(&runtime, private.runtime_key)
        .unwrap()
        .unwrap();
    let action_tag = assignment.action_tag().unwrap();
    let payload = manager.read_tag(action_tag).unwrap();
    let compiled = action::decode(&payload).unwrap();
    assert_eq!(compiled.groups[0].effects.len(), 2);
    assert!(
        compiled.groups[0]
            .effects
            .iter()
            .any(|effect| effect.native == node.bytes)
    );
    assert_ne!(Some(action_tag.0), stock.action);
    assert_eq!(
        sandbox_perk_runtime_assignment(&runtime, stock.runtime_key)
            .unwrap()
            .unwrap()
            .action_tag()
            .map(|tag| tag.0),
        stock.action
    );
    drop(manager);
    let mut record =
        verification::Record::new(&perk, crate::ItemKind::Weapon, "86657.20.08.23").unwrap();
    record.bind_staged(&report.run_directory).unwrap();
    assert!(!record.packages.is_empty());
    assert!(!record.gameplay_verified());
    let saved = record.save(&report.run_directory).unwrap();
    assert_eq!(verification::Record::load(&saved).unwrap(), record);
    crate::test_support::artifact("perk-catalog-staged-readback.json", &record);
    crate::test_support::artifact(
        "perk-catalog-native-action.json",
        &serde_json::json!({
            "private_action": format!("0x{:08X}", action_tag.0),
            "stock_action": stock.action, "stock_assignment_unchanged": true,
            "copied_native_node": node, "compiled_action_bytes": payload,
            "gameplay_tested": false,
        }),
    );
}
