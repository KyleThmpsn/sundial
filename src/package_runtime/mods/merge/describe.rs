//! Merge data for the packages a build writes (Sunrise Studio 007dd7a6
//! `ProjectBuilder.MergeSidecars.cs`). Each changed installed entry is described as a table the
//! loader can merge, a loading manifest, or opaque bytes, and every field that names a row the
//! build adds becomes a site, so the loader can renumber it.
//!
//! Where Studio stops the build, an entry that can't be described exactly is described as opaque
//! with the reason, so merge data never makes a build fail.
use super::layout;
use super::manifest::adds_only;
use super::recipes::{self, Field, Recipe, Step};
use super::sidecar::{
    Diff, Entry, EntryKind, FieldKind, METADATA_DIGEST_SIZE, NO_ELEMENT_SHAPE, OP_LOCAL_TARGET,
    ORDERED, Op, OpKind, SIGNED_SITE, ShapeField, Sidecar, Site, TOP_LEVEL_OP, Target, UNIQUE_KEY,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

const ARRAY_MARKER: u32 = 0x8080_9FBD;
/// A self-relative pointer lands on a typed object, whose class word sits just before it.
const CLASS_WORD_MASK: u32 = 0xFFFF_0000;
const CLASS_WORD_PREFIX: u32 = 0x8080_0000;
const TAG_BASE: u32 = 0x8080_0000;
const ENTRY_BITS: u32 = 13;

/// One entry a build changes in an installed package.
pub struct Edited {
    /// The entry's class, its reference word.
    pub class: u32,
    pub stock: Vec<u8>,
    pub built: Vec<u8>,
}

/// One entry of a package the build adds, of a class whose fields may name rows it adds.
pub struct Added {
    pub class: u32,
    pub built: Vec<u8>,
}

/// What a build changes, by tag.
#[derive(Default)]
pub struct Build {
    pub edited: BTreeMap<u32, Edited>,
    pub added: BTreeMap<u32, Added>,
}

/// One package file the build writes.
#[derive(Clone, Copy, Debug)]
pub struct Package {
    pub id: u16,
    pub patch: u16,
    /// Copy of the header's metadata digest, which the sidecar binds to.
    pub digest: [u8; METADATA_DIGEST_SIZE],
    /// Whether the installation has the package, so the file patches it.
    pub installed: bool,
}

/// The installation's own entries, for the row counts of tables a site names.
pub trait Stock {
    /// Whether the installation holds the entry.
    fn has(&mut self, tag: u32) -> bool;
    /// The entry's installed bytes, or `None` when they can't be read.
    fn read(&mut self, tag: u32) -> Option<Vec<u8>>;
}

/// How one entry was described, for the build's report.
#[derive(Clone, Debug, Serialize)]
pub struct Description {
    pub tag: String,
    pub class: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub stock_size: usize,
    pub built_size: usize,
    pub diffs: usize,
    pub ops: Vec<String>,
    pub sites: BTreeMap<String, usize>,
}

/// One sidecar per package, in the packages' order, and how each entry was described.
pub struct Described {
    pub sidecars: Vec<Sidecar>,
    pub entries: Vec<Description>,
    /// Pair groups whose member tables gained different row counts. The loader merges no rows
    /// while one does.
    pub pairs: Vec<String>,
}

fn package_of(tag: u32) -> u16 {
    (tag.wrapping_sub(TAG_BASE) >> ENTRY_BITS) as u16
}

fn entry_of(tag: u32) -> u32 {
    tag.wrapping_sub(TAG_BASE) & ((1 << ENTRY_BITS) - 1)
}

/// The i64 at `at`, or 0 when it leaves the bytes.
fn i64_at(bytes: &[u8], at: i64) -> i64 {
    usize::try_from(at)
        .ok()
        .and_then(|at| layout::i64_at(bytes, at))
        .unwrap_or(0)
}

/// The u32 at `at`, or 0 when it leaves the bytes.
fn u32_at(bytes: &[u8], at: i64) -> u32 {
    usize::try_from(at)
        .ok()
        .and_then(|at| layout::u32_at(bytes, at))
        .unwrap_or(0)
}

fn len(bytes: &[u8]) -> i64 {
    bytes.len() as i64
}

/// The self-relative target of the i64 at `at`, or -1 for 0.
fn target(bytes: &[u8], at: i64) -> i64 {
    match i64_at(bytes, at) {
        0 => -1,
        relative => at + relative,
    }
}

/// Why an entry can't be described exactly. It is described as opaque with this reason.
struct Unmergeable(String);

type Checked<T> = Result<T, Unmergeable>;

fn unmergeable<T>(reason: impl Into<String>) -> Checked<T> {
    Err(Unmergeable(reason.into()))
}

/// One array as one copy holds it.
#[derive(Clone, Copy, PartialEq, Eq)]
struct View {
    count: i64,
    header: i64,
    stride: i64,
}

impl View {
    fn rows(self) -> i64 {
        self.header + 16
    }
}

/// Whether a typed array header repeats `count` behind its marker and, given a stride, its rows
/// fit.
fn header_valid(bytes: &[u8], header: i64, count: i64, stride: i64) -> bool {
    (0..=i64::from(recipes::EDITOR_ROW_LIMIT)).contains(&count)
        && header >= 4
        && header + 16 <= len(bytes)
        && u32_at(bytes, header - 4) == ARRAY_MARKER
        && i64_at(bytes, header) == count
        && (stride <= 0 || header + 16 + count * stride <= len(bytes))
}

/// `None` for an empty array without a header.
fn read_view(bytes: &[u8], descriptor: i64, stride: i64) -> Checked<Option<View>> {
    if descriptor < 0 || descriptor + 16 > len(bytes) {
        return unmergeable(format!("it has no array descriptor at +{descriptor}"));
    }
    let count = i64_at(bytes, descriptor);
    let relative = i64_at(bytes, descriptor + 8);
    if count == 0 && relative == 0 {
        return Ok(None);
    }
    let header = descriptor + 8 + relative;
    if !header_valid(bytes, header, count, stride) {
        return unmergeable(format!("its array at +{descriptor} is malformed"));
    }
    Ok(Some(View {
        count,
        header,
        stride,
    }))
}

/// The element class word of the typed array whose header is at `header`.
fn row_class(bytes: &[u8], header: i64) -> Option<&[u8]> {
    let at = usize::try_from(header + 8).ok()?;
    bytes.get(at..at.checked_add(8)?)
}

/// A row layout an op rebuilds: a recipe row, or the elements of a nested array.
#[derive(Clone, Copy)]
struct RowShape {
    stride: usize,
    fields: &'static [Field],
}

struct OpPlan {
    name: String,
    kind: OpKind,
    descriptor: i64,
    parent_op: u32,
    parent_row: u32,
    row: RowShape,
    stock_count: u32,
    mod_count: u32,
    limit: u32,
    pair_group: Option<&'static str>,
    flags: u8,
    key_offset: Option<usize>,
}

/// One field that names a row the build adds.
#[derive(Clone, Copy)]
struct SiteHit {
    offset: u32,
    width: u8,
    signed: bool,
    table: u32,
    array: i64,
    op_local: bool,
    rule: &'static str,
}

impl SiteHit {
    /// Whether two hits give the field the same meaning, whichever rule found them.
    fn same(&self, other: &Self) -> bool {
        self.offset == other.offset
            && self.width == other.width
            && self.signed == other.signed
            && self.table == other.table
            && self.array == other.array
            && self.op_local == other.op_local
    }
}

struct EntryPlan<'a> {
    tag: u32,
    class: u32,
    built: &'a [u8],
    /// Empty for an entry of a new package.
    stock: &'a [u8],
    kind: EntryKind,
    recipe: Option<&'static str>,
    reason: Option<String>,
    diffs: Vec<Diff>,
    ops: Vec<OpPlan>,
    sites: BTreeMap<u32, SiteHit>,
    growth: Vec<Target>,
}

