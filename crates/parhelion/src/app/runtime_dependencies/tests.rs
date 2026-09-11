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
        }],
        caster: None,
    }
}

fn text(shapes: &[egui::epaint::ClippedShape]) -> String {
    fn append(shape: &egui::Shape, output: &mut String) {
        match shape {
            egui::Shape::Text(text) => {
                output.push_str(&text.galley.job.text);
                output.push('\n');
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    append(shape, output);
                }
            }
            _ => {}
        }
    }
    let mut output = String::new();
    for shape in shapes {
        append(&shape.shape, &mut output);
    }
    output
}

#[test]
fn dependency_browser_preserves_unknowns_and_fits_narrow_windows() {
    for width in [360.0, 780.0] {
        for page in [Page::Perks, Page::Patterns] {
            let original = Arc::new(fixture());
            let mut browser = Browser {
                index: Some(original.clone()),
                page,
                show_unnamed: true,
                ..Default::default()
            };
            let ctx = egui::Context::default();
            let mut labels = String::new();
            for frame in 0..3 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 1600.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let right = ui.max_rect().right();
                            browser.show(ui, &[], &[], None);
                            assert!(
                                ui.min_rect().right() <= right + 1.0,
                                "dependency content overflows at {width}"
                            );
                        });
                    },
                );
                labels = text(&output.shapes);
                if frame == 2 {
                    super::super::ui_tests::build_flow::capture(
                        &ctx,
                        output,
                        &format!("dependencies-{}-{width}", page as u8),
                        width,
                    );
                }
            }
            assert!(!labels.contains("Stock defaults are observations"));
            match page {
                Page::Perks => {
                    assert!(labels.contains("Marker Only"));
                    assert!(labels.contains("What It Needs"));
                    assert!(labels.contains("Unknown. Test with your weapon in game."));
                    assert!(labels.contains("Technical Details"));
                }
                Page::Patterns => assert!(labels.contains("Inactive pattern row")),
            }
            assert_eq!(browser.index.as_ref().unwrap().as_ref(), original.as_ref());
            assert!(!browser.busy());
        }
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
            .contains("runtime-data scan")
    );
    assert!(app.has_background_work());
    drop(sender);
    app.runtime_dependencies.poll();
    assert!(!app.runtime_dependencies.busy());
}
