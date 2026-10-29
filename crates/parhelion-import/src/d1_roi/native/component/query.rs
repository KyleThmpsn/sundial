//! Bind the bounded source query shape to its matching native component and dispatch records.
use super::*;
use crate::d1_roi::native::perk::label;
use crate::tiger::sequence::{object, relative};
use std::collections::BTreeMap;
use std::sync::Arc;

const TEMPLATES: [(u32, u32); 3] = [
    (0x81522CBA, 0x80BB890A),
    (0x8152C721, 0x80BB893B),
    (0x81524A0F, 0x815248AF),
];
/// Range scalar classes paired by query package structure, with a stock query holding the native
/// class. Each class measures its own quantity, so other providers reject.
const SCALARS: [(u32, u32, u32); 2] = [
    (0x8080306D, 0x80804D61, 0x8152B20D),
    (0x808027D3, 0x80804D6E, 0x80FEEB6C),
];

/// Source label dictionary and the Shadowkeep registry that query label terms compile against.
struct Labels<'a> {
    source: &'a Source,
    registry: &'a Payload,
}

/// The native range scalar class and its eight parameter bytes. Both versions follow the
/// scalar base with them, at source `+20` and native `+30`.
fn scalar(p: &Payload, value: usize, source: bool) -> Result<(u32, u64)> {
    let scalar = p.pointer(value)?;
    let found = p.u32(scalar - 4)?;
    let &(from, to, _) = SCALARS
        .iter()
        .find(|row| found == if source { row.0 } else { row.1 })
        .with_context(|| {
            format!("Query range scalar provider {found:08X} has no established counterpart")
        })?;
    let class = if source { from } else { to };
    ensure!(
        p.u64(scalar)? == u64::from(class)
            && p.u32(scalar + 8)? == p.u32(p.pointer(16)?)?
            && p.u64(scalar + 16)? == scalar as u64,
        "Query range scalar provider differs"
    );
    let parameters = if source {
        ensure!(
            p.u32(scalar + 12)? == 0 && p.u64(scalar + 24)? == u64::from(class),
            "Source query range scalar differs"
        );
        p.u64(scalar + 32)?
    } else {
        ensure!(
            p.u32(scalar + 12)? == class,
            "Native query range scalar class differs"
        );
        p.u64(scalar + 48)?
    };
    ensure!(
        p.f32(value + 8)?.is_finite()
            && p.f32(value + 12)?.is_finite()
            && p.f32(value + 8)? <= p.f32(value + 12)?
            && p.u32(value + 16)? == 0,
        "Query range bounds or flags differ"
    );
    Ok((to, parameters))
}

/// A native label term's declared lists, which name the Shadowkeep registry.
fn native_labels(p: &Payload, value: usize) -> Result<[Vec<u32>; 4]> {
    ensure!(
        p.u32(value + 0x48)? == 0x80C70CA1,
        "Native query label term names another registry"
    );
    let mut lists: [Vec<u32>; 4] = Default::default();
    for (operation, names) in lists.iter_mut().enumerate() {
        for row in p.array(value + operation * 16, 24, Some(0x808094B3))? {
            names.push(p.u32(row)?);
        }
    }
    Ok(lists)
}

/// Whether a source filter can hold for a Shadowkeep object. Each candidate takes the output of
/// the first filter whose suffix byte for a match or a miss is not FF. In a conjunction that
/// passes misses on, a label term that no Shadowkeep object satisfies makes the filter inert.
fn reachable(p: &Payload, filter: usize, labels: &Labels) -> Result<bool> {
    for term in p.array(filter + 16, 16, Some(0x808003AE))? {
        let value = p.pointer(term + 8)?;
        if p.u32(value - 4)? == 0x8080280E
            && label::destination(labels.source, p, value, labels.registry)?.is_none()
        {
            ensure!(
                p.u64(term)? == 0 && p.u8(filter + 8)? == 0 && p.u8(filter + 33)? == 0xFF,
                "A query label term that no Shadowkeep object satisfies needs an unnegated term in a conjunction that passes misses on"
            );
            return Ok(false);
        }
    }
    Ok(true)
}

