//! Configured package-to-predicate alias E2E, specified before implementation.
use super::*;
use crate::d2_mot::native::effects::controller::response;
use crate::d2_mot::native::effects::resources::selectors::{
    self, FragmentContract, GroupBindings, Resources,
};
use serde_json::{Value, json};

#[test]
#[ignore = "requires configured category alias Native-call fixture and Source predicate"]
fn private_group_alias_package_oracle() -> Result<()> {
    let corpus = PathBuf::from(std::env::var("PARHELION_SELECTOR_CORPUS")?);
    let output = PathBuf::from(std::env::var("PARHELION_CATEGORY_ALIAS_OUTPUT")?);
    ensure!(!output.exists(), "alias oracle output already exists");
    let load = |path: &str| -> Result<Payload> { Ok(Payload(fs::read(corpus.join(path))?)) };
    let source = load("assembly-verification/source-category/raw/80CEA5F8.bin")?;
    let stock = load("assembly-verification/native-category/raw/80C70CA1.bin")?;
    let baseline = emit_static(&source, &stock, &BTreeSet::new())?;
    let original_groups = baseline.groups.clone();
    let requested = BTreeSet::from([0x905E8440, 0x4D31E591]);
    let namespace = baseline.with_group_aliases(&requested)?;
    let expected = load("assembly-verification/category-private-alias/private-dictionary.bin")?;
    ensure!(
        namespace.payload.0 == expected.0,
        "alias dictionary differs from independently executed Native fixture"
    );
    for (name, members) in original_groups {
        ensure!(
            namespace.groups.get(&name) == Some(&members),
            "stock group changed"
        );
    }
    let native_receipt: Value = serde_json::from_slice(&fs::read(
        corpus.join("assembly-verification/category-private-alias/report.json"),
    )?)?;
    ensure!(
        native_receipt["native_calls"]
            .as_u64()
            .is_some_and(|count| count > 0)
            && native_receipt["getter_cases"]
                .as_u64()
                .is_some_and(|count| count > 0),
        "Native alias probe evidence missing"
    );
    let resources =
        Resources::from_namespace(&namespace, BTreeMap::from([(0x80CEA5F8, 0x81FD1FFD)]));
    let groups = GroupBindings::from_namespace(&namespace);
    let predicate = load("collision-corpus/modern/80CEF60E.bin")?;
    let at = (4..predicate.0.len() - 104)
        .step_by(4)
        .find(|at| predicate.u32(at - 4).ok() == Some(0x808042CB))
        .context("Source response predicate missing")?;
    let mut owner = Payload(vec![0xA5; 173]);
    let fragment = selectors::append_category(
        &predicate,
        &mut owner,
        FragmentContract {
            root_class: 0x808042CB,
            field: at,
            extent: 104,
        },
        &resources,
        &groups,
    )?;
    ensure!(
        owner.0[..173] == [0xA5; 173],
        "alias fragment changed prior owner bytes"
    );
    for row in &fragment.group_aliases {
        ensure!(
            owner.u32(row.native_offset)? == row.native
                && predicate.u32(row.source_offset)? == row.source,
            "actual predicate group hashes differ from Source and Native rewrite evidence"
        );
    }
    for source_hash in requested {
        let alias = namespace
            .group_aliases
            .get(&source_hash)
            .context("alias correspondence missing")?;
        ensure!(
            fragment
                .group_aliases
                .iter()
                .any(|row| row.source == source_hash && row.native == alias.native),
            "selector alias rewrite evidence missing"
        );
        for member in &alias.unresolved {
            ensure!(
                fragment.gates.iter().any(|gate| gate.name == *member),
                "unmapped Source group member hidden"
            );
        }
    }
    ensure!(
        !fragment.references.is_empty()
            && fragment
                .references
                .iter()
                .all(|row| row.target == Some(0x81FD1FFD)),
        "private dictionary dependency differs"
    );
    let mut refused = Payload(vec![0xA5; 173]);
    ensure!(
        selectors::append_category(
            &predicate,
            &mut refused,
            FragmentContract {
                root_class: 0x808042CB,
                field: at,
                extent: 104
            },
            &resources,
            &GroupBindings::from_namespace(&emit_static(&source, &stock, &BTreeSet::new())?)
        )
        .is_err()
            && refused.0 == vec![0xA5; 173],
        "default group refusal changed"
    );
    ensure!(
        emit_static(&source, &stock, &BTreeSet::new())?
            .with_group_aliases(&BTreeSet::from([0x01020304]))
            .is_err(),
        "unknown Source group accepted"
    );
    let hash_catalog: Value =
        serde_json::from_slice(&fs::read(corpus.join("collision-corpus/tag64.json"))?)?;
    let hashes = hash_catalog["hashes"]
        .as_object()
        .context("response hash catalog")?
        .iter()
        .map(|(hash, tag)| -> Result<(u64, u32)> {
            Ok((
                u64::from_str_radix(hash, 16)?,
                u32::from_str_radix(tag.as_str().context("response hash tag")?, 16)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let complete = response::emit_with_groups(
        &predicate,
        &load("collision-corpus/shadowkeep/80FB547F.bin")?,
        &load("network-corpus/shadowkeep/80C70B90.bin")?,
        &response::Resources {
            tags: resources.tags.clone(),
            hashes,
            categories: resources.categories.clone(),
            names: resources.names.clone(),
        },
        0x81FD1101,
        0x81FD1102,
        &groups,
    )?;
    ensure!(
        !complete.group_aliases.is_empty(),
        "complete response dropped alias evidence"
    );
    for row in &complete.group_aliases {
        ensure!(
            complete.owner.u32(row.native_offset)? == row.native
                && predicate.u32(row.source_offset)? == row.source,
            "complete response alias offsets differ from actual owner bytes"
        );
    }
    for alias in namespace.group_aliases.values() {
        for member in &alias.unresolved {
            ensure!(
                complete.gates.iter().any(|gate| matches!(gate,
                response::Gate::Predicate { name,.. } if name == member)),
                "complete response dropped an unresolved alias member"
            );
        }
    }
    fs::create_dir_all(&output)?;
    fs::write(output.join("dictionary.bin"), &namespace.payload.0)?;
    fs::write(output.join("predicate-owner.bin"), &owner.0)?;
    fs::write(output.join("response-owner.bin"), &complete.owner.0)?;
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "dictionary_sha256":format!("{:x}",Sha256::digest(&namespace.payload.0)),
            "predicate_sha256":format!("{:x}",Sha256::digest(&owner.0)),
            "aliases":namespace.group_aliases,"rewrites":fragment.group_aliases,"gates":fragment.gates,
            "response_sha256":format!("{:x}",Sha256::digest(&complete.owner.0)),
            "response_rewrites":complete.group_aliases,"response_gates":complete.gates,
            "native_probe":native_receipt,"full_4D73_evaluator_proven":false,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}
