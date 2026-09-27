use super::*;
use sundial::package_authoring::sandbox_perk::dependencies::Pattern;

fn shared_sources() -> PerkSources {
    [(1, "Thorn Catalyst"), (2, "Masterwork Weapon")]
        .into_iter()
        .map(|(hash, name)| {
            (
                453,
                sundial::investment::PerkSource {
                    hash,
                    name: name.into(),
                    type_name: String::new(),
                },
            )
        })
        .collect()
}

#[test]
fn alias_search_returns_shared_identity_and_keeps_unrelated_effects_out() {
    let sources = shared_sources();
    for query in ["thorn catalyst", "masterwork", "453"] {
        assert_eq!(
            selector_rows(Page::Perks, 500, &sources, &[], false, query),
            [(453, "Shared Effect 453".into())]
        );
    }
    assert!(selector_rows(Page::Perks, 500, &sources, &[], false, "outlaw").is_empty());
}

#[test]
fn default_references_preserve_individual_items_and_plugs_within_a_shared_pattern() {
    let usage = |weapon_hash, perk_index, source_plug| PerkPatternUse {
        pattern_index: 12,
        weapon_hash,
        weapon_name: format!("Weapon {weapon_hash}"),
        perk_index,
        source_plug,
    };
    let uses = vec![
        usage(1, 453, Some(2)),
        usage(2, 421, Some(3)),
        usage(3, 453, None),
    ];
    let defaults = details::default_uses(&uses, 453, Some(12));
    assert_eq!(defaults, [uses[0].clone(), uses[2].clone()]);
    assert!(details::default_uses(&uses, 453, Some(13)).is_empty());
}

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
