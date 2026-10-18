//! The importable Marathon corpus, read from the installed packages: typed items whose pattern
//! entity carries a model, with their localized name and type.
use crate::tiger::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Bumped when the stored row changes, so an older listing is read again.
const SCHEMA: u32 = 2;

/// Each of these classes has exactly one instance, the table the listing reads.
const STRING_INDEX: u32 = 0x8080_71C8;
const ITEM_STRINGS: u32 = 0x8080_6EF0;
const ITEM_DEFINITIONS: u32 = 0x8080_9685;
const PATTERNS: u32 = 0x8080_6CAC;
const COSMETICS: u32 = 0x8080_3081;
const ASSIGNMENTS: u32 = 0x8080_6590;

/// Resource classes.
const ITEM: u32 = 0x8080_91D3;
const MODEL: u32 = 0x8080_8678;
const PLACEMENTS: u32 = 0x8080_9F76;
const ATTACHMENTS: u32 = 0x8080_4A14;
const SKELETON: u32 = 0x8080_9FB7;
const ATTACHMENT: u32 = 0x8080_32C4;
const ATTACHMENT_POINT: u32 = 0x8080_32C9;

/// Item strings offsets of the name and type labels.
const NAME: usize = 0xBC;
const TYPE: usize = 0xD0;

const IDENTITY: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Entry {
    pub item: u32,
    pub name: String,
    /// The localized item type. Empty when its string bank does not resolve.
    pub kind: String,
    /// The native weapon type the item fires as.
    #[serde(default)]
    pub native: Option<String>,
    /// The pattern's entity, which holds the body, its attachments and their placements.
    pub entity: u32,
    /// The cosmetic entity, whose models join the body.
    pub cosmetic: Option<u32>,
    pub models: usize,
}

impl Entry {
    /// The native weapon type this item fires as.
    pub fn weapon_type(&self) -> Option<&'static str> {
        self.native
            .as_deref()
            .and_then(crate::weapon_type::canonical)
    }

    fn namespace(&self) -> String {
        format!("parhelion.marathon.{:08x}", self.item)
    }

    /// The authored item hash an import of this entry receives.
    pub fn destination(&self) -> Result<u32> {
        crate::graph::hash(
            &crate::d2_mot::batch::identity(&self.namespace())?,
            "item_hash",
        )
    }

    /// The converter accepts lowercase letters and hyphens.
    fn slug(&self) -> String {
        let mut slug = String::new();
        for c in self.name.to_lowercase().chars() {
            if c.is_ascii_lowercase() {
                slug.push(c);
            } else if !slug.ends_with('-') {
                slug.push('-');
            }
        }
        let slug = slug.trim_matches('-');
        if slug.is_empty() {
            "item".to_owned()
        } else {
            slug.to_owned()
        }
    }
}

/// The packages folder of a chosen folder: the folder itself or the installation's `packages`.
pub fn packages_path(chosen: &Path) -> Result<PathBuf> {
    [chosen.to_path_buf(), chosen.join("packages")]
        .into_iter()
        .find(|path| crate::tiger::reader::package_stamp(path).is_ok())
        .with_context(|| format!("No Marathon packages in {}", chosen.display()))
}

fn single(r: &Reader, class: u32) -> Result<u32> {
    let tags = r.classes(class);
    ensure!(
        tags.len() == 1,
        "Expected one Marathon {class:08X} table, found {}",
        tags.len()
    );
    Ok(tags[0])
}

/// A nonzero relative pointer and the class word before its target.
fn resource(b: &Payload, o: usize) -> Result<Option<(usize, u32)>> {
    if b.u64(o)? == 0 {
        return Ok(None);
    }
    let target = b.pointer(o)?;
    Ok(Some((
        target,
        b.u32(target.checked_sub(4).context("resource class")?)?,
    )))
}

fn cancelled(error: anyhow::Error) -> Result<()> {
    if crate::cancellation::is_cancelled(&error) {
        Err(error)
    } else {
        Ok(())
    }
}

/// Localized string banks, read on first use.
struct Strings {
    banks: Vec<u32>,
    loaded: BTreeMap<usize, BTreeMap<u32, String>>,
    /// The weapon type most strings of each read bank name.
    types: BTreeMap<usize, Option<&'static str>>,
}

