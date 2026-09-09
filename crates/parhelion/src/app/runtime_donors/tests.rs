//! Headless donor-review contracts. Reports and runtime graphs are synthetic, without packages.
use super::*;
use crate::runtime::compatibility::ComponentDonorAssessment;
use std::sync::atomic::{AtomicBool, Ordering};
use sundial::package_authoring::weapon_runtime::WeaponRuntimeBinding;

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
    app.runtime_donors.picker = Some(Picker {
        binding_hash: BINDING,
        key: Some(key.clone()),
        baseline_hash: Some(BASELINE),
        query: String::new(),
        experimental: false,
        rejected: false,
        selected: None,
        error: None,
    });
    app.runtime_donors
        .reports
        .insert(BINDING, (key, Arc::new(report())));
    app
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

fn candidate_label(name: &str, status: DonorCompatibility) -> String {
    format!("{name} · Sidearm · {}", status_label(status))
}

fn click(ctx: &egui::Context, app: &mut PackageAuthoringApp, position: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            VIEWPORT,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
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
        assert!(labels.iter().any(|(text, _)| text.contains("2 lower risk")));
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
        assert_eq!(app.recipe, before);
        assert!(
            !app.runtime_donors.busy(),
            "cached reports must not start package workers"
        );
    }
}

#[test]
fn selected_donor_review_keeps_apply_visible_in_a_small_viewport_with_long_details() {
    let viewport = egui::vec2(480.0, 640.0);
    let ctx = egui::Context::default();
    let mut app = app();
    app.runtime_donors.picker.as_mut().unwrap().selected = Some(SAFE_Z);
    let report = Arc::make_mut(&mut app.runtime_donors.reports.get_mut(&BINDING).unwrap().1);
    report.current_error = Some(
        "The current combination could not be verified because a previously selected component has a different shared-owner structure. Review the replacement donor and its affected bindings before changing the requested component."
            .into(),
    );
    let assessment = report.candidates.get_mut(&SAFE_Z).unwrap();
    assessment.reasons = vec![
        "The selected donor has a long compatibility explanation covering its native runtime family, animation data, instance schema, definition schema, and dependencies shared with other component bindings. This explanation must remain readable without hiding the apply control."
            .into(),
        "The complete owner contains additional component bindings. The list below must scroll independently so a user can review every affected resource before explicitly applying the selected donor."
            .into(),
    ];
    assessment.affected_bindings = (0..20).map(|index| 0xABCD_0000 + index).collect();
    report.affected_bindings = assessment
        .affected_bindings
        .iter()
        .map(|&hash| {
            (
                hash,
                format!("Additional Shared Runtime Component 0x{hash:08X}"),
            )
        })
        .collect();
    let before = app.recipe.clone();
    let output = settle(&ctx, &mut app, viewport);
    let apply = label_rect(&output, "Apply Donor");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, viewport);
    assert!(
        screen.contains_rect(apply),
        "Apply Donor must remain within {viewport:?}, got {apply:?}"
    );
    fn has_label(shape: &egui::Shape, label: &str) -> bool {
        match shape {
            egui::Shape::Text(text) => text.galley.job.text == label,
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| has_label(shape, label)),
            _ => false,
        }
    }
    let clip = output
        .shapes
        .iter()
        .find(|shape| has_label(&shape.shape, "Apply Donor"))
        .expect("Apply Donor must be rendered")
        .clip_rect;
    assert!(
        clip.contains_rect(apply),
        "Apply Donor must not be clipped, label {apply:?}, clip {clip:?}"
    );
    assert_eq!(app.recipe, before);
    assert!(!app.runtime_donors.busy());
}

#[test]
fn enabling_all_statuses_keeps_lower_risk_matches_first_without_mutating_the_recipe() {
    let ctx = egui::Context::default();
    let mut app = app();
    let before = app.recipe.clone();
    let picker = app.runtime_donors.picker.as_mut().unwrap();
    picker.experimental = true;
    picker.rejected = true;
    let output = settle(&ctx, &mut app, VIEWPORT);
    let labels = [
        candidate_label("A Safe Donor", DonorCompatibility::LowerRisk),
        candidate_label("Z Safe Donor", DonorCompatibility::LowerRisk),
        candidate_label("A Experimental Donor", DonorCompatibility::Experimental),
        candidate_label("A Rejected Donor", DonorCompatibility::Incompatible),
    ];
    let positions = labels.map(|label| label_rect(&output, &label).top());
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(app.recipe, before);
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
            .any(|(text, _)| text == "This selection affects the complete shared owner:")
    );
    click(&ctx, &mut app, label_rect(&output, "Apply Donor").center());
    assert_eq!(
        app.recipe.runtime_component_donor(BINDING),
        Some(&reference(SAFE_Z, "Z Safe Donor"))
    );
    assert!(app.runtime_donors.picker.is_none());
    assert!(app.runtime_donors.reports.is_empty());
    assert!(app.runtime_graph.is_none());
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
        label_rect(&output, "Show Experimental Matches").center(),
    );
    assert!(app.runtime_donors.picker.as_ref().unwrap().experimental);
    assert_eq!(app.recipe, before, "showing candidates must not apply one");
    let output = settle(&ctx, &mut app, VIEWPORT);
    click(&ctx, &mut app, label_rect(&output, "Apply Donor").center());
    assert_eq!(
        app.recipe.runtime_component_donor(BINDING),
        Some(&reference(EXPERIMENTAL, "A Experimental Donor"))
    );
}

