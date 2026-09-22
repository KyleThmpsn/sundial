//! Diagnostic report: how much of every native node template is semantically mapped.
//!
//! Run with `cargo test --release -p sundial --lib -- --ignored --nocapture field_map`.
//! The report is written under the ignored `docs/` directory and summarised on stdout.
//! It attributes every template byte to one of five classes so that the remaining work
//! before templates could ever be synthesised is an explicit list, not an estimate.
//!
//! When a captured stock survey is available, every unmapped byte is also tabulated across
//! all stock occurrences of its block class, so the list says whether each byte is a constant,
//! a small enumeration, a resource reference or a free parameter in the shipped game data.
//! Free parameters get a study section attributing each observed value to the stock perks
//! that carry it, which is the same evidence `fields::stock_values` was built from.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::{record, template};
use crate::hash::fnv1_name_hash;
use crate::package_runtime::labels;
use crate::sandbox_perk::action::ACTION_ROOT_CLASS;
use crate::sandbox_perk::action::native::{Graph, fields, fields::Format};
use crate::sandbox_perk::nodes;

const REPORT: &str = "docs/template-field-map-2026-09-16.md";
/// Value-program bytecode. `describe` sees opaque bytes, `program::decompile` models it fully.
const INSTRUCTION_BYTES: u32 = 0x8080_0009;
/// Compiled label mask bits, modelled completely by `native::labels`.
const LABEL_MASK_CLASSES: [u32; 2] = [0x8080_93F5, 0x8080_93F6];
const NULL_TAG: u32 = 0xFFFF_FFFF;

/// Captured stock actions, one file per action tag, used when the survey variable is unset.
const SURVEY_FALLBACK: &str = "tmp/projectile-picker-20260910/runtime/actions";
/// The capture includes nineteen synthetic pattern actions absent from the clean client.
const SYNTHETIC_FIRST: u32 = 0x80B7_7947;
const SYNTHETIC_LAST: u32 = 0x80B7_79C5;
const SYNTHETIC_STEP: u32 = 7;
/// At most this many distinct stock values still reads as an enumeration.
const ENUM_LIMIT: usize = 8;

/// The two predicate classes whose unnamed key and float pair are studied together.
const PREDICATE_CLASSES: [u32; 2] = [0x8080_3DCE, 0x8080_3DCC];
const PREDICATE_FLAGS: usize = 0x80;
const PREDICATE_KEY: usize = 0xD4;
const PREDICATE_LOW: usize = 0xD8;
const PREDICATE_HIGH: usize = 0xDC;

/// What the codebase knows about one byte range of a template.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    /// Pointer, array count, class word or string body. The decoder models these fully.
    Structural,
    /// A field with a recovered human name and contract in `fields::describe`.
    Named,
    /// Understood by another subsystem or a fixed sentinel: program bytecode, label masks,
    /// the label globals tag, the null tag, the empty key, or a key that resolves to a label.
    Decoded,
    /// Identical in every stock node of the class, recorded in `fields::fixed_values`. The
    /// value is reproducible from evidence, but its role is not resolved.
    Fixed,
    /// A key or resource hash with a known type but no recovered meaning at this site.
    TypedUnnamed,
    /// Bytes no declaration or contract covers.
    Unnamed,
}

const CLASSES: [(Class, &str); 6] = [
    (
        Class::Structural,
        "Structural (pointers, counts, class words, strings)",
    ),
    (Class::Named, "Named (recovered human contract)"),
    (
        Class::Decoded,
        "Decoded elsewhere (program bytecode, label masks, label globals tag, sentinels, resolved labels)",
    ),
    (
        Class::Fixed,
        "Fixed (identical in every stock node of the class, role not resolved)",
    ),
    (
        Class::TypedUnnamed,
        "Typed but unnamed (key or resource hash)",
    ),
    (Class::Unnamed, "Unnamed native bytes"),
];

const SHAPES: [&str; 5] = ["constant", "enum", "reference", "varying", "unobserved"];

/// Block class, byte offset and width: the identity of one byte range across every template
/// and every stock occurrence.
type SiteKey = (u32, usize, usize);

#[derive(Clone)]
struct Site {
    block: usize,
    block_class: u32,
    row: usize,
    offset: usize,
    width: usize,
    format: Format,
    label: String,
    value: Vec<u8>,
    class: Class,
    evidence: Option<String>,
}

impl Site {
    fn nonzero(&self) -> bool {
        self.value.iter().any(|byte| *byte != 0)
    }
    fn key(&self) -> SiteKey {
        (self.block_class, self.offset, self.width)
    }
}

#[derive(Default)]
struct Tally {
    bytes: [usize; 6],
    nonzero: [usize; 6],
}

impl Tally {
    fn add(&mut self, class: Class, value: &[u8]) {
        let index = class as usize;
        self.bytes[index] += value.len();
        if value.iter().any(|byte| *byte != 0) {
            self.nonzero[index] += value.len();
        }
    }
    fn merge(&mut self, other: &Self) {
        for index in 0..self.bytes.len() {
            self.bytes[index] += other.bytes[index];
            self.nonzero[index] += other.nonzero[index];
        }
    }
    fn total(&self) -> usize {
        self.bytes.iter().sum()
    }
    /// Bytes whose role is not resolved. A fixed byte is reproducible but not understood, so
    /// it counts here and keeps the strict bar honest.
    fn unmapped(&self) -> usize {
        self.bytes[Class::Fixed as usize]
            + self.bytes[Class::TypedUnnamed as usize]
            + self.bytes[Class::Unnamed as usize]
    }
    /// Non-zero bytes the workbench would show as opaque. A fixed byte is locked with its
    /// stock value and stated as such, so it is not opaque.
    fn unmapped_nonzero(&self) -> usize {
        self.nonzero[Class::TypedUnnamed as usize] + self.nonzero[Class::Unnamed as usize]
    }
}

/// Unmapped non-zero sites grouped by the block class they live in.
#[derive(Default)]
struct BlockGap {
    sites: usize,
    bytes: usize,
    offsets: BTreeSet<usize>,
}

#[derive(Default)]
struct Collected {
    overall: Tally,
    per_kind: Vec<(String, u32, Tally)>,
    unobserved: Vec<String>,
    work: BTreeMap<String, Vec<Site>>,
    content: Vec<(String, Site)>,
    by_block: BTreeMap<u32, BlockGap>,
}