/// Predicates in native terms. A source query has `labels` and leaves out inert filters.
fn predicates(p: &Payload, labels: Option<&Labels>) -> Result<Value> {
    let source = labels.is_some();
    let r = p.pointer(24)?;
    let mut groups = Vec::new();
    for row in p.array(
        r + if source { 0x98 } else { 0x80 },
        24,
        Some(if source { 0x808030AA } else { 0x80803832 }),
    )? {
        let kind = p.pointer(row)?;
        ensure!(
            p.u32(kind - 4)? == if source { 0x808030F3 } else { 0x80803838 },
            "Query group implementation differs"
        );
        let mut filters = Vec::new();
        for filter in p.array(
            row + 8,
            40,
            Some(if source { 0x80803182 } else { 0x80803835 }),
        )? {
            if let Some(labels) = labels
                && !reachable(p, filter, labels)?
            {
                continue;
            }
            let mut terms = Vec::new();
            for term in p.array(
                filter + 16,
                16,
                Some(if source { 0x808003AE } else { 0x80809316 }),
            )? {
                let value = p.pointer(term + 8)?;
                let flags = p.u64(term)?;
                terms.push(match (labels, p.u32(value - 4)?) {
                    (Some(_), 0x8080295D) | (None, 0x80804D76) => {
                        json!([flags, 0, [p.u32(value)?]])
                    }
                    (Some(_), 0x808026B3) | (None, 0x80804D7A) => {
                        json!([flags, 1, [p.u32(value)?, p.u32(value + 4)?]])
                    }
                    (Some(_), 0x80802C1C) | (None, 0x80804D5F) => {
                        let (class, parameters) = scalar(p, value, source)?;
                        let bounds = [p.u32(value + 8)?, p.u32(value + 12)?];
                        json!([flags, 2, bounds, class, parameters])
                    }
                    (Some(labels), 0x8080280E) => {
                        let lists = label::destination(labels.source, p, value, labels.registry)?
                            .context("Inert query label term")?;
                        json!([flags, 3, lists])
                    }
                    (None, 0x80804D73) => json!([flags, 3, native_labels(p, value)?]),
                    (_, class) => anyhow::bail!("Unsupported query predicate class {class:08X}"),
                });
            }
            filters.push(json!([
                p.bytes::<16>(filter)?,
                terms,
                p.bytes::<8>(filter + 32)?
            ]));
        }
        groups.push(json!([p.u32(kind)?, filters]));
    }
    Ok(json!(groups))
}

/// Source filters left out because no Shadowkeep object satisfies them, with their labels.
fn omitted(p: &Payload, labels: &Labels) -> Result<Vec<Value>> {
    let r = p.pointer(24)?;
    let mut result = Vec::new();
    for (group, row) in p
        .array(r + 0x98, 24, Some(0x808030AA))?
        .into_iter()
        .enumerate()
    {
        for (index, filter) in p
            .array(row + 8, 40, Some(0x80803182))?
            .into_iter()
            .enumerate()
        {
            if reachable(p, filter, labels)? {
                continue;
            }
            let mut names = Vec::new();
            for term in p.array(filter + 16, 16, Some(0x808003AE))? {
                let value = p.pointer(term + 8)?;
                if p.u32(value - 4)? == 0x8080280E {
                    for operation in 0..4 {
                        let rows = p.array(value + operation * 16, 24, Some(0x80800365))?;
                        let rows = rows.into_iter().map(|row| p.u32(row));
                        names.push(rows.collect::<Result<Vec<_>>>()?);
                    }
                }
            }
            result.push(json!({"group":group,"filter":index,"output":p.u8(filter + 32)?,
                "source_label_operations":names,"reason":"labels absent from the Shadowkeep registry"}));
        }
    }
    Ok(result)
}

/// The first range scalar of `class` in a stock query. Its body supplies native initial state.
fn native_scalar(p: &Payload, class: u32) -> Result<usize> {
    let r = p.pointer(24)?;
    for group in p.array(r + 0x80, 24, Some(0x80803832))? {
        for filter in p.array(group + 8, 40, Some(0x80803835))? {
            for term in p.array(filter + 16, 16, Some(0x80809316))? {
                let predicate = p.pointer(term + 8)?;
                if p.u32(predicate - 4)? == 0x80804D5F && p.u32(p.pointer(predicate)? - 4)? == class
                {
                    // Only this range term is checked. Other terms may use unadmitted classes.
                    scalar(p, predicate, false)?;
                    return p.pointer(predicate);
                }
            }
        }
    }
    anyhow::bail!("Native query template has no {class:08X} range scalar")
}

