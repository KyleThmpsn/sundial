//! The native layout imported weapon appearances take. A stock weapon ornament socket starts with
//! Default Ornament, and its socket type whitelists the reset's plug category and one ornament
//! category. The Collections list every stock weapon ornament once, under Exotics / Weapon
//! Ornaments / its weapon type, and an imported ornament's collectible joins its layout plug's
//! leaf. Each appearance clones a distinct stock ornament as its private plug's layout.
use super::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    sync::Arc,
};

/// Default Ornament, the reset every stock weapon ornament socket starts with.
pub const RESET: u32 = 0xAEBAE371;
/// The description of every imported ornament's private plug.
const DESCRIPTION: &str =
    "Changes this weapon's appearance. Select Default Ornament to restore the original appearance.";
/// Presentation node hashes of Exotics and its Weapon Ornaments child.
const EXOTICS: u32 = 0x3FB0E331;
const WEAPON_ORNAMENTS: u32 = 0x47D9E328;
/// The plug block of an item definition, which holds the plug category.
const PLUG_BLOCK: u32 = 0x808077E3;

/// Where a weapon's imported appearances go and the stock ornaments they are laid out on.
pub struct Layout {
    /// The ornament lane: the donor's own, or one appended after its sockets.
    pub lane: usize,
    pub appended: bool,
    pub socket_type: u16,
    /// The donor's socket count, before any appended lane.
    pub sockets: usize,
    /// The ornament category the socket type whitelists besides the reset's. Every private plug
    /// takes it, so the socket accepts each choice.
    pub category: u32,
    /// One distinct stock ornament per appearance, in choice order.
    pub plugs: Vec<u32>,
    pub report: Value,
}

impl Layout {
    /// Moves an appended lane past the `columns` lanes a recipe already uses, since a gameplay
    /// translation can add sockets after the donor's. Choose it before any appearance's identities,
    /// which follow the lane.
    pub fn append_after(&mut self, columns: usize) -> Result<()> {
        if !self.appended || self.lane >= columns {
            return Ok(());
        }
        ensure!(
            columns < sundial::investment::MAX_WEAPON_SOCKETS,
            "The recipe has {columns} sockets and no room for an ornament socket"
        );
        self.lane = columns;
        self.report["lane"] = json!(columns);
        Ok(())
    }
}

struct Tables {
    root: Arc<Payload>,
    items: Arc<Payload>,
    rows: Vec<usize>,
    /// Item table row by item hash.
    index: BTreeMap<u32, usize>,
}

impl Tables {
    fn read(r: &mut Reader) -> Result<Self> {
        let globals = r
            .manager
            .lookup
            .named_tags
            .iter()
            .find(|t| t.name == "investment_globals")
            .context("native globals")?
            .hash
            .0;
        let globals = r.tag(globals, None)?;
        let root = r.tag(globals.u32(16)?, Some(0x80807D84))?;
        let items = r.tag(root.u32(8 + 48 * 16)?, None)?;
        let rows = items.array(8, 24, Some(0x80807BE8))?;
        let mut index = BTreeMap::new();
        for (i, &row) in rows.iter().enumerate() {
            index.entry(items.u32(row)?).or_insert(i);
        }
        Ok(Self {
            root,
            items,
            rows,
            index,
        })
    }

    fn hash(&self, item: usize) -> Result<u32> {
        self.items
            .u32(*self.rows.get(item).context("native item index")?)
    }

    fn definition(&self, r: &mut Reader, item: usize) -> Result<Arc<Payload>> {
        let row = *self.rows.get(item).context("native item index")?;
        r.tag(self.items.u32(row + 16)?, Some(0x80807BEA))
    }

    fn definition_of(&self, r: &mut Reader, hash: u32) -> Result<Arc<Payload>> {
        let item = *self
            .index
            .get(&hash)
            .with_context(|| format!("native item {hash:08X} missing"))?;
        self.definition(r, item)
    }
}

/// The plug category of an item definition, when it has exactly one plug block.
fn category(definition: &Payload) -> Result<Option<u32>> {
    let found = (0x100..definition.0.len().min(0x300).saturating_sub(8))
        .step_by(4)
        .filter(|&at| definition.u32(at).ok() == Some(PLUG_BLOCK))
        .map(|at| definition.u32(at + 4))
        .collect::<Result<Vec<_>>>()?;
    Ok((found.len() == 1).then(|| found[0]))
}

