//! Diagnostic report: how many stock perks open in the workbench with the custom-perk flow.
//!
//! Run with `cargo test --release -p sundial --lib -- --ignored --nocapture decompile_coverage`.
//! Every captured stock action is decoded and recovered through `program::decompile`, the
//! same path the workbench uses to open a stock perk. Each action lands in one of three
//! tiers: it cannot open, it opens as a fully typed program, or it opens while carrying
//! native nodes. Carried kinds are cross-referenced with the template field map so the
//! report says whether they render with named fields or with opaque native bytes.
//!
//! This lives beside the field map because it shares the capture readers and the map.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use super::field_map::{Context, action_tag, is_synthetic, opaque_bytes_by_kind, survey_dir};
use super::record;
use crate::sandbox_perk::action::native::{Graph, fields};
use crate::sandbox_perk::action::{ACTION_ROOT_CLASS, decode};
use crate::sandbox_perk::nodes;
use crate::sandbox_perk::program::decompile::{Unsupported, decompile};
use crate::sandbox_perk::program::{Action, Trigger};

const REPORT: &str = "docs/stock-perk-decompile-coverage-2026-09-16.md";

enum Outcome {
    Undecodable(String),
    Unsupported(String),
    Opened {
        trigger: Trigger,
        native: Vec<(bool, u8)>,
    },
}

fn examine(data: &[u8], name: &str) -> Outcome {
    let decoded = match decode(data) {
        Ok(decoded) => decoded,
        Err(error) => return Outcome::Undecodable(error),
    };
    match decompile(&decoded, name, |tag| tag) {
        Err(Unsupported(message)) => Outcome::Unsupported(message),
        Ok(program) => {
            let mut native = Vec::new();
            if let Some(node) = &program.native_trigger {
                native.push((true, node.kind));
            }
            if let Some(node) = &program.native_removal {
                native.push((true, node.kind));
            }
            for action in &program.actions {
                if let Action::Native { node } = action {
                    native.push((false, node.kind));
                }
            }
            Outcome::Opened {
                trigger: program.trigger,
                native,
            }
        }
    }
}

