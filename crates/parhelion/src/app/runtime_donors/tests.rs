//! Headless donor-review contracts. Reports and runtime graphs are synthetic, without packages.
use super::*;
use crate::runtime::compatibility::ComponentDonorAssessment;
use std::sync::atomic::{AtomicBool, Ordering};
use sundial::package_authoring::runtime::WeaponRuntimeBinding;

const BASELINE: u32 = 0x10;
const SAFE_Z: u32 = 0x20;
const SAFE_A: u32 = 0x21;
const EXPERIMENTAL: u32 = 0x30;
const REJECTED: u32 = 0x40;
const BINDING: u32 = WEAPON_RELOAD_COMPONENT_KEY;
const VIEWPORT: egui::Vec2 = egui::vec2(1000.0, 900.0);

fn donor(hash: u32, name: &str) -> WeaponDonorSummary {
    WeaponDonorSummary {
        hash,
        name: name.into(),
        type_name: "Sidearm".into(),
        bucket_hash: 0,
        collection_backed: true,
        power_cap: None,
        damage_type: None,
        inventory_slot: None,
        ammo_type: None,
        weapon_pattern_index: None,
        weapon_translation_group: None,
        stat_group_index: None,
        damage_profile: WeaponDamageProfile::Unknown,
        rarity: WeaponRarity::Legendary,
    }
}

fn report() -> ComponentCompatibilityReport {
    ComponentCompatibilityReport {
        binding_hash: BINDING,
        candidates: [
            (SAFE_Z, DonorCompatibility::LowerRisk),
            (SAFE_A, DonorCompatibility::LowerRisk),
            (EXPERIMENTAL, DonorCompatibility::Experimental),
            (REJECTED, DonorCompatibility::Incompatible),
        ]
        .into_iter()
        .map(|(hash, status)| {
            (
                hash,
                ComponentDonorAssessment {
                    status,
                    reasons: vec![format!("Synthetic assessment for 0x{hash:08X}.")],
                    affected_bindings: vec![BINDING, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY],
                },
            )
        })
        .collect(),
        affected_bindings: vec![
            (BINDING, "Reload Behavior".into()),
            (
                WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
                "Weapon Stat Translator".into(),
            ),
        ],
        current_sources: BTreeMap::new(),
        current_error: None,
    }
}

fn app() -> PackageAuthoringApp {
    let mut app = PackageAuthoringApp {
        show_experimental_options: true,
        donor_summaries: vec![
            donor(REJECTED, "A Rejected Donor"),
            donor(EXPERIMENTAL, "A Experimental Donor"),
            donor(SAFE_Z, "Z Safe Donor"),
            donor(SAFE_A, "A Safe Donor"),
            donor(BASELINE, "Base Runtime"),
        ],
        ..Default::default()
    };
    app.recipe.donor = reference(BASELINE, "Base Runtime");
    app.recipe.overrides.weapon_pattern_index = Some(7);
    let key = app.runtime_graph_key().expect("synthetic runtime baseline");
    app.runtime_donors.picker = Some(Picker::new(
        BINDING,
        Some(key.clone()),
        Some(BASELINE),
        String::new(),
        None,
    ));
    app.runtime_donors
        .reports
        .insert(BINDING, (key, Arc::new(report())));
    seed_reviews(&mut app);
    app
}

fn seed_reviews(app: &mut PackageAuthoringApp) {
    let report = Arc::clone(&app.runtime_donors.reports[&BINDING].1);
    for (&hash, assessment) in &report.candidates {
        let donor = app
            .donor_summaries
            .iter()
            .find(|donor| donor.hash == hash)
            .unwrap();
        let mut after = app.recipe.clone();
        for &binding in &assessment.affected_bindings {
            after.set_runtime_component_donor(binding, Some(reference(hash, &donor.name)));
        }
        let plan = crate::runtime::swap::Preview {
            before: app.recipe.clone(),
            after,
            binding_hash: BINDING,
            donor_hash: hash,
            group: assessment.affected_bindings.clone(),
            kept: 0,
            transferred: vec![],
            resets: vec![],
            error: None,
        };
        app.runtime_donors.reviews.insert(
            (BINDING, hash),
            Arc::new(Review {
                recipe: app.recipe.clone(),
                binding_hash: BINDING,
                donor_hash: hash,
                result: Ok(plan),
            }),
        );
    }
}