impl Collected {
    fn absorb(&mut self, key: String, class: u32, tally: Tally, sites: Vec<Site>) {
        self.overall.merge(&tally);
        let (decoded, unmapped): (Vec<Site>, Vec<Site>) = sites
            .into_iter()
            .partition(|site| site.class == Class::Decoded);
        for site in decoded.into_iter().filter(incidental) {
            self.content.push((key.clone(), site));
        }
        for site in unmapped.iter().filter(|site| site.nonzero()) {
            let gap = self.by_block.entry(site.block_class).or_default();
            gap.sites += 1;
            gap.bytes += site.width;
            gap.offsets.insert(site.offset);
        }
        if !unmapped.is_empty() {
            self.work.insert(key.clone(), unmapped);
        }
        self.per_kind.push((key, class, tally));
    }

    fn strict_complete(&self) -> usize {
        self.per_kind
            .iter()
            .filter(|(_, _, tally)| tally.unmapped() == 0)
            .count()
    }

    fn lenient_complete(&self) -> usize {
        self.per_kind
            .iter()
            .filter(|(_, _, tally)| tally.unmapped_nonzero() == 0)
            .count()
    }

    fn unmapped_sites(&self) -> impl Iterator<Item = &Site> {
        self.work.values().flatten()
    }

    /// Unmapped sites of one kind, empty when every byte of the kind is mapped.
    fn sites_of(&self, key: &str) -> &[Site] {
        self.work.get(key).map_or(&[], Vec::as_slice)
    }
}

/// How one byte range behaves across every stock occurrence of its block class.
#[derive(Default)]
struct Distribution {
    occurrences: usize,
    values: BTreeMap<Vec<u8>, usize>,
}

impl Distribution {
    fn shape(&self, format: Format) -> &'static str {
        match (self.occurrences, self.values.len()) {
            (0, _) => "unobserved",
            (_, 1) => "constant",
            (_, distinct) if distinct <= ENUM_LIMIT => "enum",
            _ if matches!(format, Format::Tag | Format::Key) => "reference",
            _ => "varying",
        }
    }
    fn ranked(&self) -> Vec<(&Vec<u8>, usize)> {
        let mut ranked: Vec<_> = self
            .values
            .iter()
            .map(|(value, count)| (value, *count))
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        ranked
    }
    /// The most common stock value, ties broken by the smallest value.
    fn mode(&self) -> Option<&Vec<u8>> {
        self.ranked().first().map(|(value, _)| *value)
    }
    fn top(&self, limit: usize) -> String {
        self.ranked()
            .into_iter()
            .take(limit)
            .map(|(value, count)| format!("{}×{count}", hex(value)))
            .collect::<Vec<_>>()
            .join(" ")
    }
    fn share(&self, value: &[u8]) -> String {
        percent(
            self.values.get(value).copied().unwrap_or(0),
            self.occurrences,
        )
    }
}

/// Perk names by finished-sandbox-perk index, and perk indices by action tag, from the
/// capture's `item-context.json` and `catalog.json` when they sit beside the actions.
#[derive(Default)]
pub(super) struct Context {
    pub(super) names: BTreeMap<u64, BTreeSet<String>>,
    /// Perk indices behind each captured action tag.
    pub(super) perks: BTreeMap<u32, BTreeSet<u64>>,
    /// Plug item hashes behind each perk index.
    items: BTreeMap<u64, BTreeSet<u64>>,
    /// Perks granted by an intrinsic or armor perk that only exotic items carry.
    pub(super) exotic: BTreeSet<u64>,
    /// In-game descriptions per plug item hash, evidence for what an effect does.
    descriptions: BTreeMap<u64, String>,
}

impl Context {
    pub(super) fn load(actions: &Path) -> Self {
        let mut context = Self::default();
        let Some(runtime) = actions.parent() else {
            return context;
        };
        if let Some(value) = read_json(&runtime.join("item-context.json")) {
            context.load_names(&value);
        }
        if let Some(value) = read_json(&runtime.join("catalog.json")) {
            context.load_perks(&value);
        }
        if let Some(value) = read_json(&runtime.join("item-rarity.json")) {
            context.load_exotics(&value);
        }
        if let Some(value) = read_json(&runtime.join("item-descriptions.json")) {
            context.load_descriptions(&value);
        }
        context
    }

    fn load_descriptions(&mut self, value: &serde_json::Value) {
        let Some(plugs) = value["plugs"].as_object() else {
            return;
        };
        for (hash, plug) in plugs {
            if let (Ok(hash), Some(text)) = (hash.parse::<u64>(), plug["description"].as_str()) {
                if !text.is_empty() {
                    self.descriptions.insert(hash, text.to_owned());
                }
            }
        }
    }

    /// The in-game description of the first plug behind a perk, when the catalog has one.
    pub(super) fn description(&self, perk: u64) -> String {
        self.items
            .get(&perk)
            .into_iter()
            .flatten()
            .find_map(|item| self.descriptions.get(item))
            .cloned()
            .unwrap_or_default()
            .chars()
            .take(120)
            .collect()
    }

    fn load_names(&mut self, value: &serde_json::Value) {
        for perk in iter(&value["effects"]).flat_map(|effect| iter(&effect["perks"])) {
            let Some(index) = perk["index"].as_u64() else {
                continue;
            };
            let names = self.names.entry(index).or_default();
            let items = self.items.entry(index).or_default();
            for item in iter(&perk["items"]) {
                if let Some(name) = item["name"].as_str() {
                    names.insert(name.to_owned());
                }
                if let Some(hash) = item["item_hash"].as_u64() {
                    items.insert(hash);
                }
            }
        }
    }

    /// A plug is an exotic intrinsic when it is an intrinsic or armor perk and every item
    /// whose socket carries it is exotic. A frame shared with legendaries is not one.
    fn load_exotics(&mut self, value: &serde_json::Value) {
        let Some(plugs) = value["plugs"].as_object() else {
            return;
        };
        let exotic_plugs: BTreeSet<u64> = plugs
            .iter()
            .filter(|(_, plug)| {
                matches!(plug["type"].as_str(), Some("Intrinsic" | "Armor Perk"))
                    && iter(&plug["owner_rarities"]).count() > 0
                    && iter(&plug["owner_rarities"]).all(|r| r.as_str() == Some("exotic"))
            })
            .filter_map(|(hash, _)| hash.parse().ok())
            .collect();
        for (perk, items) in &self.items {
            if items.iter().any(|item| exotic_plugs.contains(item)) {
                self.exotic.insert(*perk);
            }
        }
    }