impl<'a> EntryPlan<'a> {
    fn new(tag: u32, class: u32, built: &'a [u8], stock: &'a [u8], kind: EntryKind) -> Self {
        Self {
            tag,
            class,
            built,
            stock,
            kind,
            recipe: None,
            reason: None,
            diffs: Vec::new(),
            ops: Vec::new(),
            sites: BTreeMap::new(),
            growth: Vec::new(),
        }
    }

    fn top_op(&self, descriptor: i64) -> Option<(usize, &OpPlan)> {
        self.ops
            .iter()
            .enumerate()
            .find(|(_, op)| op.parent_op == TOP_LEVEL_OP && op.descriptor == descriptor)
    }
}

/// Row counts of target arrays before and after the build, read once each.
struct Context<'a> {
    build: &'a Build,
    stock: &'a mut dyn Stock,
    counts: HashMap<(u32, i64), (u32, u32)>,
}

/// The row count a descriptor holds.
fn count_at(bytes: &[u8], descriptor: i64) -> Option<u32> {
    if descriptor < 0 || descriptor + 16 > len(bytes) {
        return None;
    }
    u32::try_from(i64_at(bytes, descriptor)).ok()
}

impl Context<'_> {
    /// Row counts of a target array before and after this build. Both are 0 when the
    /// installation lacks the table or its array can't be counted.
    fn counts(&mut self, table: u32, descriptor: i64) -> (u32, u32) {
        if let Some(&known) = self.counts.get(&(table, descriptor)) {
            return known;
        }
        let found = match self.build.edited.get(&table) {
            Some(edited) => {
                count_at(&edited.stock, descriptor).zip(count_at(&edited.built, descriptor))
            }
            None => self
                .stock
                .read(table)
                .and_then(|stock| count_at(&stock, descriptor))
                .map(|count| (count, count)),
        };
        let counts = found.unwrap_or((0, 0));
        self.counts.insert((table, descriptor), counts);
        counts
    }
}

/// Describes one changed installed entry, then finds its sites.
fn plan_installed<'a>(context: &mut Context<'_>, tag: u32, edited: &'a Edited) -> EntryPlan<'a> {
    let (stock, built) = (&edited.stock[..], &edited.built[..]);
    let mut plan = EntryPlan::new(tag, edited.class, built, stock, EntryKind::Opaque);
    if edited.class == recipes::MANIFEST_CLASS {
        if adds_only(stock, built) {
            plan.kind = EntryKind::Manifest;
        } else {
            plan.reason = Some("it changes more of its loading manifest than the groups".into());
        }
    } else if let Some(recipe) = recipes::table(tag) {
        plan.recipe = Some(recipe.name);
        match classify_table(&mut plan, recipe) {
            Ok(()) => plan.kind = EntryKind::Table,
            Err(Unmergeable(reason)) => {
                plan.reason = Some(reason);
                plan.diffs.clear();
                plan.ops.clear();
                record_growth(&mut plan, recipe);
            }
        }
    } else if built.len() == stock.len() {
        // A same-size edit ships its old bytes, so two mods' edits to other words both survive.
        plan.kind = EntryKind::Table;
        let structural = if i64_at(stock, 0) == len(stock) {
            vec![(0, 8)]
        } else {
            Vec::new()
        };
        // Only an edit outside every span is refused, and this pass refuses none.
        plan.diffs = diffs(stock, built, &structural, &[], false).unwrap_or_default();
    } else {
        plan.reason = Some("its size changed".into());
    }
    find_sites(context, &mut plan);
    plan
}