fn reference(hash: u32, name: &str) -> WeaponDonorReference {
    WeaponDonorReference {
        item_hash: hash.into(),
        expected_name: Some(name.into()),
    }
}

fn frame(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    viewport: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, viewport)),
            events,
            ..Default::default()
        },
        |ctx| app.draw_runtime_donor_browser(ctx),
    )
}

fn settle(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    viewport: egui::Vec2,
) -> egui::FullOutput {
    frame(ctx, app, viewport, vec![]);
    frame(ctx, app, viewport, vec![]);
    frame(ctx, app, viewport, vec![])
}

fn rendered_labels(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    fn visit(shape: &egui::Shape, labels: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(text) => labels.push((
                text.galley.job.text.clone(),
                egui::Rect::from_min_size(text.pos, text.galley.size()),
            )),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, labels);
                }
            }
            _ => {}
        }
    }
    let mut labels = Vec::new();
    for clipped in &output.shapes {
        visit(&clipped.shape, &mut labels);
    }
    labels
}

fn label_rect(output: &egui::FullOutput, label: &str) -> egui::Rect {
    let labels = rendered_labels(output);
    labels
        .iter()
        .find_map(|(text, rect)| (text == label).then_some(*rect))
        .unwrap_or_else(|| panic!("Missing rendered label: {label}. Rendered labels: {labels:#?}"))
}

fn candidate_label(name: &str, _status: DonorCompatibility) -> String {
    name.to_owned()
}

fn click(ctx: &egui::Context, app: &mut PackageAuthoringApp, position: egui::Pos2) {
    for events in crate::test_support::driver::tap(position) {
        frame(ctx, app, VIEWPORT, events);
    }
}

#[test]
fn incompatible_donors_never_apply_and_experimental_donors_require_opt_in() {
    for experimental in [false, true] {
        assert!(can_apply(DonorCompatibility::LowerRisk, experimental));
        assert!(!can_apply(DonorCompatibility::Incompatible, experimental));
        assert_eq!(
            can_apply(DonorCompatibility::Experimental, experimental),
            experimental
        );
    }
    assert!(
        status_rank(DonorCompatibility::LowerRisk) < status_rank(DonorCompatibility::Experimental)
    );
    assert!(
        status_rank(DonorCompatibility::Experimental)
            < status_rank(DonorCompatibility::Incompatible)
    );
}

#[test]
fn default_review_lists_only_lower_risk_matches_and_preserves_the_recipe() {
    for viewport in [
        egui::vec2(480.0, 800.0),
        egui::vec2(900.0, 640.0),
        egui::vec2(1320.0, 900.0),
    ] {
        let ctx = egui::Context::default();
        let mut app = app();
        let before = app.recipe.clone();
        let output = settle(&ctx, &mut app, viewport);
        let labels = rendered_labels(&output);
        assert!(labels.iter().any(|(text, _)| text == "Lower Risk 2"));
        assert!(
            !labels.iter().any(|(text, _)| text.contains("Unchecked")),
            "status slop must not be rendered"
        );
        assert!(
            !labels
                .iter()
                .any(|(text, _)| text.contains("A Experimental Donor"))
        );
        assert!(
            !labels
                .iter()
                .any(|(text, _)| text.contains("A Rejected Donor"))
        );
        let first = label_rect(
            &output,
            &candidate_label("A Safe Donor", DonorCompatibility::LowerRisk),
        );
        let second = label_rect(
            &output,
            &candidate_label("Z Safe Donor", DonorCompatibility::LowerRisk),
        );
        assert!(
            first.top() < second.top(),
            "lower-risk names should be sorted"
        );
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, viewport);
        assert!(
            screen.contains_rect(first),
            "candidate outside {viewport:?}: {first:?}"
        );
        assert!(
            screen.contains_rect(second),
            "candidate outside {viewport:?}: {second:?}"
        );
        assert!(
            labels
                .iter()
                .any(|(text, _)| text.contains("Sidearm · 0x00000021"))
        );
        assert_eq!(app.recipe, before);
        assert!(
            !app.runtime_donors.busy(),
            "cached reports must not start package workers"
        );
    }
}