/// A weapon's socket count and its ornament lane with that lane's socket type, the one socket
/// whose initial plug is Default Ornament.
fn ornament_lane(r: &mut Reader, t: &Tables, weapon: u32) -> Result<(usize, Option<(usize, u16)>)> {
    let definition = t.definition_of(r, weapon)?;
    if definition.u64(0x68)? == 0 {
        return Ok((0, None));
    }
    let sockets = definition.array(definition.pointer(0x68)?, 0x50, Some(0x808077C4))?;
    let mut lanes = Vec::new();
    for (lane, &row) in sockets.iter().enumerate() {
        let initial = definition.u16(row + 2)?;
        if initial != u16::MAX && t.hash(usize::from(initial))? == RESET {
            lanes.push((lane, definition.u16(row)?));
        }
    }
    ensure!(
        lanes.len() <= 1,
        "Native weapon {weapon:08X} has several ornament sockets"
    );
    Ok((sockets.len(), lanes.first().copied()))
}

/// The plug categories a socket type whitelists, from the socket type table's 96-byte rows.
fn whitelists(r: &mut Reader, t: &Tables) -> Result<Vec<Vec<u32>>> {
    let table = r.tag(t.root.u32(8 + 94 * 16)?, None)?;
    table
        .array(8, 96, Some(0x80807ABB))?
        .into_iter()
        .map(|row| {
            table
                .array(row + 0x28, 8, Some(0x80807AC0))?
                .into_iter()
                .map(|at| table.u32(at))
                .collect()
        })
        .collect()
}

/// A stock weapon ornament the Collections list: its leaf node, hash and plug category.
struct Ornament {
    leaf: usize,
    hash: u32,
    category: u32,
}

/// Every stock weapon ornament under Exotics / Weapon Ornaments with one art row, one plug
/// category and one Collections leaf, in Collections order. The emission requires all three.
fn collection(r: &mut Reader, t: &Tables) -> Result<Vec<Ornament>> {
    let collectibles = r.tag(t.root.u32(8 + 19 * 16)?, None)?;
    let collectible_rows = collectibles.array(8, 0xB8, Some(0x80803475))?;
    let nodes = r.tag(t.root.u32(8 + 63 * 16)?, None)?;
    let node_rows = nodes.array(8, 0xA8, Some(0x80803056))?;
    let parents = |p: &Payload, row: usize| -> Result<Vec<u16>> {
        p.array(row + 0x18, 2, Some(0x80803962))?
            .into_iter()
            .map(|at| p.u16(at))
            .collect()
    };
    let hashes = node_rows
        .iter()
        .map(|&row| nodes.u32(row + 0x28))
        .collect::<Result<Vec<_>>>()?;
    let mut weapon_ornaments = Vec::new();
    for (index, &row) in node_rows.iter().enumerate() {
        if hashes[index] == WEAPON_ORNAMENTS
            && parents(&nodes, row)?
                .iter()
                .map(|&p| hashes.get(usize::from(p)).copied())
                .collect::<Vec<_>>()
                == [Some(EXOTICS)]
        {
            weapon_ornaments.push(index);
        }
    }
    let [weapon_ornaments] = weapon_ornaments[..] else {
        anyhow::bail!("Native Collections have no single Exotics / Weapon Ornaments node");
    };
    let mut found = Vec::new();
    for (leaf, &row) in node_rows.iter().enumerate() {
        if parents(&nodes, row)? != [weapon_ornaments as u16] {
            continue;
        }
        for at in nodes.array(row + 0x78, 4, Some(0x80803068))? {
            let collectible = *collectible_rows
                .get(usize::from(nodes.u16(at)?))
                .context("native collectible index")?;
            if parents(&collectibles, collectible)? != [leaf as u16] {
                continue;
            }
            let item = usize::from(collectibles.u16(collectible + 0x2C)?);
            let definition = t.definition(r, item)?;
            let art = if definition.u64(0x88)? == 0 {
                0
            } else {
                definition
                    .array(definition.pointer(0x88)?, 4, Some(0x808077B5))?
                    .len()
            };
            if let (1, Some(category)) = (art, category(&definition)?) {
                found.push(Ornament {
                    leaf,
                    hash: t.hash(item)?,
                    category,
                });
            }
        }
    }
    Ok(found)
}

