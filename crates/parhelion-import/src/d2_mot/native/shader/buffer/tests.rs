//! Complete package/native loader oracle, written before the buffer adapter.
use super::*;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

#[test]
#[ignore = "requires exported material buffers, native packages and the captured buffer loader probe"]
fn immutable_material_buffer_oracle() -> Result<()> {
    let root = PathBuf::from(std::env::var("PARHELION_MATERIAL_BUFFER_CORPUS")?).canonicalize()?;
    let output = crate::d2_mot::reader::outside(
        &PathBuf::from(std::env::var("PARHELION_MATERIAL_BUFFER_OUTPUT")?),
        &root,
    )?;
    ensure!(!output.exists(), "material buffer output already exists");
    let load =
        |name: &str| -> Result<Value> { Ok(serde_json::from_slice(&fs::read(root.join(name))?)?) };
    let native = load("native-material-buffers.json")?;
    let source = load("modern-material-buffers.json")?;
    let source_rows = source.as_array().context("source buffers")?;
    let source_tags = source_rows
        .iter()
        .map(|row| Ok(row["tag"].as_str().context("source buffer tag")?.to_owned()))
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    ensure!(
        !source_tags.is_empty() && source_tags.len() == source_rows.len(),
        "empty or duplicate source buffer census"
    );
    let read = |era: &str, row: &Value, field: &str| -> Result<Vec<u8>> {
        Ok(fs::read(root.join(format!(
            "{era}-material-buffers/{}.bin",
            row[field].as_str().context("buffer tag")?
        )))?)
    };
    let scratch = tempfile::tempdir()?;
    let mut reader = Reader::discovery(
        &PathBuf::from(std::env::var("PARHELION_IMPORT_NATIVE_PACKAGES")?),
        scratch.path(),
        false,
    )?;
    let mut source_reader = Reader::discovery(
        &PathBuf::from(std::env::var("PARHELION_IMPORT_SOURCE_PACKAGES")?),
        &scratch.path().join("source"),
        true,
    )?;
    let control = native
        .as_array()
        .context("native buffers")?
        .first()
        .context("native control")?;
    let template = Template::read(
        &mut reader,
        u32::from_str_radix(control["tag"].as_str().unwrap(), 16)?,
    )?;
    ensure!(
        Template::read(&mut reader, template.data_tag()).is_err(),
        "buffer data accepted as an allocation header"
    );
    for row in native.as_array().unwrap() {
        let data = read("native", row, "data")?;
        let buffer = Buffer::new(data)?;
        ensure!(
            buffer.header() == read("native", row, "tag")?,
            "buffer header differs from complete native package control"
        );
    }
    let mut pending = Vec::new();
    let mut buffers = Vec::new();
    for row in source.as_array().context("source buffers")? {
        let tag = row["tag"].as_str().context("source buffer tag")?;
        let header = read("modern", row, "tag")?;
        let data = read("modern", row, "data")?;
        let buffer = Buffer::read(&header, data.clone())?;
        let actual = Buffer::from_package(&mut source_reader, u32::from_str_radix(tag, 16)?)?;
        ensure!(
            actual.header() == header && actual.data() == data,
            "source buffer changed after export"
        );
        ensure!(
            Buffer::from_package(
                &mut source_reader,
                u32::from_str_radix(row["data"].as_str().unwrap(), 16)?
            )
            .is_err(),
            "data entry accepted as a source buffer header"
        );
        ensure!(
            Template::read(&mut source_reader, u32::from_str_radix(tag, 16)?).is_err(),
            "source buffer accepted as a native allocation template"
        );
        ensure!(
            buffer.data() == data && buffer.header() == header,
            "constant bytes changed"
        );
        let pair = buffer.assets(&format!("buffer-{tag}"), &template)?;
        let mut dynamic = header.clone();
        dynamic[8] = 1;
        ensure!(
            Buffer::read(&dynamic, data.clone()).is_err(),
            "dynamic buffer was accepted"
        );
        ensure!(
            Buffer::read(&header, data[..data.len() - 16].to_vec()).is_err(),
            "buffer size mismatch accepted"
        );
        ensure!(
            buffer.assets("../buffer", &template).is_err(),
            "invalid graph symbol accepted"
        );
        let file = |symbol: &str| -> Result<String> {
            Ok(pair
                .nodes
                .iter()
                .find(|node| node["symbol"] == symbol)
                .context("buffer graph symbol missing")?["file"]
                .as_str()
                .context("buffer graph file missing")?
                .to_owned())
        };
        let header_file = file(&format!("buffer-{tag}"))?;
        let data_file = file(&format!("buffer-{tag}-data"))?;
        buffers.push(json!({"source":tag,"header":header_file,"data":data_file,
            "source_header_sha256":hex::encode(Sha256::digest(&header)),
            "source_data_sha256":hex::encode(Sha256::digest(&data)),
            "receipt":buffer.receipt(),"nodes":pair.nodes}));
        pending.push((header_file, pair.header, data_file, pair.data));
    }
    ensure!(
        Buffer::new(vec![]).is_err()
            && Buffer::new(vec![0; 15]).is_err()
            && Buffer::new(vec![0; 65552]).is_err(),
        "invalid buffer capacity accepted"
    );
    fs::create_dir_all(&output)?;
    for (header_file, header, data_file, data) in pending {
        fs::write(output.join(header_file), header)?;
        fs::write(output.join(data_file), data)?;
    }
    fs::write(
        output.join("buffers.json"),
        serde_json::to_vec_pretty(&json!({
            "schema":1,"buffers":buffers,"material_inputs_bound":false,"package_enrolled":false
        }))?,
    )?;
    let probe = PathBuf::from(std::env::var("PARHELION_MATERIAL_BUFFER_PROBE")?).canonicalize()?;
    let python = std::env::var_os("PARHELION_PYTHON").unwrap_or_else(|| "python".into());
    let result = Command::new(python)
        .arg(probe)
        .arg("--root")
        .arg(&root)
        .arg("--image")
        .arg(std::env::var("PARHELION_NATIVE_IMAGE")?)
        .arg("--buffers")
        .arg(output.join("buffers.json"))
        .arg("--output")
        .arg(output.join("native-loader"))
        .output()?;
    ensure!(
        result.status.success(),
        "native buffer probe failed: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value =
        serde_json::from_slice(&fs::read(output.join("native-loader/buffer-loader.json"))?)?;
    ensure!(
        receipt["source_buffers"].as_u64() == Some(source_tags.len() as u64)
            && receipt["native_controls"]
                .as_u64()
                .is_some_and(|count| count > 0),
        "incomplete buffer oracle"
    );
    let loaded_tags = receipt["results"]
        .as_array()
        .context("buffer results")?
        .iter()
        .filter(|row| row["era"] == "source_buffer")
        .map(|row| {
            Ok(row["source"]
                .as_str()
                .context("loaded buffer source")?
                .to_owned())
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        loaded_tags.len() == source_tags.len()
            && loaded_tags
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                == source_tags,
        "native buffer loader source coverage differs"
    );
    for buffer in &buffers {
        let result = receipt["results"]
            .as_array()
            .context("buffer results")?
            .iter()
            .find(|row| row["era"] == "source_buffer" && row["source"] == buffer["source"])
            .context("emitted buffer was not loaded")?;
        ensure!(
            result["status"] == 0
                && result["guards_intact"] == true
                && result["creation"]["data_sha256"] == buffer["receipt"]["data_sha256"]
                && result["readback_sha256"] == buffer["receipt"]["data_sha256"],
            "native GPU buffer content differs"
        );
    }
    Ok(())
}
