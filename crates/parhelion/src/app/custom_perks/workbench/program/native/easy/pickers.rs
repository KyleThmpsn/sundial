//! Suggested order in the workbench's pickers, read from the installed game the way a user
//! meets it.
//!
//! The report lists what each picker shows first: the projectile, spawn and attachment pickers,
//! Choose a Condition, Add from Perk and the State list. The checks hold the curated lists to the
//! game data. Every curated perk and state name must still name one, each asset picker must open
//! on an asset a player knows by name, and a search for a super by its game name must find it.
//!
//! Opt in with `PARHELION_WORKBENCH_INSTALL` and `PARHELION_WORKBENCH_CATALOG` (a copy of the
//! catalog cache, never the live one), as the easy perks do. `PARHELION_PICKERS_OUT` names the
//! report directory. With `PARHELION_UI_CAPTURE_DIR` set, the pickers are captured for
//! rendering.
use super::*;
use crate::app::custom_perks::workbench::{assets, guidance};

/// The rows each report section lists.
const SHOWN: usize = 30;

#[test]
#[ignore = "requires PARHELION_WORKBENCH_INSTALL and PARHELION_WORKBENCH_CATALOG"]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn pickers_list_well_known_choices_first() {
    let install = env_path("PARHELION_WORKBENCH_INSTALL");
    let cache = env_path("PARHELION_WORKBENCH_CATALOG");
    let out = std::env::var_os("PARHELION_PICKERS_OUT").map_or_else(
        || std::env::temp_dir().join("parhelion-pickers"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&out).unwrap();
    let weapon = WeaponRecipe::new_unbound("Pickers").expect("an unbound weapon recipe");
    let mut harness = Harness::open(&install, &cache, &weapon);
    harness.app.perk_workbench.refresh_asset_labels();
    let ctx = harness.ctx.clone();
    let screen = egui::vec2(1100.0, 900.0);
    let mut report = String::from("# Suggested Order in the Workbench Pickers\n\n");
    let mut problems = Vec::<String>::new();

    let workbench = &mut harness.app.perk_workbench;
    let browser = assets::Browser {
        catalog: harness.app.catalog.as_ref(),
        discovery: &workbench.discovery,
        perk_names: &workbench.perk_names,
        item_names: &workbench.item_names,
        asset_labels: &workbench.asset_labels,
        markers: None,
        carried: workbench
            .ingredients
            .as_ref()
            .map(|(_, _, ingredients)| &ingredients.sources),
    };

    // The asset pickers open on what a player knows by name.
    for (title, scope) in [
        ("Choose a Projectile", assets::AssetScope::Projectiles),
        (
            "Choose an Object or Effect to Spawn",
            assets::AssetScope::Spawnable,
        ),
        ("Choose an Attachment", assets::AssetScope::Any),
    ] {
        let listed = browser.listing(scope, "");
        let known = listed.iter().filter(|asset| asset.well_known).count();
        writeln!(
            report,
            "## {title}\n\n{} rows, {known} well known.\n",
            listed.len()
        )
        .unwrap();
        write_rows(&mut report, &listed, SHOWN);
        if !listed.first().is_some_and(|asset| asset.well_known) {
            problems.push(format!("{title} does not open on a well-known asset"));
        }
        // Variants fold into their family, so one super no longer fills the first screen.
        let first_screen = listed
            .iter()
            .take(10)
            .filter(|asset| asset.well_known)
            .map(|asset| assets::suggested::rank(&asset.label.to_lowercase()))
            .collect::<BTreeSet<_>>();
        if first_screen.len() < 3 {
            problems.push(format!(
                "{title}'s first ten rows name {} well-known entries",
                first_screen.len()
            ));
        }
    }

    // Each curated entry names assets this installation has, or the report says it does not.
    let every = browser.listing(assets::AssetScope::Any, "");
    let matched = every
        .iter()
        .map(|asset| assets::suggested::rank(&asset.label.to_lowercase()))
        .collect::<BTreeSet<_>>();
    let unmatched = assets::suggested::LEAD
        .iter()
        .enumerate()
        .filter(|(rank, _)| !matched.contains(rank))
        .map(|(_, names)| names[0])
        .collect::<Vec<_>>();
    writeln!(
        report,
        "## Well-Known Assets Not Found\n\n{} of {} entries name no listed asset: {}.\n",
        unmatched.len(),
        assets::suggested::LEAD.len(),
        unmatched.join(", ")
    )
    .unwrap();
    for super_name in ["nova bomb", "hammer of sol", "golden gun", "blade barrage"] {
        if unmatched.contains(&super_name) {
            problems.push(format!("no asset is listed for {super_name}"));
        }
    }

    // A super is found by the name the game gives it.
    for query in ["hammer of sol", "blade barrage", "nova bomb"] {
        let listed = browser.listing(assets::AssetScope::Spawnable, query);
        writeln!(report, "## Search: \"{query}\"\n").unwrap();
        write_rows(&mut report, &listed, 8);
        let first = listed.first().map(|asset| {
            format!(
                "{} {}",
                asset.label,
                asset.game.as_deref().unwrap_or_default()
            )
            .to_lowercase()
        });
        if !first.is_some_and(|first| first.contains(query)) {
            problems.push(format!("a search for \"{query}\" does not lead with it"));
        }
    }

    // The spawn picker as a user sees it: closed, with its first family opened, and searched.
    // Each call draws three frames and keeps the asset the picker returned, if any.
    let spawn = |query: &mut String, events: Vec<egui::Event>, reset: bool| {
        let mut output = None;
        let mut picked = None;
        for events in [events, Vec::new(), Vec::new()] {
            output = Some(ctx.run(input(screen, events), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    crate::app::style::workbench_style(ui);
                    let used = browser.draw(
                        ui,
                        assets::AssetScope::Spawnable,
                        query,
                        reset,
                        Some("Use for Spawn"),
                        None,
                    );
                    picked = picked.take().or(used);
                });
            }));
        }
        (output.unwrap(), picked)
    };
    // Clicks one point `times` times in quick succession, as a double-click does when twice.
    // A pause first, over half a second of frames, keeps an earlier click from counting toward
    // these unless `hurry` asks for exactly that.
    let click = |query: &mut String, at: egui::Pos2, times: usize, hurry: bool| {
        let (mut output, mut picked) = spawn(query, vec![egui::Event::PointerMoved(at)], false);
        for _ in 0..if hurry { 0 } else { 12 } {
            output = spawn(query, Vec::new(), false).0;
        }
        for pressed in std::iter::repeat_n([true, false], times).flatten() {
            let (next, used) = spawn(
                query,
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
                false,
            );
            output = next;
            picked = picked.or(used);
        }
        (output, picked)
    };
    let find = |output: &egui::FullOutput, wanted: &dyn Fn(&str) -> bool| {
        texts(output)
            .into_iter()
            .find(|(text, _)| wanted(text))
            .map(|(_, rect)| rect.center())
    };
    let capture = crate::app::custom_perks::workbench::tests::capture::write;
    let mut query = String::new();
    let (closed, _) = spawn(&mut query, Vec::new(), true);
    capture(&ctx, &closed, "pickers-spawn");
    match find(&closed, &|text| text.contains(" Variants · ")) {
        Some(row) => {
            // A double-click opens a family and uses nothing.
            let (opened, picked) = click(&mut query, row, 2, false);
            capture(&ctx, &opened, "pickers-spawn-open");
            if !texts(&opened).iter().any(|(text, _)| text == "Variant 2") {
                problems.push("double-clicking a family shows no variants".into());
            }
            if picked.is_some() {
                problems.push("double-clicking a family used an asset".into());
            }
            let (fourth, third) = (
                find(&opened, &|text| text == "Variant 4"),
                find(&opened, &|text| text == "Variant 3"),
            );
            match fourth.zip(third) {
                Some((fourth, third)) => {
                    // One click selects a variant. A quick click on the next one is two
                    // clicks on two rows, not a double-click, so it only selects that one.
                    let (_, picked) = click(&mut query, fourth, 1, false);
                    let (_, hurried) = click(&mut query, third, 1, true);
                    if picked.or(hurried).is_some() {
                        problems.push("single clicks on two variants used one".into());
                    }
                    // A double-click uses the variant, as Use for Spawn does.
                    let (_, picked) = click(&mut query, third, 2, false);
                    let label = picked.and_then(|asset| workbench.asset_labels.get(&asset.graph));
                    if !label.is_some_and(|label| label.ends_with(" · Variant 3")) {
                        problems.push(format!("double-clicking Variant 3 used {label:?}"));
                    }
                }
                None => problems.push("an open family shows no Variant 3 and 4".into()),
            }
        }
        None => problems.push("the spawn picker shows no family of variants".into()),
    }
    // Opening the picker on an asset already in use selects it, in its family opened for it.
    let deep = browser
        .listing(assets::AssetScope::Spawnable, "")
        .into_iter()
        .skip(1)
        .find(|row| row.variants >= 3)
        .and_then(|row| {
            let label = format!("{} · Variant {}", row.label, row.variants);
            workbench
                .asset_labels
                .iter()
                .find(|(_, other)| **other == label)
                .map(|(graph, _)| (*graph, label, row.variants))
        });
    match deep {
        Some((graph, label, number)) => {
            let mut query = String::new();
            let mut output = None;
            for frame in 0..3 {
                output = Some(ctx.run(input(screen, Vec::new()), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        crate::app::style::workbench_style(ui);
                        browser.draw(
                            ui,
                            assets::AssetScope::Spawnable,
                            &mut query,
                            frame == 0,
                            Some("Use for Spawn"),
                            Some(graph),
                        );
                    });
                }));
            }
            let output = output.unwrap();
            capture(&ctx, &output, "pickers-spawn-current");
            let shown = texts(&output);
            let row = format!("Variant {number}");
            if !shown.iter().any(|(text, _)| *text == label)
                || !shown.iter().any(|(text, _)| *text == row)
            {
                problems.push(format!(
                    "opening the spawn picker on {label} does not show it"
                ));
            }
        }
        None => problems.push("no family of three variants to open the spawn picker on".into()),
    }
    query = "hammer of sol".into();
    let (searched, _) = spawn(&mut query, Vec::new(), true);
    capture(&ctx, &searched, "pickers-spawn-search");
    if !texts(&searched)
        .iter()
        .any(|(text, _)| text.contains("Part of Hammer of Sol"))
    {
        problems.push("the searched spawn picker never says Part of Hammer of Sol".into());
    }

    // Add from Perk leads with famous perks, all of which the game still carries.
    let choices = workbench
        .ingredients
        .as_ref()
        .map(|(_, _, ingredients)| ingredients.choices.clone())
        .expect("ingredient catalog");
    let stock = choices
        .iter()
        .map(|choice| choice.representative_name.to_lowercase())
        .collect::<BTreeSet<_>>();
    for lead in guidance::EFFECT_LEAD {
        if !stock.contains(&lead.to_lowercase()) {
            problems.push(format!("Add from Perk's lead names no stock perk: {lead}"));
        }
    }
    let sources = workbench
        .ingredients
        .as_ref()
        .map(|(_, _, ingredients)| ingredients.sources.clone())
        .unwrap_or_default();
    let mut effects = choices
        .iter()
        .filter(|choice| {
            workbench
                .discovery
                .behavior(choice.perk_index)
                .is_some_and(|behavior| !behavior.effect_kinds.is_empty())
                && !choice.representative_name.starts_with("Ability ")
                && !choice.representative_name.starts_with("Effect ")
        })
        .collect::<Vec<_>>();
    effects.sort_by_cached_key(|choice| {
        guidance::effect_sort_key(
            guidance::EffectOrder::Suggested,
            &choice.representative_type_name,
            &choice.representative_name.to_lowercase(),
            false,
            guidance::effect_rank(&choice.representative_name, sources.get(&choice.perk_index)),
        )
    });
    writeln!(report, "## Add from Perk\n").unwrap();
    for (position, choice) in effects.iter().take(SHOWN).enumerate() {
        writeln!(
            report,
            "{}. {} ({})",
            position + 1,
            choice.representative_name,
            choice.representative_type_name
        )
        .unwrap();
    }
    report.push('\n');

    // Choose a Condition, opened the way a user opens it.
    let label = "Choose a Condition…";
    let mut condition = |events: Vec<egui::Event>| {
        ctx.run(input(screen, events), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                crate::app::style::workbench_style(ui);
                workbench.behaviors.draw_condition_named(
                    ui,
                    &workbench.discovery,
                    &workbench.perk_names,
                    &workbench.asset_labels,
                    label,
                );
            });
        })
    };
    let first = condition(Vec::new());
    let button = texts(&first)
        .into_iter()
        .find(|(text, _)| text == label)
        .map(|(_, rect)| rect.center())
        .expect("the Choose a Condition button");
    condition(vec![egui::Event::PointerMoved(button)]);
    for pressed in [true, false] {
        condition(vec![egui::Event::PointerButton {
            pos: button,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }]);
    }
    // The stock behaviors are read in the background the first time a picker opens.
    let start = std::time::Instant::now();
    let mut opened = condition(Vec::new());
    while start.elapsed() < std::time::Duration::from_secs(300)
        && !texts(&opened)
            .iter()
            .any(|(text, _)| text == "After a Delay" || text == "On Holster")
    {
        std::thread::sleep(std::time::Duration::from_millis(50));
        opened = condition(Vec::new());
    }
    crate::app::custom_perks::workbench::tests::capture::write(&ctx, &opened, "pickers-condition");
    let mut rows = texts(&opened)
        .into_iter()
        .filter(|(_, rect)| rect.height() < 30.0)
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()));
    writeln!(
        report,
        "## Choose a Condition\n\nText on screen, top to bottom:\n"
    )
    .unwrap();
    for (text, _) in rows.iter().take(SHOWN * 2) {
        writeln!(report, "- {text}").unwrap();
    }
    report.push('\n');
    let position = |title: &str| rows.iter().position(|(text, _)| text == title);
    match (position("After a Delay"), position("On Holster")) {
        (Some(delay), Some(holster)) if delay < holster => {}
        found => problems.push(format!(
            "Choose a Condition does not lead with After a Delay, then On Holster: {found:?}"
        )),
    }

    // The State list leads with everyday states, every one of which the key table names.
    let states = node_fields::keys::known(0x8080_3DCE, 0xD4);
    for lead in super::super::STATE_LEAD {
        if !states.iter().any(|key| key.name == *lead) {
            problems.push(format!("the State list's lead names no state: {lead}"));
        }
    }
    writeln!(
        report,
        "## State\n\n{} states. Leads with: {}.\n",
        states.len(),
        super::super::STATE_LEAD.join(", ")
    )
    .unwrap();

    // The cards the pickers feed, drawn as the workbench draws them: Auto-Loading Holster's and
    // Harbinger's Pulse's Reload from Reserves read their shares by weapon, the declaration 479
    // offers actions like any effect, and Kill Clip keeps what it sets on its card.
    let mut cards = PerkRecipe::new();
    cards.name = "Picker Cards".into();
    for index in [335, 667, 479, 367] {
        cards.effects.push(PerkRecipe::effect(index));
    }
    let mut log = PerkLog {
        number: 90,
        name: cards.name.clone(),
        ..PerkLog::default()
    };
    harness.show(&cards, &mut log);
    let drawn = texts(&harness.settle());
    for wanted in ["This Weapon", "Kinetic Slot", "Energy Slot", "Power Slot"] {
        if !drawn.iter().any(|(text, _)| text == wanted) {
            problems.push(format!(
                "the Reload from Reserves cards never show {wanted}"
            ));
        }
    }
    for unwanted in ["Constant Vector", "Selected Slot", "no standalone action"] {
        if drawn.iter().any(|(text, _)| text.contains(unwanted)) {
            problems.push(format!("a card still shows {unwanted}"));
        }
    }
    writeln!(
        report,
        "## Cards\n\nCaptured as easy-90. Text that points at a problem: {}.\n",
        if log.ui.is_empty() {
            "none".to_owned()
        } else {
            log.ui.join(", ")
        }
    )
    .unwrap();
    problems.extend(log.problems);

    // Add from Perk lists a perk once, and double-clicking it adds all of its effects, as
    // Demolitionist's two.
    let demolitionist = harness
        .app
        .sandbox_perk_choices
        .iter()
        .filter(|choice| choice.representative_name == "Demolitionist")
        .map(|choice| choice.perk_index)
        .collect::<BTreeSet<_>>();
    // Frames advance 1/60 of a second each, so 40 of them first carry an earlier click, such
    // as the one on the search field, past egui's 0.6 second triple-click window.
    let click = |harness: &mut Harness, at: egui::Pos2, times: usize| {
        harness.frame(vec![egui::Event::PointerMoved(at)]);
        for _ in 0..40 {
            harness.frame(Vec::new());
        }
        for pressed in std::iter::repeat_n([true, false], times).flatten() {
            harness.frame(vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }]);
        }
        harness.settle()
    };
    let find = |output: &egui::FullOutput, wanted: &dyn Fn(&str) -> bool| {
        texts(output)
            .into_iter()
            .find(|(text, _)| wanted(text))
            .map(|(_, rect)| rect.center())
    };
    let before = harness.settle();
    match find(&before, &|text| text == "Add from Perk…") {
        Some(button) => {
            let opened = click(&mut harness, button, 1);
            if let Some(search) = find(&opened, &|text| text == "Search Effects or Perks") {
                click(&mut harness, search, 1);
            }
            harness.frame(vec![egui::Event::Text("demolitionist".into())]);
            let searched = harness.settle();
            capture(&ctx, &searched, "pickers-add-from-perk");
            let rows = texts(&searched);
            let families = rows
                .iter()
                .filter(|(text, _)| text.starts_with(&format!("{} Effects", demolitionist.len())))
                .count();
            writeln!(
                report,
                "## Add from Perk: \"demolitionist\"\n\nEffects {demolitionist:?}, {families} perk rows.\n"
            )
            .unwrap();
            // The row's own line, since the details beside it repeat the perk's name.
            let count = format!("{} Effects", demolitionist.len());
            match find(&searched, &|text| text.starts_with(&count)) {
                Some(row) if demolitionist.len() > 1 && families == 1 => {
                    click(&mut harness, row, 2);
                    let workbench = &harness.app.perk_workbench;
                    let added = workbench.documents[workbench.selected]
                        .recipe
                        .effects
                        .iter()
                        .map(|effect| effect.source_perk_index)
                        .collect::<BTreeSet<_>>();
                    if !demolitionist.is_subset(&added) {
                        problems.push(format!(
                            "double-clicking Demolitionist added {:?} of {demolitionist:?}",
                            demolitionist.intersection(&added).collect::<Vec<_>>()
                        ));
                    }
                }
                _ => problems.push(format!(
                    "Add from Perk does not list Demolitionist's {} effects as one row",
                    demolitionist.len()
                )),
            }
        }
        None => problems.push("the workbench shows no Add from Perk… button".into()),
    }

    writeln!(report, "## Problems\n").unwrap();
    if problems.is_empty() {
        report.push_str("None.\n");
    }
    for problem in &problems {
        writeln!(report, "- {problem}").unwrap();
    }
    std::fs::write(out.join("report.md"), &report).unwrap();
    println!("{report}");
    assert!(problems.is_empty(), "{problems:#?}");
}

/// The first rows of a listing, a family with its count and a label with its game name.
fn write_rows(report: &mut String, rows: &[assets::Listed], count: usize) {
    for (position, row) in rows.iter().take(count).enumerate() {
        let variants = if row.variants > 1 {
            format!(" ({} Variants)", row.variants)
        } else {
            String::new()
        };
        let game = row
            .game
            .as_ref()
            .map(|game| format!(" · Part of {game}"))
            .unwrap_or_default();
        writeln!(report, "{}. {}{variants}{game}", position + 1, row.label).unwrap();
    }
    report.push('\n');
}

fn input(screen: egui::Vec2, events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
        events,
        ..Default::default()
    }
}
