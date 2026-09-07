use super::*;

fn key(hash: u32) -> RuntimeGraphKey {
    RuntimeGraphKey::new(None, hash, [])
}

fn disconnected_job(key: RuntimeGraphKey) -> RuntimeGraphJob {
    let (sender, receiver) = mpsc::channel();
    drop(sender);
    RuntimeGraphJob {
        key,
        receiver,
        worker: thread::spawn(|| {}),
    }
}

#[test]
fn stale_worker_disconnect_does_not_poison_the_new_selection() {
    let mut app = PackageAuthoringApp {
        runtime_graph_target: Some(key(2)),
        runtime_graph_job: Some(disconnected_job(key(1))),
        ..PackageAuthoringApp::default()
    };
    app.poll_runtime_graph();
    assert!(app.runtime_graph_job.is_none());
    assert!(app.runtime_graph_error.is_none());
    assert_eq!(app.runtime_graph_target, Some(key(2)));
}

#[test]
fn current_worker_disconnect_is_reported_and_releases_the_job() {
    let mut app = PackageAuthoringApp {
        runtime_graph_target: Some(key(1)),
        runtime_graph_job: Some(disconnected_job(key(1))),
        ..PackageAuthoringApp::default()
    };
    app.poll_runtime_graph();
    assert!(app.runtime_graph_job.is_none());
    let (failed, message) = app.runtime_graph_error.unwrap();
    assert_eq!(failed, key(1));
    assert!(message.contains("without a result"));
}

#[test]
fn stale_worker_error_preserves_the_current_error() {
    let (sender, receiver) = mpsc::channel();
    sender.send(Err("old failure".to_owned())).unwrap();
    drop(sender);
    let current_error = Some((key(2), "current failure".to_owned()));
    let mut app = PackageAuthoringApp {
        runtime_graph_target: Some(key(2)),
        runtime_graph_error: current_error.clone(),
        runtime_graph_job: Some(RuntimeGraphJob {
            key: key(1),
            receiver,
            worker: thread::spawn(|| {}),
        }),
        ..PackageAuthoringApp::default()
    };
    app.poll_runtime_graph();
    assert!(app.runtime_graph_job.is_none());
    assert_eq!(app.runtime_graph_error, current_error);
}

#[test]
fn empty_channel_keeps_the_worker_owned_until_completion() {
    let (sender, receiver) = mpsc::channel();
    let mut app = PackageAuthoringApp {
        runtime_graph_target: Some(key(1)),
        runtime_graph_job: Some(RuntimeGraphJob {
            key: key(1),
            receiver,
            worker: thread::spawn(|| {}),
        }),
        ..PackageAuthoringApp::default()
    };
    app.poll_runtime_graph();
    assert!(app.runtime_graph_job.is_some());
    assert!(app.runtime_graph_error.is_none());
    assert!(app.has_background_work());
    drop(sender);
    app.poll_runtime_graph();
    assert!(app.runtime_graph_job.is_none());
}