#[test]
fn choosing_a_candidate_only_applies_after_the_apply_button() {
    let ctx = egui::Context::default();
    let mut app = app();
    let before = app.recipe.clone();
    let output = settle(&ctx, &mut app, VIEWPORT);
    click(
        &ctx,
        &mut app,
        label_rect(
            &output,
            &candidate_label("Z Safe Donor", DonorCompatibility::LowerRisk),
        )
        .center(),
    );
    assert_eq!(
        app.runtime_donors.picker.as_ref().unwrap().selected,
        Some(SAFE_Z)
    );
    assert_eq!(
        app.recipe, before,
        "highlighting a donor is not an apply action"
    );
    let output = settle(&ctx, &mut app, VIEWPORT);
    assert!(
        rendered_labels(&output)
            .iter()
            .any(|(text, _)| text == "Changes Together")
    );
    click(&ctx, &mut app, label_rect(&output, "Apply Donor").center());
    assert_eq!(
        app.recipe.runtime_component_donor(BINDING),
        Some(&reference(SAFE_Z, "Z Safe Donor"))
    );
    assert!(app.runtime_donors.picker.is_none());
    assert!(app.runtime_donors.reports.is_empty());
    assert!(app.runtime_graph.is_none());
    assert_eq!(
        app.recipe
            .runtime_component_donor(WEAPON_STAT_TRANSLATOR_COMPONENT_KEY),
        Some(&reference(SAFE_Z, "Z Safe Donor"))
    );
    assert_eq!(app.runtime_donors.last_change.as_ref().unwrap().0, before);
}

#[test]
fn unsupported_settings_require_the_listed_reset_choice_before_applying() {
    let ctx = egui::Context::default();
    let mut app = app();
    app.runtime_donors.picker.as_mut().unwrap().selected = Some(SAFE_Z);
    let review = Arc::make_mut(
        app.runtime_donors
            .reviews
            .get_mut(&(BINDING, SAFE_Z))
            .unwrap(),
    );
    review
        .result
        .as_mut()
        .unwrap()
        .resets
        .push("Customized Reload Setting".into());
    let before = app.recipe.clone();
    let output = settle(&ctx, &mut app, VIEWPORT);
    click(&ctx, &mut app, label_rect(&output, "Apply Donor").center());
    assert_eq!(app.recipe, before);
    let output = settle(&ctx, &mut app, VIEWPORT);
    click(
        &ctx,
        &mut app,
        label_rect(&output, "Reset Listed Edits").center(),
    );
    assert!(
        app.runtime_donors
            .picker
            .as_ref()
            .unwrap()
            .reset_unsupported
    );
    let output = settle(&ctx, &mut app, VIEWPORT);
    click(&ctx, &mut app, label_rect(&output, "Apply Donor").center());
    assert_eq!(
        app.recipe.runtime_component_donor(BINDING),
        Some(&reference(SAFE_Z, "Z Safe Donor"))
    );
}

