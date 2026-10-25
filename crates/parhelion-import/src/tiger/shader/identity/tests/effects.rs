//! Complete effect closure oracle, written before compute subtype support.
use super::*;

#[test]
#[ignore = "requires complete effect shader exports and a fresh artifact directory"]
fn effect_shader_package_oracle() -> Result<()> {
    let root = PathBuf::from(std::env::var("PARHELION_EFFECT_SHADER_CORPUS")?);
    let output = PathBuf::from(std::env::var("PARHELION_EFFECT_SHADER_OUTPUT")?);
    ensure!(
        !output.exists(),
        "effect shader oracle output already exists"
    );
    let load =
        |file: &str| -> Result<Value> { Ok(serde_json::from_slice(&fs::read(root.join(file))?)?) };
    let native = load("native-shaders.json")?;
    let source = load("source-shaders.json")?;
    let correspondence = load("shader-equivalence.json")?;
    let tags = |rows: &Value| -> Result<std::collections::BTreeSet<String>> {
        let rows = rows.as_array().context("shader census rows")?;
        let tags = rows
            .iter()
            .map(|row| Ok(row["tag"].as_str().context("shader census tag")?.to_owned()))
            .collect::<Result<std::collections::BTreeSet<_>>>()?;
        ensure!(tags.len() == rows.len(), "duplicate shader census tag");
        Ok(tags)
    };
    let source_tags = tags(&source)?;
    ensure!(
        !source_tags.is_empty() && source_tags == tags(&correspondence["shaders"])?,
        "independent effect shader census coverage differs"
    );
    let read = |era: &str, tag: &str, extension: &str| -> Result<Vec<u8>> {
        Ok(fs::read(
            root.join(format!("{era}-shaders/{tag}.{extension}")),
        )?)
    };
    let stage = |entry: &Value| -> Result<Stage> {
        ensure!(
            entry["file_type"] == 33,
            "shader package entry type differs"
        );
        Stage::from_subtype(u8::try_from(
            entry["subtype"].as_u64().context("shader subtype")?,
        )?)
    };
    let mut catalog = Catalog::default();
    for entry in native.as_array().context("native shaders")? {
        let tag = entry["tag"].as_str().context("native shader tag")?;
        catalog.insert(
            u32::from_str_radix(tag, 16)?,
            stage(entry)?,
            &Payload(read("native", tag, "bin")?),
            &read("native", tag, "dxbc")?,
        )?;
    }
    let mut rows = Vec::new();
    let mut exact = 0;
    let mut identities = 0;
    let mut unmatched = 0;
    for entry in source.as_array().context("source shaders")? {
        let tag = entry["tag"].as_str().context("source shader tag")?;
        let header = Payload(read("modern", tag, "bin")?);
        let code = read("modern", tag, "dxbc")?;
        let expected = correspondence["shaders"]
            .as_array()
            .context("shader correspondence")?
            .iter()
            .find(|row| row["tag"] == tag)
            .context("shader correspondence entry")?;
        let native_matches = expected["native_exact_matches"]
            .as_array()
            .context("shader matches")?;
        let (binding, refusal) = match catalog.find(stage(entry)?, &header, &code) {
            Ok(Some(binding)) => {
                ensure!(
                    native_matches.iter().any(|value| value
                        .as_str()
                        .and_then(|name| u32::from_str_radix(name, 16).ok())
                        == Some(binding.tag)),
                    "shader lacks an independently captured native match"
                );
                let target = format!("{:08X}", binding.tag);
                ensure!(
                    header.0 == read("native", &target, "bin")?
                        && code == read("native", &target, "dxbc")?,
                    "shader bytes changed"
                );
                exact += 1;
                (Some(binding), None)
            }
            Ok(None) => {
                ensure!(
                    native_matches.is_empty(),
                    "an exact native shader was missed"
                );
                unmatched += 1;
                (None, Some("shader adapter required".to_owned()))
            }
            Err(error) => {
                ensure!(
                    header.u32(12)? != u32::MAX && native_matches.is_empty(),
                    "supported source shader refused: {error:#}"
                );
                identities += 1;
                (None, Some(format!("{error:#}")))
            }
        };
        rows.push(json!({"source":tag,"stage":stage(entry)?,"binding":binding,
            "refusal":refusal,"header_sha256":digest(&header.0),"bytecode_sha256":digest(&code)}));
    }
    ensure!(
        exact + identities + unmatched == source_tags.len() && rows.len() == source_tags.len(),
        "complete effect shader census coverage differs"
    );
    let header = Payload(read("modern", "80D89BEA", "bin")?);
    let code = read("modern", "80D89BEA", "dxbc")?;
    validate(Stage::Compute, &header, &code)?;
    ensure!(
        validate(Stage::Vertex, &header, &code).is_err()
            && validate(Stage::Pixel, &header, &code).is_err(),
        "compute stage was confused with graphics"
    );
    let header = Payload(read("modern", "80D89D32", "bin")?);
    let code = read("modern", "80D89D32", "dxbc")?;
    ensure!(
        validate(Stage::Compute, &header, &code).is_err(),
        "vertex stage accepted as compute"
    );
    fs::create_dir_all(&output)?;
    fs::write(
        output.join("effect-shader-bindings.json"),
        serde_json::to_vec_pretty(&json!({
            "native_candidates":native.as_array().unwrap().len(),"source_shaders":rows.len(),
            "reused":exact,"compiled_identity_refusals":identities,"adapters_required":unmatched,
            "bindings":rows,"materials_bound":false,"installed":false
        }))?,
    )?;
    Ok(())
}
