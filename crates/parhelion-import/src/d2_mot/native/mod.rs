//! Native material and geometry conversion for every prepared source model.
pub(crate) mod automatic;
pub mod collection;
mod contracts;
pub mod effects;
mod mesh;
pub(crate) mod shader;

use crate::d2_mot::{
    convert,
    payload::Payload,
    plates,
    reader::{outside, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

fn load(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read {}", path.display()))?,
    )?)
}
fn put(bytes: &mut [u8], at: usize, value: &[u8]) -> Result<()> {
    bytes
        .get_mut(at..at.checked_add(value.len()).context("write overflow")?)
        .context("write outside payload")?
        .copy_from_slice(value);
    Ok(())
}

pub fn compile_shader(input: &Path, out: &Path, stage: &str) -> Result<Value> {
    ensure!(
        matches!(stage, "ps_5_0" | "vs_5_0"),
        "unsupported shader stage"
    );
    ensure!(!out.exists(), "shader output already exists");
    let (code, warnings) = shader::compile(&fs::read_to_string(input)?, stage == "vs_5_0")?;
    fs::create_dir_all(out)?;
    fs::write(out.join("bytecode.bin"), &code)?;
    fs::write(out.join("compile.log"), &warnings)?;
    Ok(json!({"bytecode_bytes":code.len(),"warnings":warnings}))
}

struct Graph {
    root: PathBuf,
    manifest: Value,
}

pub fn merge_models(plan: &Path, out: &Path) -> Result<Value> {
    let input = load(plan)?;
    let entries = input["parts"].as_array().context("model parts")?;
    let mut parts = vec![];
    for entry in entries {
        let mapped = Path::new(entry["mapped"].as_str().context("mapped model")?);
        outside(out, mapped)?;
        let materials = entry["materials"]
            .as_object()
            .context("part material assignments")?
            .iter()
            .map(|(k, v)| Ok((k.clone(), v.as_str().context("material symbol")?.to_owned())))
            .collect::<Result<BTreeMap<_, _>>>()?;
        parts.push(mesh::load_raw(mapped, materials)?);
    }
    ensure!(!out.exists(), "merge output already exists");
    let result = mesh::merge(
        parts,
        u16::try_from(input["max_bone"].as_u64().context("maximum bone")?)?,
    )?;
    fs::create_dir_all(out)?;
    fs::write(out.join("model.bin"), &result.header)?;
    for (name, data) in ["positions.bin", "attributes.bin", "indices.bin"]
        .iter()
        .zip(&result.streams)
    {
        fs::write(out.join(name), data)?;
    }
    let report = json!({"patches":result.patches,"count":result.count,"max_position_error":result.position_error,"bounding_sphere_radius":result.radius});
    write_json(&out.join("merged.json"), &report)?;
    Ok(report)
}

