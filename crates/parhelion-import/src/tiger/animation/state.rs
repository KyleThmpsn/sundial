//! Clone named native state routes without changing aliased or unrelated actions.
use crate::{
    presentation::{append, put},
    tiger::payload::Payload,
};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};
pub mod parameter;

fn node(p: &Payload, name: u32) -> Result<usize> {
    let names = p.array(8, 8, Some(0x8080342E))?;
    let selected = names
        .into_iter()
        .filter(|&at| p.u32(at).ok() == Some(name))
        .collect::<Vec<_>>();
    ensure!(
        selected.len() == 1,
        "Requested native action {name:08X} is absent or ambiguous"
    );
    let nodes = p.array(24, 16, Some(0x8080342F))?;
    nodes
        .get(p.u32(selected[0] + 4)? as usize)
        .copied()
        .context("Named action has an absent state")
}

fn choices(p: &Payload, row: usize) -> Result<Vec<usize>> {
    ensure!(p.u64(row + 8)? != 0, "Native action has no state payload");
    let at = p.pointer(row + 8)?;
    match p.u64(row)? {
        1 => Ok(vec![at]),
        2 => {
            ensure!(
                p.0.get(at + 64..at + 120)
                    .context("State selector tail")?
                    .iter()
                    .all(|&v| v == 0),
                "State selector has unsupported trailing data"
            );
            p.array(at, 16, Some(0x80803437))
        }
        other => anyhow::bail!("Unsupported native state kind {other}"),
    }
}

pub fn descriptors(p: &Payload, name: u32) -> Result<BTreeSet<usize>> {
    let mut result = BTreeSet::new();
    for choice in choices(p, node(p, name)?)? {
        for at in p.array(choice, 8, Some(0x80803439))? {
            ensure!(p.f32(at + 4)?.is_finite(), "Invalid state choice weight");
            result.insert(p.u32(at)? as usize);
        }
    }
    ensure!(!result.is_empty(), "Requested native action has no clips");
    Ok(result)
}

fn structure(out: &mut Vec<u8>, p: &Payload, at: usize, length: usize) -> Result<usize> {
    let start = (out.len() + 19) & !15;
    out.resize(start - 4, 0);
    out.extend(p.u32(at - 4)?.to_le_bytes());
    out.extend(
        p.0.get(at..at + length)
            .context("State structure boundary")?,
    );
    Ok(start)
}

fn weighted(
    out: &mut Vec<u8>,
    field: usize,
    p: &Payload,
    original: usize,
    map: &BTreeMap<usize, Vec<usize>>,
) -> Result<()> {
    let mut bytes = Vec::new();
    let mut previous = 0.0;
    for at in p.array(original, 8, Some(0x80803439))? {
        let descriptors = map
            .get(&(p.u32(at)? as usize))
            .context("Untranslated state choice")?;
        ensure!(!descriptors.is_empty(), "Translated state choice is empty");
        let cutoff = p.f32(at + 4)?;
        ensure!(
            cutoff.is_finite() && cutoff >= previous && cutoff <= 1.0,
            "Native cumulative animation weights are invalid"
        );
        for (index, &descriptor) in descriptors.iter().enumerate() {
            bytes.extend(u32::try_from(descriptor)?.to_le_bytes());
            let next = if index + 1 == descriptors.len() {
                cutoff
            } else {
                previous + (cutoff - previous) * (index + 1) as f32 / descriptors.len() as f32
            };
            bytes.extend(next.to_le_bytes());
        }
        previous = cutoff;
    }
    append(out, field, 0x80803439, &bytes, 8)
}

pub fn clone_actions(
    p: &Payload,
    routes: &BTreeMap<u32, BTreeMap<usize, usize>>,
) -> Result<Payload> {
    let routes = routes
        .iter()
        .map(|(&name, map)| {
            (
                name,
                map.iter().map(|(&from, &to)| (from, vec![to])).collect(),
            )
        })
        .collect();
    clone_variants(p, &routes)
}

/// By action name, the choices to replace by index, each as its descriptors with their
/// cumulative weight cutoffs as `f32` bits.
pub type ChoiceRoutes = BTreeMap<u32, BTreeMap<usize, Vec<(usize, u32)>>>;