/// An opaque table that grew still names its growth, so the loader turns its row merge off
/// rather than renumbering around it.
fn record_growth(plan: &mut EntryPlan<'_>, recipe: &Recipe) {
    for array in recipe
        .arrays
        .iter()
        .filter(|array| array.kind == OpKind::Append)
    {
        let (descriptor, stride) = (array.descriptor as i64, array.row.stride as i64);
        let (Ok(stock), Ok(built)) = (
            read_view(plan.stock, descriptor, stride),
            read_view(plan.built, descriptor, stride),
        ) else {
            plan.reason = plan
                .reason
                .take()
                .map(|reason| format!("{reason}, and its {} array is malformed", array.name));
            continue;
        };
        let (stock_count, built_count) =
            (stock.map_or(0, |v| v.count), built.map_or(0, |v| v.count));
        if built_count > stock_count {
            plan.growth.push(Target {
                table: plan.tag,
                descriptor: array.descriptor as u32,
                stock_count: stock_count as u32,
                mod_count: built_count as u32,
            });
        }
    }
}

/// Stock ranges the loader rebuilds: descriptors, header counts and the size word, then whole
/// arrays.
#[derive(Default)]
struct Spans {
    structural: Vec<(i64, i64)>,
    arrays: Vec<(i64, i64)>,
}

/// Fills a recipe table's ops and diffs, or names the first rule its build breaks.
fn classify_table(plan: &mut EntryPlan<'_>, recipe: &Recipe) -> Checked<()> {
    let (stock, built) = (plan.stock, plan.built);
    if built.len() < stock.len() {
        return unmergeable("it is smaller than the installed entry");
    }
    let size_word = i64_at(stock, 0) == len(stock);
    if size_word && i64_at(built, 0) != len(built) {
        return unmergeable("its size word doesn't match its size");
    }
    let mut spans = Spans::default();
    if size_word {
        spans.structural.push((0, 8));
    }
    let (stock_views, built_views) = read_views(plan, recipe, &mut spans)?;
    for array in recipe.arrays {
        validate_rows(
            stock,
            &stock_views,
            array.descriptor as i64,
            array,
            i64::MAX,
        )?;
        validate_rows(
            built,
            &built_views,
            array.descriptor as i64,
            array,
            len(stock),
        )?;
    }
    build_ops(plan, recipe, &stock_views, &built_views)?;
    nested_ops(plan, recipe, &stock_views, &built_views, &mut spans)?;
    plan.diffs = diffs(stock, built, &spans.structural, &spans.arrays, true)?;
    Ok(())
}

type Views = HashMap<i64, View>;

/// Reads every recipe array in both copies and records its stock spans.
fn read_views(plan: &EntryPlan<'_>, recipe: &Recipe, spans: &mut Spans) -> Checked<(Views, Views)> {
    let mut stock_views = Views::new();
    let mut built_views = Views::new();
    for array in recipe.arrays {
        let (descriptor, stride) = (array.descriptor as i64, array.row.stride as i64);
        let Some(before) = read_view(plan.stock, descriptor, stride)? else {
            return unmergeable(format!("its installed {} array is empty", array.name));
        };
        let Some(after) = read_view(plan.built, descriptor, stride)? else {
            return unmergeable(format!("its built {} array is empty", array.name));
        };
        if after.count < before.count {
            return unmergeable(format!("rows were removed from {}", array.name));
        }
        if after.count > i64::from(array.limit) {
            return unmergeable(format!(
                "{} holds more than {} rows",
                array.name, array.limit
            ));
        }
        if row_class(plan.built, after.header) != row_class(plan.stock, before.header) {
            return unmergeable(format!("the {} array changed its row class", array.name));
        }
        stock_views.insert(descriptor, before);
        built_views.insert(descriptor, after);
        spans.structural.push((descriptor, descriptor + 16));
        spans.structural.push((before.header, before.header + 8));
        spans
            .arrays
            .push((before.header - 4, before.rows() + before.count * stride));
    }
    Ok((stock_views, built_views))
}

