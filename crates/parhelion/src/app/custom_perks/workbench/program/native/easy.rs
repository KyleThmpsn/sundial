//! Twenty custom perks whose behavior is easy to see in game, each on its own stock weapon.
//!
//! Every perk is authored on its cards the way a user builds one: a trigger preset, conditions
//! from the pickers, stock actions picked under the perk that carries them, recipes from Add
//! Action and only the fields a card offers. The workbench checks and draws each perk, then the
//! weapons are built, staged and read back. Each perk's description says what to do in game to
//! see it work, so the weapons carry their own test instructions.
//!
//! Opt in with `PARHELION_WORKBENCH_INSTALL`, `PARHELION_WORKBENCH_CATALOG` (a copy of the
//! catalog cache, never the live one) and `PARHELION_CLEAN_STOCK_PACKAGES`, as the smoke run
//! does. `PARHELION_EASY_OUT` names the report directory. `PARHELION_EASY_LIBRARY` names a
//! Parhelion data directory to save the perks and weapons into, with the weapons checked for
//! the next build as the library's check boxes check them.
use super::structure::{self, Edit, Part};
use crate::WeaponRecipe;
use crate::app::custom_perks::workbench::attachment::{Change, Target};
use crate::perk::PerkRecipe;
use WeaponInventorySlot::{Energy, Kinetic, Power};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use sundial::investment::discovery::behaviors as stock_behaviors;
use sundial::investment::{
    InvestmentCatalog, WeaponDonor, WeaponInventorySlot, WeaponInvestmentStat, WeaponRarity,
    WeaponSandboxPerkChoice,
};
use sundial::package_authoring::open_shadowkeep_package_manager;
use sundial::package_authoring::sandbox_perk::{
    self as native_perks, action,
    action::native::{
        Graph,
        fields::{self as node_fields, Format as FieldFormat},
    },
    nodes,
    program::{NativeNode, NativeProgram, Program, Trigger, native_draft},
};
use tiger_pkg::TagHash;

mod bundled;
mod cards;
mod complex;
mod newcomer;
mod pickers;

/// A counter, and the row that adds one of its contributing conditions.
const COUNTER: u32 = 0x8080_3E30;
const CONTRIBUTION: u32 = 0x8080_3E32;

/// One test perk: its name, what to do in game to see it work, the weapon it goes on and how
/// it is authored.
struct Plan {
    name: &'static str,
    check: &'static str,
    slot: WeaponInventorySlot,
    /// Weapon types in order of preference. Another type in the slot is used when none is left.
    types: &'static [&'static str],
    author: fn(&mut Author<'_>) -> Result<(), String>,
}

const PLANS: [Plan; 20] = [
    Plan {
        name: "Full Auto",
        check: "Hold the trigger. It keeps firing.",
        slot: Kinetic,
        types: &["Hand Cannon"],
        author: full_auto,
    },
    Plan {
        name: "Reload on Kill",
        check: "Every kill refills the magazine from reserves, however full it is.",
        slot: Kinetic,
        types: &["Scout Rifle"],
        author: reload_on_kill,
    },
    Plan {
        name: "Melee Kill Reload",
        check: "Fire once, then get a melee kill with this weapon in hand. It reloads. With it stowed, nothing happens.",
        slot: Energy,
        types: &["Shotgun"],
        author: melee_kill_reload,
    },
    Plan {
        name: "Orbs on Kill",
        check: "Each kill drops three Orbs of Light where the enemy fell.",
        slot: Energy,
        types: &["Pulse Rifle"],
        author: orbs_on_kill,
    },
    Plan {
        name: "Grenade on Kill",
        check: "Throw your grenade, then get a kill. Your grenade is ready again.",
        slot: Kinetic,
        types: &["Pulse Rifle"],
        author: grenade_on_kill,
    },
    Plan {
        name: "Melee on Kill",
        check: "Use your melee, then get a kill. Your melee is ready again.",
        slot: Energy,
        types: &["Submachine Gun"],
        author: melee_on_kill,
    },
    Plan {
        name: "Super on Kill",
        check: "Each kill fills a quarter of your Super.",
        slot: Power,
        types: &["Machine Gun"],
        author: super_on_kill,
    },
    Plan {
        name: "Class Ability on Kill",
        check: "Use your class ability, then get a kill. It is ready again.",
        slot: Energy,
        types: &["Sidearm"],
        author: class_on_kill,
    },
    Plan {
        name: "Rampage Stacks",
        check: "Kills show the Rampage buff, stacking to x3.",
        slot: Kinetic,
        types: &["Submachine Gun"],
        author: rampage,
    },
    Plan {
        name: "Outlaw Reload",
        check: "Precision kills show the Outlaw buff for 5 seconds and reload much faster.",
        slot: Energy,
        types: &["Scout Rifle"],
        author: outlaw,
    },
    Plan {
        name: "Overflow Rounds",
        check: "Each kill adds 5 rounds, even past a full magazine.",
        slot: Kinetic,
        types: &["Sidearm"],
        author: overflow_rounds,
    },
    Plan {
        name: "Explode on Kill",
        check: "Every kill explodes, precision or not.",
        slot: Energy,
        types: &["Hand Cannon"],
        author: explode_on_kill,
    },
    Plan {
        name: "Tracking Rockets",
        check: "Aim at a target and fire. The rocket follows it.",
        slot: Power,
        types: &["Rocket Launcher"],
        author: tracking_rockets,
    },
    Plan {
        name: "Stat Boost",
        check: "Inspect it. Handling, Reload Speed and Range are much higher than on the stock weapon.",
        slot: Power,
        types: &["Linear Fusion Rifle"],
        author: stat_boost,
    },
    Plan {
        name: "Full Auto After Kill",
        check: "Get a kill, then hold the trigger. It fires full auto for 10 seconds.",
        slot: Kinetic,
        types: &["Sniper Rifle", "Scout Rifle", "Hand Cannon"],
        author: full_auto_after_kill,
    },
    Plan {
        name: "Rapid Kills",
        check: "Three kills within 10 seconds of each other refill your grenade and melee.",
        slot: Power,
        types: &["Grenade Launcher"],
        author: rapid_kills,
    },
    Plan {
        name: "Reload Then Kill",
        check: "Reload, then get a kill within 5 seconds. Half your Super fills. Other kills do nothing.",
        slot: Kinetic,
        types: &["Auto Rifle"],
        author: reload_then_kill,
    },
    Plan {
        name: "Precision or Melee",
        check: "A precision kill or a melee kill refills your class ability. Body shot kills do not.",
        slot: Kinetic,
        types: &["Combat Bow", "Bow"],
        author: precision_or_melee,
    },
    Plan {
        name: "Two Behaviors",
        check: "Kills reload it. Grenade kills also fill a quarter of your Super.",
        slot: Energy,
        types: &["Fusion Rifle"],
        author: two_behaviors,
    },
    Plan {
        name: "Explosion Cooldown",
        check: "The first kill explodes. Kills in the next 5 seconds do not.",
        slot: Power,
        types: &["Sword"],
        author: explosion_cooldown,
    },
];

fn full_auto(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Full Auto")?;
    card.preset(0, Trigger::Equipped)?;
    card.actions(
        0,
        author.kit.recipe("Fire at Full Auto")?,
        "Fire at Full Auto",
    )?;
    author.push(card)
}

fn reload_on_kill(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Reload on Kill")?;
    card.kill(0, Trigger::WeaponKill)?;
    card.action(
        0,
        author.kit.carried("Pulse Monitor", 16)?,
        "Pulse Monitor's",
    )?;
    author.push(card)
}