/// Replace complete weighted choices while preserving the native selector and
/// every unselected variant. Each named action receives a private node.
pub fn replace_selected_choices(p: &Payload, routes: &ChoiceRoutes) -> Result<Payload> {
    let mut out = p.0.clone();
    let mut nodes = p
        .array(24, 16, Some(0x8080342F))?
        .into_iter()
        .map(|row| {
            Ok((
                p.u64(row)?,
                if p.u64(row + 8)? == 0 {
                    None
                } else {
                    Some(p.pointer(row + 8)?)
                },
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut replacements = BTreeMap::new();
    for (&name, replacements_by_choice) in routes {
        let row = node(p, name)?;
        let at = p.pointer(row + 8)?;
        let kind = p.u64(row)?;
        let selected = choices(p, row)?;
        ensure!(
            !replacements_by_choice.is_empty()
                && replacements_by_choice
                    .keys()
                    .all(|&index| index < selected.len()),
            "Selected action choice is absent"
        );
        let target = structure(&mut out, p, at, if kind == 1 { 16 } else { 120 })?;
        let fields = if kind == 1 {
            vec![target]
        } else {
            append(
                &mut out,
                target,
                0x80803437,
                &vec![0; selected.len() * 16],
                16,
            )?;
            Payload(out.clone()).array(target, 16, Some(0x80803437))?
        };
        for (index, (field, choice)) in fields.into_iter().zip(selected).enumerate() {
            let values = if let Some(values) = replacements_by_choice.get(&index) {
                values.clone()
            } else {
                p.array(choice, 8, Some(0x80803439))?
                    .into_iter()
                    .map(|r| Ok((p.u32(r)? as usize, p.u32(r + 4)?)))
                    .collect::<Result<Vec<_>>>()?
            };
            let mut previous = 0.;
            let mut bytes = Vec::new();
            for (descriptor, weight) in values {
                let cutoff = f32::from_bits(weight);
                ensure!(
                    cutoff.is_finite() && cutoff >= previous && cutoff <= 1.,
                    "Invalid weighted animation choice"
                );
                previous = cutoff;
                bytes.extend(u32::try_from(descriptor)?.to_le_bytes());
                bytes.extend(weight.to_le_bytes());
            }
            // Empty native variants can belong to unreachable profiles. Changed
            // variants must cover the full interval, as do nonempty originals.
            ensure!(
                (bytes.is_empty() && !replacements_by_choice.contains_key(&index))
                    || previous == 1.,
                "Animation choice does not cover the interval"
            );
            append(&mut out, field, 0x80803439, &bytes, 8)?;
        }
        if kind == 2 {
            for (offset, stride, class) in [
                (16, 1, 0x80808DA4),
                (32, 28, 0x8080347D),
                (48, 4, 0x8080347C),
            ] {
                let bytes = &p.0[p.array_range(at + offset, stride, Some(class))?];
                append(&mut out, target + offset, u64::from(class), bytes, stride)?;
            }
        }
        replacements.insert(name, nodes.len());
        nodes.push((kind, Some(target)));
    }
    append(&mut out, 24, 0x8080342F, &vec![0; nodes.len() * 16], 16)?;
    for (field, (kind, target)) in Payload(out.clone())
        .array(24, 16, Some(0x8080342F))?
        .into_iter()
        .zip(nodes)
    {
        put(&mut out, field, &kind.to_le_bytes())?;
        put(
            &mut out,
            field + 8,
            &target
                .map_or(0, |target| target as i64 - field as i64 - 8)
                .to_le_bytes(),
        )?;
    }
    for field in p.array(8, 8, Some(0x8080342E))? {
        if let Some(&index) = replacements.get(&p.u32(field)?) {
            put(&mut out, field + 4, &u32::try_from(index)?.to_le_bytes())?;
        }
    }
    let length = out.len() as u64;
    put(&mut out, 0, &length.to_le_bytes())?;
    Ok(Payload(out))
}

/// Split each original cumulative interval evenly among its translated choices.
/// Named states receive private nodes so an aliased action retains its old route.
pub fn clone_variants(
    p: &Payload,
    routes: &BTreeMap<u32, BTreeMap<usize, Vec<usize>>>,
) -> Result<Payload> {
    let mut out = p.0.clone();
    let rows = p.array(24, 16, Some(0x8080342F))?;
    let mut nodes = rows
        .iter()
        .map(|&row| {
            Ok((
                p.u64(row)?,
                if p.u64(row + 8)? == 0 {
                    None
                } else {
                    Some(p.pointer(row + 8)?)
                },
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut replacements = BTreeMap::new();
    for (&name, map) in routes {
        let row = node(p, name)?;
        let at = p.pointer(row + 8)?;
        let kind = p.u64(row)?;
        let target = structure(&mut out, p, at, if kind == 1 { 16 } else { 120 })?;
        let selected = choices(p, row)?;
        if kind == 1 {
            weighted(&mut out, target, p, at, map)?;
        } else {
            append(
                &mut out,
                target,
                0x80803437,
                &vec![0; selected.len() * 16],
                16,
            )?;
            let fields = Payload(out.clone()).array(target, 16, Some(0x80803437))?;
            for (&field, &choice) in fields.iter().zip(&selected) {
                weighted(&mut out, field, p, choice, map)?;
            }
            for (offset, stride, class) in [
                (16, 1, 0x80808DA4),
                (32, 28, 0x8080347D),
                (48, 4, 0x8080347C),
            ] {
                let bytes = p
                    .array(at + offset, stride, Some(class))?
                    .into_iter()
                    .flat_map(|at| p.0[at..at + stride].iter().copied())
                    .collect::<Vec<_>>();
                append(&mut out, target + offset, u64::from(class), &bytes, stride)?;
            }
        }
        replacements.insert(name, nodes.len());
        nodes.push((kind, Some(target)));
    }
    append(&mut out, 24, 0x8080342F, &vec![0; nodes.len() * 16], 16)?;
    let fields = Payload(out.clone()).array(24, 16, Some(0x8080342F))?;
    for (field, (kind, target)) in fields.into_iter().zip(nodes) {
        put(&mut out, field, &kind.to_le_bytes())?;
        put(
            &mut out,
            field + 8,
            &target
                .map_or(0, |target| target as i64 - field as i64 - 8)
                .to_le_bytes(),
        )?;
    }
    for field in p.array(8, 8, Some(0x8080342E))? {
        if let Some(&index) = replacements.get(&p.u32(field)?) {
            put(&mut out, field + 4, &u32::try_from(index)?.to_le_bytes())?;
        }
    }
    let len = out.len() as u64;
    put(&mut out, 0, &len.to_le_bytes())?;
    Ok(Payload(out))
}