    fn load_perks(&mut self, value: &serde_json::Value) {
        for family in ["conditions", "effects"] {
            for occurrence in
                iter(&value["groups"][family]).flat_map(|kind| iter(&kind["occurrences"]))
            {
                let Some(tag) = occurrence["action"]
                    .as_u64()
                    .and_then(|t| u32::try_from(t).ok())
                else {
                    continue;
                };
                let perks = self.perks.entry(tag).or_default();
                perks.extend(iter(&occurrence["perk_indices"]).filter_map(|p| p.as_u64()));
            }
        }
    }

    /// Names of every perk behind the given action tags, and how many distinct perks that is.
    pub(super) fn describe(&self, tags: &BTreeSet<u32>) -> String {
        if self.perks.is_empty() {
            let shown: Vec<_> = tags
                .iter()
                .take(3)
                .map(|tag| format!("0x{tag:08X}"))
                .collect();
            return format!("{} action(s): {}", tags.len(), shown.join(" "));
        }
        let perks: BTreeSet<u64> = tags
            .iter()
            .filter_map(|tag| self.perks.get(tag))
            .flatten()
            .copied()
            .collect();
        let names: BTreeSet<&str> = perks
            .iter()
            .filter_map(|perk| self.names.get(perk))
            .flatten()
            .map(String::as_str)
            .collect();
        let shown: Vec<_> = names.iter().take(4).copied().collect();
        let more = names.len().saturating_sub(shown.len());
        let suffix = if more > 0 {
            format!(" +{more} more")
        } else {
            String::new()
        };
        format!("{} perk(s): {}{suffix}", perks.len(), shown.join(", "))
    }
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn iter(value: &serde_json::Value) -> impl Iterator<Item = &serde_json::Value> {
    value.as_array().into_iter().flatten()
}

/// One stock predicate row: the unnamed key and float pair under study.
struct PredicateRow {
    class: u32,
    key: u32,
    low: [u8; 4],
    high: [u8; 4],
    flags: [u8; 4],
}

#[derive(Default)]
struct Survey {
    dir: Option<PathBuf>,
    actions: usize,
    decoded: usize,
    failed: usize,
    synthetic: usize,
    distributions: BTreeMap<SiteKey, Distribution>,
    /// Every stock value with the action tag that carried it, for the parameter study.
    observations: BTreeMap<SiteKey, Vec<(u32, Vec<u8>)>>,
    predicates: Vec<PredicateRow>,
    context: Context,
}

impl Survey {
    fn shape(&self, site: &Site) -> &'static str {
        self.distributions
            .get(&site.key())
            .map_or("unobserved", |distribution| distribution.shape(site.format))
    }
    fn distribution(&self, site: &Site) -> Option<&Distribution> {
        self.distributions.get(&site.key())
    }
    fn matches_mode(&self, site: &Site) -> bool {
        self.distribution(site)
            .and_then(Distribution::mode)
            .is_some_and(|mode| *mode == site.value)
    }
    /// Action tags that carry `value` at `key`.
    fn tags_with(&self, key: SiteKey, value: &[u8]) -> BTreeSet<u32> {
        self.observations
            .get(&key)
            .into_iter()
            .flatten()
            .filter(|(_, observed)| observed == value)
            .map(|(tag, _)| *tag)
            .collect()
    }
}

pub(super) fn survey_dir() -> Option<PathBuf> {
    let candidate = std::env::var_os("PARHELION_PERK_SURVEY").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join(SURVEY_FALLBACK),
        |root| PathBuf::from(root).join("actions"),
    );
    candidate.is_dir().then_some(candidate)
}

pub(super) fn is_synthetic(tag: u32) -> bool {
    (SYNTHETIC_FIRST..=SYNTHETIC_LAST).contains(&tag)
        && (tag - SYNTHETIC_FIRST) % SYNTHETIC_STEP == 0
}

pub(super) fn action_tag(path: &Path) -> Option<u32> {
    let stem = path.file_stem()?.to_str()?;
    u32::from_str_radix(stem, 16).ok()
}

/// Tabulates every wanted byte range across all decodable stock actions.
fn survey(wanted: &BTreeSet<SiteKey>) -> Survey {
    let mut survey = Survey::default();
    let Some(dir) = survey_dir() else {
        return survey;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return survey;
    };
    survey.context = Context::load(&dir);
    survey.dir = Some(dir);
    let mut by_class: BTreeMap<u32, Vec<(usize, usize)>> = BTreeMap::new();
    for &(class, offset, width) in wanted {
        by_class.entry(class).or_default().push((offset, width));
    }
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.extension().is_none_or(|extension| extension != "bin") {
            continue;
        }
        survey.actions += 1;
        let tag = action_tag(&path);
        if tag.is_some_and(is_synthetic) {
            survey.synthetic += 1;
            continue;
        }
        let graph = std::fs::read(&path)
            .ok()
            .and_then(|data| Graph::read(&data, 0, ACTION_ROOT_CLASS).ok());
        let Some(graph) = graph else {
            survey.failed += 1;
            continue;
        };
        survey.decoded += 1;
        sample(&graph, tag.unwrap_or_default(), &by_class, &mut survey);
    }
    survey
}

fn sample(
    graph: &Graph,
    tag: u32,
    by_class: &BTreeMap<u32, Vec<(usize, usize)>>,
    survey: &mut Survey,
) {
    for block in &graph.blocks {
        let Ok(stride) = record(block.class).map(|record| record.size) else {
            continue;
        };
        let rows = block.count.unwrap_or(1).max(1);
        if let Some(sites) = by_class.get(&block.class) {
            for row in 0..rows {
                sample_row(
                    block.class,
                    &block.bytes[row * stride..],
                    sites,
                    tag,
                    survey,
                );
            }
        }
        if PREDICATE_CLASSES.contains(&block.class) {
            for row in 0..rows {
                sample_predicate(block.class, &block.bytes[row * stride..], survey);
            }
        }
    }
}

