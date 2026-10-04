//! Configured source and native action fragment oracle written before support.
use super::*;

#[test]
#[ignore = "requires explicitly configured source and native action exports"]
fn named_condition_package_oracle() -> Result<()> {
    let root = PathBuf::from(env::var("PARHELION_SELECTOR_CORPUS")?);
    let output = PathBuf::from(env::var("PARHELION_NAMED_CONDITION_OUTPUT")?);
    ensure!(!output.exists(), "named condition output already exists");
    let corpus = root.join("sequence-corpus");
    let pairs: Value =
        serde_json::from_slice(&fs::read(corpus.join("named-predicate-pairs.json"))?)?;
    let resources = Resources {
        tags: BTreeMap::new(),
        names: Vec::new(),
        categories: Vec::new(),
    };
    let mut rows = Vec::new();
    fs::create_dir_all(&output)?;
    for (ordinal, pair) in pairs
        .as_array()
        .context("named predicate pairs")?
        .iter()
        .enumerate()
    {
        let read = |kind: &str, dir: &str| -> Result<Payload> {
            Ok(Payload(fs::read(corpus.join(dir).join(format!(
                "{}.bin",
                pair[kind].as_str().context("predicate owner tag")?
            )))?))
        };
        let source = read("source", "actions-modern")?;
        let native = read("native", "actions-native")?;
        let at = usize::try_from(
            pair["source_at"]
                .as_u64()
                .context("source predicate offset")?,
        )?;
        let to = usize::try_from(
            pair["native_at"]
                .as_u64()
                .context("native predicate offset")?,
        )?;
        ensure!(
            source.u32(at - 4)? == 0x808042CD && native.u32(to - 4)? == 0x80804D75,
            "named predicate fragment classes differ"
        );
        let mut fixture = Payload(vec![0u8; 92]);
        fixture.0[..8].copy_from_slice(&92u64.to_le_bytes());
        fixture.0[8..16].copy_from_slice(&0x100u64.to_le_bytes());
        fixture.0[16..24].copy_from_slice(&1u64.to_le_bytes());
        fixture.0[24..32].copy_from_slice(&24i64.to_le_bytes());
        fixture.0[44..48].copy_from_slice(&0x80809FB8u32.to_le_bytes());
        fixture.0[48..56].copy_from_slice(&1u64.to_le_bytes());
        fixture.0[56..64].copy_from_slice(&0x808091B7u64.to_le_bytes());
        fixture.0[72..80].copy_from_slice(&16i64.to_le_bytes());
        fixture.0[84..92].copy_from_slice(&source.bytes::<8>(at - 4)?);
        let emitted = emit(&fixture, &resources)?;
        let target = emitted.payload.pointer(72)?;
        ensure!(
            emitted.payload.bytes::<8>(target - 4)? == native.bytes::<8>(to - 4)?,
            "named condition differs from stock native fragment"
        );
        ensure!(
            emitted.references.is_empty() && emitted.gates.is_empty(),
            "named condition acquired resource dependencies"
        );
        ensure!(
            emitted.named_conditions == [source.u32(at)?],
            "named condition dependency not reported"
        );
        let mut truncated = fixture.clone();
        truncated.0.pop();
        truncated.0[..8].copy_from_slice(&91u64.to_le_bytes());
        ensure!(
            emit(&truncated, &resources).is_err(),
            "truncated named condition accepted"
        );
        fs::write(output.join(format!("{ordinal:02}.bin")), &emitted.payload.0)?;
        rows.push(
            json!({"pair":pair,"source_sha256":format!("{:x}",Sha256::digest(&source.0)),
            "native_owner_sha256":format!("{:x}",Sha256::digest(&native.0)),
            "emitted_sha256":format!("{:x}",Sha256::digest(&emitted.payload.0)),
            "native_fragment_exact":true,"truncation_refused":true,"group_aliases":emitted.group_aliases}),
        );
    }
    ensure!(!rows.is_empty(), "named predicate corpus empty");
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "rows":rows,"world_input_equivalence_verified":false,"installable":false,
            "gameplay_verified":false
        }))?,
    )?;
    Ok(())
}