#[test]
fn a_settings_check_is_required_and_stale_recipe_snapshots_cannot_apply() {
    let mut app = app();
    app.runtime_donors.picker.as_mut().unwrap().selected = Some(SAFE_Z);
    let picker = app.runtime_donors.picker.as_ref().unwrap();
    let review = app.runtime_donors.reviews[&(BINDING, SAFE_Z)].as_ref();
    assert!(preview::can_apply(Some(review), &app.recipe, picker));
    assert!(!preview::can_apply(None, &app.recipe, picker));
    let mut edited = app.recipe.clone();
    edited.overrides.ammo_type = Some(crate::RecipeAmmoType::Heavy);
    assert!(!preview::can_apply(Some(review), &edited, picker));
    let mut other = app.recipe.clone();
    other.name.push_str("Changed while checking");
    assert!(!preview::can_apply(Some(review), &other, picker));
}

#[test]
fn a_compiler_conflict_blocks_apply_even_when_resets_are_accepted() {
    let mut app = app();
    let picker = app.runtime_donors.picker.as_mut().unwrap();
    picker.selected = Some(SAFE_Z);
    picker.reset_unsupported = true;
    let review = Arc::make_mut(
        app.runtime_donors
            .reviews
            .get_mut(&(BINDING, SAFE_Z))
            .unwrap(),
    );
    review.result.as_mut().unwrap().error = Some("Saved edits overlap".into());
    assert!(!preview::can_apply(Some(review), &app.recipe, picker));
}

#[test]
fn undo_restores_the_entire_previous_recipe_and_preserves_later_edits() {
    for edited_after in [false, true] {
        let mut app = app();
        let before = app.recipe.clone();
        let after = app.runtime_donors.reviews[&(BINDING, SAFE_Z)]
            .result
            .as_ref()
            .unwrap()
            .after
            .clone();
        app.recipe = after.clone();
        app.runtime_donors.last_change = Some((before.clone(), after));
        if edited_after {
            app.recipe.overrides.ammo_type = Some(crate::RecipeAmmoType::Heavy);
        }
        let current = app.recipe.clone();
        let ctx = egui::Context::default();
        let mut output = egui::FullOutput::default();
        let mut draw = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, VIEWPORT)),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.draw_runtime_donor_undo(ui));
                },
            )
        };
        for _ in 0..3 {
            output = draw(vec![]);
        }
        if edited_after {
            assert!(
                !rendered_labels(&output)
                    .iter()
                    .any(|(label, _)| label == "Undo Donor Change")
            );
        } else {
            let position = label_rect(&output, "Undo Donor Change").center();
            for pressed in [true, false] {
                draw(vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
            }
        }
        assert_eq!(app.recipe, if edited_after { current } else { before });
        assert!(app.runtime_donors.last_change.is_none());
    }
}

#[test]
fn experimental_apply_button_requires_the_explicit_checkbox() {
    let ctx = egui::Context::default();
    let mut app = app();
    app.runtime_donors.picker.as_mut().unwrap().selected = Some(EXPERIMENTAL);
    let before = app.recipe.clone();
    let output = settle(&ctx, &mut app, VIEWPORT);
    click(&ctx, &mut app, label_rect(&output, "Apply Donor").center());
    assert_eq!(
        app.recipe, before,
        "a saved experimental selection is not consent"
    );
    let output = settle(&ctx, &mut app, VIEWPORT);
    click(
        &ctx,
        &mut app,
        label_rect(&output, "Accept Crash Risk").center(),
    );
    assert!(app.runtime_donors.picker.as_ref().unwrap().experimental);
    assert_eq!(
        app.recipe, before,
        "accepting the risk must not apply a donor"
    );
    let output = settle(&ctx, &mut app, VIEWPORT);
    click(&ctx, &mut app, label_rect(&output, "Apply Donor").center());
    assert_eq!(
        app.recipe.runtime_component_donor(BINDING),
        Some(&reference(EXPERIMENTAL, "A Experimental Donor"))
    );
}

