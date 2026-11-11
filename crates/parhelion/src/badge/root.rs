//! Where each badge sits in the Badges root. Without mod folders, on Dawn and Sunrise before 0.6,
//! the one mod lists Project Sunrise, each source's badge in the order below, its custom badges
//! and then the stock badges.
//!
//! On Sunrise's mod loader, Sunrise's merge keeps the root's stock child rows in their places,
//! merges them word by word and
//! adds each mod's own rows after them. Each importer's badge therefore takes one fixed stock place
//! right after Project Sunrise and writes only that row, so two importers never change one word.
//! An importer's root depends on its own badges and on whether the main mod has a badge. The main
//! mod writes Project Sunrise first and arranges the rest around the importers whose badges the
//! family builds. It keeps the stock row in each of their places for the importer to write over,
//! then lists its custom badges and every stock badge in stock order. An importer that builds with
//! the main mod has its badge in the main mod, which writes it in that importer's place. A place
//! whose importer the family doesn't build takes the next badge, so the list closes up. A main mod
//! that only hosts its importers' rows has no badge, so the importers' places start the root.
//! Before 2026-11-10 the first stock badge kept the first place then, and the importers' places
//! still counted from the second.
//!
//! A member folder removed by hand leaves its place showing the stock badge the main mod kept
//! there, which the main mod's list also shows, until the next build.
use super::*;
use std::collections::VecDeque;

/// The importers in their places after Project Sunrise, in the order Parhelion lists its sources.
const PLACES: [Importer; 4] = [
    Importer::D2Mot,
    Importer::D1Roi,
    Importer::HaloReach,
    Importer::Marathon,
];

/// A presentation node's parent array.
const PARENTS_OFFSET: usize = 0x18;

#[derive(Clone, Copy)]
enum Row {
    /// The stock child at this position of the root.
    Stock(usize),
    /// A badge group the build added.
    Badge(usize),
}

/// Lays out the Badges root's children for the member now compiling, once it has added every
/// badge it holds.
pub(crate) fn arrange(nodes: &mut Vec<u8>) -> AuthoringResult<()> {
    let (node_count, _, node_rows, _) = array_at(nodes, 8)?;
    let descriptor = node_rows
        + BADGES_ROOT_NODE_INDEX * PRESENTATION_NODE_ROW_SIZE
        + PRESENTATION_NODE_CHILD_NODES_OFFSET;
    let (count, header, rows, class) = array_at(nodes, descriptor)?;
    if class != PRESENTATION_NODE_CHILD_NODE_ROW_CLASS
        || count != STOCK_BADGES_ROOT_COMPLETION_VALUE as usize
    {
        return Err(invalid(
            "The Badges root no longer holds only its stock badges",
        ));
    }
    let mut badges = Vec::new();
    for index in STOCK_PRESENTATION_NODE_COUNT..node_count {
        if is_badge(nodes, node_rows, index)? {
            badges.push(index);
        }
    }
    // A main mod that only hosts its importers' rows adds no badge, yet still keeps their places
    // and lists the stock badges their badges write over. Without it each importer's badge took
    // a stock badge's place and that stock badge was gone from the list.
    let hosting = crate::package_profile::member().is_none()
        && crate::package_profile::host().is_standalone()
        && PLACES
            .iter()
            .any(|importer| crate::mod_family::build::has_badge(*importer));
    let (own, custom) = match badges.split_first() {
        Some((&own, custom)) => (Some(own), custom),
        None if hosting => (None, &[][..]),
        None => return Ok(()),
    };
    if own.is_some_and(|own| own != STOCK_PRESENTATION_NODE_COUNT) {
        return Err(invalid("The build's own badge isn't its first added node"));
    }
    let layout = layout(nodes, node_rows, count, own, custom)?;

    let row_at = |position: usize| rows + position * PRESENTATION_NODE_CHILD_NODE_ROW_SIZE;
    let stock_row = |position: usize| {
        nodes[row_at(position)..row_at(position) + PRESENTATION_NODE_CHILD_NODE_ROW_SIZE].to_vec()
    };
    // An added badge's row copies the Ace badge's, with no condition, as on other layouts.
    let ace = (0..count)
        .find(|&position| {
            read_u16(nodes, row_at(position)).ok() == Some(ACE_BADGE_GROUP_NODE_INDEX as u16)
        })
        .ok_or_else(|| invalid("The Badges root no longer holds the Ace badge"))?;
    let mut template = stock_row(ace);
    template[8..PRESENTATION_NODE_CHILD_NODE_ROW_SIZE].fill(0);
    let mut conditions = Vec::with_capacity(count);
    for position in 0..count {
        let at = row_at(position);
        conditions.push(if read_u64(nodes, at + 8)? == 0 {
            None
        } else {
            Some(relative_target(nodes, at + 16)?)
        });
    }
    let mut bytes = Vec::with_capacity(layout.len());
    for row in &layout {
        bytes.push(match *row {
            Row::Stock(position) => stock_row(position),
            Row::Badge(badge) => {
                let mut added = template.clone();
                write_u16(
                    &mut added,
                    0,
                    u16::try_from(badge)
                        .map_err(|_| invalid("Badge node index does not fit 16 bits"))?,
                )?;
                added
            }
        });
    }
    let header_bytes = nodes[header..rows].to_vec();

    let new_header = begin_array_block(nodes);
    nodes.extend_from_slice(&header_bytes);
    let new_rows = nodes.len();
    for row in &bytes {
        nodes.extend_from_slice(row);
    }
    while !(nodes.len() + NESTED_ARRAY_TRAILER.len()).is_multiple_of(16) {
        nodes.push(0);
    }
    nodes.extend_from_slice(&NESTED_ARRAY_TRAILER);
    write_u64(nodes, new_header, layout.len() as u64)?;
    // A stock child keeps naming its own stock condition, so the merge reads it as unchanged.
    for (position, row) in layout.iter().enumerate() {
        let Row::Stock(stock) = *row else {
            continue;
        };
        if let Some(target) = conditions[stock] {
            write_relative_pointer(
                nodes,
                new_rows + position * PRESENTATION_NODE_CHILD_NODE_ROW_SIZE + 16,
                target,
            )?;
        }
    }
    write_u64(nodes, descriptor, layout.len() as u64)?;
    write_relative_pointer(nodes, descriptor + 8, new_header)
}

