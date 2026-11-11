//! The kept firing lights in the source firing light's color.
//!
//! A light event (`80806F49`) names its light record (`80807140`) at definition `+0x40`. The
//! record names its material (`808071E8`) at `+0x90` and two parameter records (`808071DC`) at
//! `+0xA0` and `+0xA4`. The stock firing lights store no color. Their material's pixel expression
//! and their parameter expressions read object vector 0 (`4D 00`), turn it into colors through
//! stored constants and store them in output rows (`43`). Expression bytecode sits at stage `+0x20`
//! with constants at stage `+0x30` in a material and at `+0x30` and `+0x40` in a parameter record.
//!
//! A private copy keeps each program and appends a literal store of the source color over every
//! output row the object vector feeds, `34 k 43 n` (push constant row `k`, store output row `n`),
//! so the later store replaces the donor's color. The material TFX dialect's `34` and `43` are
//! established in original evaluator execution. A row is fed by the object vector when its
//! statement, the instructions after the previous store, reads `4D` or a temporary (`45`) that an
//! earlier fed statement stored (`46`). The literal keeps the donor's brightness scale: the source
//! color at unit luminance, by the luminance weights the program stores or Rec. 709, times the
//! uniform constant the statement last multiplies by (`34 k 03`). Where the statement ends with the
//! stock exposure product (`3C 04 40 3C 01 07 03 0E`), the literal ends with it too. The fourth
//! lane repeats blue, as the statements' `22 1A` lane selection leaves it. That the donor's color
//! has unit luminance before that scale is inferred, since its other arithmetic opcodes (`0B`,
//! `0E`, `0F`, `23`, `3A`) are not decoded. The record's reach stays the donor's, since its
//! material also stores an attenuation row that has to agree with the record's volume.
use super::*;
use crate::presentation::{append, put};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

/// The native light event class.
pub(super) const EVENT: u32 = 0x8080_6f49;
const LIGHT_CLASS: u32 = 0x8080_7140;
const PARAMETERS_CLASS: u32 = 0x8080_71dc;
const MATERIAL_CLASS: u32 = 0x8080_71e8;
const CODE: u32 = 0x8080_0009;
const VECTORS: u32 = 0x8080_0090;
const STAGES: [usize; 6] = [0x48, 0xe8, 0x188, 0x228, 0x2c8, 0x368];
/// The parameter record fields of a light record.
const PARAMETERS: [usize; 2] = [0xa0, 0xa4];
/// The stock light programs' exposure product, which the native light template whose parameters
/// write constant colors (`34 00 3C 04 40 3C 01 07 03 0E 43 05`) also applies.
const EXPOSURE: [u8; 8] = [0x3c, 4, 0x40, 0x3c, 1, 7, 0x03, 0x0e];
/// Reach light flags at `+0x02`: bit 2 first person only, bit 3 third person only.
const FIRST_PERSON_ONLY: u16 = 1 << 2;
const THIRD_PERSON_ONLY: u16 = 1 << 3;
const REC_709: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// A source firing light: an attached `ligh` scaled by `illumination`.
pub(super) struct Source {
    tag: Tag,
    /// Linear color, as Reach's analytical lights decode it.
    color: [f32; 3],
    /// Whether it draws in first and in third person.
    views: [bool; 2],
}

/// The source weapon's firing lights.
pub(super) fn sources(cache: &Cache, scene: &Scene) -> Result<Vec<Source>> {
    let mut result = Vec::new();
    for attachment in &scene.object.attachments {
        if attachment["tag"]["group"] != "ligh" || attachment["primary_scale"] != "illumination" {
            continue;
        }
        let tag: Tag = serde_json::from_value(attachment["tag"].clone())?;
        let flags = cache.u16(tag.address()? + 2)?;
        let (color, _) = crate::halo_reach::level::native::lights::color(cache, &tag)?;
        result.push(Source {
            tag,
            color,
            views: [
                flags & THIRD_PERSON_ONLY == 0,
                flags & FIRST_PERSON_ONLY == 0,
            ],
        });
    }
    Ok(result)
}

