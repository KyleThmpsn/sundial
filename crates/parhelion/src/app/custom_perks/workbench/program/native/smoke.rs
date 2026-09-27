//! A seeded end-to-end smoke run of the custom perk workbench, done the way a user works:
//! author a perk on its cards, let the workbench check it, look at it, apply it to a weapon,
//! build and stage the weapons, then read every built perk back from the staged packages.
//!
//! Every value comes from something a user can pick: a trigger preset, an authorable condition
//! or action, a named choice, a value stock perks store in the same field, a named key or a
//! stock asset. A draft the workbench refuses to apply is recorded and authored again, as a user
//! would fix it. The same seed authors the same perks, so a failing combination can be rerun.
//!
//! Opt in with `PARHELION_WORKBENCH_INSTALL` (an install root, for the catalog and the
//! workbench), `PARHELION_WORKBENCH_CATALOG` (a copy of the catalog cache, never the live one)
//! and `PARHELION_CLEAN_STOCK_PACKAGES` (the clean stock packages the weapons are built from).
//! `PARHELION_SMOKE_OUT` names the report directory, `PARHELION_SMOKE_SEED` the seed (hex) and
//! `PARHELION_SMOKE_PERKS` the number of perks. `PARHELION_UI_CAPTURE_DIR` also captures each
//! drawn perk. The report, perks, weapon recipes and in-game test plans land in the report
//! directory, and the staged run under its `staging` folder.
use super::structure::{self, Edit, Part};
use crate::WeaponRecipe;
use crate::app::custom_perks::workbench::attachment::{Change, Target};
use crate::perk::PerkRecipe;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use sundial::investment::InvestmentCatalog;
use sundial::package_authoring::sandbox_perk::{
    self as native_perks, action,
    action::native::fields::{self as node_fields, Format as FieldFormat, keys},
    nodes,
    program::{Action, NativeNode, Program, Trigger, native_draft},
};
use sundial::package_authoring::{open_shadowkeep_package_manager, resolve_live_named_tag};
use tiger_pkg::TagHash;

const DEFAULT_SEED: u64 = 0x5EED_2026_0925;
const DEFAULT_PERKS: usize = 30;
const WEAPONS: usize = 10;
/// A counter, and the row that adds one of its contributing conditions.
const COUNTER: u32 = 0x8080_3E30;
const CONTRIBUTION: u32 = 0x8080_3E32;
/// Condition classes whose byte +F8 inverts the check, the Not box on a card.
const PREDICATES: [u32; 2] = [0x8080_3DCE, 0x8080_3DCC];
/// Socket roles a user adds a perk to. Cosmetic and tracker sockets are left alone.
const PERK_ROLES: [&str; 7] = [
    "Trait",
    "Perk",
    "Barrel",
    "Magazine",
    "Intrinsic",
    "Frame",
    "Origin",
];

