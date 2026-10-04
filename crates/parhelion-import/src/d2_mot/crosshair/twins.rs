//! Twin crosshair rows, which tell what a modern crosshair asset is in Shadowkeep.
//!
//! A crosshair entity (object type 28) has a root controller whose owner names the piece entities.
//! Modern roots are one owner (class 80805FB7). Native roots split it into a flow graph (808084E9)
//! and an object list (80806647) that names the pieces. Modern scenes also carry object type 17
//! pieces that native scenes lack. Without those, twin scenes list the same pieces in the same
//! order, and within a piece the owners pair by the component classes below. Every aligned pair
//! is a vote, and only assets whose votes all agree are mapped.
use super::*;
use std::collections::BTreeSet;

pub(super) const MODERN_ENTITY: u32 = 0x8080_9AD8;
pub(super) const NATIVE_ENTITY: u32 = 0x8080_9C0F;
pub(super) const MODERN_ROOT: u32 = 0x8080_5FB7;
pub(super) const NATIVE_FLOW: u32 = 0x8080_84E9;
pub(super) const NATIVE_OBJECTS: u32 = 0x8080_6647;
/// Pieces of this modern object type have no native counterpart in any twin scene.
const MODERN_ONLY_PIECE: u16 = 17;

/// Component classes with the same role in both versions, modern then native.
pub(super) const CLASSES: [(u32, u32); 13] = [
    (0x8080_8B5F, 0x8080_8F8F), // rig controls
    (0x8080_25F8, 0x8080_344B), // animation lookup
    (0x8080_289B, 0x8080_36CF), // animation bank consumer
    (0x8080_2831, 0x8080_366C), // rig
    (0x8080_6A58, 0x8080_6F4F), // channels
    (0x8080_9597, 0x8080_9790), // effects controller
    (0x8080_81DE, 0x8080_8546), // skeleton
    (0x8080_819D, 0x8080_8507), // markers
    (0x8080_9433, 0x8080_9707),
    (0x8080_8646, 0x8080_8A0C),
    (0x8080_3285, 0x8080_3F47),
    (0x8080_6CDF, 0x8080_71C9),
    (0x8080_3F64, 0x8080_4AF6), // labels
];

pub(super) fn native_class(modern: u32) -> Option<u32> {
    CLASSES.iter().find(|(m, _)| *m == modern).map(|(_, n)| *n)
}

#[derive(Clone, Debug)]
pub(super) struct Piece {
    pub tag: u32,
    pub object: u16,
    pub owners: Vec<(u32, u32)>,
}

/// A twin's roots, which a source root with the same flow graph template can borrow.
pub(super) struct Family {
    pub modern_root: u32,
    pub modern_names: Vec<u32>,
    pub native_entity: u32,
    pub native_flow: u32,
    pub native_objects: u32,
    pub native_pair: u32,
}

pub(super) struct Twins {
    pub map: BTreeMap<u32, u32>,
    pub pieces: Vec<(Piece, Piece)>,
    pub families: Vec<Family>,
    /// The native pair whose scene holds each native root, piece and owner. The pair's loading
    /// index loads that asset, so a scene that borrows it must load what the pair loads.
    pub pairs: BTreeMap<u32, u32>,
}

pub(super) fn owner_class(r: &mut Reader, owner: u32) -> Result<u32> {
    let p = r.tag(owner, None)?;
    let resource = p.pointer(24)?;
    ensure!(resource >= 4, "crosshair owner {owner:08X} has no resource");
    p.u32(resource - 4)
}

pub(super) fn piece(r: &mut Reader, tag: u32, modern: bool) -> Result<Piece> {
    let (class, components, object) = if modern {
        (MODERN_ENTITY, 8, 0x92)
    } else {
        (NATIVE_ENTITY, 16, 0x96)
    };
    let entity = r.tag(tag, Some(class))?;
    let mut owners = Vec::new();
    for row in entity.array(components, 12, None)? {
        let owner = entity.u32(row)?;
        owners.push((owner, owner_class(r, owner)?));
    }
    Ok(Piece {
        tag,
        object: entity.u16(object)?,
        owners,
    })
}

/// Entities an owner names, in offset order: 64-bit references in modern owners, tags in native.
fn named(r: &mut Reader, owner: u32, modern: bool) -> Result<Vec<u32>> {
    let class = if modern { MODERN_ENTITY } else { NATIVE_ENTITY };
    let p = r.tag(owner, None)?;
    let mut found = Vec::new();
    for at in (0..p.0.len().saturating_sub(8)).step_by(if modern { 8 } else { 4 }) {
        let target = if modern {
            match r.ref64(&p, at) {
                Ok(target) => target,
                Err(_) => continue,
            }
        } else {
            p.u32(at)?
        };
        if target != owner && target >> 24 == 0x80 && r.reference(target).ok() == Some(class) {
            found.push(target);
        }
    }
    Ok(found)
}

