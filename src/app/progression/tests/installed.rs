use super::*;

#[test]
#[ignore = "Requires SUNDIAL_PROGRESSION_INSTALL and installed packages"]
fn installed_progression_coverage_and_frame_cost() {
    use crate::package_authoring::{open_shadowkeep_package_manager, resolve_live_named_tag};
    use crate::package_payload::{array_at, u32_at};
    let install = std::path::PathBuf::from(
        std::env::var_os("SUNDIAL_PROGRESSION_INSTALL").expect("install path"),
    );
    let cache = std::path::PathBuf::from("examples/progression-ui-check/installed-catalog.json");
    let catalog = Catalog::load_or_scan_with_progress(&install, cache, false, |progress| {
        eprintln!("{}", progress.message)
    })
    .unwrap();
    eprintln!(
        "Scan diagnostics: {:?}",
        catalog.progression_package_error()
    );
    let records = catalog
        .records()
        .expect("authoritative record table must scan");
    for record in records.iter().filter(|record| record.interval_count > 0) {
        assert!(
            record.redeemed_intervals.is_some(),
            "{} must retain its claimed-tier counter even when it also has a completion flag",
            record.name
        );
    }
    let manager = open_shadowkeep_package_manager(&install.join("packages")).unwrap();
    let globals = manager
        .read_tag(resolve_live_named_tag(&manager, "investment_globals", None).unwrap())
        .unwrap();
    let root = manager
        .read_tag(tiger_pkg::TagHash(u32_at(&globals, 16).unwrap()))
        .unwrap();
    let table = manager
        .read_tag(tiger_pkg::TagHash(
            u32_at(
                &root,
                8 + crate::investment_schema::ROOT_RECORD_DEFINITION_TABLE_SLOT * 16,
            )
            .unwrap(),
        ))
        .unwrap();
    let (count, _, _) = array_at(&table, 8).unwrap();
    assert_eq!(
        records.len(),
        count,
        "The browser must retain every native record row"
    );
    for (index, record) in records.iter().enumerate() {
        assert_eq!(record.index, index);
        assert!(
            record
                .objectives
                .iter()
                .all(|index| catalog.objective_definition(*index).is_some())
        );
    }
    eprintln!(
        "Records: {} total, {} named, {} without objectives or completion flags",
        records.len(),
        records.iter().filter(|r| !r.name.trim().is_empty()).count(),
        records
            .iter()
            .filter(|r| r.objectives.is_empty() && r.completion_flag.is_none())
            .count()
    );
    for value in [false, true] {
        let definitions = if value {
            catalog.unlock_value_definitions()
        } else {
            catalog.unlock_flag_definitions()
        };
        let named = (0..definitions.len())
            .filter(|index| labels::unlock(&catalog, *index, value).named)
            .count();
        eprintln!(
            "{}: {} named / {} total",
            if value { "Counters" } else { "Unlocks" },
            named,
            definitions.len()
        );
    }
    let counters = catalog
        .unlock_value_definitions()
        .iter()
        .filter(|definition| definition.bank() == 1)
        .filter_map(|definition| definition.compact_slot)
        .take(6000)
        .map(|slot| json!([slot, 3]))
        .collect::<Vec<_>>();
    let mut document = json!({"state":{"unlocks":{"account_flag_runs":[[0,5000]],"objective_values":counters,"account_progressions":[[0,500,3,8],[1,1500,7,9]]},"investment":{"family5_flag_overrides":[[20,2],[21,1]],"family5_value_overrides":[[20,25],[21,75]]}},"future":true});
    let mut raw = counters
        .iter()
        .map(|row| json!([3, row[0], 0, row[1]]))
        .collect::<Vec<_>>();
    raw.extend((0..5000).map(|slot| json!([0, slot, 0, 2])));
    document["_native_progression"] =
        json!({"unlocks":raw,"family":[[0,20,2],[0,21,1],[1,20,25],[1,21,75]]});
    let before = document.clone();
    let mut state = UiState {
        read_only: true,
        ..Default::default()
    };
    let mut collections = crate::app::collections_page::UiState::default();
    collections.read_only = true;
    for mode in [
        "content",
        "ranks",
        "saved",
        "triumphs",
        "overrides",
        "collections",
    ] {
        table_frames(mode, &mut document, &catalog, &mut state, &mut collections);
        assert_eq!(document, before);
    }
    crate::app::collections_page::bulk_benchmark(&document, &catalog);
    let mut native = document.clone();
    native["_native_progression"] = json!({});
    native["_reward_context"] = json!({"character":0,"character_count":1,"class":3,"pending":[],"consumables":[],"inventory":[],"next_serial":1});
    // Populate a valid unclaimed stage baseline. Generic counter fixtures can exceed a ladder.
    for record in catalog.records().unwrap() {
        if let Some(index) = record.redeemed_intervals {
            if let Some(slot) = catalog
                .unlock_value_definition(usize::from(index))
                .and_then(|definition| definition.compact_slot)
            {
                super::super::mutations::set_unlock_value(
                    &mut native,
                    "objective_values",
                    usize::from(slot),
                    0,
                );
            }
        }
    }
    triumphs::edit_benchmark(&native, &catalog);
}