fn translate(
    source: &Payload,
    target: &mut Payload,
    labels: &Labels,
    scalars: &BTreeMap<u32, Arc<Payload>>,
) -> Result<()> {
    let s = source.pointer(24)?;
    let n = target.pointer(24)?;
    let owner = target.u32(target.pointer(16)?)?;
    let groups = source.array(s + 0x98, 24, Some(0x808030AA))?;
    array(
        &mut target.0,
        n + 0x80,
        0x80803832,
        &vec![0; groups.len() * 24],
        24,
    )?;
    for (sg, ng) in groups
        .into_iter()
        .zip(target.array(n + 0x80, 24, Some(0x80803832))?)
    {
        let kind = source.pointer(sg)?;
        ensure!(
            source.u32(kind - 4)? == 0x808030F3 && source.u32(kind)? == 1,
            "Unsupported query group kind"
        );
        let nk = object(target, 0x80803838, 4)?;
        put(&mut target.0, nk, &1u32.to_le_bytes())?;
        relative(target, ng, nk)?;
        let mut filters = Vec::new();
        for sf in source.array(sg + 8, 40, Some(0x80803182))? {
            ensure!(
                source.u64(sf)? == 32
                    && source.u64(sf + 8)? == 0
                    && source.u64(sf + 32)? & !0xff == 0xff00,
                "Unsupported query filter flags"
            );
            if reachable(source, sf, labels)? {
                filters.push(sf);
            }
        }
        array(
            &mut target.0,
            ng + 8,
            0x80803835,
            &vec![0; filters.len() * 40],
            40,
        )?;
        for (sf, nf) in filters
            .into_iter()
            .zip(target.array(ng + 8, 40, Some(0x80803835))?)
        {
            put(&mut target.0, nf, &source.bytes::<16>(sf)?)?;
            put(&mut target.0, nf + 32, &source.bytes::<8>(sf + 32)?)?;
            let terms = source.array(sf + 16, 16, Some(0x808003AE))?;
            array(
                &mut target.0,
                nf + 16,
                0x80809316,
                &vec![0; terms.len() * 16],
                16,
            )?;
            for (st, nt) in terms
                .into_iter()
                .zip(target.array(nf + 16, 16, Some(0x80809316))?)
            {
                ensure!(source.u64(st)? <= 1, "Unsupported query predicate flags");
                put(&mut target.0, nt, &source.bytes::<8>(st)?)?;
                let value = source.pointer(st + 8)?;
                if source.u32(value - 4)? == 0x8080280E {
                    let lists = label::destination(labels.source, source, value, labels.registry)?
                        .context("Inert query label term")?;
                    let bytes = label::predicate(&lists, labels.registry)?;
                    let nv = object(target, 0x80804D73, bytes.len())?;
                    put(&mut target.0, nv, &bytes)?;
                    relative(target, nt + 8, nv)?;
                    continue;
                }
                let (class, length) = match source.u32(value - 4)? {
                    0x8080295D => (0x80804D76, 4),
                    0x808026B3 => (0x80804D7A, 8),
                    0x80802C1C => (0x80804D5F, 20),
                    _ => anyhow::bail!("Unsupported source query predicate"),
                };
                let nv = object(target, class, length)?;
                put(&mut target.0, nv, &source.0[value..value + length])?;
                relative(target, nt + 8, nv)?;
                if class == 0x80804D5F {
                    let (class, parameters) = scalar(source, value, true)?;
                    let template = &scalars[&class];
                    let from = native_scalar(template, class)?;
                    // The native body is the 0x30-byte range scalar base and the parameters.
                    let to = object(target, class, 0x38)?;
                    put(&mut target.0, to, &template.bytes::<0x38>(from)?)?;
                    put(&mut target.0, to + 8, &owner.to_le_bytes())?;
                    put(&mut target.0, to + 16, &(to as u64).to_le_bytes())?;
                    put(&mut target.0, to + 48, &parameters.to_le_bytes())?;
                    relative(target, nv, to)?;
                }
            }
        }
    }
    Ok(())
}