fn melee_kill_reload(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Melee Kill Reload")?;
    card.kill(0, Trigger::MeleeKill)?;
    card.while_held(0)?;
    card.action(
        0,
        author.kit.carried("Pulse Monitor", 16)?,
        "Pulse Monitor's",
    )?;
    author.push(card)
}

fn orbs_on_kill(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Orbs on Kill")?;
    card.kill(0, Trigger::WeaponKill)?;
    // A new Generate Orbs action, as Add Action gives one: the Masterwork orb, placed at the
    // kill, with three orbs rather than one so they are easy to see. The stock drop at a kill,
    // Light of the Fire's, uses the default orb that Striking Light's text says is for allies,
    // and no orbs from it showed in game.
    let mut orbs = NativeNode::effect(5).ok_or("no orb template")?;
    offer(&mut orbs, effect_class(5)?, "Spawn Position", 1.0)?;
    offer(&mut orbs, effect_class(5)?, "Count", 3.0)?;
    card.action(0, orbs, "Masterwork orbs, Count 3 at the Event Position")?;
    author.push(card)
}

fn grenade_on_kill(author: &mut Author<'_>) -> Result<(), String> {
    energy_on_kill(author, "Grenade on Kill", 0, 1.0)
}

fn melee_on_kill(author: &mut Author<'_>) -> Result<(), String> {
    energy_on_kill(author, "Melee on Kill", 2, 1.0)
}

fn super_on_kill(author: &mut Author<'_>) -> Result<(), String> {
    energy_on_kill(author, "Super on Kill", 1, 0.25)
}

fn class_on_kill(author: &mut Author<'_>) -> Result<(), String> {
    energy_on_kill(author, "Class Ability on Kill", 7, 1.0)
}

fn energy_on_kill(
    author: &mut Author<'_>,
    name: &str,
    target: u8,
    scale: f32,
) -> Result<(), String> {
    let mut card = author.card(name)?;
    card.kill(0, Trigger::WeaponKill)?;
    let (node, what) = author.kit.energy(target, scale)?;
    card.action(0, node, &what)?;
    author.push(card)
}

fn rampage(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Rampage Stacks")?;
    card.kill(0, Trigger::WeaponKill)?;
    let title = "Rampage's Stacking Damage";
    card.actions(0, author.kit.recipe(title)?, title)?;
    author.push(card)
}

fn outlaw(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Outlaw Reload")?;
    card.kill(0, Trigger::PrecisionKill)?;
    let title = "Outlaw's Faster Reload";
    card.actions(0, author.kit.recipe(title)?, title)?;
    author.push(card)
}

fn overflow_rounds(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Overflow Rounds")?;
    card.kill(0, Trigger::WeaponKill)?;
    let mut rounds = author.kit.carried("Triple Tap", 14)?;
    offer(&mut rounds, effect_class(14)?, "Owning Slot Amount", 5.0)?;
    offer(
        &mut rounds,
        effect_class(14)?,
        "Allow Magazine Overflow",
        1.0,
    )?;
    card.action(0, rounds, "Triple Tap's, 5 rounds with overflow")?;
    author.push(card)
}

fn explode_on_kill(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Explode on Kill")?;
    card.kill(0, Trigger::WeaponKill)?;
    card.action(
        0,
        author.kit.explosion()?,
        "Firefly's explosion, on the killed target",
    )?;
    author.push(card)
}

fn tracking_rockets(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Tracking Rockets")?;
    card.preset(0, Trigger::Drawn)?;
    card.actions(0, author.kit.recipe("Track Targets")?, "Track Targets")?;
    author.push(card)
}

/// Stat bonuses only, which the perk's stats carry without any effect.
fn stat_boost(author: &mut Author<'_>) -> Result<(), String> {
    for name in ["Handling", "Reload Speed", "Range"] {
        let stat = author
            .kit
            .stats
            .iter()
            .find(|stat| stat.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("no {name} stat to add"))?;
        author.perk.stats.push(crate::WeaponStatOverride {
            definition_index: stat.definition_index,
            value: 40,
        });
        author.steps.push(format!("Stat: {} +40", stat.name));
    }
    Ok(())
}

fn full_auto_after_kill(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Full Auto After Kill")?;
    card.preset(0, Trigger::WeaponKill)?;
    card.ending(0, 10.0)?;
    card.actions(
        0,
        author.kit.recipe("Fire at Full Auto")?,
        "Fire at Full Auto",
    )?;
    author.push(card)
}

/// A counter that three kills fill, each counting for 10 seconds, as Harbinger's Pulse counts
/// two kills for 2 seconds.
fn rapid_kills(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Rapid Kills")?;
    card.kill(0, Trigger::WeaponKill)?;
    // A new counter starts at the stock norm, clamped from -9998 to 100, so only Count Needed
    // and Resets At change.
    let mut counter = NativeNode::condition(26).ok_or("no counter template")?;
    offer(&mut counter, COUNTER, "Trigger Threshold", 3.0)?;
    offer(&mut counter, COUNTER, "Reset Threshold", 3.0)?;
    card.replace_trigger(0, counter, "When the Effect's Counter Is Reached, at 3")?;
    let kill = kill_condition(Trigger::WeaponKill)?;
    card.contribute(kill, 10.0)?;
    let (grenade, what) = author.kit.energy(0, 1.0)?;
    card.action(0, grenade, &what)?;
    let (melee, what) = author.kit.energy(2, 1.0)?;
    card.action(0, melee, &what)?;
    author.push(card)
}

/// On a kill, and reloaded within the last 5 seconds, with the reload's Stays Met For at 5.
fn reload_then_kill(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Reload Then Kill")?;
    card.kill(0, Trigger::WeaponKill)?;
    let reload = NativeNode::condition(19).ok_or("no reload template")?;
    card.and(0, reload, nodes::condition_title(19))?;
    card.hold(1, 5.0)?;
    let (node, what) = author.kit.energy(1, 0.5)?;
    card.action(0, node, &what)?;
    author.push(card)
}

fn precision_or_melee(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Precision or Melee")?;
    card.kill(0, Trigger::PrecisionKill)?;
    card.or(
        0,
        kill_condition(Trigger::MeleeKill)?,
        Trigger::MeleeKill.label(),
    )?;
    let (node, what) = author.kit.energy(7, 1.0)?;
    card.action(0, node, &what)?;
    author.push(card)
}

fn two_behaviors(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Two Behaviors")?;
    card.kill(0, Trigger::WeaponKill)?;
    card.action(
        0,
        author.kit.carried("Pulse Monitor", 16)?,
        "Pulse Monitor's",
    )?;
    card.add_group()?;
    card.kill(1, Trigger::GrenadeKill)?;
    let (node, what) = author.kit.energy(1, 0.25)?;
    card.action(1, node, &what)?;
    author.push(card)?;
    // As an extra group the grenade kill never filled Super in game, since only a main
    // behavior starts on an event, so it moves into an effect of its own as the card offers.
    author.move_group(1)
}

fn explosion_cooldown(author: &mut Author<'_>) -> Result<(), String> {
    let mut card = author.card("Explosion Cooldown")?;
    card.kill(0, Trigger::WeaponKill)?;
    card.cooldown(0, 5.0)?;
    card.action(
        0,
        author.kit.explosion()?,
        "Firefly's explosion, on the killed target",
    )?;
    author.push(card)
}

/// The condition a kill trigger preset starts with, as Or and And add it from the picker.
fn kill_condition(trigger: Trigger) -> Result<NativeNode, String> {
    structure::preset(trigger)?
        .activation
        .first()
        .map(|node| NativeNode {
            kind: node.kind,
            bytes: node.native.clone(),
        })
        .ok_or_else(|| format!("{} has no condition", trigger.label()))
}