/// The plug categories of the stock weapon ornaments the Collections list. Each names the
/// ornaments of one weapon, which its ornament socket type whitelists.
pub fn categories(r: &mut Reader) -> Result<BTreeSet<u32>> {
    let t = Tables::read(r)?;
    Ok(collection(r, &t)?
        .into_iter()
        .map(|ornament| ornament.category)
        .collect())
}

/// The layout `count` imported appearances of a weapon built on `donor` take. `kin` lists other
/// stock weapons of the donor's type in preference order. The ornament socket is the donor's own
/// when it has one, else an appended lane of the first kin weapon's ornament socket type, else of
/// the type that whitelists the first stock ornament's category. Layout plugs come first from that
/// socket type's ornament category, then from the rest of its Collections leaf, the weapon type's
/// own, then from the other leaves.
pub fn layout(r: &mut Reader, donor: u32, kin: &[u32], count: usize) -> Result<Layout> {
    ensure!(count > 0, "No imported appearances to lay out");
    let t = Tables::read(r)?;
    let reset = t.definition_of(r, RESET)?;
    let reset = category(&reset)?.context("Default Ornament plug category")?;
    let whitelists = whitelists(r, &t)?;
    let ornaments = collection(r, &t)?;
    let (sockets, own) = ornament_lane(r, &t, donor)?;
    let mut socket_type = own.map(|(_, socket_type)| socket_type);
    let mut type_source = own.map(|_| donor);
    if socket_type.is_none() {
        for &weapon in kin.iter().filter(|&&weapon| weapon != donor) {
            match ornament_lane(r, &t, weapon) {
                Ok((_, Some((_, found)))) => {
                    socket_type = Some(found);
                    type_source = Some(weapon);
                    break;
                }
                Ok(_) => {}
                Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
                // A kin weapon whose sockets fall outside this layout lends no socket type.
                Err(_) => {}
            }
        }
    }
    let socket_type = match socket_type {
        Some(socket_type) => socket_type,
        None => {
            let first = ornaments
                .first()
                .context("Native Collections list no weapon ornament")?;
            let found = whitelists
                .iter()
                .position(|list| list.contains(&first.category) && list.contains(&reset))
                .context("No native socket type takes a stock weapon ornament")?;
            u16::try_from(found)?
        }
    };
    let whitelist = whitelists
        .get(usize::from(socket_type))
        .context("Ornament socket type outside the socket type table")?;
    ensure!(
        whitelist.contains(&reset),
        "Socket type {socket_type} does not take Default Ornament"
    );
    let categories = whitelist
        .iter()
        .copied()
        .filter(|&category| category != reset)
        .collect::<Vec<_>>();
    let [category] = categories[..] else {
        anyhow::bail!("Socket type {socket_type} does not whitelist one ornament category");
    };
    let leaf = ornaments
        .iter()
        .find(|ornament| ornament.category == category)
        .map(|ornament| ornament.leaf);
    let rank = |ornament: &Ornament| {
        if ornament.category == category {
            0
        } else if Some(ornament.leaf) == leaf {
            1
        } else {
            2
        }
    };
    let mut ordered = ornaments.iter().collect::<Vec<_>>();
    // A stable sort keeps Collections order within each rank.
    ordered.sort_by_key(|ornament| rank(ornament));
    ensure!(
        ordered.len() >= count,
        "Native Collections list {} weapon ornaments, and {count} appearances need one each",
        ordered.len()
    );
    let plugs = ordered
        .iter()
        .take(count)
        .map(|ornament| ornament.hash)
        .collect::<Vec<_>>();
    let (lane, appended) = match own {
        Some((lane, _)) => (lane, false),
        None => {
            ensure!(
                sockets < sundial::investment::MAX_WEAPON_SOCKETS,
                "The native base has {sockets} sockets and no room for an ornament socket"
            );
            (sockets, true)
        }
    };
    let report = json!({"lane":lane,"appended":appended,"socket_type":socket_type,
        "socket_type_from":type_source.map(|hash| format!("{hash:08X}")),
        "category":category,"reset_category":reset,"leaf":leaf,
        "plugs":plugs.iter().map(|hash| format!("{hash:08X}")).collect::<Vec<_>>(),
        "stock_ornaments":ornaments.len()});
    Ok(Layout {
        lane,
        appended,
        socket_type,
        sockets,
        category,
        plugs,
        report,
    })
}