/// The source light for a light in view group `view`: one drawn only in that view, else one drawn
/// in it. A light outside the view groups takes one drawn in both views or in third person.
fn pick(sources: &[Source], view: Option<usize>) -> Option<&Source> {
    match view {
        Some(view) => sources
            .iter()
            .find(|source| source.views[view] && !source.views[1 - view])
            .or_else(|| sources.iter().find(|source| source.views[view])),
        None => sources
            .iter()
            .find(|source| source.views == [true, true])
            .or_else(|| sources.iter().find(|source| source.views[1]))
            .or(sources.first()),
    }
}

/// The operand bytes of a material expression opcode, by the widths Sundial's native expression
/// edits and the D1 translator use. Object vector `4D` takes one, as original evaluator execution
/// qualifies.
fn operands(op: u8) -> Result<usize> {
    Ok(match op {
        0x01..=0x13 | 0x15..=0x21 | 0x23 | 0x25 | 0x27..=0x2b | 0x2e => 0,
        0x22 | 0x34..=0x3b | 0x42..=0x4e => 1,
        0x3c..=0x41 => 2,
        _ => bail!("Native light expression opcode {op:02X} has no qualified width"),
    })
}

fn vector(constants: &[u8], row: usize) -> Option<[f32; 4]> {
    let bytes = constants.get(row * 16..row * 16 + 16)?;
    Some(std::array::from_fn(|lane| {
        f32::from_le_bytes([
            bytes[lane * 4],
            bytes[lane * 4 + 1],
            bytes[lane * 4 + 2],
            bytes[lane * 4 + 3],
        ])
    }))
}

/// The program `code` over `constants` with literal stores of `color` appended over every output
/// row its object vector feeds, and what each wrote, or none when no row is fed.
fn program(code: &[u8], constants: &[u8], color: [f32; 3]) -> Result<Option<LightExpression>> {
    ensure!(
        constants.len().is_multiple_of(16),
        "Native light expression constants differ"
    );
    let mut instructions: Vec<Range<usize>> = Vec::new();
    let mut at = 0;
    while at < code.len() {
        let end = at + 1 + operands(code[at])?;
        ensure!(end <= code.len(), "Native light expression is truncated");
        instructions.push(at..end);
        at = end;
    }
    let mut start = 0;
    let mut temporaries = BTreeSet::new();
    let mut rows = BTreeMap::new();
    for (index, instruction) in instructions.iter().enumerate() {
        let op = code[instruction.start];
        if !matches!(op, 0x43 | 0x44 | 0x46 | 0x47 | 0x49) {
            continue;
        }
        let fed = instructions[start..=index].iter().any(|range| {
            let op = code[range.start];
            op == 0x4d || (op == 0x45 && temporaries.contains(&code[range.start + 1]))
        });
        let target = code[instruction.start + 1];
        match op {
            0x46 if fed => {
                temporaries.insert(target);
            }
            0x46 => {
                temporaries.remove(&target);
            }
            0x43 if fed => {
                rows.insert(target, start..index + 1);
            }
            0x43 => {
                rows.remove(&target);
            }
            _ => ensure!(
                !fed,
                "Native light expression binds a resource from its object vector"
            ),
        }
        start = index + 1;
    }
    ensure!(
        start == instructions.len(),
        "Native light expression ends inside a statement"
    );
    if rows.is_empty() {
        return Ok(None);
    }
    let weights = (0..constants.len() / 16)
        .filter_map(|row| vector(constants, row))
        .find(|v| {
            (v[0] - 0.3).abs() < 1e-3
                && (v[1] - 0.59).abs() < 1e-3
                && (v[2] - 0.11).abs() < 1e-3
                && v[3] == 0.
        })
        .map_or(REC_709, |v| [v[0], v[1], v[2]]);
    let luminance = (0..3).map(|k| color[k] * weights[k]).sum::<f32>();
    ensure!(
        luminance.is_finite() && luminance > 1e-4 && color.iter().all(|v| v.is_finite()),
        "Source firing light is black"
    );
    let mut out = code.to_vec();
    let mut table = constants.to_vec();
    let mut report = Vec::new();
    for (row, statement) in rows {
        let span = &instructions[statement];
        let mut scale = 1f32;
        for pair in span.windows(2) {
            if code[pair[0].start] == 0x34
                && code[pair[1].start] == 0x03
                && let Some(value) = vector(constants, usize::from(code[pair[0].start + 1]))
                && value.iter().all(|lane| *lane == value[0])
                && value[0].is_finite()
                && value[0] > 0.
            {
                scale = value[0];
            }
        }
        let bytes = &code[span[0].start..span[span.len() - 1].end];
        let mut suffix = EXPOSURE.to_vec();
        suffix.extend([0x43, row]);
        let exposure = bytes.ends_with(&suffix);
        let constant = u8::try_from(table.len() / 16)?;
        ensure!(
            constant < u8::MAX,
            "Native light expression exceeds 255 constants"
        );
        let unit = color.map(|value| value / luminance * scale);
        let value = [unit[0], unit[1], unit[2], unit[2]];
        table.extend(value.iter().flat_map(|lane| lane.to_le_bytes()));
        out.extend([0x34, constant]);
        if exposure {
            out.extend(EXPOSURE);
        }
        out.extend([0x43, row]);
        report.push(json!({"row":row,"scale":scale,"exposure":exposure,"value":value}));
    }
    Ok(Some((out, table, report)))
}