/// The class of an action kind. Condition and action kinds share numbers, so a field is always
/// looked up on the class the node was made as.
fn effect_class(kind: u8) -> Result<u32, String> {
    nodes::effect(kind)
        .map(|entry| entry.class)
        .ok_or_else(|| format!("unknown action kind {kind}"))
}

/// A field a node's card offers by this label: visible and not the compiler's own.
fn offered(class: u32, label: &str) -> Result<node_fields::Field, String> {
    node_fields::describe(class)?
        .into_iter()
        .find(|field| field.label == label)
        .filter(|field| {
            field.editable
                && !super::behavior::compiler_owned(class, field.offset)
                && super::behavior::visible(field, class)
        })
        .ok_or_else(|| format!("the card offers no {label} field"))
}

/// A field's bytes for a number typed into its control.
fn encode(field: &node_fields::Field, value: f64) -> Result<Vec<u8>, String> {
    Ok(match (field.format, field.width) {
        (FieldFormat::Float, 4) => (value as f32).to_le_bytes().to_vec(),
        (FieldFormat::Integer, 4) => (value as i32).to_le_bytes().to_vec(),
        (FieldFormat::Unsigned, 4) => (value as u32).to_le_bytes().to_vec(),
        (FieldFormat::Byte | FieldFormat::Flag, 1) => vec![value as u8],
        _ => return Err(format!("{} is not a number field", field.label)),
    })
}

/// Types a value into a field the node's card offers, before the node is added.
fn offer(node: &mut NativeNode, class: u32, label: &str, value: f64) -> Result<(), String> {
    let field = offered(class, label)?;
    let bytes = encode(&field, value)?;
    node.bytes
        .get_mut(field.offset..field.offset + field.width)
        .ok_or_else(|| format!("{label} lies outside the node"))?
        .copy_from_slice(&bytes);
    Ok(())
}

/// Whether a stock node's field holds this number.
fn holds(class: u32, bytes: &[u8], label: &str, value: f64) -> bool {
    node_fields::describe(class)
        .ok()
        .and_then(|fields| fields.into_iter().find(|field| field.label == label))
        .and_then(|field| {
            let wanted = encode(&field, value).ok()?;
            Some(bytes.get(field.offset..field.offset + field.width)? == wanted.as_slice())
        })
        .unwrap_or(false)
}

/// What the pickers offer: stock behaviors under the perks that carry them, and recipes.
struct Kit<'a> {
    behaviors: &'a stock_behaviors::Catalog,
    names: &'a BTreeMap<u16, String>,
    stats: &'a [WeaponInvestmentStat],
}

impl Kit<'_> {
    /// The stock action of a kind that a named perk carries, as the action picker lists it
    /// under that perk's name. The one the most perks share wins.
    fn carried(&self, perk: &str, kind: u8) -> Result<NativeNode, String> {
        self.carried_where(perk, kind, &[])
    }

    /// Firefly's explosion, which is not a spawn: Firefly attaches it to the target it kills.
    fn explosion(&self) -> Result<NativeNode, String> {
        self.carried_where("Firefly", 1, &[("Attachment Target", 3.0)])
    }

    /// `carried`, among the perk's actions whose fields hold these numbers.
    fn carried_where(
        &self,
        perk: &str,
        kind: u8,
        wanted: &[(&str, f64)],
    ) -> Result<NativeNode, String> {
        let class = effect_class(kind)?;
        self.behaviors
            .effects
            .iter()
            .filter(|effect| {
                effect.kind == kind
                    && wanted
                        .iter()
                        .all(|(label, value)| holds(class, &effect.bytes, label, *value))
                    && effect.sources.iter().any(|source| {
                        self.names
                            .get(&source.perk)
                            .is_some_and(|name| name == perk)
                    })
            })
            .max_by_key(|effect| effect.sources.len())
            .map(|effect| NativeNode {
                kind,
                bytes: effect.bytes.clone(),
            })
            .ok_or_else(|| format!("no {perk} action of kind {kind}"))
    }

    /// The most shared stock action of a kind whose fields hold these numbers.
    fn configured(&self, kind: u8, wanted: &[(&str, f64)]) -> Result<NativeNode, String> {
        let class = effect_class(kind)?;
        self.behaviors
            .effects
            .iter()
            .filter(|effect| {
                effect.kind == kind
                    && wanted
                        .iter()
                        .all(|(label, value)| holds(class, &effect.bytes, label, *value))
            })
            .max_by_key(|effect| effect.sources.len())
            .map(|effect| NativeNode {
                kind,
                bytes: effect.bytes.clone(),
            })
            .ok_or_else(|| format!("no stock {} matches", nodes::effect_title(kind)))
    }

    /// Change Ability Energy for one ability, from the stock grant with a fixed amount, set
    /// to `scale` of the ability's energy.
    fn energy(&self, target: u8, scale: f32) -> Result<(NativeNode, String), String> {
        let mut node = self.configured(
            8,
            &[
                ("Target Selector", f64::from(target)),
                ("Input Selector", 255.0),
                ("Ability State", 0.0),
                ("Ability Version", 0.0),
            ],
        )?;
        offer(&mut node, effect_class(8)?, "Scale", f64::from(scale))?;
        let ability = match target {
            0 => "Grenade",
            1 => "Super",
            2 => "Melee",
            _ => "Class Ability",
        };
        Ok((node, format!("{ability} Energy, Scale {scale}")))
    }

    fn recipe(&self, title: &str) -> Result<Vec<NativeNode>, String> {
        crate::app::custom_perks::workbench::behaviors::recipe_actions(
            title,
            self.behaviors,
            self.names,
        )
        .ok_or_else(|| format!("Add Action offers no {title}"))
    }
}

/// One effect on its card, in the native form the first edit adopts.
struct Card {
    effect: crate::WeaponSandboxPerkRuntimeRecipe,
    native: NativeProgram,
    steps: Vec<String>,
}

impl Card {
    fn edit(&mut self, group: usize, part: Part, edit: Edit, step: String) -> Result<(), String> {
        structure::edit(&mut self.native.graph, group, part, edit)?;
        self.steps.push(step);
        Ok(())
    }

    fn preset(&mut self, group: usize, trigger: Trigger) -> Result<(), String> {
        let step = format!("Group {} Trigger: {}", group + 1, trigger.label());
        self.edit(group, Part::Trigger, Edit::Preset(trigger), step)
    }

    /// A kill trigger, as the picker's preset sets it. The effect ends at once, so every kill
    /// fires it, until an action that holds state gives it a 5 second Duration. `finish`
    /// records the ending each group settles on.
    fn kill(&mut self, group: usize, trigger: Trigger) -> Result<(), String> {
        self.preset(group, trigger)
    }

    fn action(&mut self, group: usize, node: NativeNode, what: &str) -> Result<(), String> {
        let step = format!("Action: {} ({what})", nodes::effect_title(node.kind));
        self.edit(group, Part::Actions, Edit::Add(node), step)
    }

    fn actions(&mut self, group: usize, nodes: Vec<NativeNode>, what: &str) -> Result<(), String> {
        self.edit(
            group,
            Part::Actions,
            Edit::AddAll(nodes),
            format!("Action: {what}"),
        )
    }

    fn or(&mut self, group: usize, node: NativeNode, what: &str) -> Result<(), String> {
        self.edit(group, Part::Trigger, Edit::Add(node), format!("Or: {what}"))
    }

    fn and(&mut self, group: usize, node: NativeNode, what: &str) -> Result<(), String> {
        self.edit(
            group,
            Part::Trigger,
            Edit::Require(node),
            format!("And: {what}"),
        )
    }

    fn add_group(&mut self) -> Result<(), String> {
        structure::add_group(&mut self.native.graph)?;
        self.steps.push("Add Behavior Group".into());
        Ok(())
    }