pub fn library(
    inventory: &Path,
    baseline: &Path,
    out: &Path,
    vertex: &Path,
    pixels: &Path,
    effect_inputs: Option<(&Path, &Path)>,
) -> Result<Value> {
    let out = outside(out, baseline)?;
    if let Some((refs, bindings)) = effect_inputs {
        outside(&out, refs)?;
        outside(&out, bindings)?;
    }
    ensure!(!out.exists(), "output already exists");
    let inventory = load(inventory)?;
    let entries = inventory["graphs"]
        .as_array()
        .context("library inventory")?;
    let baseline_graphs = load(&baseline.join("graphs.json"))?;
    ensure!(
        entries.len() == baseline_graphs.as_array().context("baseline graphs")?.len(),
        "inventory omits library graphs"
    );
    for (i, entry) in entries.iter().enumerate() {
        ensure!(
            baseline_graphs[i] == entry["graph"],
            "inventory graph order differs from baseline"
        );
        let prepared = Path::new(entry["prepared"].as_str().context("source preparation")?);
        checked_output(prepared, &out)?;
    }
    fs::create_dir_all(out.join("recipes"))?;
    let mut recipe_count = 0;
    for entry in fs::read_dir(baseline.join("recipes"))? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|v| v == "json") {
            fs::copy(entry.path(), out.join("recipes").join(entry.file_name()))?;
            recipe_count += 1;
        }
    }
    ensure!(
        Some(recipe_count) == inventory["recipe_count"].as_u64(),
        "inventory omits library recipes"
    );
    let mut results = vec![];
    let mut graphs = vec![];
    for (i, entry) in entries.iter().enumerate() {
        ensure!(
            baseline_graphs[i] == entry["graph"],
            "inventory graph order differs from baseline"
        );
        let prepared = Path::new(entry["prepared"].as_str().context("source preparation")?);
        let candidate = out.join(format!("graph-{i:02}"));
        let result = build(prepared, &candidate, vertex, pixels)?;
        let path = candidate.join("graph/asset-graph.json");
        let mut converted_effects = vec![];
        if let Some((refs, bindings)) = effect_inputs {
            let bindings = entry["bindings"]
                .as_str()
                .map(Path::new)
                .unwrap_or(bindings);
            let graph = Graph {
                root: candidate.join("graph"),
                manifest: load(&path)?,
            };
            converted_effects.push(effects::apply(
                prepared,
                graph,
                refs,
                bindings,
                &candidate,
                "source-all",
            )?);
        }
        let mut manifest = load(&path)?;
        let original = load(
            &Path::new(entry["graph"].as_str().context("baseline graph")?).join("asset-graph.json"),
        )?;
        ensure!(
            manifest["item_hash"] == original["item_hash"]
                && manifest["item_hash"] == entry["item_hash"],
            "candidate changes library identity"
        );
        if let Some(ornament) = original.get("ornament") {
            manifest["ornament"] = ornament.clone();
        }
        write_json(&path, &manifest)?;
        graphs.push(candidate.join("graph"));
        let mut pending = vec![];
        for model in entry["models"].as_array().context("inventory models")? {
            for surface in model["surfaces"].as_array().context("inventory surfaces")? {
                let stage = surface["stage"].as_u64().context("source draw stage")?;
                if stage == 23 {
                    continue;
                }
                if effect_inputs.is_some() && matches!(stage, 0 | 1 | 3 | 7 | 9 | 12) {
                    continue;
                }
                pending.push(json!({"model":model["model"],"surface":surface}));
            }
        }
        results.push(json!({"index":i,"name":entry["name"],"result":result,"effects":converted_effects,"unsupported_surfaces":pending}));
        eprintln!(
            "{}/{} native library graphs converted",
            i + 1,
            entries.len()
        );
        write_json(&out.join("progress.json"), &json!(results))?;
    }
    write_json(&out.join("graphs.json"), &json!(graphs))?;
    let result = json!({"recipe_count":recipe_count,"graph_count":graphs.len(),"graphs":results,
        "recipes_preserved":true,"implementation":"Rust","all_surface_families_supported":results.iter().all(|v|v["unsupported_surfaces"].as_array().is_some_and(Vec::is_empty)),"installable":false,"gameplay_verified":false});
    write_json(&out.join("coverage.json"), &result)?;
    Ok(result)
}

impl Graph {
    fn node(&self, symbol: &str) -> Result<&Value> {
        self.manifest["nodes"]
            .as_array()
            .context("graph nodes")?
            .iter()
            .find(|n| n["symbol"] == symbol)
            .with_context(|| format!("graph symbol missing: {symbol}"))
    }
    fn node_mut(&mut self, symbol: &str) -> Result<&mut Value> {
        self.manifest["nodes"]
            .as_array_mut()
            .context("graph nodes")?
            .iter_mut()
            .find(|n| n["symbol"] == symbol)
            .with_context(|| format!("graph symbol missing: {symbol}"))
    }
    fn path(&self, symbol: &str) -> Result<PathBuf> {
        let name = self.node(symbol)?["file"].as_str().context("node file")?;
        ensure!(
            Path::new(name).components().count() == 1 && Path::new(name).file_name().is_some(),
            "graph file is not a flat payload name"
        );
        Ok(self.root.join(name))
    }
    fn read(&self, symbol: &str) -> Result<Payload> {
        Ok(Payload(fs::read(self.path(symbol)?)?))
    }
    fn write(&self, symbol: &str, data: &[u8]) -> Result<()> {
        Ok(fs::write(self.path(symbol)?, data)?)
    }
    fn add(
        &mut self,
        symbol: &str,
        template: u64,
        data: &[u8],
        reference: Option<&str>,
        patches: Vec<Value>,
    ) -> Result<()> {
        ensure!(
            self.node(symbol).is_err(),
            "duplicate graph symbol {symbol}"
        );
        let file = format!("{symbol}.bin");
        fs::write(self.root.join(&file), data)?;
        self.manifest["nodes"].as_array_mut().context("graph nodes")?.push(json!({"symbol":symbol,"file":file,"template":template,"reference":reference,"patches":patches}));
        Ok(())
    }
    fn program(
        &mut self,
        name: &str,
        text: &str,
        reference: &Path,
        vertex: bool,
        out: &Path,
    ) -> Result<u64> {
        fs::write(out.join(format!("{name}.hlsl")), text)?;
        let (code, warnings) =
            shader::compile(text, vertex).with_context(|| format!("converted shader {name}"))?;
        fs::write(out.join(format!("{name}-compile.log")), warnings)?;
        let meta = load(&reference.with_extension("meta.json"))?;
        let mut header = fs::read(reference.with_extension("header.bin"))?;
        put(&mut header, 8, &u32::try_from(code.len())?.to_le_bytes())?;
        let hs = format!("{name}-shader");
        let ds = format!("{name}-bytecode");
        self.add(
            &hs,
            meta["tag"].as_u64().context("shader tag")?,
            &header,
            Some(&ds),
            vec![],
        )?;
        self.add(
            &ds,
            meta["reference"].as_u64().context("shader reference")?,
            &code,
            Some(&hs),
            vec![],
        )?;
        meta["tag"].as_u64().context("shader tag")
    }
}

