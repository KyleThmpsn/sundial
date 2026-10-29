//! The inspect screen's scene for an imported weapon whose equipment skeleton has grown.
//!
//! The inspect screen shows an item in the crosshair-table scene its strings name by bucket and
//! first type key. The row's style row begins with the key of the scene object that shows the
//! weapon. Each object names an anchor piece and an animated piece. The weapon object's anchor
//! binds the equipment skeleton and markers of one weapon family, and its controls either there or
//! on the animated piece, as controls of the scene's own with the same bones and slots. Other
//! objects carry crosshair elements or a character body. The screen draws the gear model on the
//! weapon piece, and creates no display object when the model's geometry palette has more bones
//! than that piece's skeleton. An import that appends bones to its equipment skeleton has such a
//! palette while every stock piece keeps a stock skeleton, so its inspect screen stays empty.
//!
//! Such a weapon gets a private copy of a stock scene whose weapon object anchors a piece of its
//! own family: the item's scene when it has one, else the first such scene of the item's bucket,
//! then of any bucket. The weapon object's pieces bind copies of the converted equipment rigs,
//! moved as the runtime entity's are. Scene controls are extended as the converted controls
//! are, so every bone, appended or renumbered, takes its rest pose from them. Copies of the object
//! list, scene root and pair name the pieces. The pair loads the scene through a companion cloned
//! from the native pair's. A row of the item's own names the pair under the item's bucket and a
//! private key, with the item's style row naming the copied weapon object, and only the strings'
//! first type key moves to it. The hip-fire crosshair follows the runtime content's type key, as
//! do the first-person parameters that key names, so neither changes.
use super::*;
use crate::weapon::crosshair::{Row, TABLE, Table};
use parhelion_import::tiger::{animation::rig, payload::Payload};
use sundial::package_authoring::entity::{
    retarget_weapon_component_owner, retarget_weapon_component_owner_payload,
};

/// An entity's component rows, each led by its owner tag.
const COMPONENTS: usize = 0x10;
const COMPONENT_ROW: u32 = 0x8080_9C04;
const COMPONENT_ROW_SIZE: usize = 12;
const CONTROLS: u32 = 0x8080_8F8F;
const OBJECT_LIST: u32 = 0x8080_6647;
/// The object list's objects, from its definition, with the anchor piece at +0x30 and the
/// animated piece at +0x38.
const OBJECTS: usize = 0x218;
const OBJECT: u32 = 0x8080_664F;
const OBJECT_SIZE: usize = 0x40;
/// Words standing in for the private rig copies until the linker assigns their tags. A payload
/// that already holds one cannot take them.
const STAND_INS: std::ops::Range<u32> = 0x8FFF_FF00..0x8FFF_FF10;
const PAIR: &str = "inspect-pair";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Skeleton,
    Controls,
    Markers,
}

/// A converted equipment rig of the weapon: the stock owner and its payload, the converted
/// payload and how the conversion moved the owner's interior.
struct Rig {
    native: u32,
    kind: Kind,
    original: Payload,
    converted: Vec<u8>,
    relocation: Option<(usize, usize)>,
}

/// The equipment rigs the weapon's runtime entity binds in place of its stock ones. They apply
/// only with linked first-person animation, as on the runtime entity.
fn rigs(graph: &Value, folder: &Path) -> AuthoringResult<Vec<Rig>> {
    let animation = &graph["animation"];
    if animation["first_person_status"] != "linked" {
        return Ok(Vec::new());
    }
    let file = |value: &Value| -> AuthoringResult<Vec<u8>> {
        let name = value
            .as_str()
            .ok_or_else(|| invalid("Imported rig file is missing"))?;
        fs::read(folder.join(name)).map_err(|e| invalid(format!("Imported rig {name}: {e}")))
    };
    let mut rigs = Vec::new();
    for value in animation["first_person"]["rigs"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if value["first_person"] != false {
            continue;
        }
        let kind = match value["kind"].as_str() {
            None | Some("skeleton") => Kind::Skeleton,
            Some("controls") => Kind::Controls,
            Some("markers") => Kind::Markers,
            Some(_) => return Err(invalid("Imported rig owner kind is unsupported")),
        };
        let relocation = &value["relocation"];
        let relocation = if relocation.is_null() {
            None
        } else {
            let field = |key: &str| {
                relocation[key]
                    .as_u64()
                    .and_then(|v| usize::try_from(v).ok())
                    .ok_or_else(|| invalid("Imported rig relocation is incomplete"))
            };
            Some((field("cut")?, field("delta")?))
        };
        rigs.push(Rig {
            native: value["native"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| invalid("Imported rig owner is missing"))?,
            kind,
            original: Payload(file(&value["template_file"])?),
            converted: file(&value["file"])?,
            relocation,
        });
    }
    Ok(rigs)
}