fn sample_row(class: u32, row: &[u8], sites: &[(usize, usize)], tag: u32, survey: &mut Survey) {
    for &(offset, width) in sites {
        let Some(value) = row.get(offset..offset + width) else {
            continue;
        };
        let key = (class, offset, width);
        let entry = survey.distributions.entry(key).or_default();
        entry.occurrences += 1;
        *entry.values.entry(value.to_vec()).or_default() += 1;
        survey
            .observations
            .entry(key)
            .or_default()
            .push((tag, value.to_vec()));
    }
}

fn sample_predicate(class: u32, row: &[u8], survey: &mut Survey) {
    let field = |offset: usize| -> Option<[u8; 4]> { row.get(offset..offset + 4)?.try_into().ok() };
    let (Some(key), Some(low), Some(high), Some(flags)) = (
        field(PREDICATE_KEY),
        field(PREDICATE_LOW),
        field(PREDICATE_HIGH),
        field(PREDICATE_FLAGS),
    ) else {
        return;
    };
    survey.predicates.push(PredicateRow {
        class,
        key: u32::from_le_bytes(key),
        low,
        high,
        flags,
    });
}

fn little_endian(value: &[u8]) -> u64 {
    value
        .iter()
        .rev()
        .fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte))
}

fn word(value: &[u8]) -> Option<u32> {
    let bytes: [u8; 4] = value.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn f32_text(value: &[u8]) -> String {
    let Some(word) = word(value) else {
        return String::new();
    };
    let float = f32::from_bits(word);
    if float.is_finite() {
        format!("{float}")
    } else {
        "non-finite".into()
    }
}

/// Classifies a field and explains any classification that rests on outside knowledge.
fn classify(block_class: u32, field: &fields::Field, value: &[u8]) -> (Class, Option<String>) {
    if field.format == Format::Pointer || field.label == "Entry Count" {
        return (Class::Structural, None);
    }
    if block_class == INSTRUCTION_BYTES {
        return (
            Class::Decoded,
            Some("value program bytecode, modelled by program::decompile".into()),
        );
    }
    if LABEL_MASK_CLASSES.contains(&block_class) {
        return (
            Class::Decoded,
            Some("label mask bits, modelled by native::labels".into()),
        );
    }
    if let Some(decoded) = decoded_sentinel(field.format, value) {
        return (Class::Decoded, Some(decoded));
    }
    if field.label.starts_with("Fixed Native Value +0x") {
        return (
            Class::Fixed,
            Some("identical in every stock node of this class".into()),
        );
    }
    if field.label.starts_with("Native Value +0x") {
        return (Class::Unnamed, census(block_class, field, value));
    }
    if matches!(field.label.as_str(), "Key" | "Resource") {
        return (Class::TypedUnnamed, census(block_class, field, value));
    }
    (Class::Named, None)
}

/// Fixed values whose meaning the codebase already knows without a per-site name.
fn decoded_sentinel(format: Format, value: &[u8]) -> Option<String> {
    let word = word(value)?;
    match format {
        Format::Tag if word == labels::TAG => Some("label globals tag".into()),
        Format::Tag if word == NULL_TAG => Some("null resource".into()),
        Format::Key if word == fnv1_name_hash("") => Some("empty key".into()),
        Format::Key => labels::name(word).map(|name| format!("label \"{name}\"")),
        _ => None,
    }
}

fn census(block_class: u32, field: &fields::Field, value: &[u8]) -> Option<String> {
    let numeric = little_endian(value);
    fields::stock_values::OBSERVED
        .iter()
        .filter(|selector| selector.0 == block_class && selector.1 == field.offset)
        .flat_map(|selector| selector.2.iter())
        .find(|(observed, _, _)| u64::from(*observed) == numeric)
        .map(|(_, count, perk)| format!("census: {count} stock use(s), e.g. {perk}"))
}

/// Content a template carries from the stock perk it was lifted from. Mapped, but a
/// synthesised default should not inherit it.
fn incidental(site: &Site) -> bool {
    match site.format {
        Format::Tag => {
            site.nonzero() && word(&site.value).is_none_or(|t| t != labels::TAG && t != NULL_TAG)
        }
        Format::Key => site
            .evidence
            .as_deref()
            .is_some_and(|evidence| evidence.starts_with("label ")),
        _ => false,
    }
}

fn map_template(condition: bool, node: &nodes::NodeKind) -> Result<(Tally, Vec<Site>), String> {
    let bytes = template(condition, node.kind).ok_or("no template")?;
    let graph = Graph::read(&bytes, 0, node.class)?;
    let mut tally = Tally::default();
    let mut sites = Vec::new();
    for (index, block) in graph.blocks.iter().enumerate() {
        if block.class == 0 {
            tally.add(Class::Structural, &block.bytes);
            continue;
        }
        let described = fields::describe(block.class)?;
        let rows = block.count.unwrap_or(1).max(1);
        let stride = record(block.class)?.size;
        for row in 0..rows {
            for field in &described {
                let start = row * stride + field.offset;
                let Some(value) = block.bytes.get(start..start + field.width) else {
                    continue;
                };
                let (class, evidence) = classify(block.class, field, value);
                tally.add(class, value);
                if class == Class::Structural || class == Class::Named {
                    continue;
                }
                sites.push(Site {
                    block: index,
                    block_class: block.class,
                    row,
                    offset: field.offset,
                    width: field.width,
                    format: field.format,
                    label: field.label.clone(),
                    value: value.to_vec(),
                    class,
                    evidence,
                });
            }
        }
    }
    Ok((tally, sites))
}

fn collect() -> Collected {
    let mut collected = Collected::default();
    for (condition, family, table) in [
        (true, "condition", &nodes::CONDITIONS[..]),
        (false, "effect", &nodes::EFFECTS[..]),
    ] {
        for node in table {
            let key = format!("{family} {:>3} {}", node.kind, node.name);
            match map_template(condition, node) {
                Ok((tally, sites)) => collected.absorb(key, node.class, tally, sites),
                Err(error) if error == "no template" => collected
                    .unobserved
                    .push(format!("{key} (0x{:08X})", node.class)),
                Err(error) => panic!("{key}: {error}"),
            }
        }
    }
    collected
}

fn percent(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "n/a".into();
    }
    format!("{:.1}%", 100.0 * part as f64 / whole as f64)
}