/// Collapses numbers and hashes so one message shape counts once.
fn normalize(message: &str) -> String {
    message
        .split_whitespace()
        .map(|token| {
            let core = token.trim_matches(|c: char| !c.is_alphanumeric());
            if core.starts_with("0x")
                || core.chars().all(|c| c.is_ascii_digit()) && !core.is_empty()
            {
                token.replace(core, "#")
            } else {
                token.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn kind_name(condition: bool, kind: u8) -> String {
    let entry = if condition {
        nodes::condition(kind)
    } else {
        nodes::effect(kind)
    };
    let family = if condition { "condition" } else { "effect" };
    entry.map_or_else(
        || format!("{family} {kind} (unknown kind)"),
        |node| format!("{family} {kind} {}", node.name),
    )
}

#[derive(Default)]
struct Reasons(BTreeMap<String, (usize, BTreeSet<u32>)>);

impl Reasons {
    fn add(&mut self, reason: String, tag: u32) {
        let entry = self.0.entry(reason).or_default();
        entry.0 += 1;
        entry.1.insert(tag);
    }
    fn total(&self) -> usize {
        self.0.values().map(|(count, _)| count).sum()
    }
    fn ranked(&self) -> Vec<(&String, &(usize, BTreeSet<u32>))> {
        let mut ranked: Vec<_> = self.0.iter().collect();
        ranked.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(b.0)));
        ranked
    }
}

#[derive(Default)]
struct Coverage {
    actions: usize,
    synthetic: usize,
    unreadable: usize,
    undecodable: Reasons,
    unsupported: Reasons,
    typed: usize,
    with_native: usize,
    /// Per carried kind: actions carrying it, nodes carried, example tags.
    native_kinds: BTreeMap<(bool, u8), (usize, usize, BTreeSet<u32>)>,
    triggers: BTreeMap<&'static str, usize>,
    context: Context,
}

impl Coverage {
    fn opened(&self) -> usize {
        self.typed + self.with_native
    }

    fn record(&mut self, tag: u32, outcome: Outcome) {
        match outcome {
            Outcome::Undecodable(error) => self.undecodable.add(normalize(&error), tag),
            Outcome::Unsupported(message) => self.unsupported.add(normalize(&message), tag),
            Outcome::Opened { trigger, native } => {
                *self.triggers.entry(trigger.label()).or_default() += 1;
                if native.is_empty() {
                    self.typed += 1;
                    return;
                }
                self.with_native += 1;
                let distinct: BTreeSet<(bool, u8)> = native.iter().copied().collect();
                for kind in &native {
                    self.native_kinds.entry(*kind).or_default().1 += 1;
                }
                for kind in distinct {
                    let entry = self.native_kinds.entry(kind).or_default();
                    entry.0 += 1;
                    entry.2.insert(tag);
                }
            }
        }
    }
}

fn run() -> Option<Coverage> {
    let dir = survey_dir()?;
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut coverage = Coverage {
        context: Context::load(&dir),
        ..Coverage::default()
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.extension().is_none_or(|extension| extension != "bin") {
            continue;
        }
        coverage.actions += 1;
        let Some(tag) = action_tag(&path) else {
            coverage.unreadable += 1;
            continue;
        };
        if is_synthetic(tag) {
            coverage.synthetic += 1;
            continue;
        }
        let Ok(data) = std::fs::read(&path) else {
            coverage.unreadable += 1;
            continue;
        };
        let outcome = examine(&data, &format!("0x{tag:08X}"));
        coverage.record(tag, outcome);
    }
    Some(coverage)
}

fn percent(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "n/a".into();
    }
    format!("{:.1}%", 100.0 * part as f64 / whole as f64)
}

fn write_summary(out: &mut String, c: &Coverage) {
    let stock = c.actions - c.synthetic - c.unreadable;
    let cannot = c.undecodable.total() + c.unsupported.total();
    let _ = writeln!(out, "# Stock Perk Decompile Coverage, September 16, 2026\n");
    let _ = writeln!(
        out,
        "How many captured stock actions open in the workbench through `program::decompile`, the \
         path a stock perk takes to appear with the custom-perk flow. Generated by \
         `schema::decompile_coverage::report_stock_perk_decompile_coverage`.\n"
    );
    let _ = writeln!(
        out,
        "## Summary\n\n| Tier | Actions | Share of stock |\n|---|---|---|"
    );
    let _ = writeln!(out, "| Stock actions examined | {stock} | 100% |");
    let _ = writeln!(
        out,
        "| Cannot open (undecodable or unsupported) | {cannot} | {} |",
        percent(cannot, stock)
    );
    let _ = writeln!(
        out,
        "| Opens as a fully typed program | {} | {} |",
        c.typed,
        percent(c.typed, stock)
    );
    let _ = writeln!(
        out,
        "| Opens carrying native nodes | {} | {} |",
        c.with_native,
        percent(c.with_native, stock)
    );
    let _ = writeln!(
        out,
        "| Opens at all | {} | {} |",
        c.opened(),
        percent(c.opened(), stock)
    );
    let _ = writeln!(
        out,
        "\n{} synthetic pattern actions skipped, {} files unreadable. Perk names come from the \
         capture context ({} perks named).\n",
        c.synthetic,
        c.unreadable,
        c.context.names.len()
    );
    let _ = writeln!(
        out,
        "### Triggers of opened programs\n\n| Trigger | Actions |\n|---|---|"
    );
    let mut triggers: Vec<_> = c.triggers.iter().collect();
    triggers.sort_by(|a, b| b.1.cmp(a.1));
    for (label, count) in triggers {
        let _ = writeln!(out, "| {label} | {count} |");
    }
}

fn write_reasons(out: &mut String, title: &str, intro: &str, reasons: &Reasons, c: &Coverage) {
    let _ = writeln!(out, "\n## {title}\n\n{intro}\n");
    let _ = writeln!(out, "| Reason | Actions | Perks |\n|---|---|---|");
    for (reason, (count, tags)) in reasons.ranked() {
        let _ = writeln!(out, "| {reason} | {count} | {} |", c.context.describe(tags));
    }
}

fn write_native_kinds(out: &mut String, c: &Coverage) {
    let opaque = opaque_bytes_by_kind();
    let _ = writeln!(out, "\n## Native Nodes Carried by Opened Programs\n");
    let _ = writeln!(
        out,
        "A carried node renders through the native field editor. When its kind has no unmapped \
         non-zero template bytes, every field it shows is named and the flow matches a custom \
         perk. Otherwise the count is how many opaque `Native Value` bytes the editor exposes.\n"
    );
    let _ = writeln!(
        out,
        "| Kind | Actions | Nodes | Opaque bytes | Perks |\n|---|---|---|---|---|"
    );
    let mut ranked: Vec<_> = c.native_kinds.iter().collect();
    ranked.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(b.0)));
    for ((condition, kind), (actions, nodes, tags)) in ranked {
        let _ = writeln!(
            out,
            "| {} | {actions} | {nodes} | {} | {} |",
            kind_name(*condition, *kind),
            opaque
                .get(&(*condition, *kind))
                .map_or_else(|| "no template".to_owned(), |bytes| bytes.to_string()),
            c.context.describe(tags)
        );
    }
}