    /// Checks In Hand on the group's trigger, as the card's switch does: the kill moves inside
    /// a state check that the weapon is in hand, as Grave Robber holds its melee kill. On
    /// Melee Kill alone fires on any melee kill while the perk is equipped, the weapon stowed
    /// or not.
    fn while_held(&mut self, group: usize) -> Result<(), String> {
        self.edit(group, Part::Trigger, Edit::Hold, "In Hand".into())
    }

    fn replace_trigger(
        &mut self,
        group: usize,
        node: NativeNode,
        what: &str,
    ) -> Result<(), String> {
        replace_or_add(&mut self.native.graph, group, Part::Trigger, node)?;
        self.steps.push(format!("Trigger Changed: {what}"));
        Ok(())
    }

    fn ending(&mut self, group: usize, seconds: f32) -> Result<(), String> {
        replace_or_add(&mut self.native.graph, group, Part::Ending, timer(seconds)?)?;
        self.steps.push(format!("Ending: After {seconds} s"));
        Ok(())
    }

    fn cooldown(&mut self, group: usize, seconds: f32) -> Result<(), String> {
        replace_or_add(&mut self.native.graph, group, Part::Rearm, timer(seconds)?)?;
        self.steps.push(format!("Cooldown: {seconds} s"));
        Ok(())
    }

    /// Adds a condition to the newest counter's contributions, counting for `hold` seconds.
    fn contribute(&mut self, node: NativeNode, hold: f32) -> Result<(), String> {
        self.contribute_as(node, hold, "On Weapon Kill")
    }

    fn contribute_as(&mut self, node: NativeNode, hold: f32, what: &str) -> Result<(), String> {
        let graph = &mut self.native.graph;
        // The card drops replaced allocations after each edit, so only live counters remain.
        graph.compact();
        let counter = last_block(graph, COUNTER)?;
        let list = structure::List {
            owner: structure::path_to(graph, counter)?,
            field: 0x10,
            class: CONTRIBUTION,
        };
        list.edit(graph, Edit::Add(node))?;
        graph.compact();
        write_row(graph, CONTRIBUTION, None, "Hold Duration", hold)?;
        self.steps.push(format!(
            "Counter Contribution: {what}, Hold Duration {hold} s"
        ));
        Ok(())
    }

    /// Sets how long one requirement stays met after it passes, as Set Stays Met For does.
    fn hold(&mut self, requirement: usize, seconds: f32) -> Result<(), String> {
        let graph = &mut self.native.graph;
        graph.compact();
        write_row(
            graph,
            action::SUBGROUP_ROW_CLASS,
            Some(requirement),
            "Hold Duration",
            seconds,
        )?;
        self.steps.push(format!(
            "Requirement {} Stays Met For: {seconds} s",
            requirement + 1
        ));
        Ok(())
    }

    /// The card commits an edit by dropping the allocations it replaced, then checks the assets
    /// and the graph.
    fn finish(mut self) -> Result<(crate::WeaponSandboxPerkRuntimeRecipe, Vec<String>), String> {
        self.native.graph.compact();
        self.native.sync_assets()?;
        self.native.validate()?;
        // The ending each group settled on, which a kill's actions decide.
        let decoded = action::decode(&self.native.graph.emit()?)?;
        for (index, group) in action::ActionSummary::new(&decoded)
            .groups
            .iter()
            .enumerate()
        {
            let ending = group
                .removal
                .iter()
                .filter(|line| line.depth == 0)
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>();
            self.steps.push(format!(
                "Group {} Ends: {}",
                index + 1,
                if ending.is_empty() {
                    "Never, until the perk is removed".to_owned()
                } else {
                    ending.join(" or ")
                }
            ));
        }
        let program = self
            .effect
            .program
            .as_mut()
            .ok_or("new effect has no program")?;
        *program = Program {
            name: program.name.clone(),
            native: Some(self.native),
            ..Program::default()
        };
        Ok((self.effect, self.steps))
    }
}

fn timer(seconds: f32) -> Result<NativeNode, String> {
    let mut node = NativeNode::condition(1).ok_or("no timer template")?;
    node.bytes
        .get_mut(8..12)
        .ok_or("the timer has no seconds")?
        .copy_from_slice(&seconds.to_le_bytes());
    Ok(node)
}

/// The newest block of a class. Edits append their blocks and compaction keeps the order, so
/// after an edit this is the block it added.
fn last_block(graph: &Graph, class: u32) -> Result<usize, String> {
    (0..graph.blocks.len())
        .rev()
        .find(|&index| graph.blocks[index].class == class)
        .ok_or_else(|| format!("no 0x{class:08X} block"))
}

/// Writes one row of the newest list of `class`, its last row when none is named, as the
/// row's scalar control does.
fn write_row(
    graph: &mut Graph,
    class: u32,
    row: Option<usize>,
    label: &str,
    value: f32,
) -> Result<(), String> {
    let field = node_fields::describe(class)?
        .into_iter()
        .find(|field| field.label == label)
        .ok_or_else(|| format!("no {label} field"))?;
    let index = last_block(graph, class)?;
    let row = row.unwrap_or_else(|| graph.blocks[index].count.unwrap_or(1).saturating_sub(1));
    field.write(&mut graph.blocks[index], row, &value.to_le_bytes())
}

/// Replaces a list's first entry the way picking on a filled row does, or adds one to an
/// empty list.
fn replace_or_add(
    graph: &mut Graph,
    group: usize,
    part: Part,
    node: NativeNode,
) -> Result<(), String> {
    let decoded = action::decode(&graph.emit()?)?;
    let filled = decoded
        .groups
        .get(group)
        .is_some_and(|behavior| match part {
            Part::Trigger => !behavior.activation.is_empty(),
            Part::Ending => !behavior.removal.is_empty(),
            Part::Rearm => !behavior.rearm.is_empty(),
            Part::Actions => !behavior.effects.is_empty(),
        });
    let edit = if filled {
        Edit::Replace(0, node)
    } else {
        Edit::Add(node)
    };
    structure::edit(graph, group, part, edit)
}

/// One perk being authored: Add Effect takes the workbench's next free index for each card.
struct Author<'a> {
    kit: &'a Kit<'a>,
    workbench: &'a crate::app::custom_perks::workbench::Workbench,
    choices: &'a [WeaponSandboxPerkChoice],
    perk: PerkRecipe,
    steps: Vec<String>,
}

impl Author<'_> {
    fn card(&self, name: &str) -> Result<Card, String> {
        let index = self
            .workbench
            .free_metadata_index(&self.perk, self.choices)
            .ok_or("Add Effect found no free index")?;
        let mut effect = super::super::named_effect(index);
        let program = effect.program.as_mut().ok_or("new effect has no program")?;
        program.name = name.to_owned();
        let native = native_draft(program)?;
        Ok(Card {
            effect,
            native,
            steps: Vec::new(),
        })
    }

    fn push(&mut self, card: Card) -> Result<(), String> {
        let (effect, steps) = card.finish()?;
        self.perk.effects.push(effect);
        self.steps.extend(steps);
        Ok(())
    }

    /// Moves a behavior group of the last effect into an effect of its own, as the card's
    /// Move to Its Own Effect does.
    fn move_group(&mut self, group: usize) -> Result<(), String> {
        let effect = self
            .perk
            .effects
            .last()
            .ok_or("no effect to move a behavior from")?
            .source_perk_index;
        let index = self
            .workbench
            .free_metadata_index(&self.perk, self.choices)
            .ok_or("Add Effect found no free index")?;
        super::move_group(&mut self.perk, effect, group, index)?;
        self.steps
            .push(format!("Move to Its Own Effect: Behavior {}", group + 1));
        Ok(())
    }
}

