//! Private supplemental routes retain native drivers and replace qualified source motion.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub struct Layer {
    pub name: u32,
    pub clips: Vec<(usize, f32)>,
}

pub struct Hold {
    pub original: usize,
    pub descriptor: usize,
    pub layers: Vec<Layer>,
}

fn copy_rows(
    p: &mut Payload,
    field: usize,
    class: u32,
    stride: usize,
    sources: &[usize],
    pointers: &[usize],
) -> Result<Vec<usize>> {
    let bytes = sources
        .iter()
        .flat_map(|&at| p.0[at..at + stride].iter().copied())
        .collect::<Vec<_>>();
    let targets = sources
        .iter()
        .map(|&at| {
            pointers
                .iter()
                .map(|&f| {
                    Ok((
                        f,
                        if p.u64(at + f)? == 0 {
                            None
                        } else {
                            Some(p.pointer(at + f + 8)?)
                        },
                    ))
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    append(&mut p.0, field, u64::from(class), &bytes, stride)?;
    let rows = p.array(field, stride, Some(class))?;
    for (&row, targets) in rows.iter().zip(targets) {
        for (field, target) in targets {
            if let Some(target) = target {
                put(
                    &mut p.0,
                    row + field + 8,
                    &(target as i64 - row as i64 - field as i64 - 8).to_le_bytes(),
                )?;
            }
        }
    }
    Ok(rows)
}

/// Extra tags are supplemental clips only. Existing clip and descriptor ordinals stay fixed.
#[allow(clippy::too_many_arguments)]
pub fn extend(
    bank: &mut Payload,
    layers: &mut Payload,
    directions: &mut Payload,
    original_layers: &Payload,
    original_directions: &Payload,
    extra_tags: &[u32],
    all_tags: &[u32],
    holds: &[Hold],
) -> Result<()> {
    let mut clips = rows(bank, 8, 4, 0x80808F48)?;
    // One signed 32-bit word per clip, class 0x80800007. A clip without one holds -1.
    let mut auxiliary = rows(bank, 24, 4, 0x80800007)?;
    for tag in extra_tags {
        ensure!(![0, u32::MAX].contains(tag), "Empty holding clip tag");
        clips.extend(tag.to_le_bytes());
        auxiliary.extend(u32::MAX.to_le_bytes());
    }
    let ordinals = clips
        .chunks_exact(4)
        .enumerate()
        .map(|(i, b)| (u32::from_le_bytes(b.try_into().unwrap()), i))
        .collect::<BTreeMap<_, _>>();
    append(&mut bank.0, 8, 0x80808F48, &clips, 4)?;
    append(&mut bank.0, 24, 0x80800007, &auxiliary, 4)?;
    let groups = layers.array(8, 24, Some(0x80803724))?;
    let group_names = groups
        .iter()
        .enumerate()
        .map(|(i, &at)| Ok((layers.u32(at + 16)?, i)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    ensure!(
        groups.len() == group_names.len(),
        "Duplicate holding layer names"
    );
    let mut route_sources = layers.array(56, 32, Some(0x808034EF))?;
    let original_routes = original_layers.array(56, 32, Some(0x808034EF))?;
    let original_alias = original_layers.array(40, 2, Some(0x80800006))?;
    let mut edits = Vec::new();
    for hold in holds {
        let alias = usize::from(
            original_layers.u16(
                *original_alias
                    .get(hold.original)
                    .context("Holding descriptor")?,
            )?,
        );
        let original_route = *original_routes.get(alias).context("Holding route")?;
        // Existing routes are append-only. Resolve the current row so pointers remain local.
        let route = *route_sources.get(alias).context("Current holding route")?;
        let mut edges = rows(original_layers, original_route, 4, 0x808034F1)?;
        let mut seen = BTreeSet::new();
        for layer in &hold.layers {
            let index = *group_names.get(&layer.name).context("Holding layer name")?;
            ensure!(
                seen.insert(index) && !layer.clips.is_empty(),
                "Duplicate or empty holding layer"
            );
            let edge = edges
                .chunks_exact_mut(4)
                .find(|e| usize::from(u16::from_le_bytes([e[2], e[3]])) == index)
                .context("Holding route does not select this layer")?;
            let choice = usize::from(u16::from_le_bytes([edge[0], edge[1]]));
            let mut choices = layers.array(groups[index], 16, Some(0x8080372E))?;
            let selected = *choices.get(choice).context("Holding choice")?;
            let ordinal = choices.len();
            choices.push(selected);
            let choices = copy_rows(layers, groups[index], 0x8080372E, 16, &choices, &[0])?;
            let options = layers.array(selected, 64, Some(0x80803730))?;
            ensure!(!options.is_empty(), "Holding choice has no options");
            let options = copy_rows(layers, choices[ordinal], 0x80803730, 64, &options, &[0, 16])?;
            let mut weights = Vec::new();
            for &(clip, weight) in &layer.clips {
                ensure!(
                    weight.is_finite() && weight >= 0.,
                    "Invalid holding clip weight"
                );
                let tag = *all_tags.get(clip).context("Holding clip allocation")?;
                weights.extend(
                    u32::try_from(*ordinals.get(&tag).context("Holding clip not in bank")?)?
                        .to_le_bytes(),
                );
                weights.extend(weight.to_le_bytes());
            }
            for option in options {
                append(&mut layers.0, option, 0x80803737, &weights, 8)?;
            }
            edge[..2].copy_from_slice(&u16::try_from(ordinal)?.to_le_bytes());
        }
        ensure!(!seen.is_empty(), "Body pose has no source holding layers");
        edits.push((hold.descriptor, route_sources.len(), edges));
        route_sources.push(route);
    }
    let routes = copy_rows(layers, 56, 0x808034EF, 32, &route_sources, &[0, 16])?;
    let aliases = layers.array(40, 2, Some(0x80800006))?;
    for (descriptor, route, edges) in edits {
        append(&mut layers.0, routes[route], 0x808034F1, &edges, 4)?;
        put(
            &mut layers.0,
            *aliases
                .get(descriptor)
                .context("Private holding descriptor")?,
            &u16::try_from(route)?.to_le_bytes(),
        )?;
    }
    // These body clips use native control frames, so preserve dynamic direction response.
    for field in [8, 0x38] {
        let old = original_directions.array(field, 2, Some(0x80800006))?;
        let new = directions.array(field, 2, Some(0x80800006))?;
        for hold in holds {
            put(
                &mut directions.0,
                new[hold.descriptor],
                &original_directions.u16(old[hold.original])?.to_le_bytes(),
            )?;
        }
    }
    for payload in [bank, layers, directions] {
        let length = payload.0.len() as u64;
        put(&mut payload.0, 0, &length.to_le_bytes())?;
    }
    Ok(())
}
