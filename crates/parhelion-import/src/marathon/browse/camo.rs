//! Weapon camos, Marathon's alternate weapon looks, and their import as ornaments.
//!
//! A camo is an item whose pattern names a cosmetic entity: a model, the shared weapon cosmetic
//! attachment and a channel bank. A weapon's own look is one camo, the pattern its definition's
//! art list names, and that pattern shares its hash with the camo item it presents. The
//! collectible table gives each camo item an index, and one presentation node lists that index
//! beside the indices of the weapon's other camos. Variants of one weapon share its node.
//!
//! An import assembles each other camo with its cosmetic entity in place of the weapon's, through
//! the same model and material translation, and binds it to a private ornament plug in an
//! ornament socket that starts with Default Ornament. Choices carry no unlock conditions.
//!
//! Variants of one weapon wear one model and choose the same donor, so they offer the same camos
//! with the same conversions. A camo converts once, and every other weapon that offers it on that
//! donor copies the conversion and binds the copy to its own ornament plug.
use super::*;
use crate::tiger::ornament::{Appearance, Layout, fnv};
use sha2::{Digest, Sha256};

/// The store recipe whose folder results are camo conversions.
pub(in crate::marathon) const CONVERSION: &str = "marathon-camo";

/// Single-instance tables of the camo chain.
const COLLECTIBLES: u32 = 0x8080_8FA0;
const NODES: u32 = 0x8080_8C6B;

/// One camo a weapon offers besides its own look.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Camo {
    pub item: u32,
    pub name: String,
    /// The cosmetic entity that replaces the weapon's.
    pub cosmetic: u32,
    /// The grade of the camo's tier, when its definition names one.
    #[serde(default)]
    pub grade: Option<u16>,
}

impl Camo {
    /// The Marathon tier and the Destiny rarity it maps to.
    fn tier(&self) -> Option<(&'static str, &'static str)> {
        self.grade.and_then(crate::marathon::tier::tier)
    }
}

/// Whether a model reads back at its entry's size with its mesh array. The reader indexes every
/// package when it opens and reads a package's entries later, so a package that a game update
/// replaces in between reads back other entries. A camo whose model does not read is left out.
fn readable(r: &mut Reader, model: u32) -> Result<bool> {
    let Some(size) = r
        .manager
        .get_entry(tiger_pkg::TagHash(model))
        .map(|entry| entry.file_size as usize)
    else {
        return Ok(false);
    };
    let data = match r.tag(model, Some(0x8080_881C)) {
        Ok(data) => data,
        Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
        Err(_) => return Ok(false),
    };
    Ok(data.0.len() == size && data.array(16, 0x88, Some(0x8080_87CB)).is_ok())
}

/// The camo chain's tables: each cosmetic item's collectible index, the items at each index,
/// each presentation node's collectible children and each item's definition.
pub(super) struct Index {
    collectibles: BTreeMap<u32, u16>,
    items: BTreeMap<u16, Vec<u32>>,
    nodes: Vec<Vec<u16>>,
    definitions: BTreeMap<u32, u32>,
}

impl Index {
    pub(super) fn read(r: &mut Reader, definitions: &Payload) -> Result<Self> {
        let table = r.tag(single(r, COLLECTIBLES)?, None)?;
        let mut collectibles = BTreeMap::new();
        let mut items = BTreeMap::<u16, Vec<u32>>::new();
        // 104-byte rows: the item hash, then its signed collectible index.
        for row in table.array(8, 0x68, Some(0x8080_8FA4))? {
            let (item, index) = (table.u32(row)?, table.i16(row + 4)?);
            if let Ok(index) = u16::try_from(index) {
                collectibles.insert(item, index);
                items.entry(index).or_default().push(item);
            }
        }
        let table = r.tag(single(r, NODES)?, None)?;
        // 72-byte rows whose collectible children are 16-bit indices at `+0x18`.
        let nodes = table
            .array(8, 0x48, Some(0x8080_8ACC))?
            .into_iter()
            .map(|row| {
                table
                    .array(row + 0x18, 2, Some(0x8080_B1DD))?
                    .into_iter()
                    .map(|at| table.u16(at))
                    .collect()
            })
            .collect::<Result<_>>()?;
        let definitions = definitions
            .array(8, 32, None)?
            .into_iter()
            .map(|row| Ok((definitions.u32(row)?, definitions.u32(row + 16)?)))
            .collect::<Result<_>>()?;
        Ok(Self {
            collectibles,
            items,
            nodes,
            definitions,
        })
    }
}