fn read(manager: &PackageManager, tag: u32) -> AuthoringResult<Vec<u8>> {
    manager
        .read_tag(TagHash(tag))
        .map_err(|e| invalid(format!("Inspect scene asset {tag:08X}: {e}")))
}

fn checked<T>(result: anyhow::Result<T>, what: &str) -> AuthoringResult<T> {
    result.map_err(|e| invalid(format!("Inspect scene {what}: {e:#}")))
}

/// The owners an entity binds, in component order.
fn components(entity: &[u8]) -> AuthoringResult<Vec<u32>> {
    let entity = Payload(entity.to_vec());
    checked(
        entity
            .array(COMPONENTS, COMPONENT_ROW_SIZE, Some(COMPONENT_ROW))
            .and_then(|rows| {
                rows.into_iter()
                    .map(|row| entity.u32(row))
                    .collect::<anyhow::Result<Vec<_>>>()
            }),
        "components",
    )
}

/// The definition class of an owner payload.
fn class(owner: &Payload) -> anyhow::Result<u32> {
    let base = owner.pointer(24)?;
    anyhow::ensure!(base >= 4, "owner has no definition");
    owner.u32(base - 4)
}

/// A stock crosshair scene: its pair, root entity and object list, each object's key, anchor and
/// animated piece, and the pieces present among them.
struct Scene {
    pair: u32,
    root: u32,
    list: u32,
    objects: Vec<(u32, u32, u32)>,
    pieces: Vec<u32>,
}

fn scene(manager: &PackageManager, pair: u32) -> AuthoringResult<Scene> {
    let record = read(manager, pair)?;
    if record.len() != 8 {
        return Err(invalid("Inspect scene pair layout differs"));
    }
    let root = read_u32(&record, 0)?;
    let mut lists = Vec::new();
    for owner in components(&read(manager, root)?)? {
        let payload = Payload(read(manager, owner)?);
        if checked(class(&payload), "component")? == OBJECT_LIST {
            lists.push((owner, payload));
        }
    }
    let [(owner, list)] = lists.as_slice() else {
        return Err(invalid("Inspect scene has no single object list"));
    };
    let base = checked(list.pointer(24), "object list")?;
    let mut objects = Vec::new();
    let mut pieces = Vec::new();
    for object in checked(
        list.array(base + OBJECTS, OBJECT_SIZE, Some(OBJECT)),
        "objects",
    )? {
        let field = |at| checked(list.u32(object + at), "object");
        let (key, anchor, animated) = (field(0x28)?, field(0x30)?, field(0x38)?);
        objects.push((key, anchor, animated));
        for piece in [anchor, animated] {
            if manager.get_entry(TagHash(piece)).is_some() && !pieces.contains(&piece) {
                pieces.push(piece);
            }
        }
    }
    Ok(Scene {
        pair,
        root,
        list: *owner,
        objects,
        pieces,
    })
}

/// The weapon object a style row names, as its anchor and animated piece, when its anchor binds
/// `skeleton`.
fn weapon_object(
    manager: &PackageManager,
    scene: &Scene,
    shown: u32,
    skeleton: u32,
) -> AuthoringResult<Option<[u32; 2]>> {
    let Some(&(_, anchor, animated)) = scene.objects.iter().find(|object| object.0 == shown) else {
        return Ok(None);
    };
    if !scene.pieces.contains(&anchor) || !components(&read(manager, anchor)?)?.contains(&skeleton)
    {
        return Ok(None);
    }
    Ok(Some([anchor, animated]))
}

fn holds_stand_in(data: &[u8]) -> bool {
    data.chunks_exact(4)
        .any(|word| STAND_INS.contains(&u32::from_le_bytes(word.try_into().unwrap())))
}

/// A private copy before linking: the words naming another copy, a stock tag it replaces or a
/// stand-in, become the linker's placeholders.
fn node(
    symbol: &str,
    template: u32,
    mut payload: Vec<u8>,
    names: &BTreeMap<u32, String>,
) -> AuthoringResult<linking::Node> {
    let mut patches = Vec::new();
    for at in (0..payload.len().saturating_sub(3)).step_by(4) {
        if let Some(target) = names.get(&read_u32(&payload, at)?) {
            write_u32(&mut payload, at, u32::MAX)?;
            patches.push((at, target.clone()));
        }
    }
    let mut node = linking::Node::new(symbol, template, payload);
    node.patches = patches;
    Ok(node)
}

/// Private copies of scene pieces and the owners they bind, before linking, with the stock tags
/// and stand-ins whose words name them.
struct Copies {
    drafts: Vec<(String, u32, Vec<u8>)>,
    names: BTreeMap<u32, String>,
}

