use super::*;

#[test]
fn background_export_publishes_complete_json_and_preserves_destination_on_failure() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("native-map.json");
    std::fs::write(&path, b"previous export").unwrap();
    let expected = serde_json::json!({"nodes": (0..2000).collect::<Vec<_>>()});
    let mut job = Export::default();
    job.start(path.clone(), expected.clone(), &egui::Context::default());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while job.busy() && std::time::Instant::now() < deadline {
        job.poll();
        std::thread::yield_now();
    }
    assert!(job.result.as_ref().unwrap().is_ok());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).unwrap(),
        expected
    );

    struct Failure;
    impl serde::Serialize for Failure {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom(
                "intentional serialization failure",
            ))
        }
    }
    let baseline = std::fs::read(&path).unwrap();
    job.start(path.clone(), Failure, &egui::Context::default());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while job.busy() && std::time::Instant::now() < deadline {
        job.poll();
        std::thread::yield_now();
    }
    assert!(job.result.as_ref().unwrap().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), baseline);
    crate::test_support::artifact(
        "perk-background-export.json",
        &serde_json::json!({
            "export": expected, "failed_export_preserved_previous_file": true
        }),
    );
}

#[test]
fn cancellation_before_publication_keeps_the_existing_export() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("native-map.json");
    std::fs::write(&path, b"previous export").unwrap();
    let (entered, observed) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    struct Paused {
        entered: std::sync::mpsc::Sender<()>,
        wait: std::sync::mpsc::Receiver<()>,
    }
    impl serde::Serialize for Paused {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            self.entered.send(()).unwrap();
            self.wait.recv().unwrap();
            serializer.serialize_str("complete new export")
        }
    }
    let mut job = Export::default();
    job.start(
        path.clone(),
        Paused { entered, wait },
        &egui::Context::default(),
    );
    observed
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    job.stop();
    assert!(job.busy());
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while job.busy() && std::time::Instant::now() < deadline {
        job.poll();
        std::thread::yield_now();
    }
    assert!(job.result.as_ref().unwrap().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"previous export");
}
