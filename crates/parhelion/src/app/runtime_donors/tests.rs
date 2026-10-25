//! Headless donor-review contracts. Reports and runtime graphs are synthetic, without packages.
use super::*;
use crate::runtime::compatibility::ComponentDonorAssessment;
use std::sync::atomic::{AtomicBool, Ordering};

const BASELINE: u32 = 0x10;
const SAFE_Z: u32 = 0x20;
const SAFE_A: u32 = 0x21;
const EXPERIMENTAL: u32 = 0x30;
const REJECTED: u32 = 0x40;
const BINDING: u32 = WEAPON_RELOAD_COMPONENT_KEY;

fn donor(hash: u32, name: &str) -> WeaponDonorSummary {
    crate::test_support::donor_summary(hash, name, "Sidearm")
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
