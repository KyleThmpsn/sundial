//! Builds one weapon of every rig family wearing the appearance of every other family, and
//! writes each outcome to a JSON report so a sweep can be compared against the next.
use super::*;
use std::sync::Mutex;

type Slot = sundial::investment::WeaponInventorySlot;
type Weapon = (u32, String, String, Option<Slot>);

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES and PARHELION_APPEARANCE_SWEEP_REPORT"]
fn real_every_family_builds_with_every_other_familys_appearance() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let report = PathBuf::from(std::env::var_os("PARHELION_APPEARANCE_SWEEP_REPORT").unwrap());
    let threads = std::env::var("PARHELION_APPEARANCE_SWEEP_THREADS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(4usize);
    let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
    let mut candidates = BTreeMap::<u32, Vec<Weapon>>::new();
    let mut donors = catalog.weapon_donors();
    donors.sort_by_key(|donor| (!donor.collection_backed, donor.hash));
    for donor in donors {
        let Some(group) = donor.weapon_translation_group else {
            continue;
        };
        if donor.name.is_empty() || matches!(group, 0 | 0x811C_9DC5) {
            continue;
        }
        candidates.entry(group).or_default().push((
            donor.hash,
            donor.name,
            donor.type_name,
            donor.inventory_slot,
        ));
    }
    // Each family is represented by its first weapon that builds unchanged, so a failure in the
    // sweep belongs to the appearance rather than to the gameplay weapon's own sockets.
    let groups = candidates.keys().copied().collect::<Vec<_>>();
    let chosen = parallel(threads, &groups, |_, group| {
        candidates[group]
            .iter()
            .take(12)
            .find(|weapon| build(&packages, vec![spec(weapon, None)]).is_ok())
            .cloned()
    });
    let mut families = Vec::new();
    let mut unbuildable = Vec::new();
    for (group, weapon) in groups.iter().zip(chosen) {
        match weapon {
            Some(weapon) => families.push((*group, weapon)),
            None => unbuildable.push(format!("0x{group:08X}")),
        }
    }
    let pairs = families
        .iter()
        .flat_map(|gameplay| {
            families
                .iter()
                .filter(move |appearance| appearance.0 != gameplay.0)
                .map(move |appearance| (gameplay, appearance))
        })
        .collect::<Vec<_>>();
    eprintln!(
        "{} families, {} without a buildable weapon, {} pairs",
        families.len(),
        unbuildable.len(),
        pairs.len()
    );
    // One project per gameplay weapon carries every appearance at once. Only a project that
    // fails is rebuilt one pair at a time, so each failure is attributed to its own pair.
    let batches = parallel(threads, &families, |index, gameplay| {
        let batch = pairs
            .iter()
            .filter(|(owner, _)| owner.0 == gameplay.0)
            .collect::<Vec<_>>();
        let specs = batch
            .iter()
            .map(|(gameplay, appearance)| spec(&gameplay.1, Some(&appearance.1)))
            .collect::<Vec<_>>();
        let outcomes = if build(&packages, specs).is_ok() {
            vec![None; batch.len()]
        } else {
            batch
                .iter()
                .map(|(gameplay, appearance)| {
                    build(&packages, vec![spec(&gameplay.1, Some(&appearance.1))]).err()
                })
                .collect()
        };
        eprintln!(
            "{}/{} {}: {} of {} failed",
            index + 1,
            families.len(),
            gameplay.1.1,
            outcomes.iter().flatten().count(),
            batch.len()
        );
        batch.into_iter().zip(outcomes).collect::<Vec<_>>()
    });
    let (pairs, outcomes): (Vec<_>, Vec<_>) = batches.into_iter().flatten().unzip();
    let describe = |(group, (hash, name, kind, slot)): &(u32, Weapon)| {
        serde_json::json!({
            "group": format!("0x{group:08X}"), "hash": format!("0x{hash:08X}"),
            "name": name, "type": kind, "slot": format!("{slot:?}"),
        })
    };
    let results = pairs
        .iter()
        .zip(&outcomes)
        .map(|(&(gameplay, appearance), error)| {
            serde_json::json!({
                "gameplay": describe(gameplay),
                "appearance": describe(appearance),
                "error": error,
            })
        })
        .collect::<Vec<_>>();
    let failures = outcomes.iter().filter(|error| error.is_some()).count();
    fs::write(
        &report,
        serde_json::to_vec_pretty(&serde_json::json!({
            "families": families.iter().map(describe).collect::<Vec<_>>(),
            "unbuildable_families": unbuildable,
            "pairs": results.len(),
            "failures": failures,
            "results": results,
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(failures, 0, "see {}", report.display());
}

fn spec(gameplay: &Weapon, appearance: Option<&Weapon>) -> WeaponCloneSpec {
    let namespace = format!(
        "parhelion.appearance-sweep.{:08x}.{:08x}",
        gameplay.0,
        appearance.map_or(0, |weapon| weapon.0)
    );
    // The picker allows a Power appearance on a primary weapon, and the reverse, only once the
    // authored weapon moves to the appearance's slot.
    let primary = |slot| matches!(slot, Some(Slot::Kinetic | Slot::Energy));
    let inventory_slot = appearance
        .filter(|weapon| weapon.3 != gameplay.3 && !(primary(weapon.3) && primary(gameplay.3)))
        .and_then(|weapon| weapon.3)
        .map(|slot| match slot {
            Slot::Kinetic => WeaponInventorySlot::Kinetic,
            Slot::Energy => WeaponInventorySlot::Energy,
            Slot::Power => WeaponInventorySlot::Power,
        });
    WeaponCloneSpec {
        kind: crate::ItemKind::Weapon,
        namespace: namespace.clone(),
        donor_item_hash: gameplay.0,
        expected_donor_name: Some(gameplay.1.clone()),
        presentation_donor: appearance.map(|weapon| WeaponPresentationDonorReference {
            item_hash: weapon.0,
            expected_name: Some(weapon.1.clone()),
        }),
        icon_donor: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        identity: WeaponCloneIdentity::from_namespace(&namespace).unwrap(),
        text: WeaponCloneText {
            name: format!("Sweep {:08X}", appearance.map_or(0, |weapon| weapon.0)),
            flavor: "Appearance sweep.".to_owned(),
            source: "Source: integration test".to_owned(),
            ..WeaponCloneText::default()
        },
        overrides: WeaponCloneOverrides {
            inventory_slot,
            ..WeaponCloneOverrides::default()
        },
    }
}

fn build(packages: &Path, weapons: Vec<WeaponCloneSpec>) -> Result<(), String> {
    build_weapon_project(packages, &WeaponProjectSpec { weapons })
        .map(|_| ())
        .map_err(|error| format!("{error:?}"))
}

/// Runs `work` over `items` on `threads` workers and returns the results in item order.
fn parallel<T: Sync, R: Send>(
    threads: usize,
    items: &[T],
    work: impl Fn(usize, &T) -> R + Sync,
) -> Vec<R> {
    let next = Mutex::new(0usize);
    let results = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = {
                        let mut next = next.lock().unwrap();
                        *next += 1;
                        *next - 1
                    };
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    let result = work(index, item);
                    results.lock().unwrap().push((index, result));
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}
