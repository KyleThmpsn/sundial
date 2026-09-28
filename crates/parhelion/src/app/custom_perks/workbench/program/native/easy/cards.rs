//! Every stock effect Add from Perk offers, drawn on its card as a user first sees it and read
//! for words that would stop a reader: a tag, an option by number, an engine name, an unnamed
//! key or a numbered input.
//!
//! The report ranks each kind of text by how many cards show it, with the effects that do, so the
//! commonest leftovers can be named where the evidence allows. The run fails only when drawing
//! a card panics or changes the perk.
//!
//! Opt in with `PARHELION_WORKBENCH_INSTALL` and `PARHELION_WORKBENCH_CATALOG` (a copy of the
//! catalog cache, never the live one), as the easy perks do. `PARHELION_CARDS_OUT` names the
//! report directory.
use super::*;

/// Examples listed beside each text.
const EXAMPLES: usize = 6;
/// Texts listed under each kind.
const SHOWN: usize = 25;

/// The kind of text that would stop a reader, when it is one.
fn kind(text: &str) -> Option<&'static str> {
    let numbered = |prefix: &str| {
        text.strip_prefix(prefix)
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit()))
    };
    let tag = text.match_indices("0x").any(|(at, _)| {
        text[at + 2..]
            .bytes()
            .take_while(u8::is_ascii_hexdigit)
            .count()
            >= 4
    });
    let option = text
        .strip_prefix("Option ")
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
    if tag {
        Some("Tag or hex value")
    } else if option {
        Some("Option by number")
    } else if text.starts_with("Native ") {
        Some("Engine name")
    } else if text.contains("Unnamed") {
        Some("Unnamed key or state")
    } else if numbered("Input ") || numbered("Lane ") {
        Some("Numbered input")
    } else if text == "Not Set" {
        Some("Not set")
    } else {
        None
    }
}

#[test]
#[ignore = "requires PARHELION_WORKBENCH_INSTALL and PARHELION_WORKBENCH_CATALOG"]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn every_stock_card_reads_in_plain_words() {
    let install = env_path("PARHELION_WORKBENCH_INSTALL");
    let cache = env_path("PARHELION_WORKBENCH_CATALOG");
    let out = std::env::var_os("PARHELION_CARDS_OUT").map_or_else(
        || std::env::temp_dir().join("parhelion-cards"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&out).unwrap();
    let weapon = WeaponRecipe::new_unbound("Cards").expect("an unbound weapon recipe");
    let mut harness = Harness::open(&install, &cache, &weapon);
    harness.app.perk_workbench.refresh_asset_labels();
    // Tall enough that a card's last rows are drawn, since a label below the screen paints
    // nothing.
    harness.screen = egui::vec2(1440.0, 6000.0);
    // `PARHELION_CARDS_ONLY` redraws named effects, such as "1377,1693", to follow up a report.
    let only = std::env::var("PARHELION_CARDS_ONLY")
        .ok()
        .map(|list| {
            list.split(',')
                .filter_map(|index| index.trim().parse::<u16>().ok())
                .collect::<BTreeSet<_>>()
        })
        .filter(|only| !only.is_empty());
    let effects = harness
        .app
        .sandbox_perk_choices
        .iter()
        .filter(|choice| {
            only.as_ref()
                .is_none_or(|only| only.contains(&choice.perk_index))
        })
        .map(|choice| (choice.perk_index, choice.representative_name.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut found = BTreeMap::<&str, BTreeMap<String, BTreeSet<u16>>>::new();
    let mut attention = BTreeMap::<String, BTreeSet<u16>>::new();
    let mut problems = Vec::new();
    for &index in effects.keys() {
        let mut perk = PerkRecipe::new();
        perk.name = "Card Audit".into();
        perk.effects.push(PerkRecipe::effect(index));
        // Captures, when asked for, are named by the effect.
        let mut log = PerkLog {
            number: usize::from(index),
            ..PerkLog::default()
        };
        let drawn = harness.show(&perk, &mut log);
        problems.extend(
            log.problems
                .into_iter()
                .map(|problem| format!("Effect {index}: {problem}")),
        );
        for text in log.ui {
            attention.entry(text).or_default().insert(index);
        }
        for (text, _) in drawn.as_ref().map(texts).unwrap_or_default() {
            if let Some(kind) = kind(&text) {
                found
                    .entry(kind)
                    .or_default()
                    .entry(text)
                    .or_default()
                    .insert(index);
            }
        }
    }

    let name = |index: &u16| {
        effects.get(index).map_or_else(
            || format!("Effect {index}"),
            |name| format!("{name} ({index})"),
        )
    };
    let mut report = format!(
        "# Stock Cards in Plain Words\n\n{} effects drawn, one card each.\n\n",
        effects.len()
    );
    let section = |report: &mut String, title: &str, texts: &BTreeMap<String, BTreeSet<u16>>| {
        let cards = texts.values().flatten().collect::<BTreeSet<_>>().len();
        writeln!(
            report,
            "## {title}\n\n{} distinct texts on {cards} cards.\n",
            texts.len()
        )
        .unwrap();
        let mut ranked = texts.iter().collect::<Vec<_>>();
        ranked.sort_by_key(|(text, cards)| (std::cmp::Reverse(cards.len()), (*text).clone()));
        for (text, cards) in ranked.into_iter().take(SHOWN) {
            let examples = cards.iter().take(EXAMPLES).map(name).collect::<Vec<_>>();
            writeln!(
                report,
                "- \"{text}\" on {} cards: {}",
                cards.len(),
                examples.join(", ")
            )
            .unwrap();
        }
        report.push('\n');
    };
    for (kind, texts) in &found {
        section(&mut report, kind, texts);
    }
    section(&mut report, "Error Words", &attention);
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