/// Copies of the weapon object's pieces and of any other piece binding a converted rig. Each binds
/// the converted rigs and extended copies of its scene controls in place of the stock owners. A
/// piece that binds neither stays shared.
fn copies(
    manager: &PackageManager,
    scene: &Scene,
    object: [u32; 2],
    rigs: &[Rig],
    renumbering: &rig::Renumbering,
) -> AuthoringResult<Copies> {
    let natives = rigs.iter().map(|rig| rig.native).collect::<Vec<_>>();
    let mut stand_ins = STAND_INS;
    let mut pieces = 0;
    let mut copies = Copies {
        drafts: Vec::new(),
        names: BTreeMap::new(),
    };
    for &piece in &scene.pieces {
        let mut entity = read(manager, piece)?;
        let bound = components(&entity)?;
        if !object.contains(&piece) && !bound.iter().any(|owner| natives.contains(owner)) {
            continue;
        }
        if holds_stand_in(&entity) {
            return Err(invalid("Inspect scene piece holds a private rig stand-in"));
        }
        let mut owners = Vec::new();
        for converted in rigs.iter().filter(|rig| bound.contains(&rig.native)) {
            if holds_stand_in(&converted.converted) {
                return Err(invalid("Converted rig holds a private rig stand-in"));
            }
            // The same order of moves as the runtime entity's: rebase, owner, entity, typed
            // handles.
            if let Some((cut, delta)) = converted.relocation {
                checked(
                    rig::rebase_entity(
                        &mut entity,
                        &converted.original,
                        converted.native,
                        cut,
                        delta,
                    ),
                    "rig relocation",
                )?;
            }
            owners.push((converted.native, converted.converted.clone()));
        }
        for &owner in bound.iter().filter(|owner| !natives.contains(owner)) {
            let stock = Payload(read(manager, owner)?);
            if checked(class(&stock), "component")? != CONTROLS {
                continue;
            }
            let mut converted = rigs.iter().filter(|rig| rig.kind == Kind::Controls);
            let (Some(controls), None) = (converted.next(), converted.next()) else {
                return Err(invalid(
                    "Inspect scene controls have no single converted counterpart",
                ));
            };
            let extended = checked(
                rig::extend_controls_like(
                    &stock,
                    &controls.original,
                    &Payload(controls.converted.clone()),
                    renumbering,
                ),
                "controls",
            )?;
            if holds_stand_in(&extended.0) {
                return Err(invalid(
                    "Inspect scene controls hold a private rig stand-in",
                ));
            }
            owners.push((owner, extended.0));
        }
        if owners.is_empty() {
            continue;
        }
        let name = format!("inspect-piece-{pieces}");
        pieces += 1;
        copies.names.insert(piece, name.clone());
        for (native, mut owner) in owners {
            let stand_in = stand_ins
                .next()
                .ok_or_else(|| invalid("Inspect scene binds too many converted rigs"))?;
            retarget_weapon_component_owner_payload(&mut owner, &entity, native, stand_in)
                .map_err(|e| invalid(format!("Inspect scene rig: {e}")))?;
            retarget_weapon_component_owner(&mut entity, native, stand_in)
                .map_err(|e| invalid(format!("Inspect scene piece: {e}")))?;
            checked(
                rig::retarget_rebased_references(&mut entity, owner.len(), native, stand_in),
                "rig references",
            )?;
            let symbol = format!("{name}-rig-{}", copies.drafts.len());
            copies.names.insert(stand_in, symbol.clone());
            copies.drafts.push((symbol, native, owner));
        }
        copies.drafts.push((name, piece, entity));
    }
    Ok(copies)
}

/// The private scene's assets: the piece copies, copies of the object list, root and pair, and
/// the pair's loading companion.
fn nodes(
    (directory, manager): (&Path, &PackageManager),
    source: &Scene,
    copies: Copies,
    companions: &mut linking::Companions,
) -> AuthoringResult<Vec<linking::Node>> {
    let Copies {
        mut drafts,
        mut names,
    } = copies;
    names.extend([
        (source.pair, PAIR.to_owned()),
        (source.root, "inspect-root".to_owned()),
        (source.list, "inspect-objects".to_owned()),
    ]);
    for (symbol, tag) in [
        ("inspect-objects", source.list),
        ("inspect-root", source.root),
        (PAIR, source.pair),
    ] {
        let payload = read(manager, tag)?;
        if holds_stand_in(&payload) {
            return Err(invalid("Inspect scene asset holds a private rig stand-in"));
        }
        drafts.push((symbol.to_owned(), tag, payload));
    }
    let mut nodes = drafts
        .into_iter()
        .map(|(symbol, template, payload)| node(&symbol, template, payload, &names))
        .collect::<AuthoringResult<Vec<_>>>()?;
    let template = linking::native_companion(directory, manager, source.pair, companions)?.0;
    let mut companion = linking::Node::new(format!("{PAIR}-companion"), template.0, Vec::new());
    companion.companion = Some(linking::Companion::new(PAIR, source.pair));
    nodes.push(companion);
    Ok(nodes)
}

