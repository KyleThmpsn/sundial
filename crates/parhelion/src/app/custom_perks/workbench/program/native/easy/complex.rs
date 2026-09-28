//! Complex custom perks, each built to stress one part of the effect card: several behavior
//! groups, Or inside And, a counter inside a requirement, an And block among Or alternatives,
//! Not, an ending with alternatives, long action lists, a stock nested trigger and a perk with
//! two effects. Each is authored, checked, built and read back like the easy perks, then drawn
//! at a wide and a narrow window and scrolled end to end. Every frame is checked for text past
//! the window, text cut off by its cell, overlapping text and labels cut short, and
//! `PARHELION_UI_CAPTURE_DIR` keeps each frame and its text for review.
//!
//! Opt in as the easy perks do. `PARHELION_COMPLEX_OUT` names the report directory.
use super::*;

const PLANS: [Plan; 10] = [
    Plan {
        name: "Kill Chain Engine",
        check: "Precision kills reload faster. Weapon, melee and grenade kills refill some grenade and melee energy, at most every 2 seconds. While drawn, its stats change.",
        slot: Kinetic,
        types: &["Hand Cannon"],
        author: kill_chain_engine,
    },
    Plan {
        name: "Requirements Maze",
        check: "A weapon or precision kill within 5 seconds of a reload, once two kills are counted, gives Super, orbs and a reload.",
        slot: Energy,
        types: &["Pulse Rifle"],
        author: requirements_maze,
    },
    Plan {
        name: "Counter Stack",
        check: "Five points of kills, precision kills counting twice, refill every ability and add rounds.",
        slot: Kinetic,
        types: &["Auto Rifle"],
        author: counter_stack,
    },
    Plan {
        name: "Not in State",
        check: "Kills while not in the checked state refill your class ability.",
        slot: Energy,
        types: &["Sidearm"],
        author: not_in_state,
    },
    Plan {
        name: "Timed Arsenal",
        check: "Reloading starts 6 seconds of damage, full auto and Rampage, ended early by holstering, then a 10 second cooldown.",
        slot: Kinetic,
        types: &["Submachine Gun"],
        author: timed_arsenal,
    },
    Plan {
        name: "Two Effects",
        check: "Kills reload it, and while equipped its stats change. The perk also raises Handling and Stability.",
        slot: Energy,
        types: &["Scout Rifle"],
        author: two_effects,
    },
    Plan {
        name: "Nested Alternatives",
        check: "A precision kill, or any kill within 3 seconds of a reload, gives orbs and Super.",
        slot: Kinetic,
        types: &["Scout Rifle"],
        author: nested_alternatives,
    },
    Plan {
        name: "Kill Streak Tiers",
        check: "Three quick kills refill your grenade. Six kills within 20 seconds give half your Super and orbs.",
        slot: Power,
        types: &["Machine Gun"],
        author: kill_streak_tiers,
    },
    Plan {
        name: "Everything at Once",
        check: "Any kill or finisher refills every ability, drops orbs, reloads, adds rounds and explodes.",
        slot: Energy,
        types: &["Hand Cannon"],
        author: everything_at_once,
    },
    Plan {
        name: "Kill Clip Remix",
        check: "Kill Clip's own trigger with more damage for 5 seconds.",
        slot: Kinetic,
        types: &["Pulse Rifle"],
        author: kill_clip_remix,
    },
];

pub(super) const COMPLEX: Run = Run {
    name: "complex",
    plans: &PLANS,
    ids: "c0de-2026-0926",
    weapon: "Complex",
    library: false,
    survey: true,
};

#[test]
#[ignore = "requires PARHELION_WORKBENCH_INSTALL, PARHELION_WORKBENCH_CATALOG and PARHELION_CLEAN_STOCK_PACKAGES"]
fn complex_perks_author_survey_build_and_read_back() {
    run(&COMPLEX);
}

fn kill_chain_engine(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Kill Chain Engine")?;
    card.kill(0, Trigger::PrecisionKill)?;
    let outlaw = "Outlaw's Faster Reload";
    card.actions(0, author.kit.recipe(outlaw)?, outlaw)?;
    card.add_group()?;
    card.kill(1, Trigger::WeaponKill)?;
    card.or(
        1,
        kill_condition(Trigger::MeleeKill)?,
        Trigger::MeleeKill.label(),
    )?;
    card.or(
        1,
        kill_condition(Trigger::GrenadeKill)?,
        Trigger::GrenadeKill.label(),
    )?;
    card.cooldown(1, 2.0)?;
    let (grenade, what) = author.kit.energy(0, 0.25)?;
    card.action(1, grenade, &what)?;
    let (melee, what) = author.kit.energy(2, 0.25)?;
    card.action(1, melee, &what)?;
    card.add_group()?;
    card.preset(2, Trigger::Drawn)?;
    let (stats, what) = author.kit.carried_or("Firmly Planted", 10)?;
    card.action(2, stats, &what)?;
    author.push(card)
}

