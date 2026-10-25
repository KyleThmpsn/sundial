//! Source packages through private action preparation, resource linking and persisted readback.
//! Failure inventory is recorded before the implementation in the ability linkage acceptance.
#![cfg(feature = "d2-model-importer")]
use anyhow::{Context, Result, ensure};
use parhelion_import::d1_roi::ability::{animation, dependency};
use parhelion_import::{
    d1_roi::Source,
    tiger::{
        payload::Payload,
        reader::{Reader, package_stamp},
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::PathBuf,
};

fn configured(key: &str) -> Result<PathBuf> {
    env::var_os(key)
        .map(PathBuf::from)
        .with_context(|| format!("Set {key}"))
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn word(value: &Value, key: &str) -> Result<u32> {
    Ok(u32::try_from(
        value[key]
            .as_u64()
            .with_context(|| format!("Missing {key}"))?,
    )?)
}

#[test]
#[ignore = "Requires D1_ABILITY_ANIMATION_PLAN, D1_ABILITY_ANIMATION_OUTPUT, D1_SOURCE_PACKAGES, SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_REVISION"]
fn source_actions_link_privately_and_preserve_stock_routes() -> Result<()> {
    let output = configured("D1_ABILITY_ANIMATION_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh artifact directory");
    let configuration: Value =
        serde_json::from_slice(&fs::read(configured("D1_ABILITY_ANIMATION_PLAN")?)?)?;
    let revision = env::var("SUNDIAL_TEST_REVISION")?;
    let source = Source::open(&configured("D1_SOURCE_PACKAGES")?, |_, _| {})?;
    let native_path = configured("SUNDIAL_STOCK_PACKAGES")?;
    let stamp = package_stamp(&native_path)?;
    let mut native = Reader::new(&native_path, &output.join("native"), false)?;
    let target = animation::Target {
        lookup: word(&configuration, "lookup")?,
        pose: word(&configuration, "pose")?,
        skeleton: word(&configuration, "skeleton")?,
        controls: word(&configuration, "controls")?,
        source_controls: word(&configuration, "source_controls")?,
        source_pose: word(&configuration, "source_pose")?,
    };
    let actions = configuration["actions"]
        .as_array()
        .context("Action corpus")?
        .iter()
        .map(|value| Ok(u32::try_from(value.as_u64().context("Action name")?)?))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(!actions.is_empty(), "Required action corpus is empty");
    let plan = animation::read(&source, word(&configuration, "source_lookup")?, &actions)?;
    let resources = configuration["resources"]
        .as_array()
        .context("Resource mappings")?
        .iter()
        .map(|row| {
            Ok((
                word(row, "source")?,
                row["symbol"]
                    .as_str()
                    .context("Resource symbol")?
                    .to_owned(),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let dependencies = configuration["dependencies"]
        .as_array()
        .context("Dependency closure")?
        .iter()
        .map(|row| {
            let patches = row["patches"]
                .as_array()
                .context("Dependency patches")?
                .iter()
                .map(|patch| {
                    Ok(dependency::Patch {
                        offset: usize::try_from(patch["offset"].as_u64().context("Patch offset")?)?,
                        symbol: patch["symbol"].as_str().context("Patch symbol")?.to_owned(),
                        class: word(patch, "class")?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(dependency::Asset {
                symbol: row["symbol"]
                    .as_str()
                    .context("Dependency symbol")?
                    .to_owned(),
                class: word(row, "class")?,
                payload: Payload(fs::read(row["file"].as_str().context("Dependency file")?)?),
                patches,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let original_lookup = (*native.tag(target.lookup, Some(0x80809C36))?).clone();
    let original_pose = (*native.tag(target.pose, Some(0x80809C36))?).clone();
    let lp = original_lookup.pointer(24)?;
    let original_states = native.tag(original_lookup.u32(lp + 0x9C)?, Some(0x80803465))?;
    let original_names = original_states
        .array(8, 8, Some(0x8080342E))?
        .into_iter()
        .map(|at| Ok((original_states.u32(at)?, original_states.u32(at + 4)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let prepared = animation::prepare(
        &source,
        &mut native,
        &plan,
        &target,
        "acceptance-ability",
        &resources,
        &dependencies,
    )?;
    let allocations = prepared
        .assets
        .iter()
        .enumerate()
        .map(|(index, asset)| {
            Ok((
                asset.symbol.clone(),
                0x8FFF0000u32
                    .checked_add(u32::try_from(index)?)
                    .context("Allocation overflow")?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let owners = BTreeMap::from([
        (target.lookup, original_lookup),
        (target.pose, original_pose),
    ]);
    let bound = prepared.bind(&owners, &allocations)?;
    let mut files = BTreeMap::new();
    for asset in &bound.assets {
        let path = output.join(format!("{:08X}.bin", asset.tag));
        fs::write(&path, &asset.payload.0)?;
        files.insert(asset.tag, (asset.class, path));
    }
    for (tag, owner) in &bound.owners {
        let path = output.join(format!("{tag:08X}.bin"));
        fs::write(&path, &owner.0)?;
        ensure!(
            files.insert(*tag, (0x80809C36, path)).is_none(),
            "Owner allocation collides"
        );
    }
    let reload = |tag: u32, class: u32| -> Result<Payload> {
        let (actual, file) = files.get(&tag).context("Linked resource is missing")?;
        ensure!(*actual == class, "Linked resource class differs");
        Ok(Payload(fs::read(file)?))
    };
    for asset in &prepared.assets {
        let linked = reload(allocations[&asset.symbol], asset.class)?;
        for patch in &asset.patches {
            let tag = linked.u32(patch.offset)?;
            reload(tag, patch.class)?;
            ensure!(
                tag == allocations[&patch.symbol],
                "Persisted resource identity differs"
            );
        }
    }
    let lookup = reload(target.lookup, 0x80809C36)?;
    let pose = reload(target.pose, 0x80809C36)?;
    for (tag, owner) in &bound.owners {
        ensure!(
            reload(*tag, 0x80809C36)?.0 == owner.0,
            "Persisted consumer differs"
        );
    }
    let bank_tag = lookup.u32(lp + 0x90)?;
    ensure!(
        bank_tag == pose.u32(pose.pointer(24)? + 0x108)?,
        "Lookup and pose bank differ"
    );
    let bank = reload(bank_tag, 0x808036F6)?;
    let states = reload(lookup.u32(lp + 0x9C)?, 0x80803465)?;
    let names = states
        .array(8, 8, Some(0x8080342E))?
        .into_iter()
        .map(|at| Ok((states.u32(at)?, states.u32(at + 4)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    for (name, ordinal) in &original_names {
        ensure!(
            names.get(name) == Some(ordinal),
            "Stock action route changed"
        );
    }
    let nodes = states.array(24, 16, Some(0x8080342F))?;
    let descriptors = bank.array(0x68, 32, Some(0x80809002))?;
    let clips = bank.array(8, 4, Some(0x80808F48))?;
    let mut observed = Vec::new();
    for action in &plan.actions {
        let private = prepared.actions[&action.name];
        ensure!(
            private != action.name && !original_names.contains_key(&private),
            "Action is not private"
        );
        let node = nodes[usize::try_from(names[&private])?];
        ensure!(states.u64(node)? == 1, "Unexpected action dispatch");
        let weights = states.array(states.pointer(node + 8)?, 8, Some(0x80803439))?;
        ensure!(
            weights.len() == action.choices.len(),
            "Weighted variants were lost"
        );
        for (row, source_choice) in weights.into_iter().zip(&action.choices) {
            ensure!(
                states.u32(row + 4)? == source_choice.weight,
                "Weight changed"
            );
            let descriptor = descriptors[usize::try_from(states.u32(row)?)?];
            let clip = bank.u32(clips[usize::from(bank.u16(descriptor + 24)?)])?;
            let payload = reload(clip, 0x80808F49)?;
            observed.push(json!({"source_action":action.name,"private_action":private,"source_clip":source_choice.clip,"clip":clip,"sha256":sha(&payload.0)}));
        }
    }
    // A stale consumer must fail without changing either caller-owned input.
    let mut stale = owners.clone();
    stale.get_mut(&target.lookup).unwrap().0[lp + 0x90] ^= 1;
    ensure!(
        prepared.bind(&stale, &allocations).is_err(),
        "Stale owner accepted"
    );
    ensure!(
        package_stamp(&native_path)? == stamp,
        "Native package inputs changed"
    );
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "revision":revision,"configuration":configuration,"source_plan":plan,"native_stamp":stamp,
            "allocations":allocations,"observed":observed,"preparation":prepared.evidence,
            "binding":bound.evidence,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}
