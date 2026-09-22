//! Export the complete source render inputs needed by the Rust shader adapter.
use crate::d2_mot::{
    material,
    payload::Payload,
    profile,
    reader::{Reader, outside, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{collections::BTreeSet, fs, path::Path, process::Command};

fn load(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub(crate) fn decompile(tool: &Path, path: &Path) -> Result<()> {
    let mut cmd = Command::new(tool);
    cmd.arg("-D").arg(path);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    let output = cmd.output().context("Launching shader decompiler")?;
    let mut log = output.stdout;
    log.extend(output.stderr);
    fs::write(path.with_extension("decompile.log"), log)?;
    ensure!(
        output.status.success() && path.with_extension("hlsl").exists(),
        "Shader {} decompilation failed",
        path.display()
    );
    Ok(())
}

pub fn export(
    prepared: &Path,
    modern: &Path,
    native: &Path,
    refs: &Path,
    decompiler: &Path,
    out: &Path,
) -> Result<Value> {
    export_with_progress(prepared, modern, native, refs, decompiler, out, &mut |_| {})
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited source export workflow."
)]
pub(crate) fn export_with_progress(
    prepared: &Path,
    modern: &Path,
    native: &Path,
    refs: &Path,
    decompiler: &Path,
    out: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<Value> {
    let out = outside(out, prepared)?;
    outside(&out, modern.parent().context("modern parent")?)?;
    outside(&out, native.parent().context("native parent")?)?;
    ensure!(!out.exists(), "source support output already exists");
    let source = prepared.join("source");
    let report = load(&source.join("report.json"))?;
    let mut r = Reader::new(modern, &out, true)?;
    let mut materials = BTreeSet::new();
    let mut models = Vec::new();
    for entry in report["models"].as_array().context("source models")? {
        let tag = profile::hash(entry, "model")?;
        let model = Payload(fs::read(source.join(format!("raw/{tag:08X}.bin")))?);
        for mesh in model.array(16, 128, None)? {
            let parts = model.array(mesh + 32, 36, None)?;
            for stage in [0, 1, 3, 7, 9, 12] {
                for &at in parts
                    .get(
                        model.u16(mesh + 48 + stage * 2)? as usize
                            ..model.u16(mesh + 50 + stage * 2)? as usize,
                    )
                    .context("source stage range")?
                {
                    let mat = model.u32(at)?;
                    if mat != u32::MAX {
                        materials.insert(mat);
                    }
                }
            }
        }
        models.push(tag);
    }
    let mut bindings = serde_json::Map::new();
    let mut shaders = BTreeSet::new();
    let material_count = materials.len();
    for (index, tag) in materials.into_iter().enumerate() {
        progress(format!(
            "Reading source material {} of {material_count}…",
            index + 1
        ));
        let mat = r.tag(tag, None)?;
        for at in [0x70, 0x2B0] {
            let shader = mat.u32(at)?;
            if shader != u32::MAX {
                shaders.insert(shader);
            }
            let external = mat.u32(at + 0x74)?;
            if ![0, u32::MAX, 0x811C9DC5].contains(&external) {
                r.tag(external, None)?;
                r.tag(r.reference(external)?, None)?;
            }
        }
        bindings.insert(
            format!("{tag:08X}"),
            material::inspect(&mut r, tag, true)
                .with_context(|| format!("source material {tag:08X}"))?,
        );
    }
    write_json(&out.join("bindings.json"), &Value::Object(bindings.clone()))?;
    let shader_dir = refs.join("library-surfaces-01/source-shaders");
    fs::create_dir_all(&shader_dir)?;
    for (index, tag) in shaders.iter().enumerate() {
        progress(format!(
            "Converting source shader {} of {}…",
            index + 1,
            shaders.len()
        ));
        let header = r.tag(*tag, None)?;
        let code = r.tag(r.reference(*tag)?, None)?;
        ensure!(code.0.starts_with(b"DXBC"), "source shader is not DXBC");
        let path = shader_dir.join(format!("{tag:08X}.dxbc"));
        if path.exists() {
            ensure!(fs::read(&path)? == code.0, "source shader export changed");
        } else {
            fs::write(&path, &code.0)?;
        }
        fs::write(path.with_extension("header.bin"), &header.0)?;
        if !path.with_extension("hlsl").exists() {
            decompile(decompiler, &path)?;
        }
    }
    r.finish()?;
    for tag in &models {
        let path = refs.join(format!("library-surfaces-01/vertex-colors/{tag:08X}"));
        if !path.join("colors.json").exists() {
            r.begin_export(&path)?;
            let colors = material::vertex_colors(&mut r, *tag)?;
            write_json(&path.join("colors.json"), &colors)?;
            r.finish()?;
        }
    }
    drop(r);
    let template = load(&prepared.join("native/template-report.json"))?;
    let mut owner_metadata = BTreeSet::new();
    let mut bank_metadata = BTreeSet::new();
    for entry in template["models"].as_array().context("native models")? {
        let owner = Payload(fs::read(prepared.join(format!(
            "native/raw/{}.bin",
            entry["owner"].as_str().context("native owner")?
        )))?);
        owner_metadata.insert(owner.u32(0x44)?);
        let entity = Payload(fs::read(prepared.join(format!(
            "native/raw/{}.bin",
            entry["entity"].as_str().context("native entity")?
        )))?);
        for at in entity.array(16, 12, None)? {
            let tag = entity.u32(at)?;
            let path = prepared.join(format!("native/raw/{tag:08X}.bin"));
            if !path.exists() {
                continue;
            }
            let component = Payload(fs::read(path)?);
            let instance = component.pointer(16)?;
            if component.u32(instance - 4)? == 0x8080979F {
                bank_metadata.insert(component.u32(0x44)?);
            }
        }
    }
    let mut r = Reader::new(native, &refs.join("native-owner-allocations"), false)?;
    for (folder, tags) in [
        ("native-owner-allocations", owner_metadata),
        ("native-channel-defaults", bank_metadata),
    ] {
        for tag in tags {
            let path = refs.join(format!("{folder}/{tag:08X}"));
            if !path.join(format!("raw/{tag:08X}.bin")).exists() {
                r.begin_export(&path)?;
                r.tag(tag, None)?;
                r.finish()?;
            }
        }
    }
    let result = json!({"models":models,"shaders":shaders,"materials":bindings.len(),"implementation":"Rust","gameplay_verified":false});
    write_json(&out.join("support.json"), &result)?;
    Ok(result)
}
