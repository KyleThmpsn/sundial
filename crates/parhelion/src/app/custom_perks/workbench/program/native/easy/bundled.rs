//! The rows this release added to the workbench, met headlessly the way a user meets them: the
//! bundled Everything at Once perk opened from the Custom Perks list with every card expanded
//! and paged through, the trigger and end condition pickers searched for the promoted
//! conditions, the super energy multiplier hovered for its hint, Resync Account found on the
//! Tools menu, and the spawn picker searched for a movement ability and for a Warmind Cell effect.
//! Every screen is captured and its text scanned for a number standing in for a name.
//!
//! Opt in with `PARHELION_WORKBENCH_INSTALL` and `PARHELION_WORKBENCH_CATALOG` (a copy of the
//! catalog cache, never the live one), as the easy perks do. `PARHELION_BUNDLED_OUT` names the
//! report directory, and `PARHELION_UI_CAPTURE_DIR` the captures.
use super::*;
use crate::app::custom_perks::workbench::assets;

/// Seconds a picker may take to read the stock behaviors the first time it opens.
const LOAD_SECONDS: u64 = 300;
const PERK: &str = "Everything at Once";
/// The hint the super energy multiplier shows on hover.
const MULTIPLIER_HINT: &str = "Multiplies the value.";
/// The hint the spawn picker shows for a search that names an ability.
const ABILITY_HINT: &str = "Abilities are on the ability triggers and actions.";
/// The hover text of Resync Account.
const RESYNC_HINT: &str = "Apply the installed unlocks and items to the current account again.";

/// One headless walk through the workbench, its steps listed as they are taken.
struct Walk<'a> {
    harness: &'a mut Harness,
    step: usize,
    report: String,
    problems: Vec<String>,
}

