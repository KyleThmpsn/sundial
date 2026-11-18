//! Package-to-import verification of marker bone identity and independent source bind targets.
use crate::*;
use parhelion_import::tiger::animation::pose::Pose;
use parhelion_import::tiger::reader::package_stamp;
use sha2::{Digest, Sha256};

fn pose(payload: &Payload, at: usize) -> Result<Pose> {
    let mut values = [0.; 8];
    for (lane, value) in values.iter_mut().enumerate() {
        *value = payload.f32(at + lane * 4)?;
    }
    Pose::checked(values)
}

#[test]
#[ignore = "Requires configured Marathon and native packages, an import plan and independent marker expectations"]
fn prepared_markers_keep_their_equipment_bones_and_source_bind_targets() -> Result<()> {
    let revision = env::var("SUNDIAL_TEST_REVISION")?;
    let test_binary_sha256 = hex::encode(Sha256::digest(fs::read(env::current_exe()?)?));
    let packages = configured("PARHELION_MARATHON_PACKAGES")?;
    let native = configured("PARHELION_IMPORT_NATIVE_PACKAGES")?;
    let source_stamp = package_stamp(&packages)?;
    let native_stamp = package_stamp(&native)?;
    let expectation_bytes = fs::read(configured("PARHELION_MARATHON_MARKER_CASES")?)?;
    let cases: Vec<Value> = serde_json::from_slice(&expectation_bytes)?;
    ensure!(!cases.is_empty(), "Required marker corpus is empty");
    let output = parhelion_import::tiger::reader::outside(
        &configured("PARHELION_MARATHON_MARKER_OUTPUT")?,
        packages.parent().context("Source package parent")?,
    )?;
    let output = parhelion_import::tiger::reader::outside(
        &output,
        native.parent().context("Native package parent")?,
    )?;
    ensure!(!output.exists(), "Use a fresh marker artifact directory");
    fs::create_dir_all(&output)?;
    let listing = marathon::browse::browse(&packages, &output.join("listing"), |_, _| {})?;
    let catalog = sundial::investment::InvestmentCatalog::load_with_cache_path(
        native.parent().context("Native root")?,
        &output.join("catalog.json"),
        true,
        |_| {},
    )
    .map_err(|error| anyhow::anyhow!(error))?;
    let mut donors = catalog.weapon_donors();
    donors.sort_by_key(|donor| {
        (
            !donor.collection_backed,
            donor.name.to_lowercase(),
            donor.hash,
        )
    });
    let mut receipts = Vec::new();
    let mut moving = 0;
    let mut extended = 0;
    for case in cases {
        let source_item = u32::try_from(
            case["source_item"]
                .as_u64()
                .context("Source item identity")?,
        )?;
        let entry = listing
            .entries
            .iter()
            .find(|entry| entry.item == source_item)
            .context("Required source item is not listed")?;
        ensure!(
            u64::from(entry.entity) == case["entity"].as_u64().context("Expected source entity")?,
            "Source item resolved to a different entity"
        );
        let kind = entry.weapon_type().context("Source weapon type")?;
        let candidates = donors
            .iter()
            .filter(|donor| donor.type_name == kind)
            .map(|donor| (donor.hash, donor.name.clone()))
            .collect::<Vec<_>>();
        let recipe = marathon::browse::import(
            entry,
            &candidates,
            marathon::browse::Options {
                source_animation: true,
                ornaments: false,
            },
            &packages,
            &native,
            &output.join(format!("import-{source_item:08X}")),
            &mut |message| eprintln!("{}: {message}", entry.name),
        )?;
        let directory = recipe
            .parent()
            .context("Prepared recipe parent")?
            .join("graph");
        let graph: Value = serde_json::from_slice(&fs::read(directory.join("asset-graph.json"))?)?;
        let animation = &graph["animation"]["first_person"];
        ensure!(
            animation["controller"] == "marathon_source",
            "Source controller was not compiled"
        );
        let rigs = animation["rigs"].as_array().context("Emitted rigs")?;
        let read = |kind| -> Result<Payload> {
            let rows = rigs
                .iter()
                .filter(|r| r["kind"] == kind && r["first_person"] == false)
                .collect::<Vec<_>>();
            let [row] = rows.as_slice() else {
                anyhow::bail!("Ambiguous emitted {kind}");
            };
            let file = row["file"].as_str().context("Emitted rig file")?;
            Ok(Payload(fs::read(directory.join(file))?))
        };
        let skeleton = read("skeleton")?;
        let markers = read("markers")?;
        let definition = skeleton.pointer(24)?;
        let hierarchy = skeleton.array(definition + 0x80, 16, Some(0x80808A08))?;
        let binds = skeleton.array(definition + 0x90, 32, Some(0x80809F75))?;
        ensure!(
            !binds.is_empty() && binds.len() == hierarchy.len(),
            "Equipment bind domain differs"
        );
        let root = pose(&skeleton, binds[0])?.inverse();
        let rows = markers.array(markers.pointer(24)? + 0xB0, 64, Some(0x80808513))?;
        let expected = case["markers"]
            .as_array()
            .context("Independent source marker expectations")?;
        ensure!(!expected.is_empty(), "Expected marker set is empty");
        let mut checked = Vec::new();
        for entry in expected {
            let name = u32::try_from(entry["name"].as_u64().context("Marker name")?)?;
            let occurrence =
                usize::try_from(entry["occurrence"].as_u64().context("Marker occurrence")?)?;
            let selected = rows
                .iter()
                .filter_map(|&at| (markers.u32(at + 0x30).ok() == Some(name)).then_some(at))
                .nth(occurrence)
                .context("Expected marker was not emitted")?;
            ensure!(
                markers.u32(selected)? == 0,
                "Equipment marker body group changed"
            );
            let bone = usize::try_from(markers.u32(selected + 4)?)?;
            let bone_row = *hierarchy
                .get(bone)
                .context("Marker bone escapes emitted equipment rig")?;
            ensure!(
                u64::from(skeleton.u32(bone_row)?)
                    == entry["bone_name"]
                        .as_u64()
                        .context("Expected source bone")?,
                "Marker was flattened or bound to a different equipment bone"
            );
            let actual = root
                .then(pose(&skeleton, binds[bone])?)
                .then(pose(&markers, selected + 0x10)?)
                .matrix();
            let expected: [f32; 12] = serde_json::from_value(entry["root_bind_matrix"].clone())?;
            ensure!(
                expected.iter().all(|x| x.is_finite()),
                "Expected marker matrix is not finite"
            );
            let error = actual
                .iter()
                .zip(expected)
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            ensure!(
                error < 0.0001,
                "Source marker bind target differs by {error}"
            );
            moving += usize::from(bone != 0);
            extended += usize::from(bone > 2);
            checked.push(json!({"name":name,"occurrence":occurrence,"bone":bone,"matrix":actual,"max_error":error}));
        }
        let selectors = super::selection::verify(&directory, animation, &case["selection_probes"])?;
        receipts.push(
            json!({"source_item":source_item,"name":entry.name,"recipe":recipe,
            "recipe_sha256":hex::encode(Sha256::digest(fs::read(&recipe)?)),
            "markers":checked,"selectors":selectors,
            "skeleton_sha256":hex::encode(Sha256::digest(&skeleton.0)),
            "markers_sha256":hex::encode(Sha256::digest(&markers.0))}),
        );
    }
    ensure!(
        moving > 0 && extended > 0,
        "Configured corpus does not exercise moving equipment markers"
    );
    ensure!(
        package_stamp(&packages)? == source_stamp && package_stamp(&native)? == native_stamp,
        "Mounted inputs changed"
    );
    fs::write(
        output.join("verified-equipment-markers.json"),
        serde_json::to_vec_pretty(&json!({
            "revision":revision,"test_binary_sha256":test_binary_sha256,
            "converter_revision":parhelion_import::CONVERTER_REVISION,
            "source_stamp":source_stamp,"native_stamp":native_stamp,
            "cases":receipts,"nonroot_markers":moving,"beyond_root_range":extended,
            "expectations_sha256":hex::encode(Sha256::digest(expectation_bytes)),
            "limits":"Persisted marker identities and bind targets through the complete import. Action timing, current equipment pose transfer and live hand contact are unverified."
        }))?,
    )?;
    Ok(())
}
