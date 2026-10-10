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
    assert!(review.len() >= 2, "review must report start and completion");
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
    let boundaries = [
        InstallPhase::Checking,
        InstallPhase::BackingUp,
        InstallPhase::Preparing,
        InstallPhase::Rechecking,
        InstallPhase::Installing,
        InstallPhase::Verifying,
        InstallPhase::RefreshingCaches,
        InstallPhase::Complete,
    ];
    let positions: Vec<_> = boundaries
        .iter()
        .map(|phase| {
            events
                .iter()
                .position(|event| event.phase == *phase)
                .expect("transaction boundary must be reported")
        })
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    for artifact in &report.artifacts {
        assert!(
            events
                .iter()
                .any(|event| event.phase == InstallPhase::Installing
                    && event.current_artifact.as_deref() == Some(&artifact.file_name)
                    && event.completed > 0)
        );
        assert_eq!(
            {
                use sha2::Digest as _;
                hex::encode_upper(sha2::Sha256::digest(
                    &fixture.staged_bytes[&artifact.file_name],
                ))
            },
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
        publish_package,
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