#[test]
fn rejected_donor_stays_unapplyable_even_with_the_crash_risk_accepted() {
    let ctx = egui::Context::default();
    let mut app = app();
    let picker = app.runtime_donors.picker.as_mut().unwrap();
    picker.experimental = true;
    picker.selected = Some(REJECTED);
    let before = app.recipe.clone();
    let output = settle(&ctx, &mut app, VIEWPORT);
    click(&ctx, &mut app, label_rect(&output, "Apply Donor").center());
    assert_eq!(app.recipe, before);
    assert_eq!(
        app.runtime_donors.picker.as_ref().unwrap().selected,
        Some(REJECTED)
    );
}

#[test]
fn a_stale_report_cannot_apply_after_the_runtime_baseline_changes() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = app();
    app.runtime_donors.picker.as_mut().unwrap().selected = Some(SAFE_Z);
    let output = settle(&ctx, &mut app, VIEWPORT);
    let old_apply_position = label_rect(&output, "Apply Donor").center();
    app.recipe.overrides.weapon_pattern_index = Some(8);
    let changed = app.recipe.clone();
    click(&ctx, &mut app, old_apply_position);
    assert_eq!(app.recipe, changed);
    let picker = app.runtime_donors.picker.as_ref().unwrap();
    assert!(picker.selected.is_none());
    assert!(
        picker
            .error
            .as_ref()
            .is_some_and(|error| error.contains("The runtime changed"))
    );
    assert!(!app.runtime_donors.busy());
    let output = settle(&ctx, &mut app, VIEWPORT);
    let tree = output
        .platform_output
        .accesskit_update
        .expect("stale review should expose the disabled Apply Donor control");
    let apply = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Apply Donor"))
        .expect("Apply Donor must remain discoverable when disabled");
    assert!(apply.1.is_disabled(), "stale Apply Donor must be disabled");
}

struct WorkerGate {
    release: mpsc::Sender<()>,
    result_sent: Receiver<()>,
    finished: Arc<AtomicBool>,
}

fn gated_job(app: &mut PackageAuthoringApp) -> WorkerGate {
    let key = app.runtime_graph_key().unwrap();
    app.runtime_donors.reports.clear();
    let (release, wait_for_release) = mpsc::channel();
    let (sender, receiver) = mpsc::channel();
    let (sent, result_sent) = mpsc::channel();
    let finished = Arc::new(AtomicBool::new(false));
    let worker_finished = Arc::clone(&finished);
    let worker = thread::spawn(move || {
        if wait_for_release.recv().is_ok() {
            let _ = sender.send(Ok(report()));
            let _ = sent.send(());
        }
        worker_finished.store(true, Ordering::Release);
    });
    app.runtime_donors.job = Some(Job {
        binding_hash: BINDING,
        key,
        generation: app.runtime_donors.generation,
        receiver,
        worker,
    });
    WorkerGate {
        release,
        result_sent,
        finished,
    }
}

fn finish_job(app: &mut PackageAuthoringApp, gate: WorkerGate) {
    gate.release.send(()).unwrap();
    gate.result_sent
        .recv_timeout(Duration::from_secs(5))
        .expect("synthetic worker should send its result");
    app.poll_runtime_donors();
    assert!(!app.runtime_donors.busy());
    assert!(
        gate.finished.load(Ordering::Acquire),
        "poll must join the completed worker"
    );
}

#[test]
fn closing_the_browser_retains_its_worker_until_the_result_is_joined() {
    let mut app = app();
    let before = app.recipe.clone();
    let gate = gated_job(&mut app);
    app.runtime_donors.close();
    assert!(app.runtime_donors.picker.is_none());
    assert!(app.runtime_donors.busy());
    app.poll_runtime_donors();
    assert!(
        app.runtime_donors.busy(),
        "an empty receiver must retain the worker"
    );
    finish_job(&mut app, gate);
    assert!(app.runtime_donors.reports.contains_key(&BINDING));
    assert_eq!(app.recipe, before);
}

