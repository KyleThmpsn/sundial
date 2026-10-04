//! Package predicate oracle specified before faction array implementation.
use super::*;

#[test]
#[ignore = "Requires PARHELION_SELECTOR_CORPUS and PARHELION_FACTION_OUTPUT"]
fn faction_arrays_match_native_package_fragments() -> Result<()> {
    let root = PathBuf::from(env::var_os("PARHELION_SELECTOR_CORPUS").context("faction corpus")?);
    let output = PathBuf::from(env::var_os("PARHELION_FACTION_OUTPUT").context("faction output")?);
    let corpus = root.join("sequence-corpus");
    let source = Payload(fs::read(corpus.join("actions-modern/80C30CBA.bin"))?);
    let native = Payload(fs::read(corpus.join("faction-native/80BBCAB7.bin"))?);
    let source_node = 0x388;
    let native_node = 0x208;
    ensure!(
        source.u32(source_node - 4)? == 0x808042D0,
        "source faction class"
    );
    ensure!(
        native.u32(native_node - 4)? == 0x80804D78,
        "native faction class"
    );
    let source_rows = source.array(source_node, 4, Some(0x80809446))?;
    let native_rows = native.array(native_node, 1, Some(0x80806829))?;
    ensure!(
        source_rows.len() == 1 && native_rows.len() == 1,
        "faction fixture count"
    );
    ensure!(
        source.u32(source_rows[0])? == u32::from(native.u8(native_rows[0])?),
        "paired faction value"
    );

    // Remove only the enclosing action. Copy both typed child allocations and
    // their original relative pointer, then place them in a selector resource.
    let mut bytes = vec![0; 84];
    bytes.extend_from_slice(&source.0[source_node - 4..source_rows[0] + 4]);
    bytes[8..16].copy_from_slice(&0x100u64.to_le_bytes());
    bytes[16..24].copy_from_slice(&1u64.to_le_bytes());
    bytes[24..32].copy_from_slice(&24i64.to_le_bytes());
    bytes[44..48].copy_from_slice(&0x80809FB8u32.to_le_bytes());
    bytes[48..56].copy_from_slice(&1u64.to_le_bytes());
    bytes[56..64].copy_from_slice(&u64::from(0x808091B7u32).to_le_bytes());
    bytes[72..80].copy_from_slice(&16i64.to_le_bytes());
    let count = bytes.len() as u64;
    bytes[0..8].copy_from_slice(&count.to_le_bytes());
    let source_resource = Payload(bytes);
    let resources = Resources {
        tags: BTreeMap::new(),
        names: Vec::new(),
        categories: Vec::new(),
    };
    let converted = emit(&source_resource, &resources)?;
    ensure!(
        converted.references.is_empty() && converted.gates.is_empty(),
        "faction has invented dependencies"
    );
    let node = converted
        .payload
        .pointer(converted.payload.array(16, 16, Some(0x80809316))?[0] + 8)?;
    let rows = converted.payload.array(node, 1, Some(0x80806829))?;
    ensure!(
        converted.payload.0[node - 4..rows[0] + 1] == native.0[native_node - 4..native_rows[0] + 1],
        "faction package fragment differs"
    );

    let mut refusals = Vec::new();
    for (name, at, value) in [
        ("Element Class", 120, 0x80800007u32),
        ("Array Marker", 108, 0x80809FBD),
        ("Duplicated Count", 112, 2),
        ("Too Wide", 128, 256),
        ("Signed Sentinel", 128, u32::MAX),
        ("Unused Data", 80, 1),
    ] {
        let mut invalid = source_resource.clone();
        invalid.0[at..at + 4].copy_from_slice(&value.to_le_bytes());
        ensure!(
            emit(&invalid, &resources).is_err(),
            "malformed faction accepted: {name}"
        );
        refusals.push(name);
    }
    let mut truncated = source_resource.clone();
    truncated.0.pop();
    let count = truncated.0.len() as u64;
    truncated.0[..8].copy_from_slice(&count.to_le_bytes());
    ensure!(
        emit(&truncated, &resources).is_err(),
        "truncated faction accepted"
    );
    refusals.push("Truncated Value");
    ensure!(!output.exists(), "faction output already exists");
    fs::create_dir_all(&output)?;
    fs::write(output.join("source-selector.bin"), &source_resource.0)?;
    fs::write(output.join("native-selector.bin"), &converted.payload.0)?;
    fs::write(
        output.join("native-predicate.bin"),
        &native.0[native_node - 4..native_rows[0] + 1],
    )?;
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "source_class":"808042D0", "native_class":"80804D78",
            "source_element":"80809446", "native_element":"80806829",
            "source_owner":"80C30CBA", "native_owner":"80BBCAB7",
            "native_fragment_exact":true, "refusals":refusals,
            "sha256":format!("{:x}",Sha256::digest(&converted.payload.0)),
            "group_aliases":converted.group_aliases,
            "scope":"Actual source and native package predicate fragment with preserved numeric value and checked narrowing",
            "world_input_equivalence_verified":false, "installable":false, "gameplay_verified":false
        }))?,
    )?;
    Ok(())
}
