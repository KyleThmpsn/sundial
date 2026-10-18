//! Configured cache routing against an independently decoded Assembly witness.
use super::*;
use crate::halo_reach::{animation::first_person::Action, resource::Pages};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

#[test]
#[ignore = "Requires a configured source cache, independent route witness and fresh output"]
fn middleware_source_action_routes() -> Result<()> {
    let path = |key| {
        env::var_os(key)
            .map(PathBuf::from)
            .with_context(|| format!("Set {key}"))
    };
    let cache_path = path("PARHELION_ROUTE_CACHE")?;
    let witness_bytes = fs::read(path("PARHELION_ROUTE_WITNESS")?)?;
    let witness: Value = serde_json::from_slice(&witness_bytes)?;
    let output = path("PARHELION_ROUTE_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh route output");
    let cache = Cache::open(&cache_path)?;
    let mut pages = Pages::default();
    pages.record(&cache_path)?;
    ensure!(
        pages.inputs[&cache_path].sha256 == witness["cache_sha256"],
        "Source cache changed"
    );
    let entries = witness["entries"]
        .as_array()
        .context("Independent routes")?;
    let mut graphs = std::collections::BTreeMap::new();
    let mut results = Vec::new();
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    let mut exclusions = Vec::new();
    for expected in entries {
        let kind = match expected["kind"].as_str() {
            Some("Actions") => "action",
            Some("Overlays") => "overlay",
            Some("Transitions") => "transition",
            _ => anyhow::bail!("Unknown required route kind"),
        };
        let graph_name = expected["graph"].as_str().context("Witness graph")?;
        if !graphs.contains_key(graph_name) {
            let tag = cache.find("jmad", graph_name)?;
            graphs.insert(graph_name.to_owned(), read(&cache, tag.address()?)?);
        }
        let graph = json!({"routes":graphs[graph_name]});
        let mut request = json!({"mode":expected["mode"],"weapon_class":expected["weapon_class"],
            "weapon_type":expected["weapon_type"],"set":expected["set"],"state":expected["state"]});
        if kind != "action" {
            request["kind"] = json!(kind);
        }
        if kind == "transition" {
            request["destination"] =
                json!({"mode":expected["destination_mode"],"state":expected["destination_state"]});
        }
        let action: Action = serde_json::from_value(request.clone())?;
        if expected["animation_index"] == -1 {
            ensure!(
                action.names(&graph).is_err(),
                "Absent source animation selected a clip"
            );
            exclusions.push(json!({"graph":graph_name,"action":action,"reason":"Source animation index is the absent sentinel"}));
            continue;
        }
        let selected = action.names(&graph)?;
        ensure!(
            selected
                == [expected["local_candidate"]
                    .as_str()
                    .context("Witness clip")?],
            "Route selected a different clip: {request}"
        );
        // Persist recipes and resolve again from disk, including old clip and sequence syntax.
        results.push(json!({"graph":graph_name,"action":action,"clip":selected[0]}));
        *counts.entry(kind.to_owned()).or_default() += 1;
    }
    ensure!(!results.is_empty(), "Required action corpus is empty");
    fs::create_dir_all(&output)?;
    fs::write(
        output.join("actions.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    let persisted: Vec<Value> = serde_json::from_slice(&fs::read(output.join("actions.json"))?)?;
    for row in &persisted {
        let graph = json!({"routes":graphs[row["graph"].as_str().context("Saved graph")?]});
        for value in [
            row["action"].clone(),
            row["clip"].clone(),
            json!([row["clip"], row["clip"]]),
        ] {
            let action: Action = serde_json::from_value(value)?;
            ensure!(
                action
                    .names(&graph)?
                    .iter()
                    .all(|name| *name == row["clip"].as_str().unwrap()),
                "Saved action changed"
            );
        }
        let mut missing = row["action"].clone();
        missing["weapon_type"] = json!("__absent_route__");
        let action: Action = serde_json::from_value(missing)?;
        ensure!(
            action.names(&graph).is_err(),
            "Missing route silently fell back"
        );
    }
    pages.verify_inputs()?;
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "converter_revision":crate::graph::CONVERTER_REVISION,"build":cache.build,
            "inputs":pages.inputs,"witness_sha256":hex::encode(Sha256::digest(&witness_bytes)),
            "routes":results.len(),"graphs":graphs.len(),
            "kinds":counts,"exclusions":exclusions,
            "actions_sha256":hex::encode(Sha256::digest(fs::read(output.join("actions.json"))?)),
            "limits":["Source route selection and persisted recipes. Native timing and live transitions use the existing clip translation and require gameplay acceptance."]
        }))?,
    )?;
    Ok(())
}