pub(super) struct Scene {
    pub root: Piece,
    pub pieces: Vec<Piece>,
}

pub(super) fn scene(r: &mut Reader, tag: u32, modern: bool) -> Result<Scene> {
    let root = piece(r, tag, modern)?;
    let controller = if modern { MODERN_ROOT } else { NATIVE_OBJECTS };
    let owners = root
        .owners
        .iter()
        .filter(|(_, class)| *class == controller)
        .collect::<Vec<_>>();
    let [(owner, _)] = owners.as_slice() else {
        anyhow::bail!("crosshair {tag:08X} has no unique piece list");
    };
    let mut pieces = Vec::new();
    for child in named(r, *owner, modern)? {
        if child == tag {
            continue;
        }
        let p = piece(r, child, modern)?;
        if modern && p.object == MODERN_ONLY_PIECE {
            continue;
        }
        pieces.push(p);
    }
    Ok(Scene { root, pieces })
}

/// The object names a modern root lists, in object order.
pub(super) fn modern_names(r: &mut Reader, root: u32) -> Result<Vec<u32>> {
    let p = r.tag(root, None)?;
    let resource = p.pointer(24)?;
    ensure!(
        p.u32(resource - 4)? == MODERN_ROOT,
        "crosshair root layout differs"
    );
    p.array(
        resource + super::scene::MODERN_OBJECTS,
        super::scene::MODERN_OBJECT_STRIDE,
        Some(super::scene::MODERN_OBJECT),
    )?
    .into_iter()
    .map(|row| p.u32(row + 0x40))
    .collect()
}

pub(super) fn discover(
    m: &mut Reader,
    n: &mut Reader,
    modern_rows: &[Row],
    native_rows: &[Row],
) -> Result<Twins> {
    let mut votes: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    let mut pieces = Vec::new();
    let mut families = Vec::new();
    let mut pairs = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for row in modern_rows {
        let Some(native) = native_rows
            .iter()
            .find(|native| native.bucket == row.bucket && native.key == row.key)
        else {
            continue;
        };
        let mp = m.tag(row.pair, None)?;
        let modern_root = m.ref64(&mp, 0)?;
        let np = n.tag(native.pair, None)?;
        let native_root = np.u32(0)?;
        if !seen.insert((modern_root, native_root)) {
            continue;
        }
        let ms = scene(m, modern_root, true)?;
        let ns = scene(n, native_root, false)?;
        for piece in std::iter::once(&ns.root).chain(&ns.pieces) {
            for tag in
                std::iter::once(piece.tag).chain(piece.owners.iter().map(|(owner, _)| *owner))
            {
                pairs.entry(tag).or_insert(native.pair);
            }
        }
        let flow = ns
            .root
            .owners
            .iter()
            .filter(|(_, c)| *c == NATIVE_FLOW)
            .collect::<Vec<_>>();
        let objects = ns
            .root
            .owners
            .iter()
            .filter(|(_, c)| *c == NATIVE_OBJECTS)
            .collect::<Vec<_>>();
        let roots = ms
            .root
            .owners
            .iter()
            .filter(|(_, c)| *c == MODERN_ROOT)
            .collect::<Vec<_>>();
        if let ([(flow, _)], [(objects, _)], [(modern_owner, _)]) =
            (flow.as_slice(), objects.as_slice(), roots.as_slice())
        {
            families.push(Family {
                modern_root: *modern_owner,
                modern_names: modern_names(m, *modern_owner)?,
                native_entity: native_root,
                native_flow: *flow,
                native_objects: *objects,
                native_pair: native.pair,
            });
        }
        let aligned = ms.pieces.len() == ns.pieces.len()
            && ms
                .pieces
                .iter()
                .zip(&ns.pieces)
                .all(|(a, b)| a.object == b.object);
        if !aligned {
            continue;
        }
        votes.entry(ms.root.tag).or_default().insert(ns.root.tag);
        for (a, b) in ms.pieces.iter().zip(&ns.pieces) {
            votes.entry(a.tag).or_default().insert(b.tag);
            for &(owner, class) in &a.owners {
                let Some(target) = native_class(class) else {
                    continue;
                };
                let matches = b
                    .owners
                    .iter()
                    .filter(|(_, c)| *c == target)
                    .collect::<Vec<_>>();
                if let [(native_owner, _)] = matches.as_slice() {
                    votes.entry(owner).or_default().insert(*native_owner);
                }
            }
            pieces.push((a.clone(), b.clone()));
        }
    }
    ensure!(!pieces.is_empty(), "no twin crosshair scenes align");
    let map = votes
        .into_iter()
        .filter_map(|(modern, natives)| {
            (natives.len() == 1).then(|| (modern, *natives.iter().next().unwrap()))
        })
        .collect();
    Ok(Twins {
        map,
        pieces,
        families,
        pairs,
    })
}
