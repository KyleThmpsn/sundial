//! Configured independent native package material plan oracle, before code.
use super::*;
use crate::d2_mot::native::shader::rigid::{Rigid, Scopes, Stream};
use std::{fs, path::PathBuf};

#[test]
#[ignore = "requires configured source and native packages and rigid material cases"]
fn rigid_material_plan_oracle() -> Result<()> {
    let cases = PathBuf::from(std::env::var("PARHELION_RIGID_CASES")?).canonicalize()?;
    let config: Value = serde_json::from_slice(&fs::read(&cases)?)?;
    let output = PathBuf::from(std::env::var("PARHELION_RIGID_MATERIAL_OUTPUT")?);
    ensure!(
        !output.exists(),
        "rigid material oracle output already exists"
    );
    let scratch = tempfile::tempdir()?;
    let mut source = Reader::discovery(
        &PathBuf::from(std::env::var("PARHELION_IMPORT_MODERN_PACKAGES")?),
        &scratch.path().join("source"),
        true,
    )?;
    let mut native = Reader::discovery(
        &PathBuf::from(std::env::var("PARHELION_IMPORT_NATIVE_PACKAGES")?),
        &scratch.path().join("native"),
        false,
    )?;
    let tag = |v: &Value| -> Result<u32> {
        Ok(u32::from_str_radix(
            v.as_str().context("configured tag")?,
            16,
        )?)
    };
    let scopes = Scopes::read(
        &mut source,
        &mut native,
        tag(&config["source_rigid"])?,
        tag(&config["native_rigid"])?,
        tag(&config["source_view"])?,
        tag(&config["native_view"])?,
    )?;
    let stream = Stream::read(&mut source, tag(&config["vertices"][0])?)?;
    let template_tag =
        u32::from_str_radix(&std::env::var("PARHELION_RIGID_MATERIAL_TEMPLATE")?, 16)?;
    let template = Template::read(&mut native, template_tag)?;
    let mut plans = Vec::new();
    for row in config["materials"].as_array().context("material cases")? {
        let path = PathBuf::from(row["hlsl"].as_str().context("source HLSL")?);
        let path = if path.is_absolute() {
            path
        } else {
            cases.parent().context("case directory")?.join(path)
        };
        let material = tag(&row["tag"])?;
        let rigid = Rigid::read(
            &mut source,
            material,
            &fs::read_to_string(path)?,
            &scopes,
            &stream,
        )?;
        let plan = Plan::read(&mut source, material, &rigid, &template)?;
        ensure!(
            plan.pixel().header().u32(12)? == u32::MAX,
            "source pixel identity was copied to fresh native header"
        );
        plans.push(plan.receipt().clone());
    }
    ensure!(!plans.is_empty(), "material oracle has no cases");
    fs::create_dir_all(&output)?;
    fs::write(
        output.join("material-plans.json"),
        serde_json::to_vec_pretty(&json!({
            "plans":plans,"template":template.receipt(),"material_conversion_complete":false,
            "package_enrolled":false,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}