/// A small deterministic generator, so a seed always authors the same perks.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // SplitMix64.
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    fn below(&mut self, count: usize) -> usize {
        (self.next() % count.max(1) as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    fn seconds(&mut self, low: f32, high: f32) -> f32 {
        let step = (self.next() % 1000) as f32 / 1000.0;
        ((low + step * (high - low)) * 10.0).round() / 10.0
    }
}

/// Values stock perks store in each field of each node class, so a seasoned node only ever
/// holds what the engine is known to accept there.
#[derive(Default)]
struct Pools {
    values: BTreeMap<(u32, usize), Vec<Vec<u8>>>,
    /// Assets stock nodes of each asset-bearing class reference.
    assets: BTreeMap<u32, Vec<u32>>,
}

impl Pools {
    fn harvest(
        manager: &sundial::package_authoring::PackageManager,
        globals: &[u8],
        indices: &[u16],
    ) -> Self {
        let mut pools = Self::default();
        let mut seen = BTreeSet::new();
        for &index in indices {
            let Ok(stock) = native_perks::load_sandbox_perk_runtime_action(
                manager,
                globals,
                usize::from(index),
            ) else {
                continue;
            };
            if !seen.insert(stock.action_tag) {
                continue;
            }
            let Ok(decoded) = action::decode(&stock.action_payload) else {
                continue;
            };
            for group in &decoded.groups {
                let mut stack = group
                    .activation
                    .iter()
                    .chain(&group.removal)
                    .chain(&group.rearm)
                    .collect::<Vec<_>>();
                while let Some(condition) = stack.pop() {
                    pools.observe(condition.class, &condition.native);
                    stack.extend(&condition.children);
                }
                for effect in &group.effects {
                    pools.observe(effect.class, &effect.native);
                    if let Some(tag) = effect.referenced_tag {
                        pools.assets.entry(effect.class).or_default().push(tag);
                    }
                }
            }
        }
        for list in pools.values.values_mut() {
            list.sort();
            list.dedup();
        }
        for list in pools.assets.values_mut() {
            list.sort_unstable();
            list.dedup();
        }
        pools
    }

    fn observe(&mut self, class: u32, bytes: &[u8]) {
        let Ok(fields) = node_fields::describe(class) else {
            return;
        };
        for field in fields.iter().filter(|field| field.editable) {
            if let Some(value) = bytes.get(field.offset..field.offset + field.width) {
                self.values
                    .entry((class, field.offset))
                    .or_default()
                    .push(value.to_vec());
            }
        }
    }
}

/// Fills a fresh node's fields with values a user can pick: a named choice, a value stock
/// nodes store in that field, a named key or a stock asset. Only what a card offers is touched,
/// its visible fields and the reference its asset picker sets, so compiler-owned fields, hidden
/// native values and nested records keep the template's bytes.
fn season(
    node: &mut NativeNode,
    class: u32,
    pools: &Pools,
    rng: &mut Rng,
    notes: &mut Vec<String>,
) {
    let Ok(fields) = node_fields::describe(class) else {
        return;
    };
    let offered = |field: &&node_fields::Field| {
        field.editable
            && !super::behavior::compiler_owned(class, field.offset)
            && (super::behavior::visible(field, class)
                || (field.format == FieldFormat::Tag && field.offset == 16 && picks_asset(class)))
    };
    for field in fields.iter().filter(offered) {
        let range = field.offset..field.offset + field.width;
        if node.bytes.get(range.clone()).is_none() {
            continue;
        }
        // An asset field is always chosen, as the card asks for one before it compiles.
        if field.format != FieldFormat::Tag && !rng.chance(45) {
            continue;
        }
        let contract = node_fields::contract(class, field);
        let named = keys::known(class, field.offset);
        let value: Option<Vec<u8>> = match field.format {
            FieldFormat::Byte | FieldFormat::Mask32
                if contract.bitmask && !contract.choices.is_empty() =>
            {
                let mut bits = 0u32;
                for (bit, _) in contract.choices {
                    if rng.chance(50) {
                        bits |= u32::from(*bit);
                    }
                }
                Some(bits.to_le_bytes()[..field.width.min(4)].to_vec())
            }
            FieldFormat::Byte if !contract.choices.is_empty() => {
                Some(vec![rng.pick(contract.choices).0])
            }
            FieldFormat::Byte if !contract.observed.is_empty() => {
                Some(vec![rng.pick(contract.observed).0])
            }
            FieldFormat::Key | FieldFormat::Tag if !named.is_empty() => {
                Some(rng.pick(named).hash.to_le_bytes().to_vec())
            }
            _ => pools
                .values
                .get(&(class, field.offset))
                .filter(|values| !values.is_empty())
                .map(|values| rng.pick(values).clone()),
        };
        if let Some(value) = value
            && value.len() == field.width
        {
            node.bytes[range].copy_from_slice(&value);
            notes.push(field.label.clone());
        }
    }
}

/// Every authorable kind that has a fresh node and, for actions that need one, a stock asset.
struct Kinds {
    conditions: Vec<u8>,
    actions: Vec<u8>,
}

impl Kinds {
    fn authorable(pools: &Pools) -> Self {
        let authorable =
            |entry: &&nodes::NodeKind| matches!(entry.support, nodes::Support::Authorable);
        let conditions = nodes::CONDITIONS
            .iter()
            .filter(authorable)
            .filter(|entry| NativeNode::condition(entry.kind).is_some())
            .map(|entry| entry.kind)
            .collect();
        let actions = nodes::EFFECTS
            .iter()
            .filter(authorable)
            .filter(|entry| {
                NativeNode::effect(entry.kind).is_some()
                    && (!needs_asset(entry.class) || pools.assets.contains_key(&entry.class))
            })
            .map(|entry| entry.kind)
            .collect();
        Self {
            conditions,
            actions,
        }
    }
}

/// Classes whose card asks for an object, effect or projectile before it compiles.
fn needs_asset(class: u32) -> bool {
    matches!(class, 0x80803E45 | 0x80803E44 | 0x80803E43 | 0x80803E12)
}

/// Classes whose card offers an asset picker, which a drop's optional effect adds to.
fn picks_asset(class: u32) -> bool {
    needs_asset(class) || class == 0x8080_3E47
}

fn condition_class(kind: u8) -> u32 {
    nodes::condition(kind).map_or(0, |entry| entry.class)
}

fn effect_class(kind: u8) -> u32 {
    nodes::effect(kind).map_or(0, |entry| entry.class)
}

fn with_notes(title: String, notes: &[String]) -> String {
    if notes.is_empty() {
        title
    } else {
        format!("{title} [{}]", notes.join(", "))
    }
}

/// What one authored effect was built from, for the report.
#[derive(serde::Serialize, Default, Clone)]
struct EffectLog {
    name: String,
    form: String,
    steps: Vec<String>,
    /// Edits a card refused, with its message, before the user picked again.
    refused: Vec<String>,
}

struct Author<'a> {
    rng: Rng,
    pools: &'a Pools,
    kinds: &'a Kinds,
}