/// Exercise shared-clip export against separately extracted and decoded source channels.
#[test]
#[ignore = "Requires a configured cache, independent motion witness and fresh output"]
fn middleware_shared_graph_motion() -> Result<()> {
    let witness_path = PathBuf::from(
        env::var_os("PARHELION_SHARED_WITNESS").context("Set PARHELION_SHARED_WITNESS")?,
    );
    let witness_bytes = fs::read(&witness_path)?;
    let witness: Value = serde_json::from_slice(&witness_bytes)?;
    let input =
        PathBuf::from(env::var_os("PARHELION_SHARED_CACHE").context("Set PARHELION_SHARED_CACHE")?);
    let output = PathBuf::from(
        env::var_os("PARHELION_SHARED_OUTPUT").context("Set PARHELION_SHARED_OUTPUT")?,
    );
    ensure!(!output.exists(), "Use a fresh shared motion output");
    let cache = Cache::open(&input)?;
    let mut pages = Pages::default();
    pages.record(&input)?;
    ensure!(
        pages.inputs[&input].sha256 == witness["cache_sha256"],
        "Shared motion cache changed"
    );
    let cases = witness["clips"]
        .as_array()
        .context("Independent shared motion cases")?;
    ensure!(!cases.is_empty(), "Required shared motion corpus is empty");
    let mut results = Vec::new();
    fs::create_dir_all(&output)?;
    for (index, expected) in cases.iter().enumerate() {
        let tag = cache.find("jmad", expected["graph"].as_str().context("Shared graph")?)?;
        let mut data = crate::halo_reach::behavior::animation_data(&cache, tag)?;
        data["clips"]
            .as_array_mut()
            .context("Source clips")?
            .retain(|c| c["name"] == expected["name"]);
        ensure!(
            data["clips"].as_array().unwrap().len() == 1,
            "Shared source clip is absent or ambiguous"
        );
        let root = output.join(index.to_string());
        fs::create_dir(&root)?;
        let graph = super::super::export_graph(
            &cache,
            &mut pages,
            &json!({"tag":tag,"data":data}),
            Some(&Default::default()),
            &root,
        )?;
        let shared = &graph["clips"][0]["shared"][0];
        let member = super::super::playback::member(&graph, shared)?;
        ensure!(
            member["output"]["sha256"] == expected["compressed_sha256"],
            "Shared source resource identity differs"
        );
        let motion_file = root.join(
            member["motion"]["file"]
                .as_str()
                .context("Resolved shared motion")?,
        );
        let actual: super::super::codec::Motion = serde_json::from_slice(&fs::read(&motion_file)?)?;
        let expected_file = witness_path.parent().context("Witness parent")?.join(
            expected["expected_file"]
                .as_str()
                .context("Independent decoded channels")?,
        );
        let wanted: super::super::codec::Motion =
            serde_json::from_slice(&fs::read(expected_file)?)?;
        ensure!(
            actual.frames == wanted.frames && actual.tracks.len() == wanted.tracks.len(),
            "Shared motion dimensions changed"
        );
        let mut components = 0;
        for (actual, wanted) in actual.tracks.iter().zip(&wanted.tracks) {
            for (got, expected) in [
                (
                    actual
                        .rotation
                        .iter()
                        .flatten()
                        .copied()
                        .collect::<Vec<_>>(),
                    wanted
                        .rotation
                        .iter()
                        .flatten()
                        .copied()
                        .collect::<Vec<_>>(),
                ),
                (
                    actual.translation.iter().flatten().copied().collect(),
                    wanted.translation.iter().flatten().copied().collect(),
                ),
                (actual.scale.clone(), wanted.scale.clone()),
            ] {
                ensure!(
                    got.len() == expected.len(),
                    "Shared motion channel is missing"
                );
                let extent = expected.iter().fold(1f32, |a, b| a.max(b.abs()));
                for (&a, &b) in got.iter().zip(&expected) {
                    ensure!(
                        a.is_finite()
                            && b.is_finite()
                            && (a - b).abs()
                                <= (5e-5 * b.abs().max(1.)).max(8. * f32::EPSILON * extent),
                        "Shared motion remap changed a decoded component: {a} versus {b}"
                    );
                }
                components += got.len();
            }
        }
        ensure!(components > 0, "Shared motion has no observable channels");
        fs::write(root.join("graph.json"), serde_json::to_vec_pretty(&graph)?)?;
        results.push(json!({"case":expected,"components":components,"graph":format!("{index}/graph.json"),"motion_sha256":hex::encode(Sha256::digest(fs::read(motion_file)?))}));
    }
    pages.verify_inputs()?;
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(
            &json!({"converter_revision":crate::graph::CONVERTER_REVISION,"build":cache.build,"inputs":pages.inputs,"witness_sha256":hex::encode(Sha256::digest(witness_bytes)),"results":results,"limits":["Decoded shared motion and node-map export. Native retargeting and runtime transition selection need separate verification."]}),
        )?,
    )?;
    Ok(())
}
