//! Failure contract written before external selector support.
//!
//! Reject truncated wide references, unknown forms, missing hash resolution,
//! null resources, wrong source/native entry classes and unbound dependencies.
//! Preserve and relocate pooled debug strings, row order and predicate flags.
//! Verify real source-child conversion against an independent native asset,
//! then write inspectable parent output and dependency evidence.
use super::*;

#[test]
#[ignore = "Requires PARHELION_SELECTOR_CORPUS and PARHELION_SELECTOR_REFERENCE_OUTPUT"]
fn external_selector_dependency_and_value_contract() -> Result<()> {
    let root = PathBuf::from(env::var_os("PARHELION_SELECTOR_CORPUS").context("selector corpus")?);
    let output = PathBuf::from(
        env::var_os("PARHELION_SELECTOR_REFERENCE_OUTPUT").context("selector reference output")?,
    );
    let corpus = root.join("damage-corpus/selectors");
    let child = Payload(fs::read(corpus.join("modern/80C3A589.bin"))?);
    let native_child = Payload(fs::read(corpus.join("shadowkeep/80B9FBB3.bin"))?);
    let correspondence: Value = serde_json::from_slice(&fs::read(
        root.join("assembly-verification/category-correspondence.json"),
    )?)?;
    let names = correspondence["source"]["names"]
        .as_array()
        .context("category names")?
        .iter()
        .map(|n| Ok(u32::try_from(n.as_u64().context("category name")?)?))
        .collect::<Result<Vec<_>>>()?;
    let mut categories = vec![None; names.len()];
    for (from, to) in correspondence["mapping"]
        .as_object()
        .context("category mapping")?
    {
        categories[from.parse::<usize>()?] =
            Some(u16::try_from(to.as_u64().context("category index")?)?);
    }
    let namespace = crate::d2_mot::native::categories::emit_static(
        &Payload(fs::read(
            root.join("assembly-verification/source-category/raw/80C3A713.bin"),
        )?),
        &Payload(fs::read(
            root.join("assembly-verification/native-category/raw/80C70CA1.bin"),
        )?),
        &BTreeSet::new(),
    )?;
    let groups = GroupBindings::from_namespace(&namespace);
    let resources = Resources {
        tags: BTreeMap::from([(0x80C3A589, 0x80B9FBB3), (0x80C3A713, 0x80C70CA1)]),
        names,
        categories,
    };
    ensure!(
        emit(&child, &resources)?.payload.0 == native_child.0,
        "external selector child differs from independent native control"
    );
    let resolver = ResourceResolver {
        hashes: BTreeMap::new(),
        source_classes: BTreeMap::from([(0x80C3A589, MODERN_CLASS)]),
        native_classes: BTreeMap::from([(0x80B9FBB3, NATIVE_CLASS)]),
    };
    let source = Payload(fs::read(corpus.join("modern/80C3572F.bin"))?);
    ensure!(
        emit(&source, &resources).is_err(),
        "external selector emitted without class evidence"
    );
    let converted = emit_with_bindings(&source, &resources, &resolver, &groups)?;
    let reference = converted
        .references
        .iter()
        .find(|r| r.source == 0x80C3A589)
        .context("external dependency slot")?;
    let field = reference.offset;
    ensure!(
        converted
            .external_classes
            .values()
            .filter(|&&class| class == NATIVE_CLASS)
            .count()
            == 1,
        "external selector dependency count differs"
    );
    ensure!(
        converted.external_classes.get(&field) == Some(&NATIVE_CLASS),
        "external selector expected class differs"
    );
    ensure!(
        reference.target == Some(0x80B9FBB3),
        "external selector binding differs"
    );
    ensure!(
        converted.payload.u32(field)? == 0x80B9FBB3 && converted.payload.u32(field + 4)? == 0,
        "external native reference differs"
    );
    let source_prefix = source
        .0
        .windows(4)
        .position(|v| v == 0x808091B0u32.to_le_bytes())
        .context("source external predicate")?;
    ensure!(
        converted.payload.u32(field - 12)? == 0x8080930F,
        "external predicate class differs"
    );
    let text = |p: &Payload, at: usize| -> Result<Vec<u8>> {
        let start = p.pointer(at)?;
        let count = p.0[start..]
            .iter()
            .position(|b| *b == 0)
            .context("debug terminator")?;
        Ok(p.0[start..start + count].to_vec())
    };
    ensure!(
        text(&converted.payload, field - 8)? == text(&source, source_prefix + 4)?,
        "external debug string differs"
    );
    let mut invalid = ResourceResolver {
        hashes: BTreeMap::new(),
        source_classes: resolver.source_classes.clone(),
        native_classes: BTreeMap::from([(0x80B9FBB3, 0x80809312)]),
    };
    ensure!(
        emit_with_bindings(&source, &resources, &invalid, &groups).is_err(),
        "wrong native dependency class accepted"
    );
    invalid.native_classes = resolver.native_classes.clone();
    invalid.source_classes.insert(0x80C3A589, 0x808091B1);
    ensure!(
        emit_with_bindings(&source, &resources, &invalid, &groups).is_err(),
        "wrong source dependency class accepted"
    );
    let mut wide = source.clone();
    let reference_at = source_prefix + 12;
    let hash = 0xD087B48B7CA172F1u64;
    wide.0[reference_at..reference_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    wide.0[reference_at + 4..reference_at + 8].fill(0);
    wide.0[reference_at + 8..reference_at + 16].copy_from_slice(&hash.to_le_bytes());
    ensure!(
        emit_with_resolver(&wide, &resources, &resolver).is_err(),
        "missing reference hash accepted"
    );
    let resolved = ResourceResolver {
        hashes: BTreeMap::from([(hash, 0x80C3A589)]),
        source_classes: resolver.source_classes.clone(),
        native_classes: resolver.native_classes.clone(),
    };
    ensure!(
        emit_with_bindings(&wide, &resources, &resolved, &groups)?
            .payload
            .0
            == converted.payload.0,
        "hash and direct dependency outputs differ"
    );
    let mut invalid_string = source.clone();
    invalid_string.0[source_prefix + 4..source_prefix + 12]
        .copy_from_slice(&i64::MAX.to_le_bytes());
    ensure!(
        emit_with_resolver(&invalid_string, &resources, &resolver).is_err(),
        "external debug pointer outside payload accepted"
    );
    fs::create_dir_all(&output)?;
    fs::write(output.join("selector.bin"), &converted.payload.0)?;
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "source": "80C3572F", "source_sha256": hex::encode(Sha256::digest(&source.0)),
            "native_sha256": hex::encode(Sha256::digest(&converted.payload.0)),
            "child_control": "80B9FBB3", "references": converted.references,
            "external_classes": converted.external_classes, "category_gates": converted.gates,
            "group_aliases":converted.group_aliases,
            "native_runtime_evaluation_verified": false
        }))?,
    )?;
    Ok(())
}