/// The embedded target search reads four Float32 values and a flags word, in the same order at
/// source `+D8` and native `+B8`. Flag bit 1 admits only targets that pass the owner's
/// relationship test, bit 0 only those that fail it, and zero admits both. Source initialization
/// clears both bits when both are set, so that combination is not translated.
fn search(source: &Payload, target: &mut Payload) -> Result<Value> {
    let from = source.pointer(24)? + 0xD8;
    let values = (0..4)
        .map(|i| source.f32(from + i * 4))
        .collect::<Result<Vec<_>>>()?;
    let flags = source.u32(from + 16)?;
    ensure!(
        values.iter().all(|v| v.is_finite()) && flags <= 2 && source.u32(from + 20)? == 0,
        "Source query target search differs"
    );
    let to = target.pointer(24)? + 0xB8;
    ensure!(
        target.u32(to + 16)? <= 2 && target.u32(to + 20)? == 0,
        "Native query target search differs"
    );
    put(&mut target.0, to, &source.bytes::<24>(from)?)?;
    Ok(json!({"values":values,"flags":flags}))
}

/// Query output expressions have the same row layout in both builds. The bounded
/// adapter keeps a native row only when its selectors, program, constants and scalar
/// descriptor all match. Matching predicates alone does not establish these outputs.
fn parameters(p: &Payload, source: bool) -> Result<Value> {
    let r = p.pointer(24)?;
    ensure!(
        p.u32(r - 4)? == if source { 0x80803136 } else { 0x80803820 },
        "Query resource class differs"
    );
    let at = r + if source { 0xA8 } else { 0x90 };
    let mut outputs = Vec::new();
    let rows = p.array(at, 72, Some(if source { 0x8080302A } else { 0x80803830 }))?;
    ensure!(
        !rows.is_empty() || p.u64(at + 8)? == 0,
        "Empty query output array retains a pointer"
    );
    for row in rows {
        let code = p
            .array(row + 16, 1, Some(0x80800009))?
            .into_iter()
            .map(|a| p.u8(a))
            .collect::<Result<Vec<_>>>()?;
        let constants = p
            .array(row + 32, 16, Some(0x80800090))?
            .into_iter()
            .map(|a| p.bytes::<16>(a))
            .collect::<Result<Vec<_>>>()?;
        // The checked native carrier publishes output zero from a constant-one
        // expression. Other selectors, scalar providers and programs need a consumer pair.
        ensure!(
            p.bytes::<16>(row)? == [0; 16]
                && code == [0x34, 0, 0x3E, 0]
                && constants.len() == 1
                && constants[0]
                    .chunks_exact(4)
                    .all(|value| value == 1f32.to_le_bytes())
                && p.u64(row + 48)? == 1
                && p.u32(row + 56)? == 1
                && p.u32(row + 60)? == 0
                && p.u64(row + 64)? == 0,
            "Query output expression needs another native adapter"
        );
        outputs.push(json!({"selectors":p.bytes::<16>(row)?,"code":code,
            "constants":constants,"dimensions":p.bytes::<16>(row+48)?,
            "scalar":p.bytes::<8>(row+64)?}));
    }
    Ok(
        json!({"values":p.bytes::<8>(r+if source {0x90} else {0x78})?,
        "outputs":outputs,"flags":p.bytes::<8>(at+16)?}),
    )
}

