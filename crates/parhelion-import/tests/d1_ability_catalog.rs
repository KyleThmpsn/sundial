//! Complete source corpus contract, written before catalog generalization.
//! Independent source records are the oracle. Unsupported transitive resources stay explicit.
use anyhow::{Context, Result, ensure};
use parhelion_import::{d1_roi::Source, tiger::reader::package_stamp};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, env, fs, path::PathBuf};

fn configured(key: &str) -> Result<PathBuf> {
    env::var_os(key)
        .map(PathBuf::from)
        .with_context(|| format!("Set {key}"))
}

fn number(value: &Value, key: &str) -> Result<u32> {
    Ok(u32::try_from(
        value[key]
            .as_u64()
            .with_context(|| format!("Missing {key}"))?,
    )?)
}

#[test]
#[ignore = "requires D1_SOURCE_PACKAGES, D1_ABILITY_CATALOG_OUTPUT and SUNDIAL_TEST_REVISION"]
#[allow(clippy::cognitive_complexity)]
fn complete_ability_and_subclass_corpus_preserves_declared_execution() -> Result<()> {
    let packages = configured("D1_SOURCE_PACKAGES")?;
    let output = configured("D1_ABILITY_CATALOG_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh output directory");
    let revision = env::var("SUNDIAL_TEST_REVISION")?;
    let source = Source::open(&packages, |_, _| {})?;
    let stamp = package_stamp(&packages)?;
    let abilities = source.ability_catalog()?;
    let subclasses = source.subclass_choices()?;
    ensure!(
        !abilities.is_empty() && !subclasses.is_empty(),
        "Required source corpus is empty"
    );
    let definition_tags = source.classes(0x80803432);
    ensure!(
        definition_tags.len() == 1,
        "Source ability table is missing or ambiguous"
    );
    let definitions = source.read(definition_tags[0])?;
    let rows = definitions.array(8, 40, Some(0x8080343C))?;
    ensure!(
        abilities.len() == rows.len(),
        "Every source ability table row is retained"
    );
    let mut payloads = Vec::new();
    for (index, row) in rows.into_iter().enumerate() {
        let ability = abilities
            .iter()
            .find(|a| usize::from(a.row) == index)
            .context("Source ability ordinal")?;
        ensure!(
            ability.pattern == definitions.u32(row)?,
            "Source pattern identity differs"
        );
        let graph = source.read(ability.graph)?;
        let owners = graph.array(0x20, 12, Some(0x80800715))?;
        let report = &ability.source_inputs;
        let components = report["components"]
            .as_array()
            .context("Component census")?;
        ensure!(
            components.len() == owners.len(),
            "Every component occurrence is retained"
        );
        for (occurrence, (&at, component)) in owners.iter().zip(components).enumerate() {
            let tag = graph.u32(at)?;
            ensure!(
                number(component, "tag")? == tag
                    && number(component, "occurrence")? as usize == occurrence,
                "Shared resources do not collapse component occurrences"
            );
            let owner = source.read(tag)?;
            let runtime = owner.pointer(16)?;
            let definition = owner.pointer(24)?;
            ensure!(
                number(component, "runtime_class")? == owner.u32(runtime - 4)?
                    && number(component, "definition_class")? == owner.u32(definition - 4)?,
                "Concrete owner types survive"
            );
            let lifecycle = component["lifecycle"]["methods"]
                .as_array()
                .context("Lifecycle census")?;
            let methods = owner.array(0x30, 16, Some(0x80800930))?;
            ensure!(
                lifecycle.len() == methods.len(),
                "Constructor and cleanup-only records survive"
            );
            for (&method, actual) in methods.iter().zip(lifecycle) {
                ensure!(
                    actual["target"].as_u64() == Some(owner.pointer(method)? as u64)
                        && number(actual, "implementation")? == owner.u32(method + 8)?
                        && number(actual, "constructor")? == u32::from(owner.u16(method + 12)?)
                        && number(actual, "cleanup")? == u32::from(owner.u16(method + 14)?),
                    "Lifecycle order and methods survive"
                );
            }
            let references = owner.array(0x20, 8, Some(0x8080423F))?;
            let callbacks = component["callbacks"]
                .as_array()
                .context("Callback census")?;
            ensure!(
                references.len() == callbacks.len(),
                "Every declared callback survives"
            );
            for (&reference, actual) in references.iter().zip(callbacks) {
                let at = owner.pointer(reference)?;
                ensure!(
                    actual["offset"].as_u64() == Some((at - 8) as u64)
                        && number(actual, "implementation")? == owner.u32(at - 8)?
                        && number(actual, "method")? == owner.u32(at - 4)?
                        && actual["target"].as_u64() == Some(owner.u64(at + 8)?)
                        && actual["class"].as_u64() == Some(owner.u64(at + 16)?),
                    "Callback target and method identity survive"
                );
            }
            if owner.u32(runtime - 4)? == 0x8080079A {
                let events = component["sequence"]["events"]
                    .as_array()
                    .context("Sequence events")?;
                let mut original = Vec::new();
                for (field, domain) in [(0x170, "Numeric"), (0x180, "External")] {
                    for event in owner.array(definition + field, 8, Some(0x80800629))? {
                        let body = owner.pointer(event)?;
                        original.push((
                            body,
                            owner.u32(body - 4)?,
                            owner.u32(body + 0x1C)?,
                            domain,
                        ));
                    }
                }
                ensure!(
                    events.len() == original.len(),
                    "Unsupported sequence bodies cannot disappear"
                );
                for (event, (offset, class, kind, domain)) in events.iter().zip(original) {
                    ensure!(
                        event["offset"].as_u64() == Some(offset as u64)
                            && number(event, "class")? == class
                            && number(event, "kind")? == kind
                            && event["domain"].as_str() == Some(domain),
                        "Every source sequence event survives in order"
                    );
                }
            }
            payloads.push(json!({"tag":tag,"bytes":owner.0.len(),"sha256":hex::encode(Sha256::digest(&owner.0))}));
        }
        for (field, key) in [(0x30, "connections"), (0x40, "named_connections")] {
            let source_rows = graph.array(field, 88, Some(0x8080054E))?;
            let actual = report["connections"][key]
                .as_array()
                .context("Graph connections")?;
            ensure!(source_rows.len() == actual.len(), "All graph rows survive");
            for (&at, value) in source_rows.iter().zip(actual) {
                for (offset, key) in [(0, "consumer"), (40, "provider")] {
                    let endpoint = &value[key];
                    ensure!(
                        endpoint["namespace"].as_u64() == Some(graph.u64(at + offset)?)
                            && number(endpoint, "owner")? == graph.u32(at + offset + 8)?
                            && endpoint["offset"].as_u64() == Some(graph.u64(at + offset + 16)?)
                            && endpoint["class"].as_u64() == Some(graph.u64(at + offset + 24)?)
                            && endpoint["selector"].as_u64() == Some(graph.u64(at + offset + 32)?),
                        "Endpoint ownership and selector survive"
                    );
                }
                ensure!(
                    value["channel"].as_u64() == Some(graph.u64(at + 80)?),
                    "Dispatch channel survives"
                );
            }
        }
        let bank = source.read(ability.bank)?;
        let bank_rows = bank.array(bank.pointer(24)? + 0xA0, 40, Some(0x80802A84))?;
        let actual = report["bank"]["properties"]
            .as_array()
            .context("Bank properties")?;
        ensure!(
            actual.len() == bank_rows.len(),
            "All bank companions survive"
        );
        for (&at, value) in bank_rows.iter().zip(actual) {
            ensure!(
                value["record"].as_str() == Some(hex::encode(bank.bytes::<40>(at)?).as_str())
                    && number(value, "class")? == bank.u32(bank.pointer(at + 32)? - 4)?,
                "Property order, gates, priority slots and types survive"
            );
        }
        ensure!(
            report["native_emission_verified"] == false
                && !report["unresolved"]
                    .as_array()
                    .context("Explicit remaining boundaries")?
                    .is_empty(),
            "Source census never certifies a complete native graph"
        );
    }
    let item_tables = source.classes(0x80801933);
    ensure!(
        item_tables.len() == 1,
        "Source item table is missing or ambiguous"
    );
    let items = source.read(item_tables[0])?;
    let mut expected_items = BTreeSet::new();
    for row in items.array(8, 24, Some(0x808016BD))? {
        let item = source.read(items.u32(row + 16)?)?;
        if item.u64(0x18)? != 0 && item.u8(item.pointer(0x18)? + 0x18)? == 0 {
            expected_items.insert(items.u32(row)?);
        }
    }
    ensure!(
        !expected_items.is_empty()
            && subclasses.iter().map(|s| s.item).collect::<BTreeSet<_>>() == expected_items,
        "Every source subclass item survives, including shared-grid identities"
    );
    for subclass in &subclasses {
        let shared = source.read(number(&subclass.source_inputs, "shared_grid")?)?;
        let source_nodes = shared.array(0x10, 64, Some(0x80801728))?;
        ensure!(
            subclass.nodes.len() == source_nodes.len(),
            "Every talent node survives"
        );
        let mut referenced = BTreeSet::new();
        let mut equipped_super = BTreeSet::new();
        for (&at, node) in source_nodes.iter().zip(&subclass.nodes) {
            ensure!(
                node.record == hex::encode(shared.bytes::<64>(at)?),
                "Raw node metadata survives"
            );
            for step in &node.steps {
                for variant in step.source["variants"]
                    .as_array()
                    .context("Source variants")?
                {
                    for grant in variant["ability_grants"]
                        .as_array()
                        .context("Source grants")?
                    {
                        let bytes = hex::decode(grant.as_str().context("Source grant bytes")?)?;
                        for row in [bytes[4], bytes[11]] {
                            if row != 255 {
                                referenced.insert(row);
                            }
                        }
                        if bytes[11] != 255 && bytes[12] == 1 {
                            equipped_super.insert(bytes[11]);
                        }
                    }
                }
            }
        }
        ensure!(
            equipped_super == BTreeSet::from([subclass.ability_row]),
            "Primary ability follows the equipped Super bucket"
        );
        ensure!(
            subclass
                .abilities
                .iter()
                .map(|a| a.row)
                .collect::<BTreeSet<_>>()
                == referenced,
            "All grant targets retain their graph census"
        );
    }
    ensure!(
        source.revival_choices()?.iter().all(|choice| subclasses
            .iter()
            .any(|s| s.item == choice.subclass && s.controller_class == 0x808025B8)),
        "Broader intake cannot admit another controller to the bounded revival path"
    );
    ensure!(
        package_stamp(&packages)? == stamp,
        "Source packages remain unchanged"
    );
    fs::create_dir_all(&output)?;
    let saved = json!({"abilities":abilities,"subclasses":subclasses});
    let path = output.join("catalog.json");
    fs::write(&path, serde_json::to_vec_pretty(&saved)?)?;
    let reloaded: Value = serde_json::from_slice(&fs::read(&path)?)?;
    ensure!(
        reloaded == saved,
        "Complete source contributions and census survive reload"
    );
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "revision":revision,"source_builds":source.report().builds,"source_stamp":stamp,
            "ability_count":saved["abilities"].as_array().unwrap().len(),"subclass_count":expected_items.len(),
            "source_payloads":payloads,"catalog_sha256":hex::encode(Sha256::digest(fs::read(path)?)),
            "executable_sha256":hex::encode(Sha256::digest(fs::read(env::current_exe()?)?)),
            "package_emitted":false,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}