#[test]
#[ignore = "diagnostic report writer, run explicitly with --ignored --nocapture"]
fn report_stock_perk_decompile_coverage() {
    let coverage = run().expect("captured stock survey available");
    let mut out = String::new();
    write_summary(&mut out, &coverage);
    write_reasons(
        &mut out,
        "Unsupported Programs",
        "Actions that decode but lie outside the shape the program model can hold. Each row is \
         one refusal shape from `program::decompile`, ranked by how many stock perks hit it.",
        &coverage.unsupported,
        &coverage,
    );
    write_reasons(
        &mut out,
        "Undecodable Actions",
        "Actions the native decoder itself rejected before recovery could start.",
        &coverage.undecodable,
        &coverage,
    );
    write_native_kinds(&mut out, &coverage);
    write_auxiliary_study(&mut out, &coverage.context);
    write_policy_study(&mut out, &coverage.context);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(REPORT);
    std::fs::write(&path, &out).expect("write report");
    let stock = coverage.actions - coverage.synthetic - coverage.unreadable;
    println!(
        "decompile coverage: {stock} stock actions, {} cannot open ({} unsupported, {} undecodable), {} fully typed, {} carrying native nodes, {} carried kinds, report {}",
        coverage.undecodable.total() + coverage.unsupported.total(),
        coverage.unsupported.total(),
        coverage.undecodable.total(),
        coverage.typed,
        coverage.with_native,
        coverage.native_kinds.len(),
        path.display()
    );
}

/// Root array descriptor holding the auxiliary records, and its pointer word.
const AUXILIARY_DESCRIPTOR: usize = 0x10;
const AUXILIARY_POINTER: usize = 0x18;

#[derive(Default)]
struct AuxiliaryStudy {
    actions: usize,
    counts: BTreeMap<usize, usize>,
    classes: BTreeMap<u32, (usize, usize, BTreeSet<u32>)>,
    /// Per class and field offset: distinct values seen across every row.
    fields: BTreeMap<(u32, usize), BTreeMap<Vec<u8>, usize>>,
    /// Per class: classes of the blocks its rows point at, with how often.
    targets: BTreeMap<u32, BTreeMap<u32, usize>>,
}

impl AuxiliaryStudy {
    fn record(&mut self, graph: &Graph, tag: u32) {
        let root = &graph.blocks[0];
        let Some(&index) = root
            .links
            .get(&AUXILIARY_POINTER)
            .or_else(|| root.links.get(&AUXILIARY_DESCRIPTOR))
        else {
            return;
        };
        let block = &graph.blocks[index];
        let rows = block.count.unwrap_or(1);
        self.actions += 1;
        *self.counts.entry(rows).or_default() += 1;
        let entry = self.classes.entry(block.class).or_default();
        entry.0 += 1;
        entry.1 += rows;
        entry.2.insert(tag);
        let mut visited = BTreeSet::new();
        self.walk(graph, index, 0, &mut visited);
    }

    /// Tabulates one block's fields and link targets, then follows the links a few levels
    /// down so the records the auxiliary rows point at are characterised too.
    fn walk(&mut self, graph: &Graph, index: usize, depth: usize, visited: &mut BTreeSet<usize>) {
        if depth > 3 || !visited.insert(index) {
            return;
        }
        let block = &graph.blocks[index];
        if block.class == 0 {
            return;
        }
        let rows = block.count.unwrap_or(1);
        if let (Ok(stride), Ok(described)) = (
            record(block.class).map(|record| record.size),
            fields::describe(block.class),
        ) {
            for row in 0..rows {
                for field in &described {
                    let at = row * stride + field.offset;
                    if let Some(value) = block.bytes.get(at..at + field.width) {
                        *self
                            .fields
                            .entry((block.class, field.offset))
                            .or_default()
                            .entry(value.to_vec())
                            .or_default() += 1;
                    }
                }
            }
        }
        for target in block.links.values() {
            *self
                .targets
                .entry(block.class)
                .or_default()
                .entry(graph.blocks[*target].class)
                .or_default() += 1;
            self.walk(graph, *target, depth + 1, visited);
        }
    }
}