/// The shape of a decoded action: per group, the kinds in each condition list (nested kinds in
/// brackets) and the action kinds.
fn signature(payload: &[u8]) -> Result<String, String> {
    fn condition(node: &action::DecodedCondition, out: &mut String) {
        let _ = write!(out, "{}", node.kind);
        let nested = node
            .children
            .iter()
            .chain(
                node.subgroups
                    .iter()
                    .flat_map(|subgroup| &subgroup.conditions),
            )
            .collect::<Vec<_>>();
        if !nested.is_empty() {
            out.push('[');
            for (position, child) in nested.into_iter().enumerate() {
                if position > 0 {
                    out.push(' ');
                }
                condition(child, out);
            }
            out.push(']');
        }
    }
    let decoded = action::decode(payload)?;
    let mut out = String::new();
    for (index, group) in decoded.groups.iter().enumerate() {
        let _ = write!(out, "g{index}:");
        for (name, list) in [
            ("on", &group.activation),
            ("end", &group.removal),
            ("rearm", &group.rearm),
        ] {
            let _ = write!(out, " {name}(");
            for (position, node) in list.iter().enumerate() {
                if position > 0 {
                    out.push(' ');
                }
                condition(node, &mut out);
            }
            out.push(')');
        }
        out.push_str(" do(");
        for (position, effect) in group.effects.iter().enumerate() {
            if position > 0 {
                out.push(' ');
            }
            let _ = write!(out, "{}", effect.kind);
        }
        out.push_str(") ");
    }
    Ok(out.trim_end().to_owned())
}

#[derive(serde::Serialize, Default)]
struct PerkLog {
    number: usize,
    name: String,
    check: String,
    weapon: String,
    donor: String,
    weapon_type: String,
    slot: String,
    item_hash: String,
    socket: String,
    /// Donors the build refused before this one, each with the build's reason.
    swaps: Vec<String>,
    steps: Vec<String>,
    compiled: Vec<String>,
    #[serde(skip)]
    payloads: Vec<Vec<u8>>,
    staged: Vec<String>,
    identical_bytes: Vec<bool>,
    ui: Vec<String>,
    /// What the survey found in the drawn layout, once each.
    layout: BTreeSet<String>,
    /// The survey's captures, by name.
    captures: Vec<String>,
    problems: Vec<String>,
}

fn env_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} is not set")))
}

/// The workbench window, drawn headlessly the way the app draws it.
struct Harness {
    ctx: egui::Context,
    app: crate::app::PackageAuthoringApp,
    /// The screen the window is drawn on.
    screen: egui::Vec2,
    _library: tempfile::TempDir,
}

impl Harness {
    fn open(install: &Path, cache: &Path, weapon: &WeaponRecipe) -> Self {
        let catalog = InvestmentCatalog::load_with_cache_path(install, cache, false, |_| {})
            .expect("catalog");
        let choices = catalog
            .weapon_sandbox_perk_choices_from(crate::package_profile::is_stock_item_definition);
        // A temporary library, so autosave never touches a real one.
        let library_root = tempfile::tempdir().expect("library directory");
        let library =
            crate::perk::library::Library::open(library_root.path().to_path_buf()).unwrap();
        library.materialize_bundled().unwrap();
        let mut app = crate::app::PackageAuthoringApp {
            sandbox_perk_choices: choices,
            catalog: Some(catalog),
            packages: install.join("packages"),
            recipe: weapon.clone(),
            show_experimental_options: true,
            ..Default::default()
        };
        app.perk_workbench.library = Some(library);
        app.perk_workbench.initialized = true;
        app.perk_workbench.drafts_writable = true;
        app.perk_workbench.refresh_library();
        app.perk_workbench.open = true;
        let ctx = egui::Context::default();
        ctx.set_theme(egui::Theme::Dark);
        sundial::investment::configure_authoring_fonts(&ctx, install).ok();
        let mut harness = Self {
            ctx,
            app,
            screen: egui::vec2(1440.0, 1300.0),
            _library: library_root,
        };
        // New Perk opens a document, as the button does.
        let output = harness.settle();
        if let Some((_, rect)) = texts(&output)
            .into_iter()
            .find(|(text, _)| text == "New Perk")
        {
            harness.frame(vec![egui::Event::PointerMoved(rect.center())]);
            for pressed in [true, false] {
                harness.frame(vec![egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }]);
            }
        }
        harness.settle();
        assert!(
            !harness.app.perk_workbench.documents.is_empty(),
            "New Perk opened no document"
        );
        harness
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        let app = &mut self.app;
        self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.screen)),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |_| {});
                app.draw_perk_workbench(ctx);
            },
        )
    }

    /// Frames until background reads finish, then one more for layout.
    fn settle(&mut self) -> egui::FullOutput {
        let start = std::time::Instant::now();
        let mut idle = 0;
        while idle < 6 && start.elapsed() < std::time::Duration::from_secs(600) {
            idle = if self.app.perk_workbench.busy() {
                0
            } else {
                idle + 1
            };
            self.frame(vec![]);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        self.frame(vec![])
    }

    /// Opens the perk with every card expanded and records text that points at a problem a
    /// user would see. Returns the frame it read, unless drawing panicked.
    fn show(&mut self, perk: &PerkRecipe, log: &mut PerkLog) -> Option<egui::FullOutput> {
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
        let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.settle()));
        let Ok(output) = drawn else {
            log.problems
                .push("drawing the perk in the workbench panicked".into());
            return None;
        };
        crate::app::custom_perks::workbench::tests::capture::write(
            &self.ctx,
            &output,
            &format!("easy-{:02}", log.number),
        );
        for (text, _) in texts(&output) {
            // Whole words, so "Terrors" in a perk's own description is not an error.
            let lower = text.to_lowercase();
            let words = lower
                .split(|c: char| !c.is_alphanumeric())
                .collect::<BTreeSet<_>>();
            if ["error", "errors", "invalid", "failed", "panicked"]
                .iter()
                .any(|word| words.contains(word))
                || ["could not", "needs attention"]
                    .iter()
                    .any(|phrase| lower.contains(phrase))
                || text.contains("Unnamed Key")
                || text.contains("Unnamed State")
            {
                log.ui.push(text);
            }
        }
        let drawn = &self.app.perk_workbench.documents[selected].recipe;
        if drawn != perk {
            log.problems.push(format!(
                "drawing the perk changed it: {}",
                changes(perk, drawn)
            ));
        }
        Some(output)
    }
}

/// What differs between a perk and the one drawing left, effect by effect and field by field.
fn changes(before: &PerkRecipe, after: &PerkRecipe) -> String {
    if before.effects.len() != after.effects.len() {
        return format!(
            "{} effects became {}",
            before.effects.len(),
            after.effects.len()
        );
    }
    let mut found = Vec::new();
    for (was, now) in before.effects.iter().zip(&after.effects) {
        let effect = was.source_perk_index;
        if was.program != now.program {
            found.push(format!(
                "effect {effect} program {} became {}",
                if was.program.is_some() { "set" } else { "none" },
                if now.program.is_some() { "set" } else { "none" }
            ));
        }
        if was.runtime_values != now.runtime_values {
            found.push(format!(
                "effect {effect} runtime values {:?} became {:?}",
                was.runtime_values, now.runtime_values
            ));
        }
        if was.action_float_values != now.action_float_values {
            found.push(format!(
                "effect {effect} action values {:?} became {:?}",
                was.action_float_values, now.action_float_values
            ));
        }
        if was.projectiles != now.projectiles || was.activation != now.activation {
            found.push(format!("effect {effect} projectiles or activation"));
        }
    }
    if found.is_empty() {
        "outside its effects".to_owned()
    } else {
        found.join("; ")
    }
}

fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some((
                text.galley.job.text.clone(),
                text.galley.rect.translate(text.pos.to_vec2()),
            )),
            _ => None,
        })
        .collect()
}