fn click(pos: egui::Pos2) -> Vec<egui::Event> {
    std::iter::once(egui::Event::PointerMoved(pos))
        .chain(
            [true, false]
                .into_iter()
                .map(|pressed| egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }),
        )
        .collect()
}

fn table_frames(
    mode: &str,
    document: &mut Value,
    catalog: &Catalog,
    state: &mut UiState,
    collections: &mut crate::app::collections_page::UiState,
) {
    let before = document.clone();
    state.reset_navigation();
    state.query.clear();
    let ctx = egui::Context::default();
    state.unlock_browser.tab = match mode {
        "ranks" => unlocks::Tab::Ranks,
        "saved" => unlocks::Tab::Storage,
        _ => unlocks::Tab::Entries,
    };
    let mut timings = Vec::new();
    let mut previous = egui::FullOutput::default();
    let mut frame = 0;
    for tick in 0..250 {
        let events = interaction_events(frame, mode, &previous);
        let start = std::time::Instant::now();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 760.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if mode == "collections" {
                        assert!(!crate::app::collections_page::draw_content(
                            ui,
                            document,
                            catalog,
                            collections
                        ));
                    } else {
                        let view = match mode {
                            "triumphs" => View::Triumphs,
                            "overrides" => View::Investment,
                            _ => View::Unlocks,
                        };
                        assert!(!draw_content(ui, document, catalog, None, state, view));
                    }
                });
            },
        );
        if output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text.starts_with("Preparing "))) {
                eprintln!("{mode}: preparation frame {tick}: {:?}",start.elapsed());
                continue;
            }
        if let Some(query) = match frame {
            7 => Some("exotic"),
            10 => Some("exotic catalyst"),
            _ => None,
        } {
            assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text == query)), "Search must accept typing in {mode}");
        }
        if [0, 6, 7, 10, 13, 18, 23, 24].contains(&frame) {
            eprintln!("{mode}: interaction frame {frame}: {:?}", start.elapsed());
        }
        if frame >= 5 {
            timings.push(start.elapsed());
        }
        assert!(
            output.shapes.len() < 2000,
            "{mode} must paint only the viewport"
        );
        if frame == 34 {
            crate::app::tests::capture::write(&ctx, &output, &format!("installed-{mode}"));
        }
        previous = output;
        frame += 1;
        if frame == 35 {
            break;
        }
    }
    assert_eq!(
        frame, 35,
        "The table must finish preparing and accept interactions"
    );
    timings.sort();
    eprintln!(
        "{mode}: median {:?}, slowest {:?}",
        timings[timings.len() / 2],
        timings.last().unwrap()
    );
    assert_eq!(*document, before);
}

fn interaction_events(frame: usize, mode: &str, previous: &egui::FullOutput) -> Vec<egui::Event> {
    let mut events = Vec::new();
    if frame == 6 {
        let pos = previous
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text)
                    if text.galley.job.text.starts_with("Search")
                        || text.galley.job.text.starts_with("Name, type,")
                        || text.galley.job.text.starts_with("Filter rows") =>
                {
                    Some(text.pos + egui::vec2(15.0, 5.0))
                }
                _ => None,
            })
            .expect("Search must be reachable in every progression table");
        events.extend(click(pos));
    }
    if frame == 7 {
        events.push(egui::Event::Text("exotic".into()));
    }
    if frame == 10 {
        events.push(egui::Event::Text(" catalyst".into()));
    }
    if frame == 13 {
        events.push(egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        });
        events.push(egui::Event::Key {
            key: egui::Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        });
    }
    if frame == 18 {
        events.push(egui::Event::PointerMoved(egui::pos2(550.0, 400.0)));
        events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -600.0),
            modifiers: Default::default(),
        });
    }
    if frame == 23 {
        let labels = match mode {
            "content" => &["Purpose"][..],
            "ranks" => &["Progress", "Rank"][..],
            "triumphs" => &["Progress"][..],
            "collections" => &["Collectible"][..],
            _ => &["Value"][..],
        };
        let pos = previous
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if labels.contains(&text.galley.job.text.as_str()) => {
                    Some(text.pos + egui::vec2(10.0, 5.0))
                }
                _ => None,
            })
            .expect("A sortable header must be reachable");
        events.extend(click(pos));
    }
    events
}