fn auxiliary_study() -> Option<AuxiliaryStudy> {
    let dir = survey_dir()?;
    let mut study = AuxiliaryStudy::default();
    for path in std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
    {
        if path.extension().is_none_or(|extension| extension != "bin") {
            continue;
        }
        let Some(tag) = action_tag(&path) else {
            continue;
        };
        if is_synthetic(tag) {
            continue;
        }
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        if let Ok(graph) = Graph::read(&data, 0, ACTION_ROOT_CLASS) {
            study.record(&graph, tag);
        }
    }
    Some(study)
}

fn write_auxiliary_study(out: &mut String, context: &Context) {
    let Some(study) = auxiliary_study() else {
        return;
    };
    let _ = writeln!(out, "\n## Auxiliary Records\n");
    let _ = writeln!(
        out,
        "The root array at +0x{AUXILIARY_DESCRIPTOR:02X}, which `decode` counts but does not read. \
         {} stock actions carry it.\n",
        study.actions
    );
    let _ = writeln!(out, "| Rows per action | Actions |\n|---|---|");
    for (rows, count) in &study.counts {
        let _ = writeln!(out, "| {rows} | {count} |");
    }
    let _ = writeln!(
        out,
        "\n| Row class | Name | Actions | Rows | Perks |\n|---|---|---|---|---|"
    );
    for (class, (actions, rows, tags)) in &study.classes {
        let _ = writeln!(
            out,
            "| 0x{class:08X} | {} | {actions} | {rows} | {} |",
            fields::name(*class),
            context.describe(tags)
        );
    }
    let _ = writeln!(
        out,
        "\n| Row class | Offset | Label | Distinct values | Top values |\n|---|---|---|---|---|"
    );
    for ((class, offset), values) in &study.fields {
        let label = fields::describe(*class)
            .ok()
            .and_then(|fields| fields.into_iter().find(|f| f.offset == *offset))
            .map_or_else(String::new, |f| f.label);
        let mut ranked: Vec<_> = values.iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        let top = ranked
            .iter()
            .take(4)
            .map(|(value, count)| format!("{}×{count}", hex(value)))
            .collect::<Vec<_>>()
            .join(" ");
        let _ = writeln!(
            out,
            "| 0x{class:08X} | 0x{offset:03X} | {label} | {} | {top} |",
            values.len()
        );
    }
    let _ = writeln!(out, "\n| Row class | Points at | Links |\n|---|---|---|");
    for (class, targets) in &study.targets {
        for (target, count) in targets {
            let _ = writeln!(
                out,
                "| 0x{class:08X} | 0x{target:08X} {} | {count} |",
                fields::name(*target)
            );
        }
    }
}

/// Root pointer to the execution policy's configuration record, and the selector byte.
const POLICY_CONFIGURATION: usize = 0x78;
const POLICY_SELECTOR: usize = 0xB8;
/// Root fields around the selector whose role is not resolved, as (offset, width).
const POLICY_ROOT_FIELDS: [(usize, usize); 8] = [
    (0x80, 4),
    (0xB9, 1),
    (0xBA, 1),
    (0xBB, 1),
    (0xBC, 4),
    (0xC0, 4),
    (0xC4, 4),
    (0xC8, 4),
];

#[derive(Default)]
struct PolicyStudy {
    actions: usize,
    /// Per selector value: actions and perks.
    selectors: BTreeMap<u8, (usize, BTreeSet<u32>)>,
    /// Per root field offset: distinct values across policy actions.
    root_fields: BTreeMap<usize, BTreeMap<Vec<u8>, usize>>,
    /// Per selector value: configuration record classes, or zero when there is none.
    configurations: BTreeMap<(u8, u32), usize>,
    /// Per configuration class: how many rows link onward, and to which classes.
    targets: BTreeMap<u32, BTreeMap<u32, usize>>,
    /// Per configuration class and field offset: distinct values.
    fields: BTreeMap<(u32, usize), BTreeMap<Vec<u8>, usize>>,
}

