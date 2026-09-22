//! Run the established model assembly with shaders extracted from the user's build.
use super::*;
use crate::d2_mot::reader::Reader;
use sha2::{Digest, Sha256};
mod decompiler;
mod references;

const TEMPLATES: &[(u32, &str)] = &[
    (
        0x81532FC4,
        "4d3706bdd7372eedf5d57d289c1956f2c833dbbc2a2a7e2a0d2e9e4852b534b6",
    ),
    (
        0x80EC270A,
        "9a501a619c19c736337e1d1e53f1eec78430a436ad20f813bb5ee6e657599987",
    ),
    (
        0x80EC270F,
        "3a83def3ec2ea5a21f22abd27e4a9e70d95cea8e372385f20891e8369a26f6cf",
    ),
    (
        0x80EC2711,
        "52bf58b643f8d0b63a40edec03c7ba5b6ec1e1bbac0a4514f502579e6e2bbd4b",
    ),
    (
        0x8161ECD3,
        "e82fa6bc4cacb62399e5007563dec985c10ff10590ca21ba10018b322eb2b881",
    ),
];

pub(crate) fn build(
    prepared: &Path,
    source: &Path,
    native: &Path,
    out: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<Value> {
    let tool = decompiler::prepare(progress)?;
    progress("Preparing native rendering shaders…".into());
    let manifest = load(&native.join("source-manifest.json"))?;
    let packages = Path::new(manifest["packages"].as_str().context("native packages")?);
    let templates = prepared.join("shader-templates");
    let mut reader = Reader::discovery(packages, &templates, false)?;
    for (index, &(tag, expected)) in TEMPLATES.iter().enumerate() {
        progress(format!(
            "Converting native shader {} of {}…",
            index + 1,
            TEMPLATES.len()
        ));
        let header = reader.tag(tag, None)?;
        let reference = reader.reference(tag)?;
        let bytecode = reader.tag(reference, None)?;
        ensure!(
            hex::encode(Sha256::digest(&bytecode.0)) == expected,
            "Native shader {tag:08X} differs from its supported conversion template"
        );
        let path = templates.join(format!("{tag:08X}.hlsl"));
        fs::write(path.with_extension("dxbc"), &bytecode.0)?;
        crate::d2_mot::support::decompile(&tool, &path.with_extension("dxbc"))?;
        fs::write(path.with_extension("header.bin"), &header.0)?;
        write_json(
            &path.with_extension("meta.json"),
            &json!({"tag":tag,"reference":reference}),
        )?;
    }
    drop(reader);
    let mut result = super::build_with_sources(
        prepared,
        out,
        &templates.join("81532FC4.hlsl"),
        &templates,
        source,
        native,
        progress,
    )?;
    // Link immutable rips into the layout expected by the established full converter.
    references::link_inputs(source, &prepared.join("source"))?;
    references::link_inputs(native, &prepared.join("native"))?;
    let refs = prepared.join("render-inputs");
    references::export(prepared, &refs, progress)?;
    let source_manifest = load(&source.join("source-manifest.json"))?;
    let modern = Path::new(
        source_manifest["packages"]
            .as_str()
            .context("modern packages")?,
    );
    let bindings = out.join("source-bindings");
    progress("Reading source material programs and shaders…".into());
    crate::d2_mot::support::export_with_progress(
        prepared, modern, packages, &refs, &tool, &bindings, progress,
    )?;
    progress("Converting source shaders, effects, and runtime material controls…".into());
    let effects = out.with_file_name("rendered");
    super::effects::build_with_progress(
        prepared,
        out,
        &refs,
        &bindings,
        &effects,
        "source-all",
        progress,
    )?;
    result["graph"] = json!(effects.join("graph"));
    Ok(result)
}