/// Adds one top-level op per recipe array, with the key flags the stock keys earn.
fn build_ops(
    plan: &mut EntryPlan<'_>,
    recipe: &Recipe,
    stock_views: &Views,
    built_views: &Views,
) -> Checked<()> {
    for array in recipe.arrays {
        let descriptor = array.descriptor as i64;
        let (before, after) = (stock_views[&descriptor], built_views[&descriptor]);
        let mut flags = 0;
        if let Some(key) = array.key_offset {
            let keys = |bytes: &[u8], view: View| {
                (0..view.count)
                    .map(|row| u32_at(bytes, view.rows() + row * view.stride + key as i64))
                    .collect::<Vec<_>>()
            };
            let stock_keys = keys(plan.stock, before);
            let built_keys = keys(plan.built, after);
            let stock_sorted = ascending(&stock_keys);
            if array.kind == OpKind::SortedMap {
                if !stock_sorted {
                    return unmergeable(format!("the installed {} keys aren't sorted", array.name));
                }
                if !ascending(&built_keys) {
                    return unmergeable(format!("the built {} keys aren't sorted", array.name));
                }
            } else {
                // A flag the stock keys earn must also hold for the built keys.
                if array.unique && distinct(&stock_keys) {
                    if !distinct(&built_keys) {
                        return unmergeable(format!("the built {} keys repeat", array.name));
                    }
                    flags |= UNIQUE_KEY;
                }
                if array.ordered_when_stock_ascending && stock_sorted {
                    if !ascending(&built_keys) {
                        return unmergeable(format!("the built {} keys aren't sorted", array.name));
                    }
                    flags |= ORDERED;
                }
            }
        }
        plan.ops.push(OpPlan {
            name: array.name.to_owned(),
            kind: array.kind,
            descriptor,
            parent_op: TOP_LEVEL_OP,
            parent_row: 0,
            row: RowShape {
                stride: array.row.stride,
                fields: array.row.fields,
            },
            stock_count: before.count as u32,
            mod_count: after.count as u32,
            limit: array.limit,
            pair_group: array.pair_group,
            flags,
            key_offset: array.key_offset,
        });
    }
    Ok(())
}

fn ascending(keys: &[u32]) -> bool {
    keys.windows(2).all(|pair| pair[0] < pair[1])
}

fn distinct(keys: &[u32]) -> bool {
    keys.iter().collect::<HashSet<_>>().len() == keys.len()
}

/// The row a row pointer names, or -1 for none. The rows were validated, so it lands on a row
/// start.
fn row_index(bytes: &[u8], at: i64, view: View) -> i64 {
    match i64_at(bytes, at) {
        0 => -1,
        value => (at + value - view.rows()) / view.stride,
    }
}

/// Adds a nested op for each stock row array that grew. Any other stock pointer must keep its
/// target.
fn nested_ops(
    plan: &mut EntryPlan<'_>,
    recipe: &Recipe,
    stock_views: &Views,
    built_views: &Views,
    spans: &mut Spans,
) -> Checked<()> {
    let (stock, built) = (plan.stock, plan.built);
    for (parent, array) in recipe.arrays.iter().enumerate() {
        let descriptor = array.descriptor as i64;
        let (before, after) = (stock_views[&descriptor], built_views[&descriptor]);
        let stride = array.row.stride as i64;
        for row in 0..before.count {
            for field in array.row.fields {
                let offset = field.offset as i64;
                let stock_at = before.rows() + row * stride + offset;
                let built_at = after.rows() + row * stride + offset;
                match field.kind {
                    FieldKind::RowPointer => {
                        let rows = field.target_array as i64;
                        if row_index(stock, stock_at, stock_views[&rows])
                            != row_index(built, built_at, built_views[&rows])
                        {
                            return unmergeable(format!(
                                "installed row {row} of {} points at another row",
                                array.name
                            ));
                        }
                        continue;
                    }
                    FieldKind::ObjectPointer => {
                        if target(stock, stock_at) != target(built, built_at) {
                            return unmergeable(format!(
                                "installed row {row} of {} points at another object",
                                array.name
                            ));
                        }
                        continue;
                    }
                    FieldKind::OwnedArray => {}
                }
                let (stock_count, built_count) = (i64_at(stock, stock_at), i64_at(built, built_at));
                let stock_target = target(stock, stock_at + 8);
                let built_target = target(built, built_at + 8);
                if stock_count == built_count && stock_target == built_target {
                    continue;
                }
                if !field.nestable || field.element_stride == 0 {
                    return unmergeable(format!(
                        "installed row {row} of {} changed its array at +{offset}",
                        array.name
                    ));
                }
                if stock_count <= 0
                    || built_count < stock_count
                    || built_count > i64::from(recipes::EDITOR_ROW_LIMIT)
                {
                    return unmergeable(format!(
                        "installed row {row} of {} changed an empty or shrunk array",
                        array.name
                    ));
                }
                let element_stride = field.element_stride as i64;
                let child = View {
                    count: stock_count,
                    header: stock_target,
                    stride: element_stride,
                };
                if row_class(built, built_target) != row_class(stock, child.header) {
                    return unmergeable(format!(
                        "installed row {row} of {} changed its array class",
                        array.name
                    ));
                }
                spans.arrays.push((
                    child.header - 4,
                    child.rows() + child.count * element_stride,
                ));
                spans.structural.push((child.header, child.header + 8));
                plan.ops.push(OpPlan {
                    name: format!("{}[{row}]+{offset}", array.name),
                    kind: OpKind::Append,
                    descriptor: offset,
                    parent_op: parent as u32,
                    parent_row: row as u32,
                    row: RowShape {
                        stride: field.element_stride,
                        fields: field
                            .element
                            .map(|element| element.fields)
                            .unwrap_or_default(),
                    },
                    stock_count: stock_count as u32,
                    mod_count: built_count as u32,
                    limit: recipes::EDITOR_ROW_LIMIT,
                    pair_group: None,
                    flags: 0,
                    key_offset: None,
                });
            }
        }
    }
    Ok(())
}