impl PolicyStudy {
    fn record(&mut self, graph: &Graph, tag: u32) {
        let root = &graph.blocks[0];
        let selector = root.bytes[POLICY_SELECTOR];
        let configuration = root.links.get(&POLICY_CONFIGURATION).copied();
        if selector == 0 && configuration.is_none() {
            return;
        }
        self.actions += 1;
        let entry = self.selectors.entry(selector).or_default();
        entry.0 += 1;
        entry.1.insert(tag);
        for (offset, width) in POLICY_ROOT_FIELDS {
            *self
                .root_fields
                .entry(offset)
                .or_default()
                .entry(root.bytes[offset..offset + width].to_vec())
                .or_default() += 1;
        }
        let Some(index) = configuration else {
            *self.configurations.entry((selector, 0)).or_default() += 1;
            return;
        };
        let block = &graph.blocks[index];
        *self
            .configurations
            .entry((selector, block.class))
            .or_default() += 1;
        for target in block.links.values() {
            *self
                .targets
                .entry(block.class)
                .or_default()
                .entry(graph.blocks[*target].class)
                .or_default() += 1;
        }
        let Ok(described) = fields::describe(block.class) else {
            return;
        };
        for field in &described {
            if let Some(value) = block.bytes.get(field.offset..field.offset + field.width) {
                *self
                    .fields
                    .entry((block.class, field.offset))
                    .or_default()
                    .entry(value.to_vec())
                    .or_default() += 1;
            }
        }
    }
}

fn policy_study() -> Option<PolicyStudy> {
    let dir = survey_dir()?;
    let mut study = PolicyStudy::default();
    for path in std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
    {
        if path.extension().is_none_or(|extension| extension != "bin") {
            continue;
        }
        let Some(tag) = action_tag(&path) else {
            continue;
        };
        if is_synthetic(tag) {
            continue;
        }
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        if let Ok(graph) = Graph::read(&data, 0, ACTION_ROOT_CLASS) {
            study.record(&graph, tag);
        }
    }
    Some(study)
}

fn write_policy_study(out: &mut String, context: &Context) {
    let Some(study) = policy_study() else {
        return;
    };
    let _ = writeln!(out, "\n## Execution Policies\n");
    let _ = writeln!(
        out,
        "Stock actions whose root selects a policy at +0x{POLICY_SELECTOR:02X} or links a \
         configuration record at +0x{POLICY_CONFIGURATION:02X}: {}.\n",
        study.actions
    );
    let _ = writeln!(out, "| Selector | Actions | Perks |\n|---|---|---|");
    for (selector, (actions, tags)) in &study.selectors {
        let _ = writeln!(
            out,
            "| {selector} | {actions} | {} |",
            context.describe(tags)
        );
    }
    let _ = writeln!(
        out,
        "\n| Selector | Configuration class | Actions |\n|---|---|---|"
    );
    for ((selector, class), count) in &study.configurations {
        let name = if *class == 0 {
            "none".to_owned()
        } else {
            format!("0x{class:08X} {}", fields::name(*class))
        };
        let _ = writeln!(out, "| {selector} | {name} | {count} |");
    }
    let _ = writeln!(
        out,
        "\n| Root offset | Distinct values | Top values |\n|---|---|---|"
    );
    for (offset, values) in &study.root_fields {
        let _ = writeln!(
            out,
            "| 0x{offset:02X} | {} | {} |",
            values.len(),
            top_values(values)
        );
    }
    let _ = writeln!(
        out,
        "\n| Configuration class | Offset | Label | Distinct values | Top values |\n|---|---|---|---|---|"
    );
    for ((class, offset), values) in &study.fields {
        let label = fields::describe(*class)
            .ok()
            .and_then(|fields| fields.into_iter().find(|f| f.offset == *offset))
            .map_or_else(String::new, |f| f.label);
        let _ = writeln!(
            out,
            "| 0x{class:08X} | 0x{offset:03X} | {label} | {} | {} |",
            values.len(),
            top_values(values)
        );
    }
    let _ = writeln!(
        out,
        "\n| Configuration class | Points at | Links |\n|---|---|---|"
    );
    for (class, targets) in &study.targets {
        for (target, count) in targets {
            let _ = writeln!(
                out,
                "| 0x{class:08X} | 0x{target:08X} {} | {count} |",
                fields::name(*target)
            );
        }
    }
}

fn top_values(values: &BTreeMap<Vec<u8>, usize>) -> String {
    let mut ranked: Vec<_> = values.iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    ranked
        .iter()
        .take(4)
        .map(|(value, count)| format!("{}×{count}", hex(value)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn hex(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02X}")).collect()
}
