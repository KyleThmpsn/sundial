//! A persisted ability copy must replay at its new place after an earlier subclass changes, and
//! compile again once its own changes do. Run child processes to isolate the environment-selected
//! cache.
use super::*;
use crate::subclass::{PaletteEdit, Place, SubclassAbilities, layout};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{io::Read, process::Command, time::Instant};
use sundial::package_authoring::ability_palette::ability_palettes;

const TEST: &str = "item::tests::staged_builds::ability_rebuild::ability_copies_replay_after_an_earlier_subclass_changes";
const PHASE: &str = "SUNDIAL_ABILITY_REBUILD_CHILD_PHASE";
/// Both subclasses start from this base, so they compile in their recipes' order.
const BASE: &str = "Voidwalker";
/// The earlier subclass recolors its Super, which puts private tags before the later one's copy.
const EARLIER: (&str, u8) = ("Ability Rebuild A", layout::SUPER);
/// The later subclass recolors its first grenade, whose copy the cache keeps.
const LATER: (&str, u8) = ("Ability Rebuild Z", layout::GRENADES[0]);

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and a fresh SUNDIAL_TEST_ARTIFACTS directory"]
fn ability_copies_replay_after_an_earlier_subclass_changes() {
    let output = crate::test_support::artifact_dir("ability-rebuild");
    if let Ok(phase) = std::env::var(PHASE) {
        child(&output, &phase);
        return;
    }
    assert!(!output.exists(), "Choose a fresh artifact directory");
    fs::create_dir_all(&output).unwrap();
    // Each recipe turns the hue of the first palette its ability's effects draw with.
    let packages = crate::test_support::stock_packages();
    let (view, source) = stock_view(&packages, ".parhelion-ability-rebuild-", Oodle::Required);
    fs::write(view.path().join("destiny2.exe"), []).unwrap();
    let catalog = crate::test_support::catalog(view.path()).unwrap();
    let base = catalog
        .subclasses(crate::package_profile::is_stock_item_definition)
        .into_iter()
        .find(|subclass| subclass.name == BASE)
        .unwrap_or_else(|| panic!("no stock {BASE}"));
    let manager = open_manager(&source).unwrap();
    let recipe = |(name, entry): (&str, u8), hue: i16| {
        let entity = base.entry_entities[&entry];
        let palette = ability_palettes(&manager, entity, crate::subclass::SPAWN_DEPTH)
            .unwrap()
            .first()
            .unwrap_or_else(|| panic!("{BASE} entry {entry} draws with no palette"))
            .header;
        let mut recipe = crate::WeaponRecipe::new_unbound_kind(crate::ItemKind::Subclass).unwrap();
        recipe.set_donor(base.hash, base.name.clone());
        recipe.rename_authored_item(name).unwrap();
        let mut abilities = SubclassAbilities::default();
        let mut edits = abilities.edits(base.hash, Place::Ability(entry));
        edits.set_palette(PaletteEdit {
            hue,
            ..PaletteEdit::new(palette)
        });
        abilities.set_edits(base.hash, Place::Ability(entry), edits);
        recipe.overrides.subclass_abilities = Some(abilities);
        recipe
    };
    for (file, item, hue) in [
        ("earlier", EARLIER, 45),
        ("later", LATER, 90),
        ("later-changed", LATER, -120),
    ] {
        recipe(item, hue)
            .save_json(output.join(format!("{file}.parhelion.json")))
            .unwrap();
    }
    drop((manager, view));
    let executable = std::env::current_exe().unwrap();
    for phase in ["seed", "relocated", "fresh", "changed"] {
        let mut command = Command::new(&executable);
        command
            .args(["--exact", TEST, "--ignored", "--nocapture"])
            .env(PHASE, phase)
            .env("PARHELION_RUNTIME_CACHE_DIRECTORY", output.join("cache"));
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
    let (seed, relocated, fresh, changed) = (
        receipt("seed"),
        receipt("relocated"),
        receipt("fresh"),
        receipt("changed"),
    );
    assert_ne!(
        seed["later_copy"], relocated["later_copy"],
        "The earlier subclass must move the cached copy"
    );
    assert_eq!(
        relocated["packages"], fresh["packages"],
        "A replayed copy must build every package byte a fresh compile does"
    );
    let revision = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    fs::write(
        output.join("verification.json"),
        serde_json::to_vec_pretty(&json!({
            "native_build":"86657.20.08.23.1800.d2_rc",
            "revision":String::from_utf8_lossy(&revision.stdout).trim(),
            "executable_sha256":hex::encode(Sha256::digest(fs::read(executable).unwrap())),
            "recipe_files":["earlier.parhelion.json","later.parhelion.json","later-changed.parhelion.json"],
            "source_packages":packages,
            "seed":seed,"relocated":relocated,"fresh":fresh,"changed":changed,
            "repeat_command":format!("cargo test --release -p parhelion {TEST} -- --ignored --exact --nocapture"),
            "limits":"Package readback only. No gameplay or compile timing claim."
        }))
        .unwrap(),
    )
    .unwrap();
}

fn child(output: &Path, phase: &str) {
    let packages = crate::test_support::stock_packages();
    let load = |file: &str| {
        crate::WeaponRecipe::load_json(output.join(format!("{file}.parhelion.json")))
            .unwrap()
            .to_spec()
            .unwrap()
    };
    let mut items = Vec::new();
    if phase != "seed" {
        items.push(load("earlier"));
    }
    items.push(load(if phase == "changed" {
        "later-changed"
    } else {
        "later"
    }));
    let (view, source) = stock_view(&packages, ".parhelion-ability-rebuild-", Oodle::Required);
    fs::write(view.path().join("destiny2.exe"), []).unwrap();
    validate_weapon_clone_specs_against_catalog(view.path(), items.iter()).unwrap();
    let mut progress = Vec::new();
    let start = Instant::now();
    let bundle = crate::item::build::compile_with_progress(
        &source,
        &WeaponProjectSpec { weapons: items },
        crate::branding::Branding::for_packages(&source),
        &mut |event| {
            progress.push(json!({
                "seconds":event.timestamp.saturating_duration_since(start).as_secs_f64(),
                "label":event.label,"activity":format!("{:?}",event.activity)
            }))
        },
    )
    .unwrap();
    let seconds = start.elapsed().as_secs_f64();
    // An ability is named with its subclass, and the subclass item's own runtime without one.
    let reused = |(item, _): (&str, u8)| {
        progress.iter().any(|event| {
            event["label"].as_str().is_some_and(|label| {
                label.starts_with("Reusing Runtime for ") && label.ends_with(&format!(" · {item}"))
            })
        })
    };
    let expected = match phase {
        "relocated" => (false, true),
        "changed" => (true, false),
        _ => (false, false),
    };
    assert_eq!(
        (reused(EARLIER), reused(LATER)),
        expected,
        "{phase}: reuse must follow the saved copies and their inputs"
    );
    // Each subclass's entry names its copy, read back the way the catalog reads any subclass.
    let staged = staged_view(&source, ".parhelion-ability-rebuild-readback-", &bundle);
    fs::write(staged.path().join("destiny2.exe"), []).unwrap();
    let catalog = crate::test_support::catalog(staged.path()).unwrap();
    let subclasses = catalog.subclasses(|_| true);
    let copy = |(item, entry): (&str, u8)| {
        subclasses
            .iter()
            .find(|subclass| subclass.name == item)
            .map(|subclass| subclass.entry_entities[&entry])
    };
    let later = copy(LATER).expect("the later subclass reads back");
    let stock = open_manager(&source).unwrap();
    assert!(
        stock.get_entry(TagHash(later)).is_none(),
        "The later grenade must name a private copy"
    );
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
    let directory = output.join(phase);
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("receipt.json"),
        serde_json::to_vec_pretty(&json!({
            "build_seconds":seconds,"process_id":std::process::id(),"packages":packages,
            "later_copy":format!("0x{later:08X}"),
            "earlier_copy":copy(EARLIER).map(|tag| format!("0x{tag:08X}")),
            "fragments":fragments(),"progress":progress,
        }))
        .unwrap(),
    )
    .unwrap();
}

/// Each saved ability copy in the cache: its file, whether it can move, the copy it returned and
/// how many native payloads it read.
fn fragments() -> Vec<serde_json::Value> {
    let Some(root) = std::env::var_os("PARHELION_RUNTIME_CACHE_DIRECTORY")
        .filter(|_| std::env::var_os("PARHELION_DISABLE_RUNTIME_CACHE").is_none())
    else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("runtime") {
            continue;
        }
        let mut archive = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        let mut bytes = Vec::new();
        archive
            .by_name("entry.json")
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        if saved["result"].is_null() {
            continue;
        }
        found.push(json!({
            "file":path.file_name().unwrap().to_string_lossy(),
            "relocatable":saved["relocatable"],"result":saved["result"],
            "native_reads":saved["reads"].as_object().map_or(0, serde_json::Map::len),
        }));
    }
    found
}
