//! Original package shaders through native compilation and independent GPU readback.
use anyhow::{Context, Result, ensure};
use parhelion_import::d2_mot::{
    native::{
        PackedScopes, PackedTransform as Transform, ShaderProgram as Program, ShaderStage as Stage,
        compile_shader,
    },
    reader::{self, Reader},
    tfx,
};
use serde_json::{Value, json};
use std::{env, fs, path::PathBuf};

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

#[test]
#[ignore = "Requires source and native packages, packed shader cases and a fresh artifact directory"]
fn packed_material_programs_emit_checked_native_resources() -> Result<()> {
    let packages = configured("PARHELION_IMPORT_MODERN_PACKAGES")?;
    let native_packages = configured("SUNDIAL_STOCK_PACKAGES")?;
    let output = reader::outside(
        &configured("PARHELION_PACKED_MATERIAL_OUTPUT")?,
        packages.parent().context("Source root")?,
    )?;
    let output = reader::outside(&output, native_packages.parent().context("Native root")?)?;
    ensure!(!output.exists(), "Use a fresh shader artifact directory");
    let cases_path = configured("PARHELION_PACKED_MATERIAL_CASES")?;
    let cases: Vec<Value> = serde_json::from_slice(&fs::read(&cases_path)?)?;
    ensure!(!cases.is_empty(), "Configure real packed material programs");
    let mut source = Reader::new(&packages, &output.join("tfx-modern"), true)?;
    let mut native = Reader::new(&native_packages, &output.join("tfx-native"), false)?;
    for (r, modern) in [(&mut source, true), (&mut native, false)] {
        let context = tfx::context(r, modern)?;
        reader::write_json(
            &output
                .join(if modern { "tfx-modern" } else { "tfx-native" })
                .join("context.json"),
            &context,
        )?;
    }
    let scopes = PackedScopes::read(&output)?;
    let mut receipt = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let tag = u32::try_from(case["model"].as_u64().context("Source model")?)?;
        let material_tag = u32::try_from(case["material"].as_u64().context("Source material")?)?;
        let model = source.tag(tag, Some(0x80806F07))?;
        let material = source.tag(material_tag, None)?;
        let mesh = model.array(16, 128, Some(0x80806EC5))?[0];
        let mut buffers = Vec::new();
        for offset in [0, 4] {
            let header = source.tag(model.u32(mesh + offset)?, None)?;
            ensure!(header.u16(6)? == 0, "Expected unskinned source stream");
            let data = source.tag(source.reference(model.u32(mesh + offset)?)?, None)?;
            buffers.push(data.0.clone());
        }
        let dir = output.join(format!("case-{index}"));
        fs::create_dir(&dir)?;
        fs::write(dir.join("positions.bin"), &buffers[0])?;
        fs::write(dir.join("uv.bin"), &buffers[1])?;
        let transform = Transform {
            scale: [model.f32(0x50)?, model.f32(0x54)?, model.f32(0x58)?],
            offset: [model.f32(0x60)?, model.f32(0x64)?, model.f32(0x68)?],
            uv: [
                model.f32(0x70)?,
                model.f32(0x74)?,
                model.f32(0x78)?,
                model.f32(0x7C)?,
            ],
            rect: [0, 0, 1, 1],
            size: [1, 1],
            vertex_base: 0,
        };
        let mut programs = Vec::new();
        for (stage, offset) in [(Stage::Vertex, 0x70), (Stage::Pixel, 0x2B0)] {
            let vertex = stage == Stage::Vertex;
            let name = if vertex { "vertex" } else { "pixel" };
            let header = material.u32(offset)?;
            let original = source.tag(source.reference(header)?, None)?;
            let hlsl_path = cases_path
                .parent()
                .context("Case root")?
                .join(case[name].as_str().context("Decompiled source")?);
            let hlsl = fs::read_to_string(hlsl_path)?.replace("\r\n", "\n");
            let translated = if vertex {
                scopes.vertex(&hlsl, &transform)?
            } else {
                scopes.pixel(&hlsl)?
            };
            let hlsl_output = dir.join(format!("native-{name}.hlsl"));
            fs::write(&hlsl_output, &translated)?;
            let compiled = dir.join(format!("compiled-{name}"));
            compile_shader(
                &hlsl_output,
                &compiled,
                if vertex { "vs_5_0" } else { "ps_5_0" },
            )?;
            let bytes = fs::read(compiled.join("bytecode.bin"))?;
            let program = Program::new(stage, bytes)?;
            ensure!(
                program.bytecode() != original.0,
                "Source program was not adapted"
            );
            fs::write(dir.join(format!("source-{name}.dxbc")), &original.0)?;
            fs::write(dir.join(format!("native-{name}.dxbc")), program.bytecode())?;
            fs::write(dir.join(format!("native-{name}.bin")), &program.header().0)?;
            fs::write(dir.join(format!("native-{name}.hlsl")), translated)?;
            programs.push(program.receipt());
        }
        let row = json!({"model":tag,"material":material_tag,"transform":transform,
            "programs":programs,"source_positions":"positions.bin","source_uv":"uv.bin",
            "source_vertex":"source-vertex.dxbc","native_vertex":"native-vertex.dxbc",
            "source_pixel":"source-pixel.dxbc","native_pixel":"native-pixel.dxbc"});
        reader::write_json(&dir.join("case.json"), &row)?;
        receipt.push(row);
    }
    source.finish()?;
    native.finish()?;
    reader::write_json(
        &output.join("packed-materials.json"),
        &json!({"cases":receipt,"scopes":scopes.receipt(),"game_started":false}),
    )?;
    Ok(())
}
