//! Configured source-cache collision pipeline with an independent inverse-bind witness.
use super::*;
use crate::halo_reach::{ExportRequest, cache::Cache, resource::Pages};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

#[test]
#[ignore = "Requires configured Reach maps, native template, independent capsule witness and fresh output"]
fn source_capsules_to_native_packfiles() -> Result<()> {
    let maps = configured("PARHELION_REACH_MAPS")?;
    let witness_path = configured("PARHELION_CAPSULE_WITNESS")?;
    let template = fs::read(configured("PARHELION_COLLISION_TEMPLATE")?)?;
    let output = configured("PARHELION_CAPSULE_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh collision output");
    let witness_bytes = fs::read(witness_path)?;
    let witness: Value = serde_json::from_slice(&witness_bytes)?;
    let cases = witness["cases"]
        .as_array()
        .context("Collision witness cases")?;
    ensure!(!cases.is_empty(), "Required collision corpus is empty");
    fs::create_dir_all(&output)?;
    let bones = json!({"bones":[{"inverse_object_transform":witness["native_inverse"]}]});
    let mut receipt = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let cache_name = case["cache"].as_str().context("Cache name")?;
        let cache_path = crate::halo_reach::source_path(&maps, cache_name)?;
        let cache = Cache::open(&cache_path)?;
        let request: ExportRequest = serde_json::from_value(json!({
            "cache":cache_name,"group":"vehi","path":case["path"]
        }))?;
        let mut pages = Pages::default();
        pages.record(&cache_path)?;
        ensure!(
            pages.inputs[&cache_path].sha256 == case["cache_sha256"],
            "Source cache changed"
        );
        let scene = crate::halo_reach::scene(&cache, &mut pages, &request)?;
        let bodies = scene.object.gameplay["rigid_body"]["bodies"]
            .as_array()
            .context("Bodies")?;
        let mut shapes = Vec::new();
        for body in 0..bodies.len() {
            shapes.extend(hull::build(&scene, &bones, body)?);
        }
        let groups = (0..shapes.len()).map(|i| vec![i]).collect::<Vec<_>>();
        let bytes = crate::tiger::physics::vehicle_shapes(&template, &shapes, &groups, &[])?;
        let path = output.join(format!("{index}.hkx"));
        fs::write(&path, bytes)?;
        let saved = fs::read(&path)?;
        verify_capsules(&saved, case)?;
        // Fragment compounds also consume this writer. Persist a mixed compound for native replay.
        let compound = crate::tiger::physics::vehicle_shapes(
            &template,
            &shapes,
            &[(0..shapes.len()).collect()],
            &[],
        )?;
        fs::write(output.join(format!("{index}-compound.hkx")), &compound)?;
        pages.verify_inputs()?;
        receipt.push(json!({"file":format!("{index}.hkx"),"source":case,
            "inputs":pages.inputs,"sha256":hex::encode(Sha256::digest(&saved)),
            "compound_sha256":hex::encode(Sha256::digest(&compound))}));
    }
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "converter_revision":crate::graph::CONVERTER_REVISION,
            "witness_sha256":hex::encode(Sha256::digest(&witness_bytes)),
            "template_sha256":hex::encode(Sha256::digest(&template)),"cases":receipt,
            "limits":["Collision translation and persisted geometry only. No complete vehicle or gameplay claim."]
        }))?,
    )?;
    Ok(())
}

/// Follow serialized fixups, rather than writer offsets or in-memory Shape values.
fn verify_capsules(bytes: &[u8], witness: &Value) -> Result<()> {
    let u32_at = |at: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            bytes.get(at..at + 4).context("Packfile word")?.try_into()?,
        ))
    };
    let data = u32_at(0xb4)? as usize;
    let names = u32_at(0x54)? as usize;
    let mut globals = std::collections::BTreeMap::new();
    let mut finishes = std::collections::BTreeMap::new();
    let mut slots = None;
    for at in (data + u32_at(0xb8)? as usize..data + u32_at(0xbc)? as usize).step_by(8) {
        let field = u32_at(at)?;
        if field == u32::MAX {
            break;
        }
        if field == 32 {
            slots = Some(u32_at(at + 4)? as usize);
        }
    }
    for at in (data + u32_at(0xbc)? as usize..data + u32_at(0xc0)? as usize).step_by(12) {
        let field = u32_at(at)?;
        if field == u32::MAX {
            break;
        }
        ensure!(u32_at(at + 4)? == 2, "Shape points outside data section");
        globals.insert(field as usize, u32_at(at + 8)? as usize);
    }
    for at in (data + u32_at(0xc0)? as usize..data + u32_at(0xc4)? as usize).step_by(12) {
        let object = u32_at(at)?;
        if object == u32::MAX {
            break;
        }
        ensure!(u32_at(at + 4)? == 0, "Finish name section");
        let start = names + u32_at(at + 8)? as usize;
        let end = bytes[start..]
            .iter()
            .position(|&v| v == 0)
            .context("Class terminator")?;
        finishes.insert(object as usize, &bytes[start..start + end]);
    }
    ensure!(
        u32_at(data + 40)? as u64 == witness["leaf_count"].as_u64().context("Leaf count")?,
        "Root shape count differs"
    );
    let expected = witness["capsules"].as_array().context("Capsule witness")?;
    ensure!(
        finishes
            .values()
            .filter(|&&name| name == b"hkpCapsuleShape")
            .count()
            == expected.len(),
        "Capsule count differs"
    );
    let slots = slots.context("Root shape array")?;
    for capsule in expected {
        let slot = capsule["slot"].as_u64().context("Capsule slot")? as usize;
        let object = *globals
            .get(&(slots + slot * 8))
            .context("Root shape pointer")?;
        ensure!(
            finishes.get(&object) == Some(&b"hkpCapsuleShape".as_slice()),
            "Root does not select capsule"
        );
        for (field, values) in [
            (0x20, vec![capsule["radius"].as_f64().context("Radius")?]),
            (0x30, vector::<3>(&capsule["a"])?.map(f64::from).to_vec()),
            (0x40, vector::<3>(&capsule["b"])?.map(f64::from).to_vec()),
        ] {
            for (axis, expected) in values.into_iter().enumerate() {
                let actual = f32::from_bits(u32_at(data + object + field + axis * 4)?) as f64;
                ensure!(
                    (actual - expected).abs() < 0.0002,
                    "Persisted capsule geometry differs: {actual} vs {expected}"
                );
            }
        }
    }
    Ok(())
}