/// The working folder of the package reads that find and lay out a weapon's ornaments, beside
/// the conversion's `ornaments` folder `ornaments`, which holds one folder per ornament.
pub fn reads(ornaments: &Path) -> std::path::PathBuf {
    ornaments.with_file_name("ornament-reads")
}

/// FNV-1 of `text`, how the build derives an identity from its roles.
pub fn fnv(text: &str) -> u32 {
    text.bytes().fold(0x811C_9DC5u32, |h, b| {
        h.wrapping_mul(0x0100_0193) ^ u32::from(b)
    })
}

/// The roles the build allocates the private plug identities of ornament `choice` of `lane` under.
/// The plug's item is FNV-1 of `{roles}/private-plug`, which its graph takes as its item hash.
pub fn roles(namespace: &str, lane: usize, choice: usize) -> String {
    format!("parhelion/{namespace}/socket/{lane}/choice/{choice}")
}

/// One converted appearance an ornament choice offers.
pub struct Appearance<'a> {
    /// The stock ornament its private plug is laid out on.
    pub plug: u32,
    pub name: &'a str,
    pub graph: &'a Path,
}

/// Bind converted appearances to the weapon recipe at `recipe_path`, in choice order: an ornament
/// column that starts with Default Ornament and offers each appearance's private plug, and one
/// attachment per appearance. `sockets` is the native base's socket report. The lane may already
/// hold a column that offers only Default Ornament, which a gameplay mapping leaves there.
/// Setting a column makes the build curate every inherited socket, so a randomized lane without
/// a default takes the donor's curated fallback.
pub fn attach(
    recipe_path: &Path,
    layout: &Layout,
    sockets: &Value,
    appearances: &[Appearance],
) -> Result<()> {
    let mut recipe: Value = serde_json::from_slice(&fs::read(recipe_path)?)?;
    let owner = crate::graph::hash(&recipe["identity"], "item_hash")?;
    let mut columns = recipe["overrides"]["socket_columns"].take();
    let mut fallbacks = Vec::new();
    super::shadowkeep::fill_native_defaults(sockets, &mut columns, &mut fallbacks)?;
    if columns.is_null() {
        columns = json!([]);
    }
    let lanes = columns.as_array_mut().context("Recipe socket columns")?;
    if lanes.len() < layout.lane + 1 {
        lanes.resize(layout.sockets.max(layout.lane + 1), Value::Null);
    }
    let reset = json!(format!("0x{RESET:08X}"));
    let lane = &lanes[layout.lane];
    ensure!(
        lane.is_null()
            || lane["choices"]
                .as_array()
                .is_some_and(|choices| choices.iter().all(|choice| choice == &reset)),
        "The ornament socket already has a column"
    );
    let mut choices = vec![reset];
    let mut variants = recipe["overrides"]["socket_plug_variants"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut reference: crate::GraphReference =
        serde_json::from_value(recipe["overrides"]["imported_graph"].clone())
            .context("Weapon recipe imported graph")?;
    for appearance in appearances {
        let plug = format!("0x{:08X}", appearance.plug);
        variants.push(
            json!({"socket_index":layout.lane,"choice_index":choices.len(),
            "source_plug_hash":plug,"name":appearance.name,"description":DESCRIPTION,
            "replace_effects":true,"sandbox_perks":[]}),
        );
        choices.push(json!(plug));
        reference
            .attachments
            .push(crate::GraphReference::new(appearance.graph, owner)?);
    }
    lanes[layout.lane] = json!({"choices":choices,"socket_type":layout.socket_type,
        "choice_conditions":[],"choice_weight_bits":[],"randomized_selection_program":[]});
    recipe["overrides"]["socket_columns"] = columns;
    recipe["overrides"]["socket_plug_variants"] = json!(variants);
    recipe["overrides"]["imported_graph"] = serde_json::to_value(reference)?;
    crate::io::write_json(recipe_path, &recipe)?;
    if !fallbacks.is_empty() {
        crate::io::write_json(
            &recipe_path.with_file_name("socket-fallbacks.json"),
            &json!(fallbacks),
        )?;
    }
    Ok(())
}