impl Assembly<'_> {
    pub(super) fn query(
        &mut self,
        source: &Payload,
        name: &str,
        index: usize,
    ) -> Result<Component> {
        let registry = self.native.tag(0x80C70CA1, None)?;
        let labels = Labels {
            source: self.source,
            registry: &registry,
        };
        let queries = predicates(source, Some(&labels))?;
        let omitted = omitted(source, &labels)?;
        let parameters = parameters(source, true)?;
        let mut selected = None;
        let mut exact = false;
        for (owner, entity) in TEMPLATES {
            let native = self.native.tag(owner, Some(0x80809C36))?;
            if parameters != self::parameters(&native, false)? {
                continue;
            }
            selected.get_or_insert((owner, entity));
            if queries == predicates(&native, None)? {
                selected = Some((owner, entity));
                exact = true;
                break;
            }
        }
        let (owner, entity) =
            selected.context("Source query parameters need another native adapter")?;
        let mut component = self.component(owner, entity, name, index)?;
        let native = &component.template;
        ensure!(
            source.bytes::<24>(source.pointer(24)? + 0x168)? == [0; 24],
            "Source query trailing outputs need another native adapter"
        );
        let search = search(source, &mut component.payload)?;
        let outputs = self.query_outputs(source, native)?;
        if !exact {
            let mut scalars = BTreeMap::new();
            for (_, class, tag) in SCALARS {
                scalars.insert(class, self.native.tag(tag, Some(0x80809C36))?);
            }
            translate(source, &mut component.payload, &labels, &scalars)?;
        }
        ensure!(
            queries == predicates(&component.payload, None)?,
            "Translated query predicates differ"
        );
        activation(source, &mut component.payload)?;
        component.seal()?;
        self.evidence.push(json!({"source_component":source.u32(source.pointer(16)?)?,
            "resource_class":0x80803136u32,"native_resource_class":0x80803820u32,
            "query_adapter":{"predicates":queries,"omitted_filters":omitted,"output_names":outputs,"target_search":search,"parameters":parameters},
            "gameplay_verified":false}));
        Ok(component)
    }

    fn query_outputs(&mut self, source: &Payload, native: &Payload) -> Result<Vec<u32>> {
        let (s, n) = (source.pointer(24)?, native.pointer(24)?);
        let mut outputs = Vec::new();
        for (sf, nf, si, ni, sc, nc, sm, nm, embedded) in [
            (
                0x60, 0x48, 0x80800581, 0x80809ADE, 0x808008FE, 0x80809ADD, 1, 0, false,
            ),
            (
                0x78, 0x60, 0x8080059E, 0x808097B9, 0x80800835, 0x808097B8, 3, 2, false,
            ),
            (
                0xF0, 0xD0, 0x808007E9, 0x808097BB, 0x808004CD, 0x808097BA, 1, 0, true,
            ),
            (
                0x108, 0x118, 0x8080053B, 0x80809AE1, 0x80800539, 0x80809AE2, 4, 3, false,
            ),
            (
                0x128, 0x138, 0x8080053B, 0x80809AE1, 0x80800539, 0x80809AE2, 5, 4, false,
            ),
            (
                0x148, 0x158, 0x8080053B, 0x80809AE1, 0x80800539, 0x80809AE2, 6, 5, false,
            ),
        ] {
            ensure!(
                source.pointer(s + sf)? == s + if embedded { 0xC0 } else { 0 }
                    && native.pointer(n + nf)? == n + if embedded { 0xA8 } else { 0 },
                "Query provider points outside its definition"
            );
            let a = self.source.read(source.u32(s + sf + 8)?)?;
            let b = self.native.tag(native.u32(n + nf + 8)?, None)?;
            for (p, interface, instance, implementation, method, row_class) in [
                (
                    &a,
                    si,
                    sc,
                    if embedded { 0x80802BF3 } else { 0x80803143 },
                    sm,
                    0x808005C0,
                ),
                (
                    b.as_ref(),
                    ni,
                    nc,
                    if embedded { 0x80804345 } else { 0x8080381F },
                    nm,
                    0x80809C56,
                ),
            ] {
                let rows = p.array(16, 24, Some(row_class))?;
                ensure!(
                    p.u32(8)? == interface
                        && p.u32(12)? == instance
                        && rows.len() == 1
                        && p.u32(rows[0])? == implementation
                        && p.u32(rows[0] + 4)? == method
                        && p.bytes::<16>(rows[0] + 8)? == [0; 16],
                    "Query provider dispatch differs"
                );
            }
            if sf >= 0x108 {
                let output = source.u32(s + sf + 24)?;
                ensure!(
                    output == native.u32(n + nf + 24)?,
                    "Query output name differs"
                );
                outputs.push(output);
            }
        }
        Ok(outputs)
    }
}