/// The camos a weapon whose look is the pattern row at `art` offers besides that look.
pub(super) fn find(
    r: &mut Reader,
    strings: &mut Strings,
    tables: &Tables,
    index: &Index,
    art: usize,
    entity: u32,
    look: Option<u32>,
) -> Result<Vec<Camo>> {
    let own = tables.pattern_table.u32(art)?;
    let Some(&collectible) = index.collectibles.get(&own) else {
        return Ok(Vec::new());
    };
    let mut children = Vec::new();
    for node in index
        .nodes
        .iter()
        .filter(|node| node.contains(&collectible))
    {
        for &child in node {
            if !children.contains(&child) {
                children.push(child);
            }
        }
    }
    let mut camos = Vec::<Camo>::new();
    for child in children {
        for &item in index.items.get(&child).into_iter().flatten() {
            if item == own || camos.iter().any(|camo| camo.item == item) {
                continue;
            }
            match camo(r, strings, tables, index, item) {
                Ok(Some(camo))
                    if ![Some(entity), look].contains(&Some(camo.cosmetic))
                        && !camos.iter().any(|other| other.cosmetic == camo.cosmetic) =>
                {
                    camos.push(camo);
                }
                Ok(_) => {}
                Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
                // A node member outside this layout, such as a charm, offers no camo.
                Err(_) => {}
            }
        }
    }
    Ok(camos)
}

/// The named camo `item`, when its pattern names a cosmetic entity with a model.
#[allow(clippy::cognitive_complexity)]
fn camo(
    r: &mut Reader,
    strings: &mut Strings,
    tables: &Tables,
    index: &Index,
    item: u32,
) -> Result<Option<Camo>> {
    let Some(&definition) = index.definitions.get(&item) else {
        return Ok(None);
    };
    let b = r.tag(definition, None)?;
    let Some((p, ITEM)) = resource(&b, 0x80)? else {
        return Ok(None);
    };
    let Some(&row) = usize::try_from(b.i16(p + 0x68)?)
        .ok()
        .and_then(|pattern| tables.patterns.get(pattern))
    else {
        return Ok(None);
    };
    let Some(&cosmetic) = usize::try_from(tables.pattern_table.i16(row + 12)?)
        .ok()
        .and_then(|slot| tables.cosmetics.get(slot))
    else {
        return Ok(None);
    };
    let record = r.tag(tables.cosmetic_table.u32(cosmetic + 4)?, None)?;
    let cosmetic = r.ref64(&record, 8)?;
    if [0, u32::MAX].contains(&cosmetic) {
        return Ok(None);
    }
    let models = models(r, cosmetic)?;
    if models.is_empty() {
        return Ok(None);
    }
    for model in &models {
        if !readable(r, crate::graph::hash(model, "model")?)? {
            return Ok(None);
        }
    }
    let Some(&labels) = tables.strings.get(&item).filter(|&&tag| tag != u32::MAX) else {
        return Ok(None);
    };
    let labels = r.tag(labels, None)?;
    let name = strings.label(r, &labels, NAME)?.unwrap_or_default();
    Ok((!name.is_empty()).then_some(Camo {
        item,
        name,
        cosmetic,
        grade: crate::marathon::tier::grade(&b),
    }))
}

/// The ornament layout of `entry`'s camos on `donor`, read while the native reader is open,
/// with the donor's socket report. `None` when the weapon has no camos or they cannot be laid
/// out. The weapon still imports, and `report` records why.
pub(super) fn layout(
    native: &mut Reader,
    entry: &Entry,
    donors: &[(u32, String)],
    donor: u32,
    report: &mut Value,
) -> Result<Option<(Layout, Value)>> {
    report["camos"] = json!(entry.camos.len());
    if entry.camos.is_empty() {
        return Ok(None);
    }
    let kin = donors.iter().map(|(hash, _)| *hash).collect::<Vec<_>>();
    let laid = crate::tiger::ornament::layout(native, donor, &kin, entry.camos.len())
        .and_then(|layout| Ok((layout, crate::tiger::shadowkeep::sockets(native, donor)?)));
    match laid {
        Ok((layout, sockets)) => {
            report["layout"] = layout.report.clone();
            Ok(Some((layout, sockets)))
        }
        Err(error) if crate::cancellation::is_cancelled(&error) => Err(error),
        Err(error) => {
            report["skipped"] = json!(format!("{error:#}"));
            Ok(None)
        }
    }
}

/// A camo's slug suffix, letters only: `a` through `z`, then `aa`.
fn letters(mut index: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'a' + (index % 26) as u8);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Give a planned camo the ornament choice it takes. Choices count the camos that translated, so
/// a camo that could not leaves no gap, and its private plug and art identities follow its choice
/// as the build allocates them for that socket and choice.
pub(in crate::marathon) fn number(entry: &mut Value, choice: usize) -> Result<()> {
    let roles = entry["ornament_roles"]
        .as_str()
        .context("Camo ornament roles")?
        .to_owned();
    let prefix = format!("{roles}/choice/{choice}");
    entry["target_item"] = json!(format!("{:08X}", fnv(&format!("{prefix}/private-plug"))));
    entry["art_key"] = json!(format!("{:08X}", fnv(&format!("{prefix}/art_key"))));
    entry["ornament"]["choice_index"] = json!(choice);
    Ok(())
}

/// The digest of what a planned camo's conversion depends on besides the identities it is bound
/// to: the converter, both package states in `packages`, the donor, the animation mode and the
/// camo's assembly.
pub(in crate::marathon) fn conversion(entry: &Value, packages: &Value) -> Result<String> {
    let input = json!({"revision":crate::graph::CONVERTER_REVISION,"packages":packages,
        "donor":entry["native_item"],"source_animation":entry["source_animation"],
        "models":entry["models"],"parts":entry["parts"]});
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&input)?)))
}

