//! A newcomer's first perks, built only by clicking what the workbench shows: New Perk, Add
//! Effect, the trigger's picker, In Hand, Add Action… and the pickers' search, with a double-click
//! to use a row. Every step is captured and its on-screen text listed in the report, so the cards
//! can be read as a newcomer first reads them.
//!
//! The checks read the perk each journey leaves: a kill whose action happens once ends at once,
//! Outlaw's buff holds for 5 seconds, a new orb drop is the Masterwork orb a player can pick up,
//! and In Hand holds a melee kill to the weapon in hand as Grave Robber's is. A step whose button
//! or row is not on screen is a problem, since a newcomer could not take it either.
//!
//! Opt in with `PARHELION_WORKBENCH_INSTALL` and `PARHELION_WORKBENCH_CATALOG` (a copy of the
//! catalog cache, never the live one), as the easy perks do. `PARHELION_NEWCOMER_OUT` names the
//! report directory, and `PARHELION_UI_CAPTURE_DIR` the captures.
use super::*;

/// Seconds a picker may take to read the stock behaviors the first time it opens.
const LOAD_SECONDS: u64 = 300;

/// One newcomer journey, its steps listed as they are taken.
struct Journey<'a> {
    harness: &'a mut Harness,
    name: &'static str,
    step: usize,
    report: String,
    problems: Vec<String>,
}

