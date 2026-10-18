//! Link pinned imported payloads into a private native reader for preview and artwork.
use crate::GraphReference;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use sundial::package_authoring::PackageManager;
use sundial::ui::model_preview::{LocalAppearance, LocalScene};
use tiger_pkg::TagHash;

pub fn appearance(reference: &GraphReference, kind: &str) -> LocalAppearance {
    let reference = reference.clone();
    let kind = kind.to_owned();
    let key = format!(
        "{}:{}:{kind:?}",
        reference.directory.display(),
        reference.sha256
    );
    LocalAppearance::new(key, move |manager| read(&reference, &kind, manager))
}

fn word(value: &Value) -> Result<u32, String> {
    value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| "Imported preview tag is invalid".into())
}

fn read(
    reference: &GraphReference,
    item_kind: &str,
    manager: &mut PackageManager,
) -> Result<LocalScene, String> {
    validate(reference, item_kind)?;
    let graph: Value = serde_json::from_slice(
        &fs::read(reference.directory.join("asset-graph.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let root = reference
        .directory
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let (package, symbols, entries) = prepare(&graph, &root, manager)?;
    // Validate after reading too, so concurrent file changes cannot assemble a mixed graph.
    validate(reference, item_kind)?;
    manager.add_local_package(package, entries)?;
    scene(&graph, item_kind, &symbols, manager)
}

type Symbols = BTreeMap<String, u32>;
type Entry = (tiger_pkg::package::UEntryHeader, Vec<u8>);
type Entries = Vec<Entry>;

fn tag(symbols: &Symbols, name: &Value) -> Result<u32, String> {
    let name = name.as_str().ok_or("Imported preview reference missing")?;
    symbols
        .get(name)
        .copied()
        .ok_or_else(|| format!("Imported preview symbol {name} is missing"))
}

fn prepare(
    graph: &Value,
    root: &Path,
    manager: &PackageManager,
) -> Result<(u16, Symbols, Entries), String> {
    let nodes = graph["nodes"]
        .as_array()
        .ok_or("Imported preview nodes missing")?;
    if nodes.is_empty() || nodes.len() > 8192 {
        return Err("Imported preview exceeds the local package budget".into());
    }
    let package = (1..=0x3ffu16)
        .rev()
        .find(|p| {
            !manager.package_paths.contains_key(p)
                && !manager.lookup.tag32_entries_by_pkg.contains_key(p)
        })
        .ok_or("No unused package identity for imported preview")?;
    let mut symbols = BTreeMap::new();
    for (index, node) in nodes.iter().enumerate() {
        let symbol = node["symbol"]
            .as_str()
            .ok_or("Imported preview symbol missing")?;
        if symbols
            .insert(symbol.to_owned(), TagHash::new(package, index as u16).0)
            .is_some()
        {
            return Err("Imported preview has duplicate symbols".into());
        }
    }
    let mut bound = crate::tiger::material::bind(graph, root)
        .map_err(|e| format!("Imported model bindings: {e:#}"))?;
    let models = crate::model_symbols(graph).map_err(|e| e.to_string())?;
    let entries = nodes
        .iter()
        .map(|node| {
            let model = node["symbol"]
                .as_str()
                .is_some_and(|name| models.contains(name));
            entry(node, model, root, &symbols, manager, &mut bound)
        })
        .collect::<Result<_, _>>()?;
    Ok((package, symbols, entries))
}

fn entry(
    node: &Value,
    model: bool,
    root: &Path,
    symbols: &Symbols,
    manager: &PackageManager,
    bound: &mut BTreeMap<String, Vec<u8>>,
) -> Result<Entry, String> {
    let template = word(&node["template"])?;
    let mut header = manager
        .get_entry(template)
        .ok_or_else(|| format!("Preview template 0x{template:08X} is missing"))?;
    // Loading companions contain package residency records, not model payloads. They are
    // unnecessary in a reader whose local data is already resident.
    let companion = node["symbol"] == "parent-companion" || node["shared_owner"].is_string();
    let mut data = if companion {
        Vec::new()
    } else if let Some(bytes) =
        bound.remove(node["symbol"].as_str().ok_or("Preview symbol missing")?)
    {
        bytes
    } else {
        let path = root
            .join(
                node["file"]
                    .as_str()
                    .ok_or("Imported preview payload missing")?,
            )
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !path.starts_with(root) {
            return Err("Imported preview payload escapes its folder".into());
        }
        fs::read(path).map_err(|e| e.to_string())?
    };
    if !companion {
        if model {
            crate::tiger::draws::declare_model_draw_indices(&mut data)
                .map_err(|e| e.to_string())?;
        }
        let mut patched = BTreeSet::new();
        for patch in node["patches"]
            .as_array()
            .ok_or("Imported preview fixups missing")?
        {
            let at = usize::try_from(word(&patch["offset"])?).map_err(|e| e.to_string())?;
            let target = data
                .get_mut(
                    at..at
                        .checked_add(4)
                        .ok_or("Imported preview fixup overflows")?,
                )
                .ok_or("Imported preview fixup is outside its payload")?;
            if target.iter().any(|byte| *byte != u8::MAX)
                || (at..at + 4).any(|offset| !patched.insert(offset))
            {
                return Err(
                    "Imported preview fixup overlaps or is not an unresolved reference".into(),
                );
            }
            target.copy_from_slice(&tag(symbols, &patch["symbol"])?.to_le_bytes());
        }
    }
    if node["reference"].is_string() {
        header.reference = tag(symbols, &node["reference"])?;
    }
    Ok((header, data))
}

fn scene(
    graph: &Value,
    item_kind: &str,
    symbols: &Symbols,
    manager: &PackageManager,
) -> Result<LocalScene, String> {
    // A weapon converted as art parts (D1 Rise of Iron) carries its gear art rows and is
    // read like any other arrangement below.
    if item_kind == "weapon" && graph.get("gear_art").is_none() {
        let parent = tag(symbols, &graph["parent"])?;
        if manager
            .get_entry(parent)
            .is_none_or(|e| e.reference != 0x8080744a)
        {
            return Err("Imported weapon parent has an unsupported type".into());
        }
        let entity = crate::tiger::payload::Payload(manager.read_tag(parent)?)
            .u32(0x10)
            .map_err(|e| e.to_string())?;
        // A weapon with no private dyes is painted in game by its native donor's default
        // dyes, which the gear path binds. The converted programs multiply by those vectors.
        let stock_dyes = if graph["dyes"].as_array().is_none_or(Vec::is_empty) {
            donor_default_dyes(manager, graph)?
        } else {
            Vec::new()
        };
        return Ok(LocalScene {
            entities: vec![entity],
            dyes: BTreeMap::new(),
            stock_dyes,
        });
    }
    let row = graph["gear_art"]["rows"]
        .as_array()
        .and_then(|rows| rows.first())
        .ok_or("Imported appearance has no art row")?;
    let slots = row["slots"]
        .as_array()
        .ok_or("Imported appearance selectors missing")?;
    let selected: Vec<u32> = if slots.is_empty() {
        row["singles"]
            .as_array()
            .filter(|values| values.len() == 2)
            .ok_or("Imported appearance assignments missing")?
            .iter()
            .map(word)
            .collect::<Result<_, _>>()?
    } else {
        let mut selected = Vec::new();
        for slot in slots {
            if let Some(first) = slot["assignments"]
                .as_array()
                .ok_or("Imported appearance alternatives missing")?
                .first()
            {
                selected.push(word(first)?);
            }
        }
        selected
    };
    let parts = graph["gear_art"]["parts"]
        .as_array()
        .ok_or("Imported appearance parts missing")?;
    let mut entities = Vec::new();
    for assignment in selected
        .into_iter()
        .filter(|v| ![0, u32::MAX, 0x811c9dc5].contains(v))
    {
        let part = parts
            .iter()
            .find(|p| p["source_assignment"].as_u64() == Some(u64::from(assignment)))
            .ok_or("Imported appearance assignment has no part")?;
        let parent = tag(symbols, &part["parent"])?;
        if manager
            .get_entry(parent)
            .is_none_or(|e| e.reference != 0x8080744a)
        {
            return Err("Imported appearance parent has an unsupported type".into());
        }
        entities.push(
            crate::tiger::payload::Payload(manager.read_tag(parent)?)
                .u32(0x10)
                .map_err(|e| e.to_string())?,
        );
    }
    let mut dyes = BTreeMap::new();
    if let Some(source_dyes) = graph["dyes"].as_array() {
        let layers = graph["dye_rows"]
            .as_array()
            .filter(|rows| rows.len() == 3)
            .ok_or("Imported appearance dye layers missing")?;
        for layer in [1, 0, 2] {
            for row in layers[layer]
                .as_array()
                .ok_or("Imported appearance dye layer invalid")?
            {
                let first = match item_kind {
                    "armor" => 0,
                    "ship" => 7,
                    "sparrow" => 10,
                    "ghost_shell" => 13,
                    _ => return Err("Imported appearance kind has no dye slots".into()),
                };
                let source = source_dyes
                    .get(word(&row["dye"])? as usize)
                    .ok_or("Imported preview dye missing")?;
                if row["channel"] != source["channel"] {
                    return Err("Imported preview dye channel differs".into());
                }
                let channel = word(&row["channel"])?;
                let slot = channel
                    .checked_sub(first)
                    .filter(|slot| *slot < 3)
                    .ok_or("Imported preview dye channel invalid")?;
                dyes.insert(slot as usize, tag(symbols, &source["parent"])?);
            }
        }
    }
    // A weapon's art parts are painted in game by its native donor's default dyes, as above.
    let stock_dyes = if item_kind == "weapon" && dyes.is_empty() {
        donor_default_dyes(manager, graph)?
    } else {
        Vec::new()
    };
    Ok(LocalScene {
        entities,
        dyes,
        stock_dyes,
    })
}

/// The native donor item's own default dyes, read from its definition the way the catalog
/// reads them, for a weapon whose graph records no private dyes.
fn donor_default_dyes(manager: &PackageManager, graph: &Value) -> Result<Vec<(i8, u16)>, String> {
    let donor = graph["native_item"]
        .as_u64()
        .and_then(|hash| u32::try_from(hash).ok())
        .ok_or("Imported weapon donor missing")?;
    let named = manager
        .lookup
        .named_tags
        .iter()
        .find(|entry| entry.name == "investment_globals")
        .map(|entry| entry.hash.0)
        .ok_or("Native investment globals missing")?;
    let payload = |tag: u32| -> Result<crate::tiger::payload::Payload, String> {
        Ok(crate::tiger::payload::Payload(manager.read_tag(tag)?))
    };
    let word = |payload: &crate::tiger::payload::Payload, at: usize| -> Result<u32, String> {
        payload.u32(at).map_err(|e| e.to_string())
    };
    let globals = payload(named)?;
    let root = payload(word(&globals, 16)?)?;
    let items = payload(word(&root, 8 + 48 * 16)?)?;
    let mut definition = None;
    for row in items
        .array(8, 24, Some(0x80807BE8))
        .map_err(|e| e.to_string())?
    {
        if word(&items, row)? == donor && definition.replace(word(&items, row + 16)?).is_some() {
            return Err("Native donor item is ambiguous".into());
        }
    }
    let item = manager.read_tag(definition.ok_or("Native donor item missing")?)?;
    Ok(sundial::ui::model_preview::stock_default_dyes(&item))
}

fn validate(reference: &GraphReference, kind: &str) -> Result<(), String> {
    if kind == "weapon" {
        let graph: Value = serde_json::from_slice(
            &fs::read(reference.directory.join("asset-graph.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if graph["kind"].as_str().is_some_and(|k| k != "weapon") {
            return Err("Choose a weapon appearance for weapon artwork".into());
        }
        reference
            .validate(word(&graph["item_hash"])?)
            .map_err(|e| format!("{e:#}"))
    } else {
        reference
            .validate_reusable(kind)
            .map_err(|e| format!("{e:#}"))
    }
}