fn checked_output(prepared: &Path, out: &Path) -> Result<PathBuf> {
    let out = outside(out, prepared)?;
    for folder in ["source", "native"] {
        let manifest = load(&prepared.join(folder).join("source-manifest.json"))?;
        let packages = Path::new(
            manifest["packages"]
                .as_str()
                .context("source package path")?,
        );
        outside(&out, packages.parent().context("package parent")?)?;
    }
    ensure!(!out.exists(), "output already exists");
    Ok(out)
}

pub fn build(prepared: &Path, out: &Path, vertex: &Path, pixels: &Path) -> Result<Value> {
    let source = prepared.join("source");
    let native = prepared.join("native");
    build_with_sources(prepared, out, vertex, pixels, &source, &native, &mut |_| {})
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
pub(crate) fn build_with_sources(
    prepared: &Path,
    out: &Path,
    vertex: &Path,
    pixels: &Path,
    source: &Path,
    native: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<Value> {
    let out = outside(out, prepared)?;
    for folder in [source, native] {
        outside(&out, folder)?;
        let manifest = load(&folder.join("source-manifest.json"))?;
        let packages = Path::new(
            manifest["packages"]
                .as_str()
                .context("source package path")?,
        );
        outside(&out, packages.parent().context("package parent")?)?;
    }
    ensure!(!out.exists(), "output already exists");
    progress("Composing textures for all source model pieces…".into());
    let report = load(&source.join("report.json"))?;
    let models = report["models"].as_array().context("source models")?;
    let rig = prepared.join("rig_mapping.json");
    let rig = if rig.exists() {
        Some(load(&rig)?)
    } else {
        None
    };
    let bones = rig
        .as_ref()
        .map(|r| -> Result<Vec<u16>> {
            r["bone_map"]
                .as_array()
                .context("bone map")?
                .iter()
                .map(|n| Ok(u16::try_from(n.as_u64().context("bone index")?)?))
                .collect()
        })
        .transpose()?;
    let max_bone = rig
        .as_ref()
        .map(|r| -> Result<u16> {
            Ok(u16::try_from(
                r["native_bone_count"]
                    .as_u64()
                    .context("native bone count")?
                    .checked_sub(1)
                    .context("empty rig")?,
            )?)
        })
        .transpose()?
        .unwrap_or(0);
    ensure!(
        bones.is_some() || models.iter().all(|m| m["rigid_bone_zero"] == true),
        "animated source requires a validated native rig map"
    );
    fs::create_dir_all(&out)?;
    let plate_dir = out.join("plates");
    let plate_report = plates::build(source, &plate_dir)?;
    let size = [
        plate_report["atlas_size"][0]
            .as_u64()
            .context("atlas width")? as usize,
        plate_report["atlas_size"][1]
            .as_u64()
            .context("atlas height")? as usize,
    ];
    let rectangles = plate_report["source_rectangles"]
        .as_array()
        .context("atlas rectangles")?
        .iter()
        .map(|r| -> Result<[usize; 4]> {
            Ok([
                r[0].as_u64().context("rectangle x")? as usize,
                r[1].as_u64().context("rectangle y")? as usize,
                r[2].as_u64().context("rectangle width")? as usize,
                r[3].as_u64().context("rectangle height")? as usize,
            ])
        })
        .collect::<Result<Vec<_>>>()?;
    let original = prepared.join("material-graph");
    let mut graph = Graph {
        root: out.join("graph"),
        manifest: load(&original.join("asset-graph.json"))?,
    };
    fs::create_dir_all(&graph.root)?;
    for node in graph.manifest["nodes"].as_array().context("graph nodes")? {
        let symbol = node["symbol"].as_str().context("graph symbol")?;
        fs::copy(
            original.join(node["file"].as_str().context("graph file")?),
            graph.path(symbol)?,
        )?;
    }
    // Prepared graphs may predate resident texture conversion. Normalize their
    // private detail textures before using them as donors for new resources.
    for channel in 4..=6 {
        for slot in 0..2 {
            let name = format!("dye-{channel}-texture-{slot}");
            // Undyed sources do not create private dye textures. Normalize
            // only present pairs and retain errors for incomplete resources.
            if graph.node(&name).is_err() && graph.node(&format!("{name}-data")).is_err() {
                continue;
            }
            let mut header = graph.read(&name)?.0;
            let data = graph.read(&format!("{name}-data"))?;
            crate::d2_mot::texture::resident(&mut header, data.0.len())?;
            graph.write(&name, &header)?;
        }
    }
    for texture in plate_report["textures"]
        .as_array()
        .context("atlas textures")?
    {
        let name = texture["name"].as_str().context("texture name")?;
        let data = fs::read(plate_dir.join(texture["file"].as_str().context("texture file")?))?;
        graph.write(&format!("texture-{name}-data"), &data)?;
        let mut h = graph.read(&format!("texture-{name}-header"))?.0;
        put(&mut h, 0, &u32::try_from(data.len())?.to_le_bytes())?;
        put(
            &mut h,
            4,
            &u32::try_from(texture["format"].as_u64().context("texture format")?)?.to_le_bytes(),
        )?;
        for (axis, value) in size.iter().enumerate() {
            put(&mut h, 14 + axis * 2, &u16::try_from(*value)?.to_le_bytes())?;
        }
        h[23] = u8::try_from(
            plate_report["retained_mips"]
                .as_u64()
                .context("mip count")?,
        )?;
        crate::d2_mot::texture::resident(&mut h, data.len())?;
        graph.write(&format!("texture-{name}-header"), &h)?;
        let mut h = graph.read(&format!("plate-{name}"))?.0;
        for (axis, value) in size.iter().enumerate() {
            put(
                &mut h,
                0x4C + axis * 4,
                &u32::try_from(*value)?.to_le_bytes(),
            )?;
        }
        graph.write(&format!("plate-{name}"), &h)?;
    }
    let mut parts = vec![];
    let mut detail_error = 0f64;
    let mut omitted = BTreeSet::new();
    for (i, rectangle) in rectangles.iter().enumerate() {
        progress(format!(
            "Assembling model piece {} of {}…",
            i + 1,
            rectangles.len()
        ));
        let mapped = out.join(format!("mapped-{i}"));
        convert::convert_mapped(source, native, &mapped, i, true, bones.as_deref())?;
        let mapping = load(&mapped.join("mapping.json"))?;
        let mut materials = BTreeMap::new();
        let mut created: BTreeMap<u64, String> = BTreeMap::new();
        for group in mapping["plated_groups"].as_array().context("draw groups")? {
            if group["stage"] != 0 {
                continue;
            }
            let channel = group["channel"].as_u64().context("dye channel")?;
            let donor = match channel {
                0 | 1 => 0x80EC270D,
                2 => 0x80EC2713,
                3 => 0x80EC270C,
                4 | 5 => 0x80EC2710,
                _ => anyhow::bail!("unsupported dye channel"),
            };
            let name = if let Some(name) = created.get(&donor) {
                name.clone()
            } else {
                let symbol = format!("material-plated-{donor:08X}-stage-0");
                if graph.node(&symbol).is_err() {
                    let data = fs::read(mapped.join(format!("{symbol}.bin")))?;
                    graph.add(&symbol, donor, &data, None, vec![])?;
                }
                let mut payload = graph.read(&symbol)?;
                let shader_tag = payload.u32(0x2C8)?;
                let reference = pixels.join(format!("{shader_tag:08X}.hlsl"));
                let text = shader::pixel(&fs::read_to_string(&reference)?, *rectangle, size)?;
                let name = format!("material-plated-{donor:08X}-model-{i}-stage-0");
                graph.program(&name, &text, &reference, false, &out)?;
                put(&mut payload.0, 0x2C8, &u32::MAX.to_le_bytes())?;
                graph.add(
                    &name,
                    donor,
                    &payload.0,
                    None,
                    vec![json!({"offset":0x2C8,"symbol":format!("{name}-shader")})],
                )?;
                created.insert(donor, name.clone());
                name
            };
            materials.insert(
                format!(
                    "{}:{channel}",
                    group["source_material"]
                        .as_str()
                        .context("source material")?
                ),
                name,
            );
        }
        let (part, error) = mesh::load_part(&mapped, materials, *rectangle, size)?;
        detail_error = detail_error.max(error);
        parts.push(part);
        for stage in mapping["omitted_effect_stages"]
            .as_array()
            .context("omitted stages")?
        {
            omitted.insert(stage.as_u64().context("stage")?);
        }
    }
    let merged = mesh::merge(parts, max_bone)?;
    graph.write("model", &merged.header)?;
    graph.node_mut("model")?["patches"] = json!(merged.patches);
    for (i, name) in ["positions", "attributes", "indices"].iter().enumerate() {
        graph.write(&format!("{name}-data"), &merged.streams[i])?;
        let mut h = graph.read(&format!("{name}-header"))?.0;
        put(
            &mut h,
            if i == 2 { 8 } else { 0 },
            &u32::try_from(merged.streams[i].len())?.to_le_bytes(),
        )?;
        graph.write(&format!("{name}-header"), &h)?;
    }
    let text = shader::vertex(&fs::read_to_string(vertex)?)?;
    let vertex_tag = graph.program("atlas-vertex", &text, vertex, true, &out)?;
    // Keep the established vertex symbols for the separate surface adapters.
    for (old, new) in [
        ("atlas-vertex-shader", "atlas-vertex-header"),
        ("atlas-vertex-bytecode", "atlas-vertex-data"),
    ] {
        graph.node_mut(old)?["symbol"] = json!(new);
    }
    graph.node_mut("atlas-vertex-header")?["reference"] = json!("atlas-vertex-data");
    graph.node_mut("atlas-vertex-data")?["reference"] = json!("atlas-vertex-header");
    let names = graph.manifest["nodes"]
        .as_array()
        .context("nodes")?
        .iter()
        .filter_map(|n| n["symbol"].as_str())
        .filter(|s| s.starts_with("material-plated-") && s.ends_with("-stage-0"))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for name in names {
        let mut h = graph.read(&name)?;
        ensure!(
            h.u32(0x48)? as u64 == vertex_tag,
            "material vertex contract differs"
        );
        put(&mut h.0, 0x48, &u32::MAX.to_le_bytes())?;
        graph.write(&name, &h.0)?;
        graph.node_mut(&name)?["patches"]
            .as_array_mut()
            .context("material patches")?
            .push(json!({"offset":0x48,"symbol":"atlas-vertex-header"}));
    }
    graph.manifest["native_draw_parts"] = json!(merged.count);
    graph.manifest["source_models"] = json!(
        models
            .iter()
            .map(|m| m["model"].clone())
            .collect::<Vec<_>>()
    );
    graph.manifest["appearance"] =
        json!("Source geometry and composed plates using native rendering scopes");
    graph.manifest["installable"] = json!(false);
    graph.manifest["attachment_adapter"] = json!({"atlas_size":size,"source_rectangles":rectangles,
        "source_atlas_size":plate_report["source_atlas_size"],"dropped_top_mips":plate_report["dropped_top_mips"],
        "retained_mips":plate_report["retained_mips"],"atlas_required":plate_report["atlas_required"],
        "max_position_error":merged.position_error,"max_detail_uv_error":detail_error,"bounding_sphere_radius":merged.radius,
        "native_bone_map_retained":rig.is_some(),"source_plate_placements_composed":true,
        "native_clamp_sampling_preserved":true,"omitted_stages":omitted,"implementation":"Rust","gameplay_verified":false});
    write_json(&graph.root.join("asset-graph.json"), &graph.manifest)?;
    Ok(
        json!({"graph":graph.root,"vertices":merged.streams[0].len()/8,"draw_parts":merged.count,
        "adapter":graph.manifest["attachment_adapter"]}),
    )
}
