use super::*;
use sundial::package_authoring::sandbox_perk::dependencies::Pattern;

fn fixture() -> Index {
    Index {
        patterns: vec![Pattern {
            index: 0,
            item_hash: 0,
            runtime_key: 0,
            translation_group: 0,
            entity: None,
            error: Some("Inactive pattern row".into()),
        }],
        perks: vec![Perk {
            index: 0,
            hash: 1,
            runtime_key: 0x811C_9DC5,
            action: None,
            graphs: Vec::new(),
            error: None,
            behavior: None,
        }],
        caster: None,
    }
}

#[test]
fn invalidation_retains_worker_until_join_and_discards_stale_data() {
    let (sender, receiver) = mpsc::channel();
    sender.send(Event::Finished(Ok(fixture().into()))).unwrap();
    drop(sender);
    let worker = thread::spawn(|| {});
    let mut browser = Browser {
        job: Some(Job {
            generation: 0,
            receiver,
            worker,
        }),
        ..Default::default()
    };
    browser.invalidate();
    assert!(browser.busy());
    browser.poll();
    assert!(!browser.busy());
    assert!(browser.index.is_none());
}

#[test]
fn dependency_scan_blocks_install_until_package_reader_is_released() {
    let directory = tempfile::tempdir().unwrap();
    let packages = directory.path().join("packages");
    std::fs::create_dir(&packages).unwrap();
    std::fs::write(
        directory.path().join("settings.json"),
        br#"{"version":8,"state":{"account":{"primary_soid":"0x0000000000000001","profile_items":[]},"characters":[]}}"#,
    )
    .unwrap();
    let review = crate::install::test_review(&packages, BTreeSet::new());
    let (sender, receiver) = mpsc::channel();
    let mut app = PackageAuthoringApp {
        replacement_review: Some(Ok(review)),
        runtime_dependencies: Browser {
            job: Some(Job {
                generation: 0,
                receiver,
                worker: thread::spawn(|| {}),
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    app.start_install();
    assert!(app.install_receiver.is_none());
    assert!(
        app.log
            .iter()
            .last()
            .unwrap()
            .text
            .contains("weapon data to finish loading")
    );
    assert!(app.has_background_work());
    drop(sender);
    app.runtime_dependencies.poll();
    assert!(!app.runtime_dependencies.busy());
}