impl Strings {
    fn new(r: &mut Reader) -> Result<Self> {
        let index = r.tag(single(r, STRING_INDEX)?, None)?;
        let banks = index
            .array(8, 24, None)?
            .into_iter()
            .map(|o| r.ref64(&index, o + 8).unwrap_or(u32::MAX))
            .collect();
        Ok(Self {
            banks,
            loaded: BTreeMap::new(),
            types: BTreeMap::new(),
        })
    }

    /// The weapon type the bank holding the label at `field` is grouped by, once `label` has
    /// read that bank. Weapon names share one bank per weapon category with its title and
    /// descriptions.
    fn bank_type(&mut self, strings: &Payload, field: usize) -> Result<Option<&'static str>> {
        let Ok(index) = usize::try_from(strings.u32(field)? as i32) else {
            return Ok(None);
        };
        let Some(values) = self.loaded.get(&index) else {
            return Ok(None);
        };
        Ok(*self.types.entry(index).or_insert_with(|| {
            crate::weapon_type::by_majority(values.values().map(String::as_str))
        }))
    }

    fn bank(r: &mut Reader, tag: u32) -> Result<BTreeMap<u32, String>> {
        let header = r.tag(tag, None)?;
        let hashes = header.array(8, 4, None)?;
        let data = r.tag(header.u32(0x18)?, None)?;
        let rows = data.array(0x38, 0x10, None)?;
        ensure!(
            hashes.len() == rows.len(),
            "Marathon string bank rows differ"
        );
        let mut values = BTreeMap::new();
        for (hash, row) in hashes.into_iter().zip(rows) {
            let start = data.pointer(row)?;
            let parts = data.u64(row + 8)?;
            ensure!(parts <= 4096, "Marathon string has too many parts");
            let mut text = String::new();
            for part in 0..parts as usize {
                let p = start + part * 0x20;
                let at = data.pointer(p + 8)?;
                let size = usize::from(data.u16(p + 0x14)?);
                let shift = u32::from(data.u16(p + 0x18)?);
                let bytes = data
                    .0
                    .get(at..at + size)
                    .context("Marathon string outside its bank")?;
                text.extend(
                    String::from_utf8_lossy(bytes)
                        .chars()
                        .map(|c| char::from_u32(c as u32 + shift).unwrap_or(c)),
                );
            }
            values.insert(header.u32(hash)?, text.trim_end_matches('\0').to_owned());
        }
        Ok(values)
    }

    /// The label at `field` of an item strings record. Empty when its bank does not resolve.
    fn label(&mut self, r: &mut Reader, strings: &Payload, field: usize) -> Result<Option<String>> {
        let Ok(index) = usize::try_from(strings.u32(field)? as i32) else {
            return Ok(None);
        };
        let Some(&bank) = self.banks.get(index) else {
            return Ok(None);
        };
        let hash = strings.u32(field + 4)?;
        if let std::collections::btree_map::Entry::Vacant(entry) = self.loaded.entry(index) {
            let values = if bank == u32::MAX {
                BTreeMap::new()
            } else {
                match Self::bank(r, bank) {
                    Ok(values) => values,
                    Err(error) => {
                        cancelled(error)?;
                        BTreeMap::new()
                    }
                }
            };
            entry.insert(values);
        }
        Ok(Some(
            self.loaded[&index].get(&hash).cloned().unwrap_or_default(),
        ))
    }
}

struct Component {
    tag: u32,
    data: Arc<Payload>,
    resource: usize,
    class: u32,
}

fn components(r: &mut Reader, entity: u32) -> Result<Vec<Component>> {
    let e = r.tag(entity, None)?;
    let mut out = Vec::new();
    for o in e.array(8, 12, None)? {
        let tag = e.u32(o)?;
        let data = r.tag(tag, None)?;
        if let Some((resource, class)) = resource(&data, 24)? {
            out.push(Component {
                tag,
                data,
                resource,
                class,
            });
        }
    }
    Ok(out)
}

/// Model components of an entity as converter model rows, without a transform.
fn models(r: &mut Reader, entity: u32) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for c in components(r, entity)? {
        if c.class != MODEL {
            continue;
        }
        let model = c.data.u32(c.resource + 0x234)?;
        if model != 0 && model != u32::MAX {
            out.push(json!({"entity":format!("{entity:08X}"),"component":format!("{:08X}",c.tag),"model":format!("{model:08X}")}));
        }
    }
    Ok(out)
}