/// Checks every pointer field of every row, and refuses a row word that points at a typed object
/// the recipe doesn't list. `added` is where the build's added bytes start.
fn validate_rows(
    bytes: &[u8],
    views: &Views,
    descriptor: i64,
    array: &recipes::Array,
    added: i64,
) -> Checked<()> {
    let view = views[&descriptor];
    let stride = array.row.stride as i64;
    for row in 0..view.count {
        let start = view.rows() + row * stride;
        let place = format!("{} row {row}", array.name);
        let declared = validate_fields(bytes, views, array.row.fields, start, added, &place)?;
        let mut word = (start + 7) & !7;
        while word + 8 <= start + stride {
            if !declared.contains(&word) && looks_like_pointer(bytes, word) {
                return unmergeable(format!(
                    "{place} holds an unlisted pointer at +{}",
                    word - start
                ));
            }
            word += 8;
        }
    }
    Ok(())
}

/// The offsets of the words a shape declares, so the pointer scan skips them.
fn validate_fields(
    bytes: &[u8],
    views: &Views,
    fields: &[Field],
    start: i64,
    added: i64,
    place: &str,
) -> Checked<HashSet<i64>> {
    let mut declared = HashSet::new();
    for field in fields {
        let at = start + field.offset as i64;
        match field.kind {
            FieldKind::RowPointer => {
                declared.insert(at);
                let value = i64_at(bytes, at);
                if value == 0 {
                    continue;
                }
                let rows = views[&(field.target_array as i64)];
                let offset = at + value - rows.rows();
                if offset < 0 || offset % rows.stride != 0 || offset / rows.stride >= rows.count {
                    return unmergeable(format!("{place} points outside its target rows"));
                }
            }
            FieldKind::ObjectPointer => {
                declared.insert(at);
                let relative = i64_at(bytes, at);
                if relative != 0 && (at + relative < 0 || at + relative >= len(bytes)) {
                    return unmergeable(format!(
                        "{place} points outside the entry at +{}",
                        field.offset
                    ));
                }
            }
            FieldKind::OwnedArray => {
                declared.insert(at);
                declared.insert(at + 8);
                let relative = i64_at(bytes, at + 8);
                let count = i64_at(bytes, at);
                if count == 0 {
                    let pointed = at + 8 + relative;
                    if relative != 0 && (pointed <= 0 || pointed > len(bytes)) {
                        return unmergeable(format!(
                            "{place} has an empty array pointing outside the entry"
                        ));
                    }
                    continue;
                }
                let header = at + 8 + relative;
                let stride = field.element_stride as i64;
                if relative == 0 || !header_valid(bytes, header, count, stride) {
                    return unmergeable(format!(
                        "{place} has a malformed array at +{}",
                        field.offset
                    ));
                }
                if header < added {
                    continue;
                }
                let Some(element) = field.element.filter(|_| stride > 0) else {
                    continue;
                };
                for index in 0..count {
                    let element_start = header + 16 + index * stride;
                    validate_fields(bytes, views, element.fields, element_start, added, place)?;
                    for local in element.local {
                        validate_local(bytes, local, element_start, added, place)?;
                    }
                }
            }
        }
    }
    Ok(declared)
}

/// An object an added element owns moves with the added bytes, so it and its arrays must sit
/// there too.
fn validate_local(
    bytes: &[u8],
    local: &recipes::Local,
    start: i64,
    added: i64,
    place: &str,
) -> Checked<()> {
    let owned = target(bytes, start + local.offset as i64);
    if owned < 0 {
        return Ok(());
    }
    if owned < added || owned + local.target.stride as i64 > len(bytes) {
        return unmergeable(format!("{place} owns an object outside its added bytes"));
    }
    for field in local.target.fields {
        let array = target(bytes, owned + field.offset as i64 + 8);
        if array >= 0 && array < added {
            return unmergeable(format!("{place} owns an array outside its added bytes"));
        }
    }
    Ok(())
}

fn looks_like_pointer(bytes: &[u8], word: i64) -> bool {
    let value = i64_at(bytes, word);
    if value == 0 {
        return false;
    }
    let target = word + value;
    target >= 4
        && target < len(bytes)
        && u32_at(bytes, target - 4) & CLASS_WORD_MASK == CLASS_WORD_PREFIX
}

/// Every stock-region run where the build differs, split at span edges. With `inside_only`, a run
/// outside every span is refused: a recipe table may change only its rows and structure words.
fn diffs(
    stock: &[u8],
    built: &[u8],
    structural: &[(i64, i64)],
    arrays: &[(i64, i64)],
    inside_only: bool,
) -> Checked<Vec<Diff>> {
    let spans = structural.iter().chain(arrays).copied().collect::<Vec<_>>();
    let size = len(stock);
    let mut edges = spans
        .iter()
        .flat_map(|&(begin, end)| [begin, end])
        .filter(|&edge| edge > 0 && edge < size)
        .chain([size])
        .collect::<Vec<_>>();
    edges.sort_unstable();
    edges.dedup();
    let mut output = Vec::new();
    let mut position = 0usize;
    while position < stock.len() {
        if stock[position] == built[position] {
            position += 1;
            continue;
        }
        let mut end = position + 1;
        while end < stock.len() && stock[end] != built[end] {
            end += 1;
        }
        let mut begin = position as i64;
        while begin < end as i64 {
            let next = edges
                .iter()
                .copied()
                .find(|&edge| edge > begin)
                .map_or(end as i64, |edge| edge.min(end as i64));
            if inside_only
                && !spans
                    .iter()
                    .any(|&(first, last)| first <= begin && next <= last)
            {
                return unmergeable(format!("bytes at +0x{begin:X} outside its arrays changed"));
            }
            output.push(Diff {
                offset: begin as u32,
                old: stock[begin as usize..next as usize].to_vec(),
            });
            begin = next;
        }
        position = end;
    }
    Ok(output)
}