#[test]
fn rejected_donor_stays_unapplyable_even_with_both_visibility_options_enabled() {
    let ctx = egui::Context::default();
    let mut app = app();
    let picker = app.runtime_donors.picker.as_mut().unwrap();
    picker.experimental = true;
    picker.rejected = true;
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
            .is_some_and(|error| error.contains("runtime selection changed"))
    );
    assert!(!app.runtime_donors.busy());
    let output = settle(&ctx, &mut app, VIEWPORT);
    assert!(
        !rendered_labels(&output)
            .iter()
            .any(|(text, _)| text == "Apply Donor")
    );
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
    let worker_id = app
        .runtime_donors
        .job
        .as_ref()
        .unwrap()
        .worker
        .thread()
        .id();
    app.runtime_donors.close();
    assert!(app.runtime_donors.picker.is_none());
    assert!(app.runtime_donors.busy());
    assert_eq!(
        app.runtime_donors
            .job
            .as_ref()
            .unwrap()
            .worker
            .thread()
            .id(),
        worker_id
    );
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
    let generation = app.runtime_donors.generation;
    app.runtime_donors.invalidate();
    assert_eq!(app.runtime_donors.generation, generation.wrapping_add(1));
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
fn unknown_runtime_rows_do_not_clear_an_applied_gameplay_donor() {
    let mut app = app();
    let baseline_hash = app.runtime_component_baseline_hash();
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
    assert_eq!(app.runtime_component_baseline_hash(), Some(SAFE_A));

    app.recipe.overrides.weapon_pattern_index = Some(8);
    app.recipe.overrides.weapon_pattern_donor_hash = Some(SAFE_A.into());
    assert_eq!(app.runtime_component_baseline_hash(), None);
    app.donor_summaries
        .iter_mut()
        .find(|donor| donor.hash == SAFE_Z)
        .unwrap()
        .weapon_pattern_index = Some(8);
    assert_eq!(app.runtime_component_baseline_hash(), Some(SAFE_Z));

    app.recipe.overrides.weapon_pattern_index = None;
    assert_eq!(app.runtime_component_baseline_hash(), Some(BASELINE));
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
        app.effective_runtime_source_labels(BINDING, Some(BASELINE)),
        vec![format!(
            "Effective: Z Safe Donor via {} (shared owner)",
            binding_label(WEAPON_STAT_TRANSLATOR_COMPONENT_KEY)
        )]
    );
    assert_eq!(
        app.effective_runtime_source_labels(WEAPON_BARREL_COMPONENT_KEY, Some(BASELINE)),
        vec!["Effective: Base Runtime (baseline)"]
    );
    let ctx = egui::Context::default();
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
                    app.draw_checked_runtime_donor_header(
                        ui,
                        BINDING,
                        "Follow Baseline (Base Runtime)",
                        None,
                        Some(BASELINE),
                    );
                });
            },
        );
    }
    let labels = rendered_labels(&output);
    assert!(
        labels
            .iter()
            .any(|(text, _)| text == "Requested: Follow Baseline (Base Runtime)")
    );
    assert!(
        labels
            .iter()
            .any(|(text, _)| text.contains("Effective: Z Safe Donor via"))
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
    assert!(
        app.effective_runtime_source_labels(WEAPON_INPUT_COMPONENT_KEY, Some(BASELINE))[0]
            .contains("absent")
    );
    app.recipe.overrides.weapon_pattern_index = Some(8);
    let labels = app.effective_runtime_source_labels(BINDING, Some(BASELINE));
    assert_eq!(
        labels,
        vec!["Effective source: Waiting for the current runtime graph."]
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
            app.effective_runtime_source_labels(BINDING, Some(BASELINE)),
            vec!["Effective: Base Runtime (baseline)"]
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
                        app.draw_runtime_component_donors(ui);
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
        assert!(labels.iter().any(|(text, _)| text == "Follow Baseline"));
        assert!(labels.iter().any(|(text, _)| text == "Review Donors…"));
        assert_eq!(app.recipe, before);
        assert!(!app.runtime_donors.busy());
    }
}