impl Walk<'_> {
    /// Captures a frame and lists its text in the report, top to bottom and left to right.
    fn capture(&mut self, output: &egui::FullOutput, what: &str) -> Vec<String> {
        self.step += 1;
        let capture = format!("bundled-{:02}", self.step);
        crate::app::custom_perks::workbench::tests::capture::write(
            &self.harness.ctx,
            output,
            &capture,
        );
        let mut shown = texts(output)
            .into_iter()
            .filter(|(text, _)| !text.trim().is_empty())
            .collect::<Vec<_>>();
        shown.sort_by(|a, b| {
            (a.1.top() / 8.0)
                .round()
                .total_cmp(&(b.1.top() / 8.0).round())
                .then(a.1.left().total_cmp(&b.1.left()))
        });
        let shown = shown
            .into_iter()
            .map(|(text, _)| text.replace('\n', " "))
            .collect::<Vec<_>>();
        writeln!(
            self.report,
            "{}. {what} (`{capture}`)\n\n    {}\n",
            self.step,
            shown.join(" | ")
        )
        .unwrap();
        shown
    }

    /// Frames advance 1/60 of a second each, so 40 of them carry an earlier click past egui's
    /// 0.6 second triple-click window.
    fn pause(&mut self) {
        for _ in 0..40 {
            self.harness.frame(Vec::new());
        }
    }

    /// Clicks one point `times` times, after a pause that keeps an earlier click apart.
    fn click(&mut self, at: egui::Pos2, times: usize) -> egui::FullOutput {
        self.harness.frame(vec![egui::Event::PointerMoved(at)]);
        self.pause();
        self.click_now(at, times)
    }

    /// Clicks one point `times` times at once.
    fn click_now(&mut self, at: egui::Pos2, times: usize) -> egui::FullOutput {
        self.harness.frame(vec![egui::Event::PointerMoved(at)]);
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

    /// Holds the pointer still over `at` long enough for a tooltip to open.
    fn hover(&mut self, at: egui::Pos2) -> egui::FullOutput {
        self.harness.frame(vec![egui::Event::PointerMoved(at)]);
        for _ in 0..70 {
            self.harness.frame(Vec::new());
        }
        self.harness.frame(Vec::new())
    }

    /// One wheel turn over `at`, then the pointer leaves so no tooltip opens over the frame.
    fn scroll(&mut self, at: egui::Pos2, delta: f32) -> egui::FullOutput {
        self.harness.frame(vec![egui::Event::PointerMoved(at)]);
        self.harness.frame(vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, delta),
            modifiers: egui::Modifiers::NONE,
        }]);
        for _ in 0..20 {
            self.harness.frame(Vec::new());
        }
        self.harness.frame(vec![egui::Event::PointerGone]);
        self.harness.settle()
    }

    /// Frames until text `wanted` accepts is on screen, as a user waits for a picker. The
    /// leftmost match is the one taken: a picker's details repeat the selected row's title to
    /// the right of the list, and only the row uses it.
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

    /// Clicks what a user would, `times` times, or records that it is not on screen. The
    /// target is found again after the pause: a picker still reading the stock behaviors can
    /// reorder its rows in the meantime.
    fn press(&mut self, what: &str, wanted: &dyn Fn(&str) -> bool, times: usize) -> bool {
        if self.wait_for(wanted).is_none() {
            self.problems.push(format!("no {what} on screen"));
            return false;
        }
        self.pause();
        match self.wait_for(wanted) {
            Some((_, at)) => {
                let output = self.click_now(at, times);
                let shown = self.capture(&output, what);
                self.scan(&shown, what);
                true
            }
            None => {
                self.problems
                    .push(format!("{what} left the screen before it was clicked"));
                false
            }
        }
    }

    /// The center of the control drawn under the label `label`, which starts at the label's
    /// left edge, when `accepts` takes its text.
    fn under(&mut self, label: &str, accepts: &dyn Fn(&str) -> bool) -> Option<egui::Pos2> {
        let shown = texts(&self.harness.settle());
        let label = shown
            .iter()
            .find(|(text, _)| text == label)
            .map(|(_, rect)| *rect)?;
        shown
            .iter()
            .filter(|(text, rect)| {
                rect.top() > label.bottom() - 2.0
                    && rect.top() < label.bottom() + 30.0
                    && rect.center().x > label.left()
                    && rect.center().x < label.left() + 240.0
                    && accepts(text)
            })
            .map(|(_, rect)| rect.center())
            .min_by(|a, b| a.x.total_cmp(&b.x))
    }

    /// Opens a behavior picker from the button `opens` accepts, searches it for `query` and
    /// double-clicks the row `row` accepts, as a user does. A row whose title names the query
    /// must lead the results, since such a search is answered with its title matches first.
    fn pick(
        &mut self,
        opener: &str,
        opens: &dyn Fn(&str) -> bool,
        query: &str,
        row: &dyn Fn(&str) -> bool,
    ) -> bool {
        match self.wait_for(opens) {
            Some((_, at)) => self.pick_at(opener, at, query, row),
            None => {
                self.problems.push(format!("no {opener} on screen"));
                false
            }
        }
    }

    /// Opens a behavior picker from the button at `at`, searches it for `query` and
    /// double-clicks the row `row` accepts.
    fn pick_at(
        &mut self,
        opener: &str,
        at: egui::Pos2,
        query: &str,
        row: &dyn Fn(&str) -> bool,
    ) -> bool {
        let output = self.click(at, 1);
        let shown = self.capture(&output, &format!("Open {opener}"));
        self.scan(&shown, opener);
        if !self.press(
            "Click the picker's search",
            &|text| text == "Search Behaviors",
            1,
        ) {
            return false;
        }
        self.harness
            .frame(vec![egui::Event::Text(query.to_owned())]);
        let searched = self.harness.settle();
        let shown = self.capture(&searched, &format!("Type \"{query}\""));
        self.scan(&shown, &format!("the picker searched for \"{query}\""));
        // The list's rows share the search field's left edge and sit under its toolbar. The
        // topmost text there is the first row's title.
        let placed = texts(&searched);
        let field = placed
            .iter()
            .find(|(text, _)| text == query)
            .map(|(_, rect)| *rect);
        let first = field.and_then(|field| {
            placed
                .iter()
                .filter(|(_, rect)| {
                    (rect.left() - field.left()).abs() < 24.0 && rect.top() > field.bottom() + 20.0
                })
                .min_by(|a, b| a.1.top().total_cmp(&b.1.top()))
                .map(|(text, _)| text.clone())
        });
        // Two titles can share the words, as Ends on a Game Signal shares "ends on", so the
        // first row must name the query in its title rather than be the one taken.
        self.check(
            first
                .as_deref()
                .is_some_and(|title| crate::app::pickers::matches(query, title)),
            &format!("a search for \"{query}\" leads with {first:?}, which does not name it"),
        );
        self.press(&format!("Double-click the \"{query}\" row"), row, 2)
    }

    /// The button on the End Condition row, labeled with the ending in place, which opens the
    /// end condition picker.
    fn ending_button(&mut self) -> Option<egui::Pos2> {
        let shown = texts(&self.harness.settle());
        let heading = shown
            .iter()
            .find(|(text, _)| text == "End Condition")
            .map(|(_, rect)| *rect)?;
        shown
            .iter()
            .filter(|(text, rect)| {
                (rect.center().y - heading.center().y).abs() < 10.0
                    && rect.left() > heading.right()
                    && !text.trim().is_empty()
                    && text != crate::app::style::MORE
                    && !text.starts_with("Or")
                    && !text.starts_with("And")
            })
            .map(|(_, rect)| rect.center())
            .min_by(|a, b| a.x.total_cmp(&b.x))
    }

    /// The effect the selected document ends with, decoded as the build reads it.
    fn decoded(&self) -> Option<action::DecodedAction> {
        let workbench = &self.harness.app.perk_workbench;
        let effect = workbench.documents[workbench.selected]
            .recipe
            .effects
            .last()?;
        let native = effect.program.as_ref()?.native.as_ref()?;
        action::decode(&native.graph.emit().ok()?).ok()
    }

    /// Whether the last effect's groups carry a condition of `kind` in any role.
    fn carries(&self, kind: u8) -> bool {
        self.decoded().is_some_and(|decoded| {
            decoded.groups.iter().any(|group| {
                group
                    .activation
                    .iter()
                    .chain(&group.removal)
                    .chain(&group.rearm)
                    .any(|condition| condition.kind == kind)
            })
        })
    }

    fn check(&mut self, holds: bool, problem: &str) {
        if !holds {
            self.problems.push(problem.to_owned());
        }
    }

    /// Records text that stands in for a name or reports a failure, as a user would read it.
    fn scan(&mut self, shown: &[String], context: &str) {
        for text in shown {
            let lower = text.to_lowercase();
            let words = lower
                .split(|c: char| !c.is_alphanumeric())
                .collect::<BTreeSet<_>>();
            let bare = text.split(|c: char| !c.is_alphanumeric()).any(|word| {
                word.len() == 10
                    && word.starts_with("0x")
                    && word[2..].chars().all(|c| c.is_ascii_hexdigit())
            });
            let numbered_option = text
                .split("Option ")
                .skip(1)
                .any(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
            if ["error", "errors", "invalid", "failed", "panicked"]
                .iter()
                .any(|word| words.contains(word))
                || ["could not", "needs attention", "unnamed", "unidentified"]
                    .iter()
                    .any(|phrase| lower.contains(phrase))
                || bare
                || numbered_option
            {
                self.problems
                    .push(format!("{context}: \"{}\"", text.replace('\n', " ")));
            }
        }
    }

    /// A point inside the selected document's body, right of the Custom Perks list.
    fn body(&self) -> egui::Pos2 {
        let window = self
            .harness
            .ctx
            .memory(|memory| memory.area_rect(egui::Id::new("global-custom-perk-workbench")))
            .unwrap_or(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                self.harness.screen,
            ));
        egui::pos2(window.right() - 320.0, window.center().y + 80.0)
    }

    /// Pages through the document from the top, capturing each page, until a page adds no
    /// text. Returns every text shown on any page with the page it was first seen on.
    fn pages(&mut self, what: &str) -> Vec<(String, egui::Rect, usize)> {
        let at = self.body();
        let mut output = self.scroll(at, 8000.0);
        let mut seen = Vec::<(String, egui::Rect, usize)>::new();
        for page in 1..=24 {
            let shown = self.capture(&output, &format!("{what}, page {page}"));
            self.scan(&shown, &format!("{what}, page {page}"));
            let before = seen.len();
            for (text, rect) in texts(&output) {
                if !seen.iter().any(|(known, _, _)| *known == text) {
                    seen.push((text, rect, page));
                }
            }
            if seen.len() == before && page > 1 {
                break;
            }
            output = self.scroll(at, -600.0);
        }
        seen
    }

    /// Scrolls the document from the top until text `wanted` accepts is on screen.
    fn reveal(&mut self, wanted: &dyn Fn(&str) -> bool) -> Option<egui::FullOutput> {
        let at = self.body();
        let mut output = self.scroll(at, 8000.0);
        let mut last = Vec::new();
        for _ in 0..24 {
            let shown = texts(&output);
            if shown.iter().any(|(text, _)| wanted(text)) {
                return Some(output);
            }
            let names = shown
                .iter()
                .map(|(text, _)| text.clone())
                .collect::<Vec<_>>();
            if names == last {
                return None;
            }
            last = names;
            output = self.scroll(at, -600.0);
        }
        None
    }
}