#[test]
fn invalidation_joins_the_worker_but_discards_its_stale_generation() {
    let mut app = app();
    let gate = gated_job(&mut app);
    app.runtime_donors.invalidate();
    assert!(app.runtime_donors.picker.is_none());
    assert!(app.runtime_donors.reports.is_empty());
    assert!(app.runtime_donors.busy());
    finish_job(&mut app, gate);
    assert!(
        app.runtime_donors.reports.is_empty(),
        "invalidated work must not refill the cache"
    );
}

#[test]
fn changed_runtime_key_discards_a_worker_result_without_explicit_invalidation() {
    let mut app = app();
    let gate = gated_job(&mut app);
    app.recipe.set_runtime_component_donor(
        WEAPON_BARREL_COMPONENT_KEY,
        Some(reference(SAFE_A, "A Safe Donor")),
    );
    let changed = app.recipe.clone();
    finish_job(&mut app, gate);
    assert!(app.runtime_donors.reports.is_empty());
    assert_eq!(app.recipe, changed);
}

#[test]
fn a_disconnected_scan_is_joined_and_reported_without_mutating_the_recipe() {
    let mut app = app();
    let before = app.recipe.clone();
    let (sender, receiver) = mpsc::channel();
    let (sent, finished) = mpsc::channel();
    let worker = thread::spawn(move || {
        drop(sender);
        let _ = sent.send(());
    });
    app.runtime_donors.reports.clear();
    app.runtime_donors.job = Some(Job {
        binding_hash: BINDING,
        key: app.runtime_graph_key().unwrap(),
        generation: app.runtime_donors.generation,
        receiver,
        worker,
    });
    finished.recv_timeout(Duration::from_secs(5)).unwrap();
    app.poll_runtime_donors();
    assert!(!app.runtime_donors.busy());
    assert!(app.runtime_donors.reports.is_empty());
    assert!(
        app.runtime_donors
            .picker
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .is_some_and(|error| error.contains("without a result"))
    );
    assert_eq!(app.recipe, before);
}

fn binding(binding_hash: u32, owner_tag: u32) -> WeaponRuntimeBinding {
    WeaponRuntimeBinding {
        binding_hash,
        binding_label: binding_label(binding_hash),
        resource_index: 0,
        resource_count: 1,
        owner_tag,
        concrete_class: 0x8080_0001,
        resource_offset: 16,
    }
}

fn shared_owner_graph() -> WeaponRuntimeGraph {
    WeaponRuntimeGraph {
        item_hash: BASELINE,
        pattern_global_id_hash: 7,
        entity_tag: 0x8080_0002,
        bindings: vec![
            binding(BINDING, 0x8080_0100),
            binding(BINDING, 0x8080_0100),
            binding(WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, 0x8080_0100),
            binding(WEAPON_BARREL_COMPONENT_KEY, 0x8080_0200),
        ],
        resources: vec![],
        owners: vec![],
    }
}

#[test]
fn grouped_choices_show_one_effective_source_instead_of_repeating_every_binding() {
    let mut app = app();
    for binding in [BINDING, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY] {
        app.recipe
            .set_runtime_component_donor(binding, Some(reference(SAFE_Z, "Z Safe Donor")));
    }
    app.runtime_graph = Some((
        app.runtime_graph_key().unwrap(),
        Arc::new(shared_owner_graph()),
    ));
    let chosen = ComponentSource::Donors(vec![SourceDonor {
        hash: Some(SAFE_Z),
        route: SourceRoute::Chosen,
    }]);
    assert_eq!(
        app.component_source(BINDING, Some(BASELINE), app.runtime_graph_key().as_ref()),
        chosen
    );
    let mut report = report();
    report.current_sources.insert(
        BINDING,
        [BINDING, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY]
            .into_iter()
            .map(
                |via| crate::runtime::compatibility::EffectiveComponentSource {
                    donor_item_hash: SAFE_Z,
                    via_binding_hash: Some(via),
                },
            )
            .collect(),
    );
    app.runtime_donors.reports.insert(
        BINDING,
        (app.runtime_graph_key().unwrap(), Arc::new(report)),
    );
    assert_eq!(
        app.component_source(BINDING, Some(BASELINE), app.runtime_graph_key().as_ref()),
        chosen
    );
}

