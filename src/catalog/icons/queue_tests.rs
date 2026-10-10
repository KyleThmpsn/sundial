use super::*;

#[test]
fn scrolling_defers_excess_requests_without_failure_or_unbounded_pending_work() {
    let (requests, incoming) = mpsc::sync_channel(MAX_PENDING_CATALOG_ICONS);
    let (finished, results) = mpsc::channel();
    let mut runtime = IconRuntime {
        worker: Some(IconWorker {
            requests: Some(requests),
            results,
            thread: None,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }),
        ..Default::default()
    };
    let ctx = eframe::egui::Context::default();
    for hash in 0..1024 {
        runtime.texture(&ctx, Path::new("unused-fixture"), hash, 1);
    }
    assert_eq!(runtime.pending.len(), MAX_PENDING_CATALOG_ICONS);
    assert!(runtime.diagnostic(1023).is_none());
    let first = incoming.try_recv().unwrap();
    finished
        .send(IconLoadResult {
            hash: first.hash,
            loaded: Err("Fixture missing image".into()),
        })
        .unwrap();
    runtime.texture(&ctx, Path::new("unused-fixture"), 1023, 1);
    assert!(runtime.pending.contains(&1023));
    assert_eq!(runtime.pending.len(), MAX_PENDING_CATALOG_ICONS);
    assert!(runtime.worker.is_some());
    assert!(runtime.diagnostic(1023).is_none());
    if let Some(directory) = std::env::var_os("SUNDIAL_TEST_ARTIFACTS") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("catalog-icon-queue.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "scrolled_items":1024,"pending_limit":MAX_PENDING_CATALOG_ICONS,"pending":runtime.pending.len(),"deferred_request_retried":true,
        })).unwrap()).unwrap();
    }
}

#[test]
fn cancellation_finishes_the_active_read_and_discards_the_backlog() {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let (sender, requests) = mpsc::sync_channel(8);
    let (results, finished) = mpsc::sync_channel(8);
    for hash in 0..8 {
        sender
            .send(IconLoadRequest {
                hash,
                container: 1,
                native_size: false,
                layers: IconLayers::All,
            })
            .unwrap();
    }
    drop(sender);
    let mut read = Vec::new();
    run_icon_requests(
        &eframe::egui::Context::default(),
        requests,
        results,
        &cancel,
        |request| {
            read.push(request.hash);
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            Err("Fixture icon missing".into())
        },
    );
    assert_eq!(read, [0]);
    assert_eq!(
        finished
            .into_iter()
            .map(|result| result.hash)
            .collect::<Vec<_>>(),
        [0]
    );
}
