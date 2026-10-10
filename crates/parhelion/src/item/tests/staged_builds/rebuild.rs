//! A persisted runtime fragment must survive an earlier recipe's allocation change.
//! Run child processes to isolate environment-selected caches and package worker counts.
use super::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{io::Read, process::Command, time::Instant};
use sundial::package_authoring::runtime::{
    load_weapon_runtime_entity_at_pattern_index_with_manager,
    load_weapon_runtime_entity_with_manager,
};

const TEST: &str =
    "item::tests::staged_builds::rebuild::runtime_reuse_relocates_after_an_earlier_recipe_changes";
const PHASE: &str = "SUNDIAL_REBUILD_CHILD_PHASE";

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and a fresh SUNDIAL_TEST_ARTIFACTS directory"]
fn runtime_reuse_relocates_after_an_earlier_recipe_changes() {
    let output = crate::test_support::artifact_dir("runtime-rebuild");
    if let Ok(phase) = std::env::var(PHASE) {
        child(&output, &phase);
        return;
    }
    assert!(!output.exists(), "Choose a fresh artifact directory");
    fs::create_dir_all(&output).unwrap();
    let mut later =
        crate::WeaponRecipe::new_weapon_for_donor("parhelion.rebuild.z", 0x90D4_2801, "Austringer")
            .unwrap();
    later.name = "Later Runtime".into();
    later.overrides.animation_donor = Some(crate::recipe::WeaponDonorReference {
        item_hash: 0x02E6_3C72.into(),
        expected_name: Some("Ancient Gospel".into()),
    });
    later
        .save_json(output.join("later.parhelion.json"))
        .unwrap();
    let mut earlier =
        crate::WeaponRecipe::new_weapon_for_donor("parhelion.rebuild.a", 0x90D4_2801, "Austringer")
            .unwrap();
    earlier.name = "Earlier Runtime".into();
    earlier.overrides.animation_donor = Some(crate::recipe::WeaponDonorReference {
        item_hash: 0x02E6_3C72.into(),
        expected_name: Some("Ancient Gospel".into()),
    });
    earlier
        .save_json(output.join("earlier.parhelion.json"))
        .unwrap();
    let executable = std::env::current_exe().unwrap();
    for phase in [
        "seed",
        "relocated",
        "fresh",
        "corrupt",
        "cold-1",
        "warm-1",
        "cold-2",
        "warm-2",
        "cold-4",
        "warm-4",
    ] {
        if phase == "corrupt" {
            let fragment = output.join("cache").join(format!(
                "{}.runtime",
                hex::encode(Sha256::digest(later.namespace.as_bytes()))
            ));
            assert!(fragment.is_file(), "The seed must have produced a fragment");
            fs::copy(&fragment, output.join("later-before-corruption.runtime")).unwrap();
            fs::write(fragment, b"interrupted cache write").unwrap();
        }
        let mut command = Command::new(&executable);
        command
            .args(["--exact", TEST, "--ignored", "--nocapture"])
            .env(PHASE, phase)
            .env(
                "PARHELION_RUNTIME_CACHE_DIRECTORY",
                output.join(phase.split_once('-').map_or_else(
                    || "cache".to_owned(),
                    |(_, workers)| format!("cache-{workers}"),
                )),
            )
            .env(
                "PARHELION_PACKAGE_WORKERS",
                phase
                    .split_once('-')
                    .map_or(if phase == "fresh" { "1" } else { "4" }, |(_, workers)| {
                        workers
                    }),
            );
        if phase == "fresh" {
            command.env("PARHELION_DISABLE_RUNTIME_CACHE", "1");
        } else {
            command.env_remove("PARHELION_DISABLE_RUNTIME_CACHE");
        }
        let result = command.output().unwrap();
        fs::write(output.join(format!("{phase}.stdout")), &result.stdout).unwrap();
        fs::write(output.join(format!("{phase}.stderr")), &result.stderr).unwrap();
        assert!(
            result.status.success(),
            "{phase}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let receipt = |phase: &str| -> serde_json::Value {
        serde_json::from_slice(&fs::read(output.join(phase).join("receipt.json")).unwrap()).unwrap()
    };
    let seed = receipt("seed");
    let relocated = receipt("relocated");
    let fresh = receipt("fresh");
    let corrupt = receipt("corrupt");
    let comparisons = ["cold-1", "warm-1", "cold-2", "warm-2", "cold-4", "warm-4"]
        .into_iter()
        .map(|phase| {
            let result = receipt(phase);
            assert_eq!(
                result["packages"], fresh["packages"],
                "{phase} must preserve every package byte"
            );
            (phase, result)
        })
        .collect::<BTreeMap<_, _>>();
    assert_ne!(
        seed["later_entity"], relocated["later_entity"],
        "The fixture must move the cached entity"
    );
    assert_eq!(relocated["packages"], fresh["packages"]);
    assert_eq!(corrupt["packages"], fresh["packages"]);
    let revision = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    fs::write(output.join("verification.json"), serde_json::to_vec_pretty(&json!({
        "native_build":"86657.20.08.23.1800.d2_rc",
        "revision":String::from_utf8_lossy(&revision.stdout).trim(),
        "executable_sha256":hex::encode(Sha256::digest(fs::read(executable).unwrap())),
        "recipe_files":["earlier.parhelion.json","later.parhelion.json"],
        "source_packages":crate::test_support::stock_packages(),
        "seed":seed,"relocated":relocated,"fresh":fresh,"corrupt":corrupt,"worker_comparisons":comparisons,
        "repeat_command":format!("cargo test --release -p parhelion {TEST} -- --ignored --exact --nocapture"),
        "limits":"Package readback only. No gameplay or compile/link timing claim."
    })).unwrap()).unwrap();
}

fn child(output: &Path, phase: &str) {
    let packages = crate::test_support::stock_packages();
    let later = crate::WeaponRecipe::load_json(output.join("later.parhelion.json"))
        .unwrap()
        .to_spec()
        .unwrap();
    let mut weapons = Vec::new();
    if phase != "seed" {
        weapons.push(
            crate::WeaponRecipe::load_json(output.join("earlier.parhelion.json"))
                .unwrap()
                .to_spec()
                .unwrap(),
        );
    }
    weapons.push(later.clone());
    let (view, source) = stock_view(&packages, ".parhelion-runtime-rebuild-", Oodle::Required);
    fs::write(view.path().join("destiny2.exe"), []).unwrap();
    validate_weapon_clone_specs_against_catalog(view.path(), weapons.iter()).unwrap();
    let mut progress = Vec::new();
    let start = Instant::now();
    let bundle = crate::item::build::compile_with_progress(
        &source,
        &WeaponProjectSpec { weapons },
        crate::branding::Branding::for_packages(&source),
        &mut |event| progress.push(json!({
            "seconds":event.timestamp.saturating_duration_since(start).as_secs_f64(),
            "label":event.label,"completed":event.completed,"total":event.total,"activity":format!("{:?}",event.activity)
        })),
    ).unwrap();
    let seconds = start.elapsed().as_secs_f64();
    let hit = progress
        .iter()
        .any(|event| event["label"] == "Reusing Runtime for Later Runtime");
    assert_eq!(
        hit,
        phase == "relocated" || phase.starts_with("warm-"),
        "Cache use must follow the persisted inputs and corruption"
    );
    let staged = staged_view(&source, ".parhelion-rebuild-readback-", &bundle);
    let manager = open_manager(&staged.path().join("packages")).unwrap();
    let authored =
        load_weapon_runtime_entity_with_manager(&manager, later.identity.item_hash).unwrap();
    let profile = crate::weapon::animations::profile(
        &manager,
        &authored.payload,
        Some(authored.weapon_content_group_hash),
    )
    .unwrap();
    let stock = open_manager(&source).unwrap();
    let catalog = crate::test_support::catalog(source.parent().unwrap()).unwrap();
    let pattern = catalog
        .weapon_donors()
        .into_iter()
        .find(|donor| donor.hash == 0x02E6_3C72)
        .and_then(|donor| donor.weapon_pattern_index)
        .expect("The donor must have a native pattern row");
    let donor = load_weapon_runtime_entity_at_pattern_index_with_manager(&stock, pattern).unwrap();
    let donor_profile = crate::weapon::animations::profile(
        &stock,
        &donor.payload,
        Some(donor.weapon_content_group_hash),
    )
    .unwrap();
    assert_eq!(
        profile.keys, donor_profile.keys,
        "The selected runtime must still play its animation donor"
    );
    assert!(
        stock.get_entry(TagHash(profile.owner)).is_none(),
        "The selected owner must be private"
    );
    assert_eq!(
        stock.read_tag(TagHash(donor.entity_tag)).unwrap(),
        manager.read_tag(TagHash(donor.entity_tag)).unwrap()
    );
    let directory = output.join(phase);
    fs::create_dir_all(directory.join("packages")).unwrap();
    bundle.write_new(&directory.join("packages")).unwrap();
    if phase != "fresh" {
        let path = PathBuf::from(std::env::var_os("PARHELION_RUNTIME_CACHE_DIRECTORY").unwrap())
            .join(format!(
                "{}.runtime",
                hex::encode(Sha256::digest(later.namespace.as_bytes()))
            ));
        let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut entry = Vec::new();
        archive
            .by_name("entry.json")
            .unwrap()
            .read_to_end(&mut entry)
            .unwrap();
        let entry: serde_json::Value = serde_json::from_slice(&entry).unwrap();
        fs::write(
            directory.join("source-inputs.json"),
            serde_json::to_vec_pretty(&json!({
                "cache_key":entry["key"],"native_payload_sha256":entry["reads"],
                "import_input_sha256":entry.get("inputs"),
            }))
            .unwrap(),
        )
        .unwrap();
    }
    let packages = bundle
        .artifacts
        .iter()
        .map(|artifact| {
            (
                artifact.plan.output_file_name.clone(),
                hex::encode(Sha256::digest(artifact.bytes())),
            )
        })
        .collect::<BTreeMap<_, _>>();
    fs::write(
        directory.join("receipt.json"),
        serde_json::to_vec_pretty(&json!({
            "build_seconds":seconds,"process_id":std::process::id(),"packages":packages,"progress":progress,
            "later_entity":authored.entity_tag,"private_owner":profile.owner,
            "animation_keys":profile.keys,"stock_donor_unchanged":true
        }))
        .unwrap(),
    )
    .unwrap();
}