#[test]
fn unknown_runtime_rows_do_not_clear_an_applied_gameplay_donor() {
    let mut app = app();
    let baseline_hash = app.runtime_component_baseline_hash(app.runtime_graph_key().as_ref());
    assert_eq!(baseline_hash, None);
    let picker = app.runtime_donors.picker.as_mut().unwrap();
    picker.baseline_hash = baseline_hash;
    picker.selected = Some(BASELINE);
    Arc::make_mut(&mut app.runtime_donors.reports.get_mut(&BINDING).unwrap().1)
        .candidates
        .insert(
            BASELINE,
            ComponentDonorAssessment {
                status: DonorCompatibility::LowerRisk,
                reasons: vec!["Synthetic compatible gameplay donor.".into()],
                affected_bindings: vec![BINDING],
            },
        );
    seed_reviews(&mut app);
    let ctx = egui::Context::default();
    let output = settle(&ctx, &mut app, VIEWPORT);
    click(&ctx, &mut app, label_rect(&output, "Apply Donor").center());
    assert_eq!(
        app.recipe.runtime_component_donor(BINDING),
        Some(&reference(BASELINE, "Base Runtime"))
    );
}

#[test]
fn runtime_baseline_provenance_uses_only_current_graphs_or_verified_rows() {
    let mut app = app();
    let mut graph = shared_owner_graph();
    graph.item_hash = SAFE_A;
    app.runtime_graph = Some((app.runtime_graph_key().unwrap(), Arc::new(graph)));
    assert_eq!(
        app.runtime_component_baseline_hash(app.runtime_graph_key().as_ref()),
        Some(SAFE_A)
    );

    app.recipe.overrides.weapon_pattern_index = Some(8);
    app.recipe.overrides.weapon_pattern_donor_hash = Some(SAFE_A.into());
    assert_eq!(
        app.runtime_component_baseline_hash(app.runtime_graph_key().as_ref()),
        None
    );
    app.donor_summaries
        .iter_mut()
        .find(|donor| donor.hash == SAFE_Z)
        .unwrap()
        .weapon_pattern_index = Some(8);
    assert_eq!(
        app.runtime_component_baseline_hash(app.runtime_graph_key().as_ref()),
        Some(SAFE_Z)
    );

    app.recipe.overrides.weapon_pattern_index = None;
    assert_eq!(
        app.runtime_component_baseline_hash(app.runtime_graph_key().as_ref()),
        Some(BASELINE)
    );
}