/// Orders the stock and authored badge rows for this mod's role in the family.
fn layout(
    nodes: &[u8],
    node_rows: usize,
    count: usize,
    own: Option<usize>,
    custom: &[usize],
) -> AuthoringResult<Vec<Row>> {
    // In one mod for every source, and in the main mod for an importer that builds with it, the
    // importer's items have a badge in this build too.
    let mut sources = Vec::new();
    for importer in PLACES {
        let hash = Identity::importer(importer).node_hashes[0];
        for &badge in custom {
            let row = node_rows + badge * PRESENTATION_NODE_ROW_SIZE;
            if read_u32(nodes, row + PRESENTATION_NODE_HASH_OFFSET)? == hash {
                sources.push((importer, badge));
            }
        }
    }
    let custom = custom
        .iter()
        .copied()
        .filter(|badge| !sources.iter().any(|(_, source)| source == badge))
        .collect::<Vec<_>>();
    let source_badges = sources.iter().map(|&(_, badge)| badge);
    let layout = match crate::package_profile::member() {
        // Without mod folders the main mod holds every badge and lists them together.
        None if !crate::package_profile::host().is_standalone() => own
            .into_iter()
            .chain(source_badges)
            .chain(custom)
            .map(Row::Badge)
            .chain((0..count).map(Row::Stock))
            .collect::<Vec<_>>(),
        None => {
            let mut next = custom
                .iter()
                .map(|&badge| Row::Badge(badge))
                .chain((0..count).map(Row::Stock))
                .collect::<VecDeque<_>>();
            // Project Sunrise first, when the main mod has a badge, then each importer's place.
            let mut layout = own.map(Row::Badge).into_iter().collect::<Vec<_>>();
            for importer in PLACES {
                // An importer that builds with the main mod has its badge here, in the place its
                // own mod would write. No other member writes that place.
                if let Some(&(_, badge)) = sources.iter().find(|(source, _)| *source == importer) {
                    layout.push(Row::Badge(badge));
                } else if crate::mod_family::build::has_badge(importer) {
                    layout.push(Row::Stock(layout.len()));
                } else if let Some(row) = next.pop_front() {
                    layout.push(row);
                }
            }
            layout.extend(next);
            layout
        }
        Some(importer) => {
            let own = own.ok_or_else(|| invalid("The importer's build adds no badge"))?;
            // The places follow Project Sunrise, or start the root when the main mod has no badge.
            let place = PLACES
                .iter()
                .position(|candidate| *candidate == importer)
                .ok_or_else(|| invalid("The importer has no place in the Badges root"))?
                + usize::from(crate::mod_family::build::main_has_badge());
            let mut layout = (0..count).map(Row::Stock).collect::<Vec<_>>();
            layout[place] = Row::Badge(own);
            layout.extend(source_badges.chain(custom).map(Row::Badge));
            layout
        }
    };
    Ok(layout)
}

/// Whether node `index` is a badge group, whose one parent is the Badges root.
fn is_badge(nodes: &[u8], node_rows: usize, index: usize) -> AuthoringResult<bool> {
    let parents = node_rows + index * PRESENTATION_NODE_ROW_SIZE + PARENTS_OFFSET;
    if read_u64(nodes, parents)? != 1 {
        return Ok(false);
    }
    let (_, _, rows, _) = array_at(nodes, parents)?;
    Ok(usize::from(read_u16(nodes, rows)?) == BADGES_ROOT_NODE_INDEX)
}