/// FNV-1 over the bytes of `name`.
fn fnv1(name: &str) -> u32 {
    name.bytes().fold(0x811C_9DC5u32, |hash, byte| {
        hash.wrapping_mul(16_777_619) ^ u32::from(byte)
    })
}

/// Give the item a private inspect scene when its converted equipment skeleton has more bones
/// than the stock one. `table` is the crosshair table as earlier steps left it. Returns that table
/// with the item's row added.
pub(super) fn apply(
    (directory, manager): (&Path, &PackageManager),
    emission: &mut PackageEmission,
    (graph, folder): (&Value, &Path),
    table: Option<&[u8]>,
    item: u32,
    companions: &mut linking::Companions,
) -> AuthoringResult<Option<ReplacementSpec>> {
    let rigs = rigs(graph, folder)?;
    let Some(skeleton) = rigs.iter().find(|rig| rig.kind == Kind::Skeleton) else {
        return Ok(None);
    };
    let grown = checked(
        rig::bone_count(&Payload(skeleton.converted.clone())),
        "converted skeleton",
    )? > checked(rig::bone_count(&skeleton.original), "stock skeleton")?;
    if !grown {
        return Ok(None);
    }
    let ordinal = strings_ordinal(emission, item)?;
    let (bucket, first, second) = item_string_type_keys(&emission.host_new_tags[ordinal].payload)?;
    // Stock rows only: a row an import added this build names a pair that is not native yet.
    let stock = Table::parse(&read(manager, TABLE.0)?)?;
    let Some(current) = stock.row(bucket, first).cloned() else {
        return Ok(None);
    };
    // Weapon rows carry one style row, whose key names the scene object that shows the weapon.
    let [style] = current.subs.as_slice() else {
        return Ok(None);
    };
    let renumbering = checked(
        rig::renumbering(&skeleton.original, &Payload(skeleton.converted.clone())),
        "skeleton renumbering",
    )?;
    // The item's scene, then the scenes of its bucket and the rest. A stock scene of another
    // layout, or whose weapon object cannot take the converted rigs, is no candidate.
    let rows = stock.rows();
    let mut seen = BTreeSet::new();
    let mut source = None;
    for row in std::iter::once(&current)
        .chain(rows.iter().filter(|row| row.bucket == bucket))
        .chain(rows.iter().filter(|row| row.bucket != bucket))
    {
        let [sub] = row.subs.as_slice() else {
            continue;
        };
        let shown = read_u32(sub, 0)?;
        if !seen.insert((row.pair, shown)) {
            continue;
        }
        let Ok(candidate) = scene(manager, row.pair) else {
            continue;
        };
        let Some(object) = weapon_object(manager, &candidate, shown, skeleton.native)? else {
            continue;
        };
        match copies(manager, &candidate, object, &rigs, &renumbering) {
            Ok(copies) => {
                source = Some((candidate, shown, copies));
                break;
            }
            Err(error) => eprintln!(
                "Inspect scene {:08X} cannot take the converted rigs: {error}",
                row.pair
            ),
        }
    }
    let Some((source, shown, copies)) = source else {
        eprintln!(
            "No crosshair scene shows a piece with skeleton {:08X}. The inspect screen keeps scene {:08X}",
            skeleton.native, current.pair
        );
        return Ok(None);
    };
    let nodes = nodes((directory, manager), &source, copies, companions)?;
    let linked = linking::link(
        directory,
        emission,
        manager,
        nodes,
        PAIR,
        companions,
        |_, _| Ok(()),
        None,
    )?;
    let pair = linked.symbols[PAIR];
    let mut table = match table {
        Some(data) => Table::parse(data)?,
        None => stock,
    };
    let (hash, key) = (0u32..)
        .map(|salt| {
            (
                fnv1(&format!("inspect_row_{item:08x}_{salt}")),
                fnv1(&format!("inspect_{item:08x}_{salt}")),
            )
        })
        .find(|&(hash, key)| {
            ![0, u32::MAX, 0x811C_9DC5].contains(&key)
                && !table
                    .rows()
                    .iter()
                    .any(|row| row.hash == hash || (row.bucket == bucket && row.key == key))
        })
        .ok_or_else(|| invalid("Inspect scene row has no free key"))?;
    // The rest of the style row follows the bucket, so only its key changes.
    let mut style = *style;
    style[..4].copy_from_slice(&shown.to_le_bytes());
    table.insert(Row {
        hash,
        bucket,
        key,
        pair: pair.0,
        subs: vec![style],
    })?;
    set_item_string_type_keys(&mut emission.host_new_tags[ordinal].payload, key, second)?;
    Ok(Some(ReplacementSpec {
        tag: TABLE,
        payload: table.serialize()?,
    }))
}