/// Checks a perk meets before its weapon builds, the ones a user meets in the workbench:
/// validation, readiness, compiling and saving.
fn check(
    manager: &sundial::package_authoring::PackageManager,
    perk: &PerkRecipe,
    log: &mut PerkLog,
) {
    if let Err(error) = perk.validate() {
        log.problems.push(format!("validate: {error}"));
    }
    for (position, effect) in perk.effects.iter().enumerate() {
        let Some(program) = &effect.program else {
            continue;
        };
        if let Some(native) = &program.native {
            match native.authoring_issue() {
                Ok(Some(issue)) => log.problems.push(format!(
                    "effect {}: {} needs attention: {}",
                    position + 1,
                    issue.field,
                    issue.message
                )),
                Ok(None) => {}
                Err(error) => log
                    .problems
                    .push(format!("effect {}: readiness: {error}", position + 1)),
            }
        }
        let compiled = native_perks::program::compile(manager, program).and_then(|compiled| {
            signature(&compiled.payload).map(|shape| (shape, compiled.payload))
        });
        match compiled {
            Ok((shape, payload)) => {
                log.compiled.push(shape);
                log.payloads.push(payload);
            }
            Err(error) => log
                .problems
                .push(format!("effect {}: compile: {error}", position + 1)),
        }
    }
    let json = serde_json::to_string_pretty(perk).unwrap();
    match serde_json::from_str::<PerkRecipe>(&json) {
        Ok(loaded) if loaded == *perk => {}
        Ok(_) => log.problems.push("saved perk loads back changed".into()),
        Err(error) => log
            .problems
            .push(format!("saved perk does not load: {error}")),
    }
}

/// A weapon, its donor, the Trait socket choice its perk replaces and that choice's label.
type Destination = (WeaponRecipe, WeaponDonor, Target, String);

/// The donors each plan may use, in order: stock Legendaries of its preferred types in its
/// slot, then any other stock Legendary in the slot.
fn candidates(catalog: &InvestmentCatalog, plans: &[Plan]) -> Vec<Vec<u32>> {
    let mut donors = catalog
        .weapon_donors()
        .into_iter()
        .filter(|summary| {
            crate::package_profile::is_stock_item_definition(summary.hash)
                && summary.rarity == WeaponRarity::Legendary
                && summary.collection_backed
                && !summary.type_name.trim().is_empty()
        })
        .collect::<Vec<_>>();
    donors.sort_by(|a, b| (&a.name, a.hash).cmp(&(&b.name, b.hash)));
    plans
        .iter()
        .map(|plan| {
            let in_slot = |summary: &&sundial::investment::WeaponDonorSummary| {
                summary.inventory_slot == Some(plan.slot)
            };
            let mut hashes = plan
                .types
                .iter()
                .flat_map(|wanted| {
                    donors
                        .iter()
                        .filter(move |summary| summary.type_name == *wanted)
                })
                .chain(donors.iter())
                .filter(in_slot)
                .map(|summary| summary.hash)
                .collect::<Vec<_>>();
            let mut seen = BTreeSet::new();
            hashes.retain(|hash| seen.insert(*hash));
            hashes
        })
        .collect()
}

/// The first candidate no other plan uses that has a Trait socket to replace and can build.
fn pick(
    catalog: &InvestmentCatalog,
    candidates: &[u32],
    used: &mut BTreeSet<u32>,
    name: &str,
) -> Option<Destination> {
    let picked = candidates
        .iter()
        .filter(|hash| !used.contains(*hash))
        .find_map(|hash| destination(catalog, *hash, name))?;
    used.insert(picked.1.summary.hash);
    Some(picked)
}

/// Whether the build can turn every random-roll column into a fixed one. A column with no
/// default plug and no embedded plug cannot become one, so the build refuses the weapon, as
/// it refuses Bad Reputation.
fn buildable(donor: &WeaponDonor) -> bool {
    donor.sockets.iter().all(|socket| {
        socket.randomized_plug_set_index.is_none()
            || (socket.max_authored_choices > 0
                && (socket.native_default.is_some() || !socket.ordered_embedded_choices.is_empty()))
    })
}

/// A named recipe on a stock donor, and the first Trait socket's first choice, which a perk
/// replaces so it is the one the weapon rolls with.
fn destination(catalog: &InvestmentCatalog, hash: u32, name: &str) -> Option<Destination> {
    let donor = catalog.weapon_donor(hash).filter(buildable)?;
    let mut recipe =
        WeaponRecipe::new_named_weapon_for_donor(name, hash, &donor.summary.name).ok()?;
    // A donor with no ammo type of its own asks for one, as Trust and Polaris Lance do.
    if donor.summary.ammo_type.is_none() {
        recipe.overrides.ammo_type = Some(crate::recipe::RecipeAmmoType::Special);
    }
    let (target, label) = donor.sockets.iter().find_map(|socket| {
        let target = Target::capture(&recipe, &donor, socket.index, 0).ok()?;
        let label = target.label(&donor, catalog);
        // An empty socket's first choice would add an alternative, which the weapon never rolls.
        (label.starts_with("Replace ") && label.contains("Trait")).then_some((target, label))
    })?;
    Some((recipe, donor, target, label))
}

fn weapon_name(run: &Run, position: usize, plan: &Plan) -> String {
    format!("{} {:02} {}", run.weapon, position + 1, plan.name)
}

/// Apply to Weapon: the perk replaces the weapon's first Trait choice.
fn place(
    perk: &PerkRecipe,
    destination: Destination,
    log: &mut PerkLog,
    out: &Path,
) -> WeaponRecipe {
    let (mut weapon, donor, target, label) = destination;
    log.weapon.clone_from(&weapon.name);
    log.donor.clone_from(&donor.summary.name);
    log.weapon_type.clone_from(&donor.summary.type_name);
    log.slot = donor
        .summary
        .inventory_slot
        .map_or_else(String::new, |slot| format!("{slot:?}"));
    log.item_hash = weapon
        .identity
        .item_hash
        .parse_u32()
        .map(|hash| hash.to_string())
        .unwrap_or_default();
    log.socket = label;
    let change = Change {
        target,
        perk: Some(perk.clone()),
    };
    if let Err(error) = change.apply(&mut weapon, &donor) {
        log.problems.push(format!("apply to weapon: {error}"));
    }
    if let Err(error) = weapon.validate() {
        log.problems.push(format!("weapon: {error}"));
    }
    std::fs::write(
        out.join("weapons")
            .join(format!("{}.parhelion.json", weapon.namespace)),
        serde_json::to_string_pretty(&weapon).unwrap(),
    )
    .unwrap();
    weapon
}

/// One set of perks to author, check, build and read back.
struct Run {
    /// Names the report directory and its `PARHELION_*_OUT` variable.
    name: &'static str,
    plans: &'static [Plan],
    /// Perk ids start with this, in hex and dashes as perk ids must be.
    ids: &'static str,
    /// Weapon names start with this.
    weapon: &'static str,
    /// Whether `PARHELION_EASY_LIBRARY` may save the perks and weapons to a library.
    library: bool,
    /// Whether each perk is also surveyed at two window widths, scrolled end to end.
    survey: bool,
}

const EASY: Run = Run {
    name: "easy",
    plans: &PLANS,
    ids: "ea5e-2026-0925",
    weapon: "Test",
    library: true,
    survey: false,
};

