//! Pair source response groups with the carrier's by name, then by position between named anchors.
use super::*;

/// For each source group, its paired carrier group or none when only the source has it, and the
/// carrier's own groups left out.
pub(super) struct Groups {
    pub paired: Vec<Option<usize>>,
    pub dropped: Vec<usize>,
}

fn names(owner: &graph::Owner, old: bool) -> Result<Vec<u32>> {
    owner
        .payload
        .array(
            owner.definition + if old { 0x220 } else { 0x218 },
            if old { 96 } else { 88 },
            Some(if old { 0x8080239D } else { 0x808037CD }),
        )?
        .into_iter()
        .map(|group| owner.payload.u32(group + if old { 24 } else { 16 }))
        .collect()
}

/// Each group's node names in order.
fn nodes(owner: &graph::Owner, old: bool) -> Result<Vec<Vec<u32>>> {
    let p = &owner.payload;
    p.array(
        owner.definition + if old { 0x220 } else { 0x218 },
        if old { 96 } else { 88 },
        None,
    )?
    .into_iter()
    .map(|group| {
        p.array(
            group + if old { 0x50 } else { 0x48 },
            if old { 472 } else { 392 },
            None,
        )?
        .into_iter()
        .map(|node| p.u32(node + if old { 24 } else { 16 }))
        .collect()
    })
    .collect()
}

/// Names both trees share anchor the pairing, as the longest common ordered subsequence. Between
/// anchors, each source group pairs in order with the first carrier group left that it can pair
/// with, which renames the carrier group to the source's. Carrier groups it passes over are left
/// out. A remaining named carrier group is left out, as automatic rifle carrier `80BBC8AC` adds
/// `DBDC327C`. A source group with nothing to pair is inserted, as the fusion steering group
/// `E8197368`. `materials` says whether a source and carrier group's materials pair.
pub(super) fn groups(
    from: &graph::Owner,
    to: &graph::Owner,
    materials: &mut dyn FnMut(usize, usize) -> Result<bool>,
) -> Result<Groups> {
    let (source, native) = (names(from, true)?, names(to, false)?);
    let (source_nodes, native_nodes) = (nodes(from, true)?, nodes(to, false)?);
    let unnamed = 0x811C9DC5;
    let anchors = named_anchors(&source, &native, unnamed);
    let mut paired = vec![None; source.len()];
    let mut dropped = Vec::new();
    let (mut from_source, mut from_native) = (0, 0);
    for (anchor_source, anchor_native) in anchors
        .into_iter()
        .chain(std::iter::once((source.len(), native.len())))
    {
        // A left-out carrier group must be named, with a name the source lacks.
        let leave = |n: usize, dropped: &mut Vec<usize>| -> Result<()> {
            ensure!(
                native[n] != unnamed,
                "Unnamed projectile response group differs"
            );
            ensure!(
                !source.contains(&native[n]),
                "Named projectile response group differs"
            );
            dropped.push(n);
            Ok(())
        };
        let mut cursor = from_native;
        for s in from_source..anchor_source {
            let mut found = None;
            for n in cursor..anchor_native {
                // Groups pair only when their nodes can pair: equal counts, or a shared node
                // name, as fusion carrier `80BBB1B9` group `05E1632F` with one node `EA331663`
                // cannot stand for source group `717BE84D` with `7729ECCA` and `F8778E1D`. A
                // named pair needs names neither tree shares, and every pair needs materials that
                // pair, as Thunderlord's second unnamed group pairs with carrier `81529CCB` group
                // `0EC49298` rather than the groups before it.
                let compatible = source_nodes[s].len() == native_nodes[n].len()
                    || source_nodes[s]
                        .iter()
                        .any(|name| native_nodes[n].contains(name));
                let named = source[s] != unnamed
                    && !native.contains(&source[s])
                    && !source.contains(&native[n]);
                if compatible && (named || source[s] == unnamed) && materials(s, n)? {
                    found = Some(n);
                    break;
                }
            }
            if let Some(n) = found {
                for skipped in cursor..n {
                    leave(skipped, &mut dropped)?;
                }
                paired[s] = Some(n);
                cursor = n + 1;
            }
        }
        for n in cursor..anchor_native {
            leave(n, &mut dropped)?;
        }
        if anchor_source < source.len() {
            paired[anchor_source] = Some(anchor_native);
        }
        from_source = anchor_source + 1;
        from_native = anchor_native + 1;
    }
    recover_reordered_pairs(
        (&source, &native, unnamed),
        (&source_nodes, &native_nodes),
        &mut paired,
        &mut dropped,
        materials,
    )?;
    Ok(Groups { paired, dropped })
}

fn named_anchors(source: &[u32], native: &[u32], unnamed: u32) -> Vec<(usize, usize)> {
    // Longest common ordered subsequence of named groups.
    let mut table = vec![vec![0usize; native.len() + 1]; source.len() + 1];
    for i in (0..source.len()).rev() {
        for j in (0..native.len()).rev() {
            table[i][j] = if source[i] == native[j] && source[i] != unnamed {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let mut anchors = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < source.len() && j < native.len() {
        if source[i] == native[j] && source[i] != unnamed {
            anchors.push((i, j));
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    anchors
}

fn recover_reordered_pairs(
    (source, native, unnamed): (&[u32], &[u32], u32),
    (source_nodes, native_nodes): (&[Vec<u32>], &[Vec<u32>]),
    paired: &mut [Option<usize>],
    dropped: &mut Vec<usize>,
    materials: &mut dyn FnMut(usize, usize) -> Result<bool>,
) -> Result<()> {
    // Inserting a group rebuilds the groups in source order, so a source group still unpaired may
    // then take a left-out carrier group from another position it can pair with, as Thorn's
    // `98D81938` takes carrier `80BBC0FE` group `CFD673C2`, which follows `6E584E5D` natively.
    // At least one source group stays inserted, so the rebuild happens.
    let mut taken = Vec::new();
    for s in (0..source.len()).filter(|&s| paired[s].is_none()) {
        for (at, &n) in dropped.iter().enumerate() {
            if taken.iter().any(|(_, index)| *index == at) {
                continue;
            }
            // Away from its position a pair needs a shared node name, not only equal counts.
            let shared = source_nodes[s]
                .iter()
                .any(|name| native_nodes[n].contains(name));
            let names = source[s] == unnamed
                || (!native.contains(&source[s]) && !source.contains(&native[n]));
            if shared && names && materials(s, n)? {
                taken.push((s, at));
                break;
            }
        }
    }
    if taken.len() < paired.iter().filter(|p| p.is_none()).count() {
        for &(s, at) in &taken {
            paired[s] = Some(dropped[at]);
        }
        let used = taken.iter().map(|(_, at)| *at).collect::<BTreeSet<_>>();
        *dropped = dropped
            .iter()
            .copied()
            .enumerate()
            .filter(|(at, _)| !used.contains(at))
            .map(|(_, n)| n)
            .collect();
    }
    Ok(())
}
