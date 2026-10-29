//! Configured Marathon catalog import: every listed weapon converts onto the bases the importer
//! offers it, with its donor's animations and all of its camos, and its recipe loads as the
//! library saves it. Weapons that share a camo on one donor convert it once between them, even
//! while their imports run at the same time.
#![cfg(feature = "d2-model-importer")]

use anyhow::{Context, Result, anyhow, ensure};
use parhelion::WeaponRecipe;
use parhelion_import::marathon::browse;
use serde_json::{Value, json};
use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Instant,
};
use sundial::investment::InvestmentCatalog;

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

/// The importer's worker count, unless `PARHELION_MARATHON_WORKERS` sets one.
fn workers() -> Result<usize> {
    Ok(match env::var("PARHELION_MARATHON_WORKERS") {
        Ok(value) => value.parse()?,
        Err(_) => thread::available_parallelism().map_or(1, |cores| (cores.get() / 4).clamp(1, 3)),
    })
}

/// One weapon's conversion and recipe load, with its donor choice and camo report.
fn import(
    entry: &browse::Entry,
    donors: &[(u32, String)],
    (packages, native): (&Path, &Path),
    directory: &Path,
) -> Result<Value> {
    let path = browse::import(
        entry,
        donors,
        browse::Options {
            source_animation: false,
            ornaments: true,
        },
        packages,
        native,
        directory,
        &mut |_| {},
    )?;
    let recipe = WeaponRecipe::load_json(&path).map_err(|error| anyhow!("{error}"))?;
    let read = |name: &str| -> Result<Value> {
        Ok(serde_json::from_slice(&fs::read(
            path.with_file_name(name),
        )?)?)
    };
    let donor = read("donor-choice.json")?;
    Ok(
        json!({"recipe":path,"name":recipe.name,"donor":donor["donor"],
        "donor_name":donor["donor_name"],"chosen_by":donor["chosen_by"],
        "camos":read("ornaments.json")?}),
    )
}

#[test]
#[ignore = "Requires PARHELION_MARATHON_PACKAGES, SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn every_marathon_weapon_imports_with_donor_animations() -> Result<()> {
    let packages = configured("PARHELION_MARATHON_PACKAGES")?;
    let native = configured("SUNDIAL_STOCK_PACKAGES")?;
    let output = configured("SUNDIAL_TEST_ARTIFACTS")?.join("marathon-catalog");
    ensure!(!output.exists(), "Use a fresh catalog artifact directory");
    fs::create_dir_all(&output)?;
    // The run's own store, as the app opens one in its models folder.
    parhelion_import::store::open(&output);
    let started = Instant::now();
    let stamp = parhelion_import::tiger::reader::package_stamp(&packages)?;
    let listing = browse::browse_cached(&packages, &output.join("listing"), true, |_, _| {})?;
    let listed = started.elapsed().as_secs_f64();
    // The importer's Weapons tab: rows whose type is a native weapon type. A comma-separated
    // `PARHELION_MARATHON_WEAPONS` keeps only the named source weapons.
    let only = env::var("PARHELION_MARATHON_WEAPONS").ok();
    let weapons = listing
        .entries
        .iter()
        .filter(|entry| entry.weapon_type().is_some())
        .filter(|entry| {
            only.as_deref()
                .is_none_or(|names| names.split(',').any(|name| name.trim() == entry.name))
        })
        .collect::<Vec<_>>();
    ensure!(!weapons.is_empty(), "The Marathon listing has no weapons");
    let catalog = InvestmentCatalog::load_with_cache_path(
        native.parent().context("Native root")?,
        &output.join("catalog-cache.json"),
        true,
        |_| {},
    )
    .map_err(|error| anyhow!(error))?;
    // The importer's bases: collection-backed items first, then by name, without items whose
    // collectible depends on an unrelated condition. Clean stock packages hold no authored items.
    let risky = catalog.items_with_unrelated_collection_conditions();
    let mut bases = catalog.weapon_donors();
    bases.retain(|donor| !risky.contains(&donor.hash));
    bases.sort_by_key(|donor| {
        (
            !donor.collection_backed,
            donor.name.to_lowercase(),
            donor.hash,
        )
    });
    let workers = workers()?.clamp(1, weapons.len());
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(entry) = weapons.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let index = weapons.iter().position(|e| e.item == entry.item);
                    let kind = entry.weapon_type().unwrap_or_default();
                    let donors = bases
                        .iter()
                        .filter(|donor| donor.type_name == kind)
                        .map(|donor| (donor.hash, donor.name.clone()))
                        .collect::<Vec<_>>();
                    let begun = Instant::now();
                    let directory = output.join(format!("import-{:08X}", entry.item));
                    let outcome = import(entry, &donors, (&packages, &native), &directory);
                    let mut row = json!({"index":index,"source":entry.name,
                        "item":format!("{:08X}",entry.item),"type":kind,"candidates":donors.len(),
                        "seconds":begun.elapsed().as_secs_f64()});
                    match outcome {
                        Ok(report) => row["import"] = report,
                        Err(error) => row["error"] = json!(format!("{error:#}")),
                    }
                    eprintln!(
                        "{}: {}",
                        entry.name,
                        row["error"].as_str().unwrap_or("imported")
                    );
                    results.lock().expect("Result list").push(row);
                }
            });
        }
    });
    let mut results = results.into_inner().expect("Result list");
    results.sort_by_key(|row| row["index"].as_u64());
    let failed = results
        .iter()
        .filter_map(|row| {
            row["error"]
                .as_str()
                .map(|error| format!("{}: {error}", row["source"]))
        })
        .collect::<Vec<_>>();
    let omitted = results
        .iter()
        .flat_map(|row| {
            row["import"]["camos"]["omitted"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|camo| format!("{} {}: {}", row["source"], camo["name"], camo["reason"]))
        })
        .collect::<Vec<_>>();
    // Each camo conversion is made once. Every other weapon that offers it copies it.
    let mut converted = std::collections::BTreeMap::<String, Vec<String>>::new();
    let mut reused = 0;
    for row in &results {
        for camo in row["import"]["camos"]["conversions"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if camo["reused"] == true {
                reused += 1;
            } else {
                converted
                    .entry(camo["conversion"].as_str().unwrap_or_default().to_owned())
                    .or_default()
                    .push(format!("{} {}", row["source"], camo["name"]));
            }
        }
    }
    let repeated = converted
        .values()
        .filter(|camos| camos.len() > 1)
        .map(|camos| camos.join(", "))
        .collect::<Vec<_>>();
    fs::write(
        output.join("marathon-catalog.json"),
        serde_json::to_vec_pretty(&json!({
            "packages_stamp": stamp,
            "converter_revision": parhelion_import::CONVERTER_REVISION,
            "source_animation": false,
            "workers": workers,
            "listing_seconds": listed,
            "total_seconds": started.elapsed().as_secs_f64(),
            "weapons": weapons.len(),
            "imported": weapons.len() - failed.len(),
            "failed": failed,
            "omitted_camos": omitted,
            "camo_conversions": converted.len(),
            "camos_copied": reused,
            "camos_converted_again": repeated,
            "results": results,
        }))?,
    )?;
    ensure!(
        failed.is_empty() && omitted.is_empty(),
        "{} of {} weapons failed and {} camos were left out:\n{}\n{}",
        failed.len(),
        weapons.len(),
        omitted.len(),
        failed.join("\n"),
        omitted.join("\n")
    );
    ensure!(
        repeated.is_empty(),
        "Camos converted more than once:\n{}",
        repeated.join("\n")
    );
    Ok(())
}