fn hex(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02X}")).collect()
}

/// Kinds whose every unmapped site carries the stock modal value, so a synthesiser that
/// emits the mode for each byte regenerates the template exactly.
fn mode_reproducible(c: &Collected, survey: &Survey) -> usize {
    c.per_kind
        .iter()
        .filter(|(key, _, _)| c.sites_of(key).iter().all(|site| survey.matches_mode(site)))
        .count()
}

fn write_summary(out: &mut String, c: &Collected, survey: &Survey) {
    let total = c.overall.total();
    let _ = writeln!(out, "# Template Field Map, September 16, 2026\n");
    let _ = writeln!(
        out,
        "Every byte of every native node template attributed to what the codebase knows about it. \
         Generated by `schema::field_map::report_template_field_map`.\n"
    );
    let _ = writeln!(out, "## Summary\n\n| Measure | Value |\n|---|---|");
    let _ = writeln!(out, "| Templates mapped | {} |", c.per_kind.len());
    let _ = writeln!(
        out,
        "| Declared kinds with no template | {} |",
        c.unobserved.len()
    );
    let _ = writeln!(out, "| Template bytes | {total} |");
    for (class, label) in CLASSES {
        let i = class as usize;
        let _ = writeln!(
            out,
            "| {label} | {} bytes ({}), {} non-zero |",
            c.overall.bytes[i],
            percent(c.overall.bytes[i], total),
            c.overall.nonzero[i]
        );
    }
    let kinds = c.per_kind.len();
    let _ = writeln!(
        out,
        "| Kinds fully mapped, strict (every non-structural byte named or decoded) | {} of {kinds} |",
        c.strict_complete()
    );
    let _ = writeln!(
        out,
        "| Kinds fully mapped, lenient (no non-zero unmapped byte) | {} of {kinds} |",
        c.lenient_complete()
    );
    if survey.dir.is_some() {
        let _ = writeln!(
            out,
            "| Kinds reproducible from the stock mode (every unmapped byte carries its modal stock value) | {} of {kinds} |",
            mode_reproducible(c, survey)
        );
    }
    let _ = writeln!(
        out,
        "| Incidental stock content sites | {} |",
        c.content.len()
    );
    let _ = writeln!(
        out,
        "\nStrict is the bar for synthesising a template from knowledge alone. Lenient counts a \
         zero default as reproducible without understanding it. Mode-reproducible is the bar for \
         synthesising from stock evidence without understanding: emit each byte's modal stock value.\n"
    );
}

/// Sites and bytes per stock shape, for one group of sites.
fn shape_counts<'a>(
    survey: &Survey,
    sites: impl Iterator<Item = &'a Site>,
) -> BTreeMap<&'static str, (usize, usize)> {
    let mut counts = BTreeMap::new();
    for site in sites {
        let entry = counts.entry(survey.shape(site)).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += site.width;
    }
    counts
}

fn write_survey(out: &mut String, c: &Collected, survey: &Survey) {
    let _ = writeln!(out, "## Stock Survey Evidence\n");
    let Some(dir) = &survey.dir else {
        let _ = writeln!(
            out,
            "No captured stock survey was found. Set `PARHELION_PERK_SURVEY` or place the capture \
             at `{SURVEY_FALLBACK}` to tabulate unmapped bytes across stock data.\n"
        );
        return;
    };
    let _ = writeln!(
        out,
        "Every unmapped byte range tabulated across all stock occurrences of its block class. \
         A constant needs no understanding to reproduce. An enumeration is recoverable from which \
         perks carry which value. A reference is a tag or key that names a resource and varies \
         because it is the parameter. A varying byte is a numeric parameter that needs a contract.\n"
    );
    let _ = writeln!(out, "| Measure | Value |\n|---|---|");
    let _ = writeln!(out, "| Survey directory | `{}` |", dir.display());
    let _ = writeln!(
        out,
        "| Action files | {} ({} decoded, {} failed to decode, {} synthetic skipped) |",
        survey.actions, survey.decoded, survey.failed, survey.synthetic
    );
    let _ = writeln!(
        out,
        "| Perk name context | {} perks named, {} actions attributed |",
        survey.context.names.len(),
        survey.context.perks.len()
    );
    for (label, filter) in [
        ("Non-zero unmapped sites", true),
        ("Zero-valued unmapped sites", false),
    ] {
        let counts = shape_counts(survey, c.unmapped_sites().filter(|s| s.nonzero() == filter));
        for shape in SHAPES {
            let (sites, bytes) = counts.get(shape).copied().unwrap_or((0, 0));
            let _ = writeln!(out, "| {label}, {shape} | {sites} sites, {bytes} bytes |");
        }
    }
    let _ = writeln!(out);
}

fn write_block_classes(out: &mut String, c: &Collected, survey: &Survey) {
    let _ = writeln!(out, "## Unmapped Non-Zero Bytes by Block Class\n");
    let _ = writeln!(
        out,
        "Sub-object classes are shared across kinds, so mapping one class can clear many templates.\n"
    );
    let _ = writeln!(
        out,
        "| Block class | Name | Sites | Bytes | Distinct offsets | Constant | Enum | Reference | Varying |\n|---|---|---|---|---|---|---|---|---|"
    );
    let mut ranked: Vec<_> = c.by_block.iter().collect();
    ranked.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes));
    for (class, gap) in ranked {
        let shapes = shape_counts(
            survey,
            c.unmapped_sites()
                .filter(|site| site.nonzero() && site.block_class == *class),
        );
        let count = |shape| shapes.get(shape).map_or(0, |entry| entry.0);
        let _ = writeln!(
            out,
            "| 0x{class:08X} | {} | {} | {} | {} | {} | {} | {} | {} |",
            fields::name(*class),
            gap.sites,
            gap.bytes,
            gap.offsets.len(),
            count("constant"),
            count("enum"),
            count("reference"),
            count("varying")
        );
    }
}

