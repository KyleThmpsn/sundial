//! Configured original cues through complete audio preparation and persisted private profiles.
use anyhow::{Context, Result, ensure};
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
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    time::Instant,
};

fn configured(key: &str) -> Result<PathBuf> {
    env::var_os(key)
        .map(PathBuf::from)
        .with_context(|| format!("Set {key}"))
}

fn number(value: &Value) -> Result<u32> {
    Ok(u32::try_from(value.as_u64().context("Integer identity")?)?)
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn node_file(root: &Path, node: &Value) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let file = root
        .join(node["file"].as_str().context("Node file")?)
        .canonicalize()?;
    ensure!(file.starts_with(root), "Node escaped its prepared folder");
    Ok(file)
}

fn target<'a>(
    node: &Value,
    offset: usize,
    nodes: &'a BTreeMap<String, Value>,
) -> Result<&'a Value> {
    let patches = node["patches"].as_array().context("Node relocations")?;
    let matching = patches
        .iter()
        .filter(|p| p["offset"].as_u64() == Some(offset as u64))
        .collect::<Vec<_>>();
    ensure!(matching.len() == 1, "Cue reference is absent or ambiguous");
    nodes
        .get(matching[0]["symbol"].as_str().context("Target symbol")?)
        .context("Unpublished cue target")
}

