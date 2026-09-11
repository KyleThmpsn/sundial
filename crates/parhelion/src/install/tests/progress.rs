use super::*;

#[test]
fn account_review_reports_its_work_and_finishes_before_backup() {
    let fixture = Fixture::new();
    let mut request = fixture.request();
    fs::write(
        fixture.target.parent().unwrap().join("settings.json"),
        br#"{"version":8}"#,
    )
    .unwrap();
    request.skip_replacement_review = false;
    let mut events = Vec::new();
    install_staged_packages_with_progress(&request, |event| events.push(event)).unwrap();
    let review = events
        .iter()
        .filter(|event| event.phase == InstallPhase::ReviewingAccount)
        .collect::<Vec<_>>();
    assert!(review.len() >= 6);
    assert_eq!(review.first().unwrap().completed, 0);
    let complete = review.last().unwrap();
    assert_eq!(complete.completed, complete.total);
    assert!(
        review
            .windows(2)
            .all(|pair| pair[0].completed < pair[1].completed)
    );
    let review_end = events
        .iter()
        .rposition(|event| event.phase == InstallPhase::ReviewingAccount)
        .unwrap();
    assert!(
        events[..review_end]
            .iter()
            .all(|event| event.phase == InstallPhase::Checking
                || event.phase == InstallPhase::ReviewingAccount)
    );
    assert_eq!(events.last().unwrap().phase, InstallPhase::Complete);
}

#[test]
fn progress_follows_the_verified_transaction_and_reports_every_installed_file() {
    let fixture = Fixture::new();
    let mut events = Vec::new();
    let report =
        install_staged_packages_with_progress(&fixture.request(), |event| events.push(event))
            .unwrap();
    assert_eq!(events.first().unwrap().phase, InstallPhase::Checking);
    assert_eq!(events.last().unwrap().phase, InstallPhase::Complete);
    let mut phases = events.iter().map(|event| event.phase).collect::<Vec<_>>();
    phases.dedup();
    let expected = InstallPhase::STAGES
        .iter()
        .copied()
        .filter(|phase| *phase != InstallPhase::ReviewingAccount)
        .collect::<Vec<_>>();
    assert_eq!(&phases[..phases.len() - 1], &expected);
    for artifact in &report.artifacts {
        assert!(
            events
                .iter()
                .any(|event| event.phase == InstallPhase::Installing
                    && event.current_artifact.as_deref() == Some(&artifact.file_name)
                    && event.completed > 0)
        );
        assert_eq!(
            digest_file(&artifact.target_path).unwrap().sha256,
            artifact.sha256
        );
    }
    let verified = events
        .iter()
        .rfind(|event| event.phase == InstallPhase::Verifying)
        .unwrap();
    assert_eq!(verified.completed, report.artifacts.len());
    assert_eq!(verified.completed, verified.total);
}

#[test]
fn failed_install_reports_recovery_without_claiming_completion() {
    let fixture = Fixture::new();
    let mut events = Vec::new();
    let error = install_with_progress(
        &fixture.request(),
        Some(1),
        DEFAULT_CACHE_INVALIDATION_OPS,
        sundial::package_authoring::replace_file_from_path_atomically,
        &mut |event| events.push(event),
    )
    .unwrap_err();
    assert!(error.rollback.unwrap().succeeded());
    assert_eq!(events.last().unwrap().phase, InstallPhase::RollingBack);
    assert!(
        !events
            .iter()
            .any(|event| event.phase == InstallPhase::Complete)
    );
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        assert!(!fixture.target.join(name).exists());
    }
}