#[test]
fn effective_source_labels_expose_shared_owner_donors_despite_requested_baseline() {
    let mut app = app();
    app.recipe.set_runtime_component_donor(
        WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
        Some(reference(SAFE_Z, "Z Safe Donor")),
    );
    app.runtime_graph = Some((
        app.runtime_graph_key().unwrap(),
        Arc::new(shared_owner_graph()),
    ));
    let before = app.recipe.clone();
    assert!(app.recipe.runtime_component_donor(BINDING).is_none());
    assert_eq!(
        app.component_source(BINDING, Some(BASELINE), app.runtime_graph_key().as_ref()),
        ComponentSource::Donors(vec![SourceDonor {
            hash: Some(SAFE_Z),
            route: SourceRoute::Shared(WEAPON_STAT_TRANSLATOR_COMPONENT_KEY),
        }])
    );
    assert_eq!(
        app.component_source(
            WEAPON_BARREL_COMPONENT_KEY,
            Some(BASELINE),
            app.runtime_graph_key().as_ref()
        ),
        ComponentSource::Donors(vec![SourceDonor {
            hash: Some(BASELINE),
            route: SourceRoute::Baseline,
        }])
    );
    let ctx = egui::Context::default();
    let key = app.runtime_graph_key();
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(480.0, 640.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    workbench_style(ui);
                    app.draw_runtime_component_row(
                        ui,
                        &ComponentRow {
                            binding_hash: BINDING,
                            label: "Reload Behavior",
                            tooltip: "",
                            baseline_hash: Some(BASELINE),
                            current_key: key.as_ref(),
                        },
                    );
                });
            },
        );
    }
    let labels = rendered_labels(&output);
    assert!(labels.iter().any(|(text, _)| text == "Reload Behavior"));
    assert!(
        labels
            .iter()
            .any(|(text, _)| text.contains("Z Safe Donor via"))
    );
    assert!(
        !labels
            .iter()
            .any(|(text, _)| text.starts_with("Requested:")),
        "a component with no saved choice has no request to show"
    );
    assert_eq!(app.recipe, before);
}

#[test]
fn effective_source_labels_do_not_reuse_stale_graphs_or_claim_missing_bindings() {
    let mut app = app();
    app.runtime_graph = Some((
        app.runtime_graph_key().unwrap(),
        Arc::new(shared_owner_graph()),
    ));
    assert_eq!(
        app.component_source(
            WEAPON_INPUT_COMPONENT_KEY,
            Some(BASELINE),
            app.runtime_graph_key().as_ref()
        ),
        ComponentSource::Absent
    );
    app.recipe.overrides.weapon_pattern_index = Some(8);
    assert_eq!(
        app.component_source(BINDING, Some(BASELINE), app.runtime_graph_key().as_ref()),
        ComponentSource::NotLoaded
    );
}

#[test]
fn same_baseline_item_or_pattern_donors_do_not_claim_a_shared_owner_change() {
    for requested in [BASELINE, SAFE_Z] {
        let mut app = app();
        app.donor_summaries
            .iter_mut()
            .find(|donor| donor.hash == SAFE_Z)
            .unwrap()
            .weapon_pattern_index = Some(7);
        app.recipe.set_runtime_component_donor(
            WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
            Some(reference(requested, "No-Op Donor")),
        );
        app.runtime_graph = Some((
            app.runtime_graph_key().unwrap(),
            Arc::new(shared_owner_graph()),
        ));
        assert_eq!(
            app.component_source(BINDING, Some(BASELINE), app.runtime_graph_key().as_ref()),
            ComponentSource::Donors(vec![SourceDonor {
                hash: Some(BASELINE),
                route: SourceRoute::Baseline,
            }])
        );
    }
}

#[test]
fn failed_runtime_graph_keeps_saved_donor_repair_controls_visible() {
    for width in [480.0, 1200.0] {
        let ctx = egui::Context::default();
        let mut app = app();
        app.recipe
            .set_runtime_component_donor(BINDING, Some(reference(SAFE_Z, "Z Safe Donor")));
        let key = app.runtime_graph_key().unwrap();
        app.runtime_graph_target = Some(key.clone());
        app.runtime_graph_error = Some((key, "Synthetic incompatible owner".into()));
        assert!(app.runtime_graph.is_none());
        let before = app.recipe.clone();
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 1600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        workbench_style(ui);
                        app.draw_gameplay_parts(ui, None);
                    });
                },
            );
        }
        let labels = rendered_labels(&output);
        assert!(
            labels
                .iter()
                .any(|(text, _)| text.contains("Synthetic incompatible owner"))
        );
        assert!(labels.iter().any(|(text, _)| text == "Remove Saved Choice"));
        assert!(labels.iter().any(|(text, _)| text == "Change…"));
        assert_eq!(app.recipe, before);
        assert!(!app.runtime_donors.busy());
    }
}