fn write_per_kind(out: &mut String, c: &Collected, survey: &Survey) {
    let _ = writeln!(out, "\n## Per Kind\n");
    let _ = writeln!(
        out,
        "| Kind | Class | Bytes | Named | Decoded | Structural | Unmapped | Unmapped non-zero | Strict | Deviations from mode | Mode-reproducible |\n|---|---|---|---|---|---|---|---|---|---|---|"
    );
    for (key, class, t) in &c.per_kind {
        let deviations = c
            .sites_of(key)
            .iter()
            .filter(|site| !survey.matches_mode(site))
            .count();
        let _ = writeln!(
            out,
            "| {key} | 0x{class:08X} | {} | {} | {} | {} | {} | {} | {} | {deviations} | {} |",
            t.total(),
            percent(t.bytes[Class::Named as usize], t.total()),
            percent(t.bytes[Class::Decoded as usize], t.total()),
            percent(t.bytes[Class::Structural as usize], t.total()),
            t.unmapped(),
            t.unmapped_nonzero(),
            if t.unmapped() == 0 { "yes" } else { "no" },
            if deviations == 0 { "yes" } else { "no" }
        );
    }
}

fn write_unobserved(out: &mut String, c: &Collected) {
    let _ = writeln!(out, "\n## Declared Kinds Without a Template\n");
    let _ = writeln!(
        out,
        "These kinds exist in `nodes` but were never observed in the stock survey, so they cannot \
         be authored today by any path.\n"
    );
    for line in &c.unobserved {
        let _ = writeln!(out, "- {line}");
    }
}

fn write_content(out: &mut String, c: &Collected) {
    let _ = writeln!(out, "\n## Incidental Stock Content\n");
    let _ = writeln!(
        out,
        "Values a template carries from the stock perk it was lifted from. Their meaning is known, \
         but a synthesised default should start neutral rather than inherit them.\n"
    );
    let _ = writeln!(
        out,
        "| Kind | Block class | Block name | Offset | Format | Value | Evidence |\n|---|---|---|---|---|---|---|"
    );
    for (kind, site) in &c.content {
        let _ = writeln!(
            out,
            "| {kind} | 0x{:08X} | {} | 0x{:03X} | {:?} | {} | {} |",
            site.block_class,
            fields::name(site.block_class),
            site.offset,
            site.format,
            hex(&site.value),
            site.evidence.as_deref().unwrap_or("")
        );
    }
}

/// One varying numeric parameter location and every kind whose template carries it.
struct StudySite {
    name: String,
    offset: usize,
    width: usize,
    kinds: BTreeSet<String>,
}

fn study_sites(c: &Collected, survey: &Survey) -> BTreeMap<SiteKey, StudySite> {
    let mut sites: BTreeMap<SiteKey, StudySite> = BTreeMap::new();
    for (kind, kind_sites) in &c.work {
        for site in kind_sites
            .iter()
            .filter(|site| survey.shape(site) == "varying")
        {
            sites
                .entry(site.key())
                .or_insert_with(|| StudySite {
                    name: fields::name(site.block_class),
                    offset: site.offset,
                    width: site.width,
                    kinds: BTreeSet::new(),
                })
                .kinds
                .insert(kind.clone());
        }
    }
    sites
}

fn write_study(out: &mut String, c: &Collected, survey: &Survey) {
    let sites = study_sites(c, survey);
    let _ = writeln!(out, "\n## Parameter Study\n");
    let _ = writeln!(
        out,
        "Every varying numeric parameter, with every stock value attributed to the perks that carry \
         it. A count is evidence of use and the perk names are evidence of meaning, exactly as in \
         `fields::stock_values`. {} location(s) across all templates.\n",
        sites.len()
    );
    for (key, site) in &sites {
        let Some(distribution) = survey.distributions.get(key) else {
            continue;
        };
        let _ = writeln!(
            out,
            "### {} +0x{:03X} ({} bytes)\n\nCarried by: {}\n\n{} stock occurrences, {} distinct values.\n",
            site.name,
            site.offset,
            site.width,
            site.kinds.iter().cloned().collect::<Vec<_>>().join("; "),
            distribution.occurrences,
            distribution.values.len()
        );
        let _ = writeln!(
            out,
            "| Value | As u32 | As f32 | Occurrences | Share | Perks |\n|---|---|---|---|---|---|"
        );
        for (value, count) in distribution.ranked() {
            let tags = survey.tags_with(*key, value);
            let _ = writeln!(
                out,
                "| {} | {} | {} | {count} | {} | {} |",
                hex(value),
                word(value).map_or_else(String::new, |w| w.to_string()),
                f32_text(value),
                percent(count, distribution.occurrences),
                survey.context.describe(&tags)
            );
        }
        let _ = writeln!(out);
    }
    write_predicate_study(out, survey);
}