impl Journey<'_> {
    fn capture(&mut self, output: &egui::FullOutput, what: &str) {
        self.step += 1;
        let capture = format!("newcomer-{}-{:02}", self.name, self.step);
        crate::app::custom_perks::workbench::tests::capture::write(
            &self.harness.ctx,
            output,
            &capture,
        );
        let mut shown = texts(output)
            .into_iter()
            .filter(|(text, rect)| !text.trim().is_empty() && rect.height() < 40.0)
            .collect::<Vec<_>>();
        shown.sort_by(|a, b| {
            (a.1.top() / 8.0)
                .round()
                .total_cmp(&(b.1.top() / 8.0).round())
                .then(a.1.left().total_cmp(&b.1.left()))
        });
        let shown = shown
            .into_iter()
            .map(|(text, _)| text)
            .collect::<Vec<_>>()
            .join(" | ");
        writeln!(
            self.report,
            "{}. {what} (`{capture}`)\n\n    {shown}\n",
            self.step
        )
        .unwrap();
    }

    /// Clicks one point `times` times. Frames advance 1/60 of a second each, so 40 of them first
    /// carry an earlier click past egui's 0.6 second triple-click window.
    fn click(&mut self, at: egui::Pos2, times: usize) -> egui::FullOutput {
        self.harness.frame(vec![egui::Event::PointerMoved(at)]);
        for _ in 0..40 {
            self.harness.frame(Vec::new());
        }
        for pressed in std::iter::repeat_n([true, false], times).flatten() {
            self.harness.frame(vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }]);
        }
        self.harness.settle()
    }

    /// Frames until text `wanted` accepts is on screen, as a newcomer waits for a picker. The
    /// leftmost match is the one taken: a picker's details repeat the selected row's title to the
    /// right of the list, and only the row uses it.
    fn wait_for(
        &mut self,
        wanted: &dyn Fn(&str) -> bool,
    ) -> Option<(egui::FullOutput, egui::Pos2)> {
        let start = std::time::Instant::now();
        loop {
            let output = self.harness.settle();
            let found = texts(&output)
                .into_iter()
                .filter(|(text, _)| wanted(text))
                .map(|(_, rect)| rect.center())
                .min_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
            if let Some(at) = found {
                return Some((output, at));
            }
            if start.elapsed() > std::time::Duration::from_secs(LOAD_SECONDS) {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// Clicks what a newcomer would, `times` times, or records that it is not on screen.
    fn press(&mut self, what: &str, wanted: &dyn Fn(&str) -> bool, times: usize) -> bool {
        match self.wait_for(wanted) {
            Some((_, at)) => {
                let output = self.click(at, times);
                self.capture(&output, what);
                true
            }
            None => {
                self.problems
                    .push(format!("{}: no {what} on screen", self.name));
                false
            }
        }
    }

    /// Opens a behavior picker from the button `opener` accepts, searches it for `query` and
    /// double-clicks the row `row` accepts, as a newcomer uses one.
    fn pick(
        &mut self,
        opener: &str,
        opens: &dyn Fn(&str) -> bool,
        query: &str,
        row: &dyn Fn(&str) -> bool,
    ) -> bool {
        if !self.press(&format!("Open {opener}"), opens, 1)
            || !self.press(
                "Click the picker's search",
                &|text| text == "Search Behaviors",
                1,
            )
        {
            return false;
        }
        self.harness
            .frame(vec![egui::Event::Text(query.to_owned())]);
        let searched = self.harness.settle();
        self.capture(&searched, &format!("Type \"{query}\""));
        self.press(&format!("Double-click the {query} row"), row, 2)
    }

    /// The effect this journey authored, decoded as the build reads it.
    fn decoded(&self) -> Option<action::DecodedAction> {
        let workbench = &self.harness.app.perk_workbench;
        let effect = workbench.documents[workbench.selected]
            .recipe
            .effects
            .last()?;
        let native = effect.program.as_ref()?.native.as_ref()?;
        action::decode(&native.graph.emit().ok()?).ok()
    }

    /// Starts the journey on a new perk with one new effect, triggered by `trigger`.
    fn start(&mut self, trigger: Trigger) -> bool {
        self.press("Click New Perk", &|text| text == "New Perk", 1)
            && self.press("Click Add Effect", &|text| text == "Add Effect", 1)
            && self.pick(
                "the trigger's picker",
                &|text| text == "On Draw" || text == "While Drawn",
                &trigger.label().to_lowercase(),
                &|text| text == trigger.label(),
            )
    }

    /// Opens the effect's ⋯ menu, the one on the line with the effect's name.
    fn open_effect_menu(&mut self) -> bool {
        let shown = texts(&self.harness.settle());
        let menu = shown
            .iter()
            .find(|(text, _)| text == "New Effect")
            .map(|(_, title)| *title)
            .and_then(|title| {
                shown
                    .iter()
                    .filter(|(text, rect)| {
                        text == crate::app::style::MORE
                            && (rect.center().y - title.center().y).abs() < 12.0
                            && rect.left() > title.left()
                    })
                    .map(|(_, rect)| rect.center())
                    .max_by(|a, b| a.x.total_cmp(&b.x))
            });
        let Some(at) = menu else {
            self.problems
                .push(format!("{}: no effect menu on screen", self.name));
            return false;
        };
        let output = self.click(at, 1);
        self.capture(&output, "Open the effect's menu");
        true
    }

    fn check(&mut self, holds: bool, problem: &str) {
        if !holds {
            self.problems.push(format!("{}: {problem}", self.name));
        }
    }
}

/// The timer seconds a group's ending holds, or none for an ending at once.
fn ending(decoded: &action::DecodedAction) -> Vec<(u8, Option<String>)> {
    decoded.groups.first().map_or_else(Vec::new, |group| {
        group
            .removal
            .iter()
            .map(|node| {
                let seconds = node
                    .facts
                    .iter()
                    .find(|fact| fact.label == "Duration")
                    .map(|fact| fact.value.render());
                (node.kind, seconds)
            })
            .collect()
    })
}

#[test]
#[ignore = "requires PARHELION_WORKBENCH_INSTALL and PARHELION_WORKBENCH_CATALOG"]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn a_newcomer_builds_perks_by_clicking() {
    let install = env_path("PARHELION_WORKBENCH_INSTALL");
    let cache = env_path("PARHELION_WORKBENCH_CATALOG");
    let out = std::env::var_os("PARHELION_NEWCOMER_OUT").map_or_else(
        || std::env::temp_dir().join("parhelion-newcomer"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&out).unwrap();
    let weapon = WeaponRecipe::new_unbound("Newcomer").expect("an unbound weapon recipe");
    let mut harness = Harness::open(&install, &cache, &weapon);
    harness.app.perk_workbench.refresh_asset_labels();
    let mut report = String::from("# A Newcomer's First Perks\n\n");
    let mut problems = Vec::new();

    // A reload on every kill: an action that happens once, so the kill ends at once.
    let mut journey = Journey {
        harness: &mut harness,
        name: "reload",
        step: 0,
        report: String::new(),
        problems: Vec::new(),
    };
    if journey.start(Trigger::WeaponKill) {
        let shown = texts(&journey.harness.settle());
        journey.check(
            shown.iter().any(|(text, _)| text == "At Once"),
            "a kill with no actions does not read At Once",
        );
        if journey.pick(
            "Add Action…",
            &|text| text == "Add Action…",
            "reload from reserves",
            &|text| text == "Reload from Reserves",
        ) {
            let decoded = journey.decoded();
            journey.check(
                decoded.as_ref().is_some_and(|decoded| {
                    decoded.groups[0]
                        .effects
                        .iter()
                        .any(|effect| effect.kind == 16)
                }),
                "Reload from Reserves was not added",
            );
            journey.check(
                decoded
                    .as_ref()
                    .is_some_and(|decoded| ending(decoded) == [(0, None)]),
                "a kill that reloads does not end at once",
            );
        }
    }
    writeln!(report, "## Reload on Every Kill\n\n{}", journey.report).unwrap();
    problems.append(&mut journey.problems);

    // Outlaw's buff on a precision kill: an attachment that ends with the effect, so the kill
    // holds it for 5 seconds.
    let mut journey = Journey {
        harness: &mut harness,
        name: "outlaw",
        step: 0,
        report: String::new(),
        problems: Vec::new(),
    };
    if journey.start(Trigger::PrecisionKill)
        && journey.pick(
            "Add Action…",
            &|text| text == "Add Action…",
            "outlaw",
            &|text| text == "Outlaw's Faster Reload",
        )
    {
        let decoded = journey.decoded();
        journey.check(
            decoded
                .as_ref()
                .is_some_and(|decoded| ending(decoded) == [(1, Some("5 s".to_owned()))]),
            &format!(
                "Outlaw's buff does not last 5 seconds: {:?}",
                decoded.as_ref().map(ending)
            ),
        );
        let shown = texts(&journey.harness.settle());
        journey.check(
            !shown.iter().any(|(text, _)| text == "At Once"),
            "a kill that holds a buff still reads At Once",
        );
    }
    writeln!(
        report,
        "## Outlaw's Buff on a Precision Kill\n\n{}",
        journey.report
    )
    .unwrap();
    problems.append(&mut journey.problems);

    // Orbs on a kill: a new Generate Orbs action drops the Masterwork orb.
    let mut journey = Journey {
        harness: &mut harness,
        name: "orbs",
        step: 0,
        report: String::new(),
        problems: Vec::new(),
    };
    if journey.start(Trigger::WeaponKill)
        && journey.pick(
            "Add Action…",
            &|text| text == "Add Action…",
            "orbs",
            &|text| text == "Generate Orbs of Light",
        )
    {
        let shown = texts(&journey.harness.settle());
        journey.check(
            shown.iter().any(|(text, _)| text == "Masterwork Orb"),
            "a new orb drop does not read Masterwork Orb",
        );
        let decoded = journey.decoded();
        journey.check(
            decoded
                .as_ref()
                .is_some_and(|decoded| ending(decoded) == [(0, None)]),
            "a kill that drops orbs does not end at once",
        );
    }
    writeln!(report, "## Orbs on a Kill\n\n{}", journey.report).unwrap();
    problems.append(&mut journey.problems);

    // A reload on a melee kill with this weapon in hand. On Melee Kill alone counts any melee
    // kill, so In Hand holds it to the weapon in hand, as Grave Robber's trigger does.
    let mut journey = Journey {
        harness: &mut harness,
        name: "melee",
        step: 0,
        report: String::new(),
        problems: Vec::new(),
    };
    if journey.start(Trigger::MeleeKill)
        && journey.press("Check In Hand", &|text| text == "In Hand", 1)
        && journey.pick(
            "Add Action…",
            &|text| text == "Add Action…",
            "reload from reserves",
            &|text| text == "Reload from Reserves",
        )
    {
        let decoded = journey.decoded();
        let trigger = decoded
            .as_ref()
            .and_then(|decoded| decoded.groups.first())
            .and_then(|group| group.activation.first());
        journey.check(
            trigger.is_some_and(structure::held_kill),
            "In Hand did not hold the melee kill to the weapon in hand",
        );
        journey.check(
            decoded
                .as_ref()
                .is_some_and(|decoded| ending(decoded) == [(0, None)]),
            "a melee kill that reloads does not end at once",
        );
    }
    writeln!(
        report,
        "## Reload on a Melee Kill in Hand\n\n{}",
        journey.report
    )
    .unwrap();
    problems.append(&mut journey.problems);

    // A second behavior on its own event. As an extra group it never started in game, so the
    // card says so and Move to Its Own Effect gives it an effect of its own.
    let mut journey = Journey {
        harness: &mut harness,
        name: "group",
        step: 0,
        report: String::new(),
        problems: Vec::new(),
    };
    if journey.start(Trigger::WeaponKill)
        && journey.open_effect_menu()
        && journey.press(
            "Click Add Behavior Group",
            &|text| text == "Add Behavior Group",
            1,
        )
        && journey.pick(
            "the new behavior's trigger",
            &|text| text == "Always",
            &Trigger::GrenadeKill.label().to_lowercase(),
            &|text| text == Trigger::GrenadeKill.label(),
        )
    {
        let shown = texts(&journey.harness.settle());
        journey.check(
            shown
                .iter()
                .any(|(text, _)| text == "Only a main behavior starts on an event in game."),
            "a behavior group started by a grenade kill shows no warning",
        );
        if journey.press(
            "Click Move to Its Own Effect",
            &|text| text == "Move to Its Own Effect",
            1,
        ) {
            let workbench = &journey.harness.app.perk_workbench;
            let effects = &workbench.documents[workbench.selected].recipe.effects;
            let moved = effects
                .last()
                .and_then(|effect| effect.program.as_ref())
                .and_then(|program| program.native.as_ref())
                .and_then(|native| action::decode(&native.graph.emit().ok()?).ok());
            let two = effects.len() == 2;
            journey.check(two, "Move to Its Own Effect did not make a second effect");
            journey.check(
                moved.is_some_and(|decoded| {
                    decoded.groups.len() == 1
                        && decoded.groups[0]
                            .activation
                            .iter()
                            .any(|condition| condition.kind == 2)
                }),
                "the moved behavior is not its own effect's main behavior",
            );
        }
    }
    writeln!(
        report,
        "## A Second Behavior on Its Own Event\n\n{}",
        journey.report
    )
    .unwrap();
    problems.append(&mut journey.problems);

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