/// Rewrites the program whose bytecode and constants descriptors sit at `code` and `constants`,
/// and returns what it wrote, or none when its object vector feeds no row.
fn rewrite(
    payload: &mut Payload,
    (code, constants): (usize, usize),
    color: [f32; 3],
) -> Result<Option<Vec<Value>>> {
    let bytes = payload.0[payload.array_range(code, 1, Some(CODE))?].to_vec();
    let table = payload.0[payload.array_range(constants, 16, Some(VECTORS))?].to_vec();
    let Some((bytes, table, rows)) = program(&bytes, &table, color)? else {
        return Ok(None);
    };
    append(&mut payload.0, code, u64::from(CODE), &bytes, 1)?;
    append(&mut payload.0, constants, u64::from(VECTORS), &table, 16)?;
    Ok(Some(rows))
}

/// Private copies of light record `record` whose programs store `color`, named under `prefix`,
/// and what they wrote, or none when no program reads its object vector. Every copy is checked
/// before any is placed.
fn recolor(
    native: &mut Reader,
    graph: &mut Graph,
    record: u32,
    color: [f32; 3],
    prefix: &str,
) -> Result<Option<(String, Value)>> {
    let mut light = (*native.tag(record, Some(LIGHT_CLASS))?).clone();
    let mut written = Vec::new();
    let material_tag = light.u32(0x90)?;
    let mut material = (*native.tag(material_tag, Some(MATERIAL_CLASS))?).clone();
    for stage in STAGES {
        if let Some(rows) = rewrite(&mut material, (stage + 0x20, stage + 0x30), color)? {
            written.push(json!({"material_stage":stage,"rows":rows}));
        }
    }
    // A container that loads only a program's header leaves its data unloaded, so a material
    // copy carries both entries of every program.
    let recolored = !written.is_empty();
    let mut programs = Vec::new();
    if recolored {
        for stage in STAGES {
            // Its sampler rows name the stock samplers the donor material already loads.
            ensure!(
                material.array(stage + 8, 8, Some(0x8080_7211))?.is_empty()
                    && matches!(material.u32(stage + 0x84)?, 0 | u32::MAX | 0x811c_9dc5),
                "Native firing light material binds its own textures or buffers"
            );
            let program = material.u32(stage)?;
            if program != u32::MAX {
                let data = native.tag(native.reference(program)?, None)?.0.clone();
                programs.push((stage, program, data));
            }
        }
    }
    let mut parameters = Vec::new();
    for field in PARAMETERS {
        let tag = light.u32(field)?;
        if tag == u32::MAX {
            continue;
        }
        let mut copy = (*native.tag(tag, Some(PARAMETERS_CLASS))?).clone();
        if let Some(rows) = rewrite(&mut copy, (0x30, 0x40), color)? {
            written.push(json!({"parameters":format!("{field:#X}"),"rows":rows}));
            parameters.push((field, tag, copy));
        }
    }
    if written.is_empty() {
        return Ok(None);
    }
    let mut patches = Vec::new();
    if recolored {
        let mut bound = Vec::new();
        for (stage, program, data) in programs {
            let symbol = format!("{prefix}-program-{program:08X}");
            if !graph.contains(&symbol) {
                super::super::material::bytecode(
                    native,
                    graph,
                    &symbol,
                    program,
                    &data,
                    "Stock firing light program, copied with its data",
                )?;
            }
            put(&mut material.0, stage, &u32::MAX.to_le_bytes())?;
            bound.push(json!({"offset":stage,"symbol":symbol}));
        }
        let symbol = format!("{prefix}-material");
        graph.add(&symbol, material_tag, &material.0, None, bound)?;
        put(&mut light.0, 0x90, &u32::MAX.to_le_bytes())?;
        patches.push(json!({"offset":0x90,"symbol":symbol}));
    }
    for (field, tag, copy) in parameters {
        let symbol = format!("{prefix}-parameters-{field:02X}");
        graph.add(&symbol, tag, &copy.0, None, Vec::new())?;
        put(&mut light.0, field, &u32::MAX.to_le_bytes())?;
        patches.push(json!({"offset":field,"symbol":symbol}));
    }
    let symbol = format!("{prefix}-record");
    graph.add(&symbol, record, &light.0, None, patches)?;
    Ok(Some((
        symbol,
        json!({"record":format!("{record:08X}"),"written":written}),
    )))
}

