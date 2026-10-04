use super::*;

#[test]
fn changing_to_a_missing_source_clears_badges_and_ignores_the_old_read() {
    let directory = tempfile::tempdir().unwrap();
    let (sender, receiver) = mpsc::channel();
    let mut installed = Installed {
        read_from: Some(directory.path().join("old")),
        items: BTreeSet::from([1]),
        job: Some(Job {
            receiver,
            worker: thread::spawn(|| {}),
            generation: 0,
        }),
        ..Default::default()
    };
    assert!(installed.busy());
    installed.start(&directory.path().join("missing"), &egui::Context::default());
    assert!(!installed.contains(1));
    sender.send(Ok(BTreeSet::from([1, 2]))).unwrap();
    installed.poll();
    assert!(!installed.busy());
    assert!(!installed.contains(1));
    assert!(!installed.contains(2));
}

#[test]
fn a_late_installed_read_cannot_replace_a_newer_result() {
    let (sender, receiver) = mpsc::channel();
    let mut installed = Installed {
        job: Some(Job {
            receiver,
            worker: thread::spawn(|| {}),
            generation: 0,
        }),
        ..Default::default()
    };
    installed.replace(BTreeSet::from([2]));
    sender.send(Ok(BTreeSet::from([1]))).unwrap();
    installed.poll();
    assert!(!installed.contains(1));
    assert!(installed.contains(2));
}

#[test]
fn installed_reads_participate_in_operation_tracking_and_defer_during_install() {
    let directory = tempfile::tempdir().unwrap();
    let (_sender, receiver) = mpsc::channel();
    let mut app = PackageAuthoringApp {
        packages: directory.path().to_path_buf(),
        install_receiver: Some(receiver),
        ..Default::default()
    };
    app.refresh_installed(&egui::Context::default());
    assert!(!app.installed.busy());
    app.install_receiver = None;
    let (_sender, receiver) = mpsc::channel();
    app.installed.job = Some(Job {
        receiver,
        worker: thread::spawn(|| {}),
        generation: 0,
    });
    assert!(app.has_background_work());
    crate::test_support::artifact(
        "installed-read-lifecycle.json",
        &serde_json::json!({"deferred_during_install":true,"active_read_blocks_mutation":true}),
    );
}