/// A frame that draws the app's Tools menu above the workbench, as the main window does.
fn menu_frame(harness: &mut Harness, events: Vec<egui::Event>) -> egui::FullOutput {
    let app = &mut harness.app;
    let screen = harness.screen;
    harness.ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::TopBottomPanel::top("bundled-tools-menu").show(ctx, |ui| {
                egui::menu::bar(ui, |ui| app.draw_tools_menu(ui));
            });
            egui::CentralPanel::default().show(ctx, |_| {});
        },
    )
}

#[test]
#[ignore = "requires PARHELION_WORKBENCH_INSTALL and PARHELION_WORKBENCH_CATALOG"]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn the_new_rows_read_by_name_on_every_screen() {
    let install = env_path("PARHELION_WORKBENCH_INSTALL");
    let cache = env_path("PARHELION_WORKBENCH_CATALOG");
    let out = std::env::var_os("PARHELION_BUNDLED_OUT").map_or_else(
        || std::env::temp_dir().join("parhelion-bundled"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&out).unwrap();
    let weapon = WeaponRecipe::new_unbound("Bundled").expect("an unbound weapon recipe");
    let mut harness = Harness::open(&install, &cache, &weapon);
    harness.app.perk_workbench.refresh_asset_labels();
    let mut walk = Walk {
        harness: &mut harness,
        step: 0,
        report: String::from("# The New Rows, Met in the Workbench\n\n"),
        problems: Vec::new(),
    };

    // The bundled perk shares the icon the other bundled perks carry.
    let icon = |name: &str| {
        walk.harness
            .app
            .perk_workbench
            .entries
            .iter()
            .find(|entry| entry.recipe.name == name)
            .map(|entry| entry.recipe.icon.clone())
    };
    let (shared, everything) = (icon("Borrowed Time"), icon(PERK));
    walk.check(
        shared.is_some() && shared == everything,
        &format!("{PERK} carries icon {everything:?}, Borrowed Time {shared:?}"),
    );

    // Resync Account sits on the Tools menu with its hover text.
    let output = menu_frame(walk.harness, Vec::new());
    let tools = texts(&output)
        .into_iter()
        .find(|(text, _)| text == "Tools")
        .map(|(_, rect)| rect.center());
    match tools {
        Some(at) => {
            menu_frame(walk.harness, vec![egui::Event::PointerMoved(at)]);
            for pressed in [true, false] {
                menu_frame(
                    walk.harness,
                    vec![egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
            let opened = menu_frame(walk.harness, Vec::new());
            let shown = walk.capture(&opened, "Open the Tools menu");
            walk.scan(&shown, "the Tools menu");
            let resync = texts(&opened)
                .into_iter()
                .find(|(text, _)| text == "Resync Account")
                .map(|(_, rect)| rect.center());
            walk.check(resync.is_some(), "no Resync Account on the Tools menu");
            walk.check(
                walk.harness.app.account_resync_receiver.is_none(),
                "Resync Account is disabled with no resync running",
            );
            if let Some(at) = resync {
                menu_frame(walk.harness, vec![egui::Event::PointerMoved(at)]);
                let mut hovered = menu_frame(walk.harness, Vec::new());
                for _ in 0..70 {
                    hovered = menu_frame(walk.harness, Vec::new());
                }
                let shown = walk.capture(&hovered, "Hover Resync Account");
                walk.check(
                    shown.iter().any(|text| text == RESYNC_HINT),
                    "hovering Resync Account shows no hint",
                );
            }
            menu_frame(
                walk.harness,
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            menu_frame(walk.harness, Vec::new());
        }
        None => walk.problems.push("no Tools menu on screen".into()),
    }

    // The promoted conditions are found by name in the pickers a new effect offers.
    let opened = walk.harness.settle();
    let shown = walk.capture(&opened, "The workbench with a new perk");
    walk.scan(&shown, "the workbench");
    if walk.press("Click Add Effect", &|text| text == "Add Effect", 1)
        && walk.pick(
            "the trigger's picker",
            &|text| text == "On Draw" || text == "While Drawn",
            "swap",
            &|text| text == "On Weapon Swap",
        )
    {
        walk.check(
            walk.carries(18),
            "choosing On Weapon Swap does not place a weapon swap condition",
        );
        if walk.pick(
            "the trigger's picker again",
            &|text| text == "On Weapon Swap",
            "releas",
            &|text| text == "On Releasing the Trigger",
        ) {
            walk.check(
                walk.carries(13),
                "choosing On Releasing the Trigger does not place a trigger release condition",
            );
        }
        match walk.ending_button() {
            Some(at)
                if walk.pick_at("the end condition picker", at, "ends on", &|text| {
                    text == "Ends on a Specific Ability"
                }) =>
            {
                walk.check(
                    walk.carries(11),
                    "choosing Ends on a Specific Ability does not place an ability ending",
                );
                // The ending's ability is chosen by name from the stock grenades and Supers.
                match walk.under("Ability", &|text| text == "None") {
                    Some(at) => {
                        let opened = walk.click(at, 1);
                        let shown = walk.capture(&opened, "Open the ending's Ability choice");
                        walk.scan(&shown, "the Ability choice");
                        if walk.press("Click Fusion Grenade", &|text| text == "Fusion Grenade", 1) {
                            let stored = walk.decoded().and_then(|decoded| {
                                decoded.groups.iter().find_map(|group| {
                                    group
                                        .removal
                                        .iter()
                                        .find(|condition| condition.kind == 11)
                                        .and_then(|condition| {
                                            condition.native.get(0x10..0x14).map(|bytes| {
                                                u32::from_le_bytes(bytes.try_into().unwrap())
                                            })
                                        })
                                })
                            });
                            walk.check(
                                stored == Some(0x80B8_0B2C),
                                &format!(
                                    "choosing Fusion Grenade stored {stored:08X?} rather than the thermal_flux pattern"
                                ),
                            );
                        }
                    }
                    None => walk
                        .problems
                        .push("the ending offers no Ability choice reading None".into()),
                }
            }
            Some(_) => {}
            None => walk
                .problems
                .push("no end condition button on the new effect".into()),
        }
    }

    // The bundled perk opens from the list, and every card reads by name.
    if walk.press(
        &format!("Click {PERK} in the list"),
        &|text| text == PERK,
        1,
    ) {
        let workbench = &walk.harness.app.perk_workbench;
        let recipe = workbench.documents[workbench.selected].recipe.clone();
        walk.check(
            recipe.name == PERK,
            &format!("clicking {PERK} opened \"{}\"", recipe.name),
        );
        if let Err(error) = recipe.validate() {
            walk.problems
                .push(format!("{PERK} does not validate: {error}"));
        }
        let count = recipe.effects.len();
        for (position, effect) in recipe.effects.iter().enumerate() {
            crate::app::custom_perks::workbench::cards::Card::new(
                &recipe.id,
                effect.source_perk_index,
                position,
                count,
            )
            .set_expanded(&walk.harness.ctx, true);
        }
        walk.harness.settle();
        let seen = walk.pages(&format!("{PERK} expanded"));
        writeln!(
            walk.report,
            "{PERK} shows {} distinct texts over its pages.\n",
            seen.len()
        )
        .unwrap();
        for wanted in [
            "Devour",
            "Truesight",
            "Firefly",
            "Cellular Suppression",
            "Multiplier",
        ] {
            walk.check(
                seen.iter().any(|(text, _, _)| text.contains(wanted)),
                &format!("{PERK}'s cards never show \"{wanted}\""),
            );
        }
        let after = {
            let workbench = &walk.harness.app.perk_workbench;
            workbench.documents[workbench.selected].recipe.clone()
        };
        walk.check(
            after == recipe,
            &format!("drawing {PERK} changed it: {}", changes(&recipe, &after)),
        );

        // The super energy multiplier explains its scale on hover.
        match walk.reveal(&|text| text == "Multiplier") {
            Some(_) => {
                let value = walk.under("Multiplier", &|text| {
                    text.trim()
                        .trim_end_matches(|c: char| c.is_alphabetic() || c == ' ')
                        .parse::<f32>()
                        .is_ok()
                });
                match value {
                    Some(at) => {
                        let hovered = walk.hover(at);
                        let shown = walk.capture(&hovered, "Hover the energy Multiplier");
                        walk.check(
                            shown.iter().any(|text| text.starts_with(MULTIPLIER_HINT)),
                            "hovering the Multiplier shows no hint",
                        );
                    }
                    None => walk
                        .problems
                        .push("no value sits under the Multiplier label".into()),
                }
            }
            None => walk
                .problems
                .push(format!("{PERK} shows no Multiplier row on any page")),
        }
    }

    // The spawn picker turns an ability search into a pointer and finds a Warmind effect.
    let ctx = walk.harness.ctx.clone();
    let screen = walk.harness.screen;
    let mut report = std::mem::take(&mut walk.report);
    let mut problems = std::mem::take(&mut walk.problems);
    let step = walk.step;
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
    for (query, expected) in [("strafe glide", 0usize), ("cellular suppression", 1)] {
        let listed = browser.listing(assets::AssetScope::Spawnable, query);
        writeln!(
            report,
            "## Spawn search \"{query}\"\n\n{} rows: {}\n",
            listed.len(),
            listed
                .iter()
                .take(8)
                .map(|row| row.label.clone())
                .collect::<Vec<_>>()
                .join(" | ")
        )
        .unwrap();
        if expected == 0 && !listed.is_empty() {
            problems.push(format!(
                "a spawn search for \"{query}\" lists {} rows",
                listed.len()
            ));
        }
        if expected > 0 && listed.is_empty() {
            problems.push(format!("a spawn search for \"{query}\" lists nothing"));
        }
    }
    let mut query = String::from("strafe glide");
    let mut output = None;
    for _ in 0..3 {
        output = Some(ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    crate::app::style::workbench_style(ui);
                    let _used = browser.draw(
                        ui,
                        assets::AssetScope::Spawnable,
                        &mut query,
                        false,
                        Some("Use for Spawn"),
                        None,
                    );
                });
            },
        ));
    }
    let output = output.unwrap();
    let capture = format!("bundled-{:02}", step + 1);
    crate::app::custom_perks::workbench::tests::capture::write(&ctx, &output, &capture);
    let shown = texts(&output)
        .into_iter()
        .map(|(text, _)| text)
        .collect::<Vec<_>>();
    writeln!(
        report,
        "{}. The spawn picker searched for \"strafe glide\" (`{capture}`)\n\n    {}\n",
        step + 1,
        shown.join(" | ")
    )
    .unwrap();
    if !shown.iter().any(|text| text.starts_with(ABILITY_HINT)) {
        problems.push("an ability search in the spawn picker shows no hint".into());
    }

    writeln!(report, "## Problems\n").unwrap();
    if problems.is_empty() {
        writeln!(report, "None.").unwrap();
    }
    for problem in &problems {
        writeln!(report, "- {problem}").unwrap();
    }
    std::fs::write(out.join("report.md"), report).unwrap();
    assert!(problems.is_empty(), "{problems:#?}");
}
