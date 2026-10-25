//! Configured source graph to private namespace oracle, written before the bridge.
use super::*;
use crate::d2_mot::native::categories;

#[test]
#[ignore = "requires explicitly configured selector and private category exports"]
fn private_category_selector_oracle() -> Result<()> {
    let corpus = PathBuf::from(std::env::var("PARHELION_SELECTOR_CORPUS")?);
    let output = PathBuf::from(std::env::var("PARHELION_PRIVATE_SELECTOR_OUTPUT")?);
    ensure!(
        !output.exists(),
        "private selector oracle output already exists"
    );
    let directory = corpus.join("damage-corpus/selectors");
    let load = |path: &str| -> Result<Payload> { Ok(Payload(fs::read(corpus.join(path))?)) };
    let namespace = categories::emit_static(
        &load("assembly-verification/source-category/raw/80CEA5F8.bin")?,
        &load("assembly-verification/native-category/raw/80C70CA1.bin")?,
        &BTreeSet::from([0xD1945C50, 0x000A69A1, 0x9C6C4BA0]),
    )?;
    let resources =
        Resources::from_namespace(&namespace, BTreeMap::from([(0x80CEA5F8, 0x81FD1FFD)]));
    let mut rows = Vec::new();
    fs::create_dir_all(&output)?;
    for tag in ["80CEBA39", "80CEBA3A"] {
        let source = Payload(fs::read(
            directory.join("modern").join(format!("{tag}.bin")),
        )?);
        let expected = fs::read(directory.join("private-model").join(format!("{tag}.bin")))?;
        let native = emit(&source, &resources)?;
        ensure!(
            native.payload.0 == expected,
            "private selector differs from independent artifact"
        );
        ensure!(
            native.gates.is_empty()
                && native
                    .references
                    .iter()
                    .all(|reference| reference.target == Some(0x81FD1FFD)),
            "private selector dependencies incomplete"
        );
        fs::write(output.join(format!("{tag}.bin")), &native.payload.0)?;
        rows.push(
            json!({"source":tag,"source_sha256":hex::encode(Sha256::digest(&source.0)),
            "native_sha256":hex::encode(Sha256::digest(&native.payload.0)),
            "dictionary_slots":native.references.len(),"missing_names":native.gates.len(),
            "group_aliases":native.group_aliases}),
        );
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "rows":rows,"independent_artifact_exact":true,"installable":false,
            "native_predicate_compiler_verified":false,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}