fn requirements_maze(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Requirements Maze")?;
    card.kill(0, Trigger::WeaponKill)?;
    card.or(
        0,
        kill_condition(Trigger::PrecisionKill)?,
        Trigger::PrecisionKill.label(),
    )?;
    let reload = NativeNode::condition(19).ok_or("no reload template")?;
    card.and(0, reload, nodes::condition_title(19))?;
    card.hold(1, 5.0)?;
    card.and(
        0,
        counter(2.0)?,
        "When the Effect's Counter Is Reached, at 2",
    )?;
    card.contribute(kill_condition(Trigger::WeaponKill)?, 8.0)?;
    let (energy, what) = author.kit.energy(1, 0.25)?;
    card.action(0, energy, &what)?;
    card.action(0, author.kit.orbs(2.0)?, "Count 2 at the Event Position")?;
    card.action(
        0,
        author.kit.carried("Pulse Monitor", 16)?,
        "Pulse Monitor's",
    )?;
    author.push(card)
}

fn counter_stack(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Counter Stack")?;
    card.kill(0, Trigger::WeaponKill)?;
    card.replace_trigger(
        0,
        counter(5.0)?,
        "When the Effect's Counter Is Reached, at 5",
    )?;
    for (trigger, hold) in [
        (Trigger::WeaponKill, 10.0),
        (Trigger::PrecisionKill, 10.0),
        (Trigger::MeleeKill, 15.0),
    ] {
        card.contribute_as(kill_condition(trigger)?, hold, trigger.label())?;
        if trigger == Trigger::PrecisionKill {
            card.success(2.0)?;
        }
    }
    for (target, scale) in [(0, 1.0), (2, 1.0), (1, 0.1), (7, 1.0)] {
        let (energy, what) = author.kit.energy(target, scale)?;
        card.action(0, energy, &what)?;
    }
    let mut rounds = author.kit.carried("Triple Tap", 14)?;
    offer(&mut rounds, effect_class(14)?, "Owning Slot Amount", 3.0)?;
    offer(
        &mut rounds,
        effect_class(14)?,
        "Allow Magazine Overflow",
        1.0,
    )?;
    card.action(0, rounds, "Triple Tap's, 3 rounds with overflow")?;
    author.push(card)
}

fn not_in_state(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Not in State")?;
    card.kill(0, Trigger::WeaponKill)?;
    let (mut state, name) = author.kit.stock_condition(20)?;
    *state
        .bytes
        .get_mut(0xF8)
        .ok_or("the state check has no Not flag")? = 1;
    card.and(0, state, &format!("Not {name}"))?;
    let (energy, what) = author.kit.energy(7, 1.0)?;
    card.action(0, energy, &what)?;
    author.push(card)
}

fn timed_arsenal(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Timed Arsenal")?;
    card.kill(0, Trigger::WeaponKill)?;
    let reload = NativeNode::condition(19).ok_or("no reload template")?;
    card.replace_trigger(0, reload, nodes::condition_title(19))?;
    card.ending(0, 6.0)?;
    let (holster, name) = author.kit.stock_condition(17)?;
    card.or_ending(0, holster, &name)?;
    card.cooldown(0, 10.0)?;
    card.action(0, author.kit.configured(40, &[])?, "as stock perks set it")?;
    card.actions(
        0,
        author.kit.recipe("Fire at Full Auto")?,
        "Fire at Full Auto",
    )?;
    let rampage = "Rampage's Stacking Damage";
    card.actions(0, author.kit.recipe(rampage)?, rampage)?;
    author.push(card)
}

fn two_effects(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Kill Reload")?;
    card.kill(0, Trigger::WeaponKill)?;
    card.action(
        0,
        author.kit.carried("Pulse Monitor", 16)?,
        "Pulse Monitor's",
    )?;
    author.push(card)?;
    let mut card = author.card("Equipped Stats")?;
    card.preset(0, Trigger::Equipped)?;
    let (stats, what) = author.kit.carried_or("Moving Target", 10)?;
    card.action(0, stats, &what)?;
    author.push(card)?;
    add_stats(author, &[("Handling", 20), ("Stability", 20)])
}