fn read_item(
    r: &mut Reader,
    strings: &mut Strings,
    tables: &Tables,
    item: u32,
    definition: u32,
) -> Result<Option<Entry>> {
    let Some(&labels) = tables.strings.get(&item) else {
        return Ok(None);
    };
    if labels == u32::MAX {
        return Ok(None);
    }
    let labels = r.tag(labels, None)?;
    let Some(kind) = strings.label(r, &labels, TYPE)? else {
        // Variants without a type repeat a typed item.
        return Ok(None);
    };
    let name = strings.label(r, &labels, NAME)?.unwrap_or_default();
    if name.is_empty() {
        return Ok(None);
    }
    // Weapon type labels are absent from the current string banks. A resolved label names a
    // mod, implant or other item, which keeps its own type.
    let native = if kind.is_empty() {
        strings
            .bank_type(&labels, NAME)?
            .or_else(|| crate::weapon_type::from_text(&name))
    } else {
        crate::weapon_type::from_text(&kind)
    };
    let b = r.tag(definition, None)?;
    let Some((p, ITEM)) = resource(&b, 0x80)? else {
        return Ok(None);
    };
    let index = b.i16(p + 0x68)?;
    let art = match b.array(p + 0x28, 4, None)?.first() {
        Some(&o) => b.u32(o)? as i32,
        None => i32::from(index),
    };
    let (Ok(index), Ok(art)) = (usize::try_from(index), usize::try_from(art)) else {
        return Ok(None);
    };
    let (Some(&pattern), Some(&art)) = (tables.patterns.get(index), tables.patterns.get(art))
    else {
        return Ok(None);
    };
    let Some(&entity) = tables
        .assignments
        .get(&tables.pattern_table.u32(pattern + 8)?)
    else {
        return Ok(None);
    };
    if entity == u32::MAX {
        return Ok(None);
    }
    let cosmetic = match usize::try_from(tables.pattern_table.i16(art + 12)?) {
        Ok(slot) => match tables.cosmetics.get(slot) {
            Some(&row) => {
                let tag = tables.cosmetic_table.u32(row + 4)?;
                let record = r.tag(tag, None)?;
                Some(r.ref64(&record, 8).unwrap_or(u32::MAX))
                    .filter(|&tag| tag != u32::MAX && tag != entity)
            }
            None => None,
        },
        Err(_) => None,
    };
    let mut count = models(r, entity)?.len();
    if let Some(cosmetic) = cosmetic {
        count += models(r, cosmetic)?.len();
    }
    if count == 0 {
        return Ok(None);
    }
    Ok(Some(Entry {
        item,
        name,
        kind,
        native: native.map(str::to_owned),
        entity,
        cosmetic,
        models: count,
    }))
}

struct Tables {
    strings: BTreeMap<u32, u32>,
    pattern_table: Arc<Payload>,
    patterns: Vec<usize>,
    cosmetic_table: Arc<Payload>,
    cosmetics: Vec<usize>,
    assignments: BTreeMap<u32, u32>,
}

/// Every typed item with a modeled pattern, sorted by name. `progress` receives the items read
/// so far and the total.
pub fn browse(
    packages: &Path,
    scratch: &Path,
    mut progress: impl FnMut(usize, usize),
) -> Result<Vec<Entry>> {
    let mut r = Reader::marathon_discovery(packages, scratch)?;
    let mut strings = Strings::new(&mut r)?;
    let table = r.tag(single(&r, ITEM_STRINGS)?, None)?;
    let item_strings = table
        .array(8, 0x20, None)?
        .into_iter()
        .map(|o| Ok((table.u32(o)?, r.ref64(&table, o + 0x10).unwrap_or(u32::MAX))))
        .collect::<Result<_>>()?;
    let pattern_table = r.tag(single(&r, PATTERNS)?, None)?;
    let cosmetic_table = r.tag(single(&r, COSMETICS)?, None)?;
    let root = r.tag(single(&r, ASSIGNMENTS)?, None)?;
    let assignment = r.tag(r.ref64(&root, 16)?, None)?;
    let tables = Tables {
        strings: item_strings,
        patterns: pattern_table.array(8, 0x48, None)?,
        cosmetics: cosmetic_table.array(8, 8, None)?,
        assignments: assignment
            .array(8, 24, None)?
            .into_iter()
            .map(|o| {
                Ok((
                    assignment.u32(o)?,
                    r.ref64(&assignment, o + 8).unwrap_or(u32::MAX),
                ))
            })
            .collect::<Result<_>>()?,
        pattern_table,
        cosmetic_table,
    };
    let definitions = r.tag(single(&r, ITEM_DEFINITIONS)?, None)?;
    let rows = definitions.array(8, 32, None)?;
    let mut entries = Vec::new();
    for (done, o) in rows.iter().enumerate() {
        if done % 64 == 0 {
            progress(done, rows.len());
        }
        let item = definitions.u32(*o)?;
        let definition = definitions.u32(o + 16)?;
        // An item whose records do not follow this layout is not a modeled pattern item.
        match read_item(&mut r, &mut strings, &tables, item, definition) {
            Ok(Some(entry)) => entries.push(entry),
            Ok(None) => {}
            Err(error) => cancelled(error)?,
        }
    }
    progress(rows.len(), rows.len());
    entries.sort_by_key(|entry| (entry.name.to_lowercase(), entry.item));
    Ok(entries)
}

