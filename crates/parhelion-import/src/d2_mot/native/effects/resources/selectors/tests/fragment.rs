//! Configured embedded predicate ownership and relocation oracle, before implementation.
use super::*;
use crate::d2_mot::native::categories;

#[test]
#[ignore = "requires explicitly configured paired selector and dictionary exports"]
fn embedded_category_predicate_oracle() -> Result<()> {
    let corpus = PathBuf::from(std::env::var("PARHELION_SELECTOR_CORPUS")?);
    let output = PathBuf::from(std::env::var("PARHELION_RESPONSE_PREDICATE_OUTPUT")?);
    ensure!(!output.exists(), "predicate oracle output already exists");
    let load = |path: &str| -> Result<Payload> { Ok(Payload(fs::read(corpus.join(path))?)) };
    let source_dictionary = load("assembly-verification/source-category/raw/80CEA5F8.bin")?;
    let stock = load("assembly-verification/native-category/raw/80C70CA1.bin")?;
    let namespace = categories::emit_static(
        &source_dictionary,
        &stock,
        &BTreeSet::from([0xD1945C50, 0x000A69A1, 0x9C6C4BA0]),
    )?;
    let resources =
        Resources::from_namespace(&namespace, BTreeMap::from([(0x80CEA5F8, 0x81FD1FFD)]));
    for hash in [0x905E8440, 0x3FBE3C2A, 0x4D31E591] {
        ensure!(
            namespace.source_groups.contains_key(&hash) && namespace.groups.contains_key(&hash),
            "actual paired dictionary group missing"
        );
    }
    let groups = GroupBindings::from_namespace(&namespace);
    let source = load("damage-corpus/selectors/modern/80CEBA3A.bin")?;
    let at = (4..source.0.len() - 104)
        .step_by(4)
        .find(|at| source.u32(at - 4).ok() == Some(0x808042CB))
        .context("actual category predicate missing")?;
    let contract = FragmentContract {
        root_class: 0x808042CB,
        field: at,
        extent: 104,
    };
    let mut owner = Payload(vec![0xA5; 173]);
    let fragment = append_category(&source, &mut owner, contract, &resources, &groups)?;
    ensure!(
        owner.0[..173] == [0xA5; 173],
        "fragment changed prior owner bytes"
    );
    ensure!(
        owner.u32(fragment.root - 4)? == 0x80804D73,
        "native predicate class differs"
    );
    ensure!(
        fragment.gates.is_empty(),
        "private namespace category gates remain"
    );
    ensure!(
        !fragment.references.is_empty()
            && fragment
                .references
                .iter()
                .all(|r| r.offset >= 173 && r.target == Some(0x81FD1FFD)),
        "fragment references did not use destination offsets"
    );
    ensure!(
        !fragment.reference_classes.is_empty()
            && fragment
                .reference_classes
                .values()
                .all(|c| *c == 0x808094B4),
        "dictionary dependency class differs"
    );
    ensure!(
        fragment
            .source_spans
            .iter()
            .any(|&(start, end)| start == at - 4 && end == at + 104),
        "fragment source extent missing"
    );
    let mut refused = Payload(vec![1; 17]);
    let before = refused.0.clone();
    ensure!(
        append_category(
            &source,
            &mut refused,
            FragmentContract {
                extent: 96,
                ..contract
            },
            &resources,
            &groups
        )
        .is_err()
            && refused.0 == before,
        "wrong extent mutated owner"
    );
    let expected = load("damage-corpus/selectors/private-model/80CEBA3A.bin")?;
    let expected_at = (4..expected.0.len() - 96)
        .step_by(4)
        .find(|at| expected.u32(at - 4).ok() == Some(0x80804D73))
        .context("independent native predicate missing")?;
    // Compare scalar fields and typed array contents, allowing actual placement to differ.
    ensure!(
        owner.u64(fragment.root + 80)? == expected.u64(expected_at + 80)?,
        "predicate kind differs"
    );
    for i in 0..4 {
        let actual = owner.array(fragment.root + i * 16, 24, Some(0x808094B3))?;
        let golden = expected.array(expected_at + i * 16, 24, Some(0x808094B3))?;
        ensure!(actual.len() == golden.len(), "predicate row count differs");
        for (a, b) in actual.into_iter().zip(golden) {
            ensure!(
                owner.u32(a)? == expected.u32(b)? && owner.u32(a + 16)? == expected.u32(b + 16)?,
                "predicate row identity differs"
            );
        }
    }
    fs::create_dir_all(&output)?;
    fs::write(output.join("owner.bin"), &owner.0)?;
    fs::write(output.join("dictionary.bin"), &namespace.payload.0)?;
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
        "root":fragment.root,"references":fragment.references.len(),
        "source_spans":fragment.source_spans,"source_references":fragment.source_references,
        "group_aliases":fragment.group_aliases,
        "native_runtime_evaluation_verified":false,
        "sha256":format!("{:x}",Sha256::digest(&owner.0))}))?,
    )?;
    Ok(())
}
