//! Configured package, graph and GPU readback oracle written before enrollment.
use super::*;
use std::{fs, path::PathBuf, process::Command};

#[test]
#[ignore = "requires configured source/native packages, geometry corpus and GPU readback probe"]
fn geometry_graph_oracle() -> Result<()> {
    let path = |name: &str| -> Result<PathBuf> { Ok(PathBuf::from(std::env::var(name)?)) };
    let root = path("PARHELION_PARTICLE_GEOMETRY_ROOT")?.canonicalize()?;
    let corpus = path("PARHELION_PARTICLE_GEOMETRY_CORPUS")?.canonicalize()?;
    let output =
        crate::d2_mot::reader::outside(&path("PARHELION_PARTICLE_GEOMETRY_OUTPUT")?, &root)?;
    ensure!(!output.exists(), "geometry output already exists");
    let scratch = tempfile::tempdir()?;
    let mut source = Reader::discovery(
        &path("PARHELION_IMPORT_SOURCE_PACKAGES")?,
        scratch.path(),
        true,
    )?;
    let mut native = Reader::discovery(
        &path("PARHELION_IMPORT_NATIVE_PACKAGES")?,
        scratch.path(),
        false,
    )?;
    let census = fs::read_to_string(corpus.join("models.tsv"))?;
    let model = census
        .lines()
        .nth(1)
        .context("native float model control")?
        .split('\t')
        .next()
        .context("native model tag")?;
    let model = u32::from_str_radix(model, 16)?;
    let wrapper =
        u32::from_str_radix(&std::env::var("PARHELION_PARTICLE_CONTAINER_TEMPLATE")?, 16)?;
    let template = Templates::read(&mut native, model, wrapper)?;
    ensure!(
        Templates::read(&mut source, model, wrapper).is_err(),
        "source allocation templates accepted"
    );
    let plan: Vec<u32> =
        serde_json::from_slice(&fs::read(root.join("enigma-geometry-plan.json"))?)?;
    ensure!(!plan.is_empty(), "empty source geometry plan");
    let manifest: Value = serde_json::from_slice(&fs::read(
        root.join("enigma-resource-dependencies/source-manifest.json"),
    )?)?;
    let mut all = Assets::default();
    let mut geometry_symbols = BTreeMap::new();
    let mut geometries = Vec::new();
    fs::create_dir_all(&output)?;
    for tag in plan {
        let geometry = super::super::convert(&mut source, tag)?;
        let materials = geometry
            .references
            .iter()
            .filter(|r| r.kind == Kind::Material)
            .map(|r| (r.source, format!("effect-material-{:08X}", r.source)))
            .collect();
        let symbol = format!("effect-geometry-{tag:08X}");
        ensure!(
            geometry
                .assets(&symbol, &template, &BTreeMap::new())
                .is_err(),
            "unresolved material omitted"
        );
        ensure!(
            geometry.assets("../escape", &template, &materials).is_err(),
            "invalid graph symbol accepted"
        );
        let assets = geometry.assets(&symbol, &template, &materials)?;
        for buffer in &geometry.buffers {
            let source_header = source.tag(buffer.source, None)?;
            let source_data = source.tag(buffer.data_source, None)?;
            ensure!(
                source_header.0 == buffer.header && source_data.0 == buffer.data,
                "source buffer changed"
            );
        }
        for row in census.lines().skip(1) {
            let tag = row.split('\t').next().context("native model tag")?;
            let shipped = fs::read(corpus.join(format!("{tag}.bin")))?;
            for range in [8..16, 48..80, 144..160] {
                ensure!(
                    geometry.bytes[range.clone()] == shipped[range],
                    "geometry header differs from native control {tag}"
                );
            }
        }
        geometry_symbols.insert(tag, symbol.clone());
        geometries.push(json!({"source":format!("{tag:08X}"),"symbol":symbol,
            "buffers":geometry.buffers.iter().map(|b| json!({"source":format!("{:08X}",b.source),
                "kind":b.kind,"header":format!("{symbol}-buffer-{:08X}.bin",b.source),
                "data":format!("{symbol}-buffer-{:08X}-data.bin",b.source)})).collect::<Vec<_>>()}));
        all.append(assets)?;
    }
    let mut wrappers = Vec::new();
    for (tag, entry) in manifest["tags"].as_object().context("source manifest")? {
        if entry["reference"].as_u64() != Some(0x80806929) {
            continue;
        }
        let source_tag = u32::from_str_radix(tag, 16)?;
        let bytes = source.tag(source_tag, Some(0x80806929))?;
        let symbol = format!("effect-models-{tag}");
        let assets = container(&bytes.0, &symbol, &template, &geometry_symbols)?;
        wrappers.push(json!({"source":tag,"symbol":symbol,
            "models":crate::d2_mot::particles::system::model_container_sources(&bytes.0)?}));
        all.append(assets)?;
    }
    all.validate_links()?;
    for (file, bytes) in &all.files {
        fs::write(output.join(file), bytes)?;
    }
    fs::write(
        output.join("geometry-assets.json"),
        serde_json::to_vec_pretty(&json!({
            "schema":1,"geometries":geometries,"wrappers":wrappers,"nodes":all.nodes,
            "external_materials":all.external_materials,"package_enrolled":false,"installed":false
        }))?,
    )?;
    let probe = path("PARHELION_PARTICLE_GEOMETRY_PROBE")?.canonicalize()?;
    let python = std::env::var("PARHELION_PYTHON").unwrap_or_else(|_| "python".into());
    let status = Command::new(python)
        .arg(probe)
        .arg("--assets")
        .arg(output.join("geometry-assets.json"))
        .arg("--source")
        .arg(root.join("enigma-resource-geometry"))
        .arg("--native-corpus")
        .arg(corpus)
        .arg("--output")
        .arg(output.join("readback.json"))
        .status()?;
    ensure!(
        status.success(),
        "independent geometry and GPU readback failed"
    );
    Ok(())
}