#[test]
#[ignore = "requires PARHELION_WORKBENCH_INSTALL, PARHELION_WORKBENCH_CATALOG and PARHELION_CLEAN_STOCK_PACKAGES"]
fn easy_perks_author_build_stage_and_read_back() {
    run(&EASY);
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn run(run: &Run) {
    let install = env_path("PARHELION_WORKBENCH_INSTALL");
    let cache = env_path("PARHELION_WORKBENCH_CATALOG");
    let stock = env_path("PARHELION_CLEAN_STOCK_PACKAGES");
    let out = std::env::var_os(format!("PARHELION_{}_OUT", run.name.to_uppercase())).map_or_else(
        || std::env::temp_dir().join(format!("parhelion-{}", run.name)),
        PathBuf::from,
    );
    if out.exists() {
        std::fs::remove_dir_all(&out).expect("clear the previous report");
    }
    for folder in ["perks", "weapons", "plans"] {
        std::fs::create_dir_all(out.join(folder)).unwrap();
    }

    let catalog =
        InvestmentCatalog::load_with_cache_path(&install, &cache, false, |_| {}).expect("catalog");
    let choices =
        catalog.weapon_sandbox_perk_choices_from(crate::package_profile::is_stock_item_definition);
    let stats = catalog.perk_stat_choices();
    let manager = open_shadowkeep_package_manager(&stock).expect("clean stock packages");
    let behaviors = stock_behaviors::discover(&stock).expect("stock behaviors");
    let candidates = candidates(&catalog, run.plans);
    let mut used = BTreeSet::new();
    let mut picked = run
        .plans
        .iter()
        .enumerate()
        .map(|(position, plan)| {
            pick(
                &catalog,
                &candidates[position],
                &mut used,
                &weapon_name(run, position, plan),
            )
        })
        .collect::<Vec<_>>();
    let first = picked
        .iter()
        .flatten()
        .next()
        .map(|(recipe, ..)| recipe.clone())
        .expect("no stock weapon donors");

    let mut harness = Harness::open(&install, &cache, &first);
    let names = harness.app.perk_workbench.perk_names.clone();
    let kit = Kit {
        behaviors: &behaviors,
        names: &names,
        stats: &stats,
    };
    let mut results = Vec::new();
    for (position, plan) in run.plans.iter().enumerate() {
        let number = position + 1;
        let mut log = PerkLog {
            number,
            name: plan.name.to_owned(),
            check: plan.check.to_owned(),
            ..PerkLog::default()
        };
        let authored = {
            let mut author = Author {
                kit: &kit,
                workbench: &harness.app.perk_workbench,
                choices: &choices,
                perk: PerkRecipe::new(),
                steps: Vec::new(),
            };
            (plan.author)(&mut author).map(|()| (author.perk, author.steps))
        };
        let mut perk = match authored {
            Ok((perk, steps)) => {
                log.steps = steps;
                perk
            }
            Err(error) => {
                log.problems.push(format!("authoring: {error}"));
                results.push((None, log));
                continue;
            }
        };
        perk.id = format!("{}-{number:04x}", run.ids);
        perk.name = plan.name.to_owned();
        perk.description = plan.check.to_owned();
        // A perk meant to work in game should draw neither a problem nor a warning.
        if let Some(issue) = harness.app.perk_workbench.validation_issue(&perk) {
            log.problems.push(format!("workbench: {}", issue.message));
        }
        check(&manager, &perk, &mut log);
        harness.show(&perk, &mut log);
        let workbench = &harness.app.perk_workbench;
        let plan_text = crate::app::custom_perks::workbench::test_plan::render(
            &perk,
            crate::ItemKind::Weapon,
            &workbench.perk_names,
            Some(&workbench.keys.catalog),
            &workbench.asset_labels,
        );
        std::fs::write(
            out.join("plans").join(format!("{number:02}.md")),
            format!("# {}\n\n{}\n\n{plan_text}", plan.name, plan.check),
        )
        .unwrap();
        std::fs::write(
            out.join("perks").join(format!("{number:02}.perk.json")),
            serde_json::to_string_pretty(&perk).unwrap(),
        )
        .unwrap();

        match picked[position].take() {
            Some(destination) => {
                let weapon = place(&perk, destination, &mut log, &out);
                results.push((Some((perk, weapon)), log));
            }
            None => {
                log.problems
                    .push(format!("no {:?} weapon with a Trait socket", plan.slot));
                results.push((None, log));
            }
        }
    }

    // Every perk at a wide and then a narrow window, scrolled end to end. The window keeps its
    // size once drawn, so the narrow pass comes second.
    if run.survey {
        for (width, tag) in [(1440.0, "wide"), (1040.0, "narrow")] {
            harness.screen = egui::vec2(width, 1300.0);
            for (built, log) in &mut results {
                if let Some((perk, _)) = built {
                    harness.survey(perk, log, run.name, tag);
                }
            }
        }
    }

    // Build and stage every weapon, then read each built perk back from the staged packages. A
    // weapon the build refuses moves to the plan's next donor, as a user picks another weapon,
    // and the report keeps the refusal.
    let build = loop {
        if results.iter().any(|(_, log)| !log.problems.is_empty()) {
            break Err("perks failed before the build".to_owned());
        }
        let recipes = results
            .iter()
            .filter_map(|(built, _)| built.as_ref().map(|(_, weapon)| weapon.clone()))
            .collect::<Vec<_>>();
        let error =
            match crate::workflow::BatchBuildSnapshot::new(crate::workflow::BatchBuildRequest {
                package_directory: stock.clone(),
                staging_root: out.join("staging"),
                ignore_installed_authored_overlays: true,
                recipes,
            })
            .and_then(|snapshot| {
                crate::workflow::build_and_stage_snapshot_with_progress(&snapshot, |_| {})
            }) {
                Ok(report) => break Ok(report),
                Err(error) => error,
            };
        let refused = results.iter().position(|(built, _)| {
            built.as_ref().is_some_and(|(_, weapon)| {
                error.contains(&format!("({})", weapon.namespace))
                    || error.contains(&format!("\"{}\"", weapon.name))
            })
        });
        let swaps = results
            .iter()
            .map(|(_, log)| log.swaps.len())
            .sum::<usize>();
        let Some(position) = refused.filter(|_| swaps < 8) else {
            break Err(error);
        };
        let (built, log) = &mut results[position];
        let Some((perk, weapon)) = built.take() else {
            break Err(error);
        };
        let reason = error.lines().last().unwrap_or(&error).trim().to_owned();
        log.swaps
            .push(format!("{} could not build: {reason}", log.donor));
        match pick(&catalog, &candidates[position], &mut used, &weapon.name) {
            Some(destination) => {
                let weapon = place(&perk, destination, log, &out);
                *built = Some((perk, weapon));
            }
            None => {
                log.problems
                    .push(format!("no other donor builds: {reason}"));
                *built = Some((perk, weapon));
                break Err(error);
            }
        }
    };
    let summary = match &build {
        Ok(build) => {
            let read = read_back(&stock, build, &mut results);
            format!(
                "Built {} weapons into {} artifacts in `{}`.{}",
                build.weapons.len(),
                build.artifacts.len(),
                build.run_directory.display(),
                read.err()
                    .map(|error| format!(" Reading back failed: {error}"))
                    .unwrap_or_default()
            )
        }
        Err(error) => format!("Build failed: {error}"),
    };

    // Save to the library as the workbench and the recipe editor do, checked for the next build.
    let passed = build.is_ok() && results.iter().all(|(_, log)| log.problems.is_empty());
    let library = match std::env::var_os("PARHELION_EASY_LIBRARY").filter(|_| run.library) {
        Some(root) if passed => save_to_library(Path::new(&root), &results),
        Some(_) => "Not saved to the library, because a perk failed.".into(),
        None => String::new(),
    };

    std::fs::write(
        out.join("report.json"),
        serde_json::to_string_pretty(&results.iter().map(|(_, log)| log).collect::<Vec<_>>())
            .unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("report.md"),
        markdown(&summary, &library, &results),
    )
    .unwrap();
    std::fs::write(out.join("checklist.md"), checklist(&results)).unwrap();
    eprintln!("Easy perk report: {}", out.join("report.md").display());
    let failed = results
        .iter()
        .filter(|(_, log)| !log.problems.is_empty())
        .map(|(_, log)| format!("{}: {}", log.name, log.problems.join("; ")))
        .collect::<Vec<_>>();
    assert!(build.is_ok(), "{summary}");
    assert!(
        failed.is_empty(),
        "{} perks failed:\n{}",
        failed.len(),
        failed.join("\n")
    );
    assert!(!library.contains("failed"), "{library}");
}

type Built = Option<(PerkRecipe, WeaponRecipe)>;

/// Opens the clean stock packages with the staged run laid over them, as the build's own check
/// does, and compares every built private perk's action with what its program compiled to.
fn read_back(
    stock: &Path,
    build: &crate::workflow::BuildReport,
    results: &mut [(Built, PerkLog)],
) -> Result<(), String> {
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let view = crate::workflow::FilteredPackageView::create(stock, &ignored)?;
    for artifact in &build.artifacts {
        view.add_overlay(&build.run_directory.join(&artifact.file_name))?;
    }
    let manager =
        open_shadowkeep_package_manager(view.path()).map_err(|error| error.to_string())?;
    let runtime_map = manager
        .read_tag(TagHash(native_perks::SANDBOX_PERK_RUNTIME_MAP_TAG))
        .map_err(|error| error.to_string())?;
    let mut built = BTreeMap::<String, Vec<(u32, usize)>>::new();
    for weapon in &build.weapons {
        for plug in &weapon.custom_plugs {
            let name = plug.name.clone().unwrap_or_default();
            for perk in &plug.perks {
                built
                    .entry(name.clone())
                    .or_default()
                    .push((perk.runtime_key, perk.source_perk_index));
            }
        }
    }
    for (authored, log) in results.iter_mut() {
        let Some((perk, _)) = authored else {
            continue;
        };
        let entries = built.get(&perk.name).map_or(&[][..], Vec::as_slice);
        if entries.len() != perk.effects.len() {
            log.problems.push(format!(
                "{} effects authored, {} private perks built",
                perk.effects.len(),
                entries.len()
            ));
        }
        for (runtime_key, source_index) in entries {
            let position = perk
                .effects
                .iter()
                .position(|effect| usize::from(effect.source_perk_index) == *source_index);
            let staged = native_perks::sandbox_perk_runtime_assignment(&runtime_map, *runtime_key)
                .ok()
                .flatten()
                .and_then(|assignment| assignment.action_tag())
                .ok_or_else(|| format!("runtime key 0x{runtime_key:08X} has no staged action"))
                .and_then(|tag| manager.read_tag(tag).map_err(|error| error.to_string()))
                .and_then(|payload| signature(&payload).map(|shape| (shape, payload)));
            let expected = position.and_then(|position| {
                Some((log.compiled.get(position)?, log.payloads.get(position)?))
            });
            match (staged, expected) {
                (Ok((shape, payload)), Some((compiled, bytes))) if shape == *compiled => {
                    log.identical_bytes.push(payload == *bytes);
                    log.staged.push(shape);
                }
                (Ok((shape, _)), Some((compiled, _))) => log.problems.push(format!(
                    "staged action differs: built {shape} but authored {compiled}"
                )),
                (Ok(_), None) => log
                    .problems
                    .push("a staged action matches no authored effect".into()),
                (Err(error), _) => log
                    .problems
                    .push(format!("staged action unreadable: {error}")),
            }
        }
    }
    Ok(())
}

/// Saves each perk to Custom Perks and each weapon to the recipe library, replacing an earlier
/// copy of the same weapon, and checks the weapons for the next build.
fn save_to_library(root: &Path, results: &[(Built, PerkLog)]) -> String {
    let saved = || -> Result<usize, String> {
        let perks = crate::perk::library::Library::open(root.join("perks"))?;
        let recipes = crate::recipe_library::RecipeLibrary::open(root.join("recipes"))?;
        let existing = recipes.scan()?.entries;
        let mut paths = Vec::new();
        for (perk, weapon) in results.iter().filter_map(|(built, _)| built.as_ref()) {
            let file = perks.root().join(format!("{}.perk.json", perk.id));
            let expected = std::fs::read(&file).ok();
            perks.save(perk, expected.as_deref())?;
            match existing
                .iter()
                .find(|entry| entry.namespace == weapon.namespace)
            {
                Some(entry) => {
                    recipes.save_existing(&entry.path, weapon)?;
                    paths.push(entry.path.clone());
                }
                None => paths.push(recipes.save_new(weapon)?),
            }
        }
        let entries = recipes.scan()?.entries;
        let mut enabled = recipes.enabled_paths(&entries)?;
        enabled.extend(paths.iter().cloned());
        recipes.save_enabled_paths(&enabled, &entries)?;
        Ok(paths.len())
    };
    match saved() {
        Ok(count) => format!(
            "Saved {count} perks and weapons to `{}`, with the weapons checked for the next build.",
            root.display()
        ),
        Err(error) => format!("Saving to the library failed: {error}"),
    }
}

fn markdown(summary: &str, library: &str, results: &[(Built, PerkLog)]) -> String {
    let mut out = String::new();
    let failed = results
        .iter()
        .filter(|(_, log)| !log.problems.is_empty())
        .count();
    let _ = writeln!(out, "# Easy custom perks\n");
    let _ = writeln!(
        out,
        "{} perks, {} passed and {} failed.\n\n{summary}\n",
        results.len(),
        results.len() - failed,
        failed
    );
    if !library.is_empty() {
        let _ = writeln!(out, "{library}\n");
    }
    for (_, log) in results {
        let status = if log.problems.is_empty() {
            "passed"
        } else {
            "FAILED"
        };
        let _ = writeln!(out, "## {:02} {} ({status})\n", log.number, log.name);
        let _ = writeln!(out, "{}\n", log.check);
        if !log.weapon.is_empty() {
            let _ = writeln!(
                out,
                "On **{}**, built on {} ({} {}, item {}), in {}.\n",
                log.weapon, log.donor, log.slot, log.weapon_type, log.item_hash, log.socket
            );
        }
        for swap in &log.swaps {
            let _ = writeln!(out, "- Moved to another donor: {swap}");
        }
        for step in &log.steps {
            let _ = writeln!(out, "- {step}");
        }
        for (position, shape) in log.compiled.iter().enumerate() {
            let staged = match log.identical_bytes.get(position) {
                Some(true) => "read back, identical bytes",
                Some(false) => "read back, same shape",
                None => "not read back",
            };
            let _ = writeln!(out, "- Compiled {}: `{shape}` ({staged})", position + 1);
        }
        for finding in &log.layout {
            let _ = writeln!(out, "- Layout: {finding}");
        }
        if !log.captures.is_empty() {
            let _ = writeln!(out, "- Captures: {}", log.captures.join(", "));
        }
        for text in &log.ui {
            let _ = writeln!(out, "- Shown: {text}");
        }
        for problem in &log.problems {
            let _ = writeln!(out, "- **Problem:** {problem}");
        }
        out.push('\n');
    }
    out
}

/// The in-game checklist: each weapon, where it goes and what to do to see its perk work.
fn checklist(results: &[(Built, PerkLog)]) -> String {
    let mut out = String::from("# In-Game Checklist\n\n");
    let _ = writeln!(
        out,
        "Each weapon's first trait is its test perk, and the perk's description repeats the check.\n"
    );
    for (_, log) in results {
        let _ = writeln!(
            out,
            "- [ ] **{}** ({} {}, {}): {}",
            log.weapon, log.slot, log.weapon_type, log.donor, log.check
        );
    }
    out
}
