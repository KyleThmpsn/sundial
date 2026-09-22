//! Add inspected native decal and reflection families to converted geometry.
mod atmosphere;
mod channels;
mod decals;
mod dyemap;
mod emission;
mod inputs;
mod layout;
mod lighting;
mod null_glow;
mod procedural;
mod reflection;
mod shader;
mod source;
mod vertex;
use super::{Graph, checked_output, load, put};
use crate::d2_mot::{
    geometry,
    payload::Payload,
    reader::{outside, write_json},
    tfx::program::{self, Bindings},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub fn build(
    prepared: &Path,
    previous: &Path,
    refs: &Path,
    bindings: &Path,
    out: &Path,
    family: &str,
) -> Result<Value> {
    build_with_progress(prepared, previous, refs, bindings, out, family, &mut |_| {})
}

pub(crate) fn build_with_progress(
    prepared: &Path,
    previous: &Path,
    refs: &Path,
    bindings: &Path,
    out: &Path,
    family: &str,
    progress: &mut dyn FnMut(String),
) -> Result<Value> {
    ensure!(
        matches!(
            family,
            "decals"
                | "reflections"
                | "dyemap-reflections"
                | "emission"
                | "source-opaque"
                | "source-transparent"
                | "source-all"
        ),
        "unsupported effect family"
    );
    let out = checked_output(prepared, out)?;
    for input in [previous, refs, bindings] {
        outside(&out, input)?;
    }
    let mut graph = Graph {
        root: previous.join("graph"),
        manifest: load(&previous.join("graph/asset-graph.json"))?,
    };
    // Resolve and read every flat graph payload before creating the output.
    let mut files = vec![];
    for node in graph.manifest["nodes"].as_array().context("graph nodes")? {
        let path = graph.path(node["symbol"].as_str().context("node symbol")?)?;
        files.push((
            path.file_name().context("node filename")?.to_owned(),
            fs::read(path)?,
        ));
    }
    graph.root = out.join("graph");
    fs::create_dir_all(&graph.root)?;
    for (name, bytes) in files {
        fs::write(graph.root.join(name), bytes)?;
    }
    apply_with_progress(prepared, graph, refs, bindings, &out, family, progress)
}

pub(super) fn apply(
    prepared: &Path,
    graph: Graph,
    refs: &Path,
    bindings: &Path,
    out: &Path,
    family: &str,
) -> Result<Value> {
    apply_with_progress(prepared, graph, refs, bindings, out, family, &mut |_| {})
}

fn apply_with_progress(
    prepared: &Path,
    graph: Graph,
    refs: &Path,
    bindings: &Path,
    out: &Path,
    family: &str,
    progress: &mut dyn FnMut(String),
) -> Result<Value> {
    ensure!(
        matches!(
            family,
            "decals"
                | "reflections"
                | "dyemap-reflections"
                | "emission"
                | "source-opaque"
                | "source-transparent"
                | "source-all"
        ),
        "unsupported effect family"
    );
    let owner = u32::try_from(
        graph.manifest["source_owner"]
            .as_u64()
            .context("native owner")?,
    )?;
    let objects = program::native_channels(&prepared.join("native"), Some(owner))?;
    let modern = load(&refs.join("tfx-modern/context.json"))?;
    let native = load(&refs.join("tfx-native/context.json"))?;
    let mut globals = BTreeMap::new();
    for row in modern["channels"].as_array().context("modern globals")? {
        if let Some(n) = native["channels"]
            .as_array()
            .context("native globals")?
            .iter()
            .find(|n| n["hash"] == row["hash"])
        {
            globals.insert(
                u8::try_from(row["index"].as_u64().context("global index")?)?,
                u8::try_from(n["index"].as_u64().context("native index")?)?,
            );
        }
    }
    let draws = Draws::read(&graph)?;
    let source = Source::read(&prepared.join("source"))?;
    let mut context = Effect {
        graph,
        draws,
        source,
        objects,
        globals,
        global_defaults: modern["channels"].clone(),
        out: out.to_owned(),
        refs: refs.to_owned(),
        bindings: load(&bindings.join("bindings.json"))?,
        bindings_root: bindings.to_owned(),
        variable: None,
        contracts: if family.starts_with("source-") {
            Some(super::contracts::Catalog::read(refs)?)
        } else {
            None
        },
    };
    if family == "source-all" {
        progress("Adapting runtime material controls…".into());
        channels::bind(&mut context, prepared).context("source channel adaptation")?;
        for (index, (stage, label)) in [
            (0, "surface shading"),
            (1, "decals"),
            (7, "transparency"),
            (9, "light shafts"),
        ]
        .into_iter()
        .enumerate()
        {
            progress(format!("Converting source {label} ({} of 6)…", index + 1));
            source::build(&mut context, prepared, stage)
                .with_context(|| format!("source stage {stage}"))?;
        }
        for (index, (stage, label)) in [(3, "shadows"), (12, "depth")].into_iter().enumerate() {
            progress(format!("Converting source {label} ({} of 6)…", index + 5));
            source::auxiliary(&mut context, prepared, stage)?;
        }
        context.graph.manifest["attachment_adapter"]["omitted_stages"] = json!([]);
        context.graph.manifest["source_shader_adapter"] = json!({"stages":[0,1,3,7,9,12],"source_compute_skinning":"converted to native vertex skinning with validated bone indices","source_shader_equations_retained":context.graph.manifest["native_forward_lighting"].is_null(),"source_material_equations_retained":true,"implementation":"Rust","gameplay_verified":false});
        context.graph.manifest["appearance"] =
            json!("Source geometry and shader equations using native rendering bindings");
    } else if family == "source-opaque" {
        channels::bind(&mut context, prepared)?;
        source::build(&mut context, prepared, 0)?;
    } else if family == "source-transparent" {
        channels::bind(&mut context, prepared)?;
        for stage in [7, 9] {
            source::build(&mut context, prepared, stage)?;
        }
    } else if family == "emission" {
        emission::build(&mut context)?;
    } else if family == "dyemap-reflections" {
        dyemap::build(&mut context, prepared)?;
    } else if family == "decals" {
        decals::build(&mut context)?;
    } else {
        reflection::build(&mut context, &prepared.join("native"))?;
    }
    context.draws.write(&mut context.graph)?;
    layout::finish(&mut context.graph)?;
    write_json(
        &context.graph.root.join("asset-graph.json"),
        &context.graph.manifest,
    )?;
    Ok(
        json!({"family":family,"nodes":context.graph.manifest["nodes"].as_array().unwrap().len(),"native_draw_parts":context.graph.manifest["native_draw_parts"],"gameplay_verified":false}),
    )
}

struct Effect {
    contracts: Option<super::contracts::Catalog>,
    variable: Option<String>,
    graph: Graph,
    draws: Draws,
    source: Source,
    objects: BTreeMap<String, u8>,
    globals: BTreeMap<u8, u8>,
    global_defaults: Value,
    out: PathBuf,
    refs: PathBuf,
    bindings: Value,
    bindings_root: PathBuf,
}
impl Effect {
    fn contracts(&self) -> Result<&super::contracts::Catalog> {
        self.contracts
            .as_ref()
            .context("Discovered native rendering contracts are missing")
    }
    fn atlas(&self, model: usize) -> Result<([usize; 4], [usize; 2])> {
        let adapter = &self.graph.manifest["attachment_adapter"];
        let r = &adapter["source_rectangles"][model];
        let s = &adapter["atlas_size"];
        Ok((
            [
                number(&r[0])?,
                number(&r[1])?,
                number(&r[2])?,
                number(&r[3])?,
            ],
            [number(&s[0])?, number(&s[1])?],
        ))
    }
    fn lower(
        &self,
        code: &[u8],
        constants: usize,
        outputs: usize,
        resources: usize,
    ) -> Result<program::Lowered> {
        let lowered = program::lower(
            code,
            &Bindings {
                objects: self.objects.clone(),
                globals: self.globals.clone(),
                constant_count: constants,
                output_count: outputs,
                sampler_count: resources,
                textures: if resources == 8 {
                    BTreeMap::from([(6, 7)])
                } else {
                    BTreeMap::new()
                },
                ..Default::default()
            },
        )?;
        ensure!(
            lowered.samplers.is_empty() && lowered.evidence.iter().all(|v| v["translated"] == true),
            "source effect controls require unsupported expressions"
        );
        Ok(lowered)
    }
}

fn number(value: &Value) -> Result<usize> {
    Ok(usize::try_from(
        value.as_u64().context("expected integer")?,
    )?)
}
fn tag(value: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().context("expected tag")?,
        16,
    )?)
}
fn array_bytes(payload: &Payload, at: usize, stride: usize) -> Result<Vec<u8>> {
    Ok(payload
        .array(at, stride, None)?
        .iter()
        .flat_map(|at| payload.0[*at..*at + stride].iter().copied())
        .collect())
}
fn append_array(
    data: &mut Vec<u8>,
    at: usize,
    class: u64,
    rows: &[u8],
    stride: usize,
) -> Result<()> {
    ensure!(
        stride > 0 && rows.len().is_multiple_of(stride),
        "array stride differs"
    );
    if rows.is_empty() {
        put(data, at, &[0; 16])?;
        return Ok(());
    }
    let header = (data.len() + 19) & !15;
    data.resize(header - 4, 0);
    data.extend(0x80809FBDu32.to_le_bytes());
    data.extend((rows.len() as u64 / stride as u64).to_le_bytes());
    data.extend(class.to_le_bytes());
    data.extend(rows);
    put(data, at, &(rows.len() as u64 / stride as u64).to_le_bytes())?;
    put(
        data,
        at + 8,
        &(i64::try_from(header)? - i64::try_from(at + 8)?).to_le_bytes(),
    )?;
    let len = data.len() as u64;
    put(data, 0, &len.to_le_bytes())
}
fn vectors(rows: &Value) -> Result<Vec<u8>> {
    let mut data = vec![];
    for row in rows.as_array().context("constant vectors")? {
        let values = row.as_array().context("constant vector")?;
        ensure!(values.len() == 4, "constant vector width");
        for v in values {
            let f = v.as_f64().context("constant value")? as f32;
            ensure!(f.is_finite(), "nonfinite effect constant");
            data.extend(f.to_le_bytes());
        }
    }
    Ok(data)
}