fn nested_alternatives(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Nested Alternatives")?;
    card.kill(0, Trigger::WeaponKill)?;
    let reload = NativeNode::condition(19).ok_or("no reload template")?;
    card.and(0, reload, nodes::condition_title(19))?;
    card.hold(1, 3.0)?;
    card.or(
        0,
        kill_condition(Trigger::PrecisionKill)?,
        Trigger::PrecisionKill.label(),
    )?;
    card.action(0, author.kit.orbs(2.0)?, "Count 2 at the Event Position")?;
    let (energy, what) = author.kit.energy(1, 0.2)?;
    card.action(0, energy, &what)?;
    author.push(card)
}

fn kill_streak_tiers(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Kill Streak Tiers")?;
    card.kill(0, Trigger::WeaponKill)?;
    card.replace_trigger(
        0,
        counter(3.0)?,
        "When the Effect's Counter Is Reached, at 3",
    )?;
    card.contribute(kill_condition(Trigger::WeaponKill)?, 5.0)?;
    let (grenade, what) = author.kit.energy(0, 1.0)?;
    card.action(0, grenade, &what)?;
    card.add_group()?;
    card.kill(1, Trigger::WeaponKill)?;
    card.replace_trigger(
        1,
        counter(6.0)?,
        "When the Effect's Counter Is Reached, at 6",
    )?;
    card.contribute(kill_condition(Trigger::WeaponKill)?, 20.0)?;
    let (energy, what) = author.kit.energy(1, 0.5)?;
    card.action(1, energy, &what)?;
    card.action(1, author.kit.orbs(3.0)?, "Count 3 at the Event Position")?;
    author.push(card)
}

fn everything_at_once(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Everything at Once")?;
    card.kill(0, Trigger::AnyKill)?;
    let (finisher, name) = author.kit.stock_condition(42)?;
    card.or(0, finisher, &name)?;
    for (target, scale) in [(0, 1.0), (2, 1.0), (1, 0.2), (7, 1.0)] {
        let (energy, what) = author.kit.energy(target, scale)?;
        card.action(0, energy, &what)?;
    }
    card.action(0, author.kit.orbs(3.0)?, "Count 3 at the Event Position")?;
    card.action(
        0,
        author.kit.carried("Pulse Monitor", 16)?,
        "Pulse Monitor's",
    )?;
    let mut rounds = author.kit.carried("Triple Tap", 14)?;
    offer(&mut rounds, effect_class(14)?, "Owning Slot Amount", 3.0)?;
    offer(
        &mut rounds,
        effect_class(14)?,
        "Allow Magazine Overflow",
        1.0,
    )?;
    card.action(0, rounds, "Triple Tap's, 3 rounds with overflow")?;
    card.action(
        0,
        author.kit.explosion()?,
        "Firefly's explosion, on the killed target",
    )?;
    author.push(card)
}

fn kill_clip_remix(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Kill Clip Remix")?;
    card.kill(0, Trigger::WeaponKill)?;
    let trigger = author.kit.stock_trigger("Kill Clip")?;
    card.replace_trigger(0, trigger, "Kill Clip's trigger")?;
    card.ending(0, 5.0)?;
    card.action(0, author.kit.configured(40, &[])?, "as stock perks set it")?;
    author.push(card)
}

/// A new counter that fires at `needed` and then starts again, as Harbinger's Pulse counts.
fn counter(needed: f64) -> Result<NativeNode, String> {
    let mut counter = NativeNode::condition(26).ok_or("no counter template")?;
    offer(&mut counter, COUNTER, "Trigger Threshold", needed)?;
    offer(&mut counter, COUNTER, "Reset Threshold", needed)?;
    Ok(counter)
}

/// Stat bonuses on the perk itself, from the stats the perk picker offers.
fn add_stats(author: &mut Author<'_>, stats: &[(&str, i32)]) -> Result<(), String> {
    for (name, value) in stats {
        let stat = author
            .kit
            .stats
            .iter()
            .find(|stat| stat.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("no {name} stat to add"))?;
        author.perk.stats.push(crate::WeaponStatOverride {
            definition_index: stat.definition_index,
            value: *value,
        });
        author.steps.push(format!("Stat: {} {value:+}", stat.name));
    }
    Ok(())
}