#[test]
#[ignore = "Requires D1_AUDIO_PLAN, D1_AUDIO_OUTPUT, D1_SOURCE_PACKAGES, SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_REVISION"]
#[allow(clippy::cognitive_complexity)]
fn source_cues_preserve_private_profiles_after_preparation_and_reload() -> Result<()> {
    let began = Instant::now();
    let output = configured("D1_AUDIO_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh output folder");
    let plan_path = configured("D1_AUDIO_PLAN")?;
    let plan_bytes = fs::read(&plan_path)?;
    let plan: Vec<Value> = serde_json::from_slice(&plan_bytes)?;
    ensure!(!plan.is_empty(), "Required source cue corpus is empty");
    let packages = configured("D1_SOURCE_PACKAGES")?;
    let native = configured("SUNDIAL_STOCK_PACKAGES")?;
    let revision = env::var("SUNDIAL_TEST_REVISION")?;
    let test_binary_sha256 = digest(&fs::read(env::current_exe()?)?);
    let source = Source::open(&packages, |_, _| {})?;
    let source_stamp = package_stamp(&packages)?;
    let native_stamp = package_stamp(&native)?;
    fs::create_dir_all(&output)?;
    fs::write(output.join("plan.json"), &plan_bytes)?;
    let mut stock = Reader::new(&native, &output.join("stock"), false)?;
    let mut unchanged = BTreeMap::new();
    let mut artifacts = Vec::new();
    let mut exact = 0;
    let mut translated = 0;
    for (index, case) in plan.iter().enumerate() {
        let cues: Vec<u32> = serde_json::from_value(case["cues"].clone())?;
        ensure!(!cues.is_empty(), "Required cue case is empty");
        let folder = output.join(format!("case-{index}"));
        let prepared = source.prepare_audio(&cues, &native, &folder);
        if let Some(reason) = case["unsupported"].as_str() {
            ensure!(!reason.is_empty(), "Unsupported case needs a reason");
            let error = prepared.expect_err("Unsupported cue closure was published");
            ensure!(
                format!("{error:#}").contains(reason),
                "Unexpected rejection: {error:#}"
            );
            ensure!(
                !folder.join("audio-graph.json").exists(),
                "Rejected closure published a graph"
            );
            artifacts.push(json!({"case":case,"unsupported":format!("{error:#}")}));
            continue;
        }
        let manifest = prepared?;
        let graph_bytes = fs::read(&manifest)?;
        let graph: Value = serde_json::from_slice(&graph_bytes)?;
        let mut nodes = BTreeMap::new();
        let mut files = Vec::new();
        for node in graph["nodes"].as_array().context("Prepared nodes")? {
            let symbol = node["symbol"].as_str().context("Node symbol")?.to_owned();
            ensure!(
                nodes.insert(symbol, node.clone()).is_none(),
                "Duplicate graph symbol"
            );
            let bytes = fs::read(node_file(&folder, node)?)?;
            ensure!(!bytes.is_empty(), "Empty required audio payload");
            files.push(json!({"file":node["file"],"sha256":digest(&bytes)}));
        }
        let mut profiles = Vec::new();
        let mut changed = false;
        for cue in &cues {
            ensure!(source.reference(*cue)? == 0x8080080A, "Source cue class");
            let original = source.read(*cue)?;
            let profile_tag = original.u32(0x30)?;
            ensure!(
                source.reference(profile_tag)? == 0x80800714,
                "Source profile class"
            );
            let profile = source.read(profile_tag)?;
            let matching = nodes
                .values()
                .filter(|n| n["source_cue_tag"].as_u64() == Some(u64::from(*cue)))
                .collect::<Vec<_>>();
            ensure!(matching.len() == 1, "Required source cue disappeared");
            let node = matching[0];
            let emitted = Payload(fs::read(node_file(&folder, node)?)?);
            ensure!(emitted.u64(0)? == emitted.0.len() as u64, "Cue size word");
            ensure!(
                emitted.bytes::<8>(8)? == original.bytes::<8>(8)?,
                "Prepared cue lost source event identities"
            );
            ensure!(
                emitted.u32(0x10)? == u32::MAX,
                "Cue points to stock settings before relocation"
            );
            let settings = target(node, 0x10, &nodes)?;
            let actual = Payload(fs::read(node_file(&folder, settings)?)?);
            ensure!(
                actual.0 == profile.0,
                "Persisted cue profile changed a source field"
            );
            ensure!(
                actual.0.len() == 24 && actual.u64(0)? == 24,
                "Profile extent"
            );
            let template = number(&settings["template"])?;
            let witness = stock.tag(template, Some(0x80808E68))?;
            unchanged.insert(template, witness.0.clone());
            ensure!(
                witness.0.len() == 24 && witness.u64(0)? == 24,
                "Native witness extent"
            );
            ensure!(
                actual.0[..12] == witness.0[..12] && actual.0[16..] == witness.0[16..],
                "Unqualified profile field changed from the native witness"
            );
            let differs = actual.u32(12)? != witness.u32(12)?;
            if differs {
                ensure!(
                    actual.f32(12)?.is_finite() && witness.f32(12)?.is_finite(),
                    "Translated profile limit is not finite"
                );
                translated += 1;
                changed = true;
            } else {
                exact += 1;
            }
            let bank = target(node, 0x14, &nodes)?;
            ensure!(
                bank["source_bank_tag"].as_u64() == Some(u64::from(original.u32(0x34)?)),
                "Cue substituted a native bank"
            );
            let media = emitted.array(0x18, 4, Some(0x80800014))?;
            let source_media = original.array(0x38, 4, Some(0x80800014))?;
            ensure!(media.len() == source_media.len(), "Cue media count changed");
            for (&at, &source_at) in media.iter().zip(&source_media) {
                let tag = original.u32(source_at)?;
                ensure!(
                    emitted.u32(at)? == u32::MAX,
                    "Cue retained an unresolved stock medium"
                );
                if tag != u32::MAX {
                    let medium = target(node, at, &nodes)?;
                    ensure!(
                        medium["source_media_id"].as_u64()
                            == Some(u64::from(source.reference(tag)?)),
                        "Cue medium identity changed"
                    );
                    ensure!(
                        fs::read(node_file(&folder, medium)?)?.starts_with(b"RIFF"),
                        "Prepared PCM medium missing"
                    );
                }
            }
            profiles.push(json!({"cue":cue,"source_cue_sha256":digest(&original.0),"source_profile":profile_tag,
                "source_profile_sha256":digest(&profile.0),"emitted_profile_sha256":digest(&actual.0),
                "native_witness":template,"native_witness_sha256":digest(&witness.0),
                "source_limit_bits":actual.u32(12)?,"native_limit_bits":witness.u32(12)?,"translated":differs}));
        }
        ensure!(
            case["changed_limit"]
                .as_bool()
                .context("Declare changed_limit")?
                == changed,
            "Required profile translation path was not exercised"
        );
        artifacts.push(json!({"case":case,"graph_sha256":digest(&graph_bytes),"profiles":profiles,"files":files}));
    }
    ensure!(
        exact > 0 && translated > 0,
        "Corpus must exercise exact and translated profile publication"
    );
    drop(stock);
    let mut stock = Reader::new(&native, &output.join("stock-after"), false)?;
    for (tag, expected) in unchanged {
        ensure!(
            stock.tag(tag, Some(0x80808E68))?.0 == expected,
            "Stock profile was mutated"
        );
    }
    ensure!(
        package_stamp(&packages)? == source_stamp && package_stamp(&native)? == native_stamp,
        "Mounted inputs changed"
    );
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "revision":revision,"test_binary_sha256":test_binary_sha256,
            "converter_revision":parhelion_import::CONVERTER_REVISION,
            "source":source.report(),"native_build":"86657.20.08.23.1800.d2_rc___release",
            "source_stamp":source_stamp,"native_stamp":native_stamp,"plan_sha256":digest(&plan_bytes),
            "artifacts":artifacts,"exact_profiles":exact,"translated_profiles":translated,
            "seconds":began.elapsed().as_secs_f64(),
            "limits":"Complete audio preparation and persisted profile readback. No native package link, loader or gameplay execution."
        }))?,
    )?;
    Ok(())
}
