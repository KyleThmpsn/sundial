//! Reusable source assets shared by gear editing, saved recipes and native emission.
use crate::ItemKind;
pub(crate) mod archive;
pub(crate) mod artwork;
pub(crate) mod preview;

pub(crate) fn source_icon(
    recipe: &crate::WeaponRecipe,
) -> Result<crate::icon_edit::ImportedIcon, String> {
    let graph = recipe
        .overrides
        .imported_graph
        .as_ref()
        .ok_or("No imported source selected")?;
    let document: serde_json::Value = serde_json::from_slice(
        &std::fs::read(graph.directory.join("asset-graph.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if recipe.kind.is_weapon() {
        let item = document["item_hash"]
            .as_u64()
            .and_then(|hash| u32::try_from(hash).ok())
            .ok_or("Imported weapon identity missing")?;
        graph.validate(item).map_err(|e| format!("{e:#}"))?;
    } else {
        let family = kind(recipe.kind).ok_or("Unsupported source item kind")?;
        graph
            .validate_reusable(family)
            .map_err(|e| format!("{e:#}"))?;
    }
    let path = match document["source_icon_png"].as_str() {
        Some(file) => {
            let root = graph.directory.canonicalize().map_err(|e| e.to_string())?;
            let path = root.join(file).canonicalize().map_err(|e| e.to_string())?;
            if !path.starts_with(&root) {
                return Err("Source icon escapes its asset folder".into());
            }
            path
        }
        None if recipe.kind == ItemKind::Shader => graph
            .directory
            .parent()
            .ok_or("Source directory missing")?
            .join("source/item-icon.png"),
        None => return Err("Source icon missing".into()),
    };
    crate::icon_edit::ImportedIcon::from_bytes(&std::fs::read(path).map_err(|e| e.to_string())?)
}

/// What a linked ornament's graph names: its private plug's item hash and the icon it shows.
type Linked = Option<(u32, crate::icon_edit::ImportedIcon)>;

/// Every graph read for its linked ornament, by folder and pin. A pin names its content, so each
/// graph is read once per session.
static LINKED: std::sync::Mutex<std::collections::BTreeMap<(std::path::PathBuf, String), Linked>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// The icon each linked ornament among `attachments` shows in game, by its private plug's item
/// hash. The importer keeps it in the ornament's conversion, not in the weapon's variant. Graphs
/// not read before are read on several threads.
pub(crate) fn ornament_icons<'a>(
    attachments: impl IntoIterator<Item = &'a parhelion_import::GraphReference>,
) -> std::collections::BTreeMap<u32, crate::icon_edit::ImportedIcon> {
    let keys = attachments
        .into_iter()
        .map(|graph| (graph.directory.clone(), graph.sha256.clone()))
        .collect::<std::collections::BTreeSet<_>>();
    let unread = {
        let read = LINKED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        keys.iter()
            .filter(|key| !read.contains_key(*key))
            .cloned()
            .collect::<Vec<_>>()
    };
    if !unread.is_empty() {
        let threads = std::thread::available_parallelism()
            .map_or(1, |count| count.get().min(8))
            .min(unread.len());
        let found = std::thread::scope(|scope| {
            unread
                .chunks(unread.len().div_ceil(threads))
                .map(|chunk| {
                    scope.spawn(move || {
                        chunk
                            .iter()
                            .map(|key| (key.clone(), linked(&key.0)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .flat_map(|worker| worker.join().unwrap_or_default())
                .collect::<Vec<_>>()
        });
        LINKED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(found);
    }
    let read = LINKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    keys.iter()
        .filter_map(|key| read.get(key).cloned().flatten())
        .collect()
}

/// The private plug and icon the graph in `directory` names for its ornament link. An icon that
/// cannot be read leaves the ornament without one.
fn linked(directory: &std::path::Path) -> Linked {
    #[derive(serde::Deserialize)]
    struct Graph {
        item_hash: Option<u32>,
        ornament: Option<serde::de::IgnoredAny>,
        ornament_icon_png: Option<std::path::PathBuf>,
    }
    let graph: Graph =
        serde_json::from_slice(&std::fs::read(directory.join("asset-graph.json")).ok()?).ok()?;
    graph.ornament?;
    let icon = graph.ornament_icon_png?;
    let path = if icon.is_absolute() {
        icon
    } else {
        directory.join(icon)
    };
    let image = crate::icon_edit::ImportedIcon::from_bytes(&std::fs::read(path).ok()?).ok()?;
    Some((graph.item_hash?, image))
}

pub(crate) const fn kind(kind: ItemKind) -> Option<&'static str> {
    match kind {
        ItemKind::Shader => Some("shader"),
        ItemKind::Armor => Some("armor"),
        ItemKind::GhostShell => Some("ghost_shell"),
        ItemKind::Ship => Some("ship"),
        ItemKind::Sparrow => Some("sparrow"),
        // A mod built on a universal ornament takes that ornament's imported appearance.
        ItemKind::Mod => Some("ornament"),
        _ => None,
    }
}
