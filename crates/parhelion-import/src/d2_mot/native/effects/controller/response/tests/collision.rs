//! Actual complete response and dependency oracle, before collision selector behavior.
use super::*;
use crate::d2_mot::native::{categories, effects::resources::selectors};
use std::collections::BTreeSet;

#[test]
#[ignore = "requires configured response, selector and native controller exports"]
fn collision_value_selector_owner_oracle() -> Result<()> {
    let root = PathBuf::from(env::var("PARHELION_RESPONSE_CORPUS")?);
    let output = PathBuf::from(env::var("PARHELION_RESPONSE_COLLISION_OUTPUT")?);
    ensure!(!output.exists(), "collision oracle output already exists");
    let load = |path: &str| -> Result<Payload> { Ok(Payload(fs::read(root.join(path))?)) };
    let namespace = categories::emit_static(
        &load("assembly-verification/source-category/raw/80C3A713.bin")?,
        &load("assembly-verification/native-category/raw/80C70CA1.bin")?,
        &BTreeSet::new(),
    )?;
    let groups = selectors::GroupBindings::from_namespace(&namespace);
    let hashes: Value =
        serde_json::from_slice(&fs::read(root.join("collision-corpus/tag64.json"))?)?;
    let hashes = hashes["hashes"]
        .as_object()
        .context("response hashes")?
        .iter()
        .map(|(k, v)| Ok((u64::from_str_radix(k, 16)?, hex(v)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let resources = Resources {
        tags: BTreeMap::from([(0x80CA4F89, 0x80B9FBB3), (0x80C3A713, 0x80C70CA1)]),
        hashes: hashes.clone(),
        names: namespace.source_names.clone(),
        categories: namespace.source_indices.clone(),
    };
    let resolver = selectors::ResourceResolver {
        hashes,
        source_classes: BTreeMap::from([(0x80CA4F89, selectors::MODERN_CLASS)]),
        native_classes: BTreeMap::from([(0x80B9FBB3, selectors::NATIVE_CLASS)]),
    };
    let source = load("collision-corpus/modern/80CA5BA1.bin")?;
    let template = load("damage-corpus/scalar-controller-corpus/80FDAC4D.bin")?;
    let allocation = load("network-corpus/shadowkeep/80C70B90.bin")?;
    let child = load("damage-corpus/selectors/modern/80CA4F89.bin")?;
    let native_child = load("damage-corpus/selectors/shadowkeep/80B9FBB3.bin")?;
    let child_resources = selectors::Resources::from_namespace(&namespace, resources.tags.clone());
    ensure!(
        selectors::emit(&child, &child_resources)?.payload.0 == native_child.0,
        "referenced selector differs from native control"
    );
    ensure!(
        emit(
            &source,
            &template,
            &allocation,
            &resources,
            0x81FD1101,
            0x81FD1102
        )
        .is_err(),
        "collision selector accepted without dependency class evidence"
    );
    let native = emit_with_bindings(
        &source,
        &template,
        &allocation,
        &resources,
        0x81FD1101,
        0x81FD1102,
        &groups,
        &resolver,
    )?;
    let at = (4..native.owner.0.len() - 40)
        .step_by(4)
        .find(|at| native.owner.u32(at - 4).ok() == Some(0x80804B0A))
        .context("native collision selector missing")?;
    let rows = native.owner.array(at + 8, 16, Some(0x80809316))?;
    ensure!(
        rows.len() == 2 && native.owner.u64(rows[0])? == 0 && native.owner.u64(rows[1])? == 1,
        "collision selector values or order differ"
    );
    let predicates = rows
        .iter()
        .map(|r| {
            let p = native.owner.pointer(r + 8)?;
            native.owner.u32(p - 4)
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        predicates == [0x8080930F, 0x80804D7C],
        "collision predicate classes differ"
    );
    let modifiers = native.owner.array(at + 24, 8, Some(0x80804B0D))?;
    ensure!(
        modifiers.len() == 1
            && native.owner.u32(native.owner.pointer(modifiers[0])? - 4)? == 0x80804B09,
        "collision pointer modifiers differ"
    );
    ensure!(
        native
            .gates
            .iter()
            .any(|g| matches!(g, Gate::Selector { .. })),
        "unverified native predicate runtime gate missing"
    );
    let externals = native
        .reference_classes
        .iter()
        .filter(|(_, c)| **c == selectors::NATIVE_CLASS)
        .map(|(o, _)| *o)
        .collect::<Vec<_>>();
    ensure!(
        externals.len() == 1 && native.owner.u32(externals[0])? == 0x80B9FBB3,
        "external selector relocation or native class differs"
    );
    for row in &native.group_aliases {
        ensure!(
            native.owner.u32(row.native_offset)? == row.native
                && source.u32(row.source_offset)? == row.source,
            "response alias hash offsets changed during fragment propagation"
        );
    }
    fs::create_dir_all(&output)?;
    fs::write(output.join("owner.bin"), &native.owner.0)?;
    fs::write(output.join("selector.bin"), &native_child.0)?;
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
        "source":"80CA5BA1","native_layout_control":"80FDAC4D","root":at,
        "sha256":format!("{:x}",Sha256::digest(&native.owner.0)),"gates":native.gates,
        "reference_classes":native.reference_classes,"group_aliases":native.group_aliases,
        "native_runtime_evaluation_verified":false}))?,
    )?;
    Ok(())
}