/// A site's row number, read at its width and sign.
fn read_value(bytes: &[u8], at: usize, width: u8, signed: bool) -> i64 {
    match (width, signed) {
        (1, true) => i64::from(bytes[at] as i8),
        (1, false) => i64::from(bytes[at]),
        (2, true) => i64::from(i16::from_le_bytes([bytes[at], bytes[at + 1]])),
        (2, false) => i64::from(u16::from_le_bytes([bytes[at], bytes[at + 1]])),
        (_, true) => i64::from(i32::from_le_bytes(
            bytes[at..at + 4].try_into().expect("4 bytes"),
        )),
        (_, false) => i64::from(u32::from_le_bytes(
            bytes[at..at + 4].try_into().expect("4 bytes"),
        )),
    }
}

/// The row counts a rule's target array holds before and after the build, or `None` when the
/// rule doesn't apply to the entry or the build adds no rows there.
fn rule_counts(
    context: &mut Context<'_>,
    plan: &EntryPlan<'_>,
    rule: &recipes::Rule,
) -> Option<(u32, u32)> {
    let array = rule.array as i64;
    let (stock_count, mod_count) = if rule.op_local {
        if plan.kind != EntryKind::Table || plan.tag != rule.table {
            return None;
        }
        let (_, op) = plan.top_op(array)?;
        (op.stock_count, op.mod_count)
    } else {
        context.counts(rule.table, array)
    };
    (mod_count > stock_count).then_some((stock_count, mod_count))
}

/// The fields a rule reaches whose value names a row in `[stock count, mod count)`.
fn rule_hits(bytes: &[u8], rule: &recipes::Rule, counts: (u32, u32)) -> Vec<SiteHit> {
    let rows = i64::from(counts.0)..i64::from(counts.1);
    walk(bytes, rule.path, 0)
        .into_iter()
        .filter_map(|node| {
            let at = node + rule.field as i64;
            if at < 0 || at + i64::from(rule.width) > len(bytes) {
                return None;
            }
            let value = read_value(bytes, at as usize, rule.width, rule.signed);
            rows.contains(&value).then_some(SiteHit {
                offset: at as u32,
                width: rule.width,
                signed: rule.signed,
                table: rule.table,
                array: rule.array as i64,
                op_local: rule.op_local,
                rule: rule.name,
            })
        })
        .collect()
}

/// Applies every site rule for the entry's table and class, keeping values in
/// `[stock count, mod count)`. Ambiguous references drop every site of the entry.
fn find_sites(context: &mut Context<'_>, plan: &mut EntryPlan<'_>) {
    if plan.kind == EntryKind::Manifest {
        return;
    }
    let rules = recipes::table_rules(plan.tag)
        .iter()
        .chain(recipes::class_rules(plan.class));
    let mut found = BTreeMap::<u32, SiteHit>::new();
    let mut ambiguous = None;
    for rule in rules {
        let Some(counts) = rule_counts(context, plan, rule) else {
            continue;
        };
        for hit in rule_hits(plan.built, rule, counts) {
            match found.get(&hit.offset) {
                Some(existing) if !existing.same(&hit) => {
                    let at = hit.offset;
                    ambiguous.get_or_insert(format!("the field at +{at} has two meanings"));
                }
                Some(_) => {}
                None => {
                    found.insert(hit.offset, hit);
                }
            }
        }
    }
    let mut end = 0;
    for site in found.values() {
        if site.offset < end {
            ambiguous.get_or_insert_with(|| "its row references overlap".into());
        }
        end = site.offset + u32::from(site.width);
    }
    match ambiguous {
        None => plan.sites = found,
        Some(reason) => {
            plan.reason = Some(match plan.reason.take() {
                Some(earlier) => format!("{earlier}, and {reason}"),
                None => reason,
            });
        }
    }
}

/// The nodes a site path reaches. A null pointer, another marker or an unreadable array ends a
/// branch.
fn walk(bytes: &[u8], path: &[Step], node: i64) -> Vec<i64> {
    let Some((step, rest)) = path.split_first() else {
        return vec![node];
    };
    let next: Vec<i64> = match *step {
        Step::Pointer { offset, marker } => {
            let at = node + offset as i64;
            let found = if at >= 0 && at + 8 <= len(bytes) {
                target(bytes, at)
            } else {
                -1
            };
            let marked = marker == 0 || (found >= 4 && u32_at(bytes, found - 4) == marker);
            if found >= 0 && found < len(bytes) && marked {
                vec![found]
            } else {
                Vec::new()
            }
        }
        Step::Each { offset, stride } => {
            match read_view(bytes, node + offset as i64, stride as i64) {
                Ok(Some(view)) => (0..view.count)
                    .map(|row| view.rows() + row * stride as i64)
                    .collect(),
                _ => Vec::new(),
            }
        }
        Step::Pool {
            offset,
            stride,
            first,
            count,
        } => {
            let (first_at, count_at) = (node + first as i64, node + count as i64);
            if first_at < 0 || first_at + 4 > len(bytes) || count_at < 0 || count_at >= len(bytes) {
                Vec::new()
            } else {
                let first = i64::from(u32_at(bytes, first_at) as i32);
                let count = i64::from(bytes[count_at as usize]);
                // The pool is the root array at `offset`, not one under the node.
                match read_view(bytes, offset as i64, stride as i64) {
                    Ok(Some(pool)) if first >= 0 && first + count <= pool.count => (first
                        ..first + count)
                        .map(|row| pool.rows() + row * stride as i64)
                        .collect(),
                    _ => Vec::new(),
                }
            }
        }
        Step::Is { offset, value } => {
            let at = node + offset as i64;
            if at >= 0 && at < len(bytes) && bytes[at as usize] == value {
                vec![node]
            } else {
                Vec::new()
            }
        }
    };
    next.into_iter()
        .flat_map(|child| walk(bytes, rest, child))
        .collect()
}