/// Check source blends and material programs before native donor selection.
/// Runtime input bindings still require the selected native render owner.
pub(crate) fn check_source_blends(root: &Path) -> Result<()> {
    let source = Source::read(root)?;
    let mut checked = std::collections::BTreeSet::new();
    for stage in [0, 1, 7, 9] {
        for draw in source.draws(stage)? {
            ensure!(
                source::supported_blend(stage, source.raw(&draw.material)?.u8(48)? & 127),
                "source stage {stage}: source blend differs"
            );
            if checked.insert(draw.material.clone()) {
                let material = source.raw(&draw.material)?;
                for base in [0x70, 0x2B0] {
                    source::preflight(&material, base).with_context(|| {
                        format!("Source material {} program {base:#x}", draw.material)
                    })?;
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn check_channels(source: &Path, native: &Path, owner: u32) -> Result<()> {
    channels::preflight(source, native, owner)
}

struct Source {
    root: PathBuf,
    report: Value,
    manifest: Value,
}
struct SourceDraw {
    model: usize,
    model_tag: String,
    material: String,
    record: Vec<u8>,
    groups: Vec<(u8, Vec<[u32; 3]>)>,
    base: usize,
}
impl Source {
    fn read(root: &Path) -> Result<Self> {
        Ok(Self {
            root: root.to_owned(),
            report: load(&root.join("report.json"))?,
            manifest: load(&root.join("source-manifest.json"))?,
        })
    }
    fn raw(&self, tag: &str) -> Result<Payload> {
        let path = self.root.join(format!("raw/{tag}.bin"));
        Ok(Payload(fs::read(&path).with_context(|| {
            format!("source payload {}", path.display())
        })?))
    }
    fn buffer(&self, tag: u32) -> Result<Payload> {
        let r = self.manifest["tags"][format!("{tag:08X}")]["reference"]
            .as_u64()
            .context("buffer reference")?;
        self.raw(&format!("{r:08X}"))
    }
    fn draws(&self, stage: usize) -> Result<Vec<SourceDraw>> {
        let mut result = vec![];
        let mut base = 0;
        for (mi, entry) in self.report["models"]
            .as_array()
            .context("source models")?
            .iter()
            .enumerate()
        {
            let model_tag = entry["model"].as_str().context("source model")?;
            let h = self.raw(model_tag)?;
            let meshes = h.array(16, 128, None)?;
            ensure!(
                meshes.len() == 1,
                "effect source requires single-mesh models"
            );
            let mesh = meshes[0];
            let vertices = self.buffer(h.u32(mesh)?)?;
            ensure!(
                vertices.0.len().is_multiple_of(24),
                "effect vertex stride differs"
            );
            let indices = self.buffer(h.u32(mesh + 16)?)?;
            let rows = h.array(mesh + 32, 36, None)?;
            let range =
                h.u16(mesh + 48 + 2 * stage)? as usize..h.u16(mesh + 50 + 2 * stage)? as usize;
            for &row in rows.get(range).context("source stage outside draw table")? {
                let start = h.u32(row + 8)? as usize;
                let count = h.u32(row + 12)? as usize;
                let input = (start..start.checked_add(count).context("source index overflow")?)
                    .map(|i| Ok(indices.u16(i * 2)? as u32))
                    .collect::<Result<Vec<_>>>()?;
                let mut groups: Vec<(u8, Vec<[u32; 3]>)> = vec![];
                for face in geometry::triangles(&input, vertices.0.len() / 24, 65535)? {
                    let channel = (vertices.u16(face[0] as usize * 24 + 14)? & 7) as u8;
                    ensure!(
                        face.iter().all(|v| vertices
                            .u16(*v as usize * 24 + 14)
                            .is_ok_and(|w| w & 7 == channel as u16)),
                        "effect triangle crosses dye selectors"
                    );
                    if let Some((_, faces)) = groups.iter_mut().find(|(c, _)| *c == channel) {
                        faces.push(face);
                    } else {
                        groups.push((channel, vec![face]));
                    }
                }
                result.push(SourceDraw {
                    model: mi,
                    model_tag: model_tag.to_owned(),
                    material: format!("{:08X}", h.u32(row)?),
                    record: h.0[row..row + 36].to_vec(),
                    groups,
                    base,
                });
            }
            base += vertices.0.len() / 24;
        }
        Ok(result)
    }
}

struct Draws {
    header: Vec<u8>,
    patches: Vec<Value>,
    indices: Vec<u8>,
    records: Vec<Vec<(Vec<u8>, String)>>,
}

impl Draws {
    fn read(g: &Graph) -> Result<Self> {
        let h = g.read("model")?;
        let rows = h.array(0xC8, 32, None)?;
        let patches = g.node("model")?["patches"]
            .as_array()
            .context("model patches")?;
        let mut records = vec![];
        for stage in 0..23 {
            let range = h.u16(0xD8 + stage * 2)? as usize..h.u16(0xDA + stage * 2)? as usize;
            let mut parts = vec![];
            for &row in rows.get(range).context("native stage outside draw table")? {
                let symbol = patches
                    .iter()
                    .find(|v| v["offset"].as_u64() == Some(row as u64))
                    .context("missing draw patch")?["symbol"]
                    .as_str()
                    .context("draw symbol")?;
                parts.push((h.0[row..row + 32].to_vec(), symbol.to_owned()));
            }
            records.push(parts);
        }
        Ok(Self {
            header: h.0.get(..0x150).context("native model header")?.to_vec(),
            patches: patches
                .iter()
                .filter(|p| p["offset"].as_u64().is_some_and(|n| n < 0x150))
                .cloned()
                .collect(),
            indices: g.read("indices-data")?.0,
            records,
        })
    }
    fn add(
        &mut self,
        stage: usize,
        donor: &[u8],
        draw: &SourceDraw,
        channel: u8,
        faces: &[[u32; 3]],
        symbol: &str,
    ) -> Result<()> {
        ensure!(
            donor.len() == 32 && channel <= 5,
            "invalid native effect draw"
        );
        let mut record = donor.to_vec();
        record[4..24].copy_from_slice(&draw.record[4..24]);
        record[26..30].copy_from_slice(&draw.record[28..32]);
        record[26] = channel;
        // Each split dye draw is its own group, regardless of the source group size.
        record[29] = 1;
        put(&mut record, 0, &u32::MAX.to_le_bytes())?;
        put(
            &mut record,
            8,
            &u32::try_from(self.indices.len() / 2)?.to_le_bytes(),
        )?;
        for face in faces {
            for v in face {
                let index = u16::try_from(*v as usize + draw.base)?;
                ensure!(index != 65535, "effect vertex collides with strip restart");
                self.indices.extend(index.to_le_bytes());
            }
            self.indices.extend(65535u16.to_le_bytes());
        }
        put(
            &mut record,
            12,
            &u32::try_from(faces.len() * 4)?.to_le_bytes(),
        )?;
        put(&mut record, 16, &u32::try_from(faces.len())?.to_le_bytes())?;
        self.records[stage].push((record, symbol.to_owned()));
        Ok(())
    }
    fn layout(&mut self, stage: usize) -> Result<()> {
        put(
            &mut self.header,
            0x108 + stage * 2,
            &(if self.records[stage].is_empty() {
                -1i16
            } else {
                139
            })
            .to_le_bytes(),
        )
    }
    fn write(&self, g: &mut Graph) -> Result<()> {
        let mut header = self.header.clone();
        let mut patches = self.patches.clone();
        let mut total = 0usize;
        for (stage, records) in self.records.iter().enumerate() {
            put(
                &mut header,
                0xD8 + stage * 2,
                &u16::try_from(total)?.to_le_bytes(),
            )?;
            for (record, symbol) in records {
                let mut record = record.clone();
                put(&mut record, 22, &u16::try_from(total)?.to_le_bytes())?;
                patches.push(json!({"offset":header.len(),"symbol":symbol}));
                header.extend(record);
                total += 1;
            }
        }
        put(
            &mut header,
            0xD8 + 23 * 2,
            &u16::try_from(total)?.to_le_bytes(),
        )?;
        let len = header.len() as u64;
        put(&mut header, 0, &len.to_le_bytes())?;
        for at in [0xC8, 0x140] {
            put(&mut header, at, &(total as u64).to_le_bytes())?;
        }
        g.write("model", &header)?;
        g.node_mut("model")?["patches"] = json!(patches);
        g.write("indices-data", &self.indices)?;
        let mut ih = g.read("indices-header")?.0;
        put(
            &mut ih,
            8,
            &u32::try_from(self.indices.len())?.to_le_bytes(),
        )?;
        g.write("indices-header", &ih)?;
        g.manifest["native_draw_parts"] = json!(total);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "Requires PARHELION_IMPORT_CASES with source/native export paths and render owners"]
    fn configured_exports_pass_early_validation() {
        let path = std::env::var_os("PARHELION_IMPORT_CASES")
            .expect("Set PARHELION_IMPORT_CASES to a JSON array of exported test cases");
        let cases = super::load(Path::new(&path)).unwrap();
        for case in cases.as_array().expect("test cases") {
            let source = Path::new(case["source"].as_str().unwrap());
            let native = Path::new(case["native"].as_str().unwrap());
            super::check_source_blends(source)
                .unwrap_or_else(|e| panic!("Source {}: {e:#}", source.display()));
            super::check_channels(
                source,
                native,
                u32::try_from(case["owner"].as_u64().unwrap()).unwrap(),
            )
            .unwrap_or_else(|e| panic!("Channels {}: {e:#}", source.display()));
        }
    }

    use super::*;

    #[test]
    fn split_effect_draws_are_all_visited_by_native_group_iteration() {
        let mut source = SourceDraw {
            model: 0,
            model_tag: String::new(),
            material: String::new(),
            record: vec![0; 36],
            groups: Vec::new(),
            base: 10,
        };
        source.record[31] = 4;
        let mut draws = Draws {
            header: vec![0; 0x150],
            patches: Vec::new(),
            indices: Vec::new(),
            records: vec![Vec::new(); 23],
        };
        for channel in 0..3 {
            draws
                .add(1, &[0; 32], &source, channel, &[[0, 1, 2]], "material")
                .unwrap();
        }
        let mut visited = Vec::new();
        let mut index = 0;
        while index < draws.records[1].len() {
            let record = &draws.records[1][index].0;
            visited.push(record[26]);
            assert_ne!(record[29], 0);
            index += usize::from(record[29]);
        }
        assert_eq!(visited, [0, 1, 2]);
        let indices: Vec<_> = draws
            .indices
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(indices, [10, 11, 12, 65535].repeat(3));
    }
}