#[derive(Deserialize, Serialize)]
struct Stored {
    stamp: String,
    entries: Vec<Entry>,
}

/// `browse` through a listing stored in `cache_dir`, read again when a package changes or
/// `refresh` is set.
pub fn browse_cached(
    packages: &Path,
    cache_dir: &Path,
    refresh: bool,
    progress: impl FnMut(usize, usize),
) -> Result<Vec<Entry>> {
    let packages = packages
        .canonicalize()
        .context("Marathon packages folder")?;
    let stamp = format!(
        "{SCHEMA}-{}",
        crate::tiger::reader::package_stamp(&packages)?
    );
    let path = cache_dir.join("marathon-listing.json");
    if !refresh
        && let Some(stored) = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
            .filter(|stored| stored.stamp == stamp)
    {
        return Ok(stored.entries);
    }
    fs::create_dir_all(cache_dir)?;
    let scratch = tempfile::Builder::new()
        .prefix("marathon-listing-")
        .tempdir_in(cache_dir)?;
    let entries = browse(&packages, scratch.path(), progress)?;
    ensure!(!entries.is_empty(), "No importable Marathon items");
    // A failed write only costs the next listing a reread.
    let _ = crate::cache::write_json(
        &path,
        &Stored {
            stamp,
            entries: entries.clone(),
        },
    );
    Ok(entries)
}

fn four(b: &Payload, o: usize) -> Result<[f64; 4]> {
    Ok([
        f64::from(b.f32(o)?),
        f64::from(b.f32(o + 4)?),
        f64::from(b.f32(o + 8)?),
        f64::from(b.f32(o + 12)?),
    ])
}