impl Kit<'_> {
    /// `carried`, or the stock action of the kind most perks share when that perk has none.
    fn carried_or(&self, perk: &str, kind: u8) -> Result<(NativeNode, String), String> {
        match self.carried(perk, kind) {
            Ok(node) => Ok((node, format!("{perk}'s"))),
            Err(_) => Ok((self.configured(kind, &[])?, "as stock perks set it".into())),
        }
    }

    /// Stock orbs at the kill, `count` of them.
    fn orbs(&self, count: f64) -> Result<NativeNode, String> {
        let mut orbs = self.configured(5, &[("Spawn Position", 1.0)])?;
        offer(&mut orbs, effect_class(5)?, "Count", count)?;
        Ok(orbs)
    }

    /// The stock configuration of a condition kind most perks share, as the condition picker
    /// lists it, and the name it reads by.
    fn stock_condition(&self, kind: u8) -> Result<(NativeNode, String), String> {
        let found = self
            .behaviors
            .conditions
            .iter()
            .filter(|entry| entry.condition.kind == kind)
            .max_by_key(|entry| entry.sources.len())
            .ok_or_else(|| format!("no stock {} condition", nodes::condition_title(kind)))?;
        let name = found
            .condition
            .name
            .clone()
            .unwrap_or_else(|| nodes::condition_title(kind).to_owned());
        Ok((
            NativeNode {
                kind,
                bytes: found.condition.bytes.clone(),
            },
            name,
        ))
    }

    /// The trigger a named stock perk starts from, nested conditions and all.
    fn stock_trigger(&self, perk: &str) -> Result<NativeNode, String> {
        self.behaviors
            .conditions
            .iter()
            .filter(|entry| {
                entry.sources.iter().any(|source| {
                    source.role.ends_with("· Trigger")
                        && self
                            .names
                            .get(&source.perk)
                            .is_some_and(|name| name == perk)
                })
            })
            .max_by_key(|entry| entry.condition.bytes.len())
            .map(|entry| NativeNode {
                kind: entry.condition.kind,
                bytes: entry.condition.bytes.clone(),
            })
            .ok_or_else(|| format!("no {perk} trigger"))
    }
}

impl Card {
    /// Or on the ending: the effect also ends on this condition.
    fn or_ending(&mut self, group: usize, node: NativeNode, what: &str) -> Result<(), String> {
        self.edit(
            group,
            Part::Ending,
            Edit::Add(node),
            format!("Or Ending: {what}"),
        )
    }

    /// What the newest contribution adds to its counter when it passes.
    fn success(&mut self, value: f32) -> Result<(), String> {
        write_row(
            &mut self.native.graph,
            CONTRIBUTION,
            None,
            "Success Value",
            value,
        )?;
        self.steps.push(format!("Counts As: {value}"));
        Ok(())
    }
}

/// A text shape a frame shows, where it is and what clips it.
struct Seen {
    text: String,
    rect: egui::Rect,
    clip: egui::Rect,
    elided: bool,
}

fn seen(output: &egui::FullOutput) -> Vec<Seen> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => {
                // The glyphs' own rows, since a label wrapped into a line lays out from the
                // line's start and only its first row is offset past the leading space.
                let rect = text
                    .galley
                    .rows
                    .iter()
                    .filter(|row| !row.glyphs.is_empty())
                    .fold(egui::Rect::NOTHING, |bounds, row| bounds.union(row.rect))
                    .translate(text.pos.to_vec2());
                let visible = rect.intersect(shape.clip_rect);
                (visible.width() > 0.5
                    && visible.height() > 0.5
                    && !text.galley.job.text.trim().is_empty())
                .then(|| Seen {
                    text: text.galley.job.text.clone(),
                    rect,
                    clip: shape.clip_rect,
                    elided: text.galley.elided,
                })
            }
            _ => None,
        })
        .collect()
}

fn short(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default();
    if line.chars().count() > 60 {
        format!("{}…", line.chars().take(59).collect::<String>())
    } else {
        line.to_owned()
    }
}

/// What a frame shows wrong: a window wider than the screen, text past the window's edge or cut
/// off by its cell, labels egui shortened, and text drawn over other text.
fn inspect(window: Option<egui::Rect>, screen: egui::Vec2, shown: &[Seen]) -> Vec<String> {
    let mut findings = Vec::new();
    let Some(window) = window else {
        return vec!["the workbench window is not open".into()];
    };
    // The window's widest is the screen less 40, and its frame adds 14.
    if window.max.x > screen.x + 0.5 || window.width() > screen.x - 25.0 {
        findings.push(format!(
            "window {:.0} wide on a {:.0} screen",
            window.width(),
            screen.x
        ));
    }
    for item in shown {
        if item.rect.max.x > window.max.x - 1.0 {
            findings.push(format!("runs past the window: {}", short(&item.text)));
        } else if item.rect.max.x > item.clip.max.x + 1.0 && item.clip.max.x < window.max.x - 24.0 {
            findings.push(format!("cut off by its cell: {}", short(&item.text)));
        }
        if item.elided {
            findings.push(format!("shortened: {}", short(&item.text)));
        }
    }
    for (index, first) in shown.iter().enumerate() {
        for second in &shown[index + 1..] {
            let overlap = first.rect.intersect(second.rect);
            if overlap.width() <= 0.0 || overlap.height() <= 0.0 {
                continue;
            }
            let smaller = first.rect.area().min(second.rect.area()).max(1.0);
            if overlap.area() > 0.25 * smaller {
                findings.push(format!(
                    "overlaps: {} and {}",
                    short(&first.text),
                    short(&second.text)
                ));
            }
        }
    }
    findings
}