/// Recolors every kept light event in `lights`, each a definition offset in the rebuilt flow and
/// its view group, by the source firing light for its view. A light's record field becomes a
/// private patch. A light whose record cannot be recolored keeps the donor's, with its reason.
pub(super) fn recolor_all(
    native: &mut Reader,
    graph: &mut Graph,
    cache: &Cache,
    scene: &Scene,
    (payload, patches): (&mut Payload, &mut Vec<Value>),
    lights: &[(usize, Option<usize>)],
) -> Result<Vec<Value>> {
    let sources = sources(cache, scene)?;
    let mut reports = Vec::new();
    for (index, &(event, view)) in lights.iter().enumerate() {
        ensure!(
            payload.u32(event - 4)? == EVENT,
            "Kept firing light event class differs"
        );
        let record = payload.u32(event + 0x40)?;
        let mut report =
            json!({"event_definition":event,"view":view,"record":format!("{record:08X}")});
        let Some(source) = pick(&sources, view) else {
            report["status"] = json!("native");
            report["reason"] = json!("The source weapon attaches no firing light for this view");
            reports.push(report);
            continue;
        };
        report["source"] = json!(source.tag);
        report["source_color"] = json!(source.color);
        match recolor(
            native,
            graph,
            record,
            source.color,
            &format!("reach-firing-light-{index}"),
        ) {
            Ok(Some((symbol, written))) => {
                put(&mut payload.0, event + 0x40, &u32::MAX.to_le_bytes())?;
                patches.push(json!({"offset":event + 0x40,"symbol":symbol}));
                report["status"] = json!("translated");
                report["private"] = written;
                report["limits"] = json!([
                    "The source color takes the donor's brightness scale at unit luminance. That the donor color has unit luminance before that scale is inferred.",
                    "The donor light's reach, falloff and duration stay. The source light's range and intensity are not carried."
                ]);
            }
            Ok(None) => {
                report["status"] = json!("native");
                report["reason"] = json!("No program of the donor light reads its object vector");
            }
            Err(error) => {
                report["status"] = json!("pending");
                report["reason"] = json!(format!("{error:#}"));
            }
        }
        reports.push(report);
    }
    Ok(reports)
}

type LightExpression = (Vec<u8>, Vec<u8>, Vec<Value>);