/// Pair groups whose members this build grows by different row counts. A member it doesn't grow
/// adds 0, and a member the installation lacks isn't checked.
fn check_pairs(stock: &mut dyn Stock, plans: &[&EntryPlan<'_>]) -> Vec<String> {
    let mut groups = BTreeMap::<&str, BTreeMap<(u32, i64), u32>>::new();
    for recipe in recipes::TABLES {
        if !stock.has(recipe.tag) {
            continue;
        }
        for array in recipe.arrays {
            if let Some(group) = array.pair_group {
                groups
                    .entry(group)
                    .or_default()
                    .insert((recipe.tag, array.descriptor as i64), 0);
            }
        }
    }
    for plan in plans {
        if plan.kind == EntryKind::Table {
            for op in &plan.ops {
                if let (TOP_LEVEL_OP, OpKind::Append, Some(group)) =
                    (op.parent_op, op.kind, op.pair_group)
                    && let Some(members) = groups.get_mut(group)
                {
                    members.insert((plan.tag, op.descriptor), op.mod_count - op.stock_count);
                }
            }
        }
        for growth in &plan.growth {
            let group = recipes::table(plan.tag).and_then(|recipe| {
                recipe
                    .arrays
                    .iter()
                    .find(|array| array.descriptor as u32 == growth.descriptor)
                    .and_then(|array| array.pair_group)
            });
            if let Some(members) = group.and_then(|group| groups.get_mut(group)) {
                members.insert(
                    (plan.tag, i64::from(growth.descriptor)),
                    growth.mod_count - growth.stock_count,
                );
            }
        }
    }
    let mut problems = Vec::new();
    for (group, members) in &groups {
        let Some((&(first_table, _), &rows)) = members.iter().next() else {
            continue;
        };
        if let Some((&(table, _), &other)) = members.iter().find(|(_, count)| **count != rows) {
            problems.push(format!(
                "pair group {group} adds {rows} rows to {first_table:08X} but {other} to {table:08X}"
            ));
        }
    }
    problems
}

/// Builds one package's sidecar: targets sorted by table and descriptor, shapes in op order, and
/// sites mapped to targets.
fn assemble(
    context: &mut Context<'_>,
    package: &Package,
    plans: &[&EntryPlan<'_>],
) -> Result<Sidecar, String> {
    let mut targets = BTreeMap::<(u32, u32), Target>::new();
    let mut add_target = |target: Target| -> Result<(), String> {
        let key = (target.table, target.descriptor);
        if targets.get(&key).is_some_and(|known| *known != target) {
            return Err(format!(
                "table {:08X} has two row counts in one build",
                target.table
            ));
        }
        targets.insert(key, target);
        Ok(())
    };
    for plan in plans {
        if plan.kind == EntryKind::Table {
            for op in plan
                .ops
                .iter()
                .filter(|op| op.parent_op == TOP_LEVEL_OP && op.kind == OpKind::Append)
            {
                add_target(Target {
                    table: plan.tag,
                    descriptor: op.descriptor as u32,
                    stock_count: op.stock_count,
                    mod_count: op.mod_count,
                })?;
            }
        }
        for growth in &plan.growth {
            add_target(*growth)?;
        }
        for site in plan.sites.values().filter(|site| !site.op_local) {
            let (stock_count, mod_count) = context.counts(site.table, site.array);
            add_target(Target {
                table: site.table,
                descriptor: site.array as u32,
                stock_count,
                mod_count,
            })?;
        }
    }
    let target_index = targets
        .keys()
        .enumerate()
        .map(|(index, key)| (*key, index as u16))
        .collect::<HashMap<_, _>>();
    let mut shapes = Vec::new();
    let mut entries = Vec::new();
    let mut ordered = plans.to_vec();
    ordered.sort_by_key(|plan| entry_of(plan.tag));
    for plan in ordered {
        let op_index = plan
            .ops
            .iter()
            .enumerate()
            .filter(|(_, op)| op.parent_op == TOP_LEVEL_OP)
            .map(|(index, op)| (op.descriptor, index as u16))
            .collect::<HashMap<_, _>>();
        let mut ops = Vec::with_capacity(plan.ops.len());
        for op in &plan.ops {
            ops.push(Op {
                kind: op.kind,
                flags: op.flags,
                key_width: if op.key_offset.is_some() { 4 } else { 0 },
                key_offset: op.key_offset.unwrap_or(0) as u32,
                parent_op: op.parent_op,
                parent_row: op.parent_row,
                descriptor: op.descriptor as u32,
                shape: add_shape(&mut shapes, op.row, &op_index)?,
                stock_count: op.stock_count,
                mod_count: op.mod_count,
                limit: op.limit,
                pair_group: op.pair_group.map_or(0, recipes::pair_group_id),
            });
        }
        let sites = plan
            .sites
            .values()
            .map(|site| {
                let target = if site.op_local {
                    op_index
                        .get(&site.array)
                        .map(|&index| OP_LOCAL_TARGET | index)
                } else {
                    target_index.get(&(site.table, site.array as u32)).copied()
                };
                Ok(Site {
                    offset: site.offset,
                    width: site.width,
                    flags: if site.signed { SIGNED_SITE } else { 0 },
                    target: target.ok_or("a site names an array without an op or target")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        entries.push(Entry {
            index: entry_of(plan.tag),
            kind: plan.kind,
            stock_size: plan.stock.len() as u32,
            built_size: plan.built.len() as u32,
            stock_sha256: sha256(plan.stock),
            diffs: plan.diffs.clone(),
            ops,
            sites,
        });
    }
    Ok(Sidecar {
        package: package.id,
        patch: package.patch,
        metadata_digest: package.digest,
        targets: targets.into_values().collect(),
        shapes,
        entries,
    })
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut digest = [0; 32];
    digest.copy_from_slice(&Sha256::digest(bytes));
    digest
}

/// Adds a row shape and, after it, the shapes of the elements it owns, so every element shape's
/// index is larger than its owner's.
fn add_shape(
    shapes: &mut Vec<super::sidecar::Shape>,
    row: RowShape,
    op_index: &HashMap<i64, u16>,
) -> Result<u32, String> {
    let index = shapes.len();
    if index >= usize::from(NO_ELEMENT_SHAPE) {
        return Err("the merge data has too many row layouts".into());
    }
    shapes.push(super::sidecar::Shape {
        stride: row.stride as u32,
        fields: Vec::new(),
    });
    let mut fields = Vec::with_capacity(row.fields.len());
    for field in row.fields {
        let reference = match field.kind {
            FieldKind::RowPointer => *op_index
                .get(&(field.target_array as i64))
                .ok_or("a row pointer names an array without an op")?,
            _ => match field.element {
                Some(element) if !element.fields.is_empty() && field.element_stride > 0 => {
                    let element = RowShape {
                        stride: field.element_stride,
                        fields: element.fields,
                    };
                    add_shape(shapes, element, op_index)? as u16
                }
                _ => NO_ELEMENT_SHAPE,
            },
        };
        fields.push(ShapeField {
            row_offset: field.offset as u32,
            kind: field.kind,
            reference,
        });
    }
    shapes[index].fields = fields;
    Ok(index as u32)
}

fn report(plan: &EntryPlan<'_>) -> Description {
    let mut sites = BTreeMap::new();
    for site in plan.sites.values() {
        let target = if site.op_local {
            format!("op +{}", site.array)
        } else {
            format!("{:08X} +{}", site.table, site.array)
        };
        *sites
            .entry(format!("{} to {target}", site.rule))
            .or_default() += 1;
    }
    Description {
        tag: format!("{:08X}", plan.tag),
        class: format!("{:08X}", plan.class),
        kind: match plan.kind {
            EntryKind::Opaque => "opaque",
            EntryKind::Table => "table",
            EntryKind::Manifest => "manifest",
            EntryKind::Added => "added",
        },
        recipe: plan.recipe,
        reason: plan.reason.clone(),
        stock_size: plan.stock.len(),
        built_size: plan.built.len(),
        diffs: plan.diffs.len(),
        ops: plan
            .ops
            .iter()
            .map(|op| format!("{} {}..{}", op.name, op.stock_count, op.mod_count))
            .collect(),
        sites,
    }
}

/// Describes every package a build writes. A package the installation has gets every entry the
/// build changes, and a new one the entries whose fields name rows the build adds. `Err` when a
/// sidecar can't be written, in which case the build ships no merge data.
pub fn describe(
    packages: &[Package],
    build: &Build,
    stock: &mut dyn Stock,
) -> Result<Described, String> {
    let mut context = Context {
        build,
        stock,
        counts: HashMap::new(),
    };
    let mut planned = Vec::with_capacity(packages.len());
    for package in packages {
        let mut plans = Vec::new();
        if package.installed {
            let edited = build
                .edited
                .iter()
                .filter(|(tag, _)| package_of(**tag) == package.id);
            for (&tag, edited) in edited {
                plans.push(plan_installed(&mut context, tag, edited));
            }
        } else {
            let added = build
                .added
                .iter()
                .filter(|(tag, _)| package_of(**tag) == package.id);
            for (&tag, added) in added {
                let mut plan =
                    EntryPlan::new(tag, added.class, &added.built, &[], EntryKind::Added);
                find_sites(&mut context, &mut plan);
                if !plan.sites.is_empty() {
                    plans.push(plan);
                }
            }
        }
        planned.push(plans);
    }
    let all = planned.iter().flatten().collect::<Vec<_>>();
    let pairs = check_pairs(&mut *context.stock, &all);
    let mut sidecars = Vec::with_capacity(packages.len());
    for (package, plans) in packages.iter().zip(&planned) {
        let plans = plans.iter().collect::<Vec<_>>();
        let sidecar = assemble(&mut context, package, &plans)?;
        // Encoding reads the sidecar back as the loader does, so one it would refuse never ships.
        sidecar.write().map_err(|status| {
            format!(
                "package {:04X}'s merge data is invalid: {}",
                package.id,
                status.text()
            )
        })?;
        sidecars.push(sidecar);
    }
    Ok(Described {
        sidecars,
        entries: planned.iter().flatten().map(report).collect(),
        pairs,
    })
}