/// Row-major rigid transform of a quaternion and a translation.
fn matrix(q: [f64; 4], t: [f64; 4]) -> [f64; 16] {
    let [x, y, z, w] = q;
    let s = 2.0 / (x * x + y * y + z * z + w * w);
    [
        1.0 - s * (y * y + z * z),
        s * (x * y - z * w),
        s * (x * z + y * w),
        t[0],
        s * (x * y + z * w),
        1.0 - s * (x * x + z * z),
        s * (y * z - x * w),
        t[1],
        s * (x * z - y * w),
        s * (y * z + x * w),
        1.0 - s * (x * x + y * y),
        t[2],
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

fn multiply(a: &[f64; 16], b: &[f64; 16]) -> [f64; 16] {
    let mut out = [0.0; 16];
    for row in 0..4 {
        for column in 0..4 {
            out[row * 4 + column] = (0..4).map(|k| a[row * 4 + k] * b[k * 4 + column]).sum();
        }
    }
    out
}

/// The body models and every attachment's models, placed by the entity's attachment points.
fn assembly(r: &mut Reader, entry: &Entry) -> Result<Vec<Value>> {
    let mut placed = Vec::new();
    for entity in [Some(entry.entity), entry.cosmetic].into_iter().flatten() {
        for mut model in models(r, entity)? {
            model["transform"] = json!(IDENTITY);
            placed.push(model);
        }
    }
    let mut placements = BTreeMap::new();
    let mut attachments = Vec::new();
    let mut skeleton = Vec::new();
    for c in components(r, entry.entity)? {
        match c.class {
            PLACEMENTS => {
                for o in c.data.array(c.resource + 0xA8, 48, None)? {
                    placements.insert(
                        c.data.u32(o + 0x28)?,
                        (
                            four(&c.data, o)?,
                            four(&c.data, o + 16)?,
                            c.data.u32(o + 0x24)?,
                        ),
                    );
                }
            }
            ATTACHMENTS => {
                for o in c.data.array(c.resource + 0x3D8, 32, None)? {
                    let tag = r.ref64(&c.data, o + 16).unwrap_or(u32::MAX);
                    if tag != u32::MAX {
                        attachments.push(tag);
                    }
                }
            }
            SKELETON => {
                skeleton = c
                    .data
                    .array(c.resource + 0xC0, 32, None)?
                    .into_iter()
                    .map(|o| Ok((four(&c.data, o)?, four(&c.data, o + 16)?)))
                    .collect::<Result<_>>()?;
            }
            _ => {}
        }
    }
    for attachment in attachments {
        let found = models(r, attachment)?;
        if found.is_empty() {
            continue;
        }
        let mut point = None;
        for c in components(r, attachment)? {
            if c.class == ATTACHMENT
                && let Some((at, class)) = resource(&c.data, c.resource + 0x148)?
            {
                ensure!(
                    class == ATTACHMENT_POINT,
                    "Unexpected Marathon attachment point"
                );
                point = Some(c.data.u32(at)?);
            }
        }
        let point = point.with_context(|| format!("Attachment {attachment:08X} has no point"))?;
        let (rotation, translation, bone) = placements
            .get(&point)
            .with_context(|| format!("Attachment point {point:08X} has no placement"))?;
        let mut transform = matrix(*rotation, *translation);
        if *bone != 0 {
            let (rotation, translation) = skeleton
                .get(*bone as usize)
                .with_context(|| format!("Attachment bone {bone} is outside the skeleton"))?;
            transform = multiply(&matrix(*rotation, *translation), &transform);
        }
        for mut model in found {
            model["transform"] = json!(transform);
            placed.push(model);
        }
    }
    Ok(placed)
}

/// Translate one entry onto `donor` into `output`, which must not exist. Returns the
/// prepared recipe.
pub fn import(
    entry: &Entry,
    donor: u32,
    donor_name: &str,
    packages: &Path,
    native_packages: &Path,
    output: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<PathBuf> {
    let parent = output.parent().context("Marathon import folder")?;
    fs::create_dir_all(parent)?;
    let work = tempfile::Builder::new()
        .prefix("marathon-plan-")
        .tempdir_in(parent)?;
    progress("Assembling source model…".into());
    let mut source = Reader::marathon_discovery(packages, &work.path().join("source"))?;
    let models = assembly(&mut source, entry)?;
    ensure!(!models.is_empty(), "{} has no models", entry.name);
    progress("Reading native weapon…".into());
    let template_dir = work.path().join("native");
    let mut native = Reader::new(native_packages, &template_dir, false)?;
    let template = crate::tiger::shadowkeep::extract(&mut native, donor)?;
    crate::io::write_json(&template_dir.join("template-report.json"), &template)?;
    native.finish()?;
    let namespace = entry.namespace();
    let identity = crate::d2_mot::batch::identity(&namespace)?;
    let target = identity["item_hash"].clone();
    let art_key = format!("parhelion/{namespace}/art_key")
        .bytes()
        .fold(0x811C_9DC5u32, |h, b| {
            h.wrapping_mul(0x0100_0193) ^ u32::from(b)
        });
    let slug = entry.slug();
    let recipe = json!({
        "schema": 1,
        "kind": "weapon",
        "namespace": namespace,
        "collection_placement": "sunrise_badge",
        "donor": {"item_hash": format!("0x{donor:08X}"), "expected_name": donor_name},
        "identity": identity,
        "name": entry.name,
        "source": "Source: Marathon",
        "overrides": {},
    });
    let plan = json!([{
        "slug": slug,
        "name": entry.name,
        "models": models,
        "native_item": format!("{donor:08X}"),
        "target_item": target,
        "art_key": format!("{art_key:08X}"),
        "native_template": template_dir,
        "recipe": recipe,
    }]);
    let plan_path = work.path().join("plan.json");
    crate::io::write_json(&plan_path, &plan)?;
    progress("Translating model…".into());
    super::prepare(&plan_path, packages, native_packages, output)?;
    Ok(output.join(slug).join("weapon.parhelion.json"))
}
