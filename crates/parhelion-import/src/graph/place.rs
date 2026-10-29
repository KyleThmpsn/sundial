//! Where a conversion keeps its graphs. Every conversion folder, `<kind>-<n>`, keeps the item's
//! graph in `graph` and each ornament's in `ornaments/<HASH>/graph`, named by the ornament's
//! source item. Each importer's working data sits in other named folders beside them.
use super::*;

/// The folder of a conversion's own graph.
pub const GRAPH: &str = "graph";
/// The folder of a conversion's ornament graphs, one `<HASH>/graph` each.
pub const ORNAMENTS: &str = "ornaments";

/// Moves the graphs the prepared recipe at `recipe` names into their places in `conversion`, and
/// names them there. Returns each graph moved, from and to. A graph already in place stays.
pub fn settle(recipe: &Path, conversion: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut document: Value = serde_json::from_slice(&fs::read(recipe)?)?;
    let saved = &document["overrides"]["imported_graph"];
    if saved.is_null() {
        return Ok(Vec::new());
    }
    let reference: GraphReference =
        serde_json::from_value(saved.clone()).context("Prepared recipe imported graph")?;
    let owner = hash(&document["identity"], "item_hash")?;
    let (placed, moves) = place(&reference, owner, conversion)?;
    if moves.is_empty() {
        return Ok(moves);
    }
    document["overrides"]["imported_graph"] = serde_json::to_value(placed)?;
    if let Err(error) = crate::io::write_json(recipe, &document) {
        unplace(&moves)?;
        return Err(error);
    }
    Ok(moves)
}

/// Moves `reference`'s graphs, owned by the item `owner`, into their places in `conversion`, and
/// returns the reference that names them there with each graph moved, from and to. A graph's
/// content pin covers only its own files, so moving it changes no pin. Nothing moves when every
/// graph is in place, and a move that fails puts back the ones before it.
pub fn place(
    reference: &GraphReference,
    owner: u32,
    conversion: &Path,
) -> Result<(GraphReference, Vec<(PathBuf, PathBuf)>)> {
    let mut moves = Vec::new();
    let placed = place_all(reference, owner, conversion, &mut moves);
    match placed {
        Ok(placed) => Ok((placed, moves)),
        Err(error) => {
            unplace(&moves)?;
            Err(error)
        }
    }
}

fn place_all(
    reference: &GraphReference,
    owner: u32,
    conversion: &Path,
    moves: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<GraphReference> {
    let directory = place_one(&reference.directory, &conversion.join(GRAPH), moves)?;
    let mut attachments = Vec::with_capacity(reference.attachments.len());
    for attachment in &reference.attachments {
        let graph: Value =
            serde_json::from_slice(&fs::read(attachment.directory.join("asset-graph.json"))?)?;
        let source = [&graph["ornament"]["source_item"], &graph["source_item"]]
            .into_iter()
            .find_map(|value| u32::try_from(value.as_u64()?).ok())
            .context("Ornament graph names no source item")?;
        let to = conversion
            .join(ORNAMENTS)
            .join(format!("{source:08X}"))
            .join(GRAPH);
        attachments.push(place_one(&attachment.directory, &to, moves)?);
    }
    if moves.is_empty() {
        return Ok(reference.clone());
    }
    let mut placed = GraphReference::new(&directory, owner)?;
    placed.material_inputs = reference.material_inputs.clone();
    placed.attachments = attachments
        .iter()
        .map(|directory| GraphReference::new(directory, owner))
        .collect::<Result<_>>()?;
    Ok(placed)
}

/// Moves the graphs `moves` took back where they were, the last first.
pub fn unplace(moves: &[(PathBuf, PathBuf)]) -> Result<()> {
    for (from, to) in moves.iter().rev() {
        crate::io::move_folder(to, from)?;
    }
    Ok(())
}

/// Whether `reference`'s graphs are in their places in their conversion folder. A graph outside
/// any conversion folder, such as one made by hand, has no place to be in.
pub fn in_place(reference: &GraphReference) -> bool {
    let Some(conversion) = conversion_folder(&reference.directory) else {
        return true;
    };
    let ornaments = conversion.join(ORNAMENTS);
    reference.directory == conversion.join(GRAPH)
        && reference.attachments.iter().all(|attachment| {
            let folder = attachment.directory.parent();
            attachment.directory.file_name() == Some(GRAPH.as_ref())
                && folder.and_then(Path::parent) == Some(ornaments.as_path())
                && folder
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    .is_some_and(item_name)
        })
}

/// The conversion folder, `<imports>/<importer>/<HASH>/<kind>-<n>`, that holds `graph`.
pub fn conversion_folder(graph: &Path) -> Option<PathBuf> {
    graph
        .ancestors()
        .find(|folder| {
            let item = folder.parent();
            let imports = item.and_then(Path::parent).and_then(Path::parent);
            imports
                .and_then(Path::file_name)
                .is_some_and(|name| name.eq_ignore_ascii_case(crate::d2_mot::service::IMPORTS))
                && item
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    .is_some_and(item_name)
                && folder
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.rsplit_once('-'))
                    .is_some_and(|(kind, index)| {
                        !kind.is_empty()
                            && !index.is_empty()
                            && index.bytes().all(|c| c.is_ascii_digit())
                    })
        })
        .map(Path::to_owned)
}

/// Whether `name` is an item folder's or ornament folder's name, eight uppercase hex digits.
fn item_name(name: &str) -> bool {
    name.len() == 8
        && name
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c))
}

/// Moves the graph folder `from` to `to` unless it is there already, and returns where it is.
fn place_one(from: &Path, to: &Path, moves: &mut Vec<(PathBuf, PathBuf)>) -> Result<PathBuf> {
    let from = from
        .canonicalize()
        .with_context(|| format!("Open {}", from.display()))?;
    if to.exists() {
        ensure!(
            to.canonicalize()? == from,
            "{} already holds another graph",
            to.display()
        );
        return Ok(from);
    }
    let parent = to.parent().context("Graph folder parent")?;
    fs::create_dir_all(parent)?;
    ensure!(
        !parent.canonicalize()?.starts_with(&from),
        "{} cannot move into itself",
        from.display()
    );
    crate::io::move_folder(&from, to)?;
    let to = to.canonicalize()?;
    moves.push((from, to.clone()));
    Ok(to)
}