/// Distribution of a four-byte pair rendered as floats, top entries only.
fn pair_text(pairs: &BTreeMap<([u8; 4], [u8; 4]), usize>, limit: usize) -> String {
    let mut ranked: Vec<_> = pairs.iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    ranked
        .into_iter()
        .take(limit)
        .map(|((low, high), count)| format!("({}, {})×{count}", f32_text(low), f32_text(high)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn write_predicate_study(out: &mut String, survey: &Survey) {
    let _ = writeln!(out, "### Predicate key and float pair\n");
    let _ = writeln!(
        out,
        "Hypothesis: in `General Predicate` and `Predicate with Nested Condition`, +0x{PREDICATE_KEY:03X} \
         is a named key and +0x{PREDICATE_LOW:03X}/+0x{PREDICATE_HIGH:03X} are a minimum and maximum, \
         the same contract `fields::known` records for other classes as Named Key, Minimum Value and \
         Maximum Value. If so, rows with the empty key should sit at their defaults, and keyed rows \
         should keep minimum at or below maximum.\n"
    );
    let empty_key = fnv1_name_hash("");
    for class in PREDICATE_CLASSES {
        let rows: Vec<_> = survey
            .predicates
            .iter()
            .filter(|row| row.class == class)
            .collect();
        let (empty, keyed): (Vec<&PredicateRow>, Vec<&PredicateRow>) =
            rows.iter().copied().partition(|row| row.key == empty_key);
        let pairs = |set: &[&PredicateRow]| {
            let mut pairs: BTreeMap<([u8; 4], [u8; 4]), usize> = BTreeMap::new();
            for row in set {
                *pairs.entry((row.low, row.high)).or_default() += 1;
            }
            pairs
        };
        let ordered = keyed
            .iter()
            .filter(|row| f32::from_le_bytes(row.low) <= f32::from_le_bytes(row.high))
            .count();
        let _ = writeln!(out, "#### {} (0x{class:08X})\n", fields::name(class));
        let _ = writeln!(out, "| Measure | Value |\n|---|---|");
        let _ = writeln!(out, "| Stock rows | {} |", rows.len());
        let _ = writeln!(
            out,
            "| Rows with the empty key | {} with pairs {} |",
            empty.len(),
            pair_text(&pairs(&empty), 4)
        );
        let _ = writeln!(
            out,
            "| Rows with a named key | {} with pairs {} |",
            keyed.len(),
            pair_text(&pairs(&keyed), 6)
        );
        let _ = writeln!(
            out,
            "| Keyed rows with minimum at or below maximum | {ordered} of {} |",
            keyed.len()
        );
        write_flag_bytes(out, &rows);
    }
}

/// The four bytes at +0x80 tabulated independently, to separate packed fields.
fn write_flag_bytes(out: &mut String, rows: &[&PredicateRow]) {
    let _ = writeln!(
        out,
        "| +0x{PREDICATE_FLAGS:03X} bytes, individually | {} |\n",
        (0..4)
            .map(|index| {
                let mut counts: BTreeMap<u8, usize> = BTreeMap::new();
                for row in rows {
                    *counts.entry(row.flags[index]).or_default() += 1;
                }
                let mut ranked: Vec<_> = counts.into_iter().collect();
                ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                format!(
                    "byte {index}: {}",
                    ranked
                        .iter()
                        .take(5)
                        .map(|(value, count)| format!("{value:02X}×{count}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    );
}

fn write_zero_but_varying(out: &mut String, c: &Collected, survey: &Survey) {
    let _ = writeln!(out, "\n## Zero in the Template but Varying in Stock\n");
    let _ = writeln!(
        out,
        "The template's zero is a choice, not a constant. These are real parameters whose \
         neutral value happens to be zero in the smallest stock instance.\n"
    );
    let _ = writeln!(
        out,
        "| Kind | Block class | Block name | Offset | Width | Shape | Occurrences | Distinct | Zero share | Top values |\n|---|---|---|---|---|---|---|---|---|---|"
    );
    for (kind, sites) in &c.work {
        for site in sites.iter().filter(|site| !site.nonzero()) {
            let Some(distribution) = survey.distribution(site) else {
                continue;
            };
            if distribution.values.len() < 2 {
                continue;
            }
            let _ = writeln!(
                out,
                "| {kind} | 0x{:08X} | {} | 0x{:03X} | {} | {} | {} | {} | {} | {} |",
                site.block_class,
                fields::name(site.block_class),
                site.offset,
                site.width,
                distribution.shape(site.format),
                distribution.occurrences,
                distribution.values.len(),
                distribution.share(&site.value),
                distribution.top(4)
            );
        }
    }
}

fn write_site(out: &mut String, site: &Site, survey: &Survey) {
    let (occurrences, distinct, share, top) = match survey.distribution(site) {
        Some(d) => (
            d.occurrences.to_string(),
            d.values.len().to_string(),
            d.share(&site.value),
            d.top(4),
        ),
        None => ("0".into(), "0".into(), "n/a".into(), String::new()),
    };
    let _ = writeln!(
        out,
        "| {} | 0x{:08X} | {} | {} | 0x{:03X} | {} | {:?} | {} | {} | {:?} | {} | {} | {occurrences} | {distinct} | {share} | {} | {top} |",
        site.block,
        site.block_class,
        fields::name(site.block_class),
        site.row,
        site.offset,
        site.width,
        site.format,
        site.label,
        hex(&site.value),
        site.class,
        site.evidence.as_deref().unwrap_or(""),
        survey.shape(site),
        if survey.matches_mode(site) {
            "yes"
        } else {
            "no"
        }
    );
}

fn write_work_list(out: &mut String, c: &Collected, survey: &Survey) {
    let _ = writeln!(out, "\n## Work List\n");
    let _ = writeln!(
        out,
        "Every unmapped byte range, per kind. Non-zero values are listed before zero values. \
         Shape, occurrences and top values come from the stock survey. Template share is how \
         often stock data carries the template's own value. Mode says whether the template \
         value is the most common stock value.\n"
    );
    for (key, sites) in &c.work {
        let (nonzero, zero): (Vec<_>, Vec<_>) = sites.iter().partition(|site| site.nonzero());
        let _ = writeln!(
            out,
            "### {key}\n\n{} non-zero, {} zero\n",
            nonzero.len(),
            zero.len()
        );
        let _ = writeln!(
            out,
            "| Block | Block class | Block name | Row | Offset | Width | Format | Label | Value | Class | Evidence | Shape | Occurrences | Distinct | Template share | Mode | Top values |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"
        );
        for site in nonzero.into_iter().chain(zero) {
            write_site(out, site, survey);
        }
        let _ = writeln!(out);
    }
}

#[test]
#[ignore = "diagnostic report writer, run explicitly with --ignored --nocapture"]
fn report_template_field_map() {
    let collected = collect();
    let wanted: BTreeSet<SiteKey> = collected.unmapped_sites().map(Site::key).collect();
    let survey = survey(&wanted);
    let mut out = String::new();
    write_summary(&mut out, &collected, &survey);
    write_survey(&mut out, &collected, &survey);
    write_block_classes(&mut out, &collected, &survey);
    write_per_kind(&mut out, &collected, &survey);
    write_unobserved(&mut out, &collected);
    write_content(&mut out, &collected);
    write_study(&mut out, &collected, &survey);
    write_zero_but_varying(&mut out, &collected, &survey);
    write_work_list(&mut out, &collected, &survey);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(REPORT);
    std::fs::write(&path, &out).expect("write report");
    let nonzero = shape_counts(&survey, collected.unmapped_sites().filter(|s| s.nonzero()));
    let count = |shape| nonzero.get(shape).map_or(0, |entry| entry.0);
    println!(
        "template field map: {} templates, {} unobserved kinds, {} bytes, strict {}/{}, lenient {}/{}, mode-reproducible {}/{}, survey {} actions ({} decoded, {} failed), non-zero unmapped sites: {} constant, {} enum, {} reference, {} varying, {} unobserved, study locations {}, report {}",
        collected.per_kind.len(),
        collected.unobserved.len(),
        collected.overall.total(),
        collected.strict_complete(),
        collected.per_kind.len(),
        collected.lenient_complete(),
        collected.per_kind.len(),
        mode_reproducible(&collected, &survey),
        collected.per_kind.len(),
        survey.actions,
        survey.decoded,
        survey.failed,
        count("constant"),
        count("enum"),
        count("reference"),
        count("varying"),
        count("unobserved"),
        study_sites(&collected, &survey).len(),
        path.display()
    );
}

/// Stock rows a site needs before one value counts as fixed. Below this, a single value is
/// weak evidence of a constant.
const FIXED_FLOOR: usize = 10;
const FIXED_VALUES: &str = "src/sandbox_perk/action/native/fields/fixed_values.rs";

/// Writes `fields::fixed_values` from the survey: every unnamed site whose value is identical
/// across all stock nodes of its class. Run after the survey or the field contracts change.
#[test]
#[ignore = "generator, run explicitly with --ignored"]
fn write_fixed_values() {
    let collected = collect();
    let wanted: BTreeSet<SiteKey> = collected
        .unmapped_sites()
        .filter(|site| matches!(site.class, Class::Unnamed | Class::Fixed))
        .map(Site::key)
        .collect();
    let survey = survey(&wanted);
    assert!(survey.dir.is_some(), "captured stock survey available");
    let mut rows: BTreeSet<(u32, usize, Vec<u8>)> = BTreeSet::new();
    for key in &wanted {
        let Some(distribution) = survey.distributions.get(key) else {
            continue;
        };
        if distribution.values.len() != 1 {
            continue;
        }
        let value = distribution.values.keys().next().expect("one value");
        // A non-zero constant needs enough rows before one value is evidence of a constant.
        // A site that is zero in every row it was seen in needs no such bar: the template
        // writes zero there anyway, so emitting zero reproduces it whatever the sample size.
        if distribution.occurrences < FIXED_FLOOR && value.iter().any(|byte| *byte != 0) {
            continue;
        }
        rows.insert((key.0, key.1, value.clone()));
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "//! Native bytes identical in every stock node of their class.\n//!\n\
         //! A fixed value is evidence that the byte is not a per-perk setting, not a claim about\n\
         //! its role. The workbench locks these fields at their stock value and says so, which\n\
         //! leaves the fields that vary as the ones a reader has to think about.\n//!\n\
         //! Generated by `schema::field_map::write_fixed_values` over the captured stock survey\n\
         //! ({} stock actions). A site needs at least {FIXED_FLOOR} stock rows. Regenerate it when the\n\
         //! survey or the field contracts change.\n",
        survey.decoded
    );
    let _ = writeln!(
        out,
        "/// Native class, byte offset and the bytes every stock node stores there.\n\
         pub type Fixed = (u32, usize, &'static [u8]);\n\n\
         pub const FIXED: &[Fixed] = &["
    );
    for (class, offset, value) in &rows {
        let bytes = value
            .iter()
            .map(|byte| format!("0x{byte:02X}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "    (0x{class:08X}, 0x{offset:02X}, &[{bytes}]),");
    }
    let _ = writeln!(
        out,
        "];\n\n/// The fixed bytes at one site, when every stock node of the class agrees.\n\
         #[must_use]\n\
         pub fn fixed(class: u32, offset: usize) -> Option<&'static [u8]> {{\n\
         \x20   FIXED\n\
         \x20       .iter()\n\
         \x20       .find(|(site_class, site_offset, _)| *site_class == class && *site_offset == offset)\n\
         \x20       .map(|(_, _, value)| *value)\n\
         }}"
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXED_VALUES);
    std::fs::write(&path, out).expect("write fixed values");
    println!("fixed values: {} sites, {}", rows.len(), path.display());
}

/// Floors for the template map, raised as contracts are recovered. Lowering one means a
/// template lost named coverage, which needs a deliberate decision rather than silent drift.
const STRICT_FLOOR: usize = 17;
/// Tracks the measured 77 of 82 in `template-field-map-2026-09-16`. It sat at 43 long after the
/// real figure passed it, which would have let a thirty-template regression through in silence.
const LENIENT_FLOOR: usize = 77;
/// Checked only when the captured stock survey is available locally. Tracks the measured 80 of
/// 82 in the same report.
const MODE_REPRODUCIBLE_FLOOR: usize = 80;

#[test]
fn template_field_map_does_not_regress() {
    let collected = collect();
    assert!(
        collected.strict_complete() >= STRICT_FLOOR,
        "strictly mapped templates fell to {} (floor {STRICT_FLOOR})",
        collected.strict_complete()
    );
    assert!(
        collected.lenient_complete() >= LENIENT_FLOOR,
        "leniently mapped templates fell to {} (floor {LENIENT_FLOOR})",
        collected.lenient_complete()
    );
    if survey_dir().is_some() {
        let wanted: BTreeSet<SiteKey> = collected.unmapped_sites().map(Site::key).collect();
        let survey = survey(&wanted);
        let reproducible = mode_reproducible(&collected, &survey);
        assert!(
            reproducible >= MODE_REPRODUCIBLE_FLOOR,
            "mode reproducible templates fell to {reproducible} (floor {MODE_REPRODUCIBLE_FLOOR})"
        );
    }
}

/// Unmapped non-zero template bytes per node kind: the opaque `Native Value` fields the
/// workbench would show when a stock perk carries a native node of that kind.
pub(super) fn opaque_bytes_by_kind() -> BTreeMap<(bool, u8), usize> {
    let collected = collect();
    let mut result = BTreeMap::new();
    for (condition, family, table) in [
        (true, "condition", &nodes::CONDITIONS[..]),
        (false, "effect", &nodes::EFFECTS[..]),
    ] {
        for node in table {
            let key = format!("{family} {:>3} {}", node.kind, node.name);
            if let Some((_, _, tally)) = collected.per_kind.iter().find(|(k, _, _)| *k == key) {
                result.insert((condition, node.kind), tally.unmapped_nonzero());
            }
        }
    }
    result
}