impl Author<'_> {
    fn condition(&mut self, steps: &mut Vec<String>, role: &str) -> NativeNode {
        let kind = *self.rng.pick(&self.kinds.conditions);
        let class = condition_class(kind);
        let mut node = NativeNode::condition(kind).expect("authorable condition");
        let mut notes = Vec::new();
        if kind == 1 {
            // A timer is the one condition a user sets by its seconds.
            let seconds = self.rng.seconds(0.2, 12.0);
            node.bytes[8..12].copy_from_slice(&seconds.to_le_bytes());
            notes.push(format!("{seconds} s"));
        } else {
            season(&mut node, class, self.pools, &mut self.rng, &mut notes);
        }
        let inverted =
            PREDICATES.contains(&class) && node.bytes.len() > 0xF8 && self.rng.chance(40);
        if inverted {
            node.bytes[0xF8] = 1;
            notes.push("Not".into());
        }
        steps.push(format!(
            "{role}: {}",
            with_notes(nodes::condition_title(kind).to_owned(), &notes)
        ));
        node
    }

    fn timer(&mut self, low: f32, high: f32) -> (NativeNode, f32) {
        let mut node = NativeNode::condition(1).expect("timer");
        let seconds = self.rng.seconds(low, high);
        node.bytes[8..12].copy_from_slice(&seconds.to_le_bytes());
        (node, seconds)
    }

    fn action(&mut self, steps: &mut Vec<String>) -> NativeNode {
        let kind = *self.rng.pick(&self.kinds.actions);
        let class = effect_class(kind);
        let mut node = NativeNode::effect(kind).expect("authorable action");
        let mut notes = Vec::new();
        season(&mut node, class, self.pools, &mut self.rng, &mut notes);
        if needs_asset(class) {
            let tag = *self.rng.pick(&self.pools.assets[&class]);
            node.bytes[16..20].copy_from_slice(&tag.to_le_bytes());
            notes.push(format!("asset 0x{tag:08X}"));
        }
        steps.push(format!(
            "Action: {}",
            with_notes(nodes::effect_title(kind).to_owned(), &notes)
        ));
        node
    }

    /// One behavior group's lists, filled the way a user fills a card: its trigger, Or
    /// alternatives, And requirements, an ending, a cooldown and its actions.
    fn group(
        &mut self,
        graph: &mut action::native::Graph,
        group: usize,
        log: &mut EffectLog,
    ) -> Result<(), String> {
        let label = format!("Group {} Trigger", group + 1);
        if self.rng.chance(55) {
            let preset = self.trigger_preset();
            structure::edit(graph, group, Part::Trigger, Edit::Preset(preset))?;
            log.steps.push(format!("{label}: {}", preset.label()));
        } else {
            self.retry(
                log,
                |author, steps| author.condition(steps, &label),
                |node| replace_or_add(graph, group, Part::Trigger, node),
            )?;
        }
        for _ in 0..self.rng.below(3) {
            self.retry(
                log,
                |author, steps| author.condition(steps, "Or"),
                |node| structure::edit(graph, group, Part::Trigger, Edit::Add(node)),
            )?;
        }
        if self.rng.chance(35) {
            for _ in 0..1 + self.rng.below(2) {
                self.retry(
                    log,
                    |author, steps| author.condition(steps, "And"),
                    |node| structure::edit(graph, group, Part::Trigger, Edit::Require(node)),
                )?;
            }
        }
        // Counting kills toward a threshold is its own common perk, so a counter is added on
        // purpose rather than left to the odds of one condition kind among forty.
        if self.rng.chance(40) {
            let counter = NativeNode::condition(26).ok_or("no counter template")?;
            let (edit, role) = if self.rng.chance(50) {
                (Edit::Require(counter), "And")
            } else {
                (Edit::Add(counter), "Or")
            };
            structure::edit(graph, group, Part::Trigger, edit)?;
            log.steps
                .push(format!("{role}: {}", nodes::condition_title(26)));
        }
        if self.rng.chance(50) {
            let (timer, seconds) = self.timer(0.5, 20.0);
            replace_or_add(graph, group, Part::Ending, timer)?;
            log.steps.push(format!("Ending: After {seconds} s"));
        }
        if self.rng.chance(30) {
            self.retry(
                log,
                |author, steps| author.condition(steps, "Or Ending"),
                |node| structure::edit(graph, group, Part::Ending, Edit::Add(node)),
            )?;
        }
        if self.rng.chance(35) {
            let (timer, seconds) = self.timer(0.5, 10.0);
            replace_or_add(graph, group, Part::Rearm, timer)?;
            log.steps.push(format!("Cooldown: {seconds} s"));
        }
        self.contributions(graph, log)?;
        for _ in 0..1 + self.rng.below(4) {
            self.retry(
                log,
                |author, steps| author.action(steps),
                |node| structure::edit(graph, group, Part::Actions, Edit::Add(node)),
            )?;
        }
        Ok(())
    }

    /// Every counter the trigger holds counts something, as a user fills one from its Add
    /// Condition button: a weapon kill about half the time, otherwise another condition. An
    /// empty counter is refused at Apply, so without this no counter was ever built.
    fn contributions(
        &mut self,
        graph: &mut action::native::Graph,
        log: &mut EffectLog,
    ) -> Result<(), String> {
        let kill = structure::preset(Trigger::WeaponKill)?
            .activation
            .first()
            .map(|node| NativeNode {
                kind: node.kind,
                bytes: node.native.clone(),
            })
            .ok_or("the kill preset has no condition")?;
        loop {
            // The card drops replaced allocations after each edit, so only live counters remain.
            graph.compact();
            let Some(counter) = (0..graph.blocks.len()).find(|&index| {
                graph.blocks[index].class == COUNTER
                    && graph.blocks[index]
                        .links
                        .get(&0x10)
                        .is_none_or(|&rows| graph.blocks[rows].count.unwrap_or(0) == 0)
            }) else {
                return Ok(());
            };
            let list = structure::List {
                owner: structure::path_to(graph, counter)?,
                field: 0x10,
                class: CONTRIBUTION,
            };
            for _ in 0..1 + self.rng.below(3) {
                self.retry(
                    log,
                    |author, steps| {
                        if author.rng.chance(50) {
                            steps.push("Counter Contribution: On Weapon Kill".into());
                            return kill.clone();
                        }
                        // A counter inside a counter is not something stock perks nest.
                        loop {
                            let node = author.condition(steps, "Counter Contribution");
                            if node.kind != 26 {
                                return node;
                            }
                            steps.pop();
                        }
                    },
                    |node| list.edit(graph, Edit::Add(node)),
                )?;
            }
        }
    }

    /// Up to five tries at one edit, authoring the node again each time the card refuses it,
    /// as a user picks again after reading the refusal.
    fn retry(
        &mut self,
        log: &mut EffectLog,
        mut make: impl FnMut(&mut Self, &mut Vec<String>) -> NativeNode,
        mut apply: impl FnMut(NativeNode) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut last = String::new();
        for _ in 0..5 {
            let node = make(self, &mut log.steps);
            match apply(node) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    let step = log.steps.pop().unwrap_or_default();
                    log.refused.push(format!("{step}: {error}"));
                    last = error;
                }
            }
        }
        Err(format!(
            "five edits in a row were refused, the last with: {last}"
        ))
    }

    fn trigger_preset(&mut self) -> Trigger {
        *self.rng.pick(&[
            Trigger::Always,
            Trigger::Equipped,
            Trigger::Drawn,
            Trigger::WeaponKill,
            Trigger::PrecisionKill,
            Trigger::MeleeKill,
            Trigger::GrenadeKill,
            Trigger::AnyKill,
        ])
    }

    /// One effect, authored as a user would on its card: a trigger, optional alternatives
    /// and requirements, an ending, a cooldown, actions and sometimes a second behavior group.
    fn effect(
        &mut self,
        metadata_index: u16,
        number: usize,
    ) -> Result<(crate::WeaponSandboxPerkRuntimeRecipe, EffectLog), String> {
        let mut effect = super::super::named_effect(metadata_index);
        let program = effect.program.as_mut().ok_or("new effect has no program")?;
        program.name = format!("Smoke Effect {number}");
        let mut log = EffectLog {
            name: program.name.clone(),
            ..EffectLog::default()
        };
        // A saved guided program is still what older perks hold, so some stay in that form.
        if self.rng.chance(25) {
            log.form = "guided".into();
            program.trigger = self.trigger_preset();
            log.steps
                .push(format!("Trigger: {}", program.trigger.label()));
            if program.trigger.is_timed() {
                program.duration_ms = (self.rng.seconds(0.5, 20.0) * 1000.0) as u32;
                log.steps
                    .push(format!("Duration: {} ms", program.duration_ms));
            }
            if program.trigger.supports_cooldown() && self.rng.chance(40) {
                program.cooldown_ms = (self.rng.seconds(0.5, 10.0) * 1000.0) as u32;
                log.steps
                    .push(format!("Cooldown: {} ms", program.cooldown_ms));
            }
            if program.trigger.is_event() && self.rng.chance(40) {
                program.chance_permyriad = [2500, 5000, 7500][self.rng.below(3)];
                log.steps
                    .push(format!("Chance: {}%", program.chance_permyriad / 100));
            }
            for _ in 0..1 + self.rng.below(3) {
                self.retry(
                    &mut log,
                    |author, steps| author.action(steps),
                    |node| {
                        program.actions.push(Action::Native { node });
                        program.validate().inspect_err(|_| {
                            program.actions.pop();
                        })
                    },
                )?;
            }
            return Ok((effect, log));
        }
        // Every other card edits the native form, which the first edit adopts.
        log.form = "native".into();
        let mut native = native_draft(program)?;
        let groups = if self.rng.chance(25) { 2 } else { 1 };
        for group in 0..groups {
            if group > 0 {
                structure::add_group(&mut native.graph)?;
                log.steps.push("Add Behavior Group".into());
            }
            self.group(&mut native.graph, group, &mut log)?;
        }
        // The card commits an edit by dropping the allocations it replaced, then checks the
        // assets and the graph.
        native.graph.compact();
        native.sync_assets()?;
        native.validate()?;
        *program = Program {
            name: program.name.clone(),
            native: Some(native),
            ..Program::default()
        };
        Ok((effect, log))
    }
}