/// Whether the camo graph folder `graph` holds the conversion `digest` names. Its graph records
/// the digest last, once every payload is written.
pub(in crate::marathon) fn holds(graph: &Path, digest: &str) -> bool {
    fs::read(graph.join("asset-graph.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .is_some_and(|report| report["conversion"] == digest)
}

/// Copy `earlier`, the same camo converted for another weapon, into `graph`, and return its report
/// bound to `entry`'s plug, art and sight identities as a conversion would bind them.
pub(in crate::marathon) fn reuse(earlier: &Path, entry: &Value, graph: &Path) -> Result<Value> {
    let mut report: Value = serde_json::from_slice(&fs::read(earlier.join("asset-graph.json"))?)?;
    // The copy records the conversion once it is bound and complete.
    report
        .as_object_mut()
        .context("Camo graph")?
        .remove("conversion");
    copy(earlier, graph)?;
    let item = crate::graph::hash(entry, "target_item")?;
    report["item_hash"] = json!(item);
    report["art_key"] = json!(crate::graph::hash(entry, "art_key")?);
    for part in report["kept_parts"].as_array_mut().into_iter().flatten() {
        let position = part["position"].as_u64().context("Camo sight position")?;
        part["key"] = json!(crate::marathon::optic::sight_key(item, position));
    }
    Ok(report)
}

/// Copy the payloads below `from` into `to`. The graph manifest is the caller's, and names that
/// start with a dot are the store's transient links.
fn copy(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        crate::cancellation::check()?;
        let entry = entry?;
        let name = entry.file_name();
        if name == "asset-graph.json" || name.to_string_lossy().starts_with('.') {
            continue;
        }
        if entry.file_type()?.is_dir() {
            copy(&entry.path(), &to.join(&name))?;
        } else {
            fs::copy(entry.path(), to.join(&name))
                .with_context(|| format!("Copying {}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Plan entries that assemble each camo on the weapon's plan entry `base`. Each wears the
/// weapon's rig, animation and sight, and becomes the private plug of its ornament choice.
pub(super) fn plan(
    source: &mut Reader,
    entry: &Entry,
    layout: &Layout,
    owner: u32,
    base: &Value,
) -> Result<Vec<Value>> {
    let namespace = entry.namespace();
    let slug = base["slug"].as_str().context("Weapon plan slug")?;
    let lane = layout.lane;
    let mut entries = Vec::new();
    for (index, (camo, &plug)) in entry.camos.iter().zip(&layout.plugs).enumerate() {
        let worn = Entry {
            cosmetic: Some(camo.cosmetic),
            ..entry.clone()
        };
        let (models, parts) = assembly(source, &worn)?;
        ensure!(!models.is_empty(), "Camo {} has no models", camo.name);
        let mut camo_entry = base.clone();
        let object = camo_entry.as_object_mut().context("Weapon plan entry")?;
        object.remove("recipe");
        object.remove("source_item_identity");
        object.insert(
            "slug".into(),
            json!(format!("{slug}-camo-{}", letters(index))),
        );
        object.insert("name".into(), json!(camo.name));
        object.insert("models".into(), json!(models));
        object.insert("parts".into(), json!(parts));
        object.insert("wears".into(), json!(slug));
        object.insert(
            "ornament_roles".into(),
            json!(format!("parhelion/{namespace}/socket/{lane}")),
        );
        let mut link = json!({"target_weapon":owner,"target_socket":lane,
            "native_plug":plug,"native_socket_type":layout.socket_type,
            "plug_category":layout.category,"name":camo.name,"source_item":camo.item});
        if let Some((tier, rarity)) = camo.tier() {
            link["source_tier"] = json!(tier);
            link["rarity"] = json!(rarity);
        }
        object.insert("ornament".into(), link);
        number(&mut camo_entry, index + 1)?;
        entries.push(camo_entry);
    }
    Ok(entries)
}

/// Bind the prepared camo graphs in `output` to the weapon's recipe, in choice order, as
/// [`crate::tiger::ornament::attach`] binds imported appearances.
pub(super) fn attach(
    recipe_path: &Path,
    output: &Path,
    camos: &[Value],
    layout: &Layout,
    sockets: &Value,
) -> Result<()> {
    let graphs = camos
        .iter()
        .map(|camo| {
            let slug = camo["slug"].as_str().context("Camo slug")?;
            Ok(output.join(slug).join("graph"))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut appearances = Vec::new();
    for (index, (camo, graph)) in camos.iter().zip(&graphs).enumerate() {
        let link = &camo["ornament"];
        ensure!(
            link["choice_index"].as_u64() == Some(index as u64 + 1),
            "Camo choices are not contiguous"
        );
        appearances.push(Appearance {
            plug: crate::graph::hash(link, "native_plug")?,
            name: link["name"].as_str().context("Camo name")?,
            graph,
        });
    }
    crate::tiger::ornament::attach(recipe_path, layout, sockets, &appearances)
}