impl Harness {
    /// Draws the perk with every card expanded, scrolls the editor from top to bottom and records
    /// what each frame shows wrong. With `PARHELION_UI_CAPTURE_DIR` set, each frame is kept with
    /// a list of its text.
    pub(super) fn survey(&mut self, perk: &PerkRecipe, log: &mut PerkLog, run: &str, tag: &str) {
        let selected = self.app.perk_workbench.selected;
        self.app.perk_workbench.documents[selected].recipe = perk.clone();
        let count = perk.effects.len();
        for (position, effect) in perk.effects.iter().enumerate() {
            crate::app::custom_perks::workbench::cards::Card::new(
                &perk.id,
                effect.source_perk_index,
                position,
                count,
            )
            .set_expanded(&self.ctx, true);
        }
        self.settle();
        let window = self.window();
        let Some(rect) = window else {
            log.layout
                .insert(format!("{tag}: the workbench window is not open"));
            return;
        };
        // Inside the editor, left of the cards' controls, so the wheel scrolls the effect list.
        let at = egui::pos2(
            rect.left() + rect.width() * 0.34,
            rect.top() + rect.height() * 0.6,
        );
        let mut output = self.scroll(at, 100_000.0);
        let mut last = None;
        for step in 0..14 {
            let shown = seen(&output);
            let signature = shown
                .iter()
                .map(|item| (item.text.clone(), item.rect.min.y.round() as i32))
                .collect::<Vec<_>>();
            if last.as_ref() == Some(&signature) {
                break;
            }
            for finding in inspect(self.window(), self.screen, &shown) {
                log.layout.insert(format!("{tag}: {finding}"));
            }
            let name = format!("{run}-{:02}-{tag}-{step}", log.number);
            crate::app::custom_perks::workbench::tests::capture::write(&self.ctx, &output, &name);
            if let Some(directory) = std::env::var_os("PARHELION_UI_CAPTURE_DIR") {
                let listing = shown
                    .iter()
                    .map(|item| {
                        format!(
                            "{:>6.0} {:>6.0} {:>5.0} {:>4.0} | {}",
                            item.rect.min.x,
                            item.rect.min.y,
                            item.rect.width(),
                            item.rect.height(),
                            item.text.replace('\n', " / ")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                std::fs::write(
                    PathBuf::from(directory).join(format!("{name}.txt")),
                    listing,
                )
                .unwrap();
            }
            log.captures.push(name);
            last = Some(signature);
            output = self.scroll(at, -900.0);
        }
        // Folded, each card reads as its summary line.
        for (position, effect) in perk.effects.iter().enumerate() {
            crate::app::custom_perks::workbench::cards::Card::new(
                &perk.id,
                effect.source_perk_index,
                position,
                count,
            )
            .set_expanded(&self.ctx, false);
        }
        let output = self.scroll(at, 100_000.0);
        let shown = seen(&output);
        for finding in inspect(self.window(), self.screen, &shown) {
            log.layout.insert(format!("{tag} folded: {finding}"));
        }
        let name = format!("{run}-{:02}-{tag}-folded", log.number);
        crate::app::custom_perks::workbench::tests::capture::write(&self.ctx, &output, &name);
        log.captures.push(name);
    }

    fn window(&self) -> Option<egui::Rect> {
        self.ctx
            .memory(|memory| memory.area_rect(egui::Id::new("global-custom-perk-workbench")))
    }

    /// One wheel turn over `at`, then the pointer leaves so no tooltip opens over the frame.
    fn scroll(&mut self, at: egui::Pos2, delta: f32) -> egui::FullOutput {
        self.frame(vec![egui::Event::PointerMoved(at)]);
        self.frame(vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, delta),
            modifiers: egui::Modifiers::NONE,
        }]);
        for _ in 0..40 {
            self.frame(vec![]);
        }
        self.frame(vec![egui::Event::PointerGone]);
        self.settle()
    }
}