/// Replaces a list's first entry the way picking on a filled row does, or adds one to an
/// empty list.
fn replace_or_add(
    graph: &mut action::native::Graph,
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

/// The shape of a decoded action: per group, the kinds in each condition list (nested kinds in
/// brackets) and the action kinds.
fn signature(payload: &[u8]) -> Result<String, String> {
    fn condition(node: &action::DecodedCondition, out: &mut String) {
        let _ = write!(out, "{}", node.kind);
        if !node.children.is_empty() {
            out.push('[');
            for (position, child) in node.children.iter().enumerate() {
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

#[derive(serde::Serialize, Default, Clone)]
struct PerkLog {
    number: usize,
    id: String,
    name: String,
    stats: Vec<String>,
    effects: Vec<EffectLog>,
    compiled: Vec<String>,
    /// Hex of each effect's compiled action, compared byte for byte with the staged one.
    #[serde(skip)]
    payloads: Vec<Vec<u8>>,
    weapon: String,
    socket: String,
    staged: Vec<String>,
    identical_bytes: Vec<bool>,
    /// Counter and requirement event masks read back from the staged actions.
    event_masks: Vec<String>,
    ui: Vec<String>,
    problems: Vec<String>,
}

#[derive(serde::Serialize)]
struct Blocked {
    name: String,
    issue: String,
    effects: Vec<EffectLog>,
}

#[derive(serde::Serialize, Default)]
struct Report {
    seed: String,
    perks: usize,
    weapons: Vec<String>,
    condition_kinds: usize,
    action_kinds: usize,
    kinds_used: BTreeMap<String, usize>,
    blocked: Vec<Blocked>,
    build: String,
    staging: String,
    results: Vec<PerkLog>,
}

fn env_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} is not set")))
}

/// The workbench window, drawn headlessly the way the app draws it.
struct Harness {
    ctx: egui::Context,
    app: crate::app::PackageAuthoringApp,
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
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440.0, 1300.0),
                )),
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
    /// user would see.
    fn show(&mut self, perk: &PerkRecipe, log: &mut PerkLog, diffs: &Path) {
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
            return;
        };
        crate::app::custom_perks::workbench::tests::capture::write(
            &self.ctx,
            &output,
            &format!("smoke-{:02}", log.number),
        );
        let shown = texts(&output);
        for (text, _) in &shown {
            let lower = text.to_lowercase();
            if [
                "error",
                "could not",
                "invalid",
                "failed",
                "panicked",
                "needs attention",
            ]
            .iter()
            .any(|word| lower.contains(word))
                || text == "Choose an object or effect."
                || text == "Choose a projectile."
            {
                log.ui.push(text.clone());
            }
        }
        for vague in ["Unnamed Key", "Option ", "Unnamed State"] {
            let hits = shown
                .iter()
                .filter(|(text, _)| text.contains(vague))
                .count();
            if hits > 0 {
                log.ui.push(format!("{hits}× {vague}"));
            }
        }
        let after = &self.app.perk_workbench.documents[selected].recipe;
        if after != perk {
            log.problems.push("drawing the perk changed it".into());
            std::fs::create_dir_all(diffs).unwrap();
            for (side, recipe) in [("before", perk), ("after", after)] {
                std::fs::write(
                    diffs.join(format!("{:02}-{side}.json", log.number)),
                    serde_json::to_string_pretty(recipe).unwrap(),
                )
                .unwrap();
            }
        }
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
        match native_perks::program::compile(manager, program) {
            Ok(compiled) => match signature(&compiled.payload) {
                Ok(shape) => {
                    log.compiled.push(shape);
                    log.payloads.push(compiled.payload);
                }
                Err(error) => log.problems.push(format!(
                    "effect {}: compiled action does not decode: {error}",
                    position + 1
                )),
            },
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

#[test]
#[ignore = "requires PARHELION_WORKBENCH_INSTALL, PARHELION_WORKBENCH_CATALOG and PARHELION_CLEAN_STOCK_PACKAGES"]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn seeded_custom_perks_author_build_stage_and_read_back() {
    let install = env_path("PARHELION_WORKBENCH_INSTALL");
    let cache = env_path("PARHELION_WORKBENCH_CATALOG");
    let stock = env_path("PARHELION_CLEAN_STOCK_PACKAGES");
    let seed = std::env::var("PARHELION_SMOKE_SEED")
        .ok()
        .and_then(|text| u64::from_str_radix(text.trim_start_matches("0x"), 16).ok())
        .unwrap_or(DEFAULT_SEED);
    let count = std::env::var("PARHELION_SMOKE_PERKS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(DEFAULT_PERKS);
    let out = std::env::var_os("PARHELION_SMOKE_OUT").map_or_else(
        || std::env::temp_dir().join(format!("parhelion-smoke-{seed:x}")),
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
    let manager = open_shadowkeep_package_manager(&stock).expect("clean stock packages");
    let globals = manager
        .read_tag(resolve_live_named_tag(&manager, "investment_globals", None).unwrap())
        .unwrap();
    let mut indices = choices
        .iter()
        .map(|choice| choice.perk_index)
        .collect::<Vec<_>>();
    indices.sort_unstable();
    indices.dedup();
    let pools = Pools::harvest(&manager, &globals, &indices);
    let kinds = Kinds::authorable(&pools);
    let stats = catalog.perk_stat_choices();
    let mut report = Report {
        seed: format!("{seed:x}"),
        perks: count,
        condition_kinds: kinds.conditions.len(),
        action_kinds: kinds.actions.len(),
        ..Report::default()
    };

    // Weapons of different types, as a user picks gameplay weapons to put perks on.
    let mut rng = Rng(seed);
    let mut donors = catalog
        .weapon_donors()
        .into_iter()
        .filter(|summary| {
            crate::package_profile::is_stock_item_definition(summary.hash)
                && !summary.type_name.trim().is_empty()
        })
        .collect::<Vec<_>>();
    donors.sort_by(|a, b| (&a.type_name, &a.name, a.hash).cmp(&(&b.type_name, &b.name, b.hash)));
    let mut by_type = BTreeMap::<String, Vec<u32>>::new();
    for donor in &donors {
        by_type
            .entry(donor.type_name.clone())
            .or_default()
            .push(donor.hash);
    }
    let mut types = by_type.keys().cloned().collect::<Vec<_>>();
    let mut weapons = Vec::new();
    while weapons.len() < WEAPONS && !types.is_empty() {
        let type_name = types.remove(rng.below(types.len()));
        let hashes = &by_type[&type_name];
        let Some(donor) = catalog.weapon_donor(*rng.pick(hashes)) else {
            continue;
        };
        let name = format!("Smoke Test {}", weapons.len() + 1);
        let Ok(mut recipe) = WeaponRecipe::new_named_weapon_for_donor(
            &name,
            donor.summary.hash,
            &donor.summary.name,
        ) else {
            continue;
        };
        // A donor with no ammo type of its own asks for one, as Trust and Polaris Lance do.
        if donor.summary.ammo_type.is_none() {
            recipe.overrides.ammo_type = Some(crate::recipe::RecipeAmmoType::Special);
        }
        report.weapons.push(format!(
            "{name}: {} ({})",
            donor.summary.name, donor.summary.type_name
        ));
        weapons.push((recipe, donor));
    }
    assert!(!weapons.is_empty(), "no stock weapon donors");

    // Author each perk on its cards, let the workbench check it and look at it.
    let mut harness = Harness::open(&install, &cache, &weapons[0].0);
    let mut author = Author {
        rng: Rng(seed ^ 0xA5A5_A5A5),
        pools: &pools,
        kinds: &kinds,
    };
    let mut perks = Vec::new();
    let mut attempts = 0;
    while perks.len() < count && attempts < count * 5 {
        attempts += 1;
        let number = perks.len() + 1;
        let mut perk = PerkRecipe::new();
        perk.id = format!("5a0e-{seed:x}-{number:03x}-{attempts:03x}");
        let mut log = PerkLog {
            number,
            ..PerkLog::default()
        };
        let effects = 1 + usize::from(author.rng.chance(40)) + usize::from(author.rng.chance(15));
        for position in 0..effects {
            // Add Effect takes the workbench's next free layout index.
            let Some(index) = harness
                .app
                .perk_workbench
                .free_metadata_index(&perk, &choices)
            else {
                log.problems.push("Add Effect found no free index".into());
                break;
            };
            match author.effect(index, position + 1) {
                Ok((effect, effect_log)) => {
                    perk.effects.push(effect);
                    log.effects.push(effect_log);
                }
                Err(error) => {
                    log.problems
                        .push(format!("authoring effect {}: {error}", position + 1));
                    break;
                }
            }
        }
        if author.rng.chance(30) {
            for _ in 0..1 + author.rng.below(2) {
                let stat = author.rng.pick(&stats);
                if perk
                    .stats
                    .iter()
                    .any(|entry| entry.definition_index == stat.definition_index)
                {
                    continue;
                }
                let value = [-20, -10, 5, 10, 15, 25][author.rng.below(6)];
                perk.stats.push(crate::WeaponStatOverride {
                    definition_index: stat.definition_index,
                    value,
                });
                log.stats.push(format!("{} {value:+}", stat.name));
            }
        }
        let headline = log
            .effects
            .first()
            .and_then(|effect| {
                effect
                    .steps
                    .iter()
                    .find_map(|step| step.strip_prefix("Action: "))
            })
            .map(|text| text.split(" [").next().unwrap_or(text).to_owned())
            .unwrap_or_else(|| "Effect".into());
        perk.name = format!("Smoke {number:02} {headline}");
        perk.description = format!("Seeded smoke perk {number} of {count}.");
        log.id.clone_from(&perk.id);
        log.name.clone_from(&perk.name);
        // Apply to Weapon stays disabled while the workbench reports a problem. A warning, such
        // as a behavior that never ends, is shown but leaves the perk free to apply.
        if let Some(issue) = harness
            .app
            .perk_workbench
            .validation_issue(&perk)
            .filter(|issue| issue.blocking)
        {
            report.blocked.push(Blocked {
                name: perk.name.clone(),
                issue: issue.message,
                effects: log.effects,
            });
            continue;
        }
        for effect in &log.effects {
            for step in &effect.steps {
                let Some((role, rest)) = step.split_once(": ") else {
                    continue;
                };
                if ["Action", "Or", "And", "Or Ending", "Counter Contribution"].contains(&role)
                    || role.ends_with("Trigger")
                {
                    let kind = rest.split(" [").next().unwrap_or(rest);
                    *report.kinds_used.entry(kind.to_owned()).or_default() += 1;
                }
            }
        }
        check(&manager, &perk, &mut log);
        harness.show(&perk, &mut log, &out.join("diffs"));
        let workbench = &harness.app.perk_workbench;
        let plan = crate::app::custom_perks::workbench::test_plan::render(
            &perk,
            crate::ItemKind::Weapon,
            &workbench.perk_names,
            Some(&workbench.keys.catalog),
            &workbench.asset_labels,
        );
        std::fs::write(out.join("plans").join(format!("{number:02}.md")), plan).unwrap();
        std::fs::write(
            out.join("perks").join(format!("{number:02}.perk.json")),
            serde_json::to_string_pretty(&perk).unwrap(),
        )
        .unwrap();
        perks.push((perk, log));
    }

    // Apply to Weapon: each perk takes a new choice in a perk socket, or replaces one.
    for (position, (perk, log)) in perks.iter_mut().enumerate() {
        if !log.problems.is_empty() {
            continue;
        }
        let slot = position % weapons.len();
        let (weapon, donor) = &mut weapons[slot];
        let mut destinations = Vec::new();
        for socket in &donor.sockets {
            let best = (0..=10)
                .rev()
                .find_map(|choice| Target::capture(weapon, donor, socket.index, choice).ok());
            if let Some(target) = best {
                let label = target.label(donor, &catalog);
                if PERK_ROLES.iter().any(|role| label.contains(role)) {
                    destinations.push((target, label));
                }
            }
        }
        if destinations.is_empty() {
            log.problems
                .push(format!("no perk socket on {}", donor.summary.name));
            continue;
        }
        let (target, label) = destinations.remove(author.rng.below(destinations.len()));
        let change = Change {
            target,
            perk: Some(perk.clone()),
        };
        match change.apply(weapon, donor) {
            Ok(()) => {
                log.weapon = format!("{} ({})", weapon.name, donor.summary.name);
                log.socket = label;
            }
            Err(error) => log.problems.push(format!("apply to weapon: {error}")),
        }
    }
    let mut invalid = Vec::new();
    for (weapon, _) in &weapons {
        if let Err(error) = weapon.validate() {
            invalid.push(format!("{}: {error}", weapon.name));
        }
        let path = out
            .join("weapons")
            .join(format!("{}.parhelion.json", weapon.namespace));
        std::fs::write(path, serde_json::to_string_pretty(weapon).unwrap()).unwrap();
    }

    // Build and stage every weapon, then read each built perk back from the staged packages.
    let recipes = weapons
        .iter()
        .map(|(weapon, _)| weapon.clone())
        .collect::<Vec<_>>();
    let built = if invalid.is_empty() {
        crate::workflow::BatchBuildSnapshot::new(crate::workflow::BatchBuildRequest {
            package_directory: stock.clone(),
            staging_root: out.join("staging"),
            ignore_installed_authored_overlays: true,
            recipes,
        })
        .and_then(|snapshot| {
            crate::workflow::build_and_stage_snapshot_with_progress(&snapshot, |_| {})
        })
    } else {
        Err(format!("weapons fail validation: {}", invalid.join("; ")))
    };
    match built {
        Ok(build) => {
            report.build = format!(
                "Built {} weapons into {} artifacts",
                build.weapons.len(),
                build.artifacts.len()
            );
            report.staging = build.run_directory.display().to_string();
            if let Err(error) = read_back(&stock, &build, &mut perks) {
                report.build = format!("{}. Reading back failed: {error}", report.build);
            }
        }
        Err(error) => report.build = format!("Build failed: {error}"),
    }

    report.results = perks.into_iter().map(|(_, log)| log).collect();
    std::fs::write(
        out.join("report.json"),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
    std::fs::write(out.join("report.md"), markdown(&report)).unwrap();
    eprintln!("Smoke report: {}", out.join("report.md").display());
    let failed = report
        .results
        .iter()
        .filter(|log| !log.problems.is_empty())
        .map(|log| format!("{}: {}", log.name, log.problems.join("; ")))
        .collect::<Vec<_>>();
    assert_eq!(report.results.len(), count, "too many drafts were blocked");
    assert!(
        report.build.starts_with("Built") && !report.build.contains("failed"),
        "{}",
        report.build
    );
    assert!(
        failed.is_empty(),
        "{} perks failed:\n{}",
        failed.len(),
        failed.join("\n")
    );
}

/// Opens the clean stock packages with the staged run laid over them, as the build's own check
/// does, and compares every built private perk's action with what its program compiled to.
fn read_back(
    stock: &Path,
    build: &crate::workflow::BuildReport,
    perks: &mut [(PerkRecipe, PerkLog)],
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
    for (perk, log) in perks.iter_mut() {
        if !log.problems.is_empty() || log.weapon.is_empty() {
            continue;
        }
        let Some(entries) = built.get(&perk.name) else {
            log.problems
                .push("the build report lists no private perk for it".into());
            continue;
        };
        if entries.len() != perk.effects.len() {
            log.problems.push(format!(
                "{} effects authored, {} private perks built",
                perk.effects.len(),
                entries.len()
            ));
        }
        compare(perk, log, entries, &runtime_map, &manager);
    }
    Ok(())
}

/// Compares each private perk the build made for one authored perk with what its effect
/// compiled to: the same decoded shape, and whether the bytes are identical too.
fn compare(
    perk: &PerkRecipe,
    log: &mut PerkLog,
    entries: &[(u32, usize)],
    runtime_map: &[u8],
    manager: &sundial::package_authoring::PackageManager,
) {
    for (runtime_key, source_index) in entries {
        let position = perk
            .effects
            .iter()
            .position(|effect| usize::from(effect.source_perk_index) == *source_index);
        let expected = position.and_then(|position| {
            Some((
                log.compiled.get(position)?.clone(),
                log.payloads.get(position)?.clone(),
            ))
        });
        let staged = native_perks::sandbox_perk_runtime_assignment(runtime_map, *runtime_key)
            .ok()
            .flatten()
            .and_then(|assignment| assignment.action_tag())
            .ok_or_else(|| format!("runtime key 0x{runtime_key:08X} has no staged action"))
            .and_then(|tag| manager.read_tag(tag).map_err(|error| error.to_string()));
        match (staged, expected) {
            (Ok(payload), Some((shape, compiled))) => match signature(&payload) {
                Ok(built_shape) if built_shape == shape => {
                    log.staged.push(built_shape);
                    log.identical_bytes.push(payload == compiled);
                    match event_masks(&payload) {
                        Ok((checked, wrong)) => {
                            log.event_masks.extend(checked);
                            log.problems.extend(wrong);
                        }
                        Err(error) => log.problems.push(format!("event masks: {error}")),
                    }
                }
                Ok(built_shape) => log.problems.push(format!(
                    "staged action differs: built {built_shape} but authored {shape}"
                )),
                Err(error) => log
                    .problems
                    .push(format!("staged action does not decode: {error}")),
            },
            (Ok(_), None) => log
                .problems
                .push("a staged action matches no authored effect".into()),
            (Err(error), _) => log
                .problems
                .push(format!("staged action unreadable: {error}")),
        }
    }
}

/// The event masks a staged action carries for its counters and requirements, checked against
/// the rule the clean stock packages follow rather than against the compiler: a counter stores
/// the kinds of its contributing conditions and their children at +0x18 (141 of 142 stock
/// counters, and the one with none stores 1), and a requirement row stores the kinds of its
/// alternatives at +0x18. Both shipped as zero, and those counters and requirements never
/// passed in game.
fn event_masks(payload: &[u8]) -> Result<(Vec<String>, Vec<String>), String> {
    fn kinds(node: &action::DecodedCondition) -> u64 {
        let nested = node.children.iter().map(kinds);
        let alternatives = node
            .subgroups
            .iter()
            .flat_map(|subgroup| subgroup.conditions.iter().map(kinds));
        nested
            .chain(alternatives)
            .fold(1 << node.kind, |mask, kind| mask | kind)
    }
    let stored = |at: usize| {
        payload
            .get(at..at + 8)
            .map(|bytes| u64::from_le_bytes(bytes.try_into().expect("eight bytes")))
            .ok_or("an event mask lies outside the action")
    };
    let decoded = action::decode(payload)?;
    let (mut checked, mut wrong) = (Vec::new(), Vec::new());
    for node in decoded.conditions() {
        let mut expect = Vec::new();
        if node.kind == 26 {
            let mask = node
                .children
                .iter()
                .map(kinds)
                .fold(0, |mask, kind| mask | kind);
            expect.push(("counter", node.offset, if mask == 0 { 1 } else { mask }));
        }
        for subgroup in &node.subgroups {
            let mask = subgroup
                .conditions
                .iter()
                .map(kinds)
                .fold(0, |mask, kind| mask | kind);
            expect.push(("requirement", subgroup.offset, mask));
        }
        for (what, offset, mask) in expect {
            let found = stored(offset + 0x18)?;
            checked.push(format!("{what} 0x{found:X}"));
            if found != mask {
                wrong.push(format!(
                    "{what} event mask 0x{found:X}, its conditions need 0x{mask:X}"
                ));
            }
        }
    }
    Ok((checked, wrong))
}

/// One perk's part of the report: where it went, what it was built from and how it read back.
fn perk_section(out: &mut String, log: &PerkLog) {
    let status = if log.problems.is_empty() {
        "passed"
    } else {
        "FAILED"
    };
    let _ = writeln!(out, "### {} ({status})\n", log.name);
    if !log.weapon.is_empty() {
        let _ = writeln!(out, "On {} in {}.\n", log.weapon, log.socket);
    }
    if !log.stats.is_empty() {
        let _ = writeln!(out, "Stats: {}.\n", log.stats.join(", "));
    }
    for effect in &log.effects {
        let _ = writeln!(out, "- {} ({})", effect.name, effect.form);
        for step in &effect.steps {
            let _ = writeln!(out, "  - {step}");
        }
        for refused in &effect.refused {
            let _ = writeln!(out, "  - Refused, picked again: {refused}");
        }
    }
    for (position, shape) in log.compiled.iter().enumerate() {
        let staged = match log.identical_bytes.get(position) {
            Some(true) => "read back, identical bytes",
            Some(false) => "read back, same shape",
            None => "not read back",
        };
        let _ = writeln!(out, "- Compiled {}: `{shape}` ({staged})", position + 1);
    }
    if !log.event_masks.is_empty() {
        let _ = writeln!(
            out,
            "- Event masks read back: {}",
            log.event_masks.join(", ")
        );
    }
    for text in &log.ui {
        let _ = writeln!(out, "- Shown: {text}");
    }
    for problem in &log.problems {
        let _ = writeln!(out, "- **Problem:** {problem}");
    }
    out.push('\n');
}

fn markdown(report: &Report) -> String {
    let mut out = String::new();
    let failed = report
        .results
        .iter()
        .filter(|log| !log.problems.is_empty())
        .count();
    let _ = writeln!(out, "# Custom perk smoke run\n");
    let _ = writeln!(
        out,
        "Seed `{}`. {} perks, {} passed and {} failed. {} drafts were blocked by the workbench and authored again. {} authorable conditions and {} authorable actions were available.\n",
        report.seed,
        report.results.len(),
        report.results.len() - failed,
        failed,
        report.blocked.len(),
        report.condition_kinds,
        report.action_kinds
    );
    let _ = writeln!(out, "Build: {}\n", report.build);
    let _ = writeln!(
        out,
        "A built action is read back from the staged packages by its runtime key. The same shape with different bytes is expected where Change Incoming Damage appears, since the build points its self-reference at the new action.\n"
    );
    if !report.staging.is_empty() {
        let _ = writeln!(out, "Staged run: `{}`\n", report.staging);
    }
    let _ = writeln!(out, "## Weapons\n");
    for weapon in &report.weapons {
        let _ = writeln!(out, "- {weapon}");
    }
    let _ = writeln!(out, "\n## Perks\n");
    for log in &report.results {
        perk_section(&mut out, log);
    }
    if !report.blocked.is_empty() {
        let _ = writeln!(out, "## Drafts the workbench blocked\n");
        for blocked in &report.blocked {
            let _ = writeln!(out, "- {}: {}", blocked.name, blocked.issue);
        }
        out.push('\n');
    }
    let _ = writeln!(out, "## Kinds used\n");
    for (kind, uses) in &report.kinds_used {
        let _ = writeln!(out, "- {kind}: {uses}");
    }
    out
}
